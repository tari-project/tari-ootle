//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, mem};

use serde::{Deserialize, Serialize};
use tari_crypto::{
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey},
    tari_utilities::ByteArray,
};
use tari_template_abi::rust::collections::BTreeSet;
use tari_template_lib::{
    models::{
        ConfidentialOutputStatement,
        ConfidentialWithdrawProof,
        NonFungibleAddress,
        NonFungibleId,
        ResourceAddress,
    },
    prelude::ResourceType,
    types::{crypto::PedersenCommitmentBytes, Amount},
};

use crate::{
    confidential::{validate_confidential_statement, validate_confidential_withdraw},
    crypto::PrivateOutput,
    substate::SubstateId,
    ToByteType,
};

/// Instances of a single resource kept in Buckets and Vaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum ResourceContainer {
    Fungible {
        address: ResourceAddress,
        amount: Amount,
        locked_amount: Amount,
    },
    NonFungible {
        address: ResourceAddress,
        token_ids: BTreeSet<NonFungibleId>,
        locked_token_ids: BTreeSet<NonFungibleId>,
    },
    Confidential {
        address: ResourceAddress,
        commitments: BTreeMap<PedersenCommitmentBytes, PrivateOutput>,
        revealed_amount: Amount,
        locked_commitments: BTreeMap<PedersenCommitmentBytes, PrivateOutput>,
        locked_revealed_amount: Amount,
    },
}

impl ResourceContainer {
    pub fn fungible<T: Into<Amount>>(address: ResourceAddress, amount: T) -> ResourceContainer {
        ResourceContainer::Fungible {
            address,
            amount: amount.into(),
            locked_amount: Amount::zero(),
        }
    }

    pub fn non_fungible(address: ResourceAddress, token_ids: BTreeSet<NonFungibleId>) -> ResourceContainer {
        ResourceContainer::NonFungible {
            address,
            token_ids,
            locked_token_ids: BTreeSet::new(),
        }
    }

    pub fn confidential<I: IntoIterator<Item = (PedersenCommitmentBytes, PrivateOutput)>>(
        address: ResourceAddress,
        commitment: I,
        revealed_amount: Amount,
    ) -> ResourceContainer {
        ResourceContainer::Confidential {
            address,
            commitments: commitment.into_iter().collect(),
            revealed_amount,
            locked_commitments: BTreeMap::new(),
            locked_revealed_amount: Amount::zero(),
        }
    }

    pub fn mint_confidential(
        address: ResourceAddress,
        proof: ConfidentialOutputStatement,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<ResourceContainer, ResourceError> {
        if proof.change_statement.is_some() {
            return Err(ResourceError::InvalidConfidentialMintWithChange);
        }
        if !proof.change_revealed_amount.is_zero() {
            return Err(ResourceError::InvalidConfidentialProof {
                details: "Change revealed amount must be zero for minting".to_string(),
            });
        }
        let validated_proof = validate_confidential_statement(&proof, view_key)?;
        assert!(
            validated_proof.change_output.is_none(),
            "invariant failed: validate_confidential_proof returned change with no change in input proof"
        );
        Ok(ResourceContainer::Confidential {
            address,
            commitments: validated_proof
                .output
                .into_iter()
                .map(|o| (o.commitment.to_byte_type(), o.into()))
                .collect(),
            revealed_amount: validated_proof.output_revealed_amount,
            locked_commitments: BTreeMap::new(),
            locked_revealed_amount: Amount::zero(),
        })
    }

    pub fn amount(&self) -> Amount {
        match self {
            ResourceContainer::Fungible { amount, .. } => *amount,
            ResourceContainer::NonFungible { token_ids, .. } => Amount::new(token_ids.len().into()),
            ResourceContainer::Confidential { revealed_amount, .. } => *revealed_amount,
        }
    }

    pub fn number_of_confidential_commitments(&self) -> usize {
        match self {
            ResourceContainer::Confidential {
                commitments,
                locked_commitments,
                ..
            } => commitments.len() + locked_commitments.len(),
            _ => 0,
        }
    }

    pub fn locked_amount(&self) -> Amount {
        match self {
            ResourceContainer::Fungible { locked_amount, .. } => *locked_amount,
            ResourceContainer::NonFungible { locked_token_ids, .. } => Amount::new(locked_token_ids.len().into()),
            ResourceContainer::Confidential {
                locked_revealed_amount, ..
            } => *locked_revealed_amount,
        }
    }

    pub fn get_commitment_count(&self) -> u64 {
        match self {
            ResourceContainer::Fungible { .. } => 0,
            ResourceContainer::NonFungible { .. } => 0,
            ResourceContainer::Confidential { commitments, .. } => commitments.len() as u64,
        }
    }

    pub fn resource_address(&self) -> &ResourceAddress {
        match self {
            ResourceContainer::Fungible { address, .. } => address,
            ResourceContainer::NonFungible { address, .. } => address,
            ResourceContainer::Confidential { address, .. } => address,
        }
    }

    pub fn resource_type(&self) -> ResourceType {
        match self {
            ResourceContainer::Fungible { .. } => ResourceType::Fungible,
            ResourceContainer::NonFungible { .. } => ResourceType::NonFungible,
            ResourceContainer::Confidential { .. } => ResourceType::Confidential,
        }
    }

    pub fn non_fungible_token_ids(&self) -> &BTreeSet<NonFungibleId> {
        static EMPTY_BTREE_SET: BTreeSet<NonFungibleId> = BTreeSet::new();
        match self {
            ResourceContainer::NonFungible { token_ids, .. } => token_ids,
            _ => &EMPTY_BTREE_SET,
        }
    }

    pub fn into_non_fungible_ids(self) -> Option<BTreeSet<NonFungibleId>> {
        match self {
            ResourceContainer::NonFungible { token_ids, .. } => Some(token_ids),
            _ => None,
        }
    }

    pub fn child_substates(&self) -> impl Iterator<Item = SubstateId> + '_ {
        self.non_fungible_token_ids()
            .iter()
            .map(|id| SubstateId::NonFungible(NonFungibleAddress::new(*self.resource_address(), id.clone())))
    }

    pub fn deposit(&mut self, other: Self) -> Result<(), ResourceError> {
        if self.resource_address() != other.resource_address() {
            return Err(ResourceError::ResourceAddressMismatch {
                expected: *self.resource_address(),
                actual: *other.resource_address(),
            });
        }

        match (self, other) {
            (
                Self::Fungible { amount, .. },
                Self::Fungible {
                    amount: other_amount, ..
                },
            ) => {
                *amount += other_amount;
            },
            (
                Self::NonFungible { token_ids, .. },
                Self::NonFungible {
                    token_ids: other_token_ids,
                    ..
                },
            ) => {
                // General protection against overflow. Currently, usize::MAX is the limit, however we likely
                // want to greatly reduce this limit w.r.t nfts to prevent memory exhaustion.
                if token_ids.len().checked_add(other_token_ids.len()).is_none() {
                    return Err(ResourceError::OperationNotAllowed(
                        "Non-fungible deposit would exceed maximum number of tokens".to_string(),
                    ));
                }
                token_ids.extend(other_token_ids);
            },
            (
                Self::Confidential {
                    commitments,
                    revealed_amount,
                    ..
                },
                Self::Confidential {
                    commitments: other_commitments,
                    revealed_amount: other_amount,
                    ..
                },
            ) => {
                for (commit, output) in other_commitments {
                    if commitments.insert(commit, output).is_some() {
                        return Err(ResourceError::InvariantError(
                            "Confidential deposit contained duplicate commitment".to_string(),
                        ));
                    }
                }
                *revealed_amount += other_amount;
            },
            (this, other) => {
                return Err(ResourceError::ResourceTypeMismatch {
                    operate: "deposit",
                    expected: this.resource_type(),
                    given: other.resource_type(),
                })
            },
        }
        Ok(())
    }

    pub fn withdraw(&mut self, withdraw_amt: Amount) -> Result<ResourceContainer, ResourceError> {
        if !withdraw_amt.is_positive() {
            return Err(ResourceError::InvariantError(format!(
                "Amount must be positive (greater than 0). Got :{withdraw_amt}"
            )));
        }
        match self {
            ResourceContainer::Fungible { amount, .. } => {
                if withdraw_amt > *amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket contained insufficient funds. Required: {}, Available: {}",
                            withdraw_amt, amount
                        ),
                    });
                }
                *amount -= withdraw_amt;
                Ok(ResourceContainer::fungible(*self.resource_address(), withdraw_amt))
            },
            ResourceContainer::NonFungible { token_ids, .. } => {
                if withdraw_amt > token_ids.len() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket contained insufficient tokens. Required: {}, Available: {}",
                            withdraw_amt,
                            token_ids.len()
                        ),
                    });
                }
                let num_to_take = usize::try_from(withdraw_amt)
                    .expect("checked that withdraw_amt < token_ids.len() therefore it is <= usize::MAX");
                let taken_tokens = (0..num_to_take)
                    .map(|_| {
                        token_ids
                            .pop_first()
                            .expect("Invariant violation: token_ids.len() < amt")
                    })
                    .collect();

                Ok(ResourceContainer::non_fungible(*self.resource_address(), taken_tokens))
            },
            ResourceContainer::Confidential { revealed_amount, .. } => {
                if withdraw_amt > *revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket contained insufficient revealed funds. Required: {}, Available: {}",
                            withdraw_amt, revealed_amount
                        ),
                    });
                }
                *revealed_amount -= withdraw_amt;
                Ok(ResourceContainer::confidential(
                    *self.resource_address(),
                    None,
                    withdraw_amt,
                ))
            },
        }
    }

    pub fn recall_all(&mut self) -> Result<ResourceContainer, ResourceError> {
        match self {
            ResourceContainer::Fungible { .. } | ResourceContainer::NonFungible { .. } => self.withdraw(self.amount()),
            ResourceContainer::Confidential {
                commitments,
                revealed_amount,
                ..
            } => {
                let amount = *revealed_amount;
                *revealed_amount = Amount::zero();
                let commitments = mem::take(commitments);
                Ok(ResourceContainer::confidential(
                    *self.resource_address(),
                    commitments,
                    amount,
                ))
            },
        }
    }

    pub fn withdraw_by_ids(&mut self, ids: &BTreeSet<NonFungibleId>) -> Result<ResourceContainer, ResourceError> {
        match self {
            ResourceContainer::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw by NFT token id from a fungible resource".to_string(),
            )),
            ResourceContainer::NonFungible { token_ids, .. } => {
                let taken_tokens = ids
                    .iter()
                    .map(|id| {
                        token_ids
                            .take(id)
                            .ok_or_else(|| ResourceError::NonFungibleTokenIdNotFound { token: id.clone() })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(ResourceContainer::non_fungible(*self.resource_address(), taken_tokens))
            },
            ResourceContainer::Confidential { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw by NFT token id from a confidential resource".to_string(),
            )),
        }
    }

    pub fn withdraw_confidential(
        &mut self,
        proof: ConfidentialWithdrawProof,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<ResourceContainer, ResourceError> {
        match self {
            ResourceContainer::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a fungible resource".to_string(),
            )),
            ResourceContainer::NonFungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a non-fungible resource".to_string(),
            )),
            ResourceContainer::Confidential {
                commitments,
                revealed_amount,
                ..
            } => {
                let inputs = proof
                    .inputs
                    .iter()
                    .map(|input| {
                        let commitment = PedersenCommitment::from_canonical_bytes(input.as_bytes()).map_err(|_| {
                            ResourceError::InvalidConfidentialProof {
                                details: "Malformed input commitment".to_string(),
                            }
                        })?;
                        match commitments.remove(input) {
                            Some(_) => Ok(commitment),
                            None => Err(ResourceError::InvalidConfidentialProof {
                                details: format!(
                                    "withdraw_confidential: input commitment {} not found in resource",
                                    commitment.as_public_key()
                                ),
                            }),
                        }
                    })
                    .collect::<Result<Vec<_>, ResourceError>>()?;

                let validated_proof = validate_confidential_withdraw(&inputs, view_key, proof)?;

                // Withdraw revealed amount
                if *revealed_amount < validated_proof.input_revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket contained insufficient funds with withdrawing revealed amount. Required: {}, \
                             Available: {}",
                            validated_proof.input_revealed_amount, revealed_amount
                        ),
                    });
                }
                *revealed_amount -= validated_proof.input_revealed_amount;

                if let Some(change) = validated_proof.change_output {
                    if commitments
                        .insert(change.commitment.to_byte_type(), change.into())
                        .is_some()
                    {
                        return Err(ResourceError::InvariantError(
                            "Confidential withdraw contained duplicate commitment in change commitment".to_string(),
                        ));
                    }

                    if *revealed_amount < validated_proof.change_revealed_amount {
                        return Err(ResourceError::InsufficientBalance {
                            details: format!(
                                "withdraw_confidential: resource container did not contain enough revealed funds for \
                                 change. Required: {}, Available: {}",
                                validated_proof.change_revealed_amount, revealed_amount
                            ),
                        });
                    }

                    *revealed_amount += validated_proof.change_revealed_amount;
                }

                Ok(ResourceContainer::confidential(
                    *self.resource_address(),
                    validated_proof.output.map(|o| (o.commitment.to_byte_type(), o.into())),
                    validated_proof.output_revealed_amount,
                ))
            },
        }
    }

    pub fn recall_confidential_commitments(
        &mut self,
        commitments: &BTreeSet<PedersenCommitmentBytes>,
        revealed_amount: Amount,
    ) -> Result<ResourceContainer, ResourceError> {
        match self {
            ResourceContainer::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a fungible resource".to_string(),
            )),
            ResourceContainer::NonFungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a non-fungible resource".to_string(),
            )),
            ResourceContainer::Confidential {
                commitments: existing_commitments,
                revealed_amount: existing_revealed_amount,
                ..
            } => {
                if *existing_revealed_amount < revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "recall_confidential_commitments: resource container did not contain enough revealed \
                             funds. Required: {}, Available: {}",
                            revealed_amount, existing_revealed_amount
                        ),
                    });
                }

                *existing_revealed_amount -= revealed_amount;

                let recalled = commitments
                    .iter()
                    .map(|commitment| {
                        let output = existing_commitments.remove(commitment).ok_or_else(|| {
                            ResourceError::InvalidConfidentialProof {
                                details: format!(
                                    "recall_confidential_commitments: input commitment {} not found in resource",
                                    commitment,
                                ),
                            }
                        })?;
                        Ok((*commitment, output))
                    })
                    .collect::<Result<Vec<_>, ResourceError>>()?;

                Ok(ResourceContainer::confidential(
                    *self.resource_address(),
                    recalled,
                    revealed_amount,
                ))
            },
        }
    }

    /// Returns all confidential commitments. If the resource is not confidential, None is returned.
    pub fn get_confidential_commitments(&self) -> Option<&BTreeMap<PedersenCommitmentBytes, PrivateOutput>> {
        match self {
            ResourceContainer::Fungible { .. } | ResourceContainer::NonFungible { .. } => None,
            ResourceContainer::Confidential { commitments, .. } => Some(commitments),
        }
    }

    pub fn into_confidential_commitments(self) -> Option<BTreeMap<PedersenCommitmentBytes, PrivateOutput>> {
        match self {
            ResourceContainer::Fungible { .. } | ResourceContainer::NonFungible { .. } => None,
            ResourceContainer::Confidential { commitments, .. } => Some(commitments),
        }
    }

    pub fn lock_all(&mut self) -> Result<ResourceContainer, ResourceError> {
        let resource_address = *self.resource_address();
        match self {
            ResourceContainer::Fungible {
                amount, locked_amount, ..
            } => {
                if amount.is_zero() {
                    return Err(ResourceError::InsufficientBalance {
                        details: "lock_all: resource container contained no funds".to_string(),
                    });
                }
                let newly_locked_amount = mem::take(amount);
                *locked_amount += newly_locked_amount;
                Ok(ResourceContainer::fungible(resource_address, newly_locked_amount))
            },
            ResourceContainer::NonFungible {
                token_ids,
                locked_token_ids,
                ..
            } => {
                if token_ids.is_empty() {
                    return Err(ResourceError::InsufficientBalance {
                        details: "lock_all: resource container contained no tokens".to_string(),
                    });
                }
                let newly_locked_token_ids = mem::take(token_ids);
                locked_token_ids.extend(newly_locked_token_ids.iter().cloned());

                Ok(ResourceContainer::non_fungible(
                    resource_address,
                    newly_locked_token_ids,
                ))
            },
            ResourceContainer::Confidential {
                commitments,
                revealed_amount,
                locked_commitments,
                locked_revealed_amount,
                ..
            } => {
                if commitments.is_empty() {
                    return Err(ResourceError::InsufficientBalance {
                        details: "lock_all: resource container contained no commitments".to_string(),
                    });
                }
                let newly_locked_commitments = mem::take(commitments);
                let newly_locked_revealed_amount = *revealed_amount;
                locked_commitments.extend(newly_locked_commitments.iter().map(|(c, o)| (*c, o.clone())));
                *locked_revealed_amount += newly_locked_revealed_amount;

                Ok(ResourceContainer::confidential(
                    resource_address,
                    newly_locked_commitments,
                    newly_locked_revealed_amount,
                ))
            },
        }
    }

    pub fn unlock(&mut self, container: ResourceContainer) -> Result<(), ResourceError> {
        if self.resource_type() != container.resource_type() {
            return Err(ResourceError::ResourceTypeMismatch {
                operate: "unlock",
                expected: self.resource_type(),
                given: container.resource_type(),
            });
        }
        if self.resource_address() != container.resource_address() {
            return Err(ResourceError::ResourceAddressMismatch {
                expected: *self.resource_address(),
                actual: *container.resource_address(),
            });
        }

        match self {
            ResourceContainer::Fungible {
                amount, locked_amount, ..
            } => {
                if *locked_amount < container.amount() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "unlock: resource container did not contain enough locked funds. Required: {}, Available: \
                             {}",
                            container.amount(),
                            locked_amount
                        ),
                    });
                }
                *amount += container.amount();
                *locked_amount -= container.amount();
            },
            ResourceContainer::NonFungible {
                token_ids,
                locked_token_ids,
                ..
            } => {
                if locked_token_ids.len() < container.non_fungible_token_ids().len() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "unlock: resource container did not contain enough locked tokens. Required: {}, \
                             Available: {}",
                            container.non_fungible_token_ids().len(),
                            locked_token_ids.len()
                        ),
                    });
                }
                for token in container.non_fungible_token_ids() {
                    let token = locked_token_ids.take(token).ok_or_else(|| {
                        ResourceError::InvariantError(format!(
                            "unlock: tried to unlock token {token} that was not locked",
                        ))
                    })?;
                    token_ids.insert(token);
                }
            },
            ResourceContainer::Confidential {
                commitments,
                locked_commitments,
                revealed_amount,
                locked_revealed_amount,
                ..
            } => {
                if (locked_commitments.len() as u64) < container.get_commitment_count() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "unlock: resource container did not contain enough locked commitments. Required: {}, \
                             Available: {}",
                            container.get_commitment_count(),
                            locked_commitments.len()
                        ),
                    });
                }

                if *locked_revealed_amount < container.amount() {
                    return Err(ResourceError::InvariantError(format!(
                        "unlock: resource container did not contain enough locked revealed amount. Required: {}, \
                         Available: {}",
                        container.amount(),
                        locked_revealed_amount
                    )));
                }

                for (commitment, _) in container.get_confidential_commitments().into_iter().flatten() {
                    let (commitment, output) = locked_commitments.remove_entry(commitment).ok_or_else(|| {
                        ResourceError::InvariantError(
                            "unlock: tried to unlock commitment that was not locked".to_string(),
                        )
                    })?;
                    if commitments.insert(commitment, output).is_some() {
                        return Err(ResourceError::InvariantError(
                            "unlock: container contained duplicate commitment".to_string(),
                        ));
                    }
                }
                *revealed_amount += container.amount();
                *locked_revealed_amount -= container.amount();
            },
        }

        Ok(())
    }

    pub fn lock_by_non_fungible_ids(&mut self, ids: BTreeSet<NonFungibleId>) -> Result<Self, ResourceError> {
        match self {
            ResourceContainer::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot lock by NFT token id from a fungible resource".to_string(),
            )),
            ResourceContainer::NonFungible {
                token_ids,
                locked_token_ids,
                ..
            } => {
                let mut newly_locked = BTreeSet::new();
                for id in ids {
                    if let Some(token) = token_ids.take(&id) {
                        newly_locked.insert(token.clone());
                        locked_token_ids.insert(token);
                    } else {
                        return Err(ResourceError::NonFungibleTokenIdNotFound { token: id });
                    }
                }
                Ok(ResourceContainer::non_fungible(*self.resource_address(), newly_locked))
            },
            ResourceContainer::Confidential { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot lock by NFT token id from a confidential resource".to_string(),
            )),
        }
    }

    pub fn lock_by_amount(&mut self, amount: Amount) -> Result<Self, ResourceError> {
        match self {
            ResourceContainer::Fungible {
                amount: available_amount,
                locked_amount,
                ..
            } => {
                if amount > *available_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "lock_by_amount: resource container did not contain enough funds. Required: {}, \
                             Available: {}",
                            amount, available_amount
                        ),
                    });
                }
                *available_amount -= amount;
                *locked_amount += amount;
                Ok(ResourceContainer::fungible(*self.resource_address(), amount))
            },
            ResourceContainer::NonFungible {
                token_ids,
                locked_token_ids,
                ..
            } => {
                if amount > token_ids.len() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "lock_by_amount: resource container did not contain enough tokens. Required: {}, \
                             Available: {}",
                            amount,
                            token_ids.len()
                        ),
                    });
                }
                let num_to_take = usize::try_from(amount)
                    .expect("checked that amount <= token_ids.len() therefore it is <= usize::MAX");
                let newly_locked_token_ids = (0..num_to_take)
                    .map(|_| {
                        token_ids
                            .pop_first()
                            .expect("Invariant violation: tokens.len() < amount")
                    })
                    .collect::<BTreeSet<_>>();
                locked_token_ids.extend(newly_locked_token_ids.iter().cloned());

                Ok(ResourceContainer::non_fungible(
                    *self.resource_address(),
                    newly_locked_token_ids,
                ))
            },
            ResourceContainer::Confidential {
                revealed_amount,
                locked_revealed_amount,
                ..
            } => {
                if amount > *revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "lock_by_amount: resource container did not contain enough revealed funds. Required: {}, \
                             Available: {}",
                            amount, revealed_amount
                        ),
                    });
                }
                *revealed_amount -= amount;
                *locked_revealed_amount += amount;
                Ok(ResourceContainer::confidential(*self.resource_address(), None, amount))
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("Attempted to {operate} a {expected} resource, but the resource type is {given}")]
    ResourceTypeMismatch {
        operate: &'static str,
        expected: ResourceType,
        given: ResourceType,
    },
    #[error("Resource addresses do not match: expected:{expected}, actual:{actual}")]
    ResourceAddressMismatch {
        expected: ResourceAddress,
        actual: ResourceAddress,
    },
    #[error("Resource did not contain sufficient balance: {details}")]
    InsufficientBalance { details: String },
    #[error("Invariant error: {0}")]
    InvariantError(String),
    #[error("Operation not allowed: {0}")]
    OperationNotAllowed(String),
    #[error("Non fungible token not found: {token}")]
    NonFungibleTokenIdNotFound { token: NonFungibleId },
    #[error("Invalid balance proof: {details}")]
    InvalidBalanceProof { details: String },
    #[error("Invalid confidential proof: {details}")]
    InvalidConfidentialProof { details: String },
    #[error("Invalid confidential mint, no change should be specified")]
    InvalidConfidentialMintWithChange,
}
