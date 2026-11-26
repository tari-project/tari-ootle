//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::str::FromStr;

use crate::{Hash32, address_prefixes};

/// The address of a Template
// TODO: should we refactor TemplateAddress as a newtype ?
pub type TemplateAddress = Hash32;

pub fn parse_template_address(s: &str) -> Option<TemplateAddress> {
    let (prefix, hash_str) = s.split_once('_')?;
    if prefix != address_prefixes::TEMPLATE {
        return None;
    }
    TemplateAddress::from_str(hash_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_and_rejects_malformed_template_addresses() {
        let hex = "7cbfe29101c24924b1b6ccefbfff98986d648622272ae24f7585dab5ffffffff";
        let addr = parse_template_address(&format!("template_{hex}")).expect("valid template_ address should parse");
        assert_eq!(addr.to_string(), hex);

        assert!(parse_template_address(&format!("template{hex}")).is_none());
        assert!(parse_template_address(&format!("component_{hex}")).is_none());
        assert!(parse_template_address("template_").is_none());
    }
}
