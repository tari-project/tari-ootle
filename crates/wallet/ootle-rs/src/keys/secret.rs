//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use ootle_byte_type::ToByteType;
use rand::thread_rng;
use signature::{Keypair, hazmat::PrehashSigner};
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_address::{Network, OotleAddress};
use tari_ootle_wallet_crypto::kdfs;

use crate::{
    key_provider,
    key_provider::OutputMaskProvider,
    keys::traits::HasViewOnlyKeySecret,
    signer,
    signer::StealthKeyPrehashSigner,
};

#[derive(Clone)]
pub struct OotleSecretKey {
    network: Network,
    account_secret: RistrettoSecretKey,
    view_only_secret: RistrettoSecretKey,
}

impl OotleSecretKey {
    /// Create an OotleSecretKey from existing raw Ristretto secret keys.
    pub fn new(network: Network, account_secret: RistrettoSecretKey, view_only_secret: RistrettoSecretKey) -> Self {
        Self {
            network,
            account_secret,
            view_only_secret,
        }
    }

    pub fn random(network: Network) -> Self {
        let mut rng = rand::thread_rng();
        Self::random_with(&mut rng, network)
    }

    pub fn random_with<R: rand::Rng + rand::CryptoRng>(rng: &mut R, network: Network) -> Self {
        let account_secret = RistrettoSecretKey::random(rng);
        let view_only_secret = RistrettoSecretKey::random(rng);
        Self {
            network,
            account_secret,
            view_only_secret,
        }
    }

    pub fn account_secret(&self) -> &RistrettoSecretKey {
        &self.account_secret
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn to_address(&self) -> OotleAddress {
        let account_pk = RistrettoPublicKey::from_secret_key(&self.account_secret);
        let view_only_pk = RistrettoPublicKey::from_secret_key(&self.view_only_secret);
        OotleAddress::new(self.network, view_only_pk.to_byte_type(), account_pk.to_byte_type())
    }
}

impl PrehashSigner<(RistrettoSchnorr, RistrettoPublicKey)> for OotleSecretKey {
    fn sign_prehash(&self, prehash: &[u8]) -> signature::Result<(RistrettoSchnorr, RistrettoPublicKey)> {
        let signature = RistrettoSchnorr::sign(&self.account_secret, prehash, &mut rand::thread_rng())
            .expect("sign is infallible (challenge is the correct length)");
        let public_key = self.verifying_key();
        Ok((signature, public_key))
    }
}

impl StealthKeyPrehashSigner<(RistrettoSchnorr, RistrettoPublicKey)> for OotleSecretKey {
    async fn sign_prehash_with_stealth_key(
        &self,
        public_nonce: &RistrettoPublicKey,
        prehash: &[u8],
    ) -> signer::Result<(RistrettoSchnorr, RistrettoPublicKey)> {
        let secret = kdfs::owner_stealth_dh_secret(self.network(), self.account_secret(), public_nonce);
        let signature = RistrettoSchnorr::sign(&secret, prehash, &mut thread_rng())
            .expect("sign is infallible (challenge is the correct length)");
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        Ok((signature, public_key))
    }
}

impl HasViewOnlyKeySecret for OotleSecretKey {
    fn view_only_secret(&self) -> &RistrettoSecretKey {
        &self.view_only_secret
    }
}

impl Keypair for OotleSecretKey {
    type VerifyingKey = RistrettoPublicKey;

    fn verifying_key(&self) -> Self::VerifyingKey {
        RistrettoPublicKey::from_secret_key(&self.account_secret)
    }
}

#[async_trait]
impl OutputMaskProvider for OotleSecretKey {
    async fn next_mask(&self) -> key_provider::Result<RistrettoSecretKey> {
        // For simplicity, just generate a random mask here. Another implementation may want to derive it in some
        // deterministic way.
        Ok(RistrettoSecretKey::random(&mut rand::thread_rng()))
    }
}
