//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Glue between the storage-side `SubstateRewindPlanRow` / `BlocksAfterEpochRow` plans
//! and the audit-file record types. Keeps `main.rs` / `apply.rs` uncluttered by the
//! conversion mechanics.

use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, ShardGroup, shard::Shard};
use tari_ootle_transaction::TransactionId;

use crate::{
    audit::{
        AuditFooter,
        AuditHeader,
        AuditRecord,
        AuditShard,
        AuditShardGroup,
        AuditWriter,
        SubstateAction,
        SubstateSummary,
        SubstateTransition,
        TransactionUnfinalised,
        TransitionKind,
    },
    storage::{BlocksAfterEpochRow, RewindTransitionKind, SubstateRewindPlanRow},
};

/// Structured counters accumulated while writing an audit file — become the footer.
#[derive(Default)]
pub struct AuditCounters {
    pub substates_removed: u64,
    pub substates_rewound: u64,
    pub substate_transitions: u64,
    pub transactions_unfinalised: u64,
    pub blocks_deleted: u64,
}

impl AuditCounters {
    pub fn into_footer(self) -> AuditFooter {
        AuditFooter {
            substates_removed: self.substates_removed,
            substates_rewound: self.substates_rewound,
            substate_transitions: self.substate_transitions,
            transactions_unfinalised: self.transactions_unfinalised,
            blocks_deleted: self.blocks_deleted,
        }
    }
}

pub fn write_header<W: std::io::Write>(
    writer: &mut AuditWriter<W>,
    target_epoch: Epoch,
    shard_group: ShardGroup,
    pre_rollback_tip: Option<(Epoch, BlockId)>,
    state_versions: Vec<(Shard, u64)>,
    tool_version: &str,
    generated_at_unix_secs: u64,
    dry_run: bool,
) -> Result<(), crate::audit::AuditError> {
    let header = AuditHeader {
        target_epoch: target_epoch.0,
        shard_group: AuditShardGroup {
            start: shard_group.start().as_u32(),
            end_inclusive: shard_group.end().as_u32(),
        },
        pre_rollback_tip_epoch: pre_rollback_tip.as_ref().map(|(e, _)| e.0),
        pre_rollback_tip_block: pre_rollback_tip.as_ref().map(|(_, b)| b.to_string()),
        state_version_per_shard: state_versions
            .into_iter()
            .map(|(shard, v)| (shard_to_audit_shard(shard), v))
            .collect(),
        generated_at_unix_secs,
        tool_version: tool_version.to_string(),
        dry_run,
    };
    writer.write_record(&AuditRecord::Header(header))
}

/// Consume the read-only substate rewind plan — one record stream for the per-transition
/// trail + a second pass that rolls into per-substate summaries. Updates counters.
pub fn write_substate_plan<W: std::io::Write>(
    writer: &mut AuditWriter<W>,
    counters: &mut AuditCounters,
    rows: &[SubstateRewindPlanRow],
) -> Result<(), crate::audit::AuditError> {
    // Pass 1: emit the full transition trail in reverse-application order (the storage
    // layer yields it this way).
    for row in rows {
        let transition = match row.transition {
            RewindTransitionKind::UpReverted => TransitionKind::Up,
            RewindTransitionKind::DownReverted => TransitionKind::Down,
        };
        writer.write_record(&AuditRecord::SubstateTransition(SubstateTransition {
            substate_id: substate_id_display(&row.substate_id),
            shard: shard_to_audit_shard(row.shard),
            state_version: row.state_version,
            transition,
            epoch: row.epoch.0,
        }))?;
        counters.substate_transitions += 1;
    }

    // Pass 2: derive per-substate net effect. For each affected substate track:
    //   - shard (constant for all its rows)
    //   - kind of its *earliest* post-checkpoint transition (smallest state_version) — if Up, the substate didn't exist
    //     at checkpoint and will be Removed; otherwise it existed and will be Rewound.
    //   - lowest + highest state_version touched, for the informational pre/post fields.
    use std::collections::HashMap;
    struct Agg {
        shard: Shard,
        earliest_version: u64,
        earliest_kind: RewindTransitionKind,
        highest_version: u64,
    }
    let mut by_id: HashMap<&SubstateId, Agg> = HashMap::new();
    for row in rows {
        let entry = by_id.entry(&row.substate_id).or_insert_with(|| Agg {
            shard: row.shard,
            earliest_version: row.state_version,
            earliest_kind: row.transition,
            highest_version: row.state_version,
        });
        if row.state_version < entry.earliest_version {
            entry.earliest_version = row.state_version;
            entry.earliest_kind = row.transition;
        }
        if row.state_version > entry.highest_version {
            entry.highest_version = row.state_version;
        }
    }

    for (substate_id, agg) in by_id {
        let (action, post_rollback_version) = match agg.earliest_kind {
            RewindTransitionKind::UpReverted => (SubstateAction::Removed, None),
            // Earliest transition was Down: pre the rollback the substate was at its
            // highest version; post rollback the rewind restores it to whatever survived
            // before the earliest post-checkpoint transition, i.e. earliest_version - 1.
            // Zero is fine as a sentinel for "originally genesis".
            RewindTransitionKind::DownReverted => {
                let post = agg.earliest_version.saturating_sub(1);
                (SubstateAction::Rewound, Some(version_to_u32(post)))
            },
        };
        match action {
            SubstateAction::Removed => counters.substates_removed += 1,
            SubstateAction::Rewound => counters.substates_rewound += 1,
        }
        writer.write_record(&AuditRecord::SubstateSummary(SubstateSummary {
            substate_id: substate_id_display(substate_id),
            shard: shard_to_audit_shard(agg.shard),
            action,
            pre_rollback_version: version_to_u32(agg.highest_version),
            post_rollback_version,
        }))?;
    }

    Ok(())
}

fn version_to_u32(v: u64) -> u32 {
    // Substate versions fit well within u32 in practice. Saturating cast keeps the audit
    // honest for pathological large-version cases without panicking.
    v.try_into().unwrap_or(u32::MAX)
}

/// Emit per-block `TransactionUnfinalised` records and bump counters.
pub fn write_block_plan<W: std::io::Write>(
    writer: &mut AuditWriter<W>,
    counters: &mut AuditCounters,
    rows: &[BlocksAfterEpochRow],
) -> Result<(), crate::audit::AuditError> {
    for row in rows {
        counters.blocks_deleted += 1;
        for tx_id in &row.finalising_transaction_ids {
            writer.write_record(&AuditRecord::TransactionUnfinalised(TransactionUnfinalised {
                transaction_id: transaction_id_display(tx_id),
                finalised_in_block: row.block_id.to_string(),
                finalised_at_epoch: row.epoch.0,
            }))?;
            counters.transactions_unfinalised += 1;
        }
    }
    Ok(())
}

pub fn write_footer<W: std::io::Write>(
    writer: &mut AuditWriter<W>,
    counters: AuditCounters,
) -> Result<(), crate::audit::AuditError> {
    writer.write_record(&AuditRecord::Footer(counters.into_footer()))
}

fn shard_to_audit_shard(shard: Shard) -> AuditShard {
    if shard.is_global() {
        AuditShard::Global
    } else {
        AuditShard::Numbered(shard.as_u32())
    }
}

fn substate_id_display(id: &SubstateId) -> String {
    id.to_string()
}

fn transaction_id_display(id: &TransactionId) -> String {
    id.to_string()
}
