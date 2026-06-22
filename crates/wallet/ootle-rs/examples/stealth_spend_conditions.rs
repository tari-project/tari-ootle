//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! # Stealth spend conditions — condition trees (MAST) with builtin predicates (TIP-0006)
//!
//! A normal stealth output is spent via the **key path** (a one-time `spend_key`). A TIP-0006 output can instead
//! commit a **condition tree** — a Merklized set of alternative spend conditions (a MAST). The output stores only the
//! Merkle **root**; a spender later reveals exactly ONE leaf plus an inclusion proof, and the engine evaluates it.
//!
//! Each leaf can use **builtin predicates** ([`BuiltinPredicate`]) — native, consensus-fixed primitives that need no
//! deployed template: timelocks (`AfterEpoch`/`BeforeEpoch`), a `HashLock`, and value covenants. Leaves are combined
//! into a conjunction with [`SpendCondition::All`].
//!
//! This example posts **two** transactions on localnet (default indexer `http://127.0.0.1:12500`):
//!   1. **lock** — takes free coins from the faucet and locks them, paid to our own account, behind a **hash
//!      time-locked contract (HTLC)** condition tree:
//!        - **claim**  — `All([HashLock(secret), BeforeEpoch(deadline)])`: spendable by revealing the preimage before
//!          the deadline,
//!        - **refund** — `AfterEpoch(deadline)`: spendable once the deadline passes;
//!   2. **claim** — spends that HTLC output via the **script path**, revealing the claim leaf, its inclusion proof, and
//!      the preimage as witness `data`.
//!
//! ```bash
//! cargo run -p ootle-rs --example stealth_spend_conditions
//! ```

use ootle_rs::{
    Network,
    TransactionRequest,
    builtin_templates::{UnsignedTransactionBuilder, faucet::IFaucet},
    const_nonzero_u64,
    crypto::stealth::{hashlock_digest, script_path_witness_with_data},
    default_indexer_url,
    key_provider::PrivateKeyProvider,
    provider::{PendingTransaction, Provider, ProviderBuilder, WalletProvider},
    stealth::{Output, StealthTransfer},
    template_types::{
        UtxoAddress,
        bytes::Bytes,
        constants::TARI_TOKEN,
        stealth::{BuiltinPredicate, HashAlg, SpendCondition, StealthInput},
    },
    transaction::TransactionSigner,
    wallet::OotleWallet,
};
use tari_ootle_common_types::engine_types::transaction_receipt::TransactionReceipt;
use tari_ootle_transaction::Transaction;

/// 1 TARI expressed in microTARI.
const ONE_TARI: u64 = 1_000_000;
const LOCKED_AMOUNT: u64 = 10 * ONE_TARI;
const FEE: u64 = 2000;

#[tokio::main]
async fn main() {
    let network = Network::LocalNet;
    let me = PrivateKeyProvider::random(network);
    let my_address = me.address().clone();
    println!("Account: {my_address}");

    let wallet = OotleWallet::from(me.clone());
    let mut provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(default_indexer_url(network))
        .await
        .unwrap();

    let current_epoch = provider.get_epoch().await.unwrap().as_u64();
    println!("Current epoch: {current_epoch}");

    // -------------------------------------------------------------------------------------------------
    // Build the HTLC condition tree (MAST) from builtin predicates.
    // -------------------------------------------------------------------------------------------------
    // The claimant must reveal a preimage whose SHA-256 digest matches `hash`. `SHA-256` (rather than the native
    // Blake2b) lets the same secret unlock an HTLC on an external chain (e.g. Bitcoin).
    let preimage = b"open sesame - the answer is 42";
    let hash = hashlock_digest(HashAlg::Sha256, preimage);

    // The claim window closes `deadline` epochs from now; after it, the refund path opens.
    let deadline = current_epoch + 50;

    let claim = SpendCondition::All(Box::new([
        SpendCondition::Builtin(BuiltinPredicate::HashLock {
            hash,
            alg: HashAlg::Sha256,
        }),
        SpendCondition::Builtin(BuiltinPredicate::BeforeEpoch(deadline)),
    ]));
    let refund = SpendCondition::Builtin(BuiltinPredicate::AfterEpoch(deadline));
    let conditions = vec![claim, refund];

    println!("\nHTLC condition tree ({} leaves):", conditions.len());
    println!("  preimage (the secret): {:?}", std::str::from_utf8(preimage).unwrap());
    println!("  SHA-256 hashlock      : {hash}");
    println!("  claim   : All([HashLock(secret), BeforeEpoch({deadline})])");
    println!("  refund  : AfterEpoch({deadline})");
    // NOTE: these leaves are not bound to a key, so anyone who learns the preimage (or waits for the refund epoch) can
    // spend. For a real HTLC, AND each path with an `AccessRule` requiring the claimant's / refunder's key by adding it
    // to the `SpendCondition::All` list.

    let tari_token = TARI_TOKEN;

    // -------------------------------------------------------------------------------------------------
    // TX 1 — lock: take free coins from the faucet and pay them to ourselves behind the HTLC.
    // -------------------------------------------------------------------------------------------------
    let (lock_transfer, required_signers) = StealthTransfer::new(tari_token, &provider)
        .spend_revealed_input(LOCKED_AMOUNT + FEE)
        .to_revealed_output(FEE)
        .to_stealth_output(
            Output::new(my_address.clone(), tari_token, const_nonzero_u64!(LOCKED_AMOUNT))
                .with_spend_conditions(conditions.clone())
                .with_memo_message("HTLC: reveal the preimage before the deadline epoch to claim"),
        )
        .prepare()
        .await
        .unwrap();

    // The commitment of the HTLC output we are about to create — the input we will claim in TX 2.
    let htlc_commitment = *lock_transfer.stealth_outputs()[0].commitment();

    let lock_tx = IFaucet::new(&provider)
        .take_faucet_funds()
        .into_stealth_transfer(lock_transfer)
        .and_pay_fee_from_revealed_output()
        .prepare()
        .await
        .expect("Failed to prepare lock transaction");

    let authorizer = provider.wallet().stealth_authorizer(required_signers);
    let transaction = TransactionRequest::default()
        .with_transaction(lock_tx)
        .build(&authorizer)
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    print_results("Lock (faucet → HTLC output)", &pending_tx).await;

    // -------------------------------------------------------------------------------------------------
    // TX 2 — claim: spend the HTLC output via its script path, revealing the preimage.
    // -------------------------------------------------------------------------------------------------
    // Reveal the `claim` leaf (index 0) with its inclusion proof against the committed root, and supply the preimage as
    // the witness `data` the hashlock consumes.
    let (claim_witness, _root) =
        script_path_witness_with_data(&conditions, &conditions[0], Bytes::from_vec(preimage.to_vec()))
            .expect("claim leaf is a member of the condition tree");
    let htlc_input = StealthInput::with_witness(htlc_commitment, claim_witness);

    let (claim_transfer, required_signers) = StealthTransfer::new(tari_token, &provider)
        .spend_stealth_input(my_address.clone(), htlc_input)
        .to_revealed_output(FEE)
        .to_stealth_output(Output::new(
            my_address,
            tari_token,
            const_nonzero_u64!(LOCKED_AMOUNT - FEE),
        ))
        .prepare()
        .await
        .unwrap();

    let claim_tx = Transaction::builder(provider.network())
        .with_fee_instructions_builder(|builder| {
            builder
                .stealth_transfer(tari_token, claim_transfer)
                .put_last_instruction_output_on_workspace("fees")
                .pay_fee_from_bucket("fees")
        })
        .add_input(tari_token)
        // The HTLC UTXO being spent — DOWNed if the transaction succeeds.
        .add_input(UtxoAddress::new(tari_token, htlc_commitment.into()))
        .build_unsigned();

    let authorizer = provider.wallet().stealth_authorizer(required_signers);
    let transaction = TransactionRequest::default()
        .with_transaction(claim_tx)
        .build(&authorizer)
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    print_results("Claim (reveal preimage)", &pending_tx).await;
}

async fn print_results(label: &str, pending_tx: &PendingTransaction) -> TransactionReceipt {
    println!("\n⌛️ {label} transaction pending... {}", pending_tx.tx_id());
    let outcome = pending_tx.watch().await.unwrap();
    println!("🏁 Transaction Finalized {}", pending_tx.tx_id());
    println!("✅ Outcome: {outcome:?}");

    let receipt = pending_tx.get_receipt().await.unwrap();
    println!("-------------------------------------------");
    println!("🔹 Epoch: {}", receipt.epoch);
    println!("🔹 Transaction ID: {}", pending_tx.tx_id());
    println!("🔹 Outcome: {:?}", receipt.outcome);
    println!("🔹 Fees Paid: {}", receipt.fee_receipt.total_fees_paid());
    println!("-------------------------------------------");
    receipt
}
