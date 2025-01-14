//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use chrono::NaiveDateTime;
use tari_common_types::types::{FixedHash, FixedHashSizeError};
use tari_dan_common_types::Epoch;
use tari_dan_storage::global::DbTemplate;
use tari_engine_types::TemplateAddress;
use tari_template_lib::HashParseError;
use thiserror::Error;

use crate::global::schema::*;

#[derive(Debug, Identifiable, Queryable)]
#[diesel(table_name = templates)]
pub struct TemplateModel {
    pub id: i32,
    pub template_name: String,
    pub expected_hash: Vec<u8>,
    pub template_address: Vec<u8>,
    pub url: Option<String>,
    pub epoch: i64,
    pub template_type: String,
    pub author_public_key: Vec<u8>,
    pub compiled_code: Option<Vec<u8>>,
    pub flow_json: Option<String>,
    pub status: String,
    pub manifest: Option<String>,
    pub added_at: NaiveDateTime,
}

#[derive(Debug, Error)]
pub enum TemplateConversionError {
    #[error("Fixed hash size error: {0}")]
    FixedHashSize(#[from] FixedHashSizeError),
    #[error("Hash parse error: {0}")]
    HashParse(#[from] HashParseError),
}

impl TryInto<DbTemplate> for TemplateModel {
    type Error = TemplateConversionError;

    fn try_into(self) -> Result<DbTemplate, Self::Error> {
        Ok(DbTemplate {
            author_public_key: FixedHash::try_from(self.author_public_key.as_slice())?,
            template_name: self.template_name,
            expected_hash: self.expected_hash.try_into()?,
            template_address: TemplateAddress::try_from_vec(self.template_address)?,
            template_type: self.template_type.parse().expect("DB template type corrupted"),
            compiled_code: self.compiled_code,
            flow_json: self.flow_json,
            manifest: self.manifest,
            url: self.url,
            status: self.status.parse().expect("DB status corrupted"),
            added_at: self.added_at,
            epoch: Epoch(self.epoch as u64),
        })
    }
}

#[derive(Debug, Insertable)]
#[diesel(table_name = templates)]
pub struct NewTemplateModel {
    pub author_public_key: Vec<u8>,
    pub template_address: Vec<u8>,
    pub template_name: String,
    pub expected_hash: Vec<u8>,
    pub template_type: String,
    pub compiled_code: Option<Vec<u8>>,
    pub epoch: i64,
    pub flow_json: Option<String>,
    pub status: String,
    pub manifest: Option<String>,
}

#[derive(Debug, AsChangeset)]
#[diesel(table_name = templates)]
pub struct TemplateUpdateModel {
    pub author_public_key: Option<Vec<u8>>,
    pub expected_hash: Option<Vec<u8>>,
    pub template_type: Option<String>,
    pub template_name: Option<String>,
    pub compiled_code: Option<Vec<u8>>,
    pub flow_json: Option<String>,
    pub manifest: Option<String>,
    pub status: Option<String>,
}
