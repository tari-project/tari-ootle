//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{path::PathBuf, str::FromStr};

use tari_engine_types::{
    commit_result::RejectReason,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::SubstateRequirement;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_validator_node_cli::{
    command::transaction::{handle_submit, CliArg, CliInstruction, CommonSubmitArgs, SubmitArgs},
    from_hex::FromHex,
};
use tari_validator_node_client::{types::SubmitTransactionResponse, ValidatorNodeClient};

use crate::{helpers::get_component_from_namespace, logging::get_base_dir_for_scenario, TariWorld};

pub async fn create_component(
    world: &mut TariWorld,
    outputs_name: String,
    template_name: String,
    vn_name: String,
    function_call: String,
    args: Vec<String>,
) {
    let data_dir = get_cli_data_dir(world);

    let template_address = world
        .templates
        .get(&template_name)
        .unwrap_or_else(|| panic!("Template not found with name {}", template_name))
        .address;
    let args: Vec<CliArg> = args.iter().map(|a| CliArg::from_str(a).unwrap()).collect();
    let instruction = CliInstruction::CallFunction {
        template_address: FromHex(template_address),
        function_name: function_call,
        args,
    };

    let args = SubmitArgs {
        instruction,
        common: CommonSubmitArgs {
            wait_for_result: true,
            wait_for_result_timeout: Some(300),
            inputs: vec![],
            version: None,
            dump_outputs_into: None,
            account_template_address: None,
        },
    };
    let mut client = world.get_validator_node(&vn_name).get_client();
    let resp = handle_submit(args, data_dir, &mut client).await.unwrap();

    if let Some(ref failure) = resp.dry_run_result.as_ref().unwrap().finalize.fee_reject() {
        panic!("Transaction failed: {:?}", failure);
    }
    // store the account component address and other substate ids for later reference
    add_outputs_from_diff(
        world,
        outputs_name,
        resp.dry_run_result.unwrap().finalize.result.any_accept().unwrap(),
    );
}

pub(crate) fn add_outputs_from_diff(world: &mut TariWorld, outputs_name: String, diff: &SubstateDiff) {
    let outputs = world.outputs.entry(outputs_name).or_default();
    let mut counters = [0usize; 10];
    for (addr, data) in diff.up_iter() {
        match addr {
            SubstateId::Component(component_addr) => {
                let component = data.substate_value().component().unwrap();
                if component.template_address == ACCOUNT_TEMPLATE_ADDRESS {
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
                    outputs.insert(format!("components/{}", component.module_name), SubstateRequirement {
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

    let vn_data_dir = get_cli_data_dir(world);
    let vn_client = world.get_validator_node(&vn_name).get_client();
    let mut handles = Vec::new();
    for _ in 0..times {
        let handle = tokio::spawn(call_method_inner(
            vn_client.clone(),
            vn_data_dir.clone(),
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

pub async fn call_method(
    world: &mut TariWorld,
    vn_name: String,
    fq_component_name: String,
    outputs_name: String,
    method_call: String,
) -> Result<SubmitTransactionResponse, RejectReason> {
    let data_dir = get_cli_data_dir(world);
    let component = get_component_from_namespace(world, fq_component_name);
    let vn_client = world.get_validator_node(&vn_name).get_client();
    let resp = call_method_inner(vn_client, data_dir, component, method_call).await?;

    // store the account component address and other substate ids for later reference
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

async fn call_method_inner(
    vn_client: ValidatorNodeClient,
    vn_data_dir: PathBuf,
    component: SubstateRequirement,
    method_call: String,
) -> Result<SubmitTransactionResponse, RejectReason> {
    let instruction = CliInstruction::CallMethod {
        component_address: component.substate_id.clone(),
        // TODO: actually parse the method call for arguments
        method_name: method_call,
        args: vec![],
    };

    println!("Inputs: {}", component);
    let args = SubmitArgs {
        instruction,
        common: CommonSubmitArgs {
            wait_for_result: true,
            wait_for_result_timeout: Some(60),
            inputs: vec![component],
            version: None,
            dump_outputs_into: None,
            account_template_address: None,
        },
    };
    let resp = handle_submit(args, vn_data_dir, &mut vn_client.clone()).await.unwrap();

    if let Some(failure) = resp.dry_run_result.as_ref().unwrap().finalize.fee_reject() {
        return Err(failure.clone());
    }

    Ok(resp)
}

pub(crate) fn get_cli_data_dir(world: &mut TariWorld) -> PathBuf {
    get_base_dir_for_scenario("vn_cli", world.current_scenario_name.as_ref().unwrap(), "SHARED")
}
