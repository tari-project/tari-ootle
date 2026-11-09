//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{models::ConfidentialOutputModel, storage::WalletStorageError};
use tari_template_lib_types::{
    EncryptedData,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
};
use time::PrimitiveDateTime;

use crate::{schema::confidential_outputs, serialization::deserialize_json};

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = confidential_outputs)]
pub struct ConfidentialOutput {
    pub id: i32,
    pub account_id: i32,
    pub vault_id: i32,
    pub commitment: String,
    // TODO: change to a string to allow arbitrary precision
    pub value: i64,
    pub sender_public_nonce: Option<String>,
    pub view_only_key_id: String,
    pub owner_key_id: Option<String>,
    pub public_asset_tag: Option<String>,
    pub memo_json: Option<String>,
    pub status: String,
    pub locked_at: Option<PrimitiveDateTime>,
    pub locked_by_proof: Option<i32>,
    pub encrypted_data: Vec<u8>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl ConfidentialOutput {
    pub(crate) fn try_into_output(
        self,
        account_address_str: &str,
        vault_id: &str,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        Ok(ConfidentialOutputModel {
            account_address: account_address_str
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid account address '{}'", account_address_str),
                })?,
            vault_id: vault_id.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid vault address '{}'", self.vault_id),
            })?,
            commitment: PedersenCommitmentBytes::from_hex(&self.commitment).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            value: (self.value as u64).into(),
            sender_public_nonce: self
                .sender_public_nonce
                .map(|nonce| RistrettoPublicKeyBytes::from_hex(&nonce))
                .transpose()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: "Corrupt db: invalid sender public nonce".to_string(),
                })?,
            view_only_key_id: deserialize_json(&self.view_only_key_id)?,
            owner_key_id: self.owner_key_id.map(|id| deserialize_json(&id)).transpose()?,
            encrypted_data: EncryptedData::try_from(self.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            public_asset_tag: self
                .public_asset_tag
                .map(|tag| RistrettoPublicKeyBytes::from_hex(&tag))
                .transpose()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: "Corrupt db: invalid public asset tag".to_string(),
                })?,
            memo: self.memo_json.as_ref().map(deserialize_json).transpose()?,
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            lock_id: self.locked_by_proof,
        })
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_wallet_sdk::models::KeyId;
    use tari_template_lib_types::{ComponentAddress, ObjectKey, VaultId};
    use time::macros::datetime;

    use super::*;
    use crate::serialization::{serialize_hex, serialize_json};

    fn sample_confidential_output(value: i64) -> (ConfidentialOutput, String, String) {
        let account = ComponentAddress::from_array([0x11; ObjectKey::LENGTH]);
        let vault = VaultId::new(ObjectKey::from_array([0x22; ObjectKey::LENGTH]));
        let key_id = KeyId::imported(1);
        let output = ConfidentialOutput {
            id: 1,
            account_id: 1,
            vault_id: 1,
            commitment: serialize_hex([0x33; 32]),
            value,
            sender_public_nonce: None,
            view_only_key_id: serialize_json(&key_id).unwrap(),
            owner_key_id: None,
            public_asset_tag: None,
            memo_json: None,
            status: "Unspent".to_string(),
            locked_at: None,
            locked_by_proof: None,
            encrypted_data: vec![0u8; EncryptedData::min_size()],
            created_at: datetime!(2026-01-01 00:00:00),
            updated_at: datetime!(2026-01-01 00:00:00),
        };
        (output, account.to_string(), vault.to_string())
    }

    #[test]
    fn try_into_output_round_trips_u64_max_value() {
        let (valid, account, vault) = sample_confidential_output(500);
        let model = valid.try_into_output(&account, &vault).unwrap();
        assert_eq!(model.value, 500u64);

        let (max_val, account, vault) = sample_confidential_output(u64::MAX as i64);
        let model = max_val.try_into_output(&account, &vault).unwrap();
        assert_eq!(model.value, u64::MAX);
    }
}
