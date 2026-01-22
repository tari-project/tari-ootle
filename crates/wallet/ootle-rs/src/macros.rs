//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_export]
macro_rules! resource_address {
    ($s:expr) => {
        $crate::macros::_macro_exports::ResourceAddress::from_hex($s).expect("Failed to parse resource string")
    };
}

pub mod _macro_exports {
    pub use tari_template_lib_types::ResourceAddress;
}
