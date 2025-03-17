//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::BorTag;
use tari_template_abi::{call_engine, EngineOp};

use crate::{
    args::{AddressAllocationInvokeArg, InvokeResult},
    models::{BinaryTag, ComponentAddress, ResourceAddress},
};

pub type AddressAllocationId = u32;

const COMPONENT_TAG: u64 = BinaryTag::AllocatedComponentAddress.as_u64();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(transparent)]
pub struct ComponentAddressAllocation(BorTag<AddressAllocationId, COMPONENT_TAG>);

impl ComponentAddressAllocation {
    pub fn new(id: AddressAllocationId) -> Self {
        Self(BorTag::new(id))
    }

    pub fn id(&self) -> AddressAllocationId {
        self.0.into_inner()
    }

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(transparent)]
pub struct ResourceAddressAllocation(BorTag<AddressAllocationId, RESOURCE_TAG>);

impl ResourceAddressAllocation {
    pub fn new(id: AddressAllocationId) -> Self {
        Self(BorTag::new(id))
    }

    pub fn id(&self) -> AddressAllocationId {
        self.0.into_inner()
    }

    pub fn get_address(&self) -> ResourceAddress {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::GetAddress(self.id()),
        );

        resp.decode()
            .expect("Failed to decode ResourceAddressAllocation response")
    }
}
