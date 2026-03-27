//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;

/// A trait for types that own a view-only key secret.
/// The view-only secret allows UTXOs to be decrypted but not spent.
pub trait HasViewOnlyKeySecret {
    fn view_only_secret(&self) -> &RistrettoSecretKey;
}
