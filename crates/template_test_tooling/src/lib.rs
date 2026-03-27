//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

mod builtin_component_state;
pub mod compile;
mod helpers;
pub mod mocks;
mod package_builder;
mod read_only_state_store;
pub mod support;
mod template_spec;
mod template_test;
mod track_calls;
mod wrapped_transaction;

pub use package_builder::Package;
pub use template_test::{TemplateTest, xtr_faucet_component};

// Re-export types used in public interfaces. This allows users to use these types when writing tests without including
// the various crates themselves.
pub mod crypto {
    pub use tari_crypto::{keys::*, ristretto::*};
}

pub use ootle_byte_type as byte_type;
pub use tari_engine_types as engine_types;
pub use tari_ootle_transaction as transaction;
pub use tari_ootle_wallet_crypto as wallet_crypto;
pub use tari_template_lib::types as template_lib_types;
