//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use tari_crypto::commitment::HomomorphicCommitmentFactory;
use tari_engine_types::{crypto::get_commitment_factory, ToByteType};
use tari_ootle_wallet_sdk::{
    models::{ConfidentialOutputModel, KeyId, OutputStatus},
    storage::{WalletStore, WalletStoreReader},
};
use tari_template_lib::models::EncryptedData;

use crate::support::Test;

#[test]
fn outputs_locked_and_released() {
    let test = Test::new();

    let commitment_25 = test.add_unspent_output(25);
    let commitment_49 = test.add_unspent_output(49);
    let _commitment_100 = test.add_unspent_output(100);

    let lock_id = test.new_lock();
    let (inputs, total_value) = test
        .sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 50)
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

    test.sdk()
        .confidential_outputs_api()
        .release_locked_outputs(lock_id)
        .unwrap();

    let locked = test
        .store()
        .with_read_tx(|tx| tx.confidential_outputs_get_locked_by_lock_id(lock_id))
        .unwrap();
    assert_eq!(locked.len(), 0);
}

#[test]
fn outputs_locked_and_finalized() {
    let test = Test::new();

    let commitment_25 = test.add_unspent_output(25);
    let commitment_49 = test.add_unspent_output(49);
    let commitment_100 = test.add_unspent_output(100);

    let outputs_api = test.sdk().confidential_outputs_api();
    let proof_id = test.new_lock();

    let (inputs, total_value) = outputs_api
        .lock_outputs_by_amount(proof_id, &Test::test_vault_address(), 50)
        .unwrap();
    assert_eq!(total_value, 74);
    assert_eq!(inputs.len(), 2);

    let locked = test
        .store()
        .with_read_tx(|tx| tx.confidential_outputs_get_locked_by_lock_id(proof_id))
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
            value: 24.into(),
            sender_public_nonce: None,
            view_only_key_id: KeyId::derived(0),
            owner_key_id: Some(KeyId::derived(0)),
            encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
            public_asset_tag: None,
            status: OutputStatus::LockedUnconfirmed,
            lock_id: Some(proof_id),
        })
        .unwrap();

    let balance = test.get_unspent_balance();
    assert_eq!(balance, 100);

    outputs_api.finalize_outputs_for_lock(proof_id).unwrap();

    {
        let mut tx = test.store().create_read_tx().unwrap();
        let locked = tx.confidential_outputs_get_locked_by_lock_id(proof_id).unwrap();
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
