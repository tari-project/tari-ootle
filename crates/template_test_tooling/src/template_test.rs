//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::Path,
    sync::Arc,
    time::Instant,
};

use anyhow::anyhow;
use serde::de::DeserializeOwned;
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::{hex::Hex, ByteArray},
};
use tari_engine::{
    executables::Executable,
    fees::{FeeModule, FeeTable},
    runtime::{AuthParams, RuntimeModule},
    state_store::{memory::MemoryStateStore, new_memory_store, StateWriter},
    template::LoadedTemplate,
    transaction::{TransactionError, TransactionProcessor, TransactionProcessorConfig},
    wasm::LoadedWasmTemplate,
};
use tari_engine_types::{
    commit_result::{ExecuteResult, RejectReason},
    instruction::Instruction,
    substate::{SubstateDiff, SubstateId},
    virtual_substate::{VirtualSubstate, VirtualSubstateId, VirtualSubstates},
    ToByteType,
};
use tari_ootle_common_types::{
    crypto::create_key_pair_from_seed,
    substate_type::SubstateType,
    Network,
    SubstateRequirement,
};
use tari_template_builtin::{ACCOUNT_TEMPLATE_ADDRESS, NFT_FAUCET_TEMPLATE_ADDRESS};
use tari_template_lib::{
    args::InstructionArg,
    constants::{NFT_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS},
    models::{Amount, ComponentAddress, NonFungibleAddress},
    prelude::RistrettoPublicKeyBytes,
    types::TemplateAddress,
};
use tari_transaction::{args, builder::named_args::BuilderWorkspaceKey, Transaction, TransactionBuilder};
use tari_transaction_manifest::{parse_manifest, ManifestValue};

use crate::{
    builtin_component_state::{initialize_builtin_faucet_state, initialize_builtin_nft_faucet_state},
    read_only_state_store::ReadOnlyStateStore,
    track_calls::TrackCallsModule,
    wrapped_transaction::WrappedTransaction,
    Package,
};

pub fn test_faucet_component() -> ComponentAddress {
    XTR_FAUCET_COMPONENT_ADDRESS
}

pub fn test_nft_faucet_component() -> ComponentAddress {
    NFT_FAUCET_COMPONENT_ADDRESS
}

pub struct TemplateTest {
    package: Package,
    track_calls: TrackCallsModule,
    secret_key: RistrettoSecretKey,
    public_key: RistrettoPublicKey,
    last_outputs: HashSet<SubstateId>,
    name_to_template: HashMap<String, TemplateAddress>,
    state_store: MemoryStateStore,
    enable_fees: bool,
    fee_table: FeeTable,
    virtual_substates: VirtualSubstates,
    key_seed: u8,
}

impl TemplateTest {
    pub fn new<I: IntoIterator<Item = P>, P: Clone + AsRef<Path>>(template_paths: I) -> Self {
        Self::new_internal(template_paths, None::<(String, String)>)
    }

    pub fn new_with_compile_envs<I, P, TEnvs, K, V>(template_paths: I, envs: TEnvs) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Clone + AsRef<Path>,
        TEnvs: IntoIterator<Item = (K, V)>,
        TEnvs::IntoIter: Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        Self::new_internal(template_paths, envs)
    }

    fn new_internal<I, P, TEnvs, K, V>(template_paths: I, envs: TEnvs) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Clone + AsRef<Path>,
        TEnvs: IntoIterator<Item = (K, V)>,
        TEnvs::IntoIter: Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let mut builder = Package::builder();

        // Add builtin templates
        builder.add_builtin_template(&ACCOUNT_TEMPLATE_ADDRESS);
        builder.add_builtin_template(&NFT_FAUCET_TEMPLATE_ADDRESS);

        // Add the faucet template for fungible tokens
        builder.add_template(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/faucet"));

        // Add all of the templates specified in the argument
        let envs_iter = envs.into_iter();
        for path in template_paths {
            builder.add_template_with_envs(path, envs_iter.clone());
        }

        let package = builder.build();

        let mut test = Self::from_package(package);
        test.bootstrap_state();
        test
    }

    fn from_package(package: Package) -> Self {
        let secret_key =
            RistrettoSecretKey::from_hex("8a39567509bf2f7074e5fd153337405292cdc9f574947313b62fbf8fb4cffc02").unwrap();

        let public_key = RistrettoPublicKey::from_secret_key(&secret_key);

        let mut name_to_template = HashMap::new();

        for (addr, template) in &package.templates() {
            if name_to_template
                .insert(template.template_name().to_string(), *addr)
                .is_some()
            {
                panic!("Duplicate template name: {}", template.template_name());
            }
        }

        let mut virtual_substates = VirtualSubstates::new();
        virtual_substates.insert(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(0));

        Self {
            package,
            track_calls: TrackCallsModule::new(),
            public_key,
            secret_key,
            name_to_template,
            last_outputs: HashSet::new(),
            state_store: new_memory_store(),
            virtual_substates,
            enable_fees: false,
            fee_table: FeeTable {
                per_transaction_weight_cost: 1,
                per_module_call_cost: 1,
                per_byte_storage_cost: 1,
                per_event_cost: 1,
                per_log_cost: 1,
            },
            key_seed: 1,
        }
    }

    pub fn bootstrap_state(&mut self) {
        let template_addr = self.get_template_address("TestFaucet");
        initialize_builtin_faucet_state(&mut self.state_store, &self.public_key, template_addr);
        initialize_builtin_nft_faucet_state(&mut self.state_store)
    }

    pub fn compile_new_template<T, P, TEnvs, K, V>(
        &mut self,
        name: T,
        path: P,
        features: &[&str],
        envs: TEnvs,
    ) -> TemplateAddress
    where
        T: Into<String>,
        P: AsRef<Path>,
        TEnvs: IntoIterator<Item = (K, V)>,
        TEnvs::IntoIter: Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let mut builder = Package::builder();
        for (addr, template) in self.package.templates() {
            builder.add_loaded_template(addr, template);
        }
        let template_addr = builder.add_template_opts(path, features, envs);
        self.package = builder.build();
        self.name_to_template.insert(name.into(), template_addr);

        template_addr
    }

    pub fn enable_fees(&mut self) -> &mut Self {
        self.enable_fees = true;
        self
    }

    pub fn disable_fees(&mut self) -> &mut Self {
        self.enable_fees = false;
        self
    }

    pub fn fee_table(&self) -> &FeeTable {
        &self.fee_table
    }

    pub fn set_fee_table(&mut self, fee_table: FeeTable) -> &mut Self {
        self.fee_table = fee_table;
        self
    }

    pub fn set_virtual_substate(&mut self, address: VirtualSubstateId, value: VirtualSubstate) -> &mut Self {
        self.virtual_substates.insert(address, value);
        self
    }

    pub fn read_only_state_store(&self) -> ReadOnlyStateStore<'_> {
        ReadOnlyStateStore::new(&self.state_store)
    }

    pub fn extract_component_value<T: DeserializeOwned>(&self, component_address: ComponentAddress, path: &str) -> T {
        self.read_only_state_store()
            .inspect_component(component_address)
            .unwrap()
            .get_value(path)
            .unwrap()
            .unwrap_or_else(|| panic!("Expected component to have value at '{path}' but no value was found"))
    }

    pub fn default_signing_key(&self) -> &RistrettoSecretKey {
        &self.secret_key
    }

    pub fn assert_calls(&self, expected: &[&'static str]) {
        let calls = self.track_calls.get();
        assert_eq!(calls, expected);
    }

    pub fn clear_calls(&self) {
        self.track_calls.clear();
    }

    pub fn get_previous_output_address(&self, ty: SubstateType) -> SubstateId {
        self.last_outputs
            .iter()
            .find(|addr| ty.matches(addr))
            .cloned()
            .unwrap_or_else(|| panic!("No output of type {:?}", ty))
    }

    fn commit_diff(&mut self, diff: &SubstateDiff) {
        self.last_outputs.clear();

        for (address, _) in diff.down_iter() {
            eprintln!("DOWN substate: {}", address);
            self.state_store.delete_state(address);
        }

        for (address, substate) in diff.up_iter() {
            eprintln!("UP substate: {}", address);
            self.last_outputs.insert(address.clone());
            self.state_store.set_state(address.clone(), substate.clone()).unwrap();
        }
    }

    pub fn get_module(&self, module_name: &str) -> LoadedWasmTemplate {
        let addr = self.name_to_template.get(module_name).unwrap();
        match self.package.get_template_by_address(addr).unwrap() {
            LoadedTemplate::Wasm(wasm) => wasm,
        }
    }

    pub fn get_template_address(&self, name: &str) -> TemplateAddress {
        *self
            .name_to_template
            .get(name)
            .unwrap_or_else(|| panic!("No template with name {}", name))
    }

    pub fn create_account<T>(
        &mut self,
        owner_public_key: RistrettoPublicKeyBytes,
        workspace_id: Option<BuilderWorkspaceKey>,
        proofs: Vec<NonFungibleAddress>,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let result = self
            .build_and_execute(
                Transaction::builder().create_account_with_custom_rules(owner_public_key, None, None, workspace_id),
                proofs,
            )
            .unwrap_success();
        result
            .finalize
            .execution_results
            .first()
            .expect("single instruction without execution result")
            .decode()
            .unwrap()
    }

    pub fn call_function<T>(
        &mut self,
        template_name: &str,
        func_name: &str,
        args: Vec<InstructionArg>,
        proofs: Vec<NonFungibleAddress>,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let result = self
            .execute_and_commit(
                vec![Instruction::CallFunction {
                    address: self.get_template_address(template_name),
                    function: func_name.to_owned(),
                    args,
                }],
                proofs,
            )
            .unwrap();
        result
            .finalize
            .execution_results
            .first()
            .expect("single instruction without execution result")
            .decode()
            .unwrap()
    }

    pub fn call_method<T>(
        &mut self,
        component_address: ComponentAddress,
        method_name: &str,
        args: Vec<InstructionArg>,
        proofs: Vec<NonFungibleAddress>,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let result = self
            .execute_and_commit(
                vec![Instruction::CallMethod {
                    call: component_address.into(),
                    method: method_name.to_owned(),
                    args,
                }],
                proofs,
            )
            .unwrap();

        result
            .finalize
            .execution_results
            .first()
            .expect("single instruction without execution result")
            .decode()
            .unwrap()
    }

    pub fn get_test_proof_and_secret_key(&self) -> (NonFungibleAddress, RistrettoSecretKey) {
        (self.get_test_proof(), self.secret_key.clone())
    }

    pub fn get_test_proof(&self) -> NonFungibleAddress {
        NonFungibleAddress::from_public_key(self.get_test_public_key_bytes())
    }

    pub fn secret_key(&self) -> &RistrettoSecretKey {
        &self.secret_key
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    pub fn get_test_public_key_bytes(&self) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(self.public_key.as_bytes()).unwrap()
    }

    pub fn create_empty_account(&mut self) -> (ComponentAddress, NonFungibleAddress, RistrettoSecretKey) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        let component = self.create_account(public_key.to_byte_type(), None, vec![owner_proof.clone()]);
        self.enable_fees = old_fail_fees;
        (component, owner_proof, secret_key)
    }

    #[deprecated(
        since = "0.1.0",
        note = "Please use create_funded_account instead. This method will be removed."
    )]
    pub fn create_owned_account(&mut self) -> (ComponentAddress, NonFungibleAddress, RistrettoSecretKey) {
        self.create_funded_account()
    }

    pub fn create_funded_account(&mut self) -> (ComponentAddress, NonFungibleAddress, RistrettoSecretKey) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        let result = self.execute_expect_success(
            Transaction::builder()
                .call_method(test_faucet_component(), "take_free_coins", args![])
                .put_last_instruction_output_on_workspace("bucket")
                .create_account_with_bucket(public_key.to_byte_type(), "bucket")
                .build_and_seal(&secret_key),
            vec![owner_proof.clone()],
        );

        let component = result
            .finalize
            .execution_results
            .get(2)
            .expect("instruction at 2 no execution result")
            .decode::<ComponentAddress>()
            .unwrap();

        self.enable_fees = old_fail_fees;
        (component, owner_proof, secret_key)
    }

    pub fn create_custom_funded_account(
        &mut self,
        amount: Amount,
    ) -> (
        ComponentAddress,
        NonFungibleAddress,
        RistrettoSecretKey,
        RistrettoPublicKey,
    ) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        let result = self.execute_expect_success(
            Transaction::builder()
                .call_method(test_faucet_component(), "take_free_coins_custom", args![amount])
                .put_last_instruction_output_on_workspace("bucket")
                .create_account_with_bucket(public_key.to_byte_type(), "bucket")
                .build_and_seal(&secret_key),
            vec![owner_proof.clone()],
        );

        let component = result
            .finalize
            .execution_results
            .get(2)
            .expect("instruction at 2 no execution result")
            .decode::<ComponentAddress>()
            .unwrap();

        self.enable_fees = old_fail_fees;
        (component, owner_proof, secret_key, public_key)
    }

    fn next_key_seed(&mut self) -> u8 {
        let seed = self.key_seed;
        self.key_seed += 1;
        seed
    }

    pub fn create_owner_proof(&mut self) -> (NonFungibleAddress, RistrettoPublicKey, RistrettoSecretKey) {
        let (secret_key, public_key) = create_key_pair_from_seed(self.next_key_seed());
        let owner_token = NonFungibleAddress::from_public_key(public_key.to_byte_type());
        (owner_token, public_key, secret_key)
    }

    pub fn try_execute_instructions(
        &mut self,
        fee_instructions: Vec<Instruction>,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> Result<ExecuteResult, TransactionError> {
        let transaction = Transaction::builder()
            .with_fee_instructions(fee_instructions)
            .with_instructions(instructions)
            .build_and_seal(&self.secret_key);

        self.try_execute(transaction, proofs)
    }

    pub fn try_execute(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> Result<ExecuteResult, TransactionError> {
        let mut modules: Vec<Arc<dyn RuntimeModule>> = vec![Arc::new(self.track_calls.clone())];

        if self.enable_fees {
            modules.push(Arc::new(FeeModule::new(0, self.fee_table.clone())));
        }

        let auth_params = AuthParams {
            initial_ownership_proofs: proofs,
        };
        let processor = TransactionProcessor::new(
            TransactionProcessorConfig::builder()
                .with_network(Network::LocalNet)
                .build(),
            Arc::new(self.package.clone()),
            self.state_store.clone().into_read_only(),
            auth_params,
            self.virtual_substates.clone(),
            modules,
        );

        let mut wrapped_transaction = WrappedTransaction::new(transaction);
        // Add all the substates as inputs - this avoids the need for tests to explicitly include inputs
        wrapped_transaction.extend_inputs(
            self.state_store
                .iter()
                .map(|(id, s)| SubstateRequirement::versioned(id.clone(), s.version())),
        );

        let tx_id = wrapped_transaction.to_id();
        eprintln!("START Transaction id = \"{}\"", tx_id);

        let result = processor.execute(wrapped_transaction)?;

        if self.enable_fees {
            let fee = &result.finalize.fee_receipt;
            eprintln!("Initial payment: {}", fee.total_allocated_fee_payments());
            eprintln!("Fee: {}", fee.total_fees_charged());
            eprintln!("Paid: {}", fee.total_fees_paid());
            eprintln!("Refund: {}", fee.total_refunded());
            eprintln!("Unpaid: {}", fee.unpaid_debt());
            for (source, amount) in fee.cost_breakdown.iter() {
                eprintln!("- {:?} {}", source, amount);
            }
        }

        let timer = Instant::now();
        eprintln!("Finished Transaction \"{}\" in {:.2?}", tx_id, timer.elapsed());
        eprintln!();

        Ok(result)
    }

    pub fn execute_and_commit_on_success(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> ExecuteResult {
        let result = self.try_execute(transaction, proofs).unwrap();
        if let Some(diff) = result.finalize.result.accept() {
            self.commit_diff(diff);
        }

        result
    }

    /// Executes a transaction. Panics if the transaction is not finalized (fee transaction fails). Does not panic if
    /// the main instructions fails (use execute_expect_success for that).
    pub fn execute_expect_commit(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> ExecuteResult {
        let result = self.try_execute(transaction, proofs).unwrap();
        let diff = result.expect_finalization_success();
        self.commit_diff(diff);

        result
    }

    /// Executes a transaction. Panics if the transaction fails.
    pub fn build_and_execute(&mut self, builder: TransactionBuilder, proofs: Vec<NonFungibleAddress>) -> ExecuteResult {
        let transaction = builder.build_and_seal(&self.secret_key);
        self.execute_expect_commit(transaction, proofs)
    }

    /// Executes a transaction. Panics if the transaction fails.
    pub fn execute_expect_success(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> ExecuteResult {
        let result = self.execute_expect_commit(transaction, proofs);
        result.expect_success();
        result
    }

    /// Executes a transaction. Panics if the transaction succeeds.
    pub fn execute_expect_failure(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> RejectReason {
        let result = self.try_execute(transaction, proofs).unwrap();
        result.expect_failure().clone()
    }

    pub fn execute_and_commit(
        &mut self,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> anyhow::Result<ExecuteResult> {
        self.execute_and_commit_with_fees(vec![], instructions, proofs)
    }

    pub fn execute_and_commit_with_fees(
        &mut self,
        fee_instructions: Vec<Instruction>,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> anyhow::Result<ExecuteResult> {
        let result = self.try_execute_instructions(fee_instructions, instructions, proofs)?;
        let diff = result.finalize.result.accept().ok_or_else(|| {
            anyhow!(
                "Transaction was rejected: {}",
                result.finalize.result.fee_reject().unwrap()
            )
        })?;

        // It is convenient to commit the state back to the staged state store in tests.
        self.commit_diff(diff);

        if let Some(reason) = result.finalize.any_reject() {
            return Err(anyhow!("Transaction failed: {}", reason));
        }

        Ok(result)
    }

    pub fn execute_and_commit_manifest<'a, I: IntoIterator<Item = (&'a str, ManifestValue)>>(
        &mut self,
        manifest: &str,
        variables: I,
        proofs: Vec<NonFungibleAddress>,
    ) -> anyhow::Result<ExecuteResult> {
        let template_imports = self
            .name_to_template
            .iter()
            // Account is implicitly imported.
            .filter(|(name, _)| *name != "Account")
            .map(|(name, addr)| format!("use template_{} as {};", addr, name))
            .collect::<Vec<_>>()
            .join("\n");
        let manifest = format!("{} fn main() {{ {} }}", template_imports, manifest);
        let instructions = parse_manifest(
            &manifest,
            variables.into_iter().map(|(a, b)| (a.to_string(), b)).collect(),
            Default::default(),
        )
        .unwrap();
        self.execute_and_commit(instructions.instructions, proofs)
    }

    pub fn print_state(&self) {
        for (k, v) in self.state_store.iter() {
            eprintln!("[{}]: {:?}", k, v.substate_value());
        }
    }
}
