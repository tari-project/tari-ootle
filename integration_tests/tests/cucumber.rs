//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

mod steps;
use std::{fs, future, io, panic, str::FromStr, time::Duration};

use anyhow::bail;
use cucumber::{gherkin::Step, given, then, when, writer, writer::Verbosity, ScenarioType, World, WriterExt};
use integration_tests::{
    http_server::MockHttpServer,
    logging::{create_log_config_file, get_base_dir},
    miner::{mine_blocks, register_miner_process},
    validator_node_cli,
    wallet::spawn_minotari_wallet,
    wallet_daemon::spawn_wallet_daemon,
    wallet_daemon_client,
    TariWorld,
};
use libp2p::{
    futures::{
        future::{select, Either},
        pin_mut,
    },
    Multiaddr,
};
use regex::Regex;
use tari_common::initialize_logging;
use tari_engine::abi::Type;
use tari_shutdown::Shutdown;
use tari_sidechain::QuorumDecision;
use tari_validator_node_client::types::AddPeerRequest;

const LOG_TARGET: &str = "cucumber";

#[tokio::main]
async fn main() {
    let log_path = create_log_config_file();
    let base_path = get_base_dir();
    initialize_logging(log_path.as_path(), &base_path, include_str!("./log4rs/cucumber.yml")).unwrap();

    // Start the mock server that continues to run for the duration of the tests
    let mut shutdown = Shutdown::new();

    let file = fs::File::create("cucumber-output-junit.xml").unwrap();
    let cucumber_fut = TariWorld::cucumber()
        .max_concurrent_scenarios(2)
        .with_writer(writer::Tee::new(
            writer::JUnit::new(file, Verbosity::ShowWorldAndDocString).normalized(),
            // following config needed to use eprint statements in the tests
            writer::Basic::raw(io::stdout(), writer::Coloring::Auto, Verbosity::ShowWorldAndDocString)
                .normalized()
                .summarized(),
        ))
        .before(move |_feature, _rule, scenario, world| {
            log::info!(target: LOG_TARGET, "\n\n\n");
            log::info!(target: LOG_TARGET, "-------------------------------------------------------");
            log::info!(target: LOG_TARGET, "------------- SCENARIO: {} -------------", scenario.name);
            log::info!(target: LOG_TARGET, "-------------------------------------------------------");
            log::info!(target: LOG_TARGET, "\n\n\n");
            world.current_scenario_name = Some(scenario.name.clone());
            Box::pin(async move {
                // Each scenario gets a mock connection. As each connection is dropped after the scenario, all the mock
                // urls are deregistered
                world.http_server = Some(MockHttpServer::connect().await);
            })
        })
        .after(move |_feature, _rule, scenario, _finished, maybe_world| {
            if let Some(world) = maybe_world {
                world.after(scenario);
            }
            Box::pin(future::ready(()))
        })
        .fail_on_skipped()
        .which_scenario(|feature, _, scenario| {
            let feature_has_concurrent_tag = feature.tags.iter().any(|tag| tag == "concurrent");
            let scenario_has_concurrent_tag = scenario.tags.iter().any(|tag| tag == "concurrent");

            if scenario_has_concurrent_tag || feature_has_concurrent_tag {
                return ScenarioType::Concurrent;
            }

            ScenarioType::Serial
        })
        .filter_run("tests/features/", |_, _, sc| !sc.tags.iter().any(|t| t == "ignore"));

    let ctrl_c = tokio::signal::ctrl_c();
    pin_mut!(ctrl_c);
    pin_mut!(cucumber_fut);
    match select(cucumber_fut, ctrl_c).await {
        Either::Left(_) => {},
        Either::Right((ctrl_c, _)) => ctrl_c.unwrap(),
    }

    shutdown.trigger();
}

#[then(expr = "I stop validator node {word}")]
#[when(expr = "I stop validator node {word}")]
async fn stop_validator_node(world: &mut TariWorld, vn_name: String) {
    let vn_ps = world.get_validator_node_mut(&vn_name);
    vn_ps.stop();
}

#[given(expr = "a wallet daemon {word} connected to indexer {word}")]
async fn start_wallet_daemon(world: &mut TariWorld, wallet_daemon_name: String, indexer_name: String) {
    spawn_wallet_daemon(world, wallet_daemon_name, indexer_name).await;
}

#[when(expr = "I stop wallet daemon {word}")]
async fn stop_wallet_daemon(world: &mut TariWorld, wallet_daemon_name: String) {
    let walletd_ps = world.wallet_daemons.get_mut(&wallet_daemon_name).unwrap();
    walletd_ps.stop();
}

#[when(
    expr = r#"I call function "{word}" on template "{word}" using account {word} to pay fees via wallet daemon {word} with args "{word}" named "{word}""#
)]
async fn call_template_constructor_via_wallet_daemon(
    world: &mut TariWorld,
    function_call: String,
    template_name: String,
    account_name: String,
    wallet_daemon_name: String,
    args: String,
    outputs_name: String,
) {
    let args = args.split(',').map(|a| a.trim().to_string()).collect();
    wallet_daemon_client::create_component(
        world,
        outputs_name,
        template_name,
        account_name,
        wallet_daemon_name,
        function_call,
        args,
        None,
        None,
    )
    .await;

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(
    expr = r#"I call function "{word}" on template "{word}" using account {word} to pay fees via wallet daemon {word} named "{word}""#
)]
async fn call_template_constructor_via_wallet_daemon_no_args(
    world: &mut TariWorld,
    function_call: String,
    template_name: String,
    account_name: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    wallet_daemon_client::create_component(
        world,
        outputs_name,
        template_name,
        account_name,
        wallet_daemon_name,
        function_call,
        vec![],
        None,
        None,
    )
    .await;
}

#[when(
    expr = r#"I call function "{word}" on template "{word}" with args {string} using account {word} to pay fees via wallet daemon {word} named "{word}""#
)]
async fn call_template_constructor_via_wallet_daemon_with_args(
    world: &mut TariWorld,
    function_call: String,
    template_name: String,
    args_raw: String,
    account_name: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    let args: Vec<String> = args_raw.split(',').map(|str| str.trim().to_string()).collect();
    wallet_daemon_client::create_component(
        world,
        outputs_name,
        template_name,
        account_name,
        wallet_daemon_name,
        function_call,
        args,
        None,
        None,
    )
    .await;
}

#[when(expr = r#"I call function "{word}" on template "{word}" on {word} with args "{word}" named "{word}""#)]
async fn call_template_constructor(
    world: &mut TariWorld,
    function_call: String,
    template_name: String,
    vn_name: String,
    args: String,
    outputs_name: String,
) {
    let args = args.split(',').map(|a| a.trim().to_string()).collect();
    validator_node_cli::create_component(world, outputs_name, template_name, vn_name, function_call, args).await;

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(expr = r#"I call function "{word}" on template "{word}" on {word} named "{word}""#)]
async fn call_template_constructor_with_no_args(
    world: &mut TariWorld,
    function_call: String,
    template_name: String,
    vn_name: String,
    outputs_name: String,
) {
    validator_node_cli::create_component(world, outputs_name, template_name, vn_name, function_call, vec![]).await;

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(expr = r#"I create a component {word} of template "{word}" on {word} using "{word}""#)]
async fn call_template_constructor_without_args(
    world: &mut TariWorld,
    component_name: String,
    template_name: String,
    vn_name: String,
    function_call: String,
) {
    validator_node_cli::create_component(world, component_name, template_name, vn_name, function_call, vec![]).await;

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(expr = r#"I invoke on {word} on component {word} the method call "{word}" named "{word}""#)]
async fn call_component_method(
    world: &mut TariWorld,
    vn_name: String,
    component_name: String,
    method_call: String,
    output_name: String,
) {
    let resp = validator_node_cli::call_method(world, vn_name, component_name, output_name, method_call)
        .await
        .unwrap();
    assert_eq!(resp.dry_run_result.unwrap().decision, QuorumDecision::Accept);

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(expr = r#"I invoke on {word} on component {word} the method call "{word}" concurrently {int} times"#)]
async fn call_component_method_concurrently(
    world: &mut TariWorld,
    vn_name: String,
    component_name: String,
    method_call: String,
    times: usize,
) {
    validator_node_cli::concurrent_call_method(world, vn_name, component_name, method_call, times)
        .await
        .unwrap();
}

#[when(
    expr = r#"I invoke on {word} on component {word} the method call "{word}" named "{word}" the result is error {string}"#
)]
async fn call_component_method_must_error(
    world: &mut TariWorld,
    vn_name: String,
    component_name: String,
    method_call: String,
    output_name: String,
    error_msg: String,
) {
    let res = validator_node_cli::call_method(world, vn_name, component_name, output_name, method_call).await;
    if let Err(reject) = res {
        assert!(reject.to_string().contains(&error_msg));
    } else {
        panic!("Expected an error but the call was successful");
    }
}

#[when(expr = r#"I invoke on all validator nodes on component {word} the method call "{word}" named "{word}""#)]
async fn call_component_method_on_all_vns(
    world: &mut TariWorld,
    component_name: String,
    method_call: String,
    output_name: String,
) {
    let vn_names = world.validator_nodes.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>();
    for vn_name in vn_names {
        let resp = validator_node_cli::call_method(
            world,
            vn_name,
            component_name.clone(),
            output_name.clone(),
            method_call.clone(),
        )
        .await
        .unwrap();
        assert_eq!(resp.dry_run_result.unwrap().decision, QuorumDecision::Accept);
    }
    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(expr = "I invoke on {word} on component {word} the method call \"{word}\" the result is \"{word}\"")]
async fn call_component_method_and_check_result(
    world: &mut TariWorld,
    vn_name: String,
    component_name: String,
    method_call: String,
    expected_result: String,
) {
    let resp =
        validator_node_cli::call_method(world, vn_name, component_name, "dummy_outputs".to_string(), method_call)
            .await
            .unwrap();
    let finalize_result = resp.dry_run_result.unwrap();
    assert_eq!(finalize_result.decision, QuorumDecision::Accept);

    let results = finalize_result.finalize.execution_results;
    let result = results.first().unwrap();
    match result.return_type {
        Type::U32 => {
            let u32_result: u32 = result.decode().unwrap();
            assert_eq!(u32_result.to_string(), expected_result);
        },
        // TODO: handle other possible return types
        _ => todo!(),
    };

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(
    expr = r#"I invoke on wallet daemon {word} on account {word} on component {word} the method call "{word}" the result is "{word}""#
)]
async fn call_wallet_daemon_method_and_check_result(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    output_ref: String,
    method_call: String,
    expected_result: String,
) -> anyhow::Result<()> {
    let resp = wallet_daemon_client::call_component(
        world,
        account_name,
        output_ref,
        wallet_daemon_name,
        method_call,
        None,
        true,
    )
    .await?;

    let finalize_result = resp
        .result
        .clone()
        .unwrap_or_else(|| panic!("Failed to unwrap result from response: {:?}", resp));
    let result = finalize_result
        .execution_results
        .first()
        .unwrap_or_else(|| panic!("Failed to call first() on results: {:?}", resp));
    match result.return_type {
        Type::U32 => {
            let u32_result: u32 = result.decode()?;
            assert_eq!(u32_result.to_string(), expected_result);
        },
        _ => todo!(),
    };

    Ok(())
}

#[when(expr = r#"I invoke on wallet daemon {word} on account {word} on component {word} the method call "{word}""#)]
async fn call_wallet_daemon_method(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    output_ref: String,
    method_call: String,
) -> anyhow::Result<()> {
    wallet_daemon_client::call_component(
        world,
        account_name,
        output_ref,
        wallet_daemon_name,
        method_call,
        None,
        true,
    )
    .await?;

    Ok(())
}

#[when(
    expr = r#"I invoke on wallet daemon {word} on account {word} on component {word} the method call "{word}" named "{word}""#
)]
async fn call_wallet_daemon_method_with_output_name(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    output_ref: String,
    method_call: String,
    new_output_name: String,
) -> anyhow::Result<()> {
    wallet_daemon_client::call_component(
        world,
        account_name,
        output_ref,
        wallet_daemon_name,
        method_call,
        Some(new_output_name),
        true,
    )
    .await?;

    Ok(())
}

#[when(
    expr = r#"I invoke on wallet daemon {word} on account {word} on component {word} the method call "{word}" named "{word}", I expect it to fail with {string}"#
)]
async fn call_wallet_daemon_method_with_output_name_error_result(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    output_ref: String,
    method_call: String,
    new_output_name: String,
    error_message: String,
) -> anyhow::Result<()> {
    if let Err(error) = wallet_daemon_client::call_component(
        world,
        account_name,
        output_ref,
        wallet_daemon_name,
        method_call,
        Some(new_output_name),
        // We expect this to fail due to a substate being downed so we need to use versioned inputs
        false,
    )
    .await
    {
        let error_str = error.to_string();
        let re = Regex::new(error_message.as_str()).expect("invalid regex for error message");
        if re.find(error_str.as_str()).is_none() {
            bail!(
                "Error mismatch: \"{}\" does not contain \"{}\"",
                error_str,
                error_message.as_str()
            );
        }
    } else {
        bail!("Error expected, but the transaction succeeded.");
    }

    Ok(())
}

#[when(
    expr = r#"I invoke on wallet daemon {word} on account {word} on component {word} the method call "{word}" concurrently {int} times"#
)]
async fn call_wallet_daemon_method_concurrently(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    output_ref: String,
    method_call: String,
    times: usize,
) {
    wallet_daemon_client::concurrent_call_component(
        world,
        account_name,
        output_ref,
        wallet_daemon_name,
        method_call,
        times,
    )
    .await
    .unwrap_or_else(|e| panic!("Concurrent wallet daemon call failed: {:?}", e));
}

#[when(
    expr = "I invoke on all validator nodes on component {word} the method call \"{word}\" the result is \"{word}\""
)]
async fn call_component_method_on_all_vns_and_check_result(
    world: &mut TariWorld,
    component_name: String,
    method_call: String,
    expected_result: String,
) {
    let vn_names = world.validator_nodes.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>();
    for vn_name in vn_names {
        let resp = validator_node_cli::call_method(
            world,
            vn_name,
            component_name.clone(),
            "dummy_outputs".to_string(),
            method_call.clone(),
        )
        .await
        .unwrap();
        let finalize_result = resp.dry_run_result.unwrap();
        assert_eq!(finalize_result.decision, QuorumDecision::Accept);

        let results = finalize_result.finalize.execution_results;
        let result = results.first().unwrap();
        match result.return_type {
            Type::U32 => {
                let u32_result: u32 = result.decode().unwrap();
                assert_eq!(u32_result.to_string(), expected_result);
            },
            // TODO: handle other possible return types
            _ => todo!(),
        };
    }

    // give it some time between transactions
    // tokio::time::sleep(Duration::from_secs(4)).await;
}

#[when(regex = r#"^I submit a transaction manifest via wallet daemon (\w+) with inputs "([^"]+)" named "(\w+)"$"#)]
async fn submit_transaction_manifest_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    wallet_daemon_name: String,
    inputs: String,
    outputs_name: String,
) {
    let manifest = wrap_manifest_in_main(world, step.docstring.as_ref().expect("manifest code not provided"));
    wallet_daemon_client::submit_manifest(world, wallet_daemon_name, manifest, inputs, outputs_name, None, None).await;
}

#[when(
    regex = r#"^I submit a transaction manifest via wallet daemon (\w+) signed by the key of (\w+) with inputs "([^"]+)" named "(\w+)"$"#
)]
async fn submit_transaction_manifest_via_wallet_daemon_with_signing_keys(
    world: &mut TariWorld,
    step: &Step,
    wallet_daemon_name: String,
    account_signing_key: String,
    inputs: String,
    outputs_name: String,
) {
    let manifest = wrap_manifest_in_main(world, step.docstring.as_ref().expect("manifest code not provided"));
    wallet_daemon_client::submit_manifest_with_signing_keys(
        world,
        wallet_daemon_name,
        account_signing_key,
        manifest,
        inputs,
        outputs_name,
        None,
        None,
    )
    .await;
}

#[when(expr = "I mint a new non fungible token {word} on {word} using wallet daemon {word}")]
async fn mint_new_nft_on_account(
    world: &mut TariWorld,
    nft_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    wallet_daemon_client::mint_new_nft_on_account(world, nft_name, account_name, wallet_daemon_name, None).await;
}

#[when(expr = r#"I list all non fungible tokens on {word} using wallet daemon {word} the amount is {word}"#)]
async fn list_nfts_on_account(world: &mut TariWorld, account_name: String, wallet_daemon_name: String, amount: usize) {
    let nfts = wallet_daemon_client::list_account_nfts(world, account_name, wallet_daemon_name).await;
    assert_eq!(amount, nfts.len());
}

#[when(expr = "I mint a new non fungible token {word} on {word} using wallet daemon with metadata {word}")]
async fn mint_new_nft_on_account_with_metadata(
    world: &mut TariWorld,
    nft_name: String,
    account_name: String,
    wallet_daemon_name: String,
    metadata: String,
) {
    let metadata = serde_json::from_str::<serde_json::Value>(&metadata).expect("Failed to parse metadata");
    wallet_daemon_client::mint_new_nft_on_account(world, nft_name, account_name, wallet_daemon_name, Some(metadata))
        .await;
}

fn wrap_manifest_in_main(world: &TariWorld, contents: &str) -> String {
    // define all templates
    let template_defs = world.templates.iter().fold(String::new(), |acc, (name, template)| {
        format!("{}\nuse template_{} as {};", acc, template.address, name)
    });
    format!("{} fn main() {{ {} }}", template_defs, contents)
}

#[given(expr = "all validator nodes are connected to each other")]
async fn given_all_validator_connects_to_other_vns(world: &mut TariWorld) {
    let details = world
        .validator_nodes
        .values()
        .map(|vn| {
            (
                vn.public_key,
                Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/{}", vn.p2p_port)).unwrap(),
            )
        })
        .collect::<Vec<_>>();

    for vn in world.validator_nodes.values() {
        if vn.handle.is_finished() {
            eprintln!("Skipping validator node {} that is not running", vn.name);
            continue;
        }
        let mut cli = vn.create_client();
        for (pk, addr) in details.iter().cloned() {
            if pk == vn.public_key {
                continue;
            }
            cli.add_peer(AddPeerRequest {
                public_key: pk,
                addresses: vec![addr],
                wait_for_dial: true,
            })
            .await
            .unwrap();
        }
    }
}

#[when(expr = "I wait {int} seconds")]
#[then(expr = "I wait {int} seconds")]
async fn wait_seconds(_world: &mut TariWorld, seconds: u64) {
    // println!("NOT Waiting {} seconds", seconds);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
}

#[when(expr = "I print the cucumber world")]
fn print_world(world: &mut TariWorld) {
    world.print()
}

#[when(expr = "I save the {word} database of {word}")]
async fn when_i_save_the_database(world: &mut TariWorld, database_name: String, validator_name: String) {
    let validator = world
        .validator_nodes
        .get(&validator_name)
        .expect("validator node not found");
    validator
        .save_database(
            database_name,
            get_base_dir()
                .join(
                    world
                        .current_scenario_name
                        .as_ref()
                        .unwrap_or(&"unknown_step".to_string()),
                )
                .join(format!("save_no_{}", world.num_databases_saved))
                .as_path(),
        )
        .await;
    world.num_databases_saved += 1;
}
