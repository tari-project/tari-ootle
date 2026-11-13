// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::info;
use tari_engine_types::substate::SubstateId;
use tari_ootle_address::{DestinationTag, Network, OotleAddress};
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_crypto::memo::Memo;
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission},
    types::{
        StealthUtxosDecryptValueRequest,
        StealthUtxosDecryptValueResponse,
        StealthUtxosGetValueLookupInfoRequest,
        StealthUtxosGetValueLookupInfoResponse,
        StealthUtxosListRequest,
        StealthUtxosListResponse,
        UtxoInfo,
    },
};
use tari_template_lib_types::{UtxoAddress, crypto::RistrettoPublicKeyBytes};
use tokio::{task::spawn_blocking, time::Instant};

use crate::handlers::{
    HandlerContext,
    helpers::invalid_params,
    value_lookup::{AbortQueuedOnDropGuard, ValueRangeRequest},
};

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::stealth_utxos";

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosListRequest,
) -> Result<StealthUtxosListResponse, anyhow::Error> {
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, req.account_address)])?;

    let network = context.wallet_sdk().network();

    let utxos = context.wallet_sdk().stealth_outputs_api().utxos_get_many(
        &req.resource_address,
        req.account_address.as_ref(),
        req.filter_by_status,
    )?;

    Ok(StealthUtxosListResponse {
        utxos: utxos
            .into_iter()
            .map(|o| UtxoInfo {
                address: o.to_utxo_address(),
                value: o.value.into(),
                status: o.status,
                sender_address: resolve_sender_address(o.memo.as_ref(), network),
                memo: o.memo,
                auth: o.auth,
                is_burnt: o.is_burnt,
                is_frozen: o.is_frozen,
                is_on_chain: o.is_on_chain,
            })
            .collect(),
    })
}

/// Reconstructs the sender's [`OotleAddress`] from a `SenderAddress` memo using the wallet's network, including
/// any attached destination tag. Returns `None` for any other memo variant or if the embedded keys are malformed.
fn resolve_sender_address(memo: Option<&Memo>, network: Network) -> Option<OotleAddress> {
    let (account_key, view_key, destination_tag) = memo?.as_sender_address()?;
    let account_key = RistrettoPublicKeyBytes::from_bytes(account_key).ok()?;
    let view_key = RistrettoPublicKeyBytes::from_bytes(view_key).ok()?;
    let mut address = OotleAddress::new(network, view_key, account_key);
    if let Some(destination_tag) = DestinationTag::from_bytes(destination_tag) {
        address = address.with_destination_tag(destination_tag);
    }
    Some(address)
}

#[allow(clippy::too_many_lines)]
pub async fn handle_decrypt_value(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosDecryptValueRequest,
) -> Result<StealthUtxosDecryptValueResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, None)])?;
    if req.ids.is_empty() {
        return Err(invalid_params("ids", Some("At least one UTXO ID must be provided")));
    }

    if req.ids.len() > 10 {
        return Err(invalid_params(
            "ids",
            Some("Cannot request more than 10 UTXOs at a time"),
        ));
    }

    let view_key_id = req.view_key_id;
    if let Some(branch) = view_key_id.derived_branch().filter(|b| b.is_mask()) {
        return Err(invalid_params(
            "view_key_id",
            Some(format!("{branch} keys cannot be used as a view key")),
        ));
    }

    let view_key = sdk
        .key_manager_api()
        .get_key(view_key_id)
        .optional()?
        .ok_or_else(|| {
            invalid_params(
                "view_key_id",
                Some(format!(
                    "{view_key_id} does not exist: the wallet holds no imported key with that id"
                )),
            )
        })?;

    let utxo_ids = req
        .ids
        .iter()
        .copied()
        .map(|id| SubstateId::from(UtxoAddress::new(req.resource_address, id)))
        .collect::<Vec<_>>();

    let substates = sdk.substate_api().get_substates_from_network(&utxo_ids).await?;

    let value_range = ValueRangeRequest::resolve(req.minimum_expected_value, req.maximum_expected_value);

    let proofs = req
        .ids
        .into_iter()
        .filter_map(|id| {
            let substate_id = SubstateId::from(UtxoAddress::new(req.resource_address, id));
            let s = substates.get(&substate_id)?;
            let output = s
                .substate_value()
                .as_utxo()
                .and_then(|u| u.output())
                .and_then(|o| o.output.viewable_balance.as_ref())?;
            Some((id, output))
        })
        .collect::<IndexMap<_, _>>();

    let timer = Instant::now();
    let elgamal_proofs = proofs.values().copied().cloned().collect::<Vec<_>>();
    let sdk = sdk.clone();
    let lookup_file = context.config().value_lookup_table_file.clone();
    let handle = spawn_blocking(move || {
        crate::handlers::value_lookup::brute_force_viewable_balances(
            &sdk.viewable_balance_api(),
            lookup_file.as_deref(),
            view_key.secret(),
            &elgamal_proofs,
            value_range,
        )
    });
    let _drop_guard = AbortQueuedOnDropGuard::new(handle.abort_handle());
    let recovery = handle.await??;

    info!(target: LOG_TARGET, "Brute force balance lookup took {:.2?}", timer.elapsed());

    Ok(StealthUtxosDecryptValueResponse {
        values: proofs.into_keys().zip(recovery.balances).collect(),
        searched: recovery.searched,
    })
}

pub async fn handle_get_value_lookup_info(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _req: StealthUtxosGetValueLookupInfoRequest,
) -> Result<StealthUtxosGetValueLookupInfoResponse, anyhow::Error> {
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, None)])?;
    crate::handlers::value_lookup::value_lookup_info(context.config().value_lookup_table_file.as_deref())
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
    use tari_ootle_common_types::{Epoch, signature::SignatureOutput};
    use tari_ootle_transaction::UnsignedTransaction;
    use tari_ootle_wallet_sdk::{
        apis::key_manager::KeyManagerApiError,
        models::{KeyBranch, KeyId, KeyType, StealthUtxoSpendKeyId},
    };
    use tari_template_lib_types::{UtxoId, constants::STEALTH_TARI_RESOURCE_ADDRESS};

    use super::*;
    use crate::handlers::test_support::TestDaemon;

    #[tokio::test]
    async fn decrypt_value_refuses_mask_and_unknown_view_keys_before_network_io() {
        let daemon = TestDaemon::start().await;
        let session = Some(&daemon.session);
        let request = |view_key_id| StealthUtxosDecryptValueRequest {
            resource_address: STEALTH_TARI_RESOURCE_ADDRESS,
            ids: vec![UtxoId::from_array([1u8; 32])],
            view_key_id,
            minimum_expected_value: Some(0),
            maximum_expected_value: Some(10),
        };

        for branch in [KeyBranch::ConfidentialMask, KeyBranch::StealthMask] {
            let err = handle_decrypt_value(&daemon.context, session, request(KeyId::derived(branch, 0)))
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("view_key_id") && err.to_string().contains("cannot be used as a view key"),
                "{branch}: {err}"
            );
        }

        let err = handle_decrypt_value(&daemon.context, session, request(KeyId::imported(999)))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("view_key_id") && err.to_string().contains("does not exist"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn stealth_owner_signing_requires_signing_account_key_and_non_mask_view_key() {
        let daemon = TestDaemon::start().await;
        let sdk = daemon.context.wallet_sdk();
        let km = sdk.key_manager_api();
        let (_, nonce_pk) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let public_nonce = nonce_pk.to_byte_type();
        let (_, seal_pk) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let seal_public_key = seal_pk.to_byte_type();
        let tx = UnsignedTransaction::new(Network::LocalNet.as_byte(), Epoch(1));
        let valid_view_key = KeyId::derived(KeyBranch::ViewOnlyKey, 0);
        let valid_account_key = KeyId::derived(KeyBranch::Account, 0);

        for branch in [
            KeyBranch::ConfidentialMask,
            KeyBranch::StealthMask,
            KeyBranch::ViewOnlyKey,
            KeyBranch::ElgamalEncryptionViewKey,
        ] {
            let spend_key = StealthUtxoSpendKeyId {
                account_key_id: KeyId::derived(branch, 0),
                view_key_id: valid_view_key,
                public_nonce,
            };
            let err = km
                .sign_with_stealth_key::<_, _>(&spend_key, &seal_public_key, &tx)
                .map(|_: SignatureOutput| ())
                .unwrap_err();
            assert!(
                matches!(err, KeyManagerApiError::NotASigningKey { key_id } if key_id == KeyId::derived(branch, 0)),
                "{branch}: {err:?}"
            );
        }

        let (imported_view_secret, _) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let imported_view = km
            .import_key("view-only", &imported_view_secret, KeyType::ViewOnly)
            .unwrap();
        let spend_with_imported_view = StealthUtxoSpendKeyId {
            account_key_id: imported_view,
            view_key_id: valid_view_key,
            public_nonce,
        };
        let err = km
            .sign_with_stealth_key::<_, _>(&spend_with_imported_view, &seal_public_key, &tx)
            .map(|_: SignatureOutput| ())
            .unwrap_err();
        assert!(
            matches!(err, KeyManagerApiError::NotASigningKey { key_id } if key_id == imported_view),
            "{err:?}"
        );

        let spend_with_unknown = StealthUtxoSpendKeyId {
            account_key_id: KeyId::imported(999),
            view_key_id: valid_view_key,
            public_nonce,
        };
        let err = km
            .sign_with_stealth_key::<_, _>(&spend_with_unknown, &seal_public_key, &tx)
            .map(|_: SignatureOutput| ())
            .unwrap_err();
        assert!(
            matches!(err, KeyManagerApiError::KeyNotFound { key_id } if key_id == KeyId::imported(999)),
            "{err:?}"
        );

        for branch in [KeyBranch::ConfidentialMask, KeyBranch::StealthMask] {
            let spend_key = StealthUtxoSpendKeyId {
                account_key_id: valid_account_key,
                view_key_id: KeyId::derived(branch, 0),
                public_nonce,
            };
            let err = km
                .sign_with_stealth_key::<_, _>(&spend_key, &seal_public_key, &tx)
                .map(|_: SignatureOutput| ())
                .unwrap_err();
            assert!(matches!(err, KeyManagerApiError::InvalidKeyId { .. }), "{branch}: {err:?}");
        }

        let valid_spend_key = StealthUtxoSpendKeyId {
            account_key_id: valid_account_key,
            view_key_id: valid_view_key,
            public_nonce,
        };
        let sig: SignatureOutput = km
            .sign_with_stealth_key(&valid_spend_key, &seal_public_key, &tx)
            .unwrap();
        assert_ne!(sig.public_key, km.get_public_key(valid_account_key).unwrap().public_key);
    }
}
