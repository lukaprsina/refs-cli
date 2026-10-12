//! A scripted `Registry` for tests of the CLI: no network.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{Ecosystem, Found, Package, Registry, RegistryError};

/// One lookup the fake received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub package: Package,
}

#[derive(Default)]
pub struct FakeRegistry {
    packages: HashMap<Package, Found>,
    calls: RefCell<Vec<Call>>,
}

impl FakeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry knows `name` in `ecosystem`, published from `url`.
    pub fn with(mut self, ecosystem: Ecosystem, name: &str, found: Found) -> Self {
        self.packages.insert(Package::new(ecosystem, name), found);
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Registry for FakeRegistry {
    fn lookup(&self, package: &Package) -> Result<Found, RegistryError> {
        self.calls.borrow_mut().push(Call {
            package: package.clone(),
        });
        self.packages
            .get(package)
            .cloned()
            .ok_or_else(|| RegistryError::NotFound {
                package: package.clone(),
            })
    }
}
