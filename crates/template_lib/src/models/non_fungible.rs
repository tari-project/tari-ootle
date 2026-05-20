//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::{EngineOp, call_engine, rust::vec};
use tari_template_lib_types::NonFungibleAddress;

use crate::{
    args::{InvokeResult, NonFungibleAction, NonFungibleInvokeArg},
    resource::ResourceManager,
};

/// Non-Fungible token engine API used to get/set non-fungible mutable data.
/// Each non-fungible token is uniquely addressable inside its parent resource and can hold arbitrary data.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
pub struct NonFungible {
    #[n(0)]
    address: NonFungibleAddress,
}

impl NonFungible {
    pub fn new(address: NonFungibleAddress) -> Self {
        Self { address }
    }

    /// Returns a copy of the immutable data of the token.
    /// This data is set up during the token minting process and cannot be updated
    pub fn get_data<T: for<'b> Decode<'b, ()>>(&self) -> T {
        let resp: InvokeResult = call_engine(EngineOp::NonFungibleInvoke, &NonFungibleInvokeArg {
            address: self.address.clone(),
            action: NonFungibleAction::GetData,
            args: vec![],
        });

        resp.decode().expect("[get_data] Failed to decode NonFungible data")
    }

    /// Returns a copy of the mutable data of the token
    pub fn get_mutable_data<T: for<'b> Decode<'b, ()>>(&self) -> T {
        let resp: InvokeResult = call_engine(EngineOp::NonFungibleInvoke, &NonFungibleInvokeArg {
            address: self.address.clone(),
            action: NonFungibleAction::GetMutableData,
            args: vec![],
        });

        resp.decode()
            .expect("[get_mutable_data] Failed to decode raw NonFungible mutable data")
    }

    /// Update the mutable data of the token, replacing it with the data provided as an argument.
    /// Note that this operation may be protected via access rules, resulting in a panic if the caller does not have the
    /// appropriate permissions
    pub fn set_mutable_data<T: Encode<()> + ?Sized>(&mut self, data: &T) {
        ResourceManager::get(*self.address.resource_address())
            .update_non_fungible_data(self.address.id().clone(), data);
    }
}
