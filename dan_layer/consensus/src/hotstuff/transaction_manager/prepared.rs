//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{
    committee::CommitteeInfo,
    NumPreshards,
    ShardGroup,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_dan_storage::consensus_models::{Decision, Evidence, TransactionExecution};
use tari_transaction::TransactionId;

use crate::hotstuff::substate_store::LockStatus;

#[derive(Debug)]
pub enum PreparedTransaction {
    LocalOnly(Box<LocalPreparedTransaction>),
    MultiShard(MultiShardPreparedTransaction),
}

impl PreparedTransaction {
    pub fn new_local_accept(executed: TransactionExecution, lock_status: LockStatus) -> Self {
        Self::LocalOnly(Box::new(LocalPreparedTransaction::Accept {
            execution: executed,
            lock_status,
        }))
    }

    pub fn new_local_early_abort(execution: TransactionExecution) -> Self {
        Self::LocalOnly(Box::new(LocalPreparedTransaction::EarlyAbort { execution }))
    }

    pub fn lock_status(&self) -> &LockStatus {
        static DEFAULT_LOCK_STATUS: LockStatus = LockStatus::new();
        match self {
            Self::LocalOnly(local) => match &**local {
                LocalPreparedTransaction::Accept { lock_status, .. } => lock_status,
                LocalPreparedTransaction::EarlyAbort { .. } => &DEFAULT_LOCK_STATUS,
            },
            Self::MultiShard(multishard) => &multishard.lock_status,
        }
    }

    pub fn transaction_id(&self) -> &TransactionId {
        match self {
            PreparedTransaction::LocalOnly(local) => match &**local {
                LocalPreparedTransaction::Accept { execution, .. } => execution.id(),
                LocalPreparedTransaction::EarlyAbort { execution, .. } => execution.id(),
            },
            PreparedTransaction::MultiShard(multishard) => multishard.transaction_id(),
        }
    }

    pub fn is_involved(&self, committee_info: &CommitteeInfo) -> bool {
        let num_preshards = committee_info.num_preshards();
        let num_committees = committee_info.num_committees();
        let shard_group = committee_info.shard_group();
        if VersionedSubstateId::new(self.transaction_id().into_receipt_address(), 0)
            .to_substate_address()
            .to_shard_group(num_preshards, num_committees) ==
            shard_group
        {
            return true;
        }

        match self {
            PreparedTransaction::LocalOnly(local) => match &**local {
                LocalPreparedTransaction::Accept { execution, .. } => {
                    execution.is_involved(num_preshards, num_committees, shard_group)
                },
                LocalPreparedTransaction::EarlyAbort { execution, .. } => {
                    execution.is_involved(num_preshards, num_committees, shard_group)
                },
            },
            PreparedTransaction::MultiShard(multishard) => {
                multishard.is_involved(num_preshards, num_committees, shard_group)
            },
        }
    }

    pub fn into_lock_status(self) -> LockStatus {
        match self {
            Self::LocalOnly(local) => match *local {
                LocalPreparedTransaction::Accept { lock_status, .. } => lock_status,
                LocalPreparedTransaction::EarlyAbort { .. } => LockStatus::new(),
            },
            Self::MultiShard(multishard) => multishard.lock_status,
        }
    }

    pub fn new_multishard_executed(execution: TransactionExecution, lock_status: LockStatus) -> Self {
        Self::MultiShard(MultiShardPreparedTransaction {
            execution_or_evidence: EvidenceOrExecution::Execution(Box::new(execution)),
            lock_status,
        })
    }

    pub fn new_multishard_evidence(
        transaction_id: TransactionId,
        initial_evidence: Evidence,
        lock_status: LockStatus,
    ) -> Self {
        Self::MultiShard(MultiShardPreparedTransaction {
            execution_or_evidence: EvidenceOrExecution::Evidence {
                transaction_id,
                evidence: initial_evidence,
            },
            lock_status,
        })
    }
}

#[derive(Debug)]
pub enum LocalPreparedTransaction {
    Accept {
        execution: TransactionExecution,
        lock_status: LockStatus,
    },
    EarlyAbort {
        execution: TransactionExecution,
    },
}

#[derive(Debug)]
pub enum EvidenceOrExecution {
    Evidence {
        evidence: Evidence,
        transaction_id: TransactionId,
    },
    Execution(Box<TransactionExecution>),
}

impl EvidenceOrExecution {
    pub fn execution(&self) -> Option<&TransactionExecution> {
        match self {
            Self::Evidence { .. } => None,
            Self::Execution(e) => Some(e),
        }
    }
}

#[derive(Debug)]
pub struct MultiShardPreparedTransaction {
    execution_or_evidence: EvidenceOrExecution,
    lock_status: LockStatus,
}

impl MultiShardPreparedTransaction {
    pub fn current_decision(&self) -> Decision {
        self.execution_or_evidence
            .execution()
            .map(|e| e.decision())
            .unwrap_or(Decision::Commit)
    }

    pub fn into_evidence_or_execution(self) -> EvidenceOrExecution {
        self.execution_or_evidence
    }

    pub fn transaction_id(&self) -> &TransactionId {
        match &self.execution_or_evidence {
            EvidenceOrExecution::Evidence { transaction_id, .. } => transaction_id,
            EvidenceOrExecution::Execution(ex) => ex.id(),
        }
    }

    pub fn is_involved(&self, num_preshards: NumPreshards, num_committees: u32, shard_group: ShardGroup) -> bool {
        match &self.execution_or_evidence {
            EvidenceOrExecution::Evidence { evidence, .. } => evidence.has(&shard_group),
            EvidenceOrExecution::Execution(ex) => ex.is_involved(num_preshards, num_committees, shard_group),
        }
    }
}
