//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use ledger_transport::APDUAnswer;
use tari_crypto::{
    compressed_key::CompressedKey,
    ristretto::{
        CompressedRistrettoComAndPubSig,
        RistrettoPublicKey,
        RistrettoSecretKey,
        pedersen::CompressedPedersenCommitment,
    },
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes};

use crate::LedgerClientError;

pub trait DecodeAnswer<Out> {
    fn decode<E>(&self) -> Result<Out, LedgerClientError<E>>
    where Self: Sized;
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<RistrettoPublicKeyBytes> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<RistrettoPublicKeyBytes, LedgerClientError<E>>
    where Self: Sized {
        // Ignore the version byte (TODO: validate the version?)
        let bytes = self.data().get(1..).ok_or_else(|| LedgerClientError::InvalidResponse {
            details: "response too short".to_string(),
        })?;
        RistrettoPublicKeyBytes::try_from(bytes)
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })
    }
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<String> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<String, LedgerClientError<E>>
    where Self: Sized {
        let bytes = self.data();
        str::from_utf8(bytes)
            .map(|s| s.to_string())
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })
    }
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<CompressedRistrettoComAndPubSig> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<CompressedRistrettoComAndPubSig, LedgerClientError<E>>
    where Self: Sized {
        let data = self.data();
        if data.len() < 161 {
            return Err(LedgerClientError::InvalidResponse {
                details: format!("response has invalid length: expected 161, got {}", data.len()),
            });
        }

        let public_nonce =
            CompressedPedersenCommitment::from_canonical_bytes(data.get(1..33).expect("Length already checked"))
                .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;
        let pub_key = CompressedKey::<RistrettoPublicKey>::from_canonical_bytes(
            data.get(33..65).expect("Length already checked"),
        )
        .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;
        let u_a = RistrettoSecretKey::from_canonical_bytes(data.get(65..97).expect("Length already checked"))
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;
        let u_x = RistrettoSecretKey::from_canonical_bytes(data.get(97..129).expect("Length already checked"))
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;
        let u_y = RistrettoSecretKey::from_canonical_bytes(data.get(129..161).expect("Length already checked"))
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;

        Ok(CompressedRistrettoComAndPubSig::new(
            public_nonce,
            pub_key,
            u_a,
            u_x,
            u_y,
        ))
    }
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<RistrettoSecretKey> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<RistrettoSecretKey, LedgerClientError<E>>
    where Self: Sized {
        let data = self.data();
        if data.len() < 33 {
            return Err(LedgerClientError::InvalidResponse {
                details: format!("response has invalid length: expected 33, got {}", data.len()),
            });
        }

        RistrettoSecretKey::from_canonical_bytes(data.get(1..33).expect("Length already checked"))
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })
    }
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<SchnorrSignatureBytes> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<SchnorrSignatureBytes, LedgerClientError<E>>
    where Self: Sized {
        let data = self.data();
        if data.len() < 65 {
            return Err(LedgerClientError::InvalidResponse {
                details: format!("response has invalid length: expected 65, got {}", data.len()),
            });
        }

        let public_nonce = RistrettoPublicKeyBytes::from_bytes(data.get(1..33).expect("Length already checked"))
            .expect("Length already checked");
        let sig = Scalar32Bytes::from_bytes(data.get(33..65).expect("Length already checked"))
            .expect("Length already checked");

        Ok(SchnorrSignatureBytes::new(public_nonce, sig))
    }
}

impl<B: Deref<Target = [u8]>> DecodeAnswer<RistrettoPublicKey> for APDUAnswer<B> {
    fn decode<E>(&self) -> Result<RistrettoPublicKey, LedgerClientError<E>>
    where Self: Sized {
        let data = self.data();
        if data.len() < 33 {
            return Err(LedgerClientError::InvalidResponse {
                details: format!("response has invalid length: expected 33, got {}", data.len()),
            });
        }

        RistrettoPublicKey::from_canonical_bytes(data.get(1..33).expect("Length already checked"))
            .map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })
    }
}
