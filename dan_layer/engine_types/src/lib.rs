//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

pub mod base_layer_hashing;
pub mod bucket;
pub mod commit_result;
pub mod component;
pub mod confidential;
pub mod events;
pub mod fees;
pub mod hashing;
pub mod indexed_value;
pub mod instruction;
pub mod instruction_result;
pub mod lock;
pub mod logs;
pub mod non_fungible;
pub mod non_fungible_index;
pub mod proof;
pub mod resource;
pub mod resource_container;
pub mod serde_with;
pub mod substate;
pub mod transaction_receipt;
pub mod vault;
pub mod virtual_substate;

mod template;
pub use template::{calculate_template_binary_hash, parse_template_address, TemplateAddress};

pub mod entity_id_provider;
pub mod id_provider;

mod argument_parser;
pub mod published_template;
mod substate_serde;
pub mod vn_fee_pool;

pub use argument_parser::parse_arg;

pub mod template_models {
    pub use tari_template_lib::models::*;
}
