//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use rand::rngs::OsRng;
use tari_common_types::types::PrivateKey;
use tari_crypto::{commitment::HomomorphicCommitmentFactory, keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::{
    crypto::{get_commitment_factory, ElgamalVerifiableBalance, ValueLookupTable},
    resource_container::ResourceError,
    ToByteType,
    UtxoOutput,
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, UtxoId},
    prelude::{PedersenCommitmentBytes, SchnorrSignatureBytes},
};
use tari_template_test_tooling::{
    support::{
        assert_error::assert_reject_reason,
        stealth,
        stealth::StealthUnblindedTransferData,
        AlwaysMissLookupTable,
    },
    wallet_crypto::MaskAndValue,
    TemplateTest,
};
use tari_transaction::{args, call_args, Transaction};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/stealth"];
const TEMPLATE_NAME: &str = "StealthFaucet";

fn setup(
    transfer_data: &StealthUnblindedTransferData,
    view_key: Option<&RistrettoPublicKey>,
) -> (TemplateTest, ComponentAddress, ResourceAddress) {
    let mut test = TemplateTest::new(TEMPLATE_PATHS);
    let template_addr = test.get_template_address(TEMPLATE_NAME);
    let initial_supply = transfer_data.statement.inputs_statement.revealed_amount;

    let transaction = Transaction::builder()
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
        test,
        faucet.as_component_address().unwrap(),
        resx.as_resource_address().unwrap(),
    )
}

#[test]
fn mint_initial_supply() {
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (test, _faucet, faucet_resx) = setup(&mint, None);

    let resource = test.read_only_state_store().get_resource(&faucet_resx).unwrap();
    let total_supply = resource.total_supply().unwrap();
    assert_eq!(total_supply, 11100);
}

#[test]
fn mint_more_later() {
    let mint = stealth::generate_mint_statement([1200], 0, None);
    let (mut test, faucet, faucet_resx) = setup(&mint, None);

    test.call_method::<()>(faucet, "mint", call_args![11100], vec![]);

    let resource = test.read_only_state_store().get_resource(&faucet_resx).unwrap();
    let total_supply = resource.total_supply().unwrap();
    assert_eq!(total_supply, 12300);
}

#[test]
fn basic_transfer() {
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);

    let transfer = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100.into(),
        }],
        0,
        Some(100),
        0,
    );
    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer.statement)
            .build_and_seal(test.secret_key()),
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
    let outputs = vec![100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 100, None);
    let (mut test, faucet, _faucet_resx) = setup(&mint, None);

    let vault_id = test
        .get_previous_output_address(SubstateType::Vault)
        .as_vault_id()
        .unwrap();

    let transfer = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100.into(),
        }],
        0,
        Some(75),
        25,
    );
    let result = test.execute_expect_success(
        Transaction::builder()
            .call_method(faucet, "programmatic_transfer", args![transfer.statement])
            .build_and_seal(test.secret_key()),
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
    let outputs = [100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);
    let (account, _proof, _sk) = test.create_empty_account();

    let transfer = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[1].clone(),
            value: 1000.into(),
        }],
        0,
        [100, 200],
        700,
    );
    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(account, "deposit", args![Workspace("bucket")])
            .build_and_seal(test.secret_key()),
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
    let outputs = [100, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);
    let (alice, alice_proof, alice_sk) = test.create_empty_account();
    let (bob, _proof, _sk) = test.create_empty_account();

    let transfer_from_faucet = stealth::generate_transfer_data(
        &[
            MaskAndValue {
                mask: mint.output_masks[2].clone(),
                value: 10000.into(),
            },
            MaskAndValue {
                mask: mint.output_masks[1].clone(),
                value: 1000.into(),
            },
        ],
        0,
        [999, 9901],
        100,
    );
    let transfer_from_alice_to_bob = stealth::generate_transfer_data(&[], 100, [25, 25, 25], 25);
    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(alice, "deposit", args![Workspace("bucket")])
            .call_method(alice, "withdraw", args![faucet_resx, 100])
            .put_last_instruction_output_on_workspace("alice_to_bob")
            .stealth_transfer_with_input_bucket(faucet_resx, transfer_from_alice_to_bob.statement, "alice_to_bob")
            .put_last_instruction_output_on_workspace("transfer_to_bob")
            .call_method(bob, "deposit", args![Workspace("transfer_to_bob")])
            .build_and_seal(&alice_sk),
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
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);
    let (alice, _proof, _sk) = test.create_empty_account();

    let transfer_from_faucet = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100.into(),
        }],
        0,
        [99],
        // Try to skim a little (1) off the top
        2,
    );
    let reason = test.execute_expect_failure(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(alice, "deposit", args![Workspace("bucket")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidBalanceProof {
        details: "Balance proof signature verification failed".to_string(),
    });
}

#[test]
fn transfer_invalid_ownership_proof() {
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);

    let mut transfer_from_faucet = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100.into(),
        }],
        0,
        [99],
        0,
    );
    // Set an invalid ownership proof
    transfer_from_faucet.statement.inputs_statement.inputs[0].owner_proof = SchnorrSignatureBytes::zero();

    let reason = test.execute_expect_failure(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidSpend {
        details: "Invalid ownership proof for input with commitment".to_string(),
    });
}

#[test]
fn transfer_invalid_range_proof_in_statement() {
    let outputs = [100, 1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);
    let (alice, _proof, _sk) = test.create_empty_account();

    let mut transfer_from_faucet = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100.into(),
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
    rp[100] = rp[100].wrapping_add(1); // Corrupt the range proof
    transfer_from_faucet.statement.outputs_statement.agg_range_proof = rp.try_into().unwrap();

    let reason = test.execute_expect_failure(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(alice, "deposit", args![Workspace("bucket")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "Invalid range proof");
}

#[test]
fn many_outputs_in_one_transfer() {
    use std::{iter, time::Instant};

    use tari_engine_types::limits;
    let outputs = [1000];
    let mint = stealth::generate_mint_statement(outputs, 0, None);
    let (mut test, _faucet, faucet_resx) = setup(&mint, None);

    let timer = Instant::now();

    assert_eq!(
        1000 % limits::STEALTH_LIMITS.max_outputs,
        0,
        "Balance proof will fail due to rounding. Adjust the test amount to be a multiple of the limit"
    );
    let transfer_from_faucet = stealth::generate_transfer_data(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 1000.into(),
        }],
        0,
        iter::repeat_n(
            1000 / limits::STEALTH_LIMITS.max_outputs,
            limits::STEALTH_LIMITS.max_outputs,
        ),
        0,
    );

    // Release mode: ± 23s on M1 Mac, 3.7s on Ryzen 5950x (single thread, total test time 6.1s) for 500 outputs - actual
    // limit is 8
    // TODO: verification time (depending on hardware) of 2-10+ seconds is still a problem, determine what
    // the upper bound for utxos should be. Parts of the verification could be parallelized (helps, assuming some
    // minimum CPU spec for a VN). Note that generation in Debug mode took 16 minutes on Ryzen 5950x !
    eprintln!("Generated transfer in {:.2?}", timer.elapsed());

    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer_from_faucet.statement)
            .build_and_seal(test.secret_key()),
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
    let (view_key_secret, view_key) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let mint = stealth::generate_mint_statement([1000], 0, Some(&view_key));
    let (mut test, _faucet, faucet_resx) = setup(&mint, Some(&view_key));

    let withdraw_proof = stealth::generate_transfer_data_with_view_key(
        &[MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 1000.into(),
        }],
        0,
        [100, 200, 200, 200, 200, 100],
        0,
        &view_key,
    );
    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, withdraw_proof.statement)
            .build_and_seal(test.secret_key()),
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
        try_brute_force_stealth_balance(&utxos, &view_key_secret, 0..=200, &mut AlwaysMissLookupTable).unwrap();
    assert_eq!(total_balance, Some(1000));
}

#[test]
fn freeze_then_attempt_spend() {
    let outputs = vec![100u64, 1000, 10000];
    let mint = stealth::generate_mint_statement(outputs.clone(), 0, None);
    let (mut test, faucet, faucet_resx) = setup(&mint, None);

    let transfer = stealth::generate_transfer_data(
        &[
            MaskAndValue {
                mask: mint.output_masks[0].clone(),
                value: 100.into(),
            },
            MaskAndValue {
                mask: mint.output_masks[1].clone(),
                value: 1000.into(),
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
        Transaction::builder()
            .call_method(faucet, "freeze_utxos", args![utxos])
            .build_and_seal(test.secret_key()),
        vec![owner.clone()],
    );

    // Try and spend a frozen output
    let reason = test.execute_expect_failure(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer.statement.clone())
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidSpend { details: String::new() });

    test.execute_expect_success(
        Transaction::builder()
            .call_method(faucet, "unfreeze_utxos", args![utxos])
            .build_and_seal(test.secret_key()),
        vec![owner],
    );

    // Should be able to spend now
    let result = test.execute_expect_success(
        Transaction::builder()
            .stealth_transfer(faucet_resx, transfer.statement)
            .build_and_seal(test.secret_key()),
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
