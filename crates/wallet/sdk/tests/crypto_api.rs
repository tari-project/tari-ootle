//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use tari_crypto::{commitment::HomomorphicCommitmentFactory, keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::{crypto::get_commitment_factory, ToByteType};
use tari_ootle_common_types::Network;
use tari_ootle_wallet_sdk::apis::stealth_crypto::StealthCryptoApi;

use crate::support::{random_key, random_keypair, resource_address_from_seed};

mod utxo_tag {
    use super::*;

    #[test]
    fn it_generates_the_same_tag_on_the_sender_and_recipient() {
        let api = StealthCryptoApi;
        let (k1, p1) = random_keypair();
        let (k2, p2) = random_keypair();
        let resx = resource_address_from_seed(123);
        let sender_tag_for_recipient = api.derive_stealth_output_tag(Network::Esmeralda, &k1, &p2, &resx);
        let receipient_tag_from_sender = api.derive_stealth_output_tag(Network::Esmeralda, &k2, &p1, &resx);

        assert_eq!(sender_tag_for_recipient, receipient_tag_from_sender);
    }
}

mod stealth_address {
    use super::*;

    #[test]
    fn it_generates_the_correct_private_stealth_address() {
        let api = StealthCryptoApi;
        let (k1, p1) = random_keypair();
        let (k2, p2) = random_keypair();

        let stealth_address = api.derive_stealth_owner_public_key(Network::Esmeralda, &p1, &k2);
        let stealth_secret = api.derive_stealth_owner_secret(Network::Esmeralda, &k1, &p2);
        let expected_stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
        assert_eq!(stealth_address, expected_stealth_address);
    }

    #[test]
    fn it_does_not_produce_the_same_secret_when_switching_params() {
        let api = StealthCryptoApi;
        let (k1, p1) = random_keypair();
        let (k2, p2) = random_keypair();

        let stealth_address1 = api.derive_stealth_owner_public_key(Network::Esmeralda, &p1, &k2);
        let stealth_address2 = api.derive_stealth_owner_public_key(Network::Esmeralda, &p2, &k1);
        assert_ne!(stealth_address1, stealth_address2);
    }
}

mod encrypted_data {

    use super::*;

    #[test]
    fn it_derives_the_same_encrypted_data_key_for_sender_and_recipient() {
        let api = StealthCryptoApi;
        let (k1, p1) = random_keypair();
        let (k2, p2) = random_keypair();

        let sender_key = api.derive_encrypted_data_key(&p2, &k1);
        let recipient_key = api.derive_encrypted_data_key(&p1, &k2);
        assert_eq!(sender_key, recipient_key);
    }

    #[test]
    fn it_encrypts_then_decrypts() {
        let api = StealthCryptoApi;
        let amount = 123456u64;
        let mask = random_key();
        let (k1, p1) = random_keypair();
        let (k2, p2) = random_keypair();
        let data = api.encrypt_value_and_mask(amount, &mask, &p2, &k1).unwrap();
        let commitment = get_commitment_factory().commit_value(&mask, amount).to_byte_type();

        let decrypted = api.decrypt_value_and_mask(&data, &commitment, &k2, &p1).unwrap();

        assert_eq!(decrypted.value, amount);
        assert_eq!(decrypted.mask, mask);
    }
}
