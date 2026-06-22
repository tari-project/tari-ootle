//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::MaskAndValue;
use tari_template_lib::types::{bytes::Bytes, crypto::RistrettoPublicKeyBytes, stealth::SpendCondition};

/// How a generated stealth output is gated for spending (TIP-0006).
#[derive(Debug, Clone)]
pub enum OutputAuthSpec {
    /// Key path with `spend_key` set to the output mask's public key (the default for tests).
    KeyPathFromMask,
    /// Key path with an explicit `spend_key`.
    KeyPath(RistrettoPublicKeyBytes),
    /// A condition tree (MAST) over the given leaves; the output commits its `condition_root`.
    Conditions(Vec<SpendCondition>),
}

impl From<RistrettoPublicKeyBytes> for OutputAuthSpec {
    fn from(pk: RistrettoPublicKeyBytes) -> Self {
        Self::KeyPath(pk)
    }
}

impl From<SpendCondition> for OutputAuthSpec {
    fn from(condition: SpendCondition) -> Self {
        Self::Conditions(vec![condition])
    }
}

impl From<Vec<SpendCondition>> for OutputAuthSpec {
    fn from(conditions: Vec<SpendCondition>) -> Self {
        Self::Conditions(conditions)
    }
}

pub struct OutputSpec {
    value: u64,
    auth: OutputAuthSpec,
}

impl OutputSpec {
    pub fn new(value: u64, auth: OutputAuthSpec) -> Self {
        Self { value, auth }
    }

    pub fn key_path_from_mask(value: u64) -> Self {
        Self {
            value,
            auth: OutputAuthSpec::KeyPathFromMask,
        }
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn auth(&self) -> &OutputAuthSpec {
        &self.auth
    }
}

impl From<u64> for OutputSpec {
    fn from(amount: u64) -> Self {
        Self::key_path_from_mask(amount)
    }
}

impl From<(u64, SpendCondition)> for OutputSpec {
    fn from((value, condition): (u64, SpendCondition)) -> Self {
        Self {
            value,
            auth: OutputAuthSpec::Conditions(vec![condition]),
        }
    }
}

impl From<(u64, OutputAuthSpec)> for OutputSpec {
    fn from((value, auth): (u64, OutputAuthSpec)) -> Self {
        Self { value, auth }
    }
}

/// How a spent stealth input is authorised (TIP-0006).
#[derive(Debug, Clone)]
pub enum InputAuthSpec {
    /// Key-path spend.
    KeyPath,
    /// Script-path spend: reveal `leaf` from the UTXO's committed `conditions` set, with its inclusion proof, supplying
    /// a witness `data` blob the leaf interprets (e.g. a hashlock preimage).
    ScriptPath {
        conditions: Vec<SpendCondition>,
        leaf: SpendCondition,
        data: Bytes,
    },
}

impl InputAuthSpec {
    /// A script-path spend over a single-leaf condition tree (the common case): the committed set is `{condition}` and
    /// that condition is the revealed leaf, with no witness data.
    pub fn single(condition: SpendCondition) -> Self {
        Self::ScriptPath {
            conditions: vec![condition.clone()],
            leaf: condition,
            data: Bytes::default(),
        }
    }

    /// A script-path spend revealing `leaf` from `conditions`, supplying a witness `data` blob the leaf interprets.
    pub fn script_path(conditions: Vec<SpendCondition>, leaf: SpendCondition, data: Bytes) -> Self {
        Self::ScriptPath { conditions, leaf, data }
    }
}

pub struct InputSpec {
    mask_and_value: MaskAndValue,
    auth: InputAuthSpec,
}

impl InputSpec {
    pub fn new(mask_and_value: MaskAndValue) -> Self {
        Self {
            mask_and_value,
            auth: InputAuthSpec::KeyPath,
        }
    }

    pub fn with_auth(mask_and_value: MaskAndValue, auth: InputAuthSpec) -> Self {
        Self { mask_and_value, auth }
    }

    pub fn mask_and_value(&self) -> &MaskAndValue {
        &self.mask_and_value
    }

    pub fn auth(&self) -> &InputAuthSpec {
        &self.auth
    }
}

impl From<MaskAndValue> for InputSpec {
    fn from(mask_and_value: MaskAndValue) -> Self {
        Self::new(mask_and_value)
    }
}

impl From<(MaskAndValue, SpendCondition)> for InputSpec {
    fn from((mask_and_value, condition): (MaskAndValue, SpendCondition)) -> Self {
        Self::with_auth(mask_and_value, InputAuthSpec::single(condition))
    }
}

impl From<(MaskAndValue, InputAuthSpec)> for InputSpec {
    fn from((mask_and_value, auth): (MaskAndValue, InputAuthSpec)) -> Self {
        Self::with_auth(mask_and_value, auth)
    }
}
