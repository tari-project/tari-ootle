//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use argon2::password_hash::rand_core::RngCore;
use chacha20poly1305::{AeadInPlace, Key, KeyInit, Nonce, Tag};
use rand::rngs::OsRng;
use subtle::ConstantTimeEq;
use tari_crypto::tari_utilities::safe_array::SafeArray;
use zeroize::Zeroizing;

use crate::hashers::{OotleWalletHashDomain, OotleWalletHasher32};

/// The version should be incremented for any breaking change to the format
/// NOTE: Only the most recent version is supported!
/// History:
/// 0: initial version
const ENCRYPTION_VERSION: u8 = 0u8;

const ENCRYPTED_DATA_TAG: &[u8] = b"TARI_WALLET_EXTEND_NONCE_VARIANT";

// Fixed sizes (all in bytes)
const TAG_SIZE: usize = size_of::<Tag>();
const SALT_LENGTH: usize = 5;
const ARGON2_SALT_BYTES: usize = 16;
const ENCRYPTION_KEY_BYTES_LEN: usize = 32;
const MAC_KEY_BYTE_LEN: usize = 32;
const CHECKSUM_LENGTH: usize = size_of::<u32>();
const KEY_BYTES: usize = ENCRYPTION_KEY_BYTES_LEN + MAC_KEY_BYTE_LEN;

pub fn decrypt_with_password(cipher_text: &[u8], passphrase: &[u8]) -> Result<Zeroizing<Vec<u8>>, CipherError> {
    const CIPHERTEXT_MIN_LEN: usize = 1 + MAC_KEY_BYTE_LEN + SALT_LENGTH + TAG_SIZE + CHECKSUM_LENGTH;
    if cipher_text.len() < CIPHERTEXT_MIN_LEN {
        return Err(CipherError("Ciphertext too short".to_string()));
    }

    // We only support one version right now
    let version = *cipher_text.first().expect("cipher_text is not empty");
    if version != ENCRYPTION_VERSION {
        return Err(CipherError(format!(
            "Unsupported ciphertext version: {version}, expected: {}",
            ENCRYPTION_VERSION
        )));
    }

    let len = cipher_text.len();
    // Verify the checksum first, to detect obvious errors
    let (cipher_payload, checksum) = cipher_text
        .split_at_checked(len - CHECKSUM_LENGTH)
        .ok_or_else(|| CipherError("Ciphertext too short (checksum)".to_string()))?;
    let checksum = u32::from_le_bytes(
        copy_fixed_checked(checksum)
            .ok_or_else(|| CipherError(format!("Invalid checksum length {}", checksum.len())))?,
    );
    let expected_checksum = crc32fast::hash(cipher_payload);
    if checksum != expected_checksum {
        return Err(CipherError("Ciphertext checksum mismatch".to_string()));
    }

    // Derive encryption and MAC keys from passphrase and main salt
    let len = cipher_payload.len();
    let (cipher_payload, salt) = cipher_payload
        .split_at_checked(len - SALT_LENGTH)
        .ok_or_else(|| CipherError("Ciphertext too short (salt)".to_string()))?;
    let salt: [u8; SALT_LENGTH] = copy_fixed_checked(salt).expect("Salt length is SALT_LENGTH");
    let key = derive_keys(passphrase, salt.as_slice())?;
    let (encryption_key, mac_key) = key.split_at(ENCRYPTION_KEY_BYTES_LEN);

    // Split off the tag, which is at a fixed position from the end
    let len = cipher_payload.len();
    let (cipher_payload, tag) = cipher_payload
        .split_at_checked(len - TAG_SIZE)
        .ok_or_else(|| CipherError("Ciphertext too short (tag)".to_string()))?;
    let tag = Tag::from_slice(tag);

    // Decrypt the secret data: payload and MAC (without leading version byte)
    let mut decrypted_payload = Zeroizing::new(cipher_payload[1..].to_vec());
    let nonce = encryption_nonce_hasher().chain(&salt).finalize();
    let nonce = nonce
        .as_slice()
        .get(..size_of::<Nonce>())
        .expect("Size of Nonce is greater than 32 bytes");
    let nonce = Nonce::from_slice(nonce);
    decrypt(&mut decrypted_payload, encryption_key, nonce, tag)?;

    // Verify the MAC
    let len = decrypted_payload.len();
    let mac = Zeroizing::new(decrypted_payload.split_off(len - MAC_KEY_BYTE_LEN));

    // Generate the MAC
    let expected_mac = generate_mac(version, &decrypted_payload, salt, mac_key);

    // Verify the MAC in constant time to avoid leaking information
    if mac.ct_eq(&expected_mac).into() {
        Ok(decrypted_payload)
    } else {
        Err(CipherError("Ciphertext MAC mismatch".to_string()))
    }
}

pub fn encrypt_with_password(plain_text: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, CipherError> {
    let mut salt = [0u8; SALT_LENGTH];
    OsRng.fill_bytes(salt.as_mut());
    let key = derive_keys(passphrase, salt.as_slice())?;
    let (encryption_key, mac_key) = key.split_at(ENCRYPTION_KEY_BYTES_LEN);

    // Generate the MAC
    let mac = generate_mac(ENCRYPTION_VERSION, plain_text, salt, mac_key);
    let mut encrypted_buf =
        Vec::with_capacity(1 + plain_text.len() + MAC_KEY_BYTE_LEN + SALT_LENGTH + TAG_SIZE + CHECKSUM_LENGTH);

    // Assemble the secret data to be encrypted: birthday, entropy, MAC
    encrypted_buf.push(ENCRYPTION_VERSION);
    encrypted_buf.extend_from_slice(plain_text);
    encrypted_buf.extend_from_slice(&mac);

    // Derive Nonce from the salt
    let nonce = encryption_nonce_hasher().chain(&salt).finalize();
    let nonce = nonce
        .as_slice()
        .get(..size_of::<Nonce>())
        .expect("Size of Nonce is greater than 32 bytes");
    let nonce = Nonce::from_slice(nonce);

    // Encrypt the secret data
    let tag = encipher(&mut encrypted_buf[1..], encryption_key, nonce)?;

    // Append the tag, salt and checksum
    encrypted_buf.extend_from_slice(tag.as_slice());
    encrypted_buf.extend_from_slice(salt.as_slice());
    let checksum = crc32fast::hash(encrypted_buf.as_slice()).to_le_bytes();
    encrypted_buf.extend_from_slice(&checksum);

    Ok(encrypted_buf)
}

/// Encrypt data using ChaCha20Poly1305 and append the tag
fn encipher(data: &mut [u8], encryption_key: &[u8], nonce: &Nonce) -> Result<Tag, CipherError> {
    // Encrypt the data
    let cipher = chacha20poly1305::ChaCha20Poly1305::new(Key::from_slice(encryption_key));
    let tag = cipher
        .encrypt_in_place_detached(nonce, ENCRYPTED_DATA_TAG, data)
        .map_err(|e| CipherError(format!("Unable to apply stream cipher: {e}")))?;

    Ok(tag)
}

fn decrypt(data: &mut [u8], encryption_key: &[u8], nonce: &Nonce, tag: &Tag) -> Result<(), CipherError> {
    let cipher = chacha20poly1305::ChaCha20Poly1305::new(Key::from_slice(encryption_key));
    cipher
        .decrypt_in_place_detached(nonce, ENCRYPTED_DATA_TAG, data, tag)
        .map_err(|_| CipherError("Unable to decrypt data".to_string()))
}

/// Use Argon2 to derive encryption key (first 32 bytes) and MAC key (last 32 bytes) from a passphrase and main salt
fn derive_keys(passphrase: &[u8], salt: &[u8]) -> Result<SafeArray<u8, KEY_BYTES>, CipherError> {
    // The Argon2 salt is derived from the main salt
    let argon2_salt = encryption_salt_hasher().chain(salt).finalize();
    let argon2_salt = argon2_salt
        .get(..ARGON2_SALT_BYTES)
        .expect("ARGON2_SALT_BYTES < length of 32 byte blake hash");

    // Run Argon2 with enough output to accommodate both keys, so we only run it once
    let mut main_key = SafeArray::<u8, KEY_BYTES>::default();
    // We use the recommended OWASP parameters for this:
    // https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id
    let params = argon2::Params::new(
        46 * 1024, // m-cost = 46 MiB = 46 * 1024 KiB
        1,         // t-cost
        1,         // p-cost
        Some(KEY_BYTES),
    )
    .expect("Incorrect Argon2 parameters");

    // Derive the main key from the password in place
    let hasher = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    hasher
        .hash_password_into(passphrase, argon2_salt, main_key.as_mut())
        .map_err(|_| CipherError("Problem generating Argon2 password hash".to_string()))?;

    Ok(main_key)
}

/// Generate a MAC using Blake2b
fn generate_mac(version: u8, plain_text: &[u8], salt: [u8; SALT_LENGTH], mac_key: &[u8]) -> [u8; MAC_KEY_BYTE_LEN] {
    encryption_mac_hasher()
        .chain(&version)
        .chain(plain_text)
        .chain(&salt)
        .chain(mac_key)
        .finalize()
        .into()
}

pub fn encryption_mac_hasher() -> OotleWalletHasher32<OotleWalletHashDomain> {
    OotleWalletHasher32::new_with_label("encryption_mac")
}
pub fn encryption_nonce_hasher() -> OotleWalletHasher32<OotleWalletHashDomain> {
    OotleWalletHasher32::new_with_label("encryption_nonce")
}
pub fn encryption_salt_hasher() -> OotleWalletHasher32<OotleWalletHashDomain> {
    OotleWalletHasher32::new_with_label("encryption_salt")
}

#[derive(Debug, thiserror::Error)]
#[error("Cipher error: {0}")]
pub struct CipherError(String);

fn copy_fixed_checked<const SZ: usize>(bytes: &[u8]) -> Option<[u8; SZ]> {
    if bytes.len() != SZ {
        return None;
    }
    let mut out = [0u8; SZ];
    out.copy_from_slice(bytes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_encrypts_and_decrypts() {
        let password = b"correct horse battery staple";
        let data = b"The quick brown fox jumps over the lazy dog";

        let encrypted = encrypt_with_password(data, password).expect("encryption failed");
        let decrypted = decrypt_with_password(&encrypted, password).expect("decryption failed");

        assert_eq!(&*decrypted, data);
    }

    #[test]
    fn it_fails_for_invalid_checksum() {
        let password = b"correct horse battery staple";
        let data = b"The quick brown fox jumps over the lazy dog";

        let mut encrypted = encrypt_with_password(data, password).expect("encryption failed");
        // Corrupt the last byte (part of the checksum)
        let last_index = encrypted.len() - 1;
        encrypted[last_index] ^= 0xFF;

        let result = decrypt_with_password(&encrypted, password);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.0, "Ciphertext checksum mismatch");
        }
    }

    #[test]
    fn it_fails_for_invalid_version() {
        let password = b"correct horse battery staple";
        let data = b"The quick brown fox jumps over the lazy dog";

        let mut encrypted = encrypt_with_password(data, password).expect("encryption failed");
        // Corrupt the version byte
        encrypted[0] = 255;

        let result = decrypt_with_password(&encrypted, password);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.0.starts_with("Unsupported ciphertext version"));
        }
    }

    #[test]
    fn it_fails_for_invalid_length() {
        let password = b"correct horse battery staple";
        let data = b"The quick brown fox jumps over the lazy dog";

        let encrypted = encrypt_with_password(data, password).expect("encryption failed");
        // Truncate the encrypted data to make it invalid
        let truncated = &encrypted[..5];

        let result = decrypt_with_password(truncated, password);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.0, "Ciphertext too short");
        }
    }

    #[test]
    fn it_fails_for_corrupted_payload_data() {
        let password = b"correct horse battery staple";
        let data = b"The quick brown fox jumps over the lazy dog";

        let mut encrypted = encrypt_with_password(data, password).expect("encryption failed");
        // Corrupt a byte in the payload
        let salt_start = encrypted.len() - SALT_LENGTH - CHECKSUM_LENGTH - TAG_SIZE;
        encrypted[salt_start] ^= 0xFF;

        let result = decrypt_with_password(&encrypted, password);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.0, "Ciphertext checksum mismatch");
        }
    }
}
