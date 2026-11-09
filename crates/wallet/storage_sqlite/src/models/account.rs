//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_ootle_common_types::Epoch;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use time::PrimitiveDateTime;

use crate::{
    schema::accounts,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = accounts)]
pub struct Account {
    pub id: i32,
    pub name: Option<String>,
    pub address: String,
    pub owner_public_key: String,
    pub view_only_key_id: String,
    pub owner_key_id: Option<String>,
    pub birthday_epoch: i64,
    pub is_default: bool,
    pub is_confirmed_on_chain: bool,
    pub _stealth_resource_address: String,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl Account {
    pub(crate) fn try_convert(self) -> Result<tari_ootle_wallet_sdk::models::Account, WalletStorageError> {
        let birthday_epoch = u64::try_from(self.birthday_epoch).map(Epoch).map_err(|e| {
            WalletStorageError::DecodingError {
                operation: "Account::try_convert",
                item: "birthday_epoch",
                details: format!("Invalid birthday_epoch {}: {e}", self.birthday_epoch),
            }
        })?;

        Ok(tari_ootle_wallet_sdk::models::Account {
            name: self.name,
            component_address: self.address.parse().map_err(|e| WalletStorageError::DecodingError {
                operation: "Account::try_convert",
                item: "address",
                details: format!("Invalid address: {}: {e}", self.address),
            })?,
            owner_key_id: self.owner_key_id.as_ref().map(deserialize_json).transpose()?,
            view_only_key_id: deserialize_json(&self.view_only_key_id)?,
            owner_public_key: deserialize_hex_try_from(&self.owner_public_key)?,
            birthday_epoch,
            is_confirmed_on_chain: self.is_confirmed_on_chain,
            is_default: self.is_default,
        })
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_wallet_sdk::models::KeyId;
    use tari_template_lib_types::{ComponentAddress, ObjectKey, crypto::RistrettoPublicKeyBytes};
    use time::macros::datetime;

    use super::*;
    use crate::serialization::{serialize_hex, serialize_json};

    fn sample_account(birthday_epoch: i64) -> Account {
        let address = ComponentAddress::from_array([0x22; ObjectKey::LENGTH]);
        let pk = RistrettoPublicKeyBytes::from([0x33; 32]);
        let key_id = KeyId::imported(0);
        Account {
            id: 1,
            name: Some("main".to_string()),
            address: address.to_string(),
            owner_public_key: serialize_hex(pk),
            view_only_key_id: serialize_json(&key_id).unwrap(),
            owner_key_id: Some(serialize_json(&key_id).unwrap()),
            birthday_epoch,
            is_default: true,
            is_confirmed_on_chain: true,
            _stealth_resource_address: String::new(),
            created_at: datetime!(2026-01-01 00:00:00),
            updated_at: datetime!(2026-01-01 00:00:00),
        }
    }

    #[test]
    fn try_convert_rejects_negative_birthday_epoch() {
        let converted = sample_account(42).try_convert().unwrap();
        assert_eq!(converted.birthday_epoch, Epoch(42));

        let err = sample_account(-1).try_convert().unwrap_err();
        assert!(matches!(
            err,
            WalletStorageError::DecodingError {
                item: "birthday_epoch",
                ..
            }
        ));
    }
}
