//! A scripted `Registry` for tests of the CLI: no network.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{Ecosystem, Found, Registry, RegistryError};

/// One lookup the fake received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Default)]
pub struct FakeRegistry {
    packages: HashMap<(String, String), Found>,
    calls: RefCell<Vec<Call>>,
}

impl FakeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry knows `name` in `ecosystem`, published from `url`.
    pub fn with(mut self, ecosystem: Ecosystem, name: &str, found: Found) -> Self {
        self.packages
            .insert((format!("{ecosystem:?}"), name.to_owned()), found);
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Registry for FakeRegistry {
    fn lookup(
        &self,
        ecosystem: Ecosystem,
        name: &str,
        version: Option<&str>,
    ) -> Result<Found, RegistryError> {
        self.calls.borrow_mut().push(Call {
            ecosystem,
            name: name.to_owned(),
            version: version.map(str::to_owned),
        });
        self.packages
            .get(&(format!("{ecosystem:?}"), name.to_owned()))
            .cloned()
            .ok_or_else(|| RegistryError::NotFound {
                registry: ecosystem.registry(),
                name: name.to_owned(),
            })
    }
}
