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
        if self
            .topic
            .as_ref()
            .is_some_and(|t| !Self::topic_matches(t, event.topic()))
        {
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

    /// Convert a topic filter with `*` wildcards to a SQL LIKE pattern.
    /// `*` segments become `%`. Returns `None` if no wildcards are present.
    pub fn topic_to_like_pattern(filter: &str) -> Option<String> {
        if !filter.contains('*') {
            return None;
        }
        // Escape any existing SQL LIKE special chars, then replace * with %
        let escaped = filter.replace('%', r"\%").replace('_', r"\_");
        Some(escaped.replace('*', "%"))
    }

    /// Match a topic filter against an event topic using dot-separated segments.
    ///
    /// `*` matches any single segment. Examples:
    /// - `std.vault.withdraw` matches exactly `std.vault.withdraw`
    /// - `std.vault.*` matches `std.vault.withdraw`, `std.vault.deposit`, etc.
    /// - `std.*.withdraw` matches `std.vault.withdraw`, `std.account.withdraw`, etc.
    /// - `*.*.*` matches any three-segment topic
    pub fn topic_matches(filter: &str, topic: &str) -> bool {
        if !filter.contains('*') {
            return filter == topic;
        }

        let filter_segments = filter.split('.');
        let mut topic_segments = topic.split('.');

        for filter_seg in filter_segments {
            match topic_segments.next() {
                Some(topic_seg) => {
                    if filter_seg != "*" && filter_seg != topic_seg {
                        return false;
                    }
                },
                // Filter has more segments than the topic
                None => return false,
            }
        }

        // Topic must not have extra trailing segments
        topic_segments.next().is_none()
    }
}
