//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::str::FromStr;

use crate::{Hash32, address_prefixes};

/// The address of a Template
// TODO: should we refactor TemplateAddress as a newtype ?
pub type TemplateAddress = Hash32;

pub fn parse_template_address(s: &str) -> Option<TemplateAddress> {
    let hash_str = s.strip_prefix(address_prefixes::TEMPLATE)?.strip_prefix('_')?;
    TemplateAddress::from_str(hash_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_template_address() {
        let hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let canonical = "template_0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let parsed = parse_template_address(canonical).unwrap();
        assert_eq!(parsed, TemplateAddress::from_str(hex).unwrap());
        assert!(parse_template_address(hex).is_none());
        assert!(parse_template_address("template").is_none());
        assert!(parse_template_address("template_").is_none());
    }
}
