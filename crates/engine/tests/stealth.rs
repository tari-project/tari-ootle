//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use rand::rngs::OsRng;
use tari_common_types::types::PrivateKey;
use tari_crypto::{commitment::HomomorphicCommitmentFactory, keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine::runtime::{ActionIdent, NativeAction};
use tari_engine_types::{
    crypto::{get_commitment_factory, ElgamalVerifiableBalance, ValueLookupTable},
    resource_container::ResourceError,
    ToByteType,
    UtxoOutput,
};
use tari_ootle_common_types::{crypto::create_key_pair_from_seed, substate_type::SubstateType};
use tari_template_lib::{
    auth::AccessRule,
    models::{ComponentAddress, ResourceAddress, SpendCondition, UtxoAddress, UtxoId},
    prelude::PedersenCommitmentBytes,
    rule,
};
use tari_template_test_tooling::{
    support::{
        assert_error::{assert_access_denied_for_action, assert_reject_reason},
        stealth,
        stealth::{StealthSecretTransferData, NO_INPUTS},
        GenerateValueLookup,
    },
    wallet_crypto::MaskAndValue,
    TemplateTest,
};
use tari_transaction::{args, call_args, Transaction};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/stealth"];
const TEMPLATE_NAME: &str = "StealthFaucet";

fn setup(
    test: &mut TemplateTest,
    transfer_data: &StealthSecretTransferData,
    view_key: Option<&RistrettoPublicKey>,
) -> (ComponentAddress, ResourceAddress) {
    test.enable_auto_add_proofs_from_signers();
    let template_addr = test.get_template_address(TEMPLATE_NAME);
    let initial_supply = transfer_data.statement.inputs_statement.revealed_amount;

    let transaction = Transaction::builder_localnet()
        .call_function(template_addr, "new", args![
            initial_supply,
            transfer_data.statement,
            view_key.map(|vk| vk.to_byte_type())
        ])
        .build_and_seal(test.secret_key());

    test.execute_expect_success(transaction, vec![]);

    let faucet = test.get_previous_output_address(SubstateType::Component);
    let resx = test.get_previous_output_address(SubstateType::Resource);

    (
        faucet.as_component_address().unwrap(),
        resx.as_resource_address().unwrap(),
    )
}

#[test]
fn mint_initial_supply() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);

    let resource = test.read_only_state_store().get_resource(&faucet_resx).unwrap();
    let total_supply = resource.total_supply().unwrap();
    assert_eq!(total_supply, 11100);
}

#[test]
fn mint_more_later() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let mint = stealth::generate_mint_statement([1200], 0, None);
    let (faucet, faucet_resx) = setup(&mut test, &mint, None);

    test.call_method::<()>(faucet, "mint", call_args![11100], vec![]);

    let resource = test.read_only_state_store().get_resource(&faucet_resx).unwrap();
    let total_supply = resource.total_supply().unwrap();
    assert_eq!(total_supply, 12300);
}

#[test]
fn basic_transfer() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);

    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0,
        Some(100),
        0,
    );
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement)
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 1);
    assert!(utxos[0].output().is_some());
}

#[test]
fn programmatic_transfer() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 100, None);
    let (faucet, _faucet_resx) = setup(&mut test, &mint, None);

    let vault_id = test
        .get_previous_output_address(SubstateType::Vault)
        .as_vault_id()
        .unwrap();

    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0,
        Some(75),
        25,
    );
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "programmatic_transfer", args![transfer.statement])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 1);
    assert!(utxos[0].output().is_some());
    let vault = test.read_only_state_store().get_vault(&vault_id).unwrap();
    assert_eq!(vault.balance(), 125);
}

#[test]
fn transfer_with_revealed_outputs() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = [100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);
    let (account, _proof, _sk) = test.create_empty_account();

    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[1].clone(),
            value: 1000,
        }],
        0,
        [100, 200],
        700,
    );
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(account, "deposit", args![Workspace("bucket")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[1])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 2);
    let store = test.read_only_state_store();
    let vaults = store.get_vaults_for_account(account).unwrap();
    let vault = vaults.get(&faucet_resx).unwrap();
    assert_eq!(vault.balance(), 700);
}

#[test]
fn transfer_revealed_between_accounts() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let (alice, alice_proof, alice_sk) = test.create_empty_account();
    let (bob, _proof, _sk) = test.create_empty_account();

    let outputs = [100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);

    let transfer_from_faucet = stealth::generate_transfer_data(
        [
            MaskAndValue {
                mask: mint.output_masks[2].clone(),
                value: 10000,
            },
            MaskAndValue {
                mask: mint.output_masks[1].clone(),
                value: 1000,
            },
        ],
        0,
        [999, 9901],
        100,
    );
    let transfer_from_alice_to_bob = stealth::generate_transfer_data(NO_INPUTS, 100, [25, 25, 25], 25);
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("withdrawn_funds_from_stealth_transfer")
            .call_method(alice, "deposit", args![Workspace(
                "withdrawn_funds_from_stealth_transfer"
            )])
            .call_method(alice, "withdraw", args![faucet_resx, 100])
            .put_last_instruction_output_on_workspace("alice_to_bob")
            .stealth_transfer_with_input_bucket(faucet_resx, transfer_from_alice_to_bob.statement, "alice_to_bob")
            .put_last_instruction_output_on_workspace("transfer_to_bob")
            .call_method(bob, "deposit", args![Workspace("transfer_to_bob")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[1])
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[2])
            .seal(&alice_sk),
        vec![alice_proof],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 5);
    let store = test.read_only_state_store();
    let vaults = store.get_vaults_for_account(alice).unwrap();
    let vault = vaults.get(&faucet_resx).unwrap();
    assert_eq!(vault.balance(), 0);
    let vaults = store.get_vaults_for_account(bob).unwrap();
    let vault = vaults.get(&faucet_resx).unwrap();
    assert_eq!(vault.balance(), 25);
}

#[test]
fn transfer_invalid_balance_in_statement() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);
    let (alice, _proof, _sk) = test.create_empty_account();

    let transfer_from_faucet = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0,
        [99],
        // Try to skim a little (1) off the top
        2,
    );
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(alice, "deposit", args![Workspace("bucket")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidBalanceProof {
        details: "Balance proof signature verification failed".to_string(),
    });
}

#[test]
fn transfer_fails_if_transaction_is_not_signed_by_utxo_owner() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);

    let input = MaskAndValue {
        mask: mint.output_masks[0].clone(),
        value: 100,
    };
    let commitment = input.to_commitment();
    let transfer_from_faucet = stealth::generate_transfer_data([input], 0, [100], 0);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            // Missing signer
            // .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let output_0_pk = RistrettoPublicKey::from_secret_key(&mint.output_masks[0]).to_byte_type();

    assert_reject_reason(reason, ResourceError::RequiredSignatureMissingForStealthUtxo {
        commitment: commitment.to_byte_type(),
        public_key: output_0_pk,
    });
}

#[test]
fn transfer_invalid_range_proof_in_statement() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);
    let (alice, _proof, _sk) = test.create_empty_account();

    let mut transfer_from_faucet = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0,
        [99],
        1,
    );
    let mut rp = transfer_from_faucet
        .statement
        .outputs_statement
        .agg_range_proof
        .clone()
        .into_vec();
    rp[100] ^= 0xFF; // Corrupt the range proof
    transfer_from_faucet.statement.outputs_statement.agg_range_proof = rp.try_into().unwrap();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(alice, "deposit", args![Workspace("bucket")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "Internal range proof(s) error");
}

#[test]
fn many_outputs_in_one_transfer() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    use std::{iter, time::Instant};

    use tari_engine_types::limits;
    let outputs = [1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (_faucet, faucet_resx) = setup(&mut test, &mint, None);

    let timer = Instant::now();

    assert_eq!(
        1000 % limits::STEALTH_LIMITS.max_outputs,
        0,
        "Balance proof will fail due to rounding. Adjust the test amount to be a multiple of the limit"
    );
    let transfer_from_faucet = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 1000,
        }],
        0,
        iter::repeat_n(
            u64::try_from(1000 / limits::STEALTH_LIMITS.max_outputs).unwrap(),
            limits::STEALTH_LIMITS.max_outputs,
        ),
        0,
    );

    // Release mode: ± 23s on M1 Mac, 3.7s on Ryzen 5950x (single thread, total test time 6.1s) for 500 outputs. Current
    // limit is 8 TODO: verification time (depending on hardware) of 2-10+ seconds is still a problem, determine
    // what the upper bound for utxos should be. Parts of the verification could be parallelized (helps, assuming
    // some minimum CPU spec for a VN). Note that generation in Debug mode took 16 minutes on Ryzen 5950x !
    eprintln!("Generated transfer in {:.2?}", timer.elapsed());

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 8);
}

pub fn try_brute_force_stealth_balance<I, TValueLookup>(
    utxos: &BTreeMap<PedersenCommitmentBytes, UtxoOutput>,
    secret_view_key: &PrivateKey,
    value_range: I,
    value_lookup: &mut TValueLookup,
) -> Result<Option<u64>, TValueLookup::Error>
where
    I: IntoIterator<Item = u64> + Clone,
    TValueLookup: ValueLookupTable,
{
    let decompressed_viewable_balances = utxos
        .values()
        .filter_map(|utxo| utxo.output.viewable_balance.as_ref().map(|vb| vb.try_into().unwrap()))
        .collect::<Vec<_>>();

    let balances = ElgamalVerifiableBalance::batched_brute_force(
        secret_view_key,
        value_range,
        value_lookup,
        &decompressed_viewable_balances,
    )?;

    // If any of the commitments cannot be brute forced, then we return None
    Ok(balances.into_iter().sum())
}

#[test]
fn mint_with_view_key() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let (view_key_secret, view_key) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let mint = stealth::generate_mint_statement([1000], 0, Some(&view_key));
    let (_faucet, faucet_resx) = setup(&mut test, &mint, Some(&view_key));

    let withdraw_proof = stealth::generate_transfer_data_with_view_key(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 1000,
        }],
        0,
        [100, 200, 200, 200, 200, 100],
        0,
        &view_key,
    );
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, withdraw_proof.statement)
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.result.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(addr, substate)| {
            addr.as_utxo_address().map(|addr| {
                (
                    addr.id().into_commitment_bytes(),
                    substate.substate_value().as_utxo().unwrap().clone().output.unwrap(),
                )
            })
        })
        .collect();

    let total_balance =
        try_brute_force_stealth_balance(&utxos, &view_key_secret, 0..=200, &mut GenerateValueLookup).unwrap();
    assert_eq!(total_balance, Some(1000));
}

#[test]
fn freeze_then_attempt_spend() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = vec![100u64, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs.clone(), 0, None);
    let (faucet, faucet_resx) = setup(&mut test, &mint, None);

    let transfer = stealth::generate_transfer_data(
        [
            MaskAndValue {
                mask: mint.output_masks[0].clone(),
                value: 100,
            },
            MaskAndValue {
                mask: mint.output_masks[1].clone(),
                value: 1000,
            },
        ],
        0,
        Some(1100),
        0,
    );
    let owner = test.owner_proof();
    let utxos = mint.output_masks
        .iter()
        .zip(outputs)
        .take(2) // Freeze the first two outputs
        .map(|(mask, amount)| {
            let commitment = get_commitment_factory().commit_value(mask, amount);
            UtxoId::from(commitment.to_byte_type())
        })
        .collect::<Vec<_>>();

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "freeze_utxos", args![utxos])
            .build_and_seal(test.secret_key()),
        vec![owner.clone()],
    );

    // Try and spend a frozen output
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement.clone())
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[1])
            .seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidSpend { details: String::new() });

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "unfreeze_utxos", args![utxos])
            .build_and_seal(test.secret_key()),
        vec![owner],
    );

    // Should be able to spend now
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement)
            .finish()
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[0])
            .add_signer(&test.to_public_key_bytes(), &mint.output_masks[1])
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(_, substate)| substate.substate_value().as_utxo())
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 1);
    assert!(utxos[0].output().is_some());
}

#[test]
fn burn_then_attempt_spend() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let outputs = vec![100u64, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs.clone(), 0, None);
    let (faucet, faucet_resx) = setup(&mut test, &mint, None);

    let transfer = stealth::generate_transfer_data(
        [
            MaskAndValue {
                mask: mint.output_masks[0].clone(),
                value: outputs[0],
            },
            MaskAndValue {
                mask: mint.output_masks[1].clone(),
                value: outputs[1],
            },
        ],
        0,
        Some(outputs[0] + outputs[1]),
        0,
    );
    let owner = test.owner_proof();
    let utxos_and_proofs = mint.output_masks
        .iter()
        .zip(outputs)
        .take(2) // Freeze the first two outputs
        .map(|(mask, amount)| {
            let commitment = get_commitment_factory().commit_value(mask, amount);
            let utxo_id = UtxoId::from(commitment.to_byte_type());
            let proof = stealth::generate_value_proof_mask_knowledge(amount.into(), mask);
            (utxo_id, proof)
        })
        .collect::<Vec<_>>();

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "burn_utxos", args![utxos_and_proofs.clone()])
            .build_and_seal(test.secret_key()),
        vec![owner.clone()],
    );

    // Try and spend a burnt outputs
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement.clone())
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidSpend { details: String::new() });
    for (utxo_id, _) in utxos_and_proofs {
        let utxo = test
            .read_only_state_store()
            .get_utxo(UtxoAddress::new(faucet_resx, utxo_id))
            .unwrap();
        assert!(utxo.is_burnt());
    }
}

#[test]
fn transfer_restricted_by_access_rules_n_of_m() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let (_, pk1) = create_key_pair_from_seed(100);
    let pk1 = pk1.to_byte_type();
    let (sk2, pk2) = create_key_pair_from_seed(101);
    let pk2 = pk2.to_byte_type();
    let (sk3, pk3) = create_key_pair_from_seed(102);
    let pk3 = pk3.to_byte_type();
    let (sk4, pk4) = create_key_pair_from_seed(103);
    let pk4 = pk4.to_byte_type();

    // 3-of-4 multisig rule
    let rule = rule!(m_of_n(
        3,
        public_key(pk1),
        public_key(pk2),
        public_key(pk3),
        public_key(pk4)
    ));
    let outputs = vec![(100u64, SpendCondition::AccessRule(rule))];
    let mint = stealth::generate_mint_statement(outputs.clone(), 0, None);
    let (_, faucet_resx) = setup(&mut test, &mint, None);

    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: outputs[0].0,
        }],
        0,
        [10u64, 90u64],
        0,
    );
    let test_pk = test.to_public_key_bytes();

    // First try to spend with only 2 of the required 3 signatures
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement.clone())
            .finish()
            .add_signer(&test_pk, &sk2)
            .add_signer(&test_pk, &sk3)
            .seal(test.secret_key()),
        vec![],
    );

    assert_access_denied_for_action(reason, ActionIdent::Native(NativeAction::StealthUtxoSpend));

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, transfer.statement)
            .finish()
            .add_signer(&test_pk, &sk2)
            .add_signer(&test_pk, &sk3)
            .add_signer(&test_pk, &sk4)
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(id, substate)| {
            let addr = id.as_utxo_address()?;
            let output = substate.substate_value().as_utxo().and_then(|u| u.output())?;
            Some((addr, output))
        })
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 2);
}

#[test]
fn transfer_restricted_by_access_rules_component_scope() {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);

    let outputs = vec![(100u64, SpendCondition::Signed(test.to_public_key_bytes()))];
    let mint = stealth::generate_mint_statement(outputs.clone(), 0, None);
    let (component, faucet_resx) = setup(&mut test, &mint, None);

    let component_scope_rule = rule!(component(component));
    let initial_transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: outputs[0].0,
        }],
        0,
        [
            (10u64, SpendCondition::AccessRule(component_scope_rule.clone())),
            (90u64, SpendCondition::AccessRule(component_scope_rule)),
        ],
        0,
    );

    // Create the new outputs with the component-bound spend condition
    test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(faucet_resx, initial_transfer.statement.clone())
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let transfer = stealth::generate_transfer_data(
        [
            MaskAndValue {
                mask: initial_transfer.output_masks[0].clone(),
                value: 10,
            },
            MaskAndValue {
                mask: initial_transfer.output_masks[1].clone(),
                value: 90,
            },
        ],
        0,
        [
            // Anyone with the mask and value (i.e. view key) can spend!
            (99u64, SpendCondition::AccessRule(AccessRule::AllowAll)),
        ],
        1, // programmatic transfer in this template requires a revealed output amount
    );

    // First try to spend in a template context
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_function(
                test.get_template_address(TEMPLATE_NAME),
                "static_programmatic_transfer",
                args![faucet_resx, transfer.statement.clone()],
            )
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    assert_access_denied_for_action(reason, ActionIdent::Native(NativeAction::StealthUtxoSpend));

    // Then, spend in the component context, which succeeds
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(component, "programmatic_transfer", args![transfer.statement])
            .finish()
            .seal(test.secret_key()),
        vec![],
    );

    let diff = result.finalize.any_accept().unwrap();
    let utxos = diff
        .up_iter()
        .filter_map(|(id, substate)| {
            let addr = id.as_utxo_address()?;
            let output = substate.substate_value().as_utxo().and_then(|u| u.output())?;
            Some((addr, output))
        })
        .collect::<Vec<_>>();
    assert_eq!(utxos.len(), 1);
}
