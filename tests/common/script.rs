//! A `Prompter` that answers from a script, for the tests that drive `add` or `pin` through a
//! terminal that is not there.

use std::collections::VecDeque;

use refs_cli::prompt::{Abort, Prompter};

/// One scripted answer.
pub enum Answer {
    Text(&'static str),
    Yes(bool),
    /// Selects these options in a `pick`; each must be one of those offered.
    Pick(&'static [&'static str]),
    /// Empties a prefilled box, as selecting it all and deleting would.
    Clear,
    Cancel,
    Fail(&'static str),
}

/// Answers the questions in order from a queue, and records them. An empty text answer is Enter on
/// the prefilled `initial` value; `Answer::Clear` is an empty box. A text answer that fails validation is recorded as a rejection and the next answer is
/// tried, as a terminal would re-ask.
///
/// Once the queue is empty it panics, unless the script `takes_defaults`: then every text takes
/// its initial value, every suggestion is left empty, and every yes-or-no gets one fixed answer.
/// `picking` answers the text questions that mention a word, whatever the queue holds.
#[derive(Default)]
pub struct Script {
    answers: VecDeque<Answer>,
    defaults: Option<bool>,
    pick: Option<(&'static str, Option<&'static str>)>,
    /// Every question with the initial value it came with.
    pub asked: Vec<(String, Option<String>)>,
    pub rejected: Vec<String>,
    pub suggested: Vec<Vec<String>>,
    /// The options of every `pick`.
    pub offered: Vec<Vec<String>>,
}

impl Script {
    pub fn new(answers: impl IntoIterator<Item = Answer>) -> Script {
        Script {
            answers: answers.into_iter().collect(),
            ..Script::default()
        }
    }

    /// Takes every default and gives `yes` to every yes-or-no question.
    pub fn takes_defaults(yes: bool) -> Script {
        Script {
            defaults: Some(yes),
            ..Script::default()
        }
    }

    /// Answers the text questions that mention `word` with `answer` (the initial value when `None`);
    /// an answer that fails validation cancels.
    pub fn picking(mut self, word: &'static str, answer: Option<&'static str>) -> Script {
        self.pick = Some((word, answer));
        self
    }

    /// The questions asked, without their defaults.
    pub fn questions(&self) -> Vec<&str> {
        self.asked.iter().map(|(message, _)| &**message).collect()
    }

    /// The questions that mention `word`, with their defaults.
    pub fn asked_about(&self, word: &str) -> Vec<&(String, Option<String>)> {
        let mentions = |(message, _): &&(String, Option<String>)| message.contains(word);
        self.asked.iter().filter(mentions).collect()
    }

    fn next(&mut self, message: &str, initial: Option<&str>) -> Option<Answer> {
        self.asked
            .push((message.to_owned(), initial.map(str::to_owned)));
        match self.answers.pop_front() {
            None if self.defaults.is_some() => None,
            None => panic!("no answer left for {message:?}"),
            answer => answer,
        }
    }
}

impl Prompter for Script {
    fn text(
        &mut self,
        message: &str,
        initial: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        if let Some((word, answer)) = self.pick
            && message.contains(word)
        {
            self.asked
                .push((message.to_owned(), initial.map(str::to_owned)));
            let answer = answer.or(initial).unwrap_or("").to_owned();
            return validate(&answer).map(|()| answer).map_err(|why| {
                self.rejected.push(why);
                Abort::Cancelled
            });
        }
        loop {
            let answer = match self.next(message, initial) {
                None | Some(Answer::Text("")) => initial.unwrap_or("").to_owned(),
                Some(Answer::Clear) => String::new(),
                Some(Answer::Text(text)) => text.to_owned(),
                Some(Answer::Cancel) => return Err(Abort::Cancelled),
                Some(Answer::Fail(why)) => return Err(Abort::Failed(why.to_owned())),
                Some(Answer::Yes(_) | Answer::Pick(_)) => panic!("{message:?} is a text question"),
            };
            match validate(&answer) {
                Ok(()) => return Ok(answer),
                Err(why) => self.rejected.push(why),
            }
        }
    }

    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        self.suggested.push(suggestions.to_vec());
        match self.defaults {
            Some(_) if self.answers.is_empty() => {
                self.asked.push((message.to_owned(), None));
                Ok(String::new())
            }
            _ => self.text(message, None, validate),
        }
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        match self.next(message, None) {
            None => Ok(self.defaults.unwrap_or(false)),
            Some(Answer::Yes(yes)) => Ok(yes),
            Some(Answer::Cancel) => Err(Abort::Cancelled),
            Some(Answer::Text(_) | Answer::Clear | Answer::Fail(_) | Answer::Pick(_)) => {
                panic!("{message:?} is a confirm question")
            }
        }
    }

    fn pick(&mut self, message: &str, options: &[String]) -> Result<Vec<String>, Abort> {
        self.offered.push(options.to_vec());
        match self.next(message, None) {
            None => Ok(vec![]),
            Some(Answer::Pick(chosen)) => {
                for one in chosen {
                    assert!(options.iter().any(|o| o == one), "{one:?} is not offered");
                }
                Ok(chosen.iter().map(|o| o.to_string()).collect())
            }
            Some(Answer::Cancel) => Err(Abort::Cancelled),
            Some(_) => panic!("{message:?} is a pick question"),
        }
    }
}
