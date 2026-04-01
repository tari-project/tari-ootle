//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use cucumber::{gherkin::Step, given, then, when};
use integration_tests::{
    TariWorld,
    cucumber_log,
    indexer::{IndexerProcess, spawn_indexer},
};
use libp2p::Multiaddr;
use tari_ootle_common_types::{Epoch, displayable::Displayable};

#[when(expr = "indexer {word} connects to all other validators")]
async fn given_validator_connects_to_other_vns(world: &mut TariWorld, name: String) {
    let indexer = world.get_indexer(&name);
    let details = world
        .all_running_validators_iter()
        .filter(|vn| vn.name != name)
        .map(|vn| {
            (
                vn.public_key,
                Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/{}", vn.p2p_port)).unwrap(),
            )
        });

    for (pk, addr) in details {
        indexer.add_peer(pk, vec![addr.clone()]).await;
    }
}

#[then(expr = "indexer {word} has scanned to at least height {int}")]
pub async fn indexer_has_scanned_to_at_least_height(
    world: &mut TariWorld,
    step: &Step,
    name: String,
    block_height: u64,
) {
    cucumber_log!("=== Step:{}", step.value);
    let indexer = world.get_indexer(&name);
    let client = indexer.get_indexer_client();
    let mut last_block_height = 0;
    let mut remaining = 10;
    loop {
        let stats = client.get_epoch_manager_stats().await.expect("Failed to get stats");
        if stats.current_block_height >= block_height {
            return;
        }

        if stats.current_block_height != last_block_height {
            last_block_height = stats.current_block_height;
            // Reset the timer each time the scanned height changes
            remaining = 10;
        }

        if remaining == 0 {
            panic!(
                "Indexer {} did not scan to block height {}. Current height: {}",
                name, block_height, stats.current_block_height
            );
        }
        remaining -= 1;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[given(expr = "an indexer {word} connected to base node {word}")]
async fn start_indexer(world: &mut TariWorld, indexer_name: String, bn_name: String) {
    spawn_indexer(world, indexer_name, bn_name).await;
}

#[given(expr = "an indexer {word} connected to a base node")]
async fn start_indexer_connected_to_a_base_node(world: &mut TariWorld, indexer_name: String) {
    let bn_name = world
        .base_nodes
        .keys()
        .next()
        .cloned()
        .expect("no base nodes have been started");
    spawn_indexer(world, indexer_name, bn_name).await;
}

#[then(expr = "{word} indexer GraphQL request works")]
async fn works_indexer_graphql(world: &mut TariWorld, indexer_name: String) {
    let indexer = world.indexers.get(&indexer_name).unwrap();
    let mut graphql_client = indexer.get_graphql_indexer_client().await;
    let query = r#"{ getEvents { substateId, templateAddress, txHash, topic, payload } }"#.to_string();
    let res = graphql_client
        .send_request::<HashMap<String, Vec<tari_indexer::graphql::model::events::Event>>>(&query, None, None)
        .await
        .expect("Failed to obtain getEventsForTransaction query result");
    res.get("getEvents").unwrap();
}

#[when(expr = "indexer {word} scans the network events for account {word} with topics {word}")]
async fn indexer_scans_network_events(
    world: &mut TariWorld,
    indexer_name: String,
    account_name: String,
    topics_str: String,
) {
    let indexer: &mut IndexerProcess = world.indexers.get_mut(&indexer_name).unwrap_or_else(|| {
        panic!("Indexer {} not found", indexer_name);
    });
    let account = world.wallet_accounts.get(&account_name).unwrap_or_else(|| {
        panic!("No wallet account found with name {}", account_name);
    });
    let mut graphql_client = indexer.get_graphql_indexer_client().await;
    let query = r#"{ getEvents { substateId, templateAddress, txHash, topic, payload } }"#.to_string();
    let res = graphql_client
        .send_request::<HashMap<String, Vec<tari_indexer::graphql::model::events::Event>>>(&query, None, None)
        .await
        .expect("Failed to obtain getEvents query result");

    let events = res.get("getEvents").unwrap();
    let topics_for_component = events
        .iter()
        .filter(|e| e.substate_id == Some(account.component_address().to_string()))
        .map(|e| e.topic.as_str())
        .collect::<HashSet<_>>();

    let expected_topics = topics_str.split(',');
    for (ind, topic) in expected_topics.enumerate() {
        assert!(
            topics_for_component.contains(topic),
            "Unexpected topic at index {}. Events emitted were {}. Expected topic {} (ALL {:?})",
            ind,
            topics_for_component.display(),
            topic,
            events
        );
    }
}

#[when(expr = "indexer {word} scans the network for events of resource {word}")]
async fn indexer_scans_network_events_for_resource(world: &mut TariWorld, indexer_name: String, resource_path: String) {
    let indexer: &mut IndexerProcess = world.indexers.get_mut(&indexer_name).unwrap();

    // extract the resource address from the outputs
    let (input_group, index) = resource_path.split_once('/').unwrap_or_else(|| {
        panic!(
            "Resource name must be in the format '{{group}}/resources/{{index}}', got {}",
            resource_path
        )
    });
    let resource_address = world
        .outputs
        .get(input_group)
        .unwrap_or_else(|| panic!("No outputs found with name {}", input_group))
        .iter()
        .find(|(i, _)| **i == index)
        .map(|(_, data)| data.clone())
        .unwrap_or_else(|| panic!("No resource with index {}", index))
        .substate_id()
        .as_resource_address()
        .unwrap_or_else(|| panic!("The output is not a resource {}", index));

    let mut graphql_client = indexer.get_graphql_indexer_client().await;
    let query = format!(
        r#"{{ getEvents(substateId:"{}", offset:0, limit:10) {{ substateId, templateAddress, txHash, topic, payload }} }}"#,
        resource_address
    );
    let res = graphql_client
        .send_request::<HashMap<String, Vec<tari_indexer::graphql::model::events::Event>>>(&query, None, None)
        .await
        .expect("Failed to obtain getEvents query result");

    let events = res.get("getEvents").unwrap();

    // TODO: assert the results
    eprintln!("{:?}", events);
}

#[then(expr = "the indexer {word} returns version {int} for substate {word}")]
async fn assert_indexer_substate_version(
    world: &mut TariWorld,
    indexer_name: String,
    version: u32,
    output_ref: String,
) {
    let indexer = world.indexers.get(&indexer_name).unwrap();
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let substate = indexer.get_substate(world, output_ref, version).await;
    eprintln!(
        "indexer.get_substate result: {}",
        serde_json::to_string_pretty(&substate).unwrap()
    );
    assert_eq!(substate.version, version);
}

#[then(expr = "the indexer {word} returns {int} non fungibles for resource {word}")]
async fn assert_indexer_non_fungible_list(
    world: &mut TariWorld,
    indexer_name: String,
    count: usize,
    output_ref: String,
) {
    let indexer = world.indexers.get(&indexer_name).unwrap();
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let nfts = indexer.get_non_fungibles(world, output_ref, 0, count as u64).await;
    eprintln!("indexer.get_non_fungibles result: {:?}", nfts);
    assert_eq!(
        nfts.len(),
        count,
        "Unexpected number of NFTs returned. Expected: {}, Actual: {}",
        count,
        nfts.len()
    );
}

#[then(expr = "the indexer {word} has at least {int} template(s) in the catalogue")]
async fn assert_indexer_catalogue_count(world: &mut TariWorld, indexer_name: String, min_count: usize) {
    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);

    let mut remaining = 30;
    loop {
        let resp = indexer.list_template_catalogue(None, Some(100), Some(0)).await;
        if resp.entries.len() >= min_count {
            return;
        }
        if remaining == 0 {
            panic!(
                "Indexer {} catalogue has {} entries, expected at least {}",
                indexer_name,
                resp.entries.len(),
                min_count
            );
        }
        remaining -= 1;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[then(expr = "the indexer {word} catalogue contains template {word}")]
async fn assert_indexer_catalogue_contains_template(
    world: &mut TariWorld,
    indexer_name: String,
    template_name: String,
) {
    let template = world
        .templates
        .get(&template_name)
        .unwrap_or_else(|| panic!("Template {} not registered in world", template_name));
    let template_address = template.address;

    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);

    let mut remaining = 30;
    loop {
        let client = indexer.get_indexer_client();
        match client.get_template_catalogue_entry(template_address).await {
            Ok(entry) => {
                assert_eq!(
                    entry.template_address, template_address,
                    "Template address mismatch in catalogue entry"
                );
                assert!(
                    !entry.template_name.is_empty(),
                    "template_name should not be empty for {} (address: {})",
                    template_name,
                    template_address
                );
                return;
            },
            Err(_) => {
                if remaining == 0 {
                    panic!(
                        "Indexer {} catalogue does not contain template {} (address: {})",
                        indexer_name, template_name, template_address
                    );
                }
                remaining -= 1;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            },
        }
    }
}

#[then(expr = "the indexer {word} catalogue name filter {word} returns {int} result(s)")]
async fn assert_indexer_catalogue_name_filter(
    world: &mut TariWorld,
    indexer_name: String,
    name_filter: String,
    expected_count: usize,
) {
    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let resp = indexer
        .list_template_catalogue(Some(name_filter.clone()), Some(100), Some(0))
        .await;
    assert_eq!(
        resp.entries.len(),
        expected_count,
        "Catalogue name filter '{}' returned {} entries, expected {}",
        name_filter,
        resp.entries.len(),
        expected_count
    );
}

#[then(expr = "the indexer {word} catalogue with limit {int} offset {int} returns {int} entries")]
async fn assert_indexer_catalogue_page(
    world: &mut TariWorld,
    indexer_name: String,
    limit: u64,
    offset: u64,
    expected_count: usize,
) {
    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let resp = indexer.list_template_catalogue(None, Some(limit), Some(offset)).await;
    assert_eq!(
        resp.entries.len(),
        expected_count,
        "Catalogue with limit={} offset={} returned {} entries, expected {}",
        limit,
        offset,
        resp.entries.len(),
        expected_count
    );
}

#[then(expr = "the indexer {word} catalogue with limit {int} offset {int} returns at least {int} entries")]
async fn assert_indexer_catalogue_page_min(
    world: &mut TariWorld,
    indexer_name: String,
    limit: u64,
    offset: u64,
    min_count: usize,
) {
    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let resp = indexer.list_template_catalogue(None, Some(limit), Some(offset)).await;
    assert!(
        resp.entries.len() >= min_count,
        "Catalogue with limit={} offset={} returned {} entries, expected at least {}",
        limit,
        offset,
        resp.entries.len(),
        min_count
    );
}

#[then(expr = "the indexer {word} catalogue entry for address {string} is not found")]
async fn assert_catalogue_entry_not_found(world: &mut TariWorld, indexer_name: String, address_str: String) {
    use tari_engine_types::published_template::PublishedTemplateAddress;
    let address = PublishedTemplateAddress::from_str(&address_str)
        .unwrap_or_else(|_| panic!("Invalid template address: {}", address_str))
        .as_template_address();
    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let client = indexer.get_indexer_client();
    let result = client.get_template_catalogue_entry(address).await;
    assert!(
        result.is_err(),
        "Expected 404 for address {} but got a result",
        address_str
    );
}

#[then(expr = "I wait for the indexer {word} to sync with the network")]
async fn i_wait_for_the_indexer_to_sync_with_the_network(world: &mut TariWorld, indexer_name: String) {
    let vn = world
        .validator_nodes
        .values()
        .chain(world.vn_seeds.values())
        .find(|vn| !vn.shutdown.is_triggered())
        .expect(
            "No running validator nodes found. An indexer must be connected to a running validator node to sync with \
             the network",
        );
    let consensus_stats = vn
        .get_client()
        .get_consensus_status()
        .await
        .expect("Failed to get epoch stats from VN");
    if consensus_stats.state != "Running" {
        panic!(
            "Validator node {} is not running. An indexer must be connected to a running validator node to sync with \
             the network",
            vn.name
        );
    }
    let epoch = consensus_stats.epoch;
    let prev_epoch = epoch.checked_sub(Epoch(1)).expect("Epoch is zero");
    let state_versions = consensus_stats
        .state_versions
        .unwrap_or_else(|| panic!("No state versions found in consensus stats for running VN {}", vn.name));

    let indexer = world.get_indexer(&indexer_name);
    assert!(!indexer.handle.is_finished(), "Indexer {} is not running", indexer_name);
    let client = indexer.get_indexer_client();
    let mut remaining_attempts = 60;
    loop {
        if remaining_attempts == 0 {
            panic!(
                "Indexer {} did not sync with the network in time. Current epoch: {}",
                indexer_name, prev_epoch
            );
        }

        remaining_attempts -= 1;
        let state = client.get_network_sync_state().await.unwrap();
        if let Some(ref progress) = state.sync_progress {
            if progress.last_state_versions.is_empty() {
                integration_tests::cucumber_log!(
                    "Waiting for indexer {} to sync. Current epoch: {}, no checkpoint progress yet",
                    indexer_name,
                    prev_epoch
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            if let Some((shard, (state_version, epoch))) = progress
                .last_state_versions
                .iter()
                // If the indexer is not at the epoch and not scanned to the state version for the shard, we are not synced
                .find(|(s, (v, e))| *e < prev_epoch || state_versions.get(s).is_none_or(|sv| sv > v))
            {
                integration_tests::cucumber_log!(
                    "Waiting for indexer {} to sync. Current epoch: {}, shard_group: {}, state_version: {}, scanned \
                     epoch: {}",
                    indexer_name,
                    prev_epoch,
                    shard,
                    state_version,
                    epoch
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }

            break;
        } else {
            integration_tests::cucumber_log!(
                "Waiting for indexer {} to sync. Current epoch: {}, no sync progress yet",
                indexer_name,
                prev_epoch
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}
