//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, time::Duration};

use tari_common::configuration::Network;
use tari_common_types::types::PrivateKey;
use tari_engine_types::{
    commit_result::RejectReason,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{SubstateRequirement, optional::Optional};
use tari_ootle_transaction::{Transaction, builder::TransactionBuilder};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib_types::{NonFungibleId, TemplateAddress};
use tari_transaction_components::key_manager::{SecretTransactionKeyManagerInterface, TariKeyId};
use tari_validator_node_client::{
    ValidatorNodeClient,
    types::{GetTransactionResultRequest, SubmitTransactionRequest, SubmitTransactionResponse},
};
use tokio::time::{MissedTickBehavior, interval};

use crate::{TariWorld, helpers::get_component_from_namespace};

/// Creates a component by calling a template function
pub async fn create_component(
    world: &mut TariWorld,
    outputs_name: String,
    template_name: String,
    vn_name: String,
    function_call: String,
    args: Vec<String>,
) {
    let template_address = world
        .templates
        .get(&template_name)
        .unwrap_or_else(|| panic!("Template not found with name {}", template_name))
        .address;

    // Parse arguments from strings
    let parsed_args: Result<Vec<_>, _> = args.iter().map(|a| parse_arg(a)).collect();

    let parsed_args = parsed_args.expect("Failed to parse arguments");

    // Get signing key from world's key manager
    let secret_key = get_signing_key(world).await;

    // Build and sign the transaction (Network::LocalNet == 0u8)
    let transaction = TransactionBuilder::new(Network::LocalNet)
        .call_function(template_address, function_call, parsed_args)
        .with_inputs(vec![])
        .build_and_seal(&secret_key);

    // Submit transaction
    let mut client = world.get_validator_node(&vn_name).get_client();
    let resp = submit_and_wait_for_result(&mut client, transaction, Duration::from_secs(300))
        .await
        .unwrap();

    if let Some(ref failure) = resp.dry_run_result.as_ref().unwrap().finalize.fee_reject() {
        panic!("Transaction failed: {:?}", failure);
    }

    // Store the account component address and other substate ids for later reference
    add_outputs_from_diff(
        world,
        outputs_name,
        resp.dry_run_result.unwrap().finalize.result.any_accept().unwrap(),
    );
}

/// Extracts outputs from a substate diff and stores them in the world for later reference
#[expect(clippy::too_many_lines)]
pub(crate) fn add_outputs_from_diff(world: &mut TariWorld, outputs_name: String, diff: &SubstateDiff) {
    let outputs = world.outputs.entry(outputs_name).or_default();
    let mut counters = [0usize; 10];
    for (addr, data) in diff.up_iter() {
        match addr {
            SubstateId::Component(component_addr) => {
                let component = data.substate_value().component().unwrap();
                if *component.template_address() == ACCOUNT_TEMPLATE_ADDRESS {
                    let account = world
                        .wallet_accounts
                        .values()
                        .find(|a| a.account.component_address == *component_addr);

                    let label = account
                        .map(|a| a.account.name.clone().expect("account no name"))
                        .unwrap_or_else(|| format!("notfound_{}", counters[9]));
                    outputs.insert(format!("accounts/{label}"), SubstateRequirement {
                        substate_id: addr.clone(),
                        version: Some(data.version()),
                    });

                    counters[9] += 1;
                } else {
                    let template = world
                        .templates
                        .values()
                        .find(|a| a.address == *component.template_address())
                        .unwrap_or_else(|| {
                            panic!(
                                "Template not found for component with template address {}",
                                component.template_address()
                            )
                        });
                    outputs.insert(format!("components/{}", template.name), SubstateRequirement {
                        substate_id: addr.clone(),
                        version: Some(data.version()),
                    });
                    counters[0] += 1;
                }
            },
            SubstateId::Resource(_) => {
                let resource = data.substate_value().as_resource().unwrap();
                outputs.insert(
                    format!(
                        "resources/{}",
                        resource
                            .token_symbol()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| format!("noname_{}", counters[1]))
                    ),
                    SubstateRequirement {
                        substate_id: addr.clone(),
                        version: Some(data.version()),
                    },
                );
                counters[1] += 1;
            },
            SubstateId::Vault(_) => {
                outputs.insert(format!("vaults/{}", counters[2]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[2] += 1;
            },
            SubstateId::NonFungible(_) => {
                outputs.insert(format!("nfts/{}", counters[3]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[3] += 1;
            },
            SubstateId::ClaimedOutputTombstone(_) => {
                outputs.insert(format!("layer_one_commitments/{}", counters[4]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[4] += 1;
            },
            SubstateId::TransactionReceipt(_) => {
                outputs.insert(format!("transaction_receipt/{}", counters[5]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[5] += 1;
            },
            SubstateId::Template(_) => {
                outputs.insert(format!("published_template/{}", counters[6]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[6] += 1;
            },
            SubstateId::ValidatorFeePool(_) => {
                outputs.insert(format!("validator_fee_pool/{}", counters[7]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[7] += 1;
            },
            SubstateId::Utxo(_) => {
                outputs.insert(format!("utxos/{}", counters[8]), SubstateRequirement {
                    substate_id: addr.clone(),
                    version: Some(data.version()),
                });
                counters[8] += 1;
            },
        }
    }
}

/// Calls a method on a component concurrently multiple times
pub async fn concurrent_call_method(
    world: &mut TariWorld,
    vn_name: String,
    fq_component_name: String,
    method_call: String,
    times: usize,
) -> Result<SubmitTransactionResponse, RejectReason> {
    let mut component = get_component_from_namespace(world, fq_component_name);
    // For concurrent transactions we DO NOT specify the versions
    component.version = None;

    let vn_client = world.get_validator_node(&vn_name).get_client();
    let secret_key = get_signing_key(world).await;

    let mut handles = Vec::new();
    for _ in 0..times {
        let handle = tokio::spawn(call_method_inner(
            vn_client.clone(),
            secret_key.clone(),
            component.clone(),
            method_call.clone(),
        ));
        handles.push(handle);
    }

    let mut last_resp = None;
    for handle in handles {
        let result = handle
            .await
            .map_err(|e| RejectReason::ExecutionFailure(e.to_string()))?;
        match result {
            Ok(response) => last_resp = Some(response),
            Err(e) => return Err(e),
        }
    }

    if let Some(res) = last_resp {
        Ok(res)
    } else {
        Err(RejectReason::ExecutionFailure(
            "No responses from any of the concurrent calls".to_owned(),
        ))
    }
}

/// Calls a method on a component and stores the outputs
pub async fn call_method(
    world: &mut TariWorld,
    vn_name: String,
    fq_component_name: String,
    outputs_name: String,
    method_call: String,
) -> Result<SubmitTransactionResponse, RejectReason> {
    let component = get_component_from_namespace(world, fq_component_name);
    let vn_client = world.get_validator_node(&vn_name).get_client();
    let secret_key = get_signing_key(world).await;

    let resp = call_method_inner(vn_client, secret_key, component, method_call).await?;

    // Store the account component address and other substate ids for later reference
    add_outputs_from_diff(
        world,
        outputs_name,
        resp.dry_run_result
            .as_ref()
            .unwrap()
            .finalize
            .result
            .any_accept()
            .unwrap(),
    );
    Ok(resp)
}

/// Internal helper to call a method on a component
async fn call_method_inner(
    mut vn_client: ValidatorNodeClient,
    secret_key: PrivateKey,
    component: SubstateRequirement,
    method_call: String,
) -> Result<SubmitTransactionResponse, RejectReason> {
    println!("Inputs: {}", component);

    let component_address = component.substate_id.as_component_address().ok_or_else(|| {
        RejectReason::ExecutionFailure(format!("Invalid component address: {}", component.substate_id))
    })?;

    // Build transaction
    let transaction = TransactionBuilder::new(Network::LocalNet)
        .call_method(component_address, method_call, vec![])
        .with_inputs(vec![component])
        .build_and_seal(&secret_key);

    // Submit and wait for result
    let resp = submit_and_wait_for_result(&mut vn_client, transaction, Duration::from_secs(60))
        .await
        .map_err(|e| RejectReason::ExecutionFailure(e.to_string()))?;

    if let Some(failure) = resp.dry_run_result.as_ref().unwrap().finalize.fee_reject() {
        return Err(failure.clone());
    }

    Ok(resp)
}

/// Gets the signing key from the world's key manager
async fn get_signing_key(world: &TariWorld) -> PrivateKey {
    world
        .key_manager
        .get_private_key(&TariKeyId::default())
        .expect("Failed to get private key from key manager")
}

/// Submits a transaction and waits for the result
async fn submit_and_wait_for_result(
    client: &mut ValidatorNodeClient,
    transaction: Transaction,
    timeout: Duration,
) -> anyhow::Result<SubmitTransactionResponse> {
    let request = SubmitTransactionRequest { transaction };
    let mut resp = client.submit_transaction(request).await?;
    let transaction_id = resp.transaction_id;

    println!("✅ Transaction {} submitted.", transaction_id);

    // Wait for transaction result
    let mut poll_interval = interval(Duration::from_secs(1));
    poll_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Timeout waiting for transaction result");
        }

        if let Some(execution) = client
            .get_transaction_result(GetTransactionResultRequest { transaction_id })
            .await
            .optional()?
        {
            // Populate the dry_run_result for compatibility with existing code
            resp.dry_run_result = Some(tari_validator_node_client::types::DryRunTransactionFinalizeResult {
                decision: if execution.transaction_execution.decision().is_commit() {
                    tari_sidechain::QuorumDecision::Accept
                } else {
                    tari_sidechain::QuorumDecision::Reject
                },
                fee_breakdown: Some(
                    execution
                        .transaction_execution
                        .result()
                        .finalize
                        .fee_receipt
                        .to_cost_breakdown(),
                ),
                finalize: execution.transaction_execution.result().finalize.clone(),
            });
            return Ok(resp);
        }

        poll_interval.tick().await;
    }
}

/// Parses a string argument into a NamedArg for transaction building
pub fn parse_arg(s: &str) -> Result<tari_ootle_transaction::builder::named_args::NamedArg, String> {
    use tari_ootle_transaction::arg;

    // Try parsing as primitives first
    if let Ok(v) = s.parse::<bool>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<u8>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<u16>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<u32>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<u64>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<i8>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<i16>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<i32>() {
        return Ok(arg!(v));
    }
    if let Ok(v) = s.parse::<i64>() {
        return Ok(arg!(v));
    }

    // Try parsing as SubstateId
    if let Ok(v) = SubstateId::from_str(s) {
        return Ok(match v {
            SubstateId::Component(addr) => arg!(addr),
            SubstateId::Resource(addr) => arg!(addr),
            SubstateId::Vault(addr) => arg!(addr),
            SubstateId::ClaimedOutputTombstone(addr) => arg!(addr),
            SubstateId::NonFungible(addr) => arg!(addr),
            SubstateId::TransactionReceipt(addr) => arg!(addr),
            SubstateId::Template(addr) => arg!(addr),
            SubstateId::ValidatorFeePool(addr) => arg!(addr),
            SubstateId::Utxo(addr) => arg!(addr),
        });
    }

    // Try parsing as template address
    if let Ok(v) = TemplateAddress::from_hex(s) {
        return Ok(arg!(v));
    }

    // Try parsing special prefixed types
    if let Some(("nft", nft_id)) = s.split_once('_') &&
        let Ok(v) = NonFungibleId::try_from_canonical_string(nft_id)
    {
        return Ok(arg!(v));
    }

    if let Some(("amount", amount)) = s.split_once('_') &&
        let Ok(number) = amount.parse::<i64>()
    {
        return Ok(arg!(number));
    }

    // Default to string
    Ok(arg!(s.to_string()))
}
