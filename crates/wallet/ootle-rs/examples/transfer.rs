//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_rs::{
    address,
    builtin_templates::{account::IAccount, faucet::IFaucet, InvokeBuilder},
    provider::{Provider, ProviderBuilder},
    signer::local_signer::PrivateKeySigner,
    transaction::TransactionSigner,
    wallet::OotleWallet,
    TransactionRequest,
};
use tari_ootle_common_types::Network;
use tari_ootle_wallet_sdk::{
    apis::accounts::derive_account_address_from_public_key,
    constants::{ONE_XTR, XTR},
};

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(tracing::log::LevelFilter::Debug)
        .init();

    println!("Starting integration test...");
    const NETWORK: Network = Network::LocalNet;

    const API_URL: &str = "http://127.0.0.1:12500";

    let sender_secret = PrivateKeySigner::random(NETWORK);
    let sender_address = sender_secret.address().clone();
    println!("Sender address: {sender_address}");
    // Don't print secrets in production code!
    println!(
        "Sender secrets: {} | {}",
        sender_secret.credentials().account_secret().reveal(),
        sender_secret.credentials().view_only_secret().reveal()
    );
    let account_component_addr = derive_account_address_from_public_key(sender_address.account_public_key());
    println!("Sender account address: {account_component_addr}");

    let another_signer = PrivateKeySigner::random(NETWORK);
    let another_address = another_signer.address().clone();

    let mut wallet = OotleWallet::from(sender_secret);
    wallet.register_signer(another_signer);

    let mut provider = ProviderBuilder::new()
        .with_network(Network::LocalNet)
        .wallet(wallet)
        .connect(API_URL)
        .await
        .unwrap();

    // Get the network (must be the same as the network specified in the builder).
    let network = provider.get_network().await.unwrap();
    println!("Provider network ID: {network}");
    assert_eq!(network, provider.network());
    // Get the latest block number.
    let latest_epoch = provider.get_epoch().await.unwrap();
    println!("Latest epoch: {latest_epoch}");

    // First let's transfer some faucet XTR to our account to have funds for fees and transfers.
    let unsigned_tx = IFaucet::new(&provider)
        .take_faucet_funds(10 * ONE_XTR)
        // NOTE that pay fee must be called after the faucet funds are taken
        .pay_fee(500)
        .prepare()
        .await
        .expect("Failed to prepare faucet transaction");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();

    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    println!("Taking faucet funds in tx {}", pending_tx.tx_id());
    let outcome = pending_tx.watch().await.unwrap();
    if !outcome.is_commit() {
        panic!("Faucet transaction failed: {outcome}");
    }
    println!("Faucet funds received: {}", outcome);

    // Then we'll send it to some other addresses.
    let recipient1 = address!("otl_loc_1sf0y0v7zgf62mqckytcg8esgm75ae9hfdhcdnhc6942caex2dqz350dlu824h8tj4thm3xnny77z26j3qrhquklguq0q7vawp8k4gcguz2j4c");
    let recipient2 = address!("otl_loc_1a2dcf306wgm4ce088fn9hqetxg434gesqvs3e37p2awfhrj8mp22ptu8l8xg3kddh8arzypyy3526extyqy0fheeckrl9ha0zm9rwysjvvch7");
    // Send some XTR to another address. You can replace XTR with any other fungible token resource address.
    let xtr_token = XTR; // resource_address!("resource_deadbeaf");

    let unsigned_tx = IAccount::new(&provider)
        .pay_fee(2000)
        // Multiple transfers in a single transaction
        .public_transfer(&recipient1, xtr_token, 2 * ONE_XTR)
        .public_transfer(&recipient2, xtr_token, ONE_XTR)
        .prepare()
        .await
        .expect("Failed to prepare transaction");

    let result = provider.send_dry_run(unsigned_tx.clone()).await.unwrap();
    let _diff = result.expect_success();
    println!("Estimated fees: {}", result.finalize.fee_receipt.total_fees_paid());

    // Another authorization can be added if there are additional signers required.
    // This is not required for this transaction as the sender is the only required signer, but is shown here for
    // demonstration.
    // NOTE: that you should not change the unsigned transaction after authorization as it will invalidate the
    // signature.
    let authorization = provider
        .wallet()
        .authorize_transaction(&another_address, &unsigned_tx)
        .await
        .unwrap();

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .add_authorization(authorization)
        .build(provider.wallet())
        .await
        .unwrap();

    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    println!("Pending transaction... {}", pending_tx.tx_id());
    let outcome = pending_tx.watch().await.unwrap();
    println!("Transaction finalized with outcome: {:?}", outcome);

    // Wait for the transaction to be finalized and get the receipt.
    let receipt = pending_tx.get_receipt().await.unwrap();
    println!("Transaction executed in epoch {}", receipt.epoch);
    println!("Transaction {} sent : {:?} ", pending_tx.tx_id(), receipt.outcome);
    // println!("From: {}", receipt.);
    // println!("To: {}", receipt.to.as_ref().expect("No recipient"));
    println!("Gas used: {}", receipt.fee_receipt.total_fees_paid());
    for log in receipt.logs {
        println!("Log: {}", log);
    }
    for event in receipt.events {
        println!("Event: {}", event);
    }
}
