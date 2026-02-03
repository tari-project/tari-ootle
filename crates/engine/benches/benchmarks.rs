//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod common;

use std::{hint::black_box, iter, sync::Arc};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tari_engine::{
    executables::{Executable, Instructions, WeightedExecutable},
    runtime::AuthParams,
    state_store::memory::ReadOnlyMemoryStateStore,
    transaction::TransactionProcessor,
};
use tari_engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId, VirtualSubstates};
use tari_ootle_common_types::SubstateRequirementRef;
use tari_ootle_transaction::{args::WorkspaceOffsetId, call_args, Instruction, TransactionId, TransactionWeight};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;
use tari_template_test_tooling::{mocks::AlwaysPassesProofVerifier, Package};

use crate::common::{setup_store, FAUCET_COMPONENT_ADDRESS};

type BenchTxProcessor = TransactionProcessor<ReadOnlyMemoryStateStore, Package>;

pub struct CreateAndFundAccountExecutable;

impl Executable for CreateAndFundAccountExecutable {
    fn to_id(&self) -> TransactionId {
        TransactionId::default()
    }

    fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_ {
        iter::empty()
    }

    fn signers_iter(&self) -> impl Iterator<Item = &RistrettoPublicKeyBytes> {
        static SIGNER: RistrettoPublicKeyBytes = RistrettoPublicKeyBytes::zero();
        iter::once(&SIGNER)
    }

    fn into_instructions(self) -> Instructions {
        Instructions {
            fee: vec![
                Instruction::CallMethod {
                    call: FAUCET_COMPONENT_ADDRESS.into(),
                    method: "take".try_into().unwrap(),
                    args: call_args![1000],
                },
                Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
                Instruction::CreateAccount {
                    owner_public_key: Default::default(),
                    owner_rule: None,
                    access_rules: None,
                    bucket_workspace_id: Some(WorkspaceOffsetId::new(0)),
                },
            ],
            main: vec![],
        }
    }
}

impl WeightedExecutable for CreateAndFundAccountExecutable {
    fn calculate_weight(&self) -> TransactionWeight {
        TransactionWeight::new(100)
    }
}

pub struct SharedState {
    package: Arc<Package>,
    virtual_substates: VirtualSubstates,
    state_store: ReadOnlyMemoryStateStore,
    claim_burn_proof_verifier: Arc<AlwaysPassesProofVerifier>,
}

fn setup(shared: &SharedState) -> BenchTxProcessor {
    let auth_params = AuthParams::default();
    let modules = Default::default();
    BenchTxProcessor::new(
        shared.package.clone(),
        shared.state_store.clone(),
        auth_params,
        shared.virtual_substates.clone(),
        modules,
        shared.claim_burn_proof_verifier.clone(),
    )
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut package = Package::builder();
    package.add_all_builtin_templates();
    let state_store = setup_store();
    let virtual_substates = VirtualSubstates::from_iter(iter::once((
        VirtualSubstateId::CurrentEpoch,
        VirtualSubstate::CurrentEpoch(1),
    )));
    let claim_burn_proof_verifier = Arc::new(AlwaysPassesProofVerifier);
    let shared = SharedState {
        package: Arc::new(package.build()),
        virtual_substates,
        state_store: state_store.into_read_only(),
        claim_burn_proof_verifier,
    };

    let id = BenchmarkId::new("create_and_fund_account", "");

    c.bench_with_input(id, &shared, |b, shared| {
        b.iter_batched(
            || setup(shared),
            |processor| {
                let res = black_box(processor).execute(CreateAndFundAccountExecutable).unwrap();
                res.expect_success();
                // Return to avoid the compiler optimising anything out, since criterion will black_box the return. Not
                // 100% sure if it makes a difference, but just in case
                res
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
