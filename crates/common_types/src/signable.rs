//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSchnorr};

pub trait Signable<Ctx = ()> {
    type MessageOutput: AsRef<[u8]>;

    fn to_signing_message(&self, context: Ctx) -> Self::MessageOutput;
}

pub trait IntoSigned<Ctx = ()>: Signable<Ctx> {
    type SignedOutput;

    fn into_signed(self, public_key: RistrettoPublicKey, signature: RistrettoSchnorr) -> Self::SignedOutput;
}
