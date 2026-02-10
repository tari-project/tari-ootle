//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport::{APDUCommand, APDUErrorCode, Exchange};
use minotari_ledger_wallet_common::common_types::{AppSW, Instruction, LedgerKeyBranch};
use tari_crypto::{
    ristretto::{CompressedRistrettoComAndPubSig, RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{decode::DecodeAnswer, error::LedgerClientError};

pub type LedgerClientResult<T, E> = Result<T, LedgerClientError<E>>;

const CLA: u8 = 0x80;

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

    pub async fn get_public_spend_key(&self, account: u64) -> LedgerClientResult<RistrettoPublicKeyBytes, T::Error> {
        let acc_bytes = account.to_le_bytes();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetPublicSpendKey as u8,
            p1: 0,
            p2: 0,
            data: acc_bytes.as_slice(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        answer.decode()
    }

    pub async fn get_public_key(
        &self,
        account: u64,
        index: u64,
        branch: LedgerKeyBranch,
    ) -> LedgerClientResult<RistrettoPublicKeyBytes, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            index.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(branch.as_byte()).as_slice(),
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetPublicKey as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;

        answer.decode()
    }

    pub async fn get_script_signature_managed(
        &self,
        account: u64,
        network_byte: u8,
        version: u8,
        branch: &LedgerKeyBranch,
        index: u64,
        value: &RistrettoSecretKey,
        commitment_private_key: &RistrettoSecretKey,
        commitment: &PedersenCommitmentBytes,
        message: [u8; 32],
    ) -> LedgerClientResult<CompressedRistrettoComAndPubSig, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(network_byte).as_slice(),
            cst_u8_u64_le_bytes(version).as_slice(),
            value.as_bytes(),
            commitment_private_key.as_bytes(),
            commitment.as_bytes(),
            message.as_slice(),
            cst_u8_u64_le_bytes(branch.as_byte()).as_slice(),
            index.to_le_bytes().as_slice(),
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetScriptSignatureManaged as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        answer.decode()
    }

    pub async fn get_script_signature_derived(
        &self,
        account: u64,
        network_byte: u8,
        version: u8,
        branch_key: &RistrettoSecretKey,
        value: &RistrettoSecretKey,
        commitment_private_key: &RistrettoSecretKey,
        commitment: &PedersenCommitmentBytes,
        message: [u8; 32],
    ) -> LedgerClientResult<CompressedRistrettoComAndPubSig, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(network_byte).as_slice(),
            cst_u8_u64_le_bytes(version).as_slice(),
            value.as_bytes(),
            commitment_private_key.as_bytes(),
            commitment.as_bytes(),
            message.as_slice(),
            branch_key.as_bytes(),
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetScriptSignatureDerived as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;
        answer.decode()
    }

    pub async fn get_script_offset(
        &self,
        account: u64,
        partial_script_offset: &RistrettoSecretKey,
        derived_script_keys: &[RistrettoSecretKey],
        script_key_indexes: &[(LedgerKeyBranch, u64)],
        derived_sender_offsets: &[RistrettoSecretKey],
        sender_offset_indexes: &[(LedgerKeyBranch, u64)],
    ) -> LedgerClientResult<RistrettoSecretKey, T::Error> {
        let num_commands = 1 +
            1 +
            sender_offset_indexes.len() +
            script_key_indexes.len() +
            derived_sender_offsets.len() +
            derived_script_keys.len();
        let mut data = Vec::with_capacity(num_commands);

        // Requires execution of multiple commands due to APDU data size limits, so we prepare each command's the data
        // as follows:

        // 1. Set lengths
        data.push(
            [
                account.to_le_bytes().as_slice(),
                cst_usize_u64_le_bytes(sender_offset_indexes.len()).as_slice(),
                cst_usize_u64_le_bytes(script_key_indexes.len()).as_slice(),
                cst_usize_u64_le_bytes(derived_sender_offsets.len()).as_slice(),
                cst_usize_u64_le_bytes(derived_script_keys.len()).as_slice(),
            ]
            .concat(),
        );

        // 2. Partial script offset
        data.push(partial_script_offset.as_bytes().to_vec());

        // Sender offsets
        for (branch, index) in sender_offset_indexes {
            data.push(
                [
                    cst_u8_u64_le_bytes(branch.as_byte()).as_slice(),
                    index.to_le_bytes().as_slice(),
                ]
                .concat(),
            );
        }

        // Script keys
        for (branch, index) in script_key_indexes {
            data.push(
                [
                    cst_u8_u64_le_bytes(branch.as_byte()).as_slice(),
                    index.to_le_bytes().as_slice(),
                ]
                .concat(),
            );
        }

        // Derived sender offsets
        for key in derived_sender_offsets {
            data.push(key.as_bytes().to_vec());
        }

        // Derived script keys
        for key in derived_script_keys {
            data.push(key.as_bytes().to_vec());
        }

        // Send the commands sequentially, each command updates internal state on the device until we send p2 = 0 on the
        // last command, which triggers the device to return the final result
        for (i, datum) in data.iter().enumerate() {
            let more = i + 1 != data.len();
            let command = APDUCommand {
                cla: CLA,
                ins: Instruction::GetScriptOffset as u8,
                p1: i as u8,
                p2: u8::from(more),
                data: datum.as_slice(),
            };
            let answer = self.inner.exchange(&command).await?;
            apdu_err(answer.error_code())?;
            if !more {
                return answer.decode();
            }
        }

        unreachable!(
            "Data vector is guaranteed to have at least one element, so loop will always return before reaching this \
             point"
        );
    }

    pub async fn get_secret_view_key(&self, account: u64) -> LedgerClientResult<RistrettoPublicKeyBytes, T::Error> {
        let acc_bytes = account.to_le_bytes();
        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetViewKey as u8,
            p1: 0,
            p2: 0,
            data: acc_bytes.as_slice(),
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;

        answer.decode()
    }

    pub async fn get_dh_shared_secret(
        &self,
        account: u64,
        index: u64,
        branch: LedgerKeyBranch,
        public_key: &RistrettoPublicKeyBytes,
    ) -> LedgerClientResult<RistrettoPublicKey, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            index.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(branch.as_byte()).as_slice(),
            public_key.as_slice(),
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetDHSharedSecret as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;

        answer.decode()
    }

    pub async fn get_raw_schnorr_signature(
        &self,
        account: u64,
        private_key_index: u64,
        private_key_branch: LedgerKeyBranch,
        nonce_index: u64,
        nonce_branch: LedgerKeyBranch,
        challenge: &[u8; 64],
    ) -> LedgerClientResult<SchnorrSignatureBytes, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            private_key_index.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(private_key_branch.as_byte()).as_slice(),
            nonce_index.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(nonce_branch.as_byte()).as_slice(),
            challenge.as_slice(),
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetRawSchnorrSignature as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;

        answer.decode()
    }

    pub async fn get_script_schnorr_signature(
        &self,
        account: u64,
        private_key_index: u64,
        private_key_branch: LedgerKeyBranch,
        nonce: &[u8],
    ) -> LedgerClientResult<SchnorrSignatureBytes, T::Error> {
        let data = [
            account.to_le_bytes().as_slice(),
            private_key_index.to_le_bytes().as_slice(),
            cst_u8_u64_le_bytes(private_key_branch.as_byte()).as_slice(),
            nonce,
        ]
        .concat();

        let command = APDUCommand {
            cla: CLA,
            ins: Instruction::GetScriptSchnorrSignature as u8,
            p1: 0,
            p2: 0,
            data,
        };
        let answer = self.inner.exchange(&command).await?;
        apdu_err(answer.error_code())?;

        answer.decode()
    }
}

const fn cst_u8_u64_le_bytes(value: u8) -> [u8; 8] {
    [value, 0, 0, 0, 0, 0, 0, 0]
}

const fn cst_usize_u64_le_bytes(value: usize) -> [u8; 8] {
    let value = value as u64;
    value.to_le_bytes()
}

fn apdu_err<E>(code: Result<APDUErrorCode, u16>) -> LedgerClientResult<(), E> {
    match code {
        Ok(APDUErrorCode::NoError) => Ok(()),
        Ok(code) => Err(LedgerClientError::APDUError { code }),
        Err(code) => AppSW::try_from(code)
            .map_err(|_| LedgerClientError::APDUOtherCodeError { code })
            .and_then(|app_sw| Err(LedgerClientError::AppError { code: app_sw })),
    }
}

#[cfg(all(test, feature = "speculos-transport"))]
mod tests {
    use super::*;
    use crate::speculos_transport::SpeculosTransport;

    #[tokio::test]
    #[ignore = "Requires Speculos to be running with the minotari_ledger_wallet app."]
    async fn test_commands() {
        let transport = SpeculosTransport::new();
        let client = LedgerClient::new(transport);

        let version = client.get_app_version().await.unwrap();
        assert_eq!(version, "5.3.0-pre.0");

        let app_name = client.get_app_name().await.unwrap();
        assert_eq!(app_name, "minotari_ledger_wallet");

        let public_spend_key = client.get_public_spend_key(0).await.unwrap();
        assert_eq!(public_spend_key.len(), 32);

        let public_key = client.get_public_key(0, 0, LedgerKeyBranch::Spend).await.unwrap();
        assert_eq!(public_key.len(), 32);

        let view_key = client.get_secret_view_key(0).await.unwrap();
        assert_eq!(view_key.len(), 32);

        let _valid_dh_shared_secret = client
            .get_dh_shared_secret(0, 0, LedgerKeyBranch::Spend, &public_key)
            .await
            .unwrap();

        let script_offset = client
            .get_script_offset(
                0,
                &RistrettoSecretKey::from(1),
                &[RistrettoSecretKey::from(2)],
                &[(LedgerKeyBranch::Spend, 0)],
                &[RistrettoSecretKey::from(3)],
                &[(LedgerKeyBranch::Spend, 0)],
            )
            .await
            .unwrap();
        assert_eq!(script_offset.as_bytes().len(), 32);

        let challenge = [0u8; 64];
        let raw_schnorr_signature = client
            .get_raw_schnorr_signature(0, 0, LedgerKeyBranch::Spend, 0, LedgerKeyBranch::Spend, &challenge)
            .await
            .unwrap();
        assert_eq!(raw_schnorr_signature.public_nonce().len(), 32);
        assert_eq!(raw_schnorr_signature.signature().len(), 32);

        let nonce = [0u8; 32];
        let script_schnorr_signature = client
            .get_script_schnorr_signature(0, 0, LedgerKeyBranch::Spend, &nonce)
            .await
            .unwrap();
        assert_eq!(script_schnorr_signature.public_nonce().len(), 32);
        assert_eq!(script_schnorr_signature.signature().len(), 32);
    }
}
