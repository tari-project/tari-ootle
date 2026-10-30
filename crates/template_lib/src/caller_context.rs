//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Context definitions related to the caller of an instruction

use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::{ComponentAddress, NonFungibleAddress};

use crate::{
    args::{AddressAllocationInvokeArg, CallerContextAction, CallerContextInvokeArg, InvokeResult},
    error_variants::{ERR_ENGINE_DECODE_FAIL, ERR_NOT_IN_COMPONENT_CONTEXT},
    models::{ComponentAddressAllocation, ResourceAddressAllocation},
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

        resp.decode().expect(ERR_ENGINE_DECODE_FAIL)
    }

    /// Fails the transaction unless the current call carries `badge`, evaluated as `rule!(non_fungible(badge))`
    /// against this frame. A call carries the badges the engine stamps into its frame (the transaction signers'
    /// public-key badges when the transaction calls this component or function directly, and otherwise the badges
    /// naming its direct caller: [`NonFungibleAddress::caller_component_badge`] and
    /// [`NonFungibleAddress::direct_caller_template_badge`]), the badges of the proofs the frame has authorized,
    /// and those of the proofs its caller authorized around the call. A proof shown as an argument does not count.
    pub fn assert_badge_in_scope(badge: &NonFungibleAddress) {
        let _resp: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &CallerContextInvokeArg {
            action: CallerContextAction::AssertBadgeInScope,
            args: invoke_args![badge],
        });
    }

    /// Returns the address of the component that is being called in the current instruction.
    /// Assumes that the instruction is a call method; otherwise, it will panic
    pub fn current_component_address() -> ComponentAddress {
        let resp: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &CallerContextInvokeArg {
            action: CallerContextAction::GetComponentAddress,
            args: invoke_args![],
        });

        resp.decode::<Option<ComponentAddress>>()
            .expect(ERR_ENGINE_DECODE_FAIL)
            .expect(ERR_NOT_IN_COMPONENT_CONTEXT)
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

        resp.decode().expect(ERR_ENGINE_DECODE_FAIL)
    }

    pub fn allocate_resource_address() -> ResourceAddressAllocation {
        let resp: InvokeResult = call_engine(
            EngineOp::AddressAllocationInvoke,
            &AddressAllocationInvokeArg::CreateResourceAllocation,
        );

        resp.decode().expect(ERR_ENGINE_DECODE_FAIL)
    }
}
