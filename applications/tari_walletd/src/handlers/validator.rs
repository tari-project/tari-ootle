//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, iter};

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use either::Either;
use log::*;
use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{InputDeclaration, SubstateAddress, derive_fee_pool_address};
use tari_ootle_transaction::args;
use tari_ootle_wallet_crypto::{OutputWitness, StealthInputWitness, StealthOutputWitness, memo::Memo};
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId, TransactionContext};
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission, Permissions, VnFeesAction},
    types::{
        AccountOrKeyId,
        ClaimValidatorFeesRequest,
        ClaimValidatorFeesResponse,
        FeePoolDetails,
        GetValidatorFeesRequest,
        GetValidatorFeesResponse,
    },
};
use tari_template_lib_types::{
    ValidatorFeePoolAddress,
    constants::{STEALTH_TARI_RESOURCE_ADDRESS, TARI_TOKEN},
    stealth::{SpendAuthorization, StealthTransferStatement},
};

use crate::{
    NUM_PRESHARDS,
    handlers::{
        HandlerContext,
        auth::jwt::{AuthError, enforce_scopes},
        helpers::{
            ensure_signing_key,
            get_account_or_default,
            get_account_with_inputs,
            invalid_params,
            wait_for_result,
        },
    },
};

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::validator";

fn ensure_has_vn_fees_grant(granted: &Permissions, action: VnFeesAction) -> Result<(), AuthError> {
    if !granted
        .iter()
        .any(|p| matches!(p, Permission::Admin | Permission::VnFees(_, _)))
    {
        return Err(AuthError::InsufficientPermissions {
            required: Permission::VnFees(action, None),
        });
    }
    Ok(())
}

pub async fn handle_get_validator_fees(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: GetValidatorFeesRequest,
) -> Result<GetValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    let granted = context.check_auth(token)?;
    ensure_has_vn_fees_grant(&granted, VnFeesAction::Read)?;

    let claim_public_key = match req.account_or_key {
        AccountOrKeyId::Account(acc) => {
            let account = get_account_or_default(acc.as_ref(), &sdk.accounts_api())?;
            // The account public key is the claim key of the account's owner key, and is known
            // for view-only accounts too, which have no owner key id in this wallet.
            *account.address.account_public_key()
        },
        AccountOrKeyId::KeyId(key_id) => {
            ensure_signing_key(&sdk, "account_or_key", &key_id)?;
            sdk.key_manager_api()
                .get_public_key(key_id)?
                .public_key()
                .to_byte_type()
        },
    };
    enforce_scopes(&granted, &[Permission::VnFees(
        VnFeesAction::Read,
        Some(claim_public_key),
    )])?;

    let shards = req
        .shard_group
        .map(|sg| Either::Left(sg.shard_iter()))
        .unwrap_or_else(|| Either::Right(NUM_PRESHARDS.all_shards_iter()));

    let ids = shards
        .into_iter()
        .map(|shard| {
            derive_fee_pool_address(&claim_public_key, NUM_PRESHARDS, shard)
                .map(SubstateId::from)
                .map_err(|err| invalid_params("shard_group", Some(err)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut fees = HashMap::with_capacity(ids.len());
    const CHUNK_SIZE: usize = 20;
    for id_chunk in ids.chunks(CHUNK_SIZE) {
        let substates = context
            .wallet_sdk()
            .substate_api()
            .get_substates_from_network(id_chunk)
            .await?;

        info!(target: LOG_TARGET, "🔍️ Found {}/{} fee pool substates for claim key {}", substates.len(), CHUNK_SIZE, claim_public_key);

        for (substate_id, substate) in substates {
            let Some(address) = substate_id.as_validator_fee_pool_address() else {
                warn!(target: LOG_TARGET, "Incorrect substate ID found: {}", substate_id);
                continue;
            };

            let Some(amount) = substate.substate_value().as_validator_fee_pool().map(|p| p.amount()) else {
                warn!(target: LOG_TARGET, "Incorrect substate type found at address {}", substate_id);
                continue;
            };

            if amount > 0 {
                let shard = SubstateAddress::from_substate_id(&substate_id, substate.version()).to_shard(NUM_PRESHARDS);
                fees.insert(shard, FeePoolDetails { amount, address });
            }
        }
    }

    Ok(GetValidatorFeesResponse { fees })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_claim_validator_fees(
    context: &HandlerContext,
    token: Option<&Bearer>,
    mut req: ClaimValidatorFeesRequest,
) -> Result<ClaimValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    let granted = context.check_auth(token)?;
    ensure_has_vn_fees_grant(&granted, VnFeesAction::Claim)?;
    if !req.dry_run && req.max_fee == 0 {
        return Err(invalid_params("max_fee", Some("max_fee must be greater than zero")));
    }

    let (account, inputs) = get_account_with_inputs(req.account.as_ref(), &sdk)?;
    let account_component_address = *account.component_address();
    let account_key_id = account.owner_key_id().ok_or_else(|| {
        invalid_params(
            "account",
            Some("The specified account does not have an associated owner key to derive the claim key from"),
        )
    })?;

    let claim_public_key = match req.claim_key_index {
        Some(index) => sdk
            .key_manager_api()
            .get_public_key(KeyId::derived(KeyBranch::Account, index))?
            .public_key
            .to_byte_type(),
        None => *account.address.account_public_key(),
    };
    // Checked before anything below allocates a stealth mask or signs.
    enforce_scopes(&granted, &[
        Permission::VnFees(VnFeesAction::Claim, Some(claim_public_key)),
        Permission::Transfer(Crud::Create, Some(account_component_address)),
    ])?;

    if !req.dry_run {
        context.enforce_max_fee_limit(req.max_fee)?;
    }
    if req.shards.is_empty() {
        return Err(invalid_params("shards", Some("At least one shard must be specified")));
    }

    req.shards.sort();
    req.shards.dedup();
    let fee_pool_addresses = req
        .shards
        .iter()
        .map(|shard| {
            derive_fee_pool_address(&claim_public_key, NUM_PRESHARDS, *shard)
                .map_err(|err| invalid_params("shards", Some(err)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // build the transaction
    let max_fee = req.max_fee.max(1);
    let account_public_key = *account.address.account_public_key();

    let builder = context.transaction_builder().await?.with_dry_run(req.dry_run);

    let builder = if req.output_to_revealed {
        let (first, rest) = fee_pool_addresses
            .split_first()
            .ok_or_else(|| invalid_params("shards", Some("At least one shard must be specified")))?;
        let first = *first;
        let rest = rest.to_vec();
        builder
            .with_fee_instructions_builder(move |builder| {
                let builder = builder
                    .create_account(account_public_key)
                    .claim_validator_fees(first)
                    .put_last_instruction_output_on_workspace("joined");
                rest.into_iter()
                    .enumerate()
                    .fold(builder, |b, (i, address)| {
                        let tmp = format!("tmp{i}");
                        b.claim_validator_fees(address)
                            .put_last_instruction_output_on_workspace(tmp.clone())
                            .put_into_bucket(tmp, "joined")
                    })
                    .call_method(account_component_address, "deposit", args![Workspace("joined")])
                    .call_method(account_component_address, "pay_fee", args![max_fee])
            })
            .with_inputs(
                inputs
                    .into_iter()
                    .map(|input| InputDeclaration::write(input.into_substate_id())),
            )
    } else {
        let (statement, pool_amounts) =
            build_self_stealth_statement(&sdk, &account, account_key_id, &fee_pool_addresses, max_fee).await?;
        let (first, rest) = pool_amounts
            .split_first()
            .ok_or_else(|| invalid_params("shards", Some("At least one shard must be specified")))?;
        let (first_address, first_amount) = *first;
        let rest = rest.to_vec();
        builder.with_fee_instructions_builder(move |builder| {
            let builder = builder
                .claim_validator_fees_up_to(first_address, first_amount)
                .put_last_instruction_output_on_workspace("joined");
            rest.into_iter()
                .enumerate()
                .fold(builder, |b, (i, (address, amount))| {
                    let tmp = format!("tmp{i}");
                    b.claim_validator_fees_up_to(address, amount)
                        .put_last_instruction_output_on_workspace(tmp.clone())
                        .put_into_bucket(tmp, "joined")
                })
                .take_from_bucket("joined", max_fee, "fee")
                .pay_fee_from_bucket("fee")
                .stealth_transfer_with_input_bucket(TARI_TOKEN, statement, "joined")
        })
    };

    let unsigned_transaction = builder
        .with_inputs(fee_pool_addresses.iter().copied().map(InputDeclaration::write))
        .map(|builder| {
            if let Some(index) = req.claim_key_index {
                if claim_public_key == account_public_key {
                    Ok(builder.finish())
                } else {
                    // If the claim key is different from the account secret, we need to sign with both
                    sdk.signer_api().authorize(
                        &account_public_key,
                        &[KeyId::derived(KeyBranch::Account, index)],
                        &[],
                        builder.finish(),
                    )
                }
            } else {
                Ok(builder.finish())
            }
        })?;

    let transaction = sdk.signer_api().sign(account_key_id, unsigned_transaction)?;

    // send the transaction
    if req.dry_run {
        let transaction = sdk.transaction_api().submit_dry_run_transaction(transaction).await?;
        return Ok(ClaimValidatorFeesResponse {
            transaction_id: transaction.id,
            // Dry-run forces a minimal max_fee so the call doesn't require funded vaults, which clamps
            // `total_fees_paid` to that placeholder. Report the uncapped estimate instead.
            fee: transaction
                .finalize
                .as_ref()
                .map(|f| f.required_fees())
                .unwrap_or_default(),
            result: transaction
                .finalize
                .ok_or_else(|| anyhow!("No finalize result for dry run transaction"))?,
        });
    }

    let mut events = context.notifier().subscribe();
    // Link the transaction to the account the fees are claimed into so it appears in the
    // account-filtered transaction list.
    let tx_id = context
        .transaction_service()
        .submit_transaction_with_opts(
            transaction,
            Some(TransactionContext::with_accounts([account_component_address])),
            None,
        )
        .await?;

    let finalized = wait_for_result(&mut events, tx_id).await?;

    if let Some(reject) = finalized.finalize.fee_reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however the transaction failed: {reason}",
        ));
    }
    info!(
        target: LOG_TARGET,
        "✅ Claim fee transaction {} finalized. Fee: {}",
        finalized.transaction_id,
        finalized.final_fee
    );

    Ok(ClaimValidatorFeesResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        result: finalized.finalize,
    })
}

/// Fetches each fee pool's current amount, sums them, and builds a single [`StealthTransferStatement`] that converts
/// the consolidated revealed bucket (minus `max_fee` which is carved into a separate revealed bucket on-chain to pay
/// the network fee) into one stealth UTXO addressed to the account's own owner key.
async fn build_self_stealth_statement(
    sdk: &crate::WalletSdk,
    account: &tari_ootle_wallet_sdk::models::AccountWithAddress,
    account_owner_key_id: tari_ootle_wallet_sdk::models::KeyId,
    fee_pool_addresses: &[ValidatorFeePoolAddress],
    max_fee: u64,
) -> Result<(StealthTransferStatement, Vec<(ValidatorFeePoolAddress, u64)>), anyhow::Error> {
    let network = sdk.config_api().get_network()?;
    let account_owner = sdk.key_manager_api().get_public_key(account_owner_key_id)?;
    let view_only = sdk.key_manager_api().get_public_key(account.view_only_key_id())?;

    let substate_ids: Vec<SubstateId> = fee_pool_addresses.iter().copied().map(SubstateId::from).collect();
    let mut amounts: HashMap<ValidatorFeePoolAddress, u64> = HashMap::with_capacity(substate_ids.len());
    const CHUNK_SIZE: usize = 20;
    for chunk in substate_ids.chunks(CHUNK_SIZE) {
        let substates = sdk.substate_api().get_substates_from_network(chunk).await?;
        for (id, substate) in substates {
            let Some(addr) = id.as_validator_fee_pool_address() else {
                continue;
            };
            let Some(amount) = substate.substate_value().as_validator_fee_pool().map(|p| p.amount()) else {
                continue;
            };
            amounts.insert(addr, amount);
        }
    }

    let mut pool_amounts = Vec::with_capacity(fee_pool_addresses.len());
    let mut total: u64 = 0;
    for address in fee_pool_addresses {
        let amount = amounts.get(address).copied().unwrap_or(0);
        if amount == 0 {
            return Err(invalid_params(
                "shards",
                Some(format!("Fee pool {address} is empty or could not be fetched")),
            ));
        }
        total = total
            .checked_add(amount)
            .ok_or_else(|| invalid_params("shards", Some("Total claimable fee pool amount overflows u64")))?;
        pool_amounts.push((*address, amount));
    }

    if total <= max_fee {
        return Err(invalid_params(
            "max_fee",
            Some(format!(
                "max_fee ({max_fee}) is not less than the total claimable fee pool amount ({total}); reduce max_fee \
                 or include shards with larger balances"
            )),
        ));
    }

    let stealth_amount = total - max_fee;

    let memo = Memo::new_message("Validator fees claimed to stealth").expect("valid memo");
    let mask = sdk.key_manager_api().next_key(KeyBranch::StealthMask)?;
    let (nonce, output_public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());

    let encrypted_data = sdk.stealth_crypto_api().encrypt_value_and_mask(
        stealth_amount,
        &mask.key,
        view_only.public_key(),
        &nonce,
        Some(&memo),
    )?;

    let tag = sdk.stealth_crypto_api().derive_stealth_output_tag(
        network,
        &nonce,
        view_only.public_key(),
        &STEALTH_TARI_RESOURCE_ADDRESS,
    );

    let stealth_owner_public_key = sdk.stealth_crypto_api().derive_stealth_owner_public_key(
        network,
        account_owner.public_key(),
        view_only.public_key(),
        &nonce,
    );

    let output_witness = StealthOutputWitness {
        witness: OutputWitness {
            amount: stealth_amount,
            mask: mask.key,
            sender_public_nonce: output_public_nonce,
            minimum_value_promise: 0,
            encrypted_data,
            resource_view_key: None,
        },
        auth: SpendAuthorization::Key(stealth_owner_public_key.to_byte_type()),
        tag,
    };

    let statement = sdk.stealth_crypto_api().generate_transfer_statement(
        iter::empty::<StealthInputWitness>(),
        stealth_amount,
        iter::once(&output_witness),
        None,
    )?;

    Ok((statement, pool_amounts))
}

#[cfg(test)]
mod tests {
    use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
    use tari_crypto::ristretto::RistrettoSecretKey;
    use tari_ootle_common_types::{Epoch, ShardGroup, shard::Shard};
    use tari_ootle_wallet_sdk::{apis::substate::SubstateApiError, models::KeyType};
    use tari_ootle_wallet_sdk_services::indexer_rest_api::IndexerRestApiNetworkInterfaceError;
    use tari_ootle_walletd_client::ComponentAddressOrName;
    use tari_template_lib_types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

    use super::*;
    use crate::handlers::test_support::TestDaemon;

    fn public_key_of(daemon: &TestDaemon, key_id: KeyId) -> RistrettoPublicKeyBytes {
        daemon
            .context
            .wallet_sdk()
            .key_manager_api()
            .get_public_key(key_id)
            .unwrap()
            .public_key()
            .to_byte_type()
    }

    fn account_key(index: u64) -> KeyId {
        KeyId::derived(KeyBranch::Account, index)
    }

    async fn get_fees_err(daemon: &TestDaemon, bearer: &Bearer, account_or_key: AccountOrKeyId) -> anyhow::Error {
        handle_get_validator_fees(&daemon.context, Some(bearer), GetValidatorFeesRequest {
            account_or_key,
            shard_group: Some(ShardGroup::new(Shard::first(), Shard::first())),
        })
        .await
        .unwrap_err()
    }

    async fn claim_fees_err(
        daemon: &TestDaemon,
        bearer: &Bearer,
        account: ComponentAddress,
        claim_key_index: Option<u64>,
        dry_run: bool,
        output_to_revealed: bool,
    ) -> anyhow::Error {
        handle_claim_validator_fees(&daemon.context, Some(bearer), ClaimValidatorFeesRequest {
            account: Some(account.into()),
            claim_key_index,
            max_fee: 1000,
            shards: vec![Shard::first()],
            dry_run,
            output_to_revealed,
        })
        .await
        .unwrap_err()
    }

    #[track_caller]
    fn assert_vn_fees_denied(err: &anyhow::Error, action: VnFeesAction, claim_pk: Option<RistrettoPublicKeyBytes>) {
        assert!(
            matches!(
                err.downcast_ref::<AuthError>(),
                Some(AuthError::InsufficientPermissions {
                    required: Permission::VnFees(a, pk),
                }) if *a == action && *pk == claim_pk
            ),
            "expected vn_fees:{action} ({claim_pk:?}) to be required, got: {err}"
        );
    }

    #[track_caller]
    fn assert_transfer_denied(err: &anyhow::Error, account: ComponentAddress) {
        assert!(
            matches!(
                err.downcast_ref::<AuthError>(),
                Some(AuthError::InsufficientPermissions {
                    required: Permission::Transfer(Crud::Create, Some(addr)),
                }) if *addr == account
            ),
            "expected transfer:create:{account} to be required, got: {err}"
        );
    }

    /// The test daemon's indexer is a closed port, so a request that passes every check fails on its first fee pool
    /// lookup.
    #[track_caller]
    fn assert_reached_fee_pool_lookup(err: &anyhow::Error) {
        assert!(
            matches!(
                err.downcast_ref::<SubstateApiError>(),
                Some(SubstateApiError::NetworkInterfaceError(_))
            ),
            "expected the fee pool lookup to fail on the closed indexer port, got: {err}"
        );
    }

    /// A claim that passes every check fails on the current epoch lookup that builds its transaction.
    #[track_caller]
    fn assert_reached_transaction_builder(err: &anyhow::Error) {
        assert!(
            matches!(
                err.downcast_ref::<IndexerRestApiNetworkInterfaceError>(),
                Some(IndexerRestApiNetworkInterfaceError::IndexerClientError(_))
            ),
            "expected the current epoch lookup to fail on the closed indexer port, got: {err}"
        );
    }

    fn assert_invalid_param(err: &anyhow::Error, field: &str) {
        let jrpc_err = err
            .downcast_ref::<JsonRpcError>()
            .unwrap_or_else(|| panic!("expected JsonRpcError, got: {err}"));
        assert!(matches!(jrpc_err.error_reason(), JsonRpcErrorReason::InvalidParams));
        assert!(
            err.to_string().contains(field),
            "expected invalid param for '{field}', got: {err}"
        );
    }

    #[tokio::test]
    async fn fees_are_looked_up_only_for_a_signing_key() {
        let daemon = TestDaemon::start().await;
        let reader = daemon.api_key("reader", "vn_fees:read");

        for branch in [
            KeyBranch::ConfidentialMask,
            KeyBranch::StealthMask,
            KeyBranch::ViewOnlyKey,
            KeyBranch::ElgamalEncryptionViewKey,
        ] {
            let err = handle_get_validator_fees(&daemon.context, Some(&reader), GetValidatorFeesRequest {
                account_or_key: AccountOrKeyId::KeyId(KeyId::derived(branch, 0)),
                shard_group: None,
            })
            .await
            .unwrap_err();
            assert_invalid_param(&err, "account_or_key");
        }
    }

    #[tokio::test]
    async fn fees_can_be_looked_up_for_a_view_only_account_while_claim_rejects_it_and_zero_max_fee() {
        let daemon = TestDaemon::start().await;
        let sdk = daemon.context.wallet_sdk();
        let view_only_addr = ComponentAddress::from_array([0xcc; 32]);
        let owner_pk = sdk
            .key_manager_api()
            .get_public_key(KeyId::derived(KeyBranch::Account, 0))
            .unwrap()
            .public_key
            .to_byte_type();
        sdk.accounts_api()
            .add_account(
                Some("view_only"),
                &view_only_addr,
                KeyId::derived(KeyBranch::ViewOnlyKey, 0),
                owner_pk,
                Epoch::zero(),
                false,
                false,
            )
            .unwrap();

        let get_err = handle_get_validator_fees(&daemon.context, Some(&daemon.session), GetValidatorFeesRequest {
            account_or_key: AccountOrKeyId::Account(Some(ComponentAddressOrName::ComponentAddress(view_only_addr))),
            shard_group: Some(ShardGroup::new(Shard::from(1u32), Shard::from(1u32))),
        })
        .await
        .expect_err("offline indexer fails after resolving the view-only account public key");
        assert!(
            get_err.downcast_ref::<JsonRpcError>().is_none() && !get_err.to_string().contains("owner key"),
            "view-only account lookup should reach the network instead of failing on owner key: {get_err}"
        );

        let zero_fee_err =
            handle_claim_validator_fees(&daemon.context, Some(&daemon.session), ClaimValidatorFeesRequest {
                account: Some(ComponentAddressOrName::ComponentAddress(view_only_addr)),
                claim_key_index: None,
                max_fee: 0,
                shards: vec![Shard::from(1u32)],
                dry_run: false,
                output_to_revealed: true,
            })
            .await
            .unwrap_err();
        assert_invalid_param(&zero_fee_err, "max_fee");

        let view_only_claim_err =
            handle_claim_validator_fees(&daemon.context, Some(&daemon.session), ClaimValidatorFeesRequest {
                account: Some(ComponentAddressOrName::ComponentAddress(view_only_addr)),
                claim_key_index: None,
                max_fee: 1000,
                shards: vec![Shard::from(1u32)],
                dry_run: true,
                output_to_revealed: true,
            })
            .await
            .unwrap_err();
        assert_invalid_param(&view_only_claim_err, "account");
    }

    /// Adds account `a` (owner key `Account/0`, the default) and account `b` (owner key `Account/1`).
    fn seed_two_accounts(daemon: &TestDaemon) -> (ComponentAddress, ComponentAddress) {
        let account_a = ComponentAddress::from_array([0xaa; 32]);
        let account_b = ComponentAddress::from_array([0xbb; 32]);
        for (index, (name, address)) in [("a", account_a), ("b", account_b)].into_iter().enumerate() {
            daemon
                .context
                .wallet_sdk()
                .accounts_api()
                .add_account(
                    Some(name),
                    &address,
                    KeyId::derived(KeyBranch::ViewOnlyKey, index as u64),
                    KeyId::derived(KeyBranch::Account, index as u64),
                    Epoch::zero(),
                    false,
                    index == 0,
                )
                .unwrap();
        }
        (account_a, account_b)
    }

    #[tokio::test]
    async fn get_validator_fees_requires_vn_fees_read_for_the_claim_key() {
        let daemon = TestDaemon::start().await;
        let (account_a, account_b) = seed_two_accounts(&daemon);
        let imported_key_id = daemon
            .context
            .wallet_sdk()
            .key_manager_api()
            .import_key("imported", &RistrettoSecretKey::from(44u64), KeyType::GeneralPurpose)
            .unwrap();
        let tx_key_id = KeyId::derived(KeyBranch::Transaction, 0);
        let pk_0 = public_key_of(&daemon, account_key(0));
        let pk_1 = public_key_of(&daemon, account_key(1));

        let reader_0 = daemon.api_key("reader_0", &format!("vn_fees:read:{pk_0}"));
        let delegated_reader_0 = daemon.delegated_session(&format!("vn_fees:read:{pk_0}"));
        // Account and key grants say nothing about a claim key's fee pools.
        let account_and_key_reader = daemon.api_key("account_and_key_reader", "accounts:read,keys:read");

        for bearer in [&reader_0, &delegated_reader_0] {
            for (account_or_key, claim_key_id) in [
                (AccountOrKeyId::Account(Some(account_b.into())), account_key(1)),
                (AccountOrKeyId::KeyId(account_key(1)), account_key(1)),
                (AccountOrKeyId::KeyId(account_key(99)), account_key(99)),
                (AccountOrKeyId::KeyId(tx_key_id), tx_key_id),
                (AccountOrKeyId::KeyId(imported_key_id), imported_key_id),
            ] {
                let err = get_fees_err(&daemon, bearer, account_or_key).await;
                assert_vn_fees_denied(&err, VnFeesAction::Read, Some(public_key_of(&daemon, claim_key_id)));
            }
        }
        // A token with no vn_fees grant is rejected up front before resolving the account or key.
        let missing_account = ComponentAddress::from_array([0xff; 32]);
        for account_or_key in [
            AccountOrKeyId::Account(Some(account_a.into())),
            AccountOrKeyId::Account(Some(missing_account.into())),
            AccountOrKeyId::KeyId(imported_key_id),
            AccountOrKeyId::KeyId(KeyId::derived(KeyBranch::ConfidentialMask, 0)),
        ] {
            let err = get_fees_err(&daemon, &account_and_key_reader, account_or_key).await;
            assert_vn_fees_denied(&err, VnFeesAction::Read, None);
        }

        let claimer_1 = daemon.api_key("claimer_1", &format!("vn_fees:claim:{pk_1}"));
        let reader_all = daemon.api_key("reader_all", "vn_fees:read");
        let legacy_reader = daemon.api_key("legacy_reader", "validators:read");
        for (bearer, account_or_key) in [
            (&reader_0, AccountOrKeyId::Account(Some(account_a.into()))),
            (&reader_0, AccountOrKeyId::Account(None)),
            (&delegated_reader_0, AccountOrKeyId::KeyId(account_key(0))),
            (&claimer_1, AccountOrKeyId::Account(Some(account_b.into()))),
            (&claimer_1, AccountOrKeyId::KeyId(account_key(1))),
            (&reader_all, AccountOrKeyId::KeyId(account_key(99))),
            (&reader_all, AccountOrKeyId::KeyId(imported_key_id)),
            (&legacy_reader, AccountOrKeyId::KeyId(tx_key_id)),
            (&daemon.session, AccountOrKeyId::Account(Some(account_b.into()))),
        ] {
            let err = get_fees_err(&daemon, bearer, account_or_key).await;
            assert_reached_fee_pool_lookup(&err);
        }
    }

    #[tokio::test]
    async fn claim_validator_fees_requires_vn_fees_claim_for_the_claim_key() {
        let daemon = TestDaemon::start().await;
        let (account_a, account_b) = seed_two_accounts(&daemon);
        let pk_0 = public_key_of(&daemon, account_key(0));
        let pk_1 = public_key_of(&daemon, account_key(1));
        let pk_99 = public_key_of(&daemon, account_key(99));

        let claimer_0 = format!("vn_fees:claim:{pk_0},transfer:create:{account_a}");
        let claimer_0_key = daemon.api_key("claimer_0", &claimer_0);
        let delegated_claimer_0 = daemon.delegated_session(&claimer_0);
        // Transfer rights over every account say nothing about a claim key's fee pools.
        let transfer_all = daemon.api_key("transfer_all", "transfer:create");

        for bearer in [&claimer_0_key, &delegated_claimer_0] {
            for (claim_key_index, claim_pk) in [(Some(1), pk_1), (Some(99), pk_99)] {
                for dry_run in [true, false] {
                    for output_to_revealed in [true, false] {
                        let err =
                            claim_fees_err(&daemon, bearer, account_a, claim_key_index, dry_run, output_to_revealed)
                                .await;
                        assert_vn_fees_denied(&err, VnFeesAction::Claim, Some(claim_pk));
                    }
                }
            }
            let err = claim_fees_err(&daemon, bearer, account_b, None, true, false).await;
            assert_vn_fees_denied(&err, VnFeesAction::Claim, Some(pk_1));
        }

        // A token with no vn_fees grant is rejected up front before resolving the destination account.
        let missing_account = ComponentAddress::from_array([0xff; 32]);
        for account in [account_a, account_b, missing_account] {
            let err = claim_fees_err(&daemon, &transfer_all, account, None, true, false).await;
            assert_vn_fees_denied(&err, VnFeesAction::Claim, None);
        }

        // Reading a key's fee pools does not let a caller claim them.
        let reader_0 = daemon.api_key("reader_0", &format!("vn_fees:read:{pk_0},transfer:create:{account_a}"));
        let err = claim_fees_err(&daemon, &reader_0, account_a, None, true, false).await;
        assert_vn_fees_denied(&err, VnFeesAction::Claim, Some(pk_0));

        // The claim key's grant does not cover depositing into an account the caller cannot transfer from.
        let claimer_1 = daemon.api_key(
            "claimer_1",
            &format!("vn_fees:claim:{pk_1},transfer:create:{account_a}"),
        );
        let err = claim_fees_err(&daemon, &claimer_1, account_b, None, true, false).await;
        assert_transfer_denied(&err, account_b);
        let err = claim_fees_err(&daemon, &claimer_1, account_b, Some(1), true, false).await;
        assert_transfer_denied(&err, account_b);

        // Rejecting unauthorized stealth claims must not advance `KeyBranch::StealthMask`.
        assert!(
            daemon
                .context
                .wallet_sdk()
                .key_manager_api()
                .get_all_derived_keys(KeyBranch::StealthMask)
                .unwrap()
                .is_empty()
        );

        let legacy_claimer = daemon.api_key("legacy_claimer", "validators:update,transfer:create");
        for (bearer, claim_key_index) in [
            (&claimer_0_key, None),
            (&claimer_0_key, Some(0)),
            (&delegated_claimer_0, Some(0)),
            (&claimer_1, Some(1)),
            (&legacy_claimer, Some(99)),
            (&daemon.session, Some(99)),
        ] {
            let err = claim_fees_err(&daemon, bearer, account_a, claim_key_index, true, true).await;
            assert_reached_transaction_builder(&err);
        }
    }
}
