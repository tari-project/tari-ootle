// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::schema::webauthn_registrations::dsl::webauthn_registrations;
use tari_dan_wallet_sdk::models::WebauthnRegistrationModel;
use webauthn_rs::prelude::Passkey;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = webauthn_registrations)]
pub struct WebauthnRegistration {
    pub id: i32,
    pub username: String,
    pub passkey: Vec<u8>,
}

impl TryFrom<WebauthnRegistration> for WebauthnRegistrationModel {
    type Error = serde_json::Error;

    fn try_from(reg: WebauthnRegistration) -> Result<Self, Self::Error> {
        let passkey: Passkey = serde_json::from_slice(reg.passkey.as_slice())?;
        Ok(
            Self {
                username: reg.username,
                passkey,
            }
        )
    }
}