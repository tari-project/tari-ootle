//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, mem};

use ootle_byte_type::ToByteType;
use serde::{Deserialize, Serialize};
use tari_crypto::{
    ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_template_abi::rust::collections::BTreeSet;
use tari_template_lib::{
    prelude::PUBLIC_IDENTITY_RESOURCE_ADDRESS,
    types::{
        Amount,
        NonFungibleAddress,
        NonFungibleId,
        ResourceAddress,
        ResourceType,
        UtxoId,
        confidential::{ConfidentialOutputStatement, ConfidentialWithdrawProof},
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    },
};

use crate::{confidential, crypto::OutputBody, substate::SubstateId};

/// Instances of a single resource kept in Buckets and Vaults
#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
        commitments: BTreeMap<PedersenCommitmentBytes, OutputBody>,
        revealed_amount: Amount,
        locked_commitments: BTreeMap<PedersenCommitmentBytes, OutputBody>,
        locked_revealed_amount: Amount,
    },
    Stealth {
        address: ResourceAddress,
        revealed_amount: Amount,
        locked_amount: Amount,
    },
}

impl ResourceContainer {
    pub fn public_fungible<T: Into<Amount>>(address: ResourceAddress, amount: T) -> Self {
        let amount = amount.into();
        assert!(amount.is_non_negative(), "amount must be non-negative");
        Self::Fungible {
            address,
            amount,
            locked_amount: Amount::zero(),
        }
    }

    pub fn non_fungible(address: ResourceAddress, token_ids: BTreeSet<NonFungibleId>) -> Self {
        Self::NonFungible {
            address,
            token_ids,
            locked_token_ids: BTreeSet::new(),
        }
    }

    pub fn confidential<I: IntoIterator<Item = (PedersenCommitmentBytes, OutputBody)>>(
        address: ResourceAddress,
        commitment: I,
        revealed_amount: Amount,
    ) -> Self {
        assert!(
            revealed_amount.is_non_negative(),
            "revealed amount must be non-negative"
        );
        Self::Confidential {
            address,
            commitments: commitment.into_iter().collect(),
            revealed_amount,
            locked_commitments: BTreeMap::new(),
            locked_revealed_amount: Amount::zero(),
        }
    }

    pub fn stealth(address: ResourceAddress, revealed_amount: Amount) -> Self {
        assert!(
            revealed_amount.is_non_negative(),
            "revealed amount must be non-negative"
        );
        Self::Stealth {
            address,
            revealed_amount,
            locked_amount: Amount::zero(),
        }
    }

    pub fn public_key(public_key: RistrettoPublicKeyBytes) -> Self {
        Self::NonFungible {
            address: PUBLIC_IDENTITY_RESOURCE_ADDRESS,
            token_ids: {
                let mut set = BTreeSet::new();
                set.insert(NonFungibleId::from_public_key(public_key));
                set
            },
            locked_token_ids: Default::default(),
        }
    }

    pub fn mint_confidential(
        address: ResourceAddress,
        proof: ConfidentialOutputStatement,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<Self, ResourceError> {
        if proof.change_statement.is_some() {
            return Err(ResourceError::InvalidConfidentialMintWithChange);
        }
        if !proof.change_revealed_amount.is_zero() {
            return Err(ResourceError::InvalidConfidentialProof {
                details: "Change revealed amount must be zero for minting".to_string(),
            });
        }
        let validated_proof = confidential::validate_confidential_statement(&proof, view_key)?;
        assert!(
            validated_proof.change_output.is_none(),
            "invariant failed: validate_confidential_proof returned change with no change in input proof"
        );
        Ok(Self::Confidential {
            address,
            commitments: validated_proof
                .output
                .into_iter()
                .map(|o| (o.commitment.to_byte_type(), o.into_output_body()))
                .collect(),
            revealed_amount: validated_proof.output_revealed_amount,
            locked_commitments: BTreeMap::new(),
            locked_revealed_amount: Amount::zero(),
        })
    }

    pub fn unlocked_amount(&self) -> Amount {
        match self {
            Self::Fungible { amount, .. } => *amount,
            Self::NonFungible { token_ids, .. } => Amount::new(token_ids.len() as u128),
            Self::Confidential { revealed_amount, .. } => *revealed_amount,
            Self::Stealth { revealed_amount, .. } => *revealed_amount,
        }
    }

    pub fn number_of_confidential_commitments(&self) -> usize {
        match self {
            Self::Confidential {
                commitments,
                locked_commitments,
                ..
            } => commitments.len() + locked_commitments.len(),
            _ => 0,
        }
    }

    pub fn locked_amount(&self) -> Amount {
        match self {
            Self::Fungible { locked_amount, .. } => *locked_amount,
            Self::NonFungible { locked_token_ids, .. } => Amount::new(locked_token_ids.len() as u128),
            Self::Confidential {
                locked_revealed_amount, ..
            } => *locked_revealed_amount,
            Self::Stealth { locked_amount, .. } => *locked_amount,
        }
    }

    pub fn get_commitment_count(&self) -> u64 {
        match self {
            Self::Fungible { .. } => 0,
            Self::NonFungible { .. } => 0,
            Self::Confidential { commitments, .. } => commitments.len() as u64,
            // Unknown
            Self::Stealth { .. } => 0,
        }
    }

    pub fn resource_address(&self) -> &ResourceAddress {
        match self {
            Self::Fungible { address, .. } => address,
            Self::NonFungible { address, .. } => address,
            Self::Confidential { address, .. } => address,
            Self::Stealth { address, .. } => address,
        }
    }

    pub fn resource_type(&self) -> ResourceType {
        match self {
            Self::Fungible { .. } => ResourceType::Fungible,
            Self::NonFungible { .. } => ResourceType::NonFungible,
            Self::Confidential { .. } => ResourceType::Confidential,
            Self::Stealth { .. } => ResourceType::Stealth,
        }
    }

    pub fn non_fungible_token_ids(&self) -> &BTreeSet<NonFungibleId> {
        static EMPTY_BTREE_SET: BTreeSet<NonFungibleId> = BTreeSet::new();
        match self {
            Self::NonFungible { token_ids, .. } => token_ids,
            _ => &EMPTY_BTREE_SET,
        }
    }

    pub fn into_non_fungible_ids(self) -> Option<BTreeSet<NonFungibleId>> {
        match self {
            Self::NonFungible { token_ids, .. } => Some(token_ids),
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
            (
                Self::Stealth { revealed_amount, .. },
                Self::Stealth {
                    revealed_amount: other_amount,
                    ..
                },
            ) => {
                *revealed_amount += other_amount;
            },
            (this, other) => {
                return Err(ResourceError::ResourceTypeMismatch {
                    operate: "deposit",
                    given: other.resource_type(),
                    expected: this.resource_type(),
                });
            },
        }
        Ok(())
    }

    pub fn withdraw(&mut self, withdraw_amt: Amount) -> Result<Self, ResourceError> {
        if !withdraw_amt.is_non_negative() {
            return Err(ResourceError::InvariantError(format!(
                "Amount must be non-negative (>= 0). Got :{withdraw_amt}"
            )));
        }
        match self {
            Self::Fungible { amount, .. } => {
                if withdraw_amt > *amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket contained insufficient funds. Required: {}, Available: {}",
                            withdraw_amt, amount
                        ),
                    });
                }
                *amount -= withdraw_amt;
                Ok(Self::public_fungible(*self.resource_address(), withdraw_amt))
            },
            Self::NonFungible { token_ids, .. } => {
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

                Ok(Self::non_fungible(*self.resource_address(), taken_tokens))
            },
            Self::Confidential { revealed_amount, .. } => {
                if withdraw_amt > *revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket or vault contained insufficient revealed funds. Required: {}, Available: {}",
                            withdraw_amt, revealed_amount
                        ),
                    });
                }
                *revealed_amount -= withdraw_amt;
                Ok(Self::confidential(*self.resource_address(), None, withdraw_amt))
            },
            Self::Stealth { revealed_amount, .. } => {
                if withdraw_amt > *revealed_amount {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "Bucket or vault contained insufficient revealed funds. Required: {}, Available: {}",
                            withdraw_amt, revealed_amount
                        ),
                    });
                }
                *revealed_amount -= withdraw_amt;
                Ok(Self::stealth(*self.resource_address(), withdraw_amt))
            },
        }
    }

    pub fn withdraw_all(&mut self) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible { .. } | Self::NonFungible { .. } | Self::Stealth { .. } => {
                self.withdraw(self.unlocked_amount())
            },
            Self::Confidential {
                commitments,
                revealed_amount,
                ..
            } => {
                let amount = *revealed_amount;
                *revealed_amount = Amount::zero();
                let commitments = mem::take(commitments);
                Ok(Self::confidential(*self.resource_address(), commitments, amount))
            },
        }
    }

    pub fn withdraw_by_ids(&mut self, ids: &BTreeSet<NonFungibleId>) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw by NFT token id from a fungible resource".to_string(),
            )),
            Self::NonFungible { token_ids, .. } => {
                let taken_tokens = ids
                    .iter()
                    .map(|id| {
                        token_ids
                            .take(id)
                            .ok_or_else(|| ResourceError::NonFungibleTokenIdNotFound { token: id.clone() })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Self::non_fungible(*self.resource_address(), taken_tokens))
            },
            Self::Confidential { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw by NFT token id from a confidential resource".to_string(),
            )),
            Self::Stealth { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw by NFT token id from a stealth resource".to_string(),
            )),
        }
    }

    pub fn withdraw_confidential(
        &mut self,
        proof: ConfidentialWithdrawProof,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a fungible resource (use withdraw)".to_string(),
            )),
            Self::NonFungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a non-fungible resource (use withdraw_non_fungible)"
                    .to_string(),
            )),
            Self::Stealth { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot withdraw confidential assets from a stealth resource (use withdraw)".to_string(),
            )),
            Self::Confidential {
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

                let validated_proof = confidential::validate_confidential_withdraw(&inputs, view_key, proof)?;

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
                        .insert(change.commitment.to_byte_type(), change.into_output_body())
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

                Ok(Self::confidential(
                    *self.resource_address(),
                    validated_proof
                        .output
                        .map(|o| (o.commitment.to_byte_type(), o.into_output_body())),
                    validated_proof.output_revealed_amount,
                ))
            },
        }
    }

    pub fn recall_confidential_commitments(
        &mut self,
        commitments: &BTreeSet<PedersenCommitmentBytes>,
        revealed_amount: Amount,
    ) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot recall confidential assets from a fungible resource".to_string(),
            )),
            Self::NonFungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot recall confidential assets from a non-fungible resource".to_string(),
            )),
            Self::Stealth { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot recall confidential assets from a stealth resource".to_string(),
            )),
            Self::Confidential {
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

                Ok(Self::confidential(*self.resource_address(), recalled, revealed_amount))
            },
        }
    }

    /// Returns all confidential commitments. If the resource is not confidential, None is returned.
    pub fn get_confidential_commitments(&self) -> Option<&BTreeMap<PedersenCommitmentBytes, OutputBody>> {
        match self {
            Self::Fungible { .. } | Self::NonFungible { .. } | Self::Stealth { .. } => None,
            Self::Confidential { commitments, .. } => Some(commitments),
        }
    }

    pub fn into_confidential_commitments(self) -> Option<BTreeMap<PedersenCommitmentBytes, OutputBody>> {
        match self {
            Self::Fungible { .. } | Self::NonFungible { .. } | Self::Stealth { .. } => None,
            Self::Confidential { commitments, .. } => Some(commitments),
        }
    }

    pub fn lock_all(&mut self) -> Result<Self, ResourceError> {
        let resource_address = *self.resource_address();
        match self {
            Self::Fungible {
                amount, locked_amount, ..
            } => {
                if amount.is_zero() {
                    return Err(ResourceError::InsufficientBalance {
                        details: "lock_all: resource container contained no funds".to_string(),
                    });
                }
                // Sets to zero and returns the amount
                let newly_locked_amount = mem::take(amount);
                *locked_amount += newly_locked_amount;
                Ok(Self::public_fungible(resource_address, newly_locked_amount))
            },
            Self::NonFungible {
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

                Ok(Self::non_fungible(resource_address, newly_locked_token_ids))
            },
            Self::Confidential {
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

                Ok(Self::confidential(
                    resource_address,
                    newly_locked_commitments,
                    newly_locked_revealed_amount,
                ))
            },
            Self::Stealth {
                revealed_amount,
                locked_amount,
                ..
            } => {
                if revealed_amount.is_zero() {
                    return Err(ResourceError::InsufficientBalance {
                        details: "lock_all: resource container contained no funds".to_string(),
                    });
                }

                // Sets to zero and returns the amount
                let newly_locked_amount = mem::take(revealed_amount);
                *locked_amount += newly_locked_amount;
                Ok(Self::public_fungible(resource_address, newly_locked_amount))
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn unlock(&mut self, container: Self) -> Result<(), ResourceError> {
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
            Self::Fungible {
                amount, locked_amount, ..
            } => {
                if *locked_amount < container.unlocked_amount() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "unlock: resource container did not contain enough locked funds. Required: {}, Available: \
                             {}",
                            container.unlocked_amount(),
                            locked_amount
                        ),
                    });
                }
                *amount += container.unlocked_amount();
                *locked_amount -= container.unlocked_amount();
            },
            Self::NonFungible {
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
            Self::Confidential {
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

                if *locked_revealed_amount < container.unlocked_amount() {
                    return Err(ResourceError::InvariantError(format!(
                        "unlock: resource container did not contain enough locked revealed amount. Required: {}, \
                         Available: {}",
                        container.unlocked_amount(),
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
                *revealed_amount += container.unlocked_amount();
                *locked_revealed_amount -= container.unlocked_amount();
            },
            Self::Stealth {
                revealed_amount,
                locked_amount,
                ..
            } => {
                if *locked_amount < container.unlocked_amount() {
                    return Err(ResourceError::InsufficientBalance {
                        details: format!(
                            "unlock: resource container did not contain enough locked funds. Required: {}, Available: \
                             {}",
                            container.unlocked_amount(),
                            locked_amount
                        ),
                    });
                }
                *revealed_amount += container.unlocked_amount();
                *locked_amount -= container.unlocked_amount();
            },
        }

        Ok(())
    }

    pub fn lock_by_non_fungible_ids(&mut self, ids: BTreeSet<NonFungibleId>) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot lock by NFT token id from a fungible resource".to_string(),
            )),
            Self::NonFungible {
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
                Ok(Self::non_fungible(*self.resource_address(), newly_locked))
            },
            Self::Confidential { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot lock by NFT token id from a confidential resource".to_string(),
            )),
            Self::Stealth { .. } => Err(ResourceError::OperationNotAllowed(
                "Cannot lock by NFT token id from a stealth resource".to_string(),
            )),
        }
    }

    pub fn lock_by_amount(&mut self, amount: Amount) -> Result<Self, ResourceError> {
        match self {
            Self::Fungible {
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
                Ok(Self::public_fungible(*self.resource_address(), amount))
            },
            Self::NonFungible {
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

                Ok(Self::non_fungible(*self.resource_address(), newly_locked_token_ids))
            },
            Self::Confidential {
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
                Ok(Self::confidential(*self.resource_address(), None, amount))
            },
            Self::Stealth {
                revealed_amount: available_amount,
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
                Ok(Self::stealth(*self.resource_address(), amount))
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.locked_amount().is_zero() &&
            self.unlocked_amount().is_zero() &&
            self.number_of_confidential_commitments() == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("Attempted to {operate} a {given} resource, but the container resource type is {expected}")]
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
    #[error("Invalid range proof: {details}")]
    InvalidRangeProof { details: String },
    #[error("Invalid confidential mint, no change should be specified")]
    InvalidConfidentialMintWithChange,
    #[error("Invalid spend: {details}")]
    InvalidSpend { details: String },
    #[error(
        "The transaction signature with public key {public_key} required to spend the input with commitment \
         {commitment} was not provided or is not in scope"
    )]
    RequiredSignatureMissingForStealthUtxo {
        commitment: PedersenCommitmentBytes,
        public_key: RistrettoPublicKeyBytes,
    },
    #[error("UTXO {id} failed to burn: {details}")]
    UtxoBurnFailed { id: UtxoId, details: String },
}
