//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt, fmt::Display, str::FromStr};

use ootle_network::Network;
use serde::{Deserialize, Serialize};
use tari_template_lib::types::Hash32;

use crate::{
    Epoch,
    SubstateVersion,
    ValidatorFeeWithdrawal,
    events::Event,
    fees::FeeReceipt,
    substate::{SubstateDiff, SubstateId, hash_substate},
};

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionReceipt {
    #[n(0)]
    pub outcome: FinalizeOutcome,
    #[n(1)]
    pub diff_summary: DiffSummary,
    #[n(2)]
    pub fee_withdrawals: Box<[ValidatorFeeWithdrawal]>,
    #[n(3)]
    pub events: Box<[Event]>,
    #[n(4)]
    pub fee_receipt: FeeReceipt,
    #[n(5)]
    pub epoch: Epoch,
    /// Commitment to the transaction's intent: every field the signers authorized (network, fee
    /// instructions, instructions, inputs, epoch bounds, flags and blob commitments), excluding the
    /// signatures themselves.
    ///
    /// The receipt is already bound to the transaction transitively — it is addressed by the
    /// transaction id — but reproducing that id requires the signatures and the seal signature, so
    /// establishing the link that way reveals the signers. This commitment is over the same
    /// projection minus the signatures, so whoever holds the transaction can link it to this
    /// receipt without revealing who authorized it. It identifies the intent, not the signers:
    /// transactions differing only in who signed them share a commitment.
    #[n(6)]
    pub intent_commitment: Hash32,
}

impl TransactionReceipt {
    pub fn outcome(&self) -> &FinalizeOutcome {
        &self.outcome
    }

    pub fn diff_summary(&self) -> &DiffSummary {
        &self.diff_summary
    }

    pub fn fee_withdrawals(&self) -> &[ValidatorFeeWithdrawal] {
        &self.fee_withdrawals
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn fee_receipt(&self) -> &FeeReceipt {
        &self.fee_receipt
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn intent_commitment(&self) -> Hash32 {
        self.intent_commitment
    }

    /// An upper bound on the encoded size of the receipt that finalization will persist, computed
    /// from the parts that are already fixed when storage fees are charged.
    ///
    /// The receipt is the one persisted substate whose bytes cannot be counted alongside the rest:
    /// it is built after fees are settled, and it embeds the [`FeeReceipt`] that the charge derived
    /// from this bound feeds into. Measuring it exactly would require a fixed point, so the two
    /// parts that are not yet known are replaced by worst-case stand-ins — a fee receipt whose
    /// amounts all encode at full varint width, and a max-width version on each diff-summary entry.
    ///
    /// Events are measured as they will actually be encoded, save for the amount a fee-payment event
    /// records: that renders `max_fee` in decimal, so measuring it as written would reintroduce the
    /// very fixed point the fee receipt's stand-in exists to avoid. It is priced at its widest — see
    /// [`Event::charged_size_padding`]. `fee_withdrawals`, `epoch` and `intent_commitment` are
    /// measured as they will actually be encoded.
    ///
    /// `upped` is the substates the transaction writes, and `downed` the ones it removes without
    /// writing a later version. Each gets one [`DiffSummary`] entry. Nothing joins either set after
    /// the charge is computed: fee settlement only mutates substates already in `upped`. The
    /// receipt is up'd after its own summary is built, so it is absent from both.
    ///
    /// That holds only when both sets come from the same state the receipt is built from. Spending a
    /// UTXO or confidential output removes it from the state that spent it, so a state which never
    /// ran that instruction can carry an entry a later one has dropped — a fee-intent commit is
    /// exactly that case. Callers must bound the state whose receipt they are pricing.
    pub fn encoded_size_upper_bound<'a>(
        events: &[Event],
        fee_withdrawals: &[ValidatorFeeWithdrawal],
        upped: impl Iterator<Item = &'a SubstateId>,
        downed: impl Iterator<Item = &'a SubstateId>,
        epoch: Epoch,
    ) -> usize {
        let diff_summary = DiffSummary {
            upped: upped
                .map(|substate_id| UpSubstate {
                    substate_id: substate_id.clone(),
                    version: SubstateVersion::new(u64::MAX),
                    value_hash: Hash32::from_array([0xff; Hash32::LENGTH]),
                })
                .collect(),
            downed: downed
                .map(|substate_id| DownSubstate {
                    substate_id: substate_id.clone(),
                    version: SubstateVersion::new(u64::MAX),
                })
                .collect(),
        };

        let mut len = minicbor::len(&diff_summary);
        len += minicbor::len(events);
        len += events.iter().map(Event::charged_size_padding).sum::<usize>();
        len += minicbor::len(fee_withdrawals);
        len += minicbor::len(FeeReceipt::widest());
        len += minicbor::len(epoch);
        len += minicbor::len(Hash32::from_array([0xff; Hash32::LENGTH]));
        // The outcome discriminant and the array header wrapping the receipt's fields.
        len += minicbor::len(FinalizeOutcome::Commit) + 1;
        len
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FinalizeOutcome {
    #[n(0)]
    Commit,
    #[n(1)]
    FeeIntentCommit,
}

impl FinalizeOutcome {
    pub fn is_commit(&self) -> bool {
        matches!(self, FinalizeOutcome::Commit)
    }

    pub fn is_fee_intent_commit(&self) -> bool {
        matches!(self, FinalizeOutcome::FeeIntentCommit)
    }
}

impl Display for FinalizeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commit => write!(f, "Commit"),
            Self::FeeIntentCommit => write!(f, "FeeIntentCommit"),
        }
    }
}

impl FromStr for FinalizeOutcome {
    type Err = FinalizeOutcomeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Commit" => Ok(Self::Commit),
            "FeeIntentCommit" => Ok(Self::FeeIntentCommit),
            _ => Err(FinalizeOutcomeParseError),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid FinalizeOutcome string")]
pub struct FinalizeOutcomeParseError;

#[derive(
    Debug,
    Clone,
    Default,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiffSummary {
    #[n(0)]
    pub upped: Box<[UpSubstate]>,
    /// The substates the transaction downed without upping a later version: spent UTXOs and
    /// confidential outputs.
    ///
    /// A substate that is upped is always downed at its previous version, so those downs are implied
    /// by `upped` and are left out to keep the receipt small.
    #[n(1)]
    pub downed: Box<[DownSubstate]>,
}

impl DiffSummary {
    pub fn from_diff(network: Network, diff: &SubstateDiff, epoch: Epoch) -> Self {
        let upped_ids = diff.up_iter().map(|(id, _)| id).collect::<HashSet<_>>();
        Self {
            upped: diff
                .up_iter()
                .map(|(id, s)| UpSubstate {
                    substate_id: id.clone(),
                    version: s.version(),
                    value_hash: hash_substate(network, s.substate_value(), s.version(), epoch),
                })
                .collect(),
            downed: diff
                .down_iter()
                .filter(|(id, _)| !upped_ids.contains(id))
                .map(|(id, version)| DownSubstate {
                    substate_id: id.clone(),
                    version: *version,
                })
                .collect(),
        }
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpSubstate {
    #[n(0)]
    pub substate_id: SubstateId,
    #[n(1)]
    pub version: SubstateVersion,
    #[n(2)]
    pub value_hash: Hash32,
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DownSubstate {
    #[n(0)]
    pub substate_id: SubstateId,
    #[n(1)]
    pub version: SubstateVersion,
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{ComponentAddress, Metadata, ObjectKey};

    use super::*;
    use crate::substate::Substate;

    fn substate_id(seed: u8) -> SubstateId {
        SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([seed; ObjectKey::LENGTH])))
    }

    fn event(topic: &str) -> Event {
        let mut payload = Metadata::new();
        payload.insert("amount", "1000000");
        Event::new(
            Some(substate_id(0x02)),
            Hash32::from_array([0x03; Hash32::LENGTH]),
            topic.to_string(),
            payload,
        )
    }

    /// The bound must never come in under the receipt that finalization actually persists, or the
    /// storage charge would let some of its bytes through unpriced. It must also stay close, or a
    /// transaction pays for space it never uses. Adding a field to `TransactionReceipt` fails to
    /// compile here until the bound accounts for it.
    fn assert_bounds(receipt: &TransactionReceipt, upped: &[SubstateId], downed: &[SubstateId]) {
        let TransactionReceipt {
            outcome: _,
            diff_summary,
            fee_withdrawals,
            events,
            fee_receipt: _,
            epoch,
            intent_commitment: _,
        } = receipt;
        let DiffSummary {
            upped: summary_upped,
            downed: summary_downed,
        } = diff_summary;
        assert_eq!(summary_upped.len() + summary_downed.len(), upped.len() + downed.len());

        let bound =
            TransactionReceipt::encoded_size_upper_bound(events, fee_withdrawals, upped.iter(), downed.iter(), *epoch);
        let actual = minicbor::len(receipt);

        assert!(bound >= actual, "bound {bound} is under the actual size {actual}");
        // The slack is the worst-case fee receipt and the max-width versions, both fixed-size.
        assert!(bound - actual < 256, "bound {bound} overshoots {actual} by too much");
    }

    fn receipt(events: Vec<Event>, upped: &[SubstateId], downed: &[SubstateId]) -> TransactionReceipt {
        TransactionReceipt {
            outcome: FinalizeOutcome::Commit,
            diff_summary: DiffSummary {
                upped: upped
                    .iter()
                    .map(|substate_id| UpSubstate {
                        substate_id: substate_id.clone(),
                        version: SubstateVersion::new(1),
                        value_hash: Hash32::from_array([0x04; Hash32::LENGTH]),
                    })
                    .collect(),
                downed: downed
                    .iter()
                    .map(|substate_id| DownSubstate {
                        substate_id: substate_id.clone(),
                        version: SubstateVersion::new(0),
                    })
                    .collect(),
            },
            fee_withdrawals: Box::new([]),
            events: events.into_boxed_slice(),
            fee_receipt: FeeReceipt::default(),
            epoch: Epoch(1),
            intent_commitment: Hash32::from_array([0x05; Hash32::LENGTH]),
        }
    }

    #[test]
    fn bound_holds_for_an_empty_receipt() {
        assert_bounds(&receipt(vec![], &[], &[]), &[], &[]);
    }

    #[test]
    fn bound_holds_with_events_and_upped_and_downed_substates() {
        let upped = [substate_id(0x01), substate_id(0x02), substate_id(0x03)];
        let downed = [substate_id(0x04), substate_id(0x05)];
        assert_bounds(
            &receipt(vec![event("std.deposit"), event("custom.thing")], &upped, &downed),
            &upped,
            &downed,
        );
    }

    #[test]
    fn bound_grows_with_the_event_payload() {
        let small = receipt(vec![event("a")], &[], &[]);
        let large = receipt(vec![event(&"a".repeat(4096))], &[], &[]);

        let bound_of = |r: &TransactionReceipt| {
            TransactionReceipt::encoded_size_upper_bound(&r.events, &r.fee_withdrawals, [].iter(), [].iter(), r.epoch)
        };
        assert!(bound_of(&large) - bound_of(&small) >= 4000);
        assert_bounds(&large, &[], &[]);
    }

    #[test]
    fn summary_lists_only_the_downs_that_are_not_upped() {
        let rewritten = substate_id(0x01);
        let spent = substate_id(0x02);
        let drained = substate_id(0x03);
        let created = substate_id(0x04);
        let value = || Substate::new(SubstateVersion::new(0), receipt(vec![], &[], &[]));

        let mut diff = SubstateDiff::new();
        diff.down(rewritten.clone(), SubstateVersion::new(3));
        diff.up(rewritten.clone(), value());
        diff.down(drained.clone(), SubstateVersion::new(7));
        diff.up(created.clone(), value());
        diff.down(spent.clone(), SubstateVersion::new(0));

        let summary = DiffSummary::from_diff(Network::LocalNet, &diff, Epoch(1));

        let upped = summary.upped.iter().map(|u| &u.substate_id).collect::<Vec<_>>();
        assert_eq!(upped, [&rewritten, &created]);
        let downed = summary
            .downed
            .iter()
            .map(|d| (&d.substate_id, d.version))
            .collect::<Vec<_>>();
        assert_eq!(downed, [
            (&drained, SubstateVersion::new(7)),
            (&spent, SubstateVersion::new(0))
        ]);
    }
}
