//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use ledger_transport::{APDUAnswer, APDUCommand, Exchange, async_trait};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct SpeculosTransport {
    inner: reqwest::Client,
    url: String,
}

impl SpeculosTransport {
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
            url: "http://localhost:5000/apdu".to_string(),
        }
    }

    pub fn with_url(mut self, url: String) -> Self {
        self.url = url;
        self
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
