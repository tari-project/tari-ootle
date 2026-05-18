//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{prelude::*, types::MaxVec};

// Stripped to minicbor-only: template_lib's serde feature is off in this crate (templates ship
// WASM, no JSON), so MaxVec / ResourceManager / Vault no longer implement serde here.
#[derive(minicbor::Decode, minicbor::Encode, minicbor::CborLen)]
pub struct TemplateV1 {
    #[n(0)]
    signers: MaxVec<10, RistrettoPublicKeyBytes>,
    #[n(1)]
    manager: ResourceManager,
    #[n(2)]
    supply_vault: Vault,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct User {
    #[n(0)]
    pub public_key: RistrettoPublicKeyBytes,
    #[n(1)]
    pub id: u32,
}

#[cfg(not(feature = "v1_compat"))]
#[template]
mod template {

    use super::*;

    pub struct TemplateV2 {
        signers: MaxVec<20, User>,
        current_id: u32,
        new_data: Option<MaxString<50>>,
        manager: ResourceManager,
        supply_vault: Vault,
        another_vault: Vault,
    }

    impl TemplateV2 {
        #[migration]
        pub fn migrate_v1_to_v2(previous_state: TemplateV1) -> Self {
            let current_id = previous_state.signers.len() as u32;
            Self {
                signers: previous_state
                    .signers
                    .into_iter()
                    .enumerate()
                    .map(|(id, pk)| User {
                        public_key: pk,
                        id: u32::try_from(id).unwrap(),
                    })
                    .collect(),
                new_data: None,
                current_id,
                manager: previous_state.manager,
                another_vault: Vault::new_empty(previous_state.supply_vault.resource_address()),
                supply_vault: previous_state.supply_vault,
            }
        }

        pub fn faulty_not_migrate_function(previous_state: TemplateV1) -> Self {
            Self::migrate_v1_to_v2(previous_state)
        }

        #[migration]
        pub fn migrate_v1_to_v2_with_args(previous_state: TemplateV1, new_data: MaxString<50>) -> Self {
            let current_id = previous_state.signers.len() as u32;
            Self {
                signers: previous_state
                    .signers
                    .into_iter()
                    .enumerate()
                    .map(|(id, pk)| User {
                        public_key: pk,
                        id: u32::try_from(id).unwrap(),
                    })
                    .collect(),
                new_data: Some(new_data),
                current_id,
                manager: previous_state.manager,
                another_vault: Vault::new_empty(previous_state.supply_vault.resource_address()),
                supply_vault: previous_state.supply_vault,
            }
        }

        #[migration]
        pub fn faulty_migrate_drop_vault(previous_state: TemplateV1) -> Self {
            let current_id = previous_state.signers.len() as u32;
            Self {
                signers: Default::default(),
                current_id,
                manager: previous_state.manager,
                new_data: None,
                another_vault: Vault::new_empty(previous_state.supply_vault.resource_address()),
                supply_vault: Vault::new_empty(previous_state.supply_vault.resource_address()),
            }
        }

        #[migration]
        pub fn faulty_migrate_panic(_previous_state: TemplateV1) -> Self {
            panic!("Intentional panic during migration");
        }

        #[migration]
        pub fn faulty_migrate_cross_template_call(mut previous_state: TemplateV1) -> Self {
            let bucket = previous_state.supply_vault.withdraw_all();
            TemplateManager::get(TemplateAddress::from_array([0; 32])).invoke("deposit", args![bucket]);
            let current_id = previous_state.signers.len() as u32;
            Self {
                signers: Default::default(),
                current_id,
                manager: previous_state.manager,
                new_data: None,
                another_vault: Vault::new_empty(previous_state.supply_vault.resource_address()),
                supply_vault: previous_state.supply_vault,
            }
        }

        pub fn assert_correct(&self) {
            assert_eq!(self.signers.len() as u32, self.current_id);
            self.signers.iter().for_each(|user| {
                assert!(user.id < self.current_id);
            });
        }
    }
}

#[cfg(feature = "v1_compat")]
#[template]
mod template {

    use super::*;

    /// This struct is compatible with V1 so no migration function is necessary.
    ///
    /// Field ORDER matters: minicbor encodes structs as positional arrays indexed by `#[n(N)]`,
    /// so adding fields is forwards-compatible ONLY when they're appended at the end with
    /// `#[cbor(default)]` (or `Option<T>`). V1 declared `signers, manager, supply_vault` in that
    /// order; V2 keeps those in the same positions and appends `current_id`/`new_data`.
    pub struct TemplateV2 {
        signers: MaxVec<20, RistrettoPublicKeyBytes>,
        manager: ResourceManager,
        supply_vault: Vault,
        // Set to 0 if not present in the encoded V1 state.
        #[cbor(default)]
        current_id: u32,
        // Option<T> decodes from a trailing null without an explicit cbor(default).
        new_data: Option<MaxString<50>>,
    }

    impl TemplateV2 {
        pub fn assert_correct(&self) {
            assert_eq!(self.current_id, 0);
            assert!(self.new_data.is_none());
        }
    }
}
