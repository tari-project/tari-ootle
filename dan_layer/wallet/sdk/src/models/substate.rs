//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_dan_common_types::VersionedSubstateId;
use tari_engine_types::{substate::SubstateId, TemplateAddress};

#[derive(Debug, Clone)]
pub struct SubstateModel {
    pub module_name: Option<String>,
    pub substate_id: VersionedSubstateId,
    pub parent_address: Option<SubstateId>,
    pub referenced_substates: Vec<SubstateId>,
    pub transaction_hash: FixedHash,
    pub template_address: Option<TemplateAddress>,
}
