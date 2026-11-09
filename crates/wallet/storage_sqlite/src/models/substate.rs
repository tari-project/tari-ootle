//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Identifiable, Queryable};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateId};
use tari_ootle_wallet_sdk::{models::SubstateModel, storage::WalletStorageError};
use tari_template_lib_types::Hash32;
use time::PrimitiveDateTime;

use crate::{schema::substates, serialization::deserialize_json};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = substates)]
pub struct Substate {
    pub id: i32,
    pub module_name: Option<String>,
    pub address: String,
    pub parent_address: Option<String>,
    pub referenced_substates: String,
    pub template_address: Option<String>,
    pub created_at: PrimitiveDateTime,
    pub version: i64,
}

impl Substate {
    pub fn try_to_record(&self) -> Result<SubstateModel, WalletStorageError> {
        let substate_id = SubstateId::from_str(&self.address).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_to_record",
            item: "address",
            details: e.to_string(),
        })?;
        let version = SubstateVersion::new(self.version as u64);
        let parent_address = self
            .parent_address
            .as_ref()
            .map(|s| SubstateId::from_str(s))
            .transpose()
            .map_err(|e| WalletStorageError::DecodingError {
                operation: "try_to_record",
                item: "parent_address",
                details: e.to_string(),
            })?;

        Ok(SubstateModel {
            module_name: self.module_name.clone(),
            substate_id: VersionedSubstateId::new(substate_id, version),
            parent_address,
            referenced_substates: deserialize_json(&self.referenced_substates)?,
            template_address: self
                .template_address
                .as_ref()
                .map(|s| Hash32::from_hex(s))
                .transpose()
                .map_err(|e| WalletStorageError::DecodingError {
                    operation: "try_to_record",
                    item: "template_address",
                    details: e.to_string(),
                })?,
        })
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::{ComponentAddress, ObjectKey};
    use time::macros::datetime;

    use super::*;

    fn sample_substate() -> Substate {
        let addr = SubstateId::Component(ComponentAddress::from_array([0x11; ObjectKey::LENGTH]));
        Substate {
            id: 1,
            module_name: Some("Test".to_string()),
            address: addr.to_string(),
            parent_address: None,
            referenced_substates: "[]".to_string(),
            template_address: None,
            created_at: datetime!(2026-01-01 00:00:00),
            version: 3,
        }
    }

    #[test]
    fn try_to_record_rejects_corrupt_ids_and_round_trips_u64_max_version() {
        let valid = sample_substate();
        let model = valid.try_to_record().unwrap();
        assert_eq!(model.substate_id.version(), SubstateVersion::new(3));

        let mut bad_address = sample_substate();
        bad_address.address = "not-a-substate-id".to_string();
        assert!(matches!(
            bad_address.try_to_record(),
            Err(WalletStorageError::DecodingError { item: "address", .. })
        ));

        let mut bad_parent = sample_substate();
        bad_parent.parent_address = Some("not-a-parent-id".to_string());
        assert!(matches!(
            bad_parent.try_to_record(),
            Err(WalletStorageError::DecodingError {
                item: "parent_address",
                ..
            })
        ));

        let mut max_version = sample_substate();
        max_version.version = u64::MAX as i64;
        let model = max_version.try_to_record().unwrap();
        assert_eq!(model.substate_id.version(), SubstateVersion::new(u64::MAX));
    }
}
