//! A stateful in-memory `Source` for tests of everything above the seam.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::{Observed, Pin, Source};
use crate::config::RepoRef;
use crate::diagnostic::SourceError;
use crate::outline::Outline;

/// The `Source` methods a failure can be injected into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Resolve,
    Verify,
    Materialise,
    Remove,
    Inspect,
    /// Keyed by the remote's URL, not an id.
    Tags,
    /// Keyed by the remote's URL, not an id.
    Outline,
}

/// One call the fake received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Resolve(String),
    Verify { id: String, offline: bool },
    Materialise { id: String, offline: bool },
    Remove(String),
    Tags(String),
    Outline(String),
}

/// The commit and files `outline` and `read` serve for a URL.
struct Tree {
    sha: String,
    files: Vec<(String, String)>,
}

/// What a `FakeSource` and its clones share.
#[derive(Default)]
struct State {
    disk: RefCell<HashMap<String, Observed>>,
    failures: RefCell<HashMap<(String, Method), String>>,
    commits: RefCell<HashMap<String, String>>,
    lost_refs: RefCell<HashSet<(String, String)>>,
    tags: RefCell<HashMap<String, Vec<String>>>,
    trees: RefCell<HashMap<String, Tree>>,
    calls: RefCell<Vec<Call>>,
}

/// A handle to one in-memory `Source`. Clones share its state: give a clone to the run, and
/// keep one to seed it and read its calls.
#[derive(Clone, Default)]
pub struct FakeSource(Rc<State>);

impl FakeSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a repo's checkout into any observed state.
    pub fn seed(&self, id: &str, observed: Observed) {
        self.0.disk.borrow_mut().insert(id.into(), observed);
    }

    /// Make `method` fail for `id` with `message`, on every call.
    pub fn fail(&self, id: &str, method: Method, message: &str) {
        self.0
            .failures
            .borrow_mut()
            .insert((id.into(), method), message.into());
    }

    /// Undo `fail`, so a retry can succeed.
    pub fn heal(&self, id: &str, method: Method) {
        self.0.failures.borrow_mut().remove(&(id.into(), method));
    }

    /// The commit `resolve` returns for `id` from now on (default: 40 `a`s).
    pub fn set_commit(&self, id: &str, sha: &str) {
        self.0.commits.borrow_mut().insert(id.into(), sha.into());
    }

    /// The tags `tags` returns for `url` from now on, sorted.
    pub fn set_tags<'a>(&self, url: &str, names: impl IntoIterator<Item = &'a str>) {
        let mut names: Vec<String> = names.into_iter().map(Into::into).collect();
        names.sort();
        self.0.tags.borrow_mut().insert(url.into(), names);
    }

    /// From now on `git_ref` is gone from the remote at `url`: `resolve` says so.
    pub fn lose_ref(&self, url: &str, git_ref: &str) {
        self.0
            .lost_refs
            .borrow_mut()
            .insert((url.into(), git_ref.into()));
    }

    /// The commit `outline` resolves to for `url` from now on, and its files, as path and text
    /// pairs. Without one a URL has an empty tree at a commit of 40 `a`s.
    pub fn set_tree(&self, url: &str, sha: &str, files: &[(&str, &str)]) {
        let mut files: Vec<(String, String)> = files
            .iter()
            .map(|(path, text)| (path.to_string(), text.to_string()))
            .collect();
        files.sort();
        let tree = Tree {
            sha: sha.into(),
            files,
        };
        self.0.trees.borrow_mut().insert(url.into(), tree);
    }

    /// Every `resolve`, `verify`, `materialise`, `remove`, `tags` and `outline` received so far.
    pub fn calls(&self) -> Vec<Call> {
        self.0.calls.borrow().clone()
    }

    fn record(&self, call: Call) {
        self.0.calls.borrow_mut().push(call);
    }

    fn check(&self, id: &str, method: Method) -> Result<(), SourceError> {
        match self.0.failures.borrow().get(&(id.into(), method)) {
            Some(message) => Err(SourceError::Failed {
                message: message.clone(),
            }),
            None => Ok(()),
        }
    }
}

impl Source for FakeSource {
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError> {
        self.record(Call::Resolve(repo.id.into()));
        self.check(repo.id, Method::Resolve)?;
        let lost = (
            repo.repo.url.as_ref().clone(),
            repo.repo.effective_ref().into(),
        );
        if self.0.lost_refs.borrow().contains(&lost) {
            return Err(SourceError::RefNotFound {
                url: repo.repo.url.as_ref().clone(),
                git_ref: repo.repo.effective_ref().into(),
                help: "",
            });
        }
        let sha = self.0.commits.borrow().get(repo.id).cloned();
        Ok(Pin::new(
            repo.repo.url.as_ref(),
            repo.repo.effective_ref(),
            &sha.unwrap_or_else(|| "a".repeat(40)),
            None,
        ))
    }

    fn verify(&self, repo: RepoRef, _pin: &Pin, offline: bool) -> Result<(), SourceError> {
        self.record(Call::Verify {
            id: repo.id.into(),
            offline,
        });
        self.check(repo.id, Method::Verify)
    }

    fn materialise(&self, repo: RepoRef, pin: &Pin, offline: bool) -> Result<(), SourceError> {
        self.record(Call::Materialise {
            id: repo.id.into(),
            offline,
        });
        self.check(repo.id, Method::Materialise)?;
        if let Some(Observed::Foreign | Observed::Dangling) = self.0.disk.borrow().get(repo.id) {
            return Err(SourceError::Failed {
                message: format!(
                    "{} is not a Checkout of this Repo; refs leaves it alone",
                    repo.id
                ),
            });
        }
        let paths = repo.repo.path_strings();
        self.seed(
            repo.id,
            Observed::At {
                pin: pin.clone(),
                paths,
                dirty_files: vec![],
            },
        );
        Ok(())
    }

    fn tags(&self, url: &str) -> Result<Vec<String>, SourceError> {
        self.record(Call::Tags(url.into()));
        self.check(url, Method::Tags)?;
        Ok(self.0.tags.borrow().get(url).cloned().unwrap_or_default())
    }

    fn outline(&self, url: &str, git_ref: &str) -> Result<Outline, SourceError> {
        self.record(Call::Outline(url.into()));
        self.check(url, Method::Outline)?;
        let trees = self.0.trees.borrow();
        let (sha, paths) = match trees.get(url) {
            Some(tree) => (
                tree.sha.clone(),
                tree.files.iter().map(|(path, _)| path.clone()).collect(),
            ),
            None => ("a".repeat(40), vec![]),
        };
        Ok(Outline {
            pin: Pin::new(url, git_ref, &sha, None),
            paths,
        })
    }

    fn read(
        &self,
        url: &str,
        _pin: &Pin,
        paths: &[String],
    ) -> Result<Vec<(String, String)>, SourceError> {
        let trees = self.0.trees.borrow();
        let files = trees.get(url).map(|tree| tree.files.as_slice());
        paths
            .iter()
            .map(|path| {
                let found = files.unwrap_or_default().iter().find(|(p, _)| p == path);
                match found {
                    Some((_, text)) => Ok((path.clone(), text.clone())),
                    None => Err(SourceError::Failed {
                        message: format!("{path} is not in {url}"),
                    }),
                }
            })
            .collect()
    }

    fn remove(&self, id: &str) -> Result<(), SourceError> {
        self.record(Call::Remove(id.into()));
        self.check(id, Method::Remove)?;
        let mut disk = self.0.disk.borrow_mut();
        if disk.get(id) == Some(&Observed::Foreign) {
            return Err(SourceError::Failed {
                message: format!("{id} was not made by refs; leaving it alone"),
            });
        }
        disk.remove(id);
        Ok(())
    }

    fn inspect(&self, id: &str) -> Result<Observed, SourceError> {
        self.check(id, Method::Inspect)?;
        Ok(self
            .0
            .disk
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or(Observed::Absent))
    }

    fn list(&self) -> Result<Vec<String>, SourceError> {
        let mut names: Vec<String> = self
            .0
            .disk
            .borrow()
            .iter()
            .filter(|(_, o)| **o != Observed::Absent)
            .map(|(id, _)| id.clone())
            .collect();
        names.sort();
        Ok(names)
    }
}
