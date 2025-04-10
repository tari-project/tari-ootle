//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Context definitions related to the caller of an instruction

use tari_template_abi::{call_engine, EngineOp};

use crate::{
    args::{AddressAllocationInvokeArg, CallerContextAction, CallerContextInvokeArg, InvokeResult},
    models::{ComponentAddress, ComponentAddressAllocation, ResourceAddressAllocation},
    types::crypto::RistrettoPublicKeyBytes,
};

/// Allows a template to access information about the current instruction's caller
pub struct CallerContext;

impl CallerContext {
    /// Returns the  public key used to sign the transaction that is currently being executed
    pub fn transaction_signer_public_key() -> RistrettoPublicKeyBytes {
        let resp: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &CallerContextInvokeArg {
            action: CallerContextAction::GetCallerPublicKey,
            args: invoke_args![],
        });

        resp.decode().expect("Failed to decode PublicKey")
    }

    /// Returns the address of the component that is being called in the current instruction.
    /// Assumes that the instruction is a call method; otherwise, it will panic
    pub fn current_component_address() -> ComponentAddress {
        let resp: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &CallerContextInvokeArg {
            action: CallerContextAction::GetComponentAddress,
            args: invoke_args![],
        });

        resp.decode::<Option<ComponentAddress>>()
            .expect("Failed to decode Option<ComponentAddress>")
            .expect("Not in a component instance context")
    }

    /// Alias function to allocate component address
    pub fn allocate_component_address(
        public_key_address: Option<RistrettoPublicKeyBytes>,
    ) -> ComponentAddressAllocation {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::CreateComponentAllocation {
                public_key: public_key_address,
            },
        );

        resp.decode().expect("Failed to decode ComponentAddressAllocation ")
    }

    pub fn allocate_resource_address() -> ResourceAddressAllocation {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::CreateResourceAllocation,
        );

        resp.decode().expect("Failed to decode ResourceAddressAllocation ")
    }
}
