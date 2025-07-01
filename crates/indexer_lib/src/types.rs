//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_engine_types::{substate::SubstateValue, template_lib_models::NonFungibleAddress};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NonFungibleSubstate {
    pub version: u32,
    pub address: NonFungibleAddress,
    pub substate: SubstateValue,
}
