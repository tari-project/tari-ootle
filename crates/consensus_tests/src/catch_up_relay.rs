//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A replica votes on a live proposal whatever copy of the block it processed first.
//!
//! A replica does not vote on a block it imports through catch-up, and a later delivery of a stored block is not
//! processed again. Any committee member can send a catch-up response, so one member that relays each live proposal to
//! the other replicas as a catch-up response, ahead of the proposer's own broadcast, has each replica process the
//! block as a catch-up block. The test requires that the replicas still vote on those blocks once the proposer's copy
//! arrives, and that the committee keeps committing.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use ootle_byte_type::ToByteType;
use tari_consensus::messages::HotstuffMessage;
use tari_consensus_types::{BlockId, Decision};
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{MessageFilter, MessageInjector, Test, TestAddress, helpers, logging::setup_logger};

const COMMITTED_HEIGHT_TARGET: NodeHeight = NodeHeight(6);

#[derive(Default)]
struct Observed {
    /// Blocks relayed as catch-up responses, by the replica they were relayed to.
    relayed: HashSet<(TestAddress, BlockId)>,
    /// Votes cast, by voter.
    votes: HashSet<(TestAddress, BlockId)>,
}

fn relayer() -> TestAddress {
    TestAddress::new("4")
}

/// Ahead of each proposal its proposer broadcasts to another replica, delivers the same proposal to that replica as a
/// catch-up response from the relayer.
fn relay_proposals_as_catch_up(observed: Arc<Mutex<Observed>>) -> MessageInjector {
    let relayer = relayer();
    Box::new(move |from, to, msg| {
        if *from == relayer || *to == relayer || from == to {
            return Vec::new();
        }
        let HotstuffMessage::Proposal(proposal) = msg else {
            return Vec::new();
        };
        let (_, sender_pk) = helpers::derive_keypair_from_address(from);
        if *proposal.block.proposed_by() != sender_pk.to_byte_type() {
            return Vec::new();
        }
        observed
            .lock()
            .unwrap()
            .relayed
            .insert((to.clone(), *proposal.block.id()));
        vec![(
            relayer.clone(),
            HotstuffMessage::new_catch_up_sync_response((**proposal).clone()),
        )]
    })
}

fn record_votes(observed: Arc<Mutex<Observed>>) -> MessageFilter {
    Box::new(move |from, _to, msg| {
        if let HotstuffMessage::Vote(vote) = msg {
            observed
                .lock()
                .unwrap()
                .votes
                .insert((from.clone(), vote.vote.block_id));
        }
        true
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replicas_vote_on_live_proposals_relayed_ahead_as_catch_up() {
    setup_logger();

    let observed = Arc::new(Mutex::new(Observed::default()));
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
            c.missed_proposal_suspend_threshold = 50;
        })
        .with_message_filter(record_votes(observed.clone()))
        .with_message_injector(relay_proposals_as_catch_up(observed.clone()))
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    let mut committed = NodeHeight::zero();
    while committed < COMMITTED_HEIGHT_TARGET {
        let (_, _, _, height) = tokio::time::timeout(Duration::from_secs(30), test.on_block_committed())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "no block committed within 30s (committed height {committed}) while proposals are relayed as \
                     catch-up responses"
                )
            });
        committed = committed.max(height);
    }

    {
        let observed = observed.lock().unwrap();
        assert!(
            !observed.relayed.is_empty(),
            "test premise: proposals must have been relayed as catch-up responses"
        );
        let voted_on_relayed = observed.relayed.intersection(&observed.votes).count();
        assert!(
            voted_on_relayed > 0,
            "no replica voted on any of the {} proposals relayed to it as catch-up responses",
            observed.relayed.len()
        );
    }

    test.stop();
    test.assert_clean_shutdown().await;
}
