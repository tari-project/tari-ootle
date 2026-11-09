//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::dsl;
use tari_engine_types::SubstateVersion;
use tari_ootle_common_types::Epoch;
use tari_ootle_wallet_sdk::{
    models::{ProvenUp, StealthOutputInfo, StealthOutputModel},
    storage::WalletStorageError,
};
use tari_template_lib_types::{
    ComponentAddress,
    EncryptedData,
    Hash32,
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    stealth::SpendAuthorization,
};
use time::PrimitiveDateTime;

use crate::{
    schema::stealth_outputs,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

/// Reconstructs a [`SpendAuthorization`] from the two persisted (nullable) columns. A row with neither column set is
/// corrupt — the enum makes `{no key, no conditions}` unrepresentable, so it can only arise from external tampering.
fn reconstruct_spend_authorization(
    spend_key: Option<&str>,
    condition_root: Option<&str>,
) -> Result<SpendAuthorization, WalletStorageError> {
    let spend_key = spend_key
        .map(deserialize_hex_try_from::<RistrettoPublicKeyBytes, _>)
        .transpose()?;
    let condition_root = condition_root.map(deserialize_hex_try_from::<Hash32, _>).transpose()?;
    match (spend_key, condition_root) {
        (Some(spend_key), Some(condition_root)) => Ok(SpendAuthorization::KeyAndScript {
            spend_key,
            condition_root,
        }),
        (Some(spend_key), None) => Ok(SpendAuthorization::Key(spend_key)),
        (None, Some(condition_root)) => Ok(SpendAuthorization::Script(condition_root)),
        (None, None) => Err(WalletStorageError::DecodingError {
            operation: "try_into_output",
            item: "output spend authorization",
            details: "Corrupt db: stealth output has neither spend_key nor condition_root".to_string(),
        }),
    }
}

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = stealth_outputs)]
#[allow(clippy::struct_excessive_bools)]
pub struct StealthOutput {
    pub id: i32,
    pub owner_account_id: i32,
    pub resource_address: String,
    pub commitment: String,
    pub value: i64,
    pub sender_public_nonce: String,
    pub status: String,
    pub locked_at: Option<PrimitiveDateTime>,
    pub locked_by_proof: Option<i32>,
    pub view_only_key_id: String,
    pub owner_key_id: Option<String>,
    pub encrypted_data: Vec<u8>,
    pub tag_byte: i32,
    pub memo_json: Option<String>,
    pub spend_key: Option<String>,
    pub condition_root: Option<String>,
    pub minimum_value_promise: i64,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
    pub is_condition_spendable: bool,
    pub proven_version: Option<i64>,
    pub proven_up_epoch: Option<i64>,
    pub spent_version: Option<i64>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl StealthOutput {
    pub(crate) fn try_convert(self, owner_account: ComponentAddress) -> Result<StealthOutputModel, WalletStorageError> {
        Ok(StealthOutputModel {
            owner_account,
            resource_address: self
                .resource_address
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid resource address '{}'", self.resource_address),
                })?,
            commitment: deserialize_hex_try_from(&self.commitment).map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output commitment",
                details: "Corrupt db: invalid hex representation".to_string(),
            })?,
            // `value` is u64 stored in a signed BigInt column; values >= 2^63 wrap to negative.
            value: self.value as u64,
            sender_public_nonce: RistrettoPublicKeyBytes::from_hex(&self.sender_public_nonce).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!(
                        "Corrupt db: invalid sender public nonce hex '{}'",
                        self.sender_public_nonce
                    ),
                }
            })?,
            view_only_key_id: deserialize_json(&self.view_only_key_id)?,
            owner_key_id: self.owner_key_id.as_ref().map(deserialize_json).transpose()?,
            encrypted_data: EncryptedData::try_from(self.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            tag_byte: u32::try_from(self.tag_byte).map(UtxoTag::new).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "tag_byte",
                    details: format!("Corrupt db: invalid tag_byte '{}': {e}", self.tag_byte),
                }
            })?,
            memo: self.memo_json.as_ref().map(deserialize_json).transpose()?,
            auth: reconstruct_spend_authorization(self.spend_key.as_deref(), self.condition_root.as_deref())?,
            // `minimum_value_promise` is u64 stored in a signed BigInt column; values >= 2^63 wrap to negative.
            minimum_value_promise: self.minimum_value_promise as u64,
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            is_burnt: self.is_burnt,
            is_frozen: self.is_frozen,
            is_on_chain: self.is_on_chain,
            is_condition_spendable: self.is_condition_spendable,
            lock_id: self.locked_by_proof,
            proven_up: self
                .proven_version
                .zip(self.proven_up_epoch)
                .map(|(version, epoch)| -> Result<ProvenUp, WalletStorageError> {
                    let version = u64::try_from(version).map(SubstateVersion::new).map_err(|e| {
                        WalletStorageError::DecodingError {
                            operation: "try_into_output",
                            item: "proven_version",
                            details: format!("Corrupt db: invalid proven_version '{version}': {e}"),
                        }
                    })?;
                    let epoch = u64::try_from(epoch)
                        .map(Epoch)
                        .map_err(|e| WalletStorageError::DecodingError {
                            operation: "try_into_output",
                            item: "proven_up_epoch",
                            details: format!("Corrupt db: invalid proven_up_epoch '{epoch}': {e}"),
                        })?;
                    Ok(ProvenUp { version, epoch })
                })
                .transpose()?,
            spent_version: self
                .spent_version
                .map(|version| {
                    u64::try_from(version)
                        .map(SubstateVersion::new)
                        .map_err(|e| WalletStorageError::DecodingError {
                            operation: "try_into_output",
                            item: "spent_version",
                            details: format!("Corrupt db: invalid spent_version '{version}': {e}"),
                        })
                })
                .transpose()?,
        })
    }
}

impl TryFrom<StealthOutput> for StealthOutputInfo {
    type Error = WalletStorageError;

    fn try_from(value: StealthOutput) -> Result<Self, Self::Error> {
        Ok(StealthOutputInfo {
            resource_address: value
                .resource_address
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info",
                    details: format!("Corrupt db: invalid resource address '{}'", value.resource_address),
                })?,
            public_nonce: RistrettoPublicKeyBytes::from_hex(&value.sender_public_nonce).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info public nonce",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            encrypted_data: EncryptedData::try_from(value.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info encrypted data",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            commitment: deserialize_hex_try_from(&value.commitment).map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output_info",
                item: "output info commitment",
                details: "Corrupt db: invalid hex representation".to_string(),
            })?,
            // `value` is u64 stored in a signed BigInt column; values >= 2^63 wrap to negative.
            value: value.value as u64,
            memo: value.memo_json.as_ref().map(deserialize_json).transpose()?,
            is_on_chain: value.is_on_chain,
        })
    }
}

#[derive(AsChangeset)]
#[diesel(table_name = stealth_outputs)]
pub(crate) struct StealthOutputUpdate<'a> {
    pub status: Option<&'a str>,
    pub is_burnt: Option<bool>,
    pub is_frozen: Option<bool>,
    pub updated_at: dsl::now,
}

#[cfg(test)]
mod tests {
    use tari_ootle_wallet_sdk::models::KeyId;
    use tari_template_lib_types::{ObjectKey, ResourceAddress};
    use time::macros::datetime;

    use super::*;
    use crate::serialization::{serialize_hex, serialize_json};

    fn sample_stealth_output() -> (StealthOutput, ComponentAddress) {
        let owner = ComponentAddress::from_array([0x11; ObjectKey::LENGTH]);
        let resource = ResourceAddress::new(ObjectKey::from_array([0x22; ObjectKey::LENGTH]));
        let key_id = KeyId::imported(1);
        let output = StealthOutput {
            id: 1,
            owner_account_id: 1,
            resource_address: resource.to_string(),
            commitment: serialize_hex([0x33; 32]),
            value: 1000,
            sender_public_nonce: serialize_hex([0x44; 32]),
            status: "Unspent".to_string(),
            locked_at: None,
            locked_by_proof: None,
            view_only_key_id: serialize_json(&key_id).unwrap(),
            owner_key_id: None,
            encrypted_data: vec![0u8; EncryptedData::min_size()],
            tag_byte: 7,
            memo_json: None,
            spend_key: Some(serialize_hex([0x55; 32])),
            condition_root: None,
            minimum_value_promise: 100,
            is_burnt: false,
            is_frozen: false,
            is_on_chain: true,
            is_condition_spendable: true,
            proven_version: Some(2),
            proven_up_epoch: Some(10),
            spent_version: Some(3),
            created_at: datetime!(2026-01-01 00:00:00),
            updated_at: datetime!(2026-01-01 00:00:00),
        };
        (output, owner)
    }

    #[test]
    fn stealth_output_conversions_round_trip_u64_max_amounts_and_reject_negative_non_amount_fields() {
        let (valid, owner) = sample_stealth_output();
        let converted = valid.clone().try_convert(owner).unwrap();
        assert_eq!(converted.value, 1000);
        assert_eq!(converted.tag_byte, UtxoTag::new(7));
        assert_eq!(converted.minimum_value_promise, 100);
        assert_eq!(
            converted.proven_up,
            Some(ProvenUp {
                version: SubstateVersion::new(2),
                epoch: Epoch(10),
            })
        );
        assert_eq!(converted.spent_version, Some(SubstateVersion::new(3)));

        let info = StealthOutputInfo::try_from(valid.clone()).unwrap();
        assert_eq!(info.value, 1000);

        let mut max_amounts = valid.clone();
        max_amounts.value = u64::MAX as i64;
        max_amounts.minimum_value_promise = u64::MAX as i64;
        let converted_max = max_amounts.clone().try_convert(owner).unwrap();
        assert_eq!(converted_max.value, u64::MAX);
        assert_eq!(converted_max.minimum_value_promise, u64::MAX);
        let info_max = StealthOutputInfo::try_from(max_amounts).unwrap();
        assert_eq!(info_max.value, u64::MAX);

        let mut bad_tag = valid.clone();
        bad_tag.tag_byte = -5;
        assert!(matches!(
            bad_tag.try_convert(owner),
            Err(WalletStorageError::DecodingError { item: "tag_byte", .. })
        ));

        let mut bad_proven_version = valid.clone();
        bad_proven_version.proven_version = Some(-1);
        assert!(matches!(
            bad_proven_version.try_convert(owner),
            Err(WalletStorageError::DecodingError {
                item: "proven_version",
                ..
            })
        ));

        let mut bad_proven_epoch = valid.clone();
        bad_proven_epoch.proven_up_epoch = Some(-1);
        assert!(matches!(
            bad_proven_epoch.try_convert(owner),
            Err(WalletStorageError::DecodingError {
                item: "proven_up_epoch",
                ..
            })
        ));

        let mut bad_spent_version = valid;
        bad_spent_version.spent_version = Some(-1);
        assert!(matches!(
            bad_spent_version.try_convert(owner),
            Err(WalletStorageError::DecodingError {
                item: "spent_version",
                ..
            })
        ));
    }
}
