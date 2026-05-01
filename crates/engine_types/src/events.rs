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

fn std_event(object_name: &str, action_name: &str) -> String {
    format!("{}{}.{}", STANDARD_TOPIC_PREFIX, object_name, action_name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Event {
    substate_id: Option<SubstateId>,
    template_address: TemplateAddress,
    topic: String,
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
