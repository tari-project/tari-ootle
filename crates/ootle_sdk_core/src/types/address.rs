//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Boundary address records.
//!
//! Each carries the canonical `<prefix>_<hex>` string and round-trips to/from its internal
//! `tari_template_lib_types` counterpart via that type's `Display`/`FromStr`. The contract is the
//! canonical string, never ad-hoc hex.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tari_template_lib_types::{ComponentAddress, ResourceAddress};

use crate::types::error::OotleSdkError;

/// A resource address in canonical `resource_<hex>` form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceAddressStr(pub String);

impl ResourceAddressStr {
    /// Wraps a string after validating it parses as an internal [`ResourceAddress`].
    pub fn parse(s: impl Into<String>) -> Result<Self, OotleSdkError> {
        let s = s.into();
        let internal = ResourceAddress::from_str(&s)
            .map_err(|e| OotleSdkError::Parse(format!("invalid resource address '{s}': {e}")))?;
        Ok(Self(internal.to_string()))
    }

    /// Borrows the canonical string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds from the internal [`ResourceAddress`].
    pub fn from_internal(addr: &ResourceAddress) -> Self {
        Self(addr.to_string())
    }

    /// Converts to the internal [`ResourceAddress`].
    pub fn to_internal(&self) -> Result<ResourceAddress, OotleSdkError> {
        ResourceAddress::from_str(&self.0)
            .map_err(|e| OotleSdkError::Parse(format!("invalid resource address '{}': {e}", self.0)))
    }
}

/// A component address in canonical `component_<hex>` form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentAddressStr(pub String);

impl ComponentAddressStr {
    /// Wraps a string after validating it parses as an internal [`ComponentAddress`].
    pub fn parse(s: impl Into<String>) -> Result<Self, OotleSdkError> {
        let s = s.into();
        let internal = ComponentAddress::from_str(&s)
            .map_err(|e| OotleSdkError::Parse(format!("invalid component address '{s}': {e}")))?;
        Ok(Self(internal.to_string()))
    }

    /// Borrows the canonical string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds from the internal [`ComponentAddress`].
    pub fn from_internal(addr: &ComponentAddress) -> Self {
        Self(addr.to_string())
    }

    /// Converts to the internal [`ComponentAddress`].
    pub fn to_internal(&self) -> Result<ComponentAddress, OotleSdkError> {
        ComponentAddress::from_str(&self.0)
            .map_err(|e| OotleSdkError::Parse(format!("invalid component address '{}': {e}", self.0)))
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::ObjectKey;

    use super::*;

    fn sample_resource() -> ResourceAddress {
        ResourceAddress::new(ObjectKey::from_array([0x11; ObjectKey::LENGTH]))
    }

    fn sample_component() -> ComponentAddress {
        ComponentAddress::new(ObjectKey::from_array([0x22; ObjectKey::LENGTH]))
    }

    #[test]
    fn resource_round_trips_through_string() {
        let internal = sample_resource();
        let s = internal.to_string();
        assert!(s.starts_with("resource_"));
        let boundary = ResourceAddressStr::parse(&s).unwrap();
        assert_eq!(boundary.as_str(), s);
        assert_eq!(boundary.to_internal().unwrap(), internal);
        assert_eq!(ResourceAddressStr::from_internal(&internal), boundary);
    }

    #[test]
    fn component_round_trips_through_string() {
        let internal = sample_component();
        let s = internal.to_string();
        assert!(s.starts_with("component_"));
        let boundary = ComponentAddressStr::parse(&s).unwrap();
        assert_eq!(boundary.as_str(), s);
        assert_eq!(boundary.to_internal().unwrap(), internal);
        assert_eq!(ComponentAddressStr::from_internal(&internal), boundary);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(ResourceAddressStr::parse("nope").unwrap_err().code(), "PARSE");
        assert_eq!(ComponentAddressStr::parse("nope").unwrap_err().code(), "PARSE");
    }
}
