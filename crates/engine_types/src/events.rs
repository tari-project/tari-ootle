//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_template_lib::types::{Metadata, TemplateAddress};

use crate::substate::SubstateId;

// Topics for builtin events emitted by the engine
const STANDARD_TOPIC_PREFIX: &str = "std.";

/// The widest decimal an amount renders to, amounts being `u64` microtari.
const WIDEST_AMOUNT: &str = "18446744073709551615";

fn std_event(object_name: &str, action_name: &str) -> String {
    format!("{}{}.{}", STANDARD_TOPIC_PREFIX, object_name, action_name)
}

#[derive(
    Debug,
    Clone,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    PartialEq,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Event {
    #[n(0)]
    substate_id: Option<SubstateId>,
    #[n(1)]
    template_address: TemplateAddress,
    #[n(2)]
    topic: String,
    #[n(3)]
    payload: Metadata,
}

impl Event {
    pub fn new(
        substate_id: Option<SubstateId>,
        template_address: TemplateAddress,
        topic: String,
        payload: Metadata,
    ) -> Self {
        Self {
            substate_id,
            template_address,
            topic,
            payload,
        }
    }

    pub fn custom(
        substate_id: Option<SubstateId>,
        template_address: TemplateAddress,
        topic: String,
        payload: Metadata,
    ) -> Self {
        Self::new(substate_id, template_address, topic, payload)
    }

    pub fn std(
        substate_id: Option<SubstateId>,
        template_address: TemplateAddress,
        object_name: &str,
        action_name: &str,
        payload: Metadata,
    ) -> Self {
        Self::new(
            substate_id,
            template_address,
            std_event(object_name, action_name),
            payload,
        )
    }

    /// Whether this is the standard event `object_name` emits for `action_name`. Matches the topic
    /// [`std_event`] builds, without building it.
    pub fn is_std(&self, object_name: &str, action_name: &str) -> bool {
        self.topic
            .strip_prefix(STANDARD_TOPIC_PREFIX)
            .and_then(|rest| rest.strip_prefix(object_name))
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|rest| rest == action_name)
    }

    /// The bytes to add to this event's encoded length when pricing the permanent state a
    /// transaction pays for, so that the price cannot depend on the fee being priced.
    ///
    /// A fee-payment event records the payment as a decimal string, so its length tracks `max_fee`
    /// digit for digit. Charging that verbatim lets the storage price read back the very amount it
    /// is pricing: a submission whose `max_fee` is wider than the one a dry run measured costs more
    /// than that dry run reported, and the transaction is rejected for underpayment. Pricing it at
    /// its widest breaks the loop — the same stand-in [`crate::fees::FeeReceipt::widest`] gets, for
    /// the same reason.
    ///
    /// Only the engine's own fee event is neutralized here. A template that echoes an amount its
    /// caller passed it is priced as written: that coupling is visible to whoever chose both the
    /// `max_fee` and the template, and is theirs to account for.
    pub fn charged_size_padding(&self) -> usize {
        if !self.is_std("vault", "pay_fee") {
            return 0;
        }
        self.get_payload("amount").map_or(0, |amount| {
            minicbor::len(WIDEST_AMOUNT).saturating_sub(minicbor::len(amount))
        })
    }

    pub fn validate_custom_topic<T: AsRef<str>>(topic: T) -> Result<(), String> {
        let s = topic.as_ref();
        if topic.as_ref().starts_with(STANDARD_TOPIC_PREFIX) {
            return Err("topics starting with 'std.' are reserved for standard events".to_string());
        }

        if s.len() > 255 {
            return Err("topic is too long".to_string());
        }

        // Check for only letters and numbers
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
            return Err("topic can only contain letters, numbers and underscores".to_string());
        }

        Ok(())
    }

    pub fn substate_id(&self) -> Option<&SubstateId> {
        self.substate_id.as_ref()
    }

    pub fn template_address(&self) -> &TemplateAddress {
        &self.template_address
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub fn get_payload(&self, key: &str) -> Option<&str> {
        self.payload.get(key)
    }

    pub fn payload(&self) -> &Metadata {
        &self.payload
    }

    pub fn into_payload(self) -> Metadata {
        self.payload
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "event: substate_id ")?;
        if let Some(substate_id) = &self.substate_id {
            write!(f, "{}, ", substate_id)?;
        } else {
            write!(f, "None, ")?;
        }
        write!(
            f,
            "template_address {}, topic {} and payload {}",
            self.template_address, self.topic, self.payload
        )
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::Hash32;

    use super::*;

    fn pay_fee_event(amount: &str) -> Event {
        Event::std(
            None,
            Hash32::from_array([0u8; Hash32::LENGTH]),
            "vault",
            "pay_fee",
            Metadata::from_iter([("amount", amount.to_string())]),
        )
    }

    #[test]
    fn is_std_recognises_the_topics_std_event_builds() {
        let event = pay_fee_event("1000");
        assert_eq!(event.topic(), std_event("vault", "pay_fee"));
        assert!(event.is_std("vault", "pay_fee"));
        assert!(!event.is_std("vault", "deposit"));
        assert!(!event.is_std("component", "pay_fee"));
        // A prefix of the object name must not match.
        assert!(!event.is_std("vau", "lt.pay_fee"));
    }

    #[test]
    fn a_template_cannot_reach_the_fee_topic() {
        // What makes matching on the topic safe: only the engine can emit it.
        assert!(Event::validate_custom_topic(std_event("vault", "pay_fee")).is_err());
    }

    #[test]
    fn a_fee_payment_amount_is_priced_at_one_width_whatever_its_value() {
        let widest = pay_fee_event(WIDEST_AMOUNT);
        let expected = minicbor::len(&widest) + widest.charged_size_padding();
        for amount in ["1", "1000", "398287", "18446744073709551614"] {
            let event = pay_fee_event(amount);
            assert_eq!(
                minicbor::len(&event) + event.charged_size_padding(),
                expected,
                "amount {amount} priced differently to the widest amount"
            );
        }
    }

    #[test]
    fn an_amount_a_template_records_is_priced_as_written() {
        let event = Event::custom(
            None,
            Hash32::from_array([0u8; Hash32::LENGTH]),
            "mytemplate.paid".to_string(),
            Metadata::from_iter([("amount", "398287".to_string())]),
        );
        assert_eq!(event.charged_size_padding(), 0);
    }
}
