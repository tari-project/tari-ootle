//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, convert::TryFrom, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use cucumber::{gherkin::Step, given, then, when, World};
use indexmap::IndexMap;
use serde_json::{json, Value as JsonValue};
use tari_common::configuration::Network;
use tari_common_types::types::{PrivateKey, PublicKey};
use tari_crypto::keys::PublicKey as PublicKeyTrait;
use tari_dan_common_types::{Epoch, NodeAddressable, NumPreshards, PeerAddress, ShardGroup, SubstateAddress};
use tari_dan_wallet_daemon::json_rpc::{AccountGetDefaultRequest, AccountGetRequest, AccountsCreateFreeTestCoinsRequest, AccountsGetBalancesRequest, AccountsInvokeRequest, AccountsListRequest, AccountsTransferRequest, ClaimBurnRequest, ClaimValidatorFeesRequest, KeysCreateRequest, KeysListRequest, KeysSetActiveRequest, SettingsGetRequest, SettingsSetRequest, SubstatesGetRequest, SubstatesListRequest, TransactionGetAllRequest, TransactionGetRequest, TransactionGetResultRequest, TransactionSubmitRequest, TransactionWaitResultRequest};
use tari_dan_wallet_daemon::json_rpc::ClaimBurnRequest as WalletDaemonClaimBurnRequest;
use tari_dan_wallet_sdk::apis::key_manager;
use tari_dan_wallet_sdk::storage::WalletStorageError;
use tari_dan_wallet_daemon::json_rpc::WalletDaemonJsonRpcClient;
use tari_engine_types::{commit_result::FinalizeResult, confidential::get_commitment_factory};
use tari_template_lib::{arg, args, models::{Amount, ComponentAddress}};
use tari_utilities::hex::Hex;
use tari_wallet_daemon_client::{types::{AuthLoginAcceptRequest, AuthLoginDenyRequest, AuthLoginRequest, KeysCreateRequest as WalletKeysCreateRequest, TransferRequest}, WalletDaemonClient};

use crate::TariWorld;

const LOG_TARGET: &str = "integration_tests::wallet_daemon";

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}")]
async fn when_i_claim_burn_and_spend(
    world: &mut TariWorld,
    burn_proof_var: String,
    account_var: String,
    wallet_daemon_name: String,
) -> anyhow::Result<()> {
    let burn_proof = world
        .burn_proofs
        .get(&burn_proof_var)
        .ok_or_else(|| anyhow!("Burn proof {} not found", burn_proof_var))?
        .clone();

    let account = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .get_account(&account_var)
        .await?;

    // Create ownership proof using the account's key
    let account_key = account.owner_key.clone();
    let commitment_bytes = burn_proof.commitment.as_bytes();
    let ownership_proof = tari_crypto::ristretto::RistrettoComSig::sign(
        burn_proof.ownership_proof.u(),
        &account_key,
        &commitment_bytes,
        &tari_crypto::hash_domain!("ownership_proof", 0),
    )?;

    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let claim_proof = tari_dan_wallet_daemon::json_rpc::ClaimProof {
        commitment: burn_proof.commitment.clone(),
        range_proof: burn_proof.range_proof.clone(),
        ownership_proof,
        reciprocal_claim_public_key: burn_proof.reciprocal_claim_public_key.clone(),
    };

    let req = WalletDaemonClaimBurnRequest {
        account: Some(account.account.address.clone()),
        claim_proof,
        fee: None,
        max_fee: None,
        resource_address: None,
    };

    let resp = client.claim_burn(req).await?;
    
    world
        .wallet_daemons
        .get_mut(&wallet_daemon_name)
        .unwrap()
        .add_transaction(resp.transaction_id.to_string(), resp.result);

    Ok(())
}

#[when(expr = "I claim validator fees for epoch {int} using wallet daemon {word}")]
async fn when_i_claim_validator_fees(
    world: &mut TariWorld,
    epoch: u64,
    wallet_daemon_name: String,
) -> anyhow::Result<()> {
    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let req = ClaimValidatorFeesRequest {
        account: None,
        max_fee: None,
        validator_public_key: world
            .validator_nodes
            .values()
            .next()
            .unwrap()
            .identity
            .public_key()
            .clone(),
        epoch: Epoch(epoch),
    };

    let resp = client.claim_validator_fees(req).await?;

    world
        .wallet_daemons
        .get_mut(&wallet_daemon_name)
        .unwrap()
        .add_transaction(resp.transaction_id.to_string(), resp.result);

    Ok(())
}

#[when(expr = "I transfer {int} from account {word} to account {word} on wallet daemon {word}")]
async fn when_i_transfer_between_accounts(
    world: &mut TariWorld,
    amount: u64,
    from_account: String,
    to_account: String,
    wallet_daemon_name: String,
) -> anyhow::Result<()> {
    let from_account = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .get_account(&from_account)
        .await?;
    let to_account = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .get_account(&to_account)
        .await?;

    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let req = AccountsTransferRequest {
        from_account: from_account.account.address,
        to_account: to_account.account.address,
        max_fee: None,
        amount: Amount::new(amount.try_into().unwrap()),
        resource_address: None,
        proof_from_badge_resource: None,
    };

    let resp = client.accounts_transfer(req).await?;

    world
        .wallet_daemons
        .get_mut(&wallet_daemon_name)
        .unwrap()
        .add_transaction(resp.transaction_id.to_string(), resp.result);

    Ok(())
}

#[when(expr = "I create an account {word} on wallet daemon {word}")]
async fn when_i_create_account_on_wallet_daemon(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
) -> anyhow::Result<()> {
    let wallet_daemon = world.wallet_daemons.get_mut(&wallet_daemon_name).unwrap();
    wallet_daemon.create_account(account_name).await?;
    Ok(())
}

#[when(expr = "I create {int} free test coins on wallet daemon {word}")]
async fn when_i_create_free_test_coins_on_wallet_daemon(
    world: &mut TariWorld,
    amount: u64,
    wallet_daemon_name: String,
) -> anyhow::Result<()> {
    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let req = AccountsCreateFreeTestCoinsRequest {
        account: None,
        amount: Amount::new(amount.try_into().unwrap()),
        max_fee: None,
    };

    let resp = client.accounts_create_free_test_coins(req).await?;

    world
        .wallet_daemons
        .get_mut(&wallet_daemon_name)
        .unwrap()
        .add_transaction(resp.transaction_id.to_string(), resp.result);

    Ok(())
}

#[then(expr = "the wallet daemon {word} has at least {int} XTR in account {word}")]
async fn then_wallet_daemon_has_at_least_xtr(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    amount: u64,
    account_name: String,
) -> anyhow::Result<()> {
    let account = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .get_account(&account_name)
        .await?;

    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let req = AccountsGetBalancesRequest {
        account: account.account.address,
        refresh: Some(true),
    };

    let resp = client.accounts_get_balances(req).await?;
    let balance = resp
        .balances
        .get(&world.base_layer_wallet.get_tari_resource_address())
        .cloned()
        .unwrap_or_default();

    if balance.balance < Amount::new(amount.try_into().unwrap()) {
        return Err(anyhow!(
            "Account {} on wallet daemon {} has {} XTR, expected at least {}",
            account_name,
            wallet_daemon_name,
            balance.balance,
            amount
        ));
    }

    Ok(())
}

#[then(expr = "the wallet daemon {word} has exactly {int} XTR in account {word}")]
async fn then_wallet_daemon_has_exactly_xtr(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    amount: u64,
    account_name: String,
) -> anyhow::Result<()> {
    let account = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .get_account(&account_name)
        .await?;

    let mut client = world
        .wallet_daemons
        .get(&wallet_daemon_name)
        .unwrap()
        .create_client();

    let req = AccountsGetBalancesRequest {
        account: account.account.address,
        refresh: Some(true),
    };

    let resp = client.accounts_get_balances(req).await?;
    let balance = resp
        .balances
        .get(&world.base_layer_wallet.get_tari_resource_address())
        .cloned()
        .unwrap_or_default();

    if balance.balance != Amount::new(amount.try_into().unwrap()) {
        return Err(anyhow!(
            "Account {} on wallet daemon {} has {} XTR, expected exactly {}",
            account_name,
            wallet_daemon_name,
            balance.balance,
            amount
        ));
    }

    Ok(())
}
