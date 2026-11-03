//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use ootle_byte_type::ToByteType;
use tari_crypto::commitment::HomomorphicCommitmentFactory;
use tari_engine_types::{
    confidential_output::ConfidentialOutput,
    crypto::{OutputBody, get_commitment_factory},
    substate::{Substate, SubstateDiff, SubstateId},
};
use tari_ootle_common_types::SubstateVersion;
use tari_ootle_wallet_sdk::{
    models::{BalanceChangeSource, ConfidentialOutputModel, KeyBranch, KeyId, OutputStatus},
    storage::{ReadableWalletStore, WalletStoreReader},
};
use tari_template_lib::types::{ConfidentialOutputAddress, EncryptedData, constants::STEALTH_TARI_RESOURCE_ADDRESS};

use crate::support::Test;

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

#[tokio::test]
async fn confidential_transfer_releases_lock_on_dry_run_and_keeps_timed_unlinked_lock_until_submission() {
    use tari_engine_types::resource::Resource;
    use tari_ootle_common_types::optional::IsNotFoundError;
    use tari_ootle_wallet_sdk::apis::confidential_transfer::{ConfidentialTransferParams, UtxoInputSelection};
    use tari_template_lib::types::{
        Amount,
        Metadata,
        ResourceAddress,
        ResourceType,
        SubstateOwnerRule,
        VaultId,
        access_rules::ResourceAccessRules,
        constants::TOKEN_SYMBOL,
    };

    let test = Test::new();
    let sdk = test.sdk();
    let account = sdk
        .accounts_api()
        .get_account_by_address(&Test::test_account_address())
        .unwrap();
    let destination_address = account.address().clone();

    let conf_resource: ResourceAddress = "resource_0000000000000000000000000000000000000000000000000000000000000c0f"
        .parse()
        .unwrap();
    let conf_vault: VaultId = "vault_0000000000000000000000000000000000000000000000000000000000000c0f"
        .parse()
        .unwrap();
    sdk.resources_api()
        .upsert_resource(
            &conf_resource,
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
            Test::test_account_address(),
            conf_vault,
            SubstateVersion::ZERO,
            conf_resource,
            ResourceType::Confidential,
            Some("CONF".to_string()),
            6,
        )
        .unwrap();
    sdk.accounts_api()
        .update_vault_balance_and_record_change(
            conf_vault,
            SubstateVersion::ZERO,
            Amount::from(1_000u64),
            Amount::zero(),
            BalanceChangeSource::Scan,
        )
        .unwrap();
    sdk.substate_api()
        .save_root(
            tari_ootle_common_types::VersionedSubstateIdRef::new(
                &SubstateId::Component(Test::test_account_address()),
                SubstateVersion::ZERO,
            ),
            [SubstateId::Vault(conf_vault)],
        )
        .unwrap();
    sdk.substate_api()
        .save_root(
            tari_ootle_common_types::VersionedSubstateIdRef::new(&SubstateId::Vault(conf_vault), SubstateVersion::ZERO),
            [SubstateId::Resource(conf_resource)],
        )
        .unwrap();

    let params = |is_dry_run| ConfidentialTransferParams {
        from_account: Test::test_account_address(),
        input_selection: UtxoInputSelection::RevealedOnly,
        amount: Amount::from(250u64),
        destination_address: destination_address.clone(),
        resource_address: conf_resource,
        max_fee: 100,
        output_to_revealed: true,
        proof_from_resource: None,
        memo: None,
        max_epoch: tari_ootle_common_types::Epoch(10),
        is_dry_run,
    };

    // Dry run releases the lock immediately before returning.
    let dry_run = sdk.confidential_transfer_api().transfer(params(true)).await.unwrap();
    let released = test
        .store()
        .with_read_tx(|tx| tx.locks_get(dry_run.transaction_proof_id))
        .unwrap_err();
    assert!(released.is_not_found_error(), "{released}");

    // Real transfer keeps a timed lock that is not yet linked to a transaction until submission.
    let real = sdk.confidential_transfer_api().transfer(params(false)).await.unwrap();
    let held = test
        .store()
        .with_read_tx(|tx| tx.locks_get(real.transaction_proof_id))
        .unwrap();
    assert!(held.timeout_at.is_some(), "expected a timed lock");
    assert_eq!(held.transaction_id, None, "lock must not be linked before submission");
}
