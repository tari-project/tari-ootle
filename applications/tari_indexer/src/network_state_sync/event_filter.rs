//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_template_lib_types::{EntityId, TemplateAddress};

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct EventFilter {
    pub topic: Option<Box<str>>,
    pub entity_id: Option<EntityId>,
    pub substate_id: Option<SubstateId>,
    pub template_address: Option<TemplateAddress>,
}

impl EventFilter {
    pub fn matches(&self, event: &Event) -> bool {
        if self.topic.as_ref().is_some_and(|t| !Self::topic_matches(t, event.topic())) {
            return false;
        }

        if self
            .template_address
            .as_ref()
            .is_some_and(|t| *t != event.template_address())
        {
            return false;
        }

        if self
            .substate_id
            .as_ref()
            .is_some_and(|substate_id| event.substate_id().map(|s| s != substate_id).unwrap_or(true))
        {
            return false;
        }

        self.entity_id.as_ref().is_none_or(|entity_id| {
            event
                .substate_id()
                .map(|s| s.to_object_key().as_entity_id() == *entity_id)
                .unwrap_or(false)
        })
    }

    /// Match a topic filter against an event topic.
    /// Supports exact match and prefix wildcard (e.g. "std.vault.*" matches "std.vault.withdraw").
    fn topic_matches(filter: &str, topic: &str) -> bool {
        match filter.strip_suffix('*') {
            Some(prefix) => topic.starts_with(prefix),
            None => filter == topic,
        }
    }
}
