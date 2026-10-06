//! A stateful in-memory `Source` for tests of everything above the seam.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{MaterialiseOpts, Observed, Pin, Source, VerifyOpts};
use crate::config::RepoRef;
use crate::diagnostic::SourceError;

/// The `Source` methods a failure can be injected into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Resolve,
    Verify,
    Materialise,
    Remove,
    Inspect,
}

/// One call the fake received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Resolve(String),
    Verify { id: String, offline: bool },
    Materialise { id: String, offline: bool },
    Remove(String),
}

#[derive(Default)]
pub struct FakeSource {
    disk: RefCell<HashMap<String, Observed>>,
    failures: RefCell<HashMap<(String, Method), String>>,
    commits: RefCell<HashMap<String, String>>,
    calls: RefCell<Vec<Call>>,
}

impl FakeSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a repo's checkout into any observed state.
    pub fn seed(&self, id: &str, observed: Observed) {
        self.disk.borrow_mut().insert(id.into(), observed);
    }

    /// Make `method` fail for `id` with `message`, on every call.
    pub fn fail(&self, id: &str, method: Method, message: &str) {
        self.failures
            .borrow_mut()
            .insert((id.into(), method), message.into());
    }

    /// Undo `fail`, so a retry can succeed.
    pub fn heal(&self, id: &str, method: Method) {
        self.failures.borrow_mut().remove(&(id.into(), method));
    }

    /// The commit `resolve` returns for `id` from now on (default: 40 `a`s).
    pub fn set_commit(&self, id: &str, sha: &str) {
        self.commits.borrow_mut().insert(id.into(), sha.into());
    }

    /// Every `resolve`, `verify`, `materialise` and `remove` received so far.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: Call) {
        self.calls.borrow_mut().push(call);
    }

    fn check(&self, id: &str, method: Method) -> Result<(), SourceError> {
        match self.failures.borrow().get(&(id.into(), method)) {
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
        let sha = self.commits.borrow().get(repo.id).cloned();
        Ok(Pin::git(
            repo.repo.url.as_ref(),
            repo.repo.effective_ref(),
            &sha.unwrap_or_else(|| "a".repeat(40)),
            None,
        ))
    }

    fn verify(&self, repo: RepoRef, _pin: &Pin, opts: VerifyOpts) -> Result<(), SourceError> {
        self.record(Call::Verify {
            id: repo.id.into(),
            offline: opts.offline,
        });
        self.check(repo.id, Method::Verify)
    }

    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        opts: MaterialiseOpts,
    ) -> Result<(), SourceError> {
        self.record(Call::Materialise {
            id: repo.id.into(),
            offline: opts.offline,
        });
        self.check(repo.id, Method::Materialise)?;
        if let Some(Observed::Foreign | Observed::Dangling) = self.disk.borrow().get(repo.id) {
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

    fn remove(&self, id: &str) -> Result<(), SourceError> {
        self.record(Call::Remove(id.into()));
        self.check(id, Method::Remove)?;
        let mut disk = self.disk.borrow_mut();
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
            .disk
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or(Observed::Absent))
    }

    fn list(&self) -> Result<Vec<String>, SourceError> {
        let mut names: Vec<String> = self
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

/// A borrowed fake is a `Source`, so a test can hand one to `cli::run` and keep its handle to
/// seed it and read its calls.
impl Source for &FakeSource {
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError> {
        (**self).resolve(repo)
    }
    fn verify(&self, repo: RepoRef, pin: &Pin, opts: VerifyOpts) -> Result<(), SourceError> {
        (**self).verify(repo, pin, opts)
    }
    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        opts: MaterialiseOpts,
    ) -> Result<(), SourceError> {
        (**self).materialise(repo, pin, opts)
    }
    fn remove(&self, id: &str) -> Result<(), SourceError> {
        (**self).remove(id)
    }
    fn inspect(&self, id: &str) -> Result<Observed, SourceError> {
        (**self).inspect(id)
    }
    fn list(&self) -> Result<Vec<String>, SourceError> {
        (**self).list()
    }
}
