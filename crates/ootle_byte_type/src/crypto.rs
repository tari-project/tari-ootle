//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    compressed_commitment::CompressedCommitment,
    compressed_key::CompressedKey,
    hashing::DomainSeparation,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment},
    signatures::{CommitmentSignature, CompressedSchnorrSignature, SchnorrSignature},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::{
    CommitmentSignatureBytes,
    PedersenCommitmentBytes,
    RistrettoPublicKeyBytes,
    Scalar32Bytes,
    SchnorrSignatureBytes,
};

use crate::{ConvertFromByteType, ToByteType};

impl ToByteType for RistrettoPublicKey {
    type ByteType = RistrettoPublicKeyBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        RistrettoPublicKeyBytes::from_bytes(self.as_bytes()).expect(
            "PublicKey alias is not a valid RistrettoPublicKeyBytes. This can only happen if the byte length of \
             PublicKey is not size_of::<RistrettoPublicKeyBytes>() bytes.",
        )
    }
}

impl ConvertFromByteType<RistrettoPublicKeyBytes> for RistrettoPublicKey {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &RistrettoPublicKeyBytes) -> Result<Self, Self::Error> {
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

impl ConvertFromByteType<PedersenCommitmentBytes> for PedersenCommitment {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &PedersenCommitmentBytes) -> Result<Self, Self::Error> {
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

impl<H: DomainSeparation> ConvertFromByteType<SchnorrSignatureBytes>
    for SchnorrSignature<RistrettoPublicKey, RistrettoSecretKey, H>
{
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(schnorr_bytes: &SchnorrSignatureBytes) -> Result<Self, Self::Error> {
        let public_nonce = RistrettoPublicKey::convert_from_byte_type(schnorr_bytes.public_nonce())?;
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

impl ConvertFromByteType<CommitmentSignatureBytes> for CommitmentSignature<RistrettoPublicKey, RistrettoSecretKey> {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &CommitmentSignatureBytes) -> Result<Self, Self::Error> {
        let public_nonce = PedersenCommitment::convert_from_byte_type(bytes.public_nonce())?;
        let signature = RistrettoSecretKey::from_canonical_bytes(bytes.u().as_bytes())?;
        let v = RistrettoSecretKey::from_canonical_bytes(bytes.v().as_bytes())?;
        Ok(CommitmentSignature::new(public_nonce, signature, v))
    }
}

impl ToByteType for CompressedKey<RistrettoPublicKey> {
    type ByteType = RistrettoPublicKeyBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        // PANIC: CompressedKey protects the size invariant (incl serde, borsh)
        self.as_bytes()
            .try_into()
            .expect("byte size of CompressedKeyBytes invariant")
    }
}

impl ConvertFromByteType<RistrettoPublicKeyBytes> for CompressedKey<RistrettoPublicKey> {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &RistrettoPublicKeyBytes) -> Result<Self, Self::Error> {
        Self::from_canonical_bytes(bytes.as_bytes())
    }
}

impl ToByteType for CompressedCommitment<RistrettoPublicKey> {
    type ByteType = PedersenCommitmentBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        // PANIC: CompressedCommitment protects the size invariant
        self.as_bytes()
            .try_into()
            .expect("byte size of CompressedCommitmentBytes invariant")
    }
}

impl ConvertFromByteType<PedersenCommitmentBytes> for CompressedCommitment<RistrettoPublicKey> {
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(bytes: &PedersenCommitmentBytes) -> Result<Self, Self::Error> {
        Self::from_canonical_bytes(bytes.as_bytes())
    }
}

impl<H: DomainSeparation> ToByteType for CompressedSchnorrSignature<RistrettoPublicKey, RistrettoSecretKey, H> {
    type ByteType = SchnorrSignatureBytes;

    fn to_byte_type(&self) -> Self::ByteType {
        SchnorrSignatureBytes::new(
            self.get_compressed_public_nonce().to_byte_type(),
            Scalar32Bytes::from_bytes(self.get_signature().as_bytes())
                .expect("byte size of RistrettoSecretKey is not size_of::<Scalar32Bytes>() bytes."),
        )
    }
}

impl<H: DomainSeparation> ConvertFromByteType<SchnorrSignatureBytes>
    for CompressedSchnorrSignature<RistrettoPublicKey, RistrettoSecretKey, H>
{
    type Error = tari_utilities::ByteArrayError;

    fn convert_from_byte_type(schnorr_bytes: &SchnorrSignatureBytes) -> Result<Self, Self::Error> {
        let public_nonce = CompressedKey::convert_from_byte_type(schnorr_bytes.public_nonce())?;
        let signature = RistrettoSecretKey::from_canonical_bytes(schnorr_bytes.signature().as_bytes())?;
        Ok(CompressedSchnorrSignature::new(public_nonce, signature))
    }
}
