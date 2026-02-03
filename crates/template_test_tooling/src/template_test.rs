//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsStr,
    iter,
    path::Path,
    sync::Arc,
    time::Instant,
};

use anyhow::anyhow;
use ootle_byte_type::ToByteType;
use serde::de::DeserializeOwned;
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::hex::Hex,
};
use tari_engine::{
    executables::Executable,
    fees::{FeeModule, FeeTable},
    runtime::{AuthParams, RuntimeModule},
    state_store::{
        memory::{MemoryStateStore, ReadOnlyMemoryStateStore},
        StateWriter,
    },
    template::LoadedTemplate,
    transaction::{TransactionError, TransactionProcessor},
    wasm::LoadedWasmTemplate,
};
use tari_engine_types::{
    commit_result::{ExecuteResult, RejectReason},
    indexed_value::IndexedWellKnownTypes,
    substate::{SubstateDiff, SubstateId},
    virtual_substate::{VirtualSubstate, VirtualSubstateId},
};
use tari_ootle_common_types::{crypto::create_key_pair_from_seed, substate_type::SubstateType, SubstateRequirement};
use tari_ootle_transaction::{
    args,
    args::InstructionArg,
    builder::{named_args::BuilderWorkspaceKey, MainIntent},
    Instruction,
    Transaction,
    TransactionBuilder,
};
use tari_template_lib::types::{
    constants::{NFT_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS},
    crypto::RistrettoPublicKeyBytes,
    Amount,
    ComponentAddress,
    NonFungibleAddress,
    ResourceAddress,
    TemplateAddress,
};
use tari_transaction_manifest::{parse_manifest, ManifestValue};

use crate::{
    builtin_component_state::{
        add_tari_resources,
        initialize_builtin_faucet_state,
        initialize_builtin_nft_faucet_state,
    },
    helpers::derive_account_address_from_public_key,
    mocks::AlwaysPassesProofVerifier,
    read_only_state_store::ReadOnlyStateStore,
    template_spec::TemplateSpec,
    track_calls::TrackCallsModule,
    wrapped_transaction::WrappedTransaction,
    Package,
};

pub const fn xtr_faucet_component() -> ComponentAddress {
    XTR_FAUCET_COMPONENT_ADDRESS
}

pub fn test_nft_faucet_component() -> ComponentAddress {
    NFT_FAUCET_COMPONENT_ADDRESS
}

pub struct TemplateTest {
    package: Arc<Package>,
    track_calls: TrackCallsModule,
    secret_key: RistrettoSecretKey,
    public_key: RistrettoPublicKey,
    last_outputs: HashSet<SubstateId>,
    name_to_template: HashMap<String, TemplateAddress>,
    state_store: MemoryStateStore,
    enable_fees: bool,
    fee_table: FeeTable,
    virtual_substates: HashMap<VirtualSubstateId, VirtualSubstate>,
    key_seed: u8,
    auto_add_proofs_from_signers: bool,
}

impl TemplateTest {
    /// The initial balance of a funded account created by `create_funded_account`.
    pub const FUNDED_ACCOUNT_INITIAL_BALANCE: u64 = 1_000_000_000;

    pub fn new<P: AsRef<Path>, I: IntoIterator<Item = T>, T: Into<TemplateSpec>>(
        base_path: P,
        template_paths: I,
    ) -> Self {
        Self::new_internal(base_path, template_paths, iter::empty::<(String, String)>())
    }

    pub fn new_cwd<I: IntoIterator<Item = T>, T: Into<TemplateSpec>>(template_paths: I) -> Self {
        Self::new_internal(
            env::current_dir().expect("cannot get CWD"),
            template_paths,
            None::<(String, String)>,
        )
    }

    pub fn new_no_templates() -> Self {
        Self::new(".", iter::empty::<TemplateSpec>())
    }

    pub fn new_with_compile_envs<P, I, T, TEnvs, K, V>(base_path: P, template_paths: I, envs: TEnvs) -> Self
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = T>,
        T: Into<TemplateSpec>,
        TEnvs: IntoIterator<Item = (K, V)>,
        TEnvs::IntoIter: Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        Self::new_internal(base_path, template_paths, envs)
    }

    fn new_internal<P, I, T, TEnvs, K, V>(base_path: P, templates: I, envs: TEnvs) -> Self
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = T>,
        T: Into<TemplateSpec>,
        TEnvs: IntoIterator<Item = (K, V)>,
        TEnvs::IntoIter: Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let mut builder = Package::builder();

        builder.add_all_builtin_templates();

        // Add the faucet template for non-XTR fungible tokens
        builder.add_template(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/faucet"));

        // Add all of the templates specified in the argument
        let envs_iter = envs.into_iter();
        let base_path = base_path.as_ref();
        for template in templates {
            let spec = template.into();
            builder.add_template_opts(spec.get_path(base_path), &spec.features, envs_iter.clone());
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

        let virtual_substates =
            HashMap::from_iter([(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(0))]);

        Self {
            package: Arc::new(package),
            track_calls: TrackCallsModule::new(),
            public_key,
            secret_key,
            name_to_template,
            last_outputs: HashSet::new(),
            state_store: MemoryStateStore::new(),
            virtual_substates,
            enable_fees: false,
            fee_table: FeeTable {
                per_transaction_weight_cost: 1,
                per_module_call_cost: 1,
                per_byte_storage_cost: 1,
                per_event_cost: 1,
                per_log_cost: 1,
                per_signature_verification_cost: 1,
                per_template_load_cost_unit: 1,
            },
            key_seed: 1,
            auto_add_proofs_from_signers: true,
        }
    }

    pub fn bootstrap_state(&mut self) {
        add_tari_resources(&mut self.state_store).unwrap();
        initialize_builtin_faucet_state(&mut self.state_store, &self.public_key);
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
        self.package = Arc::new(builder.build());
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

    pub fn enable_auto_add_proofs_from_signers(&mut self) -> &mut Self {
        self.auto_add_proofs_from_signers = true;
        self
    }

    pub fn disable_auto_add_proofs_from_signers(&mut self) -> &mut Self {
        self.auto_add_proofs_from_signers = false;
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

    #[track_caller]
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

        eprintln!("State changes:");
        for (address, _) in diff.down_iter() {
            eprintln!("DOWN substate: {}", address);
            self.state_store.delete_state(address);
        }

        for (address, substate) in diff.up_iter() {
            eprintln!("UP substate: {}", address);
            self.last_outputs.insert(address.clone());
            self.state_store.set_state(address.clone(), substate.clone()).unwrap();
        }
        eprintln!();
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

    #[track_caller]
    pub fn create_account(
        &mut self,
        owner_public_key: RistrettoPublicKeyBytes,
        workspace_id: Option<BuilderWorkspaceKey>,
        proofs: Vec<NonFungibleAddress>,
    ) -> ComponentAddress {
        let result = self
            .build_and_execute(
                Transaction::builder_localnet().create_account_custom(owner_public_key, None, None, workspace_id),
                proofs,
            )
            .unwrap_success();
        let diff = result.finalize.accept().expect("create account failed");
        let component = diff.up_iter().find_map(|(id, _)| id.as_component_address()).unwrap();
        component
    }

    #[track_caller]
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
                    function: func_name.to_string().try_into().unwrap(),
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

    #[track_caller]
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
                    method: method_name.try_into().unwrap(),
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
        (self.owner_proof(), self.secret_key.clone())
    }

    pub fn owner_proof(&self) -> NonFungibleAddress {
        NonFungibleAddress::from_public_key(self.public_key.to_byte_type())
    }

    pub fn secret_key(&self) -> &RistrettoSecretKey {
        &self.secret_key
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    pub fn to_public_key_bytes(&self) -> RistrettoPublicKeyBytes {
        self.public_key.to_byte_type()
    }

    #[track_caller]
    pub fn create_empty_account(&mut self) -> (ComponentAddress, NonFungibleAddress, RistrettoSecretKey) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        let component = self.create_account(public_key.to_byte_type(), None, vec![owner_proof.clone()]);
        self.enable_fees = old_fail_fees;
        (component, owner_proof, secret_key)
    }

    #[track_caller]
    pub fn create_funded_account(&mut self) -> (ComponentAddress, NonFungibleAddress, RistrettoSecretKey) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        self.execute_expect_success(
            Transaction::builder_localnet()
                .call_method(xtr_faucet_component(), "take", args![
                    Self::FUNDED_ACCOUNT_INITIAL_BALANCE
                ])
                .put_last_instruction_output_on_workspace("bucket")
                .create_account_with_bucket(public_key.to_byte_type(), "bucket")
                .build_and_seal(&secret_key),
            vec![owner_proof.clone()],
        );

        let account_address = derive_account_address_from_public_key(&public_key.to_byte_type());

        self.enable_fees = old_fail_fees;
        (account_address, owner_proof, secret_key)
    }

    #[track_caller]
    pub fn create_custom_funded_account<A: Into<Amount>>(
        &mut self,
        amount: A,
    ) -> (
        ComponentAddress,
        NonFungibleAddress,
        RistrettoSecretKey,
        RistrettoPublicKey,
    ) {
        let (owner_proof, public_key, secret_key) = self.create_owner_proof();
        let old_fail_fees = self.enable_fees;
        self.enable_fees = false;
        let public_key_bytes = public_key.to_byte_type();
        self.execute_expect_success(
            Transaction::builder_localnet()
                .call_method(xtr_faucet_component(), "take", args![amount.into()])
                .put_last_instruction_output_on_workspace("bucket")
                .create_account_with_bucket(public_key_bytes, "bucket")
                .build_and_seal(&secret_key),
            vec![owner_proof.clone()],
        );

        let component = derive_account_address_from_public_key(&public_key_bytes);

        self.enable_fees = old_fail_fees;
        (component, owner_proof, secret_key, public_key)
    }

    #[track_caller]
    pub fn create_test_faucet_component<A: Into<Amount>>(
        &mut self,
        initial_supply: A,
    ) -> (ComponentAddress, ResourceAddress) {
        let template_addr = self.get_template_address("TestFaucet");
        let result = self.execute_expect_success(
            Transaction::builder_localnet()
                .call_function(template_addr, "mint", args![initial_supply.into()])
                .build_and_seal(&self.secret_key),
            vec![],
        );

        let (addr, component) = result
            .expect_success()
            .up_iter()
            .filter_map(|(id, substate)| {
                id.as_component_address().and_then(|addr| {
                    let component = substate.substate_value().as_component()?;
                    if component.template_address == template_addr {
                        Some((addr, component.clone()))
                    } else {
                        None
                    }
                })
            })
            .next()
            .expect("No component address found in faucet creation result");

        let indexed = IndexedWellKnownTypes::from_value(component.state()).unwrap();
        let vault_id = indexed
            .vault_ids()
            .first()
            .expect("No vault id found in faucet component state");
        let vault = self
            .read_only_state_store()
            .get_vault(vault_id)
            .expect("No vault id found in faucet component state");
        (addr, *vault.resource_address())
    }

    fn next_key_seed(&mut self) -> u8 {
        let seed = self.key_seed;
        self.key_seed += 1;
        seed
    }

    #[track_caller]
    pub fn create_owner_proof(&mut self) -> (NonFungibleAddress, RistrettoPublicKey, RistrettoSecretKey) {
        let (secret_key, public_key) = create_key_pair_from_seed(self.next_key_seed());
        let owner_token = NonFungibleAddress::from_public_key(public_key.to_byte_type());
        (owner_token, public_key, secret_key)
    }

    #[track_caller]
    pub fn try_execute_instructions(
        &mut self,
        fee_instructions: Vec<Instruction>,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> Result<ExecuteResult, TransactionError> {
        let transaction = Transaction::builder_localnet()
            .with_fee_instructions(fee_instructions)
            .with_instructions(instructions)
            .build_and_seal(&self.secret_key);

        self.try_execute(transaction, proofs)
    }

    #[track_caller]
    pub fn try_execute(
        &mut self,
        transaction: Transaction,
        mut proofs: Vec<NonFungibleAddress>,
    ) -> Result<ExecuteResult, TransactionError> {
        let mut modules: Vec<Box<dyn RuntimeModule<ReadOnlyMemoryStateStore>>> = Vec::with_capacity(2);

        modules.push(Box::new(self.track_calls.clone()));

        if self.enable_fees {
            modules.push(Box::new(FeeModule::new(0, self.fee_table.clone())));
        }

        if self.auto_add_proofs_from_signers {
            proofs.extend(
                transaction
                    .signers_iter()
                    .map(|pk| NonFungibleAddress::from_public_key(*pk)),
            );
        }

        let auth_params = AuthParams {
            initial_ownership_proofs: Arc::new(proofs.into_iter().collect()),
        };

        let processor = TransactionProcessor::new(
            self.package.clone(),
            self.state_store.clone().into_read_only(),
            auth_params,
            self.virtual_substates.clone().into(),
            Arc::from(modules.into_boxed_slice()),
            Arc::new(AlwaysPassesProofVerifier),
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
            for (source, amount) in fee.fee_breakdown().iter() {
                eprintln!("- {:?} {}", source, amount);
            }
        }

        let timer = Instant::now();
        eprintln!("Finished Transaction \"{}\" in {:.2?}", tx_id, timer.elapsed());
        eprintln!();

        Ok(result)
    }

    #[track_caller]
    pub fn execute_and_commit_on_success(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> ExecuteResult {
        let result = self.try_execute(transaction, proofs).unwrap();
        if let Some(diff) = result.finalize.result.any_accept() {
            self.commit_diff(diff);
        }

        result
    }

    /// Executes a transaction. Panics if the transaction is not finalized (fee transaction fails). Does not panic if
    /// the main instructions fails (use execute_expect_success for that).
    #[track_caller]
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
    pub fn build_and_execute(
        &mut self,
        builder: TransactionBuilder<MainIntent>,
        proofs: Vec<NonFungibleAddress>,
    ) -> ExecuteResult {
        let transaction = builder.build_and_seal(&self.secret_key);
        self.execute_expect_commit(transaction, proofs)
    }

    /// Executes a transaction. Panics if the transaction fails.
    #[track_caller]
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
    #[track_caller]
    pub fn execute_expect_failure(
        &mut self,
        transaction: Transaction,
        proofs: Vec<NonFungibleAddress>,
    ) -> RejectReason {
        let result = self.try_execute(transaction, proofs).unwrap();
        result.expect_failure().clone()
    }

    #[track_caller]
    pub fn execute_and_commit(
        &mut self,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> anyhow::Result<ExecuteResult> {
        self.execute_and_commit_with_fees(vec![], instructions, proofs)
    }

    #[track_caller]
    pub fn execute_and_commit_with_fees(
        &mut self,
        fee_instructions: Vec<Instruction>,
        instructions: Vec<Instruction>,
        proofs: Vec<NonFungibleAddress>,
    ) -> anyhow::Result<ExecuteResult> {
        let result = self.try_execute_instructions(fee_instructions, instructions, proofs)?;
        let diff = result.finalize.result.any_accept().ok_or_else(|| {
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

    #[track_caller]
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
        )?;
        self.execute_and_commit(instructions.instructions, proofs)
    }

    pub fn print_state(&self) {
        for (k, v) in self.state_store.iter() {
            eprintln!("[{}]: {:?}", k, v.substate_value());
        }
    }
}
