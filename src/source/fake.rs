//! A stateful in-memory `Source` for tests of everything above the seam.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{MaterialiseOpts, Observed, Pin, Source};
use crate::config::RepoRef;
use crate::diagnostic::SourceError;

/// The `Source` methods a failure can be injected into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Resolve,
    Verify,
    Materialise,
    Remove,
}

#[derive(Default)]
pub struct FakeSource {
    disk: RefCell<HashMap<String, Observed>>,
    failures: RefCell<HashMap<(String, Method), String>>,
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
        self.check(repo.id, Method::Resolve)?;
        Ok(Pin::git(
            repo.repo.url.as_ref(),
            repo.repo.effective_ref(),
            &"a".repeat(40),
            None,
        ))
    }

    fn verify(&self, repo: RepoRef, _pin: &Pin) -> Result<(), SourceError> {
        self.check(repo.id, Method::Verify)
    }

    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        _opts: MaterialiseOpts,
    ) -> Result<(), SourceError> {
        self.check(repo.id, Method::Materialise)?;
        let paths = repo.repo.paths.iter().map(|p| p.as_ref().clone()).collect();
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
        self.check(id, Method::Remove)?;
        self.disk.borrow_mut().remove(id);
        Ok(())
    }

    fn inspect(&self, id: &str) -> Result<Observed, SourceError> {
        Ok(self
            .disk
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or(Observed::Absent))
    }
}
