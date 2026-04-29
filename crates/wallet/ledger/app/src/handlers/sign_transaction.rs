//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_ledger_common::arg_types::{GetPublicKeyRequest, GetPublicKeyResponse};

use crate::{crypto::public_key_from_scalar, key_derive::derive_from_bip32_key, state::State, status::AppStatus};

pub fn sign_transaction(
    state_mut: &mut State,
    request: GetPublicKeyRequest,
) -> Result<GetPublicKeyResponse, AppStatus> {
    let k = derive_from_bip32_key(request.account, request.index, request.key_type)?;
    let pk = public_key_from_scalar(&k);

    Ok(GetPublicKeyResponse {
        public_key: pk.compress().0,
    })
}
