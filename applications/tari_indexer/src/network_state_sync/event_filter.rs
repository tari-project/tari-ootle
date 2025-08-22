//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_template_lib::prelude::{EntityId, TemplateAddress};

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct EventFilter {
    pub topic: Option<String>,
    pub entity_id: Option<EntityId>,
    pub substate_id: Option<SubstateId>,
    pub template_address: Option<TemplateAddress>,
}

impl EventFilter {
    pub fn matches(&self, event: &Event) -> bool {
        let matches_topic = self.topic.as_ref().is_none_or(|t| *t == event.topic());
        let matches_template = self
            .template_address
            .as_ref()
            .is_none_or(|t| *t == event.template_address());

        let matches_substate_id = self
            .substate_id
            .as_ref()
            .is_none_or(|substate_id| event.substate_id().map(|s| s == substate_id).unwrap_or(false));

        let matches_entity_id = self.entity_id.as_ref().is_none_or(|entity_id| {
            event
                .substate_id()
                .map(|s| s.to_object_key().as_entity_id() == *entity_id)
                .unwrap_or(false)
        });

        matches_topic && matches_template && matches_substate_id && matches_entity_id
    }
}
