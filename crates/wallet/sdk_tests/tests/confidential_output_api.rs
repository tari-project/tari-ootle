//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use ootle_byte_type::ToByteType;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    keys::PublicKey as _,
    ristretto::RistrettoPublicKey,
};
use tari_engine_types::{
    confidential_output::ConfidentialOutput,
    crypto::{OutputBody, commit_amount, get_commitment_factory},
    resource::Resource,
    substate::{Substate, SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{Epoch, SubstateVersion, VersionedSubstateIdRef};
use tari_ootle_wallet_sdk::{
    apis::confidential_transfer::{
        ConfidentialTransferApiError,
        ConfidentialTransferParams,
        UtxoInputSelection,
    },
    models::{AccountWithAddress, ConfidentialOutputModel, KeyBranch, KeyId, OutputStatus},
    storage::{ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_template_lib::types::{
    ConfidentialOutputAddress,
    EncryptedData,
    Metadata,
    ObjectKey,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    VaultId,
    access_rules::ResourceAccessRules,
    constants::{STEALTH_TARI_RESOURCE_ADDRESS, TOKEN_SYMBOL},
    crypto::PedersenCommitmentBytes,
};

use crate::support::{PanicNetworkInterface, Test, random_key, random_keypair};

#[test]
fn outputs_locked_and_released() {
    let test = Test::new();

    let commitment_25 = test.add_unspent_output(25u64);
    let commitment_49 = test.add_unspent_output(49u64);
    let _commitment_100 = test.add_unspent_output(100u64);

    let lock_id = test.new_lock().keep_locked();
    let (inputs, total_value) = test
        .sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 50u64)
        .unwrap();
    assert_eq!(total_value, 74);
    assert_eq!(inputs.len(), 2);

    let locked = test
        .store()
        .with_read_tx(|tx| tx.confidential_outputs_get_locked_by_lock_id(lock_id))
        .unwrap();

    assert!(locked.iter().any(|l| l.commitment == commitment_25));
    assert!(locked.iter().any(|l| l.commitment == commitment_49));
    assert_eq!(locked.len(), 2);

    test.sdk().locks_api().release_lock(lock_id).unwrap();

    let locked = test
        .store()
        .with_read_tx(|tx| tx.confidential_outputs_get_locked_by_lock_id(lock_id))
        .unwrap();
    assert_eq!(locked.len(), 0);
}

#[test]
fn outputs_locked_and_finalized() {
    let test = Test::new();

    let commitment_25 = test.add_unspent_output(25u64);
    let commitment_49 = test.add_unspent_output(49u64);
    let commitment_100 = test.add_unspent_output(100u64);

    let outputs_api = test.sdk().confidential_outputs_api();
    let locks_api = test.sdk().locks_api();
    let lock_id = test.new_lock().keep_locked();

    let (inputs, total_value) = outputs_api
        .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 50u64)
        .unwrap();
    assert_eq!(total_value, 74);
    assert_eq!(inputs.len(), 2);

    let locked = test
        .store()
        .with_read_tx(|tx| tx.confidential_outputs_get_locked_by_lock_id(lock_id))
        .unwrap();

    assert!(locked.iter().any(|l| l.commitment == commitment_25));
    assert!(locked.iter().any(|l| l.commitment == commitment_49));
    assert_eq!(locked.len(), 2);

    // Add a change output belonging to this proof
    let commitment_change = get_commitment_factory()
        .commit_value(&Default::default(), 24)
        .to_byte_type();
    outputs_api
        .add_output(ConfidentialOutputModel {
            account_address: Test::test_account_address(),
            vault_id: Test::test_vault_address(),
            commitment: commitment_change,
            value: 24u64.into(),
            sender_public_nonce: None,
            view_only_key_id: KeyId::derived(KeyBranch::ViewOnlyKey, 0),
            owner_key_id: Some(KeyId::derived(KeyBranch::Account, 0)),
            encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
            public_asset_tag: None,
            memo: None,
            status: OutputStatus::LockedUnconfirmed,
            lock_id: Some(lock_id),
        })
        .unwrap();

    let balance = test.get_unspent_balance();
    assert_eq!(balance, 100);

    let output_id = |commitment| {
        SubstateId::ConfidentialOutput(ConfidentialOutputAddress::new(
            STEALTH_TARI_RESOURCE_ADDRESS,
            commitment,
        ))
    };
    let mut diff = SubstateDiff::new();
    diff.down(output_id(commitment_25), SubstateVersion::ZERO);
    diff.down(output_id(commitment_49), SubstateVersion::ZERO);
    diff.up(
        output_id(commitment_change),
        Substate::new(SubstateVersion::ZERO, ConfidentialOutput {
            output: OutputBody {
                public_nonce: Default::default(),
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                minimum_value_promise: 0,
                viewable_balance: None,
            },
            is_frozen: false,
        }),
    );
    locks_api.finalize_lock(lock_id, &diff).unwrap();

    {
        let mut tx = test.store().create_read_tx().unwrap();
        let locked = tx.confidential_outputs_get_locked_by_lock_id(lock_id).unwrap();
        assert_eq!(locked.len(), 0);

        let unspent = tx
            .confidential_outputs_get_by_account_and_status(&Test::test_account_address(), OutputStatus::Unspent)
            .unwrap();
        assert!(unspent.iter().any(|l| l.commitment == commitment_change));
        assert!(unspent.iter().any(|l| l.commitment == commitment_100));
        assert_eq!(unspent.len(), 2);
        let balance = tx
            .confidential_outputs_get_unspent_balance(&Test::test_vault_address())
            .unwrap();
        assert_eq!(balance, 124);
    }
}

const CONF_RESOURCE: ResourceAddress = ResourceAddress::new(ObjectKey::from_array([7u8; ObjectKey::LENGTH]));
const CONF_VAULT: VaultId = VaultId::new(ObjectKey::from_array([8u8; ObjectKey::LENGTH]));

fn wallet_with_confidential_vault(revealed_balance: u64) -> (Test, AccountWithAddress) {
    let test = Test::without_accounts(PanicNetworkInterface);
    let sdk = test.sdk();
    let address = sdk.key_manager_api().next_account_address().unwrap();
    let account = sdk.accounts_api().create_account(None, true, address).unwrap();
    sdk.resources_api()
        .upsert_resource(
            &CONF_RESOURCE,
            &Resource::new(
                ResourceType::Confidential,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                Metadata::from([(TOKEN_SYMBOL, "CONF".to_string())]),
                None,
                None,
                6,
                false,
            ),
        )
        .unwrap();
    sdk.accounts_api()
        .add_vault(
            *account.component_address(),
            CONF_VAULT,
            SubstateVersion::ZERO,
            CONF_RESOURCE,
            ResourceType::Confidential,
            Some("CONF".to_string()),
            6,
        )
        .unwrap();
    test.store()
        .with_write_tx(|tx| tx.vaults_update(CONF_VAULT, SubstateVersion::ZERO, revealed_balance.into(), 0u64.into()))
        .unwrap();
    let account_substate = SubstateId::Component(*account.component_address());
    let vault_substate = SubstateId::Vault(CONF_VAULT);
    sdk.substate_api()
        .save_root(
            VersionedSubstateIdRef::new(&account_substate, SubstateVersion::ZERO),
            [],
        )
        .unwrap();
    sdk.substate_api()
        .save_root(VersionedSubstateIdRef::new(&vault_substate, SubstateVersion::ZERO), [])
        .unwrap();
    (test, account)
}

fn add_confidential_output(test: &Test, account: &AccountWithAddress, amount: u64) -> PedersenCommitmentBytes {
    let sdk = test.sdk();
    let view_key = sdk.key_manager_api().get_key(account.view_only_key_id()).unwrap();
    let view_public_key = RistrettoPublicKey::from_secret_key(&view_key.secret);
    let (nonce_secret, public_nonce) = random_keypair();
    let mask = random_key();
    let commitment = commit_amount(&mask, amount.into()).unwrap().to_byte_type();
    let encrypted_data = sdk
        .confidential_crypto_api()
        .encrypt_value_and_mask(amount, &mask, &view_public_key, &nonce_secret, None)
        .unwrap();
    sdk.confidential_outputs_api()
        .add_output(ConfidentialOutputModel {
            account_address: *account.component_address(),
            vault_id: CONF_VAULT,
            commitment,
            value: amount.into(),
            sender_public_nonce: Some(public_nonce.to_byte_type()),
            view_only_key_id: account.view_only_key_id(),
            owner_key_id: account.owner_key_id(),
            encrypted_data,
            public_asset_tag: None,
            memo: None,
            status: OutputStatus::Unspent,
            lock_id: None,
        })
        .unwrap();
    commitment
}

#[tokio::test]
async fn dry_run_transfer_releases_locks_and_skips_unconfirmed_change() {
    let (test, account) = wallet_with_confidential_vault(20);
    let commitment_40 = add_confidential_output(&test, &account, 40);

    // `PreferRevealed` for 50 spends the 20 revealed balance plus the 40 confidential output (producing 10 change).
    let output = test
        .sdk()
        .confidential_transfer_api()
        .transfer(ConfidentialTransferParams {
            from_account: *account.component_address(),
            input_selection: UtxoInputSelection::PreferRevealed,
            amount: 50u64.into(),
            destination_address: account.address().clone(),
            resource_address: CONF_RESOURCE,
            max_fee: 100,
            output_to_revealed: false,
            proof_from_resource: None,
            memo: None,
            max_epoch: Epoch(10),
            is_dry_run: true,
        })
        .await
        .unwrap();

    let mut tx = test.store().create_read_tx().unwrap();
    assert!(
        tx.confidential_outputs_get_locked_by_lock_id(output.transaction_proof_id)
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.confidential_outputs_get_by_account_and_status(account.component_address(), OutputStatus::LockedForSpend)
            .unwrap()
            .is_empty()
    );
    assert!(
        tx.confidential_outputs_get_by_account_and_status(account.component_address(), OutputStatus::LockedUnconfirmed)
            .unwrap()
            .is_empty()
    );
    let unspent = tx
        .confidential_outputs_get_by_account_and_status(account.component_address(), OutputStatus::Unspent)
        .unwrap();
    assert_eq!(unspent.len(), 1);
    assert_eq!(unspent[0].commitment, commitment_40);
    drop(tx);

    let vault = test
        .sdk()
        .accounts_api()
        .get_vault_by_resource(account.component_address(), &CONF_RESOURCE)
        .unwrap();
    assert!(vault.locked_revealed_balance.is_zero());
    assert_eq!(vault.available_revealed_balance(), 20u64);
}

#[tokio::test]
async fn prefer_confidential_respects_locked_revealed_balance() {
    let (test, account) = wallet_with_confidential_vault(100);

    let first_lock = test.new_lock().keep_locked();
    test.sdk()
        .locks_api()
        .lock_funds_in_vault(first_lock, &CONF_VAULT, 60u64)
        .unwrap();

    let err = test
        .sdk()
        .confidential_transfer_api()
        .transfer(ConfidentialTransferParams {
            from_account: *account.component_address(),
            input_selection: UtxoInputSelection::PreferConfidential,
            amount: 50u64.into(),
            destination_address: account.address().clone(),
            resource_address: CONF_RESOURCE,
            max_fee: 100,
            output_to_revealed: false,
            proof_from_resource: None,
            memo: None,
            max_epoch: Epoch(10),
            is_dry_run: true,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ConfidentialTransferApiError::InsufficientFunds));
}
