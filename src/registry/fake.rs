//! A scripted `Registry` for tests of the CLI: no network.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{Ecosystem, Fetch, Found, Registry, RegistryError, Reply};

/// One lookup the fake received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub ecosystem: Ecosystem,
    pub name: String,
}

#[derive(Default)]
pub struct FakeRegistry {
    packages: HashMap<(Ecosystem, String), Found>,
    calls: RefCell<Vec<Call>>,
}

impl FakeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry knows `name` in `ecosystem`, published from `url`.
    pub fn with(mut self, ecosystem: Ecosystem, name: &str, found: Found) -> Self {
        self.packages.insert((ecosystem, name.to_owned()), found);
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Registry for FakeRegistry {
    fn lookup(&self, ecosystem: Ecosystem, name: &str) -> Result<Found, RegistryError> {
        self.calls.borrow_mut().push(Call {
            ecosystem,
            name: name.to_owned(),
        });
        self.packages
            .get(&(ecosystem, name.to_owned()))
            .cloned()
            .ok_or_else(|| RegistryError::NotFound {
                ecosystem,
                name: name.to_owned(),
            })
    }
}

/// A scripted `Fetch`: the replies it was given by URL, and the URLs it was asked for.
#[derive(Default)]
pub struct FakeFetch {
    replies: HashMap<String, Result<Reply, String>>,
    asked: RefCell<Vec<String>>,
}

impl FakeFetch {
    pub fn new() -> Self {
        Self::default()
    }

    /// `url` answers with `status` and `body`.
    pub fn replying(mut self, url: &str, status: u16, body: &str) -> Self {
        let reply = Reply {
            status,
            body: body.to_owned(),
        };
        self.replies.insert(url.to_owned(), Ok(reply));
        self
    }

    /// `url` cannot be reached, for `why`.
    pub fn failing(mut self, url: &str, why: &str) -> Self {
        self.replies.insert(url.to_owned(), Err(why.to_owned()));
        self
    }

    /// The URLs asked for, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }
}

impl Fetch for FakeFetch {
    fn get(&self, url: &str) -> Result<Reply, String> {
        self.asked.borrow_mut().push(url.to_owned());
        self.replies
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(format!("no reply scripted for {url}")))
    }
}
