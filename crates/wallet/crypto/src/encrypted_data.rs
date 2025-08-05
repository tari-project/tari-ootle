//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use chacha20poly1305::{
    aead,
    aead::{generic_array::GenericArray, OsRng},
    consts::U32,
    AeadCore,
    AeadInPlace,
    KeyInit,
    Tag,
    XChaCha20Poly1305,
    XNonce,
};
use digest::FixedOutput;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    hashing::DomainSeparatedHasher,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{crypto::get_commitment_factory, ToByteType};
use tari_hashing::TransactionSecureNonceKdfDomain;
use tari_template_lib::{models::EncryptedData, prelude::PedersenCommitmentBytes};
use tari_utilities::{safe_array::SafeArray, ByteArray};
use zeroize::{Zeroize, Zeroizing};

use crate::{kdfs, kdfs::EncryptedDataKey, MaskAndValue, WalletCryptoError};

pub fn unblind_output(
    output_commitment: &PedersenCommitmentBytes,
    output_encrypted_value: &EncryptedData,
    claim_secret: &RistrettoSecretKey,
    reciprocal_public_key: &RistrettoPublicKey,
) -> Result<MaskAndValue, WalletCryptoError> {
    let encryption_key = kdfs::encrypted_data_dh_kdf_aead(claim_secret, reciprocal_public_key);

    let (value, mask) = extract_value_and_mask(&encryption_key, output_commitment, output_encrypted_value)?;
    let commitment = get_commitment_factory().commit_value(&mask, value);
    if output_commitment.as_bytes() == commitment.as_bytes() {
        Ok(MaskAndValue {
            value: value.into(),
            mask,
        })
    } else {
        Err(WalletCryptoError::UnableToOpenCommitment)
    }
}

pub fn encrypt_value_and_mask(
    amount: u64,
    mask: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
    secret: &RistrettoSecretKey,
) -> Result<EncryptedData, WalletCryptoError> {
    let key = kdfs::encrypted_data_dh_kdf_aead(secret, public_nonce);
    let commitment = get_commitment_factory().commit_value(mask, amount).to_byte_type();
    let encrypted_data = encrypt_data(&key, &commitment, amount, mask)?;
    Ok(encrypted_data)
}

pub fn extract_value_and_mask(
    encryption_key: &RistrettoSecretKey,
    commitment: &PedersenCommitmentBytes,
    encrypted_data: &EncryptedData,
) -> Result<(u64, RistrettoSecretKey), WalletCryptoError> {
    let (value, mask) = decrypt_data_and_mask(encryption_key, commitment, encrypted_data)
        .map_err(|e| WalletCryptoError::FailedDecryptData { details: e.to_string() })?;
    Ok((value, mask))
}

fn inner_encrypted_data_kdf_aead(
    encryption_key: &RistrettoSecretKey,
    commitment: &PedersenCommitmentBytes,
) -> EncryptedDataKey {
    let mut aead_key = EncryptedDataKey::from(SafeArray::default());
    DomainSeparatedHasher::<Blake2b<U32>, TransactionSecureNonceKdfDomain>::new_with_label("encrypted_value_and_mask")
        .chain(encryption_key.as_bytes())
        .chain(commitment.as_bytes())
        .finalize_into(GenericArray::from_mut_slice(aead_key.reveal_mut()));
    aead_key
}

const ENCRYPTED_DATA_TAG: &[u8] = b"TARI_AAD_VALUE_AND_MASK_EXTEND_NONCE_VARIANT";

pub(crate) fn encrypt_data(
    encryption_key: &RistrettoSecretKey,
    commitment: &PedersenCommitmentBytes,
    value: u64,
    mask: &RistrettoSecretKey,
) -> Result<EncryptedData, aead::Error> {
    fn payload_slice_mut(bytes: &mut [u8]) -> &mut [u8] {
        bytes
            .get_mut(EncryptedData::payload_offset()..)
            .expect("invariant violation: bytes length is less than payload_offset")
    }

    fn tag_slice_mut(bytes: &mut [u8]) -> &mut [u8] {
        bytes
            .get_mut(..EncryptedData::SIZE_TAG)
            .expect("invariant violation: tag length is less than payload_offset")
    }

    fn nonce_slice_mut(bytes: &mut [u8]) -> &mut [u8] {
        bytes
            .get_mut(EncryptedData::SIZE_TAG..EncryptedData::SIZE_TAG + EncryptedData::SIZE_NONCE)
            .expect("invariant violation: nonce length is less than payload_offset")
    }

    // Produce a secure random nonce and the AEAD
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let aead_key = inner_encrypted_data_kdf_aead(encryption_key, commitment);
    let cipher = XChaCha20Poly1305::new(GenericArray::from_slice(aead_key.reveal()));

    // Encode the value and mask
    let mut bytes = vec![0; EncryptedData::min_size()];
    let payload_mut = payload_slice_mut(&mut bytes);
    payload_mut
        .get_mut(..EncryptedData::SIZE_VALUE)
        .unwrap()
        .copy_from_slice(value.to_le_bytes().as_ref());
    payload_mut
        .get_mut(EncryptedData::SIZE_VALUE..EncryptedData::SIZE_VALUE + EncryptedData::SIZE_MASK)
        .unwrap()
        .copy_from_slice(mask.as_bytes());
    // Encrypt in place
    match cipher.encrypt_in_place_detached(&nonce, ENCRYPTED_DATA_TAG, payload_mut) {
        Ok(tag) => {
            tag_slice_mut(&mut bytes).copy_from_slice(&tag);
            nonce_slice_mut(&mut bytes).copy_from_slice(&nonce);

            Ok(EncryptedData::try_from(bytes).expect("bytes length == EncryptedData::min_size()"))
        },
        Err(err) => {
            bytes.zeroize();
            Err(err)
        },
    }
}

pub fn decrypt_data_and_mask(
    encryption_key: &RistrettoSecretKey,
    commitment: &PedersenCommitmentBytes,
    encrypted_data: &EncryptedData,
) -> Result<(u64, RistrettoSecretKey), aead::Error> {
    // Extract the tag, nonce, and ciphertext
    let tag = Tag::from_slice(encrypted_data.tag_slice().ok_or(aead::Error)?);
    let nonce = XNonce::from_slice(encrypted_data.nonce_slice().ok_or(aead::Error)?);
    let mut bytes = Zeroizing::new(encrypted_data.payload_slice().ok_or(aead::Error)?.to_vec());

    // Set up the AEAD
    let aead_key = inner_encrypted_data_kdf_aead(encryption_key, commitment);
    let cipher = XChaCha20Poly1305::new(GenericArray::from_slice(aead_key.reveal()));

    // Decrypt in place
    cipher.decrypt_in_place_detached(nonce, ENCRYPTED_DATA_TAG, bytes.as_mut_slice(), tag)?;

    // Decode the value and mask
    let mut value_bytes = [0u8; EncryptedData::SIZE_VALUE];
    value_bytes.copy_from_slice(bytes.get(..EncryptedData::SIZE_VALUE).ok_or(aead::Error)?);
    Ok((
        u64::from_le_bytes(value_bytes),
        RistrettoSecretKey::from_canonical_bytes(
            bytes
                .get(EncryptedData::SIZE_VALUE..EncryptedData::SIZE_VALUE + EncryptedData::SIZE_MASK)
                .ok_or(aead::Error)?,
        )
        .expect("The length of bytes is exactly SIZE_MASK"),
    ))
}

#[cfg(test)]
mod tests {
    use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_engine_types::{crypto::get_commitment_factory, ToByteType};

    use super::*;

    #[test]
    fn it_encrypts_and_decrypts() {
        let key = RistrettoSecretKey::random(&mut OsRng);
        let amount = 100;
        let commitment = get_commitment_factory().commit_value(&key, amount).to_byte_type();
        let mask = RistrettoSecretKey::random(&mut OsRng);
        let encrypted = encrypt_data(&key, &commitment, amount, &mask).unwrap();

        let val = decrypt_data_and_mask(&key, &commitment, &encrypted).unwrap();
        assert_eq!(val.0, 100);
    }
}
