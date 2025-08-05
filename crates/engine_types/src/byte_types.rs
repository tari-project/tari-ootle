//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    hashing::DomainSeparation,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey},
    signatures::{CommitmentSignature, SchnorrSignature},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_template_lib::{
    prelude::Scalar32Bytes,
    types::crypto::{
        CommitmentSignatureBytes,
        PedersenCommitmentBytes,
        RistrettoPublicKeyBytes,
        SchnorrSignatureBytes,
    },
};

/// Defines a conversion from a type to its light-weight byte representation.
pub trait ToByteType {
    type ByteType;
    fn to_byte_type(&self) -> Self::ByteType;
}

pub trait FromByteType<T> {
    type Error;

    fn try_from_byte_type(bytes: &T) -> Result<Self, Self::Error>
    where Self: Sized;
}

impl ToByteType for RistrettoPublicKey {
    type ByteType = RistrettoPublicKeyBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        RistrettoPublicKeyBytes::from_bytes(self.as_bytes()).expect(
            "PublicKey alias is not a valid RistrettoPublicKeyBytes. This can only happen if the byte length of \
             PublicKey is not size_of::<RistrettoPublicKeyBytes>() bytes.",
        )
    }
}

impl FromByteType<RistrettoPublicKeyBytes> for RistrettoPublicKey {
    type Error = tari_utilities::ByteArrayError;

    fn try_from_byte_type(bytes: &RistrettoPublicKeyBytes) -> Result<Self, Self::Error> {
        RistrettoPublicKey::from_canonical_bytes(bytes.as_bytes())
    }
}

impl ToByteType for PedersenCommitment {
    type ByteType = PedersenCommitmentBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        PedersenCommitmentBytes::from_bytes(self.as_bytes())
            .expect("byte size of PedersenCommitment is not size_of::<PedersenCommitmentBytes>() bytes.")
    }
}

impl FromByteType<PedersenCommitmentBytes> for PedersenCommitment {
    type Error = tari_utilities::ByteArrayError;

    fn try_from_byte_type(bytes: &PedersenCommitmentBytes) -> Result<Self, Self::Error> {
        PedersenCommitment::from_canonical_bytes(bytes.as_bytes())
    }
}

impl<H: DomainSeparation> ToByteType for SchnorrSignature<RistrettoPublicKey, RistrettoSecretKey, H> {
    type ByteType = SchnorrSignatureBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        SchnorrSignatureBytes::new(
            self.get_public_nonce().to_byte_type(),
            Scalar32Bytes::from_bytes(self.get_signature().as_bytes())
                .expect("byte size of RistrettoSecretKey is not size_of::<Scalar32Bytes>() bytes."),
        )
    }
}

impl<H: DomainSeparation> FromByteType<SchnorrSignatureBytes>
    for SchnorrSignature<RistrettoPublicKey, RistrettoSecretKey, H>
{
    type Error = tari_utilities::ByteArrayError;

    fn try_from_byte_type(schnorr_bytes: &SchnorrSignatureBytes) -> Result<Self, Self::Error> {
        let public_nonce = RistrettoPublicKey::try_from_byte_type(schnorr_bytes.public_nonce())?;
        let signature = RistrettoSecretKey::from_canonical_bytes(schnorr_bytes.signature().as_bytes())?;
        Ok(SchnorrSignature::new(public_nonce, signature))
    }
}

impl ToByteType for CommitmentSignature<RistrettoPublicKey, RistrettoSecretKey> {
    type ByteType = CommitmentSignatureBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        CommitmentSignatureBytes::try_from_parts(
            self.public_nonce().as_bytes(),
            self.u().as_bytes(),
            self.v().as_bytes(),
        )
        .expect("byte size of CommitmentSignature is not size_of::<CommitmentSignatureBytes>() bytes.")
    }
}

impl FromByteType<CommitmentSignatureBytes> for CommitmentSignature<RistrettoPublicKey, RistrettoSecretKey> {
    type Error = tari_utilities::ByteArrayError;

    fn try_from_byte_type(bytes: &CommitmentSignatureBytes) -> Result<Self, Self::Error> {
        let public_nonce = PedersenCommitment::try_from_byte_type(bytes.public_nonce())?;
        let signature = RistrettoSecretKey::from_canonical_bytes(bytes.u().as_bytes())?;
        let v = RistrettoSecretKey::from_canonical_bytes(bytes.v().as_bytes())?;
        Ok(CommitmentSignature::new(public_nonce, signature, v))
    }
}

impl<T: ToByteType> ToByteType for Option<T> {
    type ByteType = Option<T::ByteType>;

    fn to_byte_type(&self) -> Self::ByteType {
        self.as_ref().map(|v| v.to_byte_type())
    }
}
