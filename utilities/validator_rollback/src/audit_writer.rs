//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Glue between the storage-side `SubstateRewindPlanRow` / `BlocksAfterEpochRow` plans
//! and the audit-file record types. Keeps `main.rs` / `apply.rs` uncluttered by the
//! conversion mechanics.

use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, ShardGroup, SubstateVersion, shard::Shard};
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

    // Pass 2: derive per-substate net effect from the substate versions the reverted transitions touched. The rewind
    // deletes every record an Up created and restores every record a Down destroyed, so the version live after it is
    // the lowest one touched: the version that was live at the checkpoint. If an Up created that lowest version, it
    // came after the checkpoint too, every record is deleted, and the substate is removed.
    use std::collections::BTreeMap;
    struct Agg {
        shard: Shard,
        lowest: SubstateVersion,
        lowest_created_after_checkpoint: bool,
        highest: SubstateVersion,
    }
    let mut by_id: BTreeMap<&SubstateId, Agg> = BTreeMap::new();
    for row in rows {
        let created = row.transition == RewindTransitionKind::UpReverted;
        let entry = by_id.entry(&row.substate_id).or_insert_with(|| Agg {
            shard: row.shard,
            lowest: row.substate_version,
            lowest_created_after_checkpoint: created,
            highest: row.substate_version,
        });
        match row.substate_version.cmp(&entry.lowest) {
            std::cmp::Ordering::Less => {
                entry.lowest = row.substate_version;
                entry.lowest_created_after_checkpoint = created;
            },
            std::cmp::Ordering::Equal => entry.lowest_created_after_checkpoint |= created,
            std::cmp::Ordering::Greater => {},
        }
        entry.highest = entry.highest.max(row.substate_version);
    }

    for (substate_id, agg) in by_id {
        let (action, post_rollback_version) = if agg.lowest_created_after_checkpoint {
            (SubstateAction::Removed, None)
        } else {
            (SubstateAction::Rewound, Some(agg.lowest.as_u64()))
        };
        match action {
            SubstateAction::Removed => counters.substates_removed += 1,
            SubstateAction::Rewound => counters.substates_rewound += 1,
        }
        writer.write_record(&AuditRecord::SubstateSummary(SubstateSummary {
            substate_id: substate_id_display(substate_id),
            shard: shard_to_audit_shard(agg.shard),
            action,
            pre_rollback_version: agg.highest.as_u64(),
            post_rollback_version,
        }))?;
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use tari_engine_types::substate::SubstateId;
    use tari_template_lib_types::{ComponentAddress, ObjectKey};

    use super::*;
    use crate::audit::AuditReader;

    fn row(seed: u8, state_version: u64, version: u64, transition: RewindTransitionKind) -> SubstateRewindPlanRow {
        SubstateRewindPlanRow {
            substate_id: SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([seed; ObjectKey::LENGTH]))),
            shard: Shard::from(1u32),
            state_version,
            substate_version: SubstateVersion::new(version),
            transition,
            epoch: Epoch(1),
        }
    }

    fn summaries(rows: &[SubstateRewindPlanRow]) -> Vec<SubstateSummary> {
        let mut buf = Vec::new();
        let mut writer = AuditWriter::new(&mut buf).unwrap();
        write_substate_plan(&mut writer, &mut AuditCounters::default(), rows).unwrap();
        writer.finish().unwrap();
        AuditReader::new(buf.as_slice())
            .unwrap()
            .records()
            .filter_map(|r| match r.unwrap() {
                AuditRecord::SubstateSummary(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn summaries_are_emitted_in_deterministic_substate_id_order() {
        use RewindTransitionKind::*;
        let rows = [
            row(9, 10, 0, UpReverted),
            row(2, 10, 0, UpReverted),
            row(5, 10, 0, UpReverted),
        ];
        let out = summaries(&rows);
        assert_eq!(out.len(), 3);
        assert!(out[0].substate_id < out[1].substate_id);
        assert!(out[1].substate_id < out[2].substate_id);
    }

    #[test]
    fn a_substate_live_at_the_checkpoint_rewinds_to_that_version() {
        use RewindTransitionKind::*;
        // v5 was live at the checkpoint; one block downs it and ups v6, and the next downs v6 and ups v7. The Down and
        // Up of one write share a state version.
        let rows = [
            row(1, 11, 7, UpReverted),
            row(1, 11, 6, DownReverted),
            row(1, 10, 6, UpReverted),
            row(1, 10, 5, DownReverted),
        ];
        let [summary] = summaries(&rows).try_into().unwrap();
        assert_eq!(summary.action, SubstateAction::Rewound);
        assert_eq!(summary.pre_rollback_version, 7);
        assert_eq!(summary.post_rollback_version, Some(5));
    }

    #[test]
    fn a_substate_created_after_the_checkpoint_is_removed() {
        use RewindTransitionKind::*;
        let rows = [
            row(2, 11, 1, UpReverted),
            row(2, 11, 0, DownReverted),
            row(2, 10, 0, UpReverted),
        ];
        let [summary] = summaries(&rows).try_into().unwrap();
        assert_eq!(summary.action, SubstateAction::Removed);
        assert_eq!(summary.pre_rollback_version, 1);
        assert_eq!(summary.post_rollback_version, None);
    }

    #[test]
    fn a_version_beyond_u32_is_reported_whole() {
        use RewindTransitionKind::*;
        let big = u64::from(u32::MAX) + 10;
        let rows = [row(3, 10, big + 1, UpReverted), row(3, 10, big, DownReverted)];
        let [summary] = summaries(&rows).try_into().unwrap();
        assert_eq!(summary.pre_rollback_version, big + 1);
        assert_eq!(summary.post_rollback_version, Some(big));
    }
}
