// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::schema::webauthn_registration_passkeys;
use crate::schema::webauthn_registrations;
use chrono::NaiveDateTime;
use tari_dan_wallet_sdk::models::{WebauthnRegistrationModel, WebauthnRegistrationPasskeyModel};
use webauthn_rs::prelude::Passkey;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = webauthn_registrations)]
pub struct WebauthnRegistration {
    pub id: i32,
    pub username: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<WebauthnRegistration> for WebauthnRegistrationModel {

    fn from(reg: WebauthnRegistration) -> Self {
        Self {
            id: reg.id as u32,
            username: reg.username,
        }
    }
}

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = webauthn_registration_passkeys)]
pub struct WebauthnRegistrationPasskey {
    pub id: i32,
    pub registration_id: i32,
    pub passkey: Vec<u8>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl TryFrom<WebauthnRegistrationPasskey> for WebauthnRegistrationPasskeyModel {
    type Error = serde_json::Error;

    fn try_from(value: WebauthnRegistrationPasskey) -> Result<Self, Self::Error> {
        let passkey: Passkey = serde_json::from_slice(&value.passkey)?;
        Ok(WebauthnRegistrationPasskeyModel{
            passkey,
        })
    }
}