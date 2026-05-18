//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport::{APDUCommand, APDUErrorCode, Exchange};
use ootle_ledger_common::{
    Instruction,
    OotleStatusWord,
    arg_types::{GetPublicKeyRequest, GetPublicKeyResponse, KeyType},
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

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
        assert_eq!(app_name, "Ootle Ledger App");

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
}
