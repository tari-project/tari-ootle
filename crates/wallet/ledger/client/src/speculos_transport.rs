//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! APDU transport for the [Speculos](https://github.com/LedgerHQ/speculos) Ledger emulator.

use std::ops::Deref;

use ledger_transport::{APDUAnswer, APDUCommand, Exchange, async_trait};
use serde::{Deserialize, Serialize};

/// [`Exchange`] implementation that sends APDUs to a running Speculos emulator over its REST API,
/// so the full client/app exchange can be exercised without a physical device.
#[derive(Debug, Default)]
pub struct SpeculosTransport {
    inner: reqwest::Client,
    url: String,
}

impl SpeculosTransport {
    /// Connects to the Speculos REST `/apdu` endpoint at the default `http://localhost:5000`.
    pub fn new() -> Self {
        Self::with_base_url("http://localhost:5000")
    }

    /// Connects to the Speculos REST `/apdu` endpoint at `base_url`; `/apdu` is appended.
    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            inner: reqwest::Client::new(),
            url: format!("{}/apdu", base_url.trim_end_matches('/')),
        }
    }
}

#[async_trait]
impl Exchange for SpeculosTransport {
    type AnswerType = Vec<u8>;
    type Error = reqwest::Error;

    async fn exchange<I>(&self, command: &APDUCommand<I>) -> Result<APDUAnswer<Self::AnswerType>, Self::Error>
    where I: Deref<Target = [u8]> + Send + Sync {
        let data = hex::encode(command.serialize());
        let response = self.inner.post(&self.url).json(&Request { data }).send().await?;

        let answer = response.json::<Answer>().await?;
        let data = hex::decode(answer.data).unwrap();
        Ok(APDUAnswer::from_answer(data).expect("answer should be valid"))
    }
}

#[derive(Debug, Clone, Serialize)]
struct Request {
    data: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Answer {
    data: String,
}
