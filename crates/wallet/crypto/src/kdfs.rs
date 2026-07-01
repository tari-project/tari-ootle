//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::Key;
use ootle_network::Network;
use tari_crypto::{
    dhke::DiffieHellmanSharedSecret,
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::base_layer_hashing::encrypted_data_hasher;
use tari_template_lib_types::{ResourceAddress, crypto::UtxoTag};
use tari_utilities::{Hidden, hidden_type, safe_array::SafeArray};
use zeroize::Zeroize;

use crate::{
    hashers::{KdfHasher, stealth_output_tag_hasher64, stealth_owner_hasher64, stealth_spend_auth_hasher64},
    safe_key::SafeAeadKey,
};

pub(crate) const AEAD_KEY_LEN: usize = size_of::<Key>();

// Type for hiding aead key encryption
hidden_type!(EncryptedDataKey, SafeArray<u8, AEAD_KEY_LEN>);
pub type SafeKey64 = SafeAeadKey<64>;

fn dh(
    public_key: &RistrettoPublicKey,
    private_key: &RistrettoSecretKey,
) -> DiffieHellmanSharedSecret<RistrettoPublicKey> {
    DiffieHellmanSharedSecret::<RistrettoPublicKey>::new(private_key, public_key)
}

/// Generate a decryption key from a private key and nonce
pub fn encrypted_data_dh_kdf_aead(
    private_key: &RistrettoSecretKey,
    public_key: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    // Must match base layer burn
    let hasher = encrypted_data_hasher();
    dh_kdf_aead(hasher, private_key, public_key)
}

/// Generate a Diffie-Hellman secret key `H(DH(s1, s2*G))`
pub fn dh_kdf_aead<H>(
    hasher: H,
    private_key: &RistrettoSecretKey,
    public_key: &RistrettoPublicKey,
) -> RistrettoSecretKey
where
    H: KdfHasher<[u8]>,
    H::HashOutput: AsRef<[u8]>,
{
    let shared_secret = dh(public_key, private_key);
    let hash = hasher.kdf_digest(shared_secret.as_bytes());
    RistrettoSecretKey::from_uniform_bytes(hash.as_ref()).unwrap()
}

/// Generate a secret key for the owner key from a private key and nonce
pub fn owner_stealth_dh_secret(
    network: Network,
    private_key: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    // c = H(r.G * k)
    let c = stealth_owner_dh(network, public_nonce, private_key);
    // c + k
    c + private_key
}

/// L1-compatible derivation of the stealth claim secret `s = H(p·R) + p` for an L1 burn claim.
///
/// `H` mirrors the L1 wallet's `diffie_hellman_stealth_domain_hasher`:
/// `WalletHasher::new_with_label("stealth_address")` over the raw 32-byte compressed DH product.
/// This MUST stay byte-identical to the L1 derivation — see
/// `tari/base_layer/transaction_components/src/transaction_components/one_sided.rs` and
/// `key_manager/manager.rs::compute_stealth_claim_public_key`.
pub fn burn_claim_stealth_secret(
    account_secret: &RistrettoSecretKey,
    sender_offset_public_key: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    let shared_secret = dh(sender_offset_public_key, account_secret);
    let hash = tari_hashing::WalletHasher::new_with_label("stealth_address")
        .chain(shared_secret.as_bytes())
        .finalize();
    let scalar = RistrettoSecretKey::from_uniform_bytes(hash.as_ref())
        .expect("Blake2b<U64> produces 64 bytes which is valid uniform input");
    scalar + account_secret
}

fn stealth_owner_dh(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
) -> RistrettoSecretKey {
    let hasher = stealth_owner_hasher64(network);
    dh_kdf_aead(hasher, secret_nonce, public_key)
}

pub fn owner_stealth_dh_stealth_address(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
) -> RistrettoPublicKey {
    // c = H(k.G * r)
    let c = stealth_owner_dh(network, public_key, secret_nonce);
    // C = c.G
    let c_g = RistrettoPublicKey::from_secret_key(&c);
    // c.G + k.G
    c_g + public_key
}

fn stealth_spend_auth_dh(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
    index: u32,
) -> RistrettoSecretKey {
    let mut hasher = stealth_spend_auth_hasher64(network);
    hasher.update_consensus_encode(&index);
    dh_kdf_aead(hasher, secret_nonce, public_key)
}

/// Derives the recovering party's one-time spend-authorization secret `p = H(s·R) + s` for co-signer
/// `index`, from the party's `account_secret` and the output's `public_nonce` (`R`). The matching
/// public key ([`spend_auth_dh_public_key`]) is what a funder commits inside a script-path
/// `AccessRule` leaf; only the holder of `account_secret` can recover `p` and authorize the spend.
/// `index` domain-separates co-signers so several independent keys hang off one output nonce (e.g. a
/// 2-of-2 swap). Distinct from [`owner_stealth_dh_secret`] so a spend-authorization key can never
/// collide with the value-owner key.
pub fn spend_auth_dh_secret(
    network: Network,
    account_secret: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
    index: u32,
) -> RistrettoSecretKey {
    // c = H(s·R)
    let c = stealth_spend_auth_dh(network, public_nonce, account_secret, index);
    // p = c + s
    c + account_secret
}

/// Derives the one-time spend-authorization public key `P = H(r·S)·G + S` for co-signer `index`,
/// which a funder computes from the party's account public key `S` (`public_key`) and the output's
/// secret nonce `r` (`R = r·G`), then commits inside a script-path `AccessRule` leaf. The party later
/// recovers the matching secret with [`spend_auth_dh_secret`].
pub fn spend_auth_dh_public_key(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
    index: u32,
) -> RistrettoPublicKey {
    // c = H(r·S)
    let c = stealth_spend_auth_dh(network, public_key, secret_nonce, index);
    // P = c.G + S
    let c_g = RistrettoPublicKey::from_secret_key(&c);
    c_g + public_key
}

pub fn utxo_tag_stealth_dh(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
    resource_address: &ResourceAddress,
) -> UtxoTag {
    let shared_secret = dh(public_key, secret_nonce);
    let result = stealth_output_tag_hasher64(network)
        .chain(shared_secret.as_bytes())
        .chain(resource_address)
        .finalize();

    let mut buf = [0u8; size_of::<u32>()];
    buf.copy_from_slice(&result[..size_of::<u32>()]);
    let tag = u32::from_le_bytes(buf);
    UtxoTag::new(tag)
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::crypto::create_key_pair;

    use super::*;

    #[test]
    fn it_generates_the_correct_private_stealth_address() {
        let network = Network::LocalNet;
        let (secret_key, public_key) = create_key_pair();
        let (secret_nonce, public_nonce) = create_key_pair();

        let stealth_address = owner_stealth_dh_stealth_address(network, &public_key, &secret_nonce);
        let stealth_secret = owner_stealth_dh_secret(network, &secret_key, &public_nonce);
        let expected_stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
        assert_eq!(stealth_address, expected_stealth_address);
    }

    #[test]
    fn it_does_not_produce_the_same_secret_when_switching_params() {
        let network = Network::LocalNet;
        let (secret_key, public_key) = create_key_pair();
        let (secret_nonce, public_nonce) = create_key_pair();

        let stealth_address1 = owner_stealth_dh_stealth_address(network, &public_key, &secret_nonce);
        let stealth_address2 = owner_stealth_dh_stealth_address(network, &public_nonce, &secret_key);

        // c + k.G != c + r.G
        // Just makes this fact clear if it isn't obvious
        assert_ne!(stealth_address1, stealth_address2);
    }

    #[test]
    fn spend_auth_public_matches_the_recovered_secret() {
        let network = Network::LocalNet;
        // `S` is the party's account key; `R = r.G` is the output nonce a funder chooses.
        let (account_secret, account_public) = create_key_pair();
        let (secret_nonce, public_nonce) = create_key_pair();

        // Funder commits P; party recovers p; they must agree that P == p.G.
        let public = spend_auth_dh_public_key(network, &account_public, &secret_nonce, 0);
        let secret = spend_auth_dh_secret(network, &account_secret, &public_nonce, 0);
        assert_eq!(public, RistrettoPublicKey::from_secret_key(&secret));
    }

    #[test]
    fn spend_auth_keys_are_independent_per_index() {
        let network = Network::LocalNet;
        let (account_secret, _) = create_key_pair();
        let (_, public_nonce) = create_key_pair();

        let signer_0 = spend_auth_dh_secret(network, &account_secret, &public_nonce, 0);
        let signer_1 = spend_auth_dh_secret(network, &account_secret, &public_nonce, 1);
        assert_ne!(signer_0, signer_1);
    }

    #[test]
    fn spend_auth_is_a_distinct_domain_from_the_owner_key() {
        let network = Network::LocalNet;
        let (account_secret, _) = create_key_pair();
        let (_, public_nonce) = create_key_pair();

        let owner = owner_stealth_dh_secret(network, &account_secret, &public_nonce);
        let spend_auth = spend_auth_dh_secret(network, &account_secret, &public_nonce, 0);
        assert_ne!(owner, spend_auth);
    }
}
