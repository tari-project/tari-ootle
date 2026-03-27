//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub trait Signable<Ctx = ()> {
    type MessageOutput: AsRef<[u8]>;
    type Signature;

    fn to_signing_message(&self, context: Ctx) -> Self::MessageOutput;
}

pub trait IntoSigned<Ctx = ()>: Signable<Ctx> {
    type SignedOutput;

    fn into_signed(self, sig: <Self as Signable<Ctx>>::Signature) -> Self::SignedOutput;
}
