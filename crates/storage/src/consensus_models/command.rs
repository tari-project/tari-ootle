//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    cmp::Ordering,
    fmt::{Display, Formatter},
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_consensus_types::{BlockId, Decision};
use tari_ootle_common_types::{hashing::command_hasher, Epoch, ShardGroup};
use tari_ootle_transaction::TransactionId;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use super::{ForeignProposalAtom, LeaderFee, TransactionRecord};
use crate::{
    consensus_models::evidence::Evidence,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionAtom {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub id: TransactionId,
    pub decision: Decision,
    pub evidence: Evidence,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub transaction_fee: u64,
    pub leader_fee: Option<LeaderFee>,
}

impl TransactionAtom {
    pub fn id(&self) -> &TransactionId {
        &self.id
    }

    pub fn get_transaction<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<TransactionRecord, StorageError> {
        TransactionRecord::get(tx, &self.id)
    }
}

impl Display for TransactionAtom {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TransactionAtom({}, {}, {}, ",
            self.id, self.decision, self.transaction_fee,
        )?;
        match self.leader_fee {
            Some(ref leader_fee) => write!(f, "{}", leader_fee)?,
            None => write!(f, "--")?,
        }
        write!(f, ")")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Command {
    // Transaction Commands
    /// Request validators to prepare a local-only transaction
    LocalOnly(TransactionAtom),
    /// Request validators to prepare a transaction.
    LocalPrepare(TransactionAtom),
    /// Request validators to  agree that all involved shard groups prepared the transaction and
    /// accept (i.e. accept COMMIT/ABORT decision) a transaction. All foreign inputs are received
    /// and the transaction is executed with the same decision.
    LocalAccept(TransactionAtom),
    /// Request validators to agree that all involved shard groups agreed to ACCEPT the transaction.
    AllAccept(TransactionAtom),
    /// Request validators to agree that one or more involved shard groups did not agreed to ACCEPT the transaction.
    SomeAccept(TransactionAtom),
    // Validator node commands
    ForeignProposal(ForeignProposalAtom),
    EvictNode(EvictNodeAtom),
    EndEpoch,
}

/// Defines the order in which commands should be processed in a block. "Smallest" comes first and "largest" comes last.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CommandOrdering<'a> {
    EvictNode,
    /// Foreign proposals should come first in the block so that they are processed before commands
    ForeignProposal(ShardGroup, &'a BlockId),
    TransactionId(&'a TransactionId),
    EndEpoch,
}

impl Command {
    pub fn transaction(&self) -> Option<&TransactionAtom> {
        match self {
            Command::LocalPrepare(tx) |
            Command::LocalAccept(tx) |
            Command::AllAccept(tx) |
            Command::SomeAccept(tx) |
            Command::LocalOnly(tx) => Some(tx),
            Command::ForeignProposal(_) | Command::EvictNode(_) | Command::EndEpoch => None,
        }
    }

    fn as_ordering(&self) -> CommandOrdering<'_> {
        match self {
            Command::LocalPrepare(tx) |
            Command::LocalAccept(tx) |
            Command::AllAccept(tx) |
            Command::SomeAccept(tx) |
            Command::LocalOnly(tx) => CommandOrdering::TransactionId(&tx.id),
            Command::ForeignProposal(foreign_proposal) => {
                // Order by shard group then by block id
                CommandOrdering::ForeignProposal(foreign_proposal.shard_group, &foreign_proposal.block_id)
            },
            Command::EvictNode(_) => CommandOrdering::EvictNode,
            Command::EndEpoch => CommandOrdering::EndEpoch,
        }
    }

    pub fn hash(&self) -> FixedHash {
        command_hasher().chain(self).finalize().into()
    }

    pub fn local_only(&self) -> Option<&TransactionAtom> {
        match self {
            Command::LocalOnly(tx) => Some(tx),
            _ => None,
        }
    }

    pub fn local_prepare(&self) -> Option<&TransactionAtom> {
        match self {
            Command::LocalPrepare(tx) => Some(tx),
            _ => None,
        }
    }

    pub fn local_accept(&self) -> Option<&TransactionAtom> {
        match self {
            Command::LocalAccept(tx) => Some(tx),
            _ => None,
        }
    }

    pub fn foreign_proposal(&self) -> Option<&ForeignProposalAtom> {
        match self {
            Command::ForeignProposal(tx) => Some(tx),
            _ => None,
        }
    }

    pub fn evict_node(&self) -> Option<&EvictNodeAtom> {
        match self {
            Command::EvictNode(atom) => Some(atom),
            _ => None,
        }
    }

    pub fn all_accept(&self) -> Option<&TransactionAtom> {
        match self {
            Command::AllAccept(tx) => Some(tx),
            _ => None,
        }
    }

    pub fn some_accept(&self) -> Option<&TransactionAtom> {
        match self {
            Command::SomeAccept(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns Some if the command should result in finalising (COMMITing or ABORTing) the transaction, otherwise None.
    pub fn finalising(&self) -> Option<&TransactionAtom> {
        self.all_accept()
            .or_else(|| self.some_accept())
            .or_else(|| self.local_only())
    }

    /// Returns Some if the command should result in committing the transaction, otherwise None.
    pub fn committing(&self) -> Option<&TransactionAtom> {
        self.all_accept()
            .or_else(|| self.local_only())
            .filter(|t| t.decision.is_commit())
    }

    /// Returns Some if the command **will** result in aborting the transaction, otherwise None.
    pub fn aborting(&self) -> Option<&TransactionAtom> {
        self.some_accept()
            .or_else(|| self.local_prepare())
            .or_else(|| self.local_accept())
            .or_else(|| self.local_only())
            .filter(|t| t.decision.is_abort())
    }

    pub fn is_epoch_end(&self) -> bool {
        matches!(self, Command::EndEpoch)
    }

    pub fn is_local_prepare(&self) -> bool {
        matches!(self, Command::LocalPrepare(_))
    }

    pub fn is_local_accept(&self) -> bool {
        matches!(self, Command::LocalAccept(_))
    }
}

impl PartialOrd for Command {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Command {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_ordering().cmp(&other.as_ordering())
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::LocalOnly(tx) => write!(f, "LocalOnly({}, {})", tx.id, tx.decision),
            Command::LocalPrepare(tx) => write!(f, "LocalPrepare({}, {})", tx.id, tx.decision),
            Command::LocalAccept(tx) => write!(f, "LocalAccept({}, {})", tx.id, tx.decision),
            Command::AllAccept(tx) => write!(f, "AllAccept({}, {})", tx.id, tx.decision),
            Command::SomeAccept(tx) => write!(f, "SomeAccept({}, {})", tx.id, tx.decision),
            Command::ForeignProposal(fp) => write!(f, "ForeignProposal {}", fp.block_id),
            Command::EvictNode(atom) => write!(f, "EvictNode({atom})"),
            Command::EndEpoch => write!(f, "EndEpoch"),
        }
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
pub struct EvictNodeAtom {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    #[serde(with = "ootle_serde::hex")]
    pub public_key: RistrettoPublicKeyBytes,
}

impl EvictNodeAtom {
    pub fn mark_as_committed_in_epoch<TTx: StateStoreWriteTransaction>(
        &self,
        tx: &mut TTx,
        epoch: Epoch,
    ) -> Result<(), StorageError> {
        tx.evicted_nodes_mark_eviction_as_committed(&self.public_key, epoch)
    }
}

impl Display for EvictNodeAtom {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.public_key)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn ordering() {
        assert!(
            CommandOrdering::ForeignProposal(ShardGroup::new(32, 63), &BlockId::zero()) >
                CommandOrdering::ForeignProposal(ShardGroup::new(0, 31), &BlockId::zero())
        );
        assert!(
            CommandOrdering::ForeignProposal(ShardGroup::new(0, 64), &BlockId::zero()) <
                CommandOrdering::TransactionId(&TransactionId::default())
        );
        let tx_id = TransactionId::new([1; 32]);

        assert!(CommandOrdering::TransactionId(&TransactionId::default()) < CommandOrdering::TransactionId(&tx_id));
        assert!(CommandOrdering::TransactionId(&tx_id) < CommandOrdering::EndEpoch);
        let mut set = BTreeSet::new();
        let cmds = [
            Command::EndEpoch,
            Command::AllAccept(TransactionAtom {
                id: TransactionId::new([1; 32]),
                decision: Decision::Commit,
                evidence: Evidence::default(),
                transaction_fee: 0,
                leader_fee: None,
            }),
            Command::ForeignProposal(ForeignProposalAtom {
                block_id: BlockId::zero(),
                shard_group: ShardGroup::new(0, 64),
            }),
            Command::LocalPrepare(TransactionAtom {
                id: TransactionId::default(),
                decision: Decision::Commit,
                evidence: Evidence::default(),
                transaction_fee: 0,
                leader_fee: None,
            }),
        ];
        let expected = [cmds[2].clone(), cmds[3].clone(), cmds[1].clone(), cmds[0].clone()];
        set.extend(cmds);

        // Check the ordering in the set
        let mut iter = set.iter();
        for exp in &expected {
            let next = iter.next().unwrap();
            assert_eq!(next, exp);
        }
    }
}
