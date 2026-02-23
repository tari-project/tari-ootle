//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use tari_template_lib::types::SubstateOwnerRule;

/// Data that is needed to represent ownership of a value (resource or component method).
/// Owners are the only ones allowed to update the value's access rules after creation
#[derive(Debug, Clone)]
pub struct Ownership<'a> {
    pub owner_rule: Cow<'a, SubstateOwnerRule>,
}
