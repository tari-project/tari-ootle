// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use webauthn_rs_proto::PublicKeyCredentialCreationOptions;

// TODO: continue impl for in memory webauthn registration store
pub struct WebauthnRegistrationService {
    registrations: Arc<RwLock<HashMap<String, PublicKeyCredentialCreationOptions>>>,
}