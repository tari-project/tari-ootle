//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::str::FromStr;

use crate::{Hash, address_prefixes};

/// The address of a Template
// TODO: should we refactor TemplateAddress as a newtype ?
pub type TemplateAddress = Hash;

pub fn parse_template_address(s: &str) -> Option<TemplateAddress> {
    if let Some(hash_str) = s.strip_prefix(address_prefixes::TEMPLATE) &&
        let Ok(address) = TemplateAddress::from_str(hash_str)
    {
        return Some(address);
    }

    None
}
