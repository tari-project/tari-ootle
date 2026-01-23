//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::MaskAndValue;
use tari_template_lib::types::stealth::SpendCondition;

pub struct OutputSpec {
    value: u64,
    spend_condition_spec: SpendConditionSpec,
}

impl OutputSpec {
    pub fn signed_by(value: u64) -> Self {
        Self {
            value,
            spend_condition_spec: SpendConditionSpec::SignedBy,
        }
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn spend_condition_spec(&self) -> &SpendConditionSpec {
        &self.spend_condition_spec
    }
}

impl From<u64> for OutputSpec {
    fn from(amount: u64) -> Self {
        Self::signed_by(amount)
    }
}

impl From<(u64, SpendCondition)> for OutputSpec {
    fn from((value, spend_condition): (u64, SpendCondition)) -> Self {
        Self {
            value,
            spend_condition_spec: SpendConditionSpec::Specified(spend_condition),
        }
    }
}

pub enum SpendConditionSpec {
    SignedBy,
    Specified(SpendCondition),
}

pub struct InputSpec {
    mask_and_value: MaskAndValue,
}

impl InputSpec {
    pub fn new(mask_and_value: MaskAndValue) -> Self {
        Self { mask_and_value }
    }

    pub fn mask_and_value(&self) -> &MaskAndValue {
        &self.mask_and_value
    }
}

impl From<MaskAndValue> for InputSpec {
    fn from(mask_and_value: MaskAndValue) -> Self {
        Self::new(mask_and_value)
    }
}
