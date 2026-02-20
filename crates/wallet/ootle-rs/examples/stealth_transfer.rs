//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_rs::{
    ToAccountAddress,
    TransactionRequest,
    builtin_templates::{UnsignedTransactionBuilder, faucet::IFaucet},
    const_nonzero_u64,
    default_indexer_url,
    key_provider::PrivateKeyProvider,
    keys::HasViewOnlyKeySecret,
    provider::{PendingTransaction, Provider, ProviderBuilder, WalletProvider},
    stealth::{Output, StealthTransfer},
    transaction::TransactionSigner,
    wallet::OotleWallet,
};
use tari_ootle_address::address;
use tari_ootle_common_types::{
    Network,
    displayable::Displayable,
    engine_types::transaction_receipt::TransactionReceipt,
};
use tari_ootle_transaction::Transaction;
use tari_template_lib_types::{
    UtxoAddress,
    constants::{ONE_XTR, XTR},
};

#[tokio::main]
async fn main() {
    // env_logger::builder()
    //     .filter_level(tracing::log::LevelFilter::Debug)
    //     .init();

    const NETWORK: Network = Network::LocalNet;

    let indexer_api_url = default_indexer_url(NETWORK);
    // This is the address that we will transfer to (Feel free to change this another address!)
    let recipient = address!(
        "otl_loc_10mc0v2lyy43kldl0ft4c2x5pe7j0ckduv8zej6jgr2z2g9m07fz7gl96ar5wwgu0qu0atmr5tl53ye7n38xr5u7ytlmudq0ruxcau0gge7rxk"
    );

    let sender_secret = PrivateKeyProvider::random(NETWORK);
    let sender_address = sender_secret.address().clone();
    println!("Sender address: {sender_address}");
    // Don't print secrets in production code!
    println!(
        "Sender secrets: {} | {}",
        sender_secret.credentials().account_secret().reveal(),
        sender_secret.credentials().view_only_secret().reveal()
    );
    let account_component_addr = sender_address.to_account_address();
    println!("Sender account address: {account_component_addr}");

    let wallet = OotleWallet::from(sender_secret.clone());

    let mut provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(indexer_api_url)
        .await
        .unwrap();

    // Get the network from the indexer (must be the same as the network specified in the builder).
    let network = provider.get_network().await.unwrap();
    println!("Provider network ID: {network}");
    assert_eq!(network, provider.network());
    // Get the latest block number.
    let latest_epoch = provider.get_epoch().await.unwrap();
    println!("Latest epoch: {latest_epoch}");

    // Send some XTR to another address. You can replace XTR with any other fungible token resource address.
    let xtr_token = XTR; // resource_address!("resource_0123456789abcdef...");

    const INPUT_AMOUNT: u64 = 10 * ONE_XTR + 1000 - 500;
    // This builder creates a stealth transfer statement (spend proof). This is added to the transaction later.
    let (faucet_transfer, required_signers) = StealthTransfer::new(xtr_token, &provider)
        // Tell the transfer to expect 10XTR (+1000 to cover fees) as revealed funds from a bucket (the faucet looks at this value and automatically provides the bucket).
        .spend_revealed_input(10 * ONE_XTR + 1000)
        // The transfer will output 500 micro XTR as revealed funds to pay for the fee
        .to_revealed_output(500u64)
        // Spend the remaining value (10XTR - fee) into an output for the sender address. NOTE: the sender address is not actually included in the output (privacy!),
        // but a supporting wallet that holds the secret key would be able to spend the output.
        // You can specify any address here and split up into many outputs as needed, as long as ∑inputs == ∑outputs.
        .to_stealth_output(
            Output::new(sender_address.clone(), xtr_token, const_nonzero_u64!(INPUT_AMOUNT))
        )
        .prepare()
        .await
        .unwrap();

    // Keep track of the input commitments to spend later.
    let input_to_spend = faucet_transfer.stealth_outputs().to_vec();

    // First let's transfer some faucet XTR to our account to have funds for fees and transfers.
    let unsigned_tx = IFaucet::new(&provider)
        .take_faucet_funds_stealth(faucet_transfer, true)
        .prepare()
        .await
        .expect("Failed to prepare faucet transaction");

    // This authorizer adds the required (stealth) signatures to spend inputs
    let authorizer = provider.wallet().stealth_authorizer(required_signers);

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(&authorizer)
        .await
        .unwrap();

    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    print_fancy_results("Faucet transfer", &pending_tx).await;

    // Then we'll send it to the recipient
    // This builder creates a stealth transfer statement (spend proof). This is added to the transaction later.
    let (transfer, required_signers) = StealthTransfer::new(xtr_token, &provider)
        // Spend an existing stealth input that is controlled by the sender address.
        // This is worth 10.000500 XTR
        .spend_stealth_input(sender_address.clone(), input_to_spend[0].commitment())
        // The transfer will output 0.000500 XTR as revealed funds to pay for the fee
        .to_revealed_output(500u64)
        // Spend to a new output (8 XTR) that we'll generate for the recipient address.
        .to_stealth_output(
            Output::new(recipient, xtr_token, const_nonzero_u64!(8 * ONE_XTR))
                // NOTE: this memo is stored on-chain, and longer memos increase fees. It is encrypted so that only the recipient can read it.
                .with_memo_message("transfer from ootle-rs!")
        )
        // Send some change (2 XTR) back to ourselves (NOTE once this example exits, we'll lose the keys for this output!)
        .to_stealth_output(Output::new(sender_address, xtr_token, const_nonzero_u64!(2*ONE_XTR)))
        // Load the inputs from the provider to build the transfer statement. NOTE: this will error if the total input amounts != total output amounts.
        .prepare()
        .await
        .unwrap();

    // We'll generate an unsigned transaction directly using the Transaction builder. In future, we may make this
    // easier.
    let unsigned_tx = Transaction::builder(provider.network())
        .with_fee_instructions_builder(|builder| {
            builder
                .stealth_transfer(xtr_token, transfer)
                .put_last_instruction_output_on_workspace("fees")
                .pay_fee_from_bucket("fees")
        })
        // This isn't necessary because all transactions implicitly use XTR for fees, but you'd need to include this if other resources are being used
        .add_input(xtr_token)
        // Add the UTXO substate as an input. This will be DOWNed (destroyed) if the transaction is successful.
        .add_input(UtxoAddress::new(xtr_token, input_to_spend[0].commitment().into()))
        .build_unsigned();

    // This authorizer adds the required (stealth) signatures to spend inputs
    let authorizer = provider.wallet().stealth_authorizer(required_signers);

    let result = provider
        .sign_and_send_dry_run_with(&authorizer, unsigned_tx.clone())
        .await
        .unwrap();
    let _diff = result.expect_success();
    println!("Dry run successful!");
    println!(
        "Estimated fees for transfer: {}",
        result.finalize.fee_receipt.total_fees_charged()
    );
    let authorizations = authorizer.create_authorizations(&unsigned_tx).await.unwrap();
    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .with_authorizations(authorizations)
        .build(&authorizer)
        .await
        .unwrap();

    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    print_fancy_results("Stealth transfer", &pending_tx).await;
}

async fn print_fancy_results(label: &str, pending_tx: &PendingTransaction) -> TransactionReceipt {
    println!("⌛️ {label} transaction pending... {}", pending_tx.tx_id());
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
    let fee_receipt = &receipt.fee_receipt;
    println!("🔹 Fees Paid: {}", fee_receipt.total_fees_paid());
    println!(
        "🔹 Fees Overcharge: {} = {} (paid) - {} (charged) - {} (refunded)",
        fee_receipt.total_fee_overcharge(),
        fee_receipt.total_fees_paid(),
        fee_receipt.total_fees_charged(),
        fee_receipt.total_refunded()
    );

    if !receipt.logs.is_empty() {
        println!("\n🪵 Logs:");
        for log in receipt.logs() {
            println!("  - {}", log);
        }
    }

    if !receipt.events.is_empty() {
        println!("\n🎉 Events:");
        for event in receipt.events() {
            println!("  - Substate ID: {}", event.substate_id().display());
            println!("    Template Address: {}", event.template_address());
            println!("    Topic: {}", event.topic());
            println!("    Payload: {{{}}}", event.payload());
            println!();
        }
    }
    println!("-------------------------------------------");
    receipt
}
