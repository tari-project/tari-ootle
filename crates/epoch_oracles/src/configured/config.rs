//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    fmt::Display,
    hash::{DefaultHasher, Hasher},
    num::NonZeroU64,
    time::Duration,
};

use anyhow::bail;
use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup, SubstateAddress, VotePower, displayable::Displayable};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

/// The most vote power the configured validators may hold between them. Leader selection simulates up to
/// `total_power × committee size` steps per call, so every committee's total power must stay a small integer.
pub const MAX_TOTAL_VOTE_POWER: u64 = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(with = "ootle_serde::duration::optional_seconds")]
    pub epoch_time: Option<Duration>,
    pub initial_epoch: Epoch,
    #[serde(with = "time::serde::iso8601")]
    pub base_time: time::OffsetDateTime,
    #[serde(default)]
    pub validators: Vec<Validator>,
}

impl Config {
    /// Checks the invariants the oracle relies on to emit a deterministic event stream. Every node must derive the
    /// same event stream from the same config, so a config that could be read two ways is rejected outright rather
    /// than resolved by a default.
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut seen = HashSet::with_capacity(self.validators.len());
        // Every committee's total power is a sum over a subset of these validators, so bounding the network-wide sum
        // bounds every committee total.
        let mut total_power = 0u64;
        for validator in &self.validators {
            total_power = total_power
                .checked_add(validator.power.get())
                .filter(|total| *total <= MAX_TOTAL_VOTE_POWER)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "The configured validators' vote power sums to more than {MAX_TOTAL_VOTE_POWER}. Lower the \
                         `power` of one or more validators."
                    )
                })?;
            // A second entry for a validator would activate it twice; listing it at two registration epochs also
            // moves its shard key.
            if !seen.insert(validator.public_key) {
                bail!(
                    "Validator {} is listed more than once. A validator has a single registration; rotate its claim \
                     key with `claim_key_changes`.",
                    validator.public_key
                );
            }
            validator.validate()?;
        }
        Ok(())
    }

    /// Checks that `self` only schedules changes after `processed_epoch` on top of `prior`: new validators, claim key
    /// changes and exits, each taking effect after `processed_epoch`. Everything `prior` holds is kept unchanged, so
    /// a node that has processed `prior` up to `processed_epoch` reaches the same validator set as one that replays
    /// `self` from a fresh store.
    pub fn check_extends(&self, prior: &Config, processed_epoch: Epoch) -> anyhow::Result<()> {
        self.validate()?;
        if self.epoch_time != prior.epoch_time ||
            self.initial_epoch != prior.initial_epoch ||
            self.base_time != prior.base_time
        {
            bail!("The config changes the epoch schedule (epoch_time, initial_epoch or base_time)");
        }

        for prior_vn in &prior.validators {
            let Some(vn) = self.validators.iter().find(|vn| vn.public_key == prior_vn.public_key) else {
                bail!(
                    "The config drops validator {}; remove a validator by setting its `exit_epoch`",
                    prior_vn.public_key
                );
            };
            if vn.claim_key != prior_vn.claim_key ||
                vn.shard_group != prior_vn.shard_group ||
                vn.registration_epoch != prior_vn.registration_epoch ||
                vn.power != prior_vn.power
            {
                bail!(
                    "The config changes the registration of validator {}, which is fixed once listed",
                    vn.public_key
                );
            }
            if let Some(change) = prior_vn
                .claim_key_changes
                .iter()
                .find(|change| !vn.claim_key_changes.contains(change))
            {
                bail!(
                    "The config drops or edits validator {}'s claim key change at epoch {}",
                    vn.public_key,
                    change.effective_epoch
                );
            }
            if let Some(change) = vn.claim_key_changes.iter().find(|change| {
                !prior_vn.claim_key_changes.contains(change) && change.effective_epoch <= processed_epoch
            }) {
                bail!(
                    "The config adds a claim key change for validator {} at epoch {}, which this node has already \
                     processed",
                    vn.public_key,
                    change.effective_epoch
                );
            }
            match (prior_vn.exit_epoch, vn.exit_epoch) {
                (Some(prior_exit), exit) if exit != Some(prior_exit) => {
                    bail!(
                        "The config changes validator {}'s exit epoch {prior_exit}",
                        vn.public_key
                    );
                },
                (None, Some(exit)) if exit <= processed_epoch => {
                    bail!(
                        "The config sets validator {}'s exit epoch to {exit}, which this node has already processed",
                        vn.public_key
                    );
                },
                _ => {},
            }
        }

        for vn in &self.validators {
            let is_new = !prior
                .validators
                .iter()
                .any(|prior_vn| prior_vn.public_key == vn.public_key);
            if is_new && vn.activation_epoch() <= processed_epoch {
                bail!(
                    "The config adds validator {} activating at epoch {}, which this node has already processed",
                    vn.public_key,
                    vn.activation_epoch()
                );
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            epoch_time: None,
            initial_epoch: Epoch(0),
            base_time: time::OffsetDateTime::now_utc(),
            validators: vec![],
        }
    }
}

impl Display for Config {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            fmt,
            "Epoch time: {}, initial_epoch: {}, btime: {}, validators: {}",
            self.epoch_time.as_ref().map(|d| d.as_secs()).display(),
            self.initial_epoch,
            self.base_time,
            self.validators.len()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Validator {
    pub public_key: RistrettoPublicKeyBytes,
    /// The claim key this validator activates with.
    ///
    /// This is an input to shard key derivation and so is fixed once the validator has activated: a node replaying
    /// an edited config from a fresh oracle store would derive a different shard key to its peers and disagree with
    /// them on committee ordering. Use `claim_key_changes` to rotate the claim key.
    pub claim_key: RistrettoPublicKeyBytes,
    pub shard_group: ShardGroup,
    pub registration_epoch: Epoch,
    /// Claim key rotations for this validator, each taking effect at the start of its `effective_epoch`. The shard
    /// key is unaffected by a rotation.
    #[serde(default)]
    pub claim_key_changes: Vec<ClaimKeyChange>,
    /// The validator's vote power: a small integer count of stake units. The power of all configured validators
    /// together is capped at [`MAX_TOTAL_VOTE_POWER`].
    ///
    /// Power is fixed for the life of the registration: a claim key rotation re-registers the validator with this
    /// same power. Shard key derivation excludes it, so a validator's place in its committee is independent of stake.
    #[serde(default = "Validator::default_power")]
    pub power: NonZeroU64,
    /// The first epoch in which this validator is no longer active. It stays in its committee up to and including
    /// `exit_epoch - 1`. An exit is permanent: the validator cannot be listed again under the same public key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_epoch: Option<Epoch>,
}

/// A scheduled claim key rotation for a [`Validator`].
///
/// Leader fees earned from `effective_epoch` onwards accrue to the fee pool derived from `claim_key`. Pools filled
/// under a previous claim key are not migrated and remain claimable with that key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClaimKeyChange {
    pub effective_epoch: Epoch,
    pub claim_key: RistrettoPublicKeyBytes,
}

impl Validator {
    /// The epoch at which this validator's initial registration becomes active.
    pub fn activation_epoch(&self) -> Epoch {
        self.registration_epoch.saturating_add(Epoch(1))
    }

    fn default_power() -> NonZeroU64 {
        NonZeroU64::MIN
    }

    pub fn vote_power(&self) -> VotePower {
        VotePower::of(self.power.get())
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.shard_group.start().is_global() {
            bail!(
                "Validator {}: shard_group start ({}) cannot be the global shard (0)",
                self.public_key,
                self.shard_group.start().as_u32()
            );
        }
        if self.registration_epoch == Epoch(u64::MAX) {
            bail!(
                "Validator {}: registration_epoch ({}) cannot activate in a subsequent epoch",
                self.public_key,
                self.registration_epoch
            );
        }

        let mut changes = self.claim_key_changes.iter().collect::<Vec<_>>();
        changes.sort_by_key(|change| change.effective_epoch);

        let mut previous_epoch = None;
        let mut previous_key = &self.claim_key;
        for change in changes {
            if change.effective_epoch <= self.activation_epoch() {
                bail!(
                    "Validator {}: claim key change at epoch {} must be after the validator's activation epoch {}",
                    self.public_key,
                    change.effective_epoch,
                    self.activation_epoch()
                );
            }
            // Two rows sharing a start_epoch are resolved by row order in one query and by max(id) in another, so
            // the subsystems that read them could disagree. Reject rather than pick a winner.
            if previous_epoch == Some(change.effective_epoch) {
                bail!(
                    "Validator {}: multiple claim key changes at epoch {}",
                    self.public_key,
                    change.effective_epoch
                );
            }
            if change.claim_key == *previous_key {
                bail!(
                    "Validator {}: claim key change at epoch {} does not change the claim key",
                    self.public_key,
                    change.effective_epoch
                );
            }
            // A rotation re-registers the validator, so one at or after the exit would bring it back.
            if let Some(exit_epoch) = self.exit_epoch &&
                change.effective_epoch >= exit_epoch
            {
                bail!(
                    "Validator {}: claim key change at epoch {} must be before the validator's exit epoch {}",
                    self.public_key,
                    change.effective_epoch,
                    exit_epoch
                );
            }
            previous_epoch = Some(change.effective_epoch);
            previous_key = &change.claim_key;
        }

        if let Some(exit_epoch) = self.exit_epoch &&
            exit_epoch <= self.activation_epoch()
        {
            bail!(
                "Validator {}: exit epoch {} must be after the validator's activation epoch {}",
                self.public_key,
                exit_epoch,
                self.activation_epoch()
            );
        }

        Ok(())
    }

    /// Generates a deterministic shard key that naturally falls within the ShardGroup for this Validator node.
    pub(crate) fn calculate_shard_key(&self) -> SubstateAddress {
        let range = self.shard_group.to_substate_address_range(NumPreshards::current());
        let mut hasher = DefaultHasher::new();
        hasher.write(&self.shard_group.encode_as_u32().to_be_bytes());
        hasher.write(self.public_key.as_bytes());
        hasher.write(self.claim_key.as_bytes());
        hasher.write(&self.registration_epoch.to_be_bytes());
        let hash = hasher.finish();

        let start = range.start();
        let len = start.object_key_bytes().len();
        let hash_size = size_of_val(&hash);
        let mut start = start.into_array();
        start
            .get_mut(len - hash_size..len)
            .expect("bounds checked")
            .copy_from_slice(&hash.to_be_bytes());
        SubstateAddress::from_array(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_key(seed: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[seed; 32]).unwrap()
    }

    fn mk_validator(claim_key_changes: Vec<ClaimKeyChange>) -> Validator {
        Validator {
            public_key: mk_key(1),
            claim_key: mk_key(2),
            shard_group: ShardGroup::new(1, 256),
            registration_epoch: Epoch(10),
            claim_key_changes,
            power: NonZeroU64::MIN,
            exit_epoch: None,
        }
    }

    fn validate(claim_key_changes: Vec<ClaimKeyChange>) -> anyhow::Result<()> {
        mk_validator(claim_key_changes).validate()
    }

    #[test]
    fn no_claim_key_changes_is_valid() {
        validate(vec![]).unwrap();
    }

    #[test]
    fn rotation_after_activation_is_valid() {
        validate(vec![
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(3),
            },
            ClaimKeyChange {
                effective_epoch: Epoch(30),
                claim_key: mk_key(4),
            },
        ])
        .unwrap();
    }

    #[test]
    fn rotations_need_not_be_listed_in_epoch_order() {
        validate(vec![
            ClaimKeyChange {
                effective_epoch: Epoch(30),
                claim_key: mk_key(4),
            },
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(3),
            },
        ])
        .unwrap();
    }

    #[test]
    fn rotation_at_or_before_activation_is_rejected() {
        // The validator activates at 11, so there is no registration for these to supersede.
        for epoch in [Epoch(5), Epoch(10), Epoch(11)] {
            let err = validate(vec![ClaimKeyChange {
                effective_epoch: epoch,
                claim_key: mk_key(3),
            }])
            .unwrap_err()
            .to_string();
            assert!(err.contains("must be after the validator's activation epoch"), "{err}");
        }
    }

    #[test]
    fn duplicate_effective_epoch_is_rejected() {
        let err = validate(vec![
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(3),
            },
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(4),
            },
        ])
        .unwrap_err()
        .to_string();
        assert!(err.contains("multiple claim key changes at epoch"), "{err}");
    }

    #[test]
    fn rotation_to_the_same_key_is_rejected() {
        let err = validate(vec![ClaimKeyChange {
            effective_epoch: Epoch(20),
            claim_key: mk_key(2),
        }])
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not change the claim key"), "{err}");

        let err = validate(vec![
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(3),
            },
            ClaimKeyChange {
                effective_epoch: Epoch(30),
                claim_key: mk_key(3),
            },
        ])
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not change the claim key"), "{err}");
    }

    #[test]
    fn rotating_back_to_a_previously_used_key_is_valid() {
        validate(vec![
            ClaimKeyChange {
                effective_epoch: Epoch(20),
                claim_key: mk_key(3),
            },
            ClaimKeyChange {
                effective_epoch: Epoch(30),
                claim_key: mk_key(2),
            },
        ])
        .unwrap();
    }

    fn mk_config(validators: Vec<Validator>) -> Config {
        Config {
            validators,
            base_time: time::OffsetDateTime::UNIX_EPOCH,
            ..Config::default()
        }
    }

    #[test]
    fn a_validator_listed_twice_is_rejected() {
        // Identical entries derive an identical shard key, so the shard key pin cannot catch this: it would emit two
        // Adds at one epoch and land two rows sharing a start_epoch.
        let err = mk_config(vec![mk_validator(vec![]), mk_validator(vec![])])
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("is listed more than once"), "{err}");

        // Differing entries for one validator would additionally move its shard key.
        let second = Validator {
            registration_epoch: Epoch(50),
            claim_key: mk_key(3),
            ..mk_validator(vec![])
        };
        let err = mk_config(vec![mk_validator(vec![]), second])
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("is listed more than once"), "{err}");
    }

    #[test]
    fn distinct_validators_are_valid() {
        let second = Validator {
            public_key: mk_key(9),
            ..mk_validator(vec![])
        };
        mk_config(vec![mk_validator(vec![]), second]).validate().unwrap();
    }

    #[test]
    fn claim_key_rotation_does_not_move_the_shard_key() {
        let without = mk_validator(vec![]);
        let with = mk_validator(vec![ClaimKeyChange {
            effective_epoch: Epoch(20),
            claim_key: mk_key(3),
        }]);
        assert_eq!(without.calculate_shard_key(), with.calculate_shard_key());
    }

    #[test]
    fn power_defaults_to_one_when_omitted() {
        let validator: Validator = serde_json::from_value(serde_json::json!({
            "public_key": mk_key(1),
            "claim_key": mk_key(2),
            "shard_group": ShardGroup::new(1, 256),
            "registration_epoch": Epoch(10),
        }))
        .unwrap();
        assert_eq!(validator.vote_power(), VotePower::of(1));
        validator.validate().unwrap();
    }

    #[test]
    fn zero_power_is_rejected() {
        let err = serde_json::from_value::<Validator>(serde_json::json!({
            "public_key": mk_key(1),
            "claim_key": mk_key(2),
            "shard_group": ShardGroup::new(1, 256),
            "registration_epoch": Epoch(10),
            "power": 0,
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("nonzero"), "{err}");
    }

    #[test]
    fn total_power_beyond_the_cap_is_rejected() {
        let with_power = |seed, power| Validator {
            public_key: mk_key(seed),
            power: NonZeroU64::new(power).unwrap(),
            ..mk_validator(vec![])
        };
        let at_limit = mk_config(vec![with_power(1, MAX_TOTAL_VOTE_POWER - 1), with_power(9, 1)]);
        at_limit.validate().unwrap();

        for validators in [
            vec![with_power(1, MAX_TOTAL_VOTE_POWER + 1)],
            vec![with_power(1, MAX_TOTAL_VOTE_POWER), with_power(9, 1)],
            vec![with_power(1, u64::MAX), with_power(9, u64::MAX)],
        ] {
            let err = mk_config(validators).validate().unwrap_err().to_string();
            assert!(err.contains("vote power sums to more than"), "{err}");
        }
    }

    #[test]
    fn power_does_not_move_the_shard_key() {
        let heavy = Validator {
            power: NonZeroU64::new(8).unwrap(),
            ..mk_validator(vec![])
        };
        assert_eq!(mk_validator(vec![]).calculate_shard_key(), heavy.calculate_shard_key());
    }

    #[test]
    fn exit_after_activation_is_valid() {
        let validator = Validator {
            exit_epoch: Some(Epoch(20)),
            ..mk_validator(vec![ClaimKeyChange {
                effective_epoch: Epoch(19),
                claim_key: mk_key(3),
            }])
        };
        validator.validate().unwrap();
    }

    #[test]
    fn exit_at_or_before_activation_is_rejected() {
        // The validator activates at 11, so it would never be active.
        for epoch in [Epoch(5), Epoch(10), Epoch(11)] {
            let err = Validator {
                exit_epoch: Some(epoch),
                ..mk_validator(vec![])
            }
            .validate()
            .unwrap_err()
            .to_string();
            assert!(err.contains("must be after the validator's activation epoch"), "{err}");
        }
    }

    #[test]
    fn rotation_at_or_after_exit_is_rejected() {
        for epoch in [Epoch(20), Epoch(25)] {
            let err = Validator {
                exit_epoch: Some(Epoch(20)),
                ..mk_validator(vec![ClaimKeyChange {
                    effective_epoch: epoch,
                    claim_key: mk_key(3),
                }])
            }
            .validate()
            .unwrap_err()
            .to_string();
            assert!(err.contains("must be before the validator's exit epoch"), "{err}");
        }
    }

    #[test]
    fn exit_epoch_does_not_move_the_shard_key() {
        let exiting = Validator {
            exit_epoch: Some(Epoch(20)),
            ..mk_validator(vec![])
        };
        assert_eq!(
            mk_validator(vec![]).calculate_shard_key(),
            exiting.calculate_shard_key()
        );
    }

    #[test]
    fn exit_epoch_defaults_to_none_when_omitted() {
        let validator: Validator = serde_json::from_value(serde_json::json!({
            "public_key": mk_key(1),
            "claim_key": mk_key(2),
            "shard_group": ShardGroup::new(1, 256),
            "registration_epoch": Epoch(10),
        }))
        .unwrap();
        assert_eq!(validator.exit_epoch, None);
    }

    fn other_validator(registration_epoch: Epoch) -> Validator {
        Validator {
            public_key: mk_key(9),
            registration_epoch,
            ..mk_validator(vec![])
        }
    }

    fn extends(prior: &Config, updated: &Config) -> Result<(), String> {
        updated.check_extends(prior, Epoch(15)).map_err(|err| err.to_string())
    }

    #[test]
    fn an_update_scheduling_changes_after_the_processed_epoch_extends_the_config() {
        let prior = mk_config(vec![mk_validator(vec![])]);
        let updated = mk_config(vec![
            Validator {
                exit_epoch: Some(Epoch(30)),
                ..mk_validator(vec![ClaimKeyChange {
                    effective_epoch: Epoch(20),
                    claim_key: mk_key(3),
                }])
            },
            other_validator(Epoch(15)),
        ]);
        extends(&prior, &updated).unwrap();
        extends(&prior, &prior).unwrap();
    }

    #[test]
    fn an_update_changing_what_was_already_processed_is_rejected() {
        let prior = mk_config(vec![mk_validator(vec![])]);
        let cases = [
            (
                mk_config(vec![Validator {
                    exit_epoch: Some(Epoch(15)),
                    ..mk_validator(vec![])
                }]),
                "already processed",
            ),
            (
                mk_config(vec![mk_validator(vec![ClaimKeyChange {
                    effective_epoch: Epoch(15),
                    claim_key: mk_key(3),
                }])]),
                "already processed",
            ),
            (
                mk_config(vec![mk_validator(vec![]), other_validator(Epoch(14))]),
                "already processed",
            ),
            (mk_config(vec![other_validator(Epoch(20))]), "drops validator"),
            (
                mk_config(vec![Validator {
                    claim_key: mk_key(3),
                    ..mk_validator(vec![])
                }]),
                "changes the registration",
            ),
            (
                Config {
                    initial_epoch: Epoch(1),
                    ..prior.clone()
                },
                "epoch schedule",
            ),
        ];
        for (updated, expected) in cases {
            let err = extends(&prior, &updated).unwrap_err();
            assert!(err.contains(expected), "expected {expected:?}: {err}");
        }
    }

    #[test]
    fn an_update_editing_a_scheduled_exit_or_rotation_is_rejected() {
        let rotation = ClaimKeyChange {
            effective_epoch: Epoch(20),
            claim_key: mk_key(3),
        };
        let prior = mk_config(vec![Validator {
            exit_epoch: Some(Epoch(30)),
            ..mk_validator(vec![rotation.clone()])
        }]);

        let moved_exit = mk_config(vec![Validator {
            exit_epoch: Some(Epoch(40)),
            ..mk_validator(vec![rotation])
        }]);
        assert!(extends(&prior, &moved_exit).unwrap_err().contains("exit epoch"));

        let dropped_rotation = mk_config(vec![Validator {
            exit_epoch: Some(Epoch(30)),
            ..mk_validator(vec![])
        }]);
        assert!(
            extends(&prior, &dropped_rotation)
                .unwrap_err()
                .contains("drops or edits")
        );
    }

    #[test]
    fn a_config_round_trips_through_json() {
        let config = Config {
            epoch_time: Some(Duration::from_secs(1200)),
            ..mk_config(vec![Validator {
                exit_epoch: Some(Epoch(30)),
                ..mk_validator(vec![ClaimKeyChange {
                    effective_epoch: Epoch(20),
                    claim_key: mk_key(3),
                }])
            }])
        };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), config);
    }

    #[test]
    fn invalid_shard_group_or_max_registration_epoch_is_rejected() {
        let global_start = Validator {
            shard_group: ShardGroup::new(0, 64),
            ..mk_validator(vec![])
        };
        assert!(
            global_start
                .validate()
                .unwrap_err()
                .to_string()
                .contains("cannot be the global shard")
        );

        let max_epoch = Validator {
            registration_epoch: Epoch(u64::MAX),
            ..mk_validator(vec![])
        };
        assert!(
            max_epoch
                .validate()
                .unwrap_err()
                .to_string()
                .contains("cannot activate in a subsequent epoch")
        );
    }
}
