// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use webauthn_rs::prelude::Passkey;

#[derive(Debug, Clone)]
pub struct WebauthnRegistrationModel {
    pub id: u32,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct WebauthnRegistrationPasskeyModel {
    pub passkey: Passkey,
}
