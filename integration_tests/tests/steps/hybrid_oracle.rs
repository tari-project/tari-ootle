//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A network whose validator set comes from a hybrid epoch oracle config rather than base layer registrations.
//!
//! Validators run the current config. The scenario's indexer starts from an outdated one, as an indexer released
//! before a validator set change would, and follows the validators' config from there.

use std::{collections::BTreeSet, fs, num::NonZeroU64, ops::ControlFlow, time::Duration};

use cucumber::{gherkin::Step, given, then};
use integration_tests::{
    TariWorld,
    base_node::spawn_base_node,
    cucumber_log,
    indexer::spawn_indexer_with_epoch_oracle,
    miner::register_miner_process,
    validator_node::{spawn_validator_node_with_fee_claim_key, validator_node_dir, validator_node_identity_file},
    wait::wait_until,
    wallet::spawn_minotari_wallet,
    wallet_daemon::spawn_wallet_daemon,
};
use multiaddr::multiaddr;
use ootle_byte_type::ToByteType;
use tari_base_node_client::BaseNodeClient;
use tari_epoch_oracles::configured::{Config, Validator};
use tari_indexer_client::types::ListEpochCheckpointsRequest;
use tari_ootle_app_utilities::{
    consensus_constants_file::ConsensusConstantsFile,
    epoch_oracle_config::{ConfiguredOracleConfig, EpochOracleConfig, EpochOracleType},
    keypair::{RistrettoKeypair, create_new_keypair},
};
use tari_ootle_common_types::{Epoch, ShardGroup, shard::Shard};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_validator_node_client::types::AddPeerRequest;

use crate::steps::{indexer, miner};

const BASE_NODE: &str = "BASE_NODE";
const MINOTARI_WALLET: &str = "MINOTARI_WALLET";
const MINER: &str = "MINER";

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HybridNetworkSpec {
    validators: Vec<HybridValidatorSpec>,
    #[serde(default = "default_indexer")]
    indexer: String,
    #[serde(default = "default_wallet_daemon")]
    wallet_daemon: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HybridValidatorSpec {
    name: String,
    /// The validator activates in the epoch after this one.
    #[serde(default)]
    registration_epoch: u64,
    #[serde(default)]
    exit_epoch: Option<u64>,
    /// Whether the indexer's outdated config lists this validator. That config schedules no exits.
    #[serde(default = "listed")]
    known_to_indexer: bool,
}

fn default_indexer() -> String {
    "INDEXER".to_string()
}

fn default_wallet_daemon() -> String {
    "WALLET_D".to_string()
}

fn listed() -> bool {
    true
}

/// Starts a base node, miner, the validators of the spec, an indexer and a wallet daemon, and mines until every
/// validator registered in epoch 0 is running epoch 2.
///
/// Every validator is started here, including those that activate later, so a late joiner has to sync the state
/// committed before it activates.
#[given(expr = "a hybrid oracle network with spec")]
async fn a_hybrid_oracle_network(world: &mut TariWorld, step: &Step) {
    let spec = yaml_serde::from_str::<HybridNetworkSpec>(
        step.docstring
            .as_ref()
            .expect("A hybrid oracle network needs a spec docstring"),
    )
    .expect("Failed to parse the hybrid oracle network spec");

    world.set_consensus_constants(&ConsensusConstantsFile::default());
    spawn_base_node(world, BASE_NODE.to_string()).await;
    spawn_minotari_wallet(world, MINOTARI_WALLET.to_string(), BASE_NODE.to_string()).await;
    register_miner_process(
        world,
        MINER.to_string(),
        BASE_NODE.to_string(),
        MINOTARI_WALLET.to_string(),
    );

    // A validator listed in the config must run under the key it is listed with, so its identity is created before
    // the config is written.
    let mut current = Vec::with_capacity(spec.validators.len());
    let mut fee_claim_keys = Vec::with_capacity(spec.validators.len());
    for vn in &spec.validators {
        let dir = validator_node_dir(world, &vn.name);
        fs::create_dir_all(&dir).expect("Failed to create the validator node directory");
        let identity =
            create_new_keypair(validator_node_identity_file(&dir)).expect("Failed to create a validator identity");
        let fee_claim_key = RistrettoKeypair::random(&mut rand::rng()).public_key().clone();
        current.push(Validator {
            public_key: identity.public_key().to_byte_type(),
            claim_key: fee_claim_key.to_byte_type(),
            shard_group: ShardGroup::new(1u32, 256u32),
            registration_epoch: Epoch(vn.registration_epoch),
            claim_key_changes: vec![],
            power: NonZeroU64::MIN,
            exit_epoch: vn.exit_epoch.map(Epoch),
        });
        fee_claim_keys.push(fee_claim_key);
    }
    let outdated = spec
        .validators
        .iter()
        .zip(&current)
        .filter(|(vn, _)| vn.known_to_indexer)
        .map(|(_, validator)| Validator {
            exit_epoch: None,
            ..validator.clone()
        })
        .collect();

    world.epoch_oracle = Some(hybrid_oracle(oracle_config(current), "current"));
    let indexer_oracle = hybrid_oracle(oracle_config(outdated), "outdated");

    for (vn, fee_claim_key) in spec.validators.iter().zip(fee_claim_keys) {
        let process =
            spawn_validator_node_with_fee_claim_key(world, vn.name.clone(), BASE_NODE.to_string(), fee_claim_key).await;
        world.validator_nodes.insert(vn.name.clone(), process);
    }
    connect_validators_to_each_other(world).await;

    spawn_indexer_with_epoch_oracle(world, spec.indexer.clone(), BASE_NODE.to_string(), indexer_oracle).await;
    let indexer = world.get_indexer(&spec.indexer);
    for vn in world.validator_nodes.values() {
        indexer
            .add_peer(vn.public_key, vec![multiaddr!(Ip4([127, 0, 0, 1]), Tcp(vn.p2p_port))])
            .await;
    }
    spawn_wallet_daemon(world, spec.wallet_daemon.clone(), spec.indexer.clone()).await;

    let first_validators = spec
        .validators
        .iter()
        .filter(|vn| vn.registration_epoch == 0)
        .map(|vn| vn.name.clone())
        .collect::<Vec<_>>();
    assert!(
        !first_validators.is_empty(),
        "A hybrid oracle network needs a validator registered in epoch 0"
    );

    miner::miner_mines_to_next_epoch(world, step, MINER.to_string()).await;
    for name in &first_validators {
        world.get_validator_node(name).wait_for_consensus_to_start().await;
    }
    miner::miner_mines_to_next_epoch(world, step, MINER.to_string()).await;
    for name in &first_validators {
        validator_reaches_epoch(world, name, Epoch(2)).await;
    }
    let tip = world
        .get_base_node(BASE_NODE)
        .create_client()
        .get_tip_info()
        .await
        .expect("Failed to get the base node tip")
        .height_of_longest_chain;
    indexer::indexer_has_scanned_to_at_least_height(
        world,
        step,
        spec.indexer.clone(),
        tip.saturating_sub(world.consensus_constants.base_layer_confirmations),
    )
    .await;
}

fn oracle_config(validators: Vec<Validator>) -> Config {
    let config = Config {
        epoch_time: None,
        initial_epoch: Epoch(0),
        base_time: time::OffsetDateTime::UNIX_EPOCH,
        validators,
    };
    config.validate().expect("The hybrid oracle config is invalid");
    config
}

fn hybrid_oracle(config: Config, label: &str) -> EpochOracleConfig {
    let json = serde_json::to_string(&config).expect("Failed to serialize the hybrid oracle config");
    cucumber_log!("Hybrid oracle config ({label}): {json}");
    let mut oracle = EpochOracleConfig::default();
    oracle.oracle_type = EpochOracleType::Hybrid;
    oracle.configured = ConfiguredOracleConfig {
        config_file: None,
        config_json: Some(json),
    };
    oracle
}

async fn connect_validators_to_each_other(world: &TariWorld) {
    for vn in world.validator_nodes.values() {
        let mut client = vn.create_client();
        for other in world.validator_nodes.values().filter(|other| other.name != vn.name) {
            client
                .add_peer(AddPeerRequest {
                    public_key: other.public_key,
                    addresses: other.get_addresses(),
                    wait_for_dial: false,
                })
                .await
                .expect("Failed to add a validator peer");
        }
    }
}

async fn validator_reaches_epoch(world: &TariWorld, name: &str, epoch: Epoch) {
    let mut client = world.get_validator_node(name).create_client();
    wait_until(Duration::from_secs(120), async || {
        match client.get_consensus_status().await {
            Ok(status) if status.epoch >= epoch && status.state == "Running" => ControlFlow::Break(()),
            Ok(status) => ControlFlow::Continue(format!("epoch {}, state {}", status.epoch, status.state)),
            Err(err) => ControlFlow::Continue(format!("status unavailable: {err}")),
        }
    })
    .await
    .unwrap_or_else(|err| panic!("Validator node {name} did not reach {epoch}: {err}"));
}

/// The indexer's committee for `epoch` must hash to the validator set that the validators' end-of-epoch checkpoint for
/// the epoch before names, and consist of exactly `members` (comma separated validator names).
///
/// The checkpoint carries the validators' quorum certificate, so an indexer whose view of the validator set diverged
/// from theirs cannot pass this.
#[then(expr = "indexer {word} proves the committee of epoch {int} is {word}")]
async fn indexer_proves_committee(
    world: &mut TariWorld,
    step: &Step,
    indexer_name: String,
    epoch: u64,
    members: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let epoch = Epoch(epoch);
    let checkpoint_epoch = epoch
        .checked_sub(Epoch(1))
        .expect("Epoch 0 has no checkpoint before it");
    let names_by_key = world
        .validator_nodes
        .values()
        .map(|vn| (vn.public_key, vn.name.clone()))
        .collect::<Vec<_>>();
    let name_of = |key: &RistrettoPublicKeyBytes| {
        names_by_key
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(|| key.to_string(), |(_, name)| name.clone())
    };
    let expected = members
        .split(',')
        .map(|name| world.get_validator_node(name.trim()).public_key)
        .collect::<BTreeSet<_>>();

    let indexer = world.get_indexer(&indexer_name);
    let client = indexer.get_indexer_client();
    let committee = wait_until(Duration::from_secs(180), async || {
        assert!(
            !indexer.handle.is_finished(),
            "Indexer {indexer_name} stopped while waiting for its committee of {epoch}"
        );
        let response = match client
            .list_epoch_checkpoints(ListEpochCheckpointsRequest {
                from_epoch: Some(checkpoint_epoch),
                limit: Some(1),
                shard: Some(Shard::from(1u32)),
                include_committees: true,
            })
            .await
        {
            Ok(response) => response,
            Err(err) => return ControlFlow::Continue(format!("checkpoints unavailable: {err}")),
        };
        let Some(checkpoint) = response
            .checkpoints
            .into_iter()
            .find(|checkpoint| checkpoint.epoch == checkpoint_epoch)
        else {
            return ControlFlow::Continue(format!("no checkpoint of {checkpoint_epoch} yet"));
        };
        let Some(signed_hash) = checkpoint.next_epoch_validator_set_hash else {
            panic!("The checkpoint of {checkpoint_epoch} names no validator set for {epoch}");
        };
        let Some(committee) = checkpoint.next_epoch_committee else {
            return ControlFlow::Continue(format!("indexer holds no validator set for {epoch}"));
        };
        if committee.validator_set_hash != signed_hash {
            return ControlFlow::Continue(format!(
                "indexer's validator set for {epoch} hashes to {}, but the validators signed {signed_hash}",
                committee.validator_set_hash
            ));
        }
        ControlFlow::Break(committee)
    })
    .await
    .unwrap_or_else(|err| panic!("Indexer {indexer_name} did not prove its committee of {epoch}: {err}"));

    let actual = committee
        .members
        .iter()
        .map(|member| member.public_key)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual.iter().map(name_of).collect::<Vec<_>>(),
        expected.iter().map(name_of).collect::<Vec<_>>(),
        "Indexer {indexer_name} proved a committee of {epoch} other than the expected one"
    );
    cucumber_log!(
        "Indexer {} proved its committee of {}: {}",
        indexer_name,
        epoch,
        actual.iter().map(name_of).collect::<Vec<_>>().join(", ")
    );
}
