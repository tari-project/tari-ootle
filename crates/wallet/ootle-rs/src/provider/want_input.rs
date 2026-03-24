//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::engine_types::substate::SubstateId;
use tari_template_lib_types::{ComponentAddress, ResourceAddress};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum WantInput {
    /// A specific vault for a given resource from a component
    /// The resolver will fetch the component, inspect it's state and extract the vaults, then attempt to find the
    /// vault for the given resource address.
    VaultForResource {
        component_address: ComponentAddress,
        resource_address: ResourceAddress,
        required: bool,
    },
    /// Adds a substate as an input. If required is false, the resolver only add the input if it was found.
    SpecificSubstate { substate_id: SubstateId, required: bool },
    /// Fetches the component state and adds ALL vaults found in it as inputs.
    /// Used by the generic component builder when the specific vault access pattern is unknown.
    AllComponentVaults { component_address: ComponentAddress },
}
