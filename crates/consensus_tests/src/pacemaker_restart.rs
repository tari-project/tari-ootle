//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the leader timeout a hotstuff worker arms when it starts again after an earlier start returned.
//!
//! The consensus state machine calls `HotstuffWorker::start` each time it enters `Running`. Each call resumes the
//! pacemaker from the stored high certificates: the view is the highest one a certificate justifies, and the leader
//! timeout backs off by `2^(view - high PC height)` seconds.

use std::{collections::HashMap, time::Duration};

use ootle_byte_type::ToByteType;
use tari_consensus::hotstuff::{HotstuffEvent, HotstuffWorker};
use tari_consensus_types::{HighPc, HighTc, TcId};
use tari_ootle_common_types::{
    Epoch,
    NodeHeight,
    ShardGroup,
    VotePower,
    committee::{Committee, CommitteeMember},
};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};
use tari_shutdown::Shutdown;
use tokio::{
    sync::broadcast,
    time::{self, Instant},
};

use crate::support::{
    SmoothWeightedRoundRobin,
    TEST_NUM_PRESHARDS,
    Test,
    TestAddress,
    TestConsensusSpec,
    TestEpochManager,
    UnstartedValidator,
    Validator,
    ValidatorBuilder,
    helpers::derive_keypair_from_address,
    logging::setup_logger,
};

const BLOCK_TIME: Duration = Duration::from_secs(10);
/// The network latency the pacemaker adds to every leader timeout.
const LATENCY_ALLOWANCE: Duration = Duration::from_secs(2);
const EPOCH: Epoch = Epoch(1);

/// The worker of validator "1" in a committee of four. The other members never run, so no view is certified and the
/// worker's leader timeouts are the only events that move it on.
struct RestartTest {
    validator: UnstartedValidator,
    events: broadcast::Receiver<HotstuffEvent>,
    _builder: ValidatorBuilder,
    _shutdown: Shutdown,
}

impl RestartTest {
    async fn new() -> Self {
        setup_logger();
        let config = Test::builder()
            .modify_consensus_constants(|constants| constants.pacemaker_block_time = BLOCK_TIME)
            .hotstuff_config();

        let committee: Committee<TestAddress> = ["1", "2", "3", "4"]
            .into_iter()
            .map(|address| {
                let address = TestAddress::new(address);
                let (_, public_key) = derive_keypair_from_address(&address);
                CommitteeMember {
                    address,
                    public_key: public_key.to_byte_type(),
                    vote_power: VotePower::of(1),
                }
            })
            .collect();
        let shard_group = ShardGroup::all_shards(TEST_NUM_PRESHARDS);
        let (tx_epoch_events, _) = broadcast::channel(10);
        let mut epoch_manager = TestEpochManager::new(tx_epoch_events);
        epoch_manager
            .add_committees(HashMap::from([(shard_group, committee)]))
            .await;
        epoch_manager.set_current_epoch(EPOCH, shard_group).await;

        let address = TestAddress::new("1");
        let (vn, _) = epoch_manager
            .all_validators()
            .await
            .into_iter()
            .find(|(vn, _)| vn.address == address)
            .expect("validator 1 is in the committee");
        let (secret_key, public_key) = derive_keypair_from_address(&address);

        let shutdown = Shutdown::new();
        let mut builder = Validator::builder();
        builder
            .with_config(config)
            .with_address_and_secret_key(address.clone(), secret_key)
            .with_shard(vn.shard_key)
            .with_shard_group(shard_group)
            .with_fee_claim_public_key(vn.fee_claim_public_key)
            .with_epoch_manager(epoch_manager.clone_for(address, public_key, vn.shard_key, vn.fee_claim_public_key))
            .with_leader_strategy(SmoothWeightedRoundRobin::new())
            .with_num_committees(1);
        let validator = builder.build(shutdown.to_signal());
        let events = validator.tx_events.subscribe();

        Self {
            validator,
            events,
            _builder: builder,
            _shutdown: shutdown,
        }
    }

    fn worker(&mut self) -> &mut HotstuffWorker<TestConsensusSpec> {
        &mut self.validator.worker
    }

    /// Starts the worker so that it returns an error once its pacemaker is running.
    async fn start_and_fail_after_pacemaker_start(&mut self) {
        self.validator.epoch_manager.fail_next_local_committee();
        let result = self.worker().start().await;
        assert!(
            result.is_err(),
            "start returned {result:?} with the local committee lookup failing"
        );
    }

    /// Starts the worker and returns how long it ran before its first leader timeout.
    async fn time_to_first_leader_timeout(&mut self) -> Duration {
        let started_at = Instant::now();
        let Self { validator, events, .. } = self;
        tokio::select! {
            result = validator.worker.start() => panic!("worker exited before a leader timeout: {result:?}"),
            () = next_leader_timeout(events) => started_at.elapsed(),
        }
    }
}

async fn next_leader_timeout(events: &mut broadcast::Receiver<HotstuffEvent>) {
    loop {
        match events.recv().await {
            Ok(HotstuffEvent::LeaderTimeout { .. }) => return,
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {},
            Err(broadcast::error::RecvError::Closed) => panic!("hotstuff event channel closed"),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_restart_after_a_failed_start_waits_a_full_leader_timeout() {
    let mut test = RestartTest::new().await;
    test.start_and_fail_after_pacemaker_start().await;

    // The state machine backs off before it starts the worker again, by which time the leader timeout the failed
    // start armed has passed.
    time::advance(Duration::from_secs(3600)).await;

    // The first view after genesis waits the block time plus a delta of twice the block time.
    let expected = BLOCK_TIME + BLOCK_TIME * 2 + LATENCY_ALLOWANCE;
    assert_eq!(test.time_to_first_leader_timeout().await, expected);
}

#[tokio::test(start_paused = true)]
async fn a_restart_after_consecutive_timeouts_resumes_the_backoff() {
    let mut test = RestartTest::new().await;
    // The first start stores the genesis block and its certificate as the high PC.
    test.start_and_fail_after_pacemaker_start().await;

    // Three timeouts past the high PC. Only the heights are read when the worker resumes; the high PC still names
    // the genesis certificate, which the worker's NEWVIEW carries.
    test.validator
        .channels
        .state_store
        .with_write_tx(|tx| {
            let mut high_pc = HighPc::get(&**tx, EPOCH)?;
            high_pc.block_height = NodeHeight(2);
            high_pc.set(tx)?;
            HighTc {
                epoch: EPOCH,
                height: NodeHeight(5),
                tc_id: TcId::zero(),
            }
            .set(tx)
        })
        .unwrap();

    let expected = BLOCK_TIME + Duration::from_secs(2u64.pow(5 - 2)) + LATENCY_ALLOWANCE;
    assert_eq!(test.time_to_first_leader_timeout().await, expected);
}
