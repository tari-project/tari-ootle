//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport::{APDUCommand, APDUErrorCode, Exchange};
use ootle_ledger_common::{
    Instruction,
    OotleStatusWord,
    arg_types::{
        FrameKind,
        GetPublicKeyRequest,
        GetPublicKeyResponse,
        KeyType,
        SEGMENT_LAST_CHUNK,
        SignMode,
        SignTransactionHeader,
        SignTransactionResponse,
        SigningField,
    },
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{decode::DecodeAnswer, error::LedgerClientError};

pub type LedgerClientResult<T, E> = Result<T, LedgerClientError<E>>;

const CLA: u8 = 0x80;

/// Max APDU data payload per chunk. The APDU data field is at most 255 bytes; stay under it.
const MAX_CHUNK: usize = 250;

/// One field of the canonical signing preimage to stream: its wire tag and borsh bytes.
pub type SegmentRef<'a> = (SigningField, &'a [u8]);

pub struct LedgerClient<T> {
    inner: T,
}

impl<T: Exchange> LedgerClient<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub async fn get_app_version(&self) -> LedgerClientResult<String, T::Error> {
        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetVersion as u8,
            p1: 0,
            p2: 0,
            data: Vec::<u8>::new(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        answer.decode()
    }

    pub async fn get_app_name(&self) -> LedgerClientResult<String, T::Error> {
        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetAppName as u8,
            p1: 0,
            p2: 0,
            data: Vec::<u8>::new(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        answer.decode()
    }

    pub async fn get_public_key(
        &self,
        account: u64,
        index: u64,
        key_type: KeyType,
    ) -> LedgerClientResult<RistrettoPublicKeyBytes, T::Error> {
        let req = GetPublicKeyRequest {
            account,
            index,
            key_type,
        };

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetPublicKey as u8,
            p1: 0,
            p2: 0,
            data: borsh::to_vec(&req).unwrap(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        let GetPublicKeyResponse { public_key } = answer.decode()?;
        Ok(public_key.into())
    }

    /// Stream a transaction to the device for signing and return the signature once the user
    /// approves on-device.
    ///
    /// `mode` selects the procedure (authorization vs seal). `segments` are the canonical preimage
    /// fields in chain order — see `tari_ootle_transaction::TransactionSignature::signing_preimage_v1`
    /// / `TransactionSealSignature::signing_preimage_v1`. The device recomputes the signing message
    /// from these bytes itself; nothing here is trusted as a precomputed hash.
    pub async fn sign_transaction(
        &self,
        account: u64,
        index: u64,
        key_type: KeyType,
        mode: SignMode,
        segments: &[SegmentRef<'_>],
    ) -> LedgerClientResult<SignTransactionResponse, T::Error> {
        let header = SignTransactionHeader {
            account,
            index,
            key_type,
            mode,
        };
        let header_bytes =
            borsh::to_vec(&header).map_err(|e| LedgerClientError::InvalidResponse { details: e.to_string() })?;
        self.send_sign_chunk(0, FrameKind::Header as u8, header_bytes).await?;

        for (field, bytes) in segments {
            let tag = *field as u8;
            if bytes.is_empty() {
                self.send_sign_chunk(tag | SEGMENT_LAST_CHUNK, FrameKind::Segment as u8, Vec::new())
                    .await?;
                continue;
            }
            let mut chunks = bytes.chunks(MAX_CHUNK).peekable();
            while let Some(chunk) = chunks.next() {
                let p1 = if chunks.peek().is_none() {
                    tag | SEGMENT_LAST_CHUNK
                } else {
                    tag
                };
                self.send_sign_chunk(p1, FrameKind::Segment as u8, chunk.to_vec())
                    .await?;
            }
        }

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::SignTransaction as u8,
            p1: 0,
            p2: FrameKind::Finalize as u8,
            data: Vec::<u8>::new(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        let response: SignTransactionResponse = answer.decode()?;
        Ok(response)
    }

    /// Send one streaming chunk and assert the device acknowledged it (empty OK expected).
    async fn send_sign_chunk(&self, p1: u8, p2: u8, data: Vec<u8>) -> LedgerClientResult<(), T::Error> {
        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::SignTransaction as u8,
            p1,
            p2,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        Ok(())
    }
}

fn apdu_err<E>(code: Result<APDUErrorCode, u16>) -> LedgerClientResult<(), E> {
    match code {
        Ok(APDUErrorCode::NoError) => Ok(()),
        Ok(code) => Err(LedgerClientError::APDUError { code }),
        Err(code) => OotleStatusWord::try_from(code)
            .map_err(|_| LedgerClientError::APDUOtherCodeError { code })
            .and_then(|app_sw| Err(LedgerClientError::AppError { code: app_sw })),
    }
}

#[cfg(all(test, feature = "speculos-transport"))]
mod tests {
    use super::*;
    use crate::speculos_transport::SpeculosTransport;

    #[tokio::test]
    #[ignore = "Requires Speculos to be running with the ootle ledger app."]
    async fn basic_instructions() {
        let transport = SpeculosTransport::new();
        let client = LedgerClient::new(transport);

        let version = client.get_app_version().await.unwrap();
        assert_eq!(version, "0.1.0");

        let app_name = client.get_app_name().await.unwrap();
        assert_eq!(app_name, "Tari Ootle");

        let public_key = client.get_public_key(0, 0, KeyType::Transaction).await.unwrap();
        assert_eq!(public_key.len(), 32);
        let other_pk = client.get_public_key(0, 0, KeyType::Transaction).await.unwrap();
        assert_eq!(public_key, other_pk);
        let other_pk = client.get_public_key(0, 1, KeyType::Transaction).await.unwrap();
        assert_ne!(public_key, other_pk);
        let other_pk = client.get_public_key(0, 0, KeyType::Account).await.unwrap();
        assert_ne!(public_key, other_pk);
        let other_pk = client.get_public_key(1, 0, KeyType::Transaction).await.unwrap();
        assert_ne!(public_key, other_pk);
    }

    use indexmap::IndexSet;
    use tari_ootle_transaction::{
        Blob,
        Blobs,
        Instruction,
        PreimageSegment,
        TransactionSealSignature,
        TransactionSignature,
        UnsealedTransactionV1,
        UnsignedTransactionV1,
    };
    use tari_template_lib_types::crypto::SchnorrSignatureBytes;

    fn sample_unsigned() -> UnsignedTransactionV1 {
        let mut blobs = Blobs::empty();
        blobs.push(Blob::from(vec![1u8, 2, 3])).unwrap();
        UnsignedTransactionV1 {
            network: 1,
            fee_instructions: vec![Instruction::DropAllProofsInWorkspace],
            instructions: vec![
                Instruction::DropAllProofsInWorkspace,
                Instruction::PutLastInstructionOutputOnWorkspace { key: 7 },
            ],
            inputs: IndexSet::new(),
            min_epoch: None,
            max_epoch: None,
            is_seal_signer_authorized: false,
            dry_run: false,
            blobs,
        }
    }

    fn to_refs(segments: &[PreimageSegment]) -> Vec<SegmentRef<'_>> {
        segments
            .iter()
            // The numeric equivalence of the two field enums is asserted by the
            // `preimage_field_tags_match_protocol` test.
            .map(|seg| (SigningField::try_from(seg.field as u8).unwrap(), seg.bytes.as_slice()))
            .collect()
    }

    #[tokio::test]
    #[ignore = "Requires Speculos to be running with the ootle ledger app."]
    async fn sign_authorization_roundtrip() {
        let client = LedgerClient::new(SpeculosTransport::new());
        let (account, index) = (0u64, 0u64);

        // Any public key works as the seal-signer context for an authorization signature.
        let seal_signer = client.get_public_key(account, index, KeyType::Account).await.unwrap();
        let tx = sample_unsigned();

        let segments = TransactionSignature::signing_preimage_v1(&seal_signer, &tx);
        let response = client
            .sign_transaction(
                account,
                index,
                KeyType::Account,
                SignMode::AddSigner,
                &to_refs(&segments),
            )
            .await
            .unwrap();

        let signature = TransactionSignature::new(
            RistrettoPublicKeyBytes::from(response.public_key),
            SchnorrSignatureBytes::try_from(&response.signature[..]).unwrap(),
        );
        assert!(
            signature.verify_v1(&seal_signer, &tx),
            "device authorization signature did not verify"
        );
    }

    #[tokio::test]
    #[ignore = "Requires Speculos to be running with the ootle ledger app."]
    async fn seal_roundtrip() {
        let client = LedgerClient::new(SpeculosTransport::new());
        let (account, index) = (0u64, 0u64);

        let unsealed = UnsealedTransactionV1::new(sample_unsigned(), vec![]);
        let segments = TransactionSealSignature::signing_preimage_v1(&unsealed);
        let response = client
            .sign_transaction(account, index, KeyType::Account, SignMode::Seal, &to_refs(&segments))
            .await
            .unwrap();

        let seal = TransactionSealSignature::new(
            RistrettoPublicKeyBytes::from(response.public_key),
            SchnorrSignatureBytes::try_from(&response.signature[..]).unwrap(),
        );
        assert!(seal.verify_v1(&unsealed), "device seal signature did not verify");
    }
}
