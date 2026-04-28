//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_template_lib_types::{EntityId, ResourceAddress, TemplateAddress};

/// Payload metadata key under which events (e.g. `std.vault.deposit`, `std.vault.withdraw`)
/// carry the resource address of the resource being transferred.
const PAYLOAD_RESOURCE_ADDRESS_KEY: &str = "resource_address";

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct EventFilter {
    pub topic: Option<Box<str>>,
    pub entity_id: Option<EntityId>,
    pub substate_id: Option<SubstateId>,
    pub template_address: Option<TemplateAddress>,
    #[serde(default)]
    pub resource_address: Option<ResourceAddress>,
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

        if let Some(filter_resource) = self.resource_address.as_ref() {
            match Self::event_resource_address(event) {
                Some(event_resource) if event_resource == *filter_resource => {},
                _ => return false,
            }
        }

        self.entity_id.as_ref().is_none_or(|entity_id| {
            event
                .substate_id()
                .map(|s| s.to_object_key().as_entity_id() == *entity_id)
                .unwrap_or(false)
        })
    }

    /// Derive the resource address an event refers to, if any.
    ///
    /// Two cases are recognised:
    /// 1. The event's `substate_id` is a `Resource(..)` — the `std.resource.*` events (`create`, `mint`, `recall`,
    ///    `freeze`, `unfreeze`, `update_access_rules`, `update_nonfungible_data`) all attach the resource address as
    ///    the substate_id.
    /// 2. The event payload contains a `resource_address` entry parseable as a `ResourceAddress` — `std.vault.deposit`
    ///    and `std.vault.withdraw` both set this.
    pub fn event_resource_address(event: &Event) -> Option<ResourceAddress> {
        if let Some(addr) = event.substate_id().and_then(|s| s.as_resource_address()) {
            return Some(addr);
        }
        event
            .get_payload(PAYLOAD_RESOURCE_ADDRESS_KEY)
            .and_then(|s| ResourceAddress::from_str(s).ok())
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

#[cfg(test)]
mod tests {
    use tari_template_lib_types::{Metadata, ObjectKey, VaultId};

    use super::*;

    fn resource(byte: u8) -> ResourceAddress {
        ResourceAddress::new(ObjectKey::from_array([byte; ObjectKey::LENGTH]))
    }

    fn template(byte: u8) -> TemplateAddress {
        TemplateAddress::from_array([byte; 32])
    }

    fn vault_deposit_event(vault_byte: u8, resource: &ResourceAddress, template_addr: TemplateAddress) -> Event {
        let vault_id = VaultId::new(ObjectKey::from_array([vault_byte; ObjectKey::LENGTH]));
        let payload = Metadata::from_iter([("resource_address", resource.to_string())]);
        Event::std(Some(vault_id.into()), template_addr, "vault", "deposit", payload)
    }

    fn resource_mint_event(resource: &ResourceAddress, template_addr: TemplateAddress) -> Event {
        Event::std(
            Some((*resource).into()),
            template_addr,
            "resource",
            "mint",
            Metadata::new(),
        )
    }

    #[test]
    fn matches_vault_deposit_by_resource_address_in_payload() {
        let token = resource(1);
        let other = resource(2);
        let event = vault_deposit_event(9, &token, template(3));

        let matching = EventFilter {
            resource_address: Some(token),
            ..Default::default()
        };
        assert!(matching.matches(&event));

        let non_matching = EventFilter {
            resource_address: Some(other),
            ..Default::default()
        };
        assert!(!non_matching.matches(&event));
    }

    #[test]
    fn matches_resource_mint_by_substate_id_resource() {
        // For std.resource.* events, the resource address is the event's substate_id.
        let token = resource(7);
        let event = resource_mint_event(&token, template(4));

        let filter = EventFilter {
            resource_address: Some(token),
            ..Default::default()
        };
        assert!(filter.matches(&event));

        let wrong = EventFilter {
            resource_address: Some(resource(8)),
            ..Default::default()
        };
        assert!(!wrong.matches(&event));
    }

    #[test]
    fn rejects_events_without_resource_when_filter_set() {
        // An event that has neither a Resource substate_id nor a resource_address payload entry
        // must not match a resource_address filter.
        let template_addr = template(5);
        let event = Event::std(None, template_addr, "component", "updated", Metadata::new());

        let filter = EventFilter {
            resource_address: Some(resource(1)),
            ..Default::default()
        };
        assert!(!filter.matches(&event));
    }

    #[test]
    fn combines_with_other_filters() {
        let token = resource(1);
        let tmpl = template(2);
        let event = vault_deposit_event(9, &token, tmpl);

        // All filters match
        let filter = EventFilter {
            topic: Some("std.vault.deposit".into()),
            template_address: Some(tmpl),
            resource_address: Some(token),
            ..Default::default()
        };
        assert!(filter.matches(&event));

        // Topic mismatches => no match even if resource matches
        let filter = EventFilter {
            topic: Some("std.vault.withdraw".into()),
            resource_address: Some(token),
            ..Default::default()
        };
        assert!(!filter.matches(&event));
    }

    #[test]
    fn empty_filter_matches_any_event() {
        let event = vault_deposit_event(9, &resource(1), template(2));
        let filter = EventFilter::default();
        assert!(filter.matches(&event));
    }

    #[test]
    fn derives_resource_address_from_payload_only_when_parseable() {
        let tmpl = template(1);
        let payload = Metadata::from_iter([("resource_address", "not-a-valid-address".to_string())]);
        let event = Event::std(None, tmpl, "vault", "deposit", payload);

        assert!(EventFilter::event_resource_address(&event).is_none());

        let filter = EventFilter {
            resource_address: Some(resource(1)),
            ..Default::default()
        };
        assert!(!filter.matches(&event));
    }
}
