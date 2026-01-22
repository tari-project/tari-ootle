//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use cucumber::{gherkin::Step, then, when};
use integration_tests::{
    claim_proof::CucumberClaimProof,
    cucumber_log,
    util::transaction_builder,
    wallet_daemon_client,
    TariWorld,
};
use rand::{rngs::OsRng, Rng};
use tari_crypto::tari_utilities::ByteArray;
use tari_engine_types::commit_result::FinalizeResult;
use tari_ootle_transaction::args;
use tari_ootle_wallet_sdk::models::KeyBranch;
use tari_template_lib::{
    constants::XTR,
    types::{bytes::Bytes, crypto::PedersenCommitmentBytes, Amount},
};
use tari_transaction_components::{
    tari_amount::T,
    transaction_components::{memo_field::TxType, MemoField},
};
use tari_wallet_daemon_client::{
    types::{TransactionSubmitRequest, TransactionWaitResultRequest},
    ComponentAddressOrName,
};

async fn claim_burn(
    world: &mut TariWorld,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) -> anyhow::Result<FinalizeResult> {
    let claim_proof = world
        .claim_proofs
        .get(&proof_name)
        .unwrap_or_else(|| panic!("Burn proof {} not found", proof_name));
    let claim_proof = claim_proof
        .confirmed()
        .unwrap_or_else(|| panic!("Burn proof {} is not confirmed, cannot claim burn", proof_name));

    cucumber_log!("Claiming burn with proof: {:?}", claim_proof);
    let walletd = world.get_wallet_daemon(&wallet_daemon_name);
    // Then burn into the new account
    let claim_burn_resp = walletd.claim_burn(&account_name, claim_proof.clone()).await?;
    let resp = walletd
        .wait_for_transaction_result(claim_burn_resp.transaction_id)
        .await;
    assert!(!resp.timed_out, "Timed out waiting for claim burn transaction result");
    Ok(resp.result.expect("transaction result is None when claiming burn"))
}

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}")]
#[then(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}")]
async fn when_i_claim_burn_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap();
    if let Some(ref reason) = result.any_reject() {
        cucumber_log!("Transaction failed: {}", reason);
        panic!("Transaction failed: {}", reason);
    }
}

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}, it fails")]
async fn when_i_claim_burn_via_wallet_daemon_it_fails(
    world: &mut TariWorld,
    step: &Step,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap();

    assert!(
        result.any_reject().is_some(),
        "Expected transaction to fail, but it succeeded: {:?}",
        result
    );
}

#[when(expr = "I claim fees for validator {word} into account {word} using the wallet daemon {word}")]
async fn when_i_claim_fees_for_validator_and_epoch(
    world: &mut TariWorld,
    step: &Step,
    validator_node: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let resp = wallet_daemon_client::claim_fees(world, wallet_daemon_name, account_name, validator_node, false)
        .await
        .unwrap();
    resp.result.result.any_accept().unwrap_or_else(|| {
        panic!(
            "Expected fee claim to succeed but failed with {}",
            resp.result.result.fee_reject().unwrap()
        )
    });
}

#[then(expr = "I run up {int} in fees using the wallet daemon {word} and account {word}")]
async fn when_i_run_up_fees(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    wallet_daemon_name: String,
    account_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let template = world
        .templates
        .get("fees")
        .expect("fees template must be registered before this step can be used");
    let account = world
        .wallet_accounts
        .get(&account_name)
        .unwrap_or_else(|| panic!("No account named {}", account_name));

    let mut fees_total = 0;

    loop {
        let payload = Bytes::from(vec![OsRng.gen::<u8>(); 64 * 1024]);

        let transaction = transaction_builder()
            .pay_fee_from_component(*account.component_address(), 100_000)
            .call_function(template.address, "new", args![payload])
            .add_input(*account.component_address())
            .build_unsigned_transaction();

        let transaction_submit_req = TransactionSubmitRequest {
            transaction,
            seal_signer: account.owner_key_id().expect("no owner key id"),
            other_signers: vec![],
            detect_inputs: true,
            detect_inputs_use_unversioned: true,
            lock_ids: vec![],
        };

        let walletd = world.get_wallet_daemon(&wallet_daemon_name);
        let mut client = walletd.get_authed_client().await;
        let resp = client.submit_transaction(transaction_submit_req).await.unwrap();

        let wait_req = TransactionWaitResultRequest {
            transaction_id: resp.transaction_id,
            timeout_secs: Some(120),
        };
        let wait_resp = client.wait_transaction_result(wait_req).await.unwrap();
        if wait_resp.timed_out {
            panic!("Timed out waiting for transaction result");
        }
        if let Some(reason) = wait_resp.result.as_ref().unwrap().any_reject() {
            panic!("Transaction failed: {}", reason);
        }

        fees_total += wait_resp.result.as_ref().unwrap().fee_receipt.total_fees_paid();
        if fees_total >= amount {
            integration_tests::cucumber_log!("Reached target of {} fees", fees_total);
            break;
        }
        integration_tests::cucumber_log!("Accumulated {} fees, continuing", fees_total);
    }
}

#[when(expr = "I claim fees for validator {word} into account {word} using the wallet daemon {word}, it fails")]
async fn when_i_claim_fees_for_validator_and_epoch_fails(
    world: &mut TariWorld,
    step: &Step,
    validator_node: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let err = wallet_daemon_client::claim_fees(world, wallet_daemon_name, account_name, validator_node, false)
        .await
        .unwrap_err();

    println!("Expected error: {}", err);
}

#[then(expr = "I create an account {word} via the wallet daemon {word}")]
#[when(expr = "I create an account {word} via the wallet daemon {word}")]
async fn when_i_create_account_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    wallet_daemon_client::create_account(world, wallet_daemon_name, account_name).await;
}

#[then(expr = "I create an account {word} via the wallet daemon {word} with {int} XTR")]
#[when(expr = "I create an account {word} via the wallet daemon {word} with {int} XTR")]
async fn when_i_create_account_via_wallet_daemon_with_free_coins(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    wallet_daemon_name: String,
    amount: i64,
) {
    cucumber_log!("==== Step: {}", step.value);
    wallet_daemon_client::create_account_with_free_coins(world, account_name, wallet_daemon_name, amount * 1_000_000)
        .await;
}

#[when(expr = "I burn {int}T on wallet {word} for wallet daemon {word} into proof {word}")]
async fn when_i_burn_funds_with_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    wallet_name: String,
    wallet_daemon_name: String,
    proof_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let mut wallet_daemon_client =
        wallet_daemon_client::get_auth_wallet_daemon_client(world, &wallet_daemon_name).await;

    let nonce = wallet_daemon_client.create_key(KeyBranch::Nonce).await.unwrap();
    // let private_ephemeral_key = RistrettoSecretKey::random(&mut OsRng);
    // let public_key = private_ephemeral_key.public_key();

    let public_key = nonce.public_key;
    integration_tests::cucumber_log!("Burning funds using claim key {}", public_key);

    let wallet = world
        .wallets
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("Wallet {} not found", wallet_name));

    let amount = (amount * T).as_u64();
    let mut client = wallet.create_client().await;
    let resp = client
        .create_burn_transaction(minotari_app_grpc::tari_rpc::CreateBurnTransactionRequest {
            amount,
            fee_per_gram: 1,
            payment_id: MemoField::new_open_from_string("Burn", TxType::Burn)
                .unwrap()
                .to_bytes(),
            claim_public_key: public_key.to_vec(),
            sidechain_deployment_key: vec![],
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.is_success, "Burn transaction failed: {}", resp.failure_message);

    let commitment = PedersenCommitmentBytes::from_bytes(&resp.commitment).expect("commitment parse error");

    // Extract kernel signature data
    let kernel_excess_sig_nonce = resp.kernel_excess_nonce.clone();
    let kernel_excess_sig_signature = resp.kernel_excess_signature.clone();

    integration_tests::cucumber_log!(
        "Burn transaction created with kernel_excess_sig nonce: {}, signature: {}",
        hex::encode(&kernel_excess_sig_nonce),
        hex::encode(&kernel_excess_sig_signature)
    );

    world.claim_proofs.insert(proof_name, CucumberClaimProof::Pending {
        commitment,
        nonce_id: nonce.id,
        kernel_excess_sig_nonce,
        kernel_excess_sig_signature,
    });
}

#[when(regex = r"I check the balance of (\S+) on wallet daemon (\S+) the amount is (at )?(\S+) (\d+)")]
async fn check_account_balance_via_daemon(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    wallet_daemon_name: String,
    _at: String,
    least_or_most: String,
    amount: i64,
) {
    cucumber_log!("==== Step: {}", step.value);
    // This also refreshes the wallet vaults
    let current_balance = wallet_daemon_client::get_balance(world, &account_name, &wallet_daemon_name, XTR).await;
    match least_or_most.to_lowercase().as_str() {
        "least" => {
            if current_balance < amount {
                println!("Expected balance to be at least {} but was {}", amount, current_balance);
                panic!("Expected balance to be at least {} but was {}", amount, current_balance);
            }
        },
        "most" => {
            if current_balance > amount {
                println!("Expected balance to be at most {} but was {}", amount, current_balance);
                panic!("Expected balance to be at most {} but was {}", amount, current_balance);
            }
        },
        "exactly" => {
            if current_balance != amount {
                println!("Expected balance to be exactly {} but was {}", amount, current_balance);
                panic!("Expected balance to be exactly {} but was {}", amount, current_balance);
            }
        },

        _ => panic!("Expected 'at least', 'at most' or 'exactly', got {}", least_or_most),
    }
}

#[when(
    regex = r"I check the balance of (\S+) for resource (\S+) on wallet daemon (\S+) the amount is (at )?(\S+) (\d+)"
)]
async fn check_account_balance_for_resource_via_daemon(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    resource_input_name: String,
    wallet_daemon_name: String,
    _at: String,
    least_or_most: String,
    amount: i64,
) {
    cucumber_log!("==== Step: {}", step.value);
    let output = world.get_output_fq(&resource_input_name);
    // This also refreshes the wallet vaults
    let current_balance = wallet_daemon_client::get_balance(
        world,
        &account_name,
        &wallet_daemon_name,
        output
            .substate_id
            .as_resource_address()
            .expect("output is not resource"),
    )
    .await;
    match least_or_most.to_lowercase().as_str() {
        "least" => {
            if current_balance < amount {
                println!("Expected balance to be at least {} but was {}", amount, current_balance);
                panic!("Expected balance to be at least {} but was {}", amount, current_balance);
            }
        },
        "most" => {
            if current_balance > amount {
                println!("Expected balance to be at most {} but was {}", amount, current_balance);
                panic!("Expected balance to be at most {} but was {}", amount, current_balance);
            }
        },
        "exactly" => {
            if current_balance != amount {
                println!("Expected balance to be exactly {} but was {}", amount, current_balance);
                panic!("Expected balance to be exactly {} but was {}", amount, current_balance);
            }
        },

        _ => panic!("Expected 'at least', 'at most' or 'exactly', got {}", least_or_most),
    }
}

#[when(expr = "I wait for {word} on wallet daemon {word} to have balance {word} {int}")]
#[then(expr = "I wait for {word} on wallet daemon {word} to have balance {word} {int}")]
async fn wait_account_balance_via_daemon(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    wallet_daemon_name: String,
    operator: String,
    amount: i64,
) {
    cucumber_log!("==== Step: {}", step.value);
    let op = match operator.as_str() {
        "gt" => |a, b| a > b,
        "gte" => |a, b| a >= b,
        "lt" => |a, b| a < b,
        "lte" => |a, b| a <= b,
        "eq" => |a, b| a == b,
        _ => panic!("Expected gt, gte, lt, lte or eq, got {}", operator),
    };

    let mut i = 0;
    loop {
        // This also refreshes the wallet vaults
        let current_balance = wallet_daemon_client::get_balance(world, &account_name, &wallet_daemon_name, XTR).await;
        if op(current_balance, amount) {
            break;
        }

        i += 1;
        if i == 30 {
            panic!("Timeout waiting for balance. Current balance = {}", current_balance);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[when(expr = "I check the confidential balance of {word} on wallet daemon {word} the amount is at {word} {int}")]
async fn check_account_confidential_balance_is_via_daemon(
    world: &mut TariWorld,
    step: &Step,
    account_name: String,
    wallet_daemon_name: String,
    least_or_most: String,
    amount: i64,
) {
    cucumber_log!("==== Step: {}", step.value);
    // This also refreshes the wallet vaults
    let current_balance = wallet_daemon_client::get_confidential_balance(world, account_name, wallet_daemon_name).await;
    match least_or_most.to_lowercase().as_str() {
        "least" => {
            if current_balance < amount {
                println!("Expected balance to be at least {} but was {}", amount, current_balance);
                panic!("Expected balance to be at least {} but was {}", amount, current_balance);
            }
        },
        "most" => {
            if current_balance > amount {
                println!("Expected balance to be at most {} but was {}", amount, current_balance);
                panic!("Expected balance to be at most {} but was {}", amount, current_balance);
            }
        },
        _ => panic!("Expected least or most, got {}", least_or_most),
    }
}

#[when(
    regex = r"I transfer (\d+) tokens of resource (\S+) from account (\S+) to account (\S+) via the wallet daemon (\S+) named (\S+)"
)]
async fn when_transfer_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    amount: i32,
    resource_name: String,
    account_name: String,
    dest_account: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let amount = Amount::new(amount);

    let resource_address = world
        .get_output_fq(&resource_name)
        .substate_id()
        .as_resource_address()
        .unwrap_or_else(|| panic!("{} is not a resource", resource_name));

    let destination_account = world
        .wallet_accounts
        .get(&dest_account)
        .unwrap_or_else(|| panic!("No account address found with name {}", dest_account));
    wallet_daemon_client::transfer(
        world,
        account_name,
        *destination_account.address.account_public_key(),
        resource_address,
        amount,
        wallet_daemon_name,
        outputs_name,
    )
    .await;
}

#[then(
    expr = "I do a stealth transfer with amount {int} from {word} to {word} creating output {word} via the \
            wallet_daemon {word}"
)]
async fn when_i_create_transfer_proof_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    source_account_name: String,
    dest_account_name: String,
    outputs_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    wallet_daemon_client::transfer_stealth(
        world,
        source_account_name,
        dest_account_name,
        amount,
        wallet_daemon_name,
        outputs_name,
        // TODO: support for custom stealth resources
        XTR,
    )
    .await;
}

#[when(
    expr = "I do a stealth transfer of {int} from account {word} to account {word} via the wallet daemon {word} named \
            {word}"
)]
async fn when_stealth_transfer_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    account_name: String,
    destination_acc_name: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    wallet_daemon_client::transfer_stealth(
        world,
        account_name,
        destination_acc_name,
        amount,
        wallet_daemon_name,
        outputs_name,
        XTR,
    )
    .await;
}

#[when(expr = "I set the default account for {word} to {word}")]
async fn when_i_set_the_default_account(world: &mut TariWorld, step: &Step, wallet_name: String, account_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let wallet = world
        .wallet_daemons
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("No wallet daemon named {}", wallet_name));
    let mut client = wallet.get_authed_client().await;
    client
        .accounts_set_default(ComponentAddressOrName::Name(account_name))
        .await
        .unwrap();
}
