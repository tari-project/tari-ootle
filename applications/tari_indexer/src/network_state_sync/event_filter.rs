//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::{events::Event, substate::SubstateId};
use tari_template_lib_types::{EntityId, ResourceAddress, TemplateAddress};

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
    /// True when no field is set, so the filter admits every event. A filter list containing one of
    /// these indexes the whole network, the same as configuring no filters at all.
    pub fn is_match_all(&self) -> bool {
        self.topic.is_none() &&
            self.entity_id.is_none() &&
            self.substate_id.is_none() &&
            self.template_address.is_none() &&
            self.resource_address.is_none()
    }

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
            .is_some_and(|t| t != event.template_address())
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

    /// The resource address an event names directly, if any.
    ///
    /// Only the `std.resource.*` events (`create`, `mint`, `recall`, `freeze`, `unfreeze`,
    /// `update_access_rule`, `update_auth_hook`, `update_metadata`, `update_nonfungible_data`)
    /// qualify: they carry the resource address as their `substate_id`.
    ///
    /// A vault event does not. Its `substate_id` is the `VaultId`, an opaque `ObjectKey` that
    /// encodes the owning entity and nothing about the resource, and the payload no longer
    /// duplicates what the vault substate already records. A subscriber that wants one resource's
    /// transfers resolves the vaults it cares about — a vault's resource is fixed for its life, so
    /// one lookup holds forever — and filters on `substate_id` instead. That is also the narrower
    /// subscription: a resource filter would match every account on the network holding it.
    pub fn event_resource_address(event: &Event) -> Option<ResourceAddress> {
        event.substate_id().and_then(|s| s.as_resource_address())
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

    fn vault_deposit_event(vault_byte: u8, template_addr: TemplateAddress) -> Event {
        let vault_id = VaultId::new(ObjectKey::from_array([vault_byte; ObjectKey::LENGTH]));
        let payload = Metadata::from_iter([("amount", "100".to_string())]);
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

    /// A vault event names no resource, so a resource filter cannot admit it. Subscribers after one
    /// resource's transfers filter on the vault's `substate_id` instead.
    #[test]
    fn a_resource_filter_never_matches_a_vault_event() {
        let vault_id = VaultId::new(ObjectKey::from_array([9; ObjectKey::LENGTH]));
        let event = vault_deposit_event(9, template(3));

        assert!(EventFilter::event_resource_address(&event).is_none());

        let by_resource = EventFilter {
            resource_address: Some(resource(1)),
            ..Default::default()
        };
        assert!(!by_resource.matches(&event));

        let by_vault = EventFilter {
            substate_id: Some(vault_id.into()),
            ..Default::default()
        };
        assert!(by_vault.matches(&event));
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
        // An event whose substate_id is not a resource must not match a resource_address filter.
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
        let tmpl = template(2);
        let vault_id = VaultId::new(ObjectKey::from_array([9; ObjectKey::LENGTH]));
        let event = vault_deposit_event(9, tmpl);

        // All filters match
        let filter = EventFilter {
            topic: Some("std.vault.deposit".into()),
            template_address: Some(tmpl),
            substate_id: Some(vault_id.into()),
            ..Default::default()
        };
        assert!(filter.matches(&event));

        // Topic mismatches => no match even if the vault matches
        let filter = EventFilter {
            topic: Some("std.vault.withdraw".into()),
            substate_id: Some(vault_id.into()),
            ..Default::default()
        };
        assert!(!filter.matches(&event));
    }

    #[test]
    fn empty_filter_matches_any_event() {
        let event = vault_deposit_event(9, template(2));
        let filter = EventFilter::default();
        assert!(filter.matches(&event));
    }

    /// A payload entry named `resource_address` is a template's own data, not a resource the filter
    /// recognises. Only `substate_id` decides.
    #[test]
    fn a_resource_address_payload_entry_does_not_make_an_event_match() {
        let tmpl = template(1);
        let token = resource(1);
        let payload = Metadata::from_iter([("resource_address", token.to_string())]);
        let event = Event::custom(None, tmpl, "mytemplate.transfer".to_string(), payload);

        assert!(EventFilter::event_resource_address(&event).is_none());

        let filter = EventFilter {
            resource_address: Some(token),
            ..Default::default()
        };
        assert!(!filter.matches(&event));
    }
}
