//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::models::{ComponentAddress, ResourceAddress};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressAllocation<T> {
    id: u32,
    address: T,
}

impl<T> AddressAllocation<T> {
    pub fn new(id: u32, address: T) -> Self {
        Self { id, address }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn address(&self) -> &T {
        &self.address
    }
}

impl From<ComponentAddress> for AddressAllocation<ComponentAddress> {
    fn from(value: ComponentAddress) -> Self {
        Self::new(0, value)
    }
}

impl From<ResourceAddress> for AddressAllocation<ResourceAddress> {
    fn from(value: ResourceAddress) -> Self {
        Self::new(0, value)
    }
}
