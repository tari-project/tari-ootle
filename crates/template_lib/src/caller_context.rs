//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Context definitions related to the caller of an instruction

use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::ComponentAddress;

use crate::{
    args::{AddressAllocationInvokeArg, CallerContextAction, CallerContextInvokeArg, InvokeResult},
    error_variants::{ERR_ENGINE_DECODE_FAIL, ERR_NOT_IN_COMPONENT_CONTEXT},
    models::{ComponentAddressAllocation, Proof, ResourceAddressAllocation},
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

    /// Returns a proof of the main/seal transaction signer
    pub fn get_main_signer_proof() -> Proof {
        Self::get_signer_proof_inner(None)
    }

    /// Returns a proof of the transaction signer with the given public key. If the public key does not match any
    /// transaction signer, this will panic.
    pub fn get_signer_proof_for_public_key(public_key: RistrettoPublicKeyBytes) -> Proof {
        Self::get_signer_proof_inner(Some(public_key))
    }

    fn get_signer_proof_inner(pk: Option<RistrettoPublicKeyBytes>) -> Proof {
        let resp: InvokeResult = call_engine(EngineOp::CallerContextInvoke, &CallerContextInvokeArg {
            action: CallerContextAction::GetSignerProof,
            args: pk.map(|pk| invoke_args![pk]).unwrap_or_default(),
        });

        resp.decode().expect(ERR_ENGINE_DECODE_FAIL)
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
