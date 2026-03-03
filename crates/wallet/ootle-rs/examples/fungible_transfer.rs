//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_rs::{
    ToAccountAddress,
    TransactionRequest,
    address,
    builtin_templates::{UnsignedTransactionBuilder, account::IAccount, faucet::IFaucet},
    default_indexer_url,
    key_provider::PrivateKeyProvider,
    keys::HasViewOnlyKeySecret,
    provider::{PendingTransaction, Provider, ProviderBuilder, WalletProvider},
    transaction::TransactionSigner,
    wallet::OotleWallet,
};
use tari_ootle_common_types::{Network, displayable::Displayable};
use tari_template_lib_types::constants::{TARI, TARI_TOKEN};

#[tokio::main]
async fn main() {
    // env_logger::builder()
    //     .filter_level(tracing::log::LevelFilter::Debug)
    //     .init();

    const NETWORK: Network = Network::LocalNet;

    let indexer_api_url = default_indexer_url(NETWORK);

    let sender_secret = PrivateKeyProvider::random(NETWORK);
    let sender_address = sender_secret.address().clone();
    println!("Sender address: {sender_address}");
    // Don't print secrets in production code!
    println!(
        "Sender secrets: {} | {}",
        sender_secret.credentials().account_secret().reveal(),
        sender_secret.credentials().view_only_secret().reveal()
    );
    let mut wallet = OotleWallet::from(sender_secret);

    let account_component_addr = sender_address.to_account_address();
    println!("Sender account address: {account_component_addr}");

    let another_signer = PrivateKeyProvider::random(NETWORK);
    let another_address = another_signer.address().clone();

    wallet.register_key_provider(another_signer);

    let mut provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(indexer_api_url)
        .await
        .unwrap();

    // Get the network (must be the same as the network specified in the builder).
    let network = provider.get_network().await.unwrap();
    println!("Provider network ID: {network}");
    assert_eq!(network, provider.network());
    // Get the latest block number.
    let latest_epoch = provider.get_epoch().await.unwrap();
    println!("Latest epoch: {latest_epoch}");

    // First let's transfer some faucet TARI to our account to have funds for fees and transfers.
    let unsigned_tx = IFaucet::new(&provider)
        .take_faucet_funds(10 * TARI)
        // NOTE that pay fee must be called after the faucet funds are taken because fees are paid from the faucet funds
        .pay_fee(500u64)
        .prepare()
        .await
        .expect("Failed to prepare faucet transaction");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();

    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    print_fancy_results(&pending_tx).await;

    // Then we'll send it to some other addresses.
    let recipient1 = address!(
        "otl_loc_10mc0v2lyy43kldl0ft4c2x5pe7j0ckduv8zej6jgr2z2g9m07fz7gl96ar5wwgu0qu0atmr5tl53ye7n38xr5u7ytlmudq0ruxcau0gge7rxk"
    );
    let recipient2 = address!(
        "otl_loc_1y2s6442wau8v72pdrr5h4kntrqppqndqug33dmqv7eqkvx5c7ue2gzrw6v56kzkhnr7l025ye3jt3gmzmunmxy6vpm573fdduw37vcc848dcz"
    );
    // Send some TARI to another address. You can replace TARI_TOKEN with any other fungible token resource address.
    let tari_token = TARI_TOKEN; // resource_address!("resource_deadbeaf");

    let unsigned_tx = IAccount::new(&provider)
        .pay_fee(1000u64)
        // Multiple transfers in a single transaction
        .public_transfer(&recipient1, tari_token, 2 * TARI)
        .public_transfer(&recipient2, tari_token, TARI)
        .prepare()
        .await
        .expect("Failed to prepare transaction");

    let result = provider.sign_and_send_dry_run(unsigned_tx.clone()).await.unwrap();
    let _diff = result.expect_success();
    println!("Dry run successful!");
    println!(
        "Estimated fees for transfer: {}",
        result.finalize.fee_receipt.total_fees_charged()
    );

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
    print_fancy_results(&pending_tx).await;
}

async fn print_fancy_results(pending_tx: &PendingTransaction) {
    println!("⌛️ Pending transaction... {}", pending_tx.tx_id());
    let outcome = pending_tx.watch().await.unwrap();
    println!("🏁 Transaction Finalized {}", pending_tx.tx_id());

    println!("✅ Outcome: {:?}", outcome);

    // Wait for the transaction to be finalized and get the receipt.
    let receipt = pending_tx.get_receipt().await.unwrap();
    println!("-------------------------------------------");
    println!("  Transaction Receipt");
    println!("-------------------------------------------");
    println!("🔹 Epoch: {}", receipt.epoch);
    println!("🔹 Transaction ID: {}", pending_tx.tx_id());
    println!("🔹 Outcome: {:?}", receipt.outcome);
    println!("🔹 Fees Paid: {}", receipt.fee_receipt.total_fees_paid());

    if !receipt.logs.is_empty() {
        println!("\n🪵 Logs:");
        for log in receipt.logs {
            println!("  - {}", log);
        }
    }

    if !receipt.events.is_empty() {
        println!("\n🎉 Events:");
        for event in receipt.events {
            println!("  - Substate ID: {}", event.substate_id().display());
            println!("    Template Address: {}", event.template_address());
            println!("    Topic: {}", event.topic());
            println!("    Payload: {{{}}}", event.payload());
            println!();
        }
    }
    println!("-------------------------------------------");
}
