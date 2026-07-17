//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Demonstrates the transaction-request flow (issue #2343) from the requester's side:
//! a tool with limited permissions builds a transfer, asks the wallet for approval,
//! and submits once a separately-permissioned principal (e.g. the web UI) approves.
//!
//! ```text
//! build transfer -> detect inputs -> dry run (fee) -> transaction_requests.create
//!     -> poll until Approved (a human approves in the web UI)
//!     -> transaction_requests.submit -> transactions.wait_result
//! ```
//!
//! The API key needs exactly:
//! - `accounts:read`              -- find the default account and its owner key
//! - `transactions:read`          -- detect inputs, dry-run fee sizing, wait for the result
//! - `transaction_requests:create` -- create the request, poll it, submit once approved
//!
//! Deliberately absent: `transactions:create` (the tool cannot submit arbitrary
//! transactions) and `transaction_requests:approve` (the tool cannot approve its own ask).
//!
//! Run against a local walletd:
//!
//! ```text
//! cargo run --release -p tari_ootle_walletd --example transaction_request_flow -- \
//!     --endpoint http://127.0.0.1:5100 --api-key tw_...
//! ```
//!
//! By default this transfers 1000 µT from the default account back to itself (a
//! self-transfer, so only the fee actually leaves). Pass `--dest <public key hex>` to
//! pay someone else.

use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use tari_engine_types::component::derive_component_address_from_public_key;
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, args};
use tari_ootle_wallet_sdk::models::EffectiveStatus;
use tari_ootle_walletd_client::{
    WalletDaemonClient,
    types::{
        AccountGetResponse,
        TransactionDetectInputsRequest,
        TransactionRequestCreateRequest,
        TransactionRequestGetRequest,
        TransactionRequestSubmitRequest,
        TransactionSubmitDryRunRequest,
        TransactionWaitResultRequest,
    },
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib_types::{Amount, ComponentAddress, constants::TARI_TOKEN, crypto::RistrettoPublicKeyBytes};

#[derive(Parser)]
struct Args {
    /// Base URL of the wallet daemon.
    #[clap(long, default_value = "http://127.0.0.1:5100")]
    endpoint: String,
    /// API key (`tw_...`) holding accounts:read, transactions:read and
    /// transaction_requests:create.
    #[clap(long, env = "WALLETD_API_KEY")]
    api_key: String,
    /// Destination public key (hex). Defaults to the default account's own key,
    /// making the demo a harmless self-transfer.
    #[clap(long)]
    dest: Option<String>,
    /// Amount to transfer, in µT.
    #[clap(long, default_value_t = 1000)]
    amount: u64,
    /// Fee ceiling used for the dry-run sizing pass, in µT.
    #[clap(long, default_value_t = 1_000_000)]
    max_fee_cap: u64,
}

#[tokio::main]
#[expect(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut client = WalletDaemonClient::connect(format!("{}/json_rpc", args.endpoint.trim_end_matches('/')), None)?;
    client.set_auth_token(args.api_key.clone().into());

    // wallet.get_info is unauthenticated; it supplies the network byte the
    // transaction must be built for.
    let info = client.get_wallet_info().await.context("wallet.get_info failed")?;
    println!("Connected to walletd {} (network {})", info.version, info.network);

    // accounts:read — the source of funds and the key that seals (and pays).
    let AccountGetResponse { account, .. } = client
        .accounts_get_default()
        .await
        .context("accounts.get_default failed")?;
    let seal_signer = account
        .owner_key_id
        .ok_or_else(|| anyhow!("default account has no owner key (view-only?)"))?;
    let source = account.component_address;
    println!(
        "Source account: {} ({})",
        source,
        account.name.as_deref().unwrap_or("unnamed"),
    );

    let dest_pk = match &args.dest {
        Some(hex) => RistrettoPublicKeyBytes::from_hex(hex).map_err(|e| anyhow!("invalid --dest: {e}"))?,
        None => account.owner_public_key,
    };

    // Pass 1: build with the fee ceiling and dry-run it to learn the real fee.
    // transactions.detect_inputs resolves the substates this tool has no way to
    // enumerate itself; transactions.submit_dry_run executes without committing
    // (both gated on transactions:read).
    //
    // Detection returns the full dependency closure (every vault the account
    // holds, not just the one being spent) — the wallet cannot know intent
    // without executing. This demo keeps the closure for simplicity; a
    // production requester should narrow the inputs to what its transaction
    // touches, since every declared input adds weight (fees) and locking.
    let transaction = build_transfer(info.network_byte, source, dest_pk, args.amount, args.max_fee_cap);
    let transaction = detect_inputs(&mut client, transaction).await?;

    let dry_run = client
        .submit_transaction_dry_run(TransactionSubmitDryRunRequest {
            transaction,
            seal_signer,
            other_signers: vec![],
            signatures: vec![],
            detect_inputs: false,
            detect_inputs_use_unversioned: true,
            lock_ids: vec![],
        })
        .await
        .context("transactions.submit_dry_run failed")?;
    println!("Dry run: required fee {} µT", dry_run.required_fees);

    // Pass 2: rebuild with the sized fee. This is the transaction that gets
    // frozen — the approver sees these exact bytes and submit seals them.
    let transaction = build_transfer(info.network_byte, source, dest_pk, args.amount, dry_run.required_fees);
    let transaction = detect_inputs(&mut client, transaction).await?;

    let created = client
        .create_transaction_request(TransactionRequestCreateRequest {
            transaction,
            seal_signer,
            other_signers: vec![],
            signatures: vec![],
            lock_ids: vec![],
            ttl_secs: None,
        })
        .await
        .context("transaction_requests.create failed")?;
    println!(
        "Created transaction request {} (expires at unix {})",
        created.request_id, created.expires_at,
    );
    println!("Waiting for approval — approve or reject it on the walletd web UI's Transaction Requests page...");

    // transaction_requests:create satisfies :read, so the requester can poll
    // its own ask without an extra grant.
    let request = loop {
        let response = client
            .get_transaction_request(TransactionRequestGetRequest {
                request_id: created.request_id,
            })
            .await
            .context("transaction_requests.get failed")?;
        match response.request.status {
            // Submitting means another principal holds the submit claim; the
            // outcome (Submitted, or back to Approved on failure) is imminent.
            EffectiveStatus::Pending | EffectiveStatus::Submitting => tokio::time::sleep(Duration::from_secs(3)).await,
            _ => break response.request,
        }
    };

    let transaction_id = match request.status {
        EffectiveStatus::Approved => {
            println!("Approved.");
            let submitted = client
                .submit_transaction_request(TransactionRequestSubmitRequest {
                    request_id: created.request_id,
                })
                .await
                .context("transaction_requests.submit failed")?;
            println!("Submitted as transaction {}", submitted.transaction_id);
            submitted.transaction_id
        },
        // Any principal holding transaction_requests:create can submit an approved
        // request (the web UI does this in one click), so the tool may find its ask
        // already submitted. That is success — the frozen transaction is the one that
        // was sealed regardless of who pressed submit.
        EffectiveStatus::Submitted => {
            let id = request
                .transaction_id
                .ok_or_else(|| anyhow!("request is Submitted but no transaction id was recorded"))?;
            println!("Approved and submitted by another principal as transaction {id}");
            id
        },
        EffectiveStatus::Rejected => bail!("the request was rejected"),
        EffectiveStatus::Expired => bail!("the request expired before it was approved"),
        EffectiveStatus::Pending | EffectiveStatus::Submitting => {
            unreachable!("the poll loop only exits on a settled status")
        },
    };

    let result = client
        .wait_transaction_result(TransactionWaitResultRequest {
            transaction_id,
            timeout_secs: Some(120),
        })
        .await
        .context("transactions.wait_result failed")?;
    if result.timed_out {
        bail!("timed out waiting for transaction {transaction_id}");
    }
    println!("Finalized: status {:?}, fee {} µT", result.status, result.final_fee,);
    Ok(())
}

/// The same shape `accounts.transfer` builds: idempotent destination account
/// creation, fee from the source account, withdraw → workspace → deposit.
fn build_transfer(
    network: u8,
    source: ComponentAddress,
    dest_pk: RistrettoPublicKeyBytes,
    amount: u64,
    max_fee: u64,
) -> UnsignedTransaction {
    let dest = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &dest_pk);
    TransactionBuilder::new(network)
        .create_account(dest_pk)
        .pay_fee_from_component(source, Amount::from_integer(max_fee))
        .call_method(source, "withdraw", args![TARI_TOKEN, Amount::from_integer(amount)])
        .put_last_instruction_output_on_workspace("bucket")
        .call_method(dest, "deposit", args![Workspace("bucket")])
        .build_unsigned()
}

async fn detect_inputs(
    client: &mut WalletDaemonClient,
    transaction: UnsignedTransaction,
) -> anyhow::Result<UnsignedTransaction> {
    let response = client
        .detect_transaction_inputs(TransactionDetectInputsRequest {
            transaction,
            use_unversioned: true,
        })
        .await
        .context("transactions.detect_inputs failed")?;
    Ok(response.transaction)
}
