//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::BorTag;
use tari_template_abi::{call_engine, EngineOp};

use crate::{
    args::{AddressAllocationInvokeArg, InvokeResult},
    models::{BinaryTag, ComponentAddress, ResourceAddress},
};

/// Represents an allocation of an address for a component or resource.
pub type AddressAllocationId = u32;

const COMPONENT_TAG: u64 = BinaryTag::AllocatedComponentAddress.as_u64();

/// Represents an allocation of an address for a component.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(transparent)]
pub struct ComponentAddressAllocation(BorTag<AddressAllocationId, COMPONENT_TAG>);

impl ComponentAddressAllocation {
    /// Creates a new `ComponentAddressAllocation` with the given ID.
    /// For internal use.
    pub fn new(id: AddressAllocationId) -> Self {
        Self(BorTag::new(id))
    }

    /// Returns the ID of the address allocation.
    pub fn id(&self) -> AddressAllocationId {
        self.0.into_inner()
    }

    /// Retrieves the allocated address for the component.
    pub fn get_address(&self) -> ComponentAddress {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::GetAddress(self.id()),
        );

        resp.decode()
            .expect("Failed to decode ComponentAddressAllocation response")
    }
}

const RESOURCE_TAG: u64 = BinaryTag::AllocatedResourceAddress.as_u64();

/// Represents an allocation of an address for a resource.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(transparent)]
pub struct ResourceAddressAllocation(BorTag<AddressAllocationId, RESOURCE_TAG>);

impl ResourceAddressAllocation {
    /// Creates a new `ResourceAddressAllocation` with the given ID.
    /// For internal use.
    pub fn new(id: AddressAllocationId) -> Self {
        Self(BorTag::new(id))
    }

    /// Returns the ID of the address allocation.
    pub fn id(&self) -> AddressAllocationId {
        self.0.into_inner()
    }

    /// Retrieves the allocated address for the resource.
    pub fn get_address(&self) -> ResourceAddress {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::GetAddress(self.id()),
        );

        resp.decode()
            .expect("Failed to decode ResourceAddressAllocation response")
    }
}
