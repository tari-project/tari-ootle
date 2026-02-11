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

use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use tari_common_types::types::FixedHash;
use tari_ootle_common_types::Epoch;
use tari_template_lib_types::{Hash32, TemplateAddress, crypto::RistrettoPublicKeyBytes};
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::global::GlobalDbAdapter;

pub struct TemplateDb<'a, 'tx, TGlobalDbAdapter: GlobalDbAdapter> {
    backend: &'a TGlobalDbAdapter,
    tx: &'tx mut TGlobalDbAdapter::DbTransaction<'a>,
}

impl<'a, 'tx, TGlobalDbAdapter: GlobalDbAdapter> TemplateDb<'a, 'tx, TGlobalDbAdapter> {
    pub fn new(backend: &'a TGlobalDbAdapter, tx: &'tx mut TGlobalDbAdapter::DbTransaction<'a>) -> Self {
        Self { backend, tx }
    }

    pub fn get_template(&mut self, key: &[u8]) -> Result<Option<DbTemplate>, TGlobalDbAdapter::Error> {
        self.backend.get_template(self.tx, key)
    }

    pub fn get_templates(&mut self, limit: usize) -> Result<Vec<DbTemplate>, TGlobalDbAdapter::Error> {
        self.backend.get_templates(self.tx, limit)
    }

    pub fn get_templates_by_addresses<'i, I: IntoIterator<Item = &'i TemplateAddress>>(
        &mut self,
        addresses: I,
    ) -> Result<Vec<DbTemplate>, TGlobalDbAdapter::Error> {
        self.backend.get_templates_by_addresses(self.tx, addresses)
    }

    pub fn get_pending_templates(&mut self, limit: usize) -> Result<Vec<DbTemplate>, TGlobalDbAdapter::Error> {
        self.backend.get_pending_templates(self.tx, limit)
    }

    pub fn insert_template(&mut self, template: DbTemplate) -> Result<(), TGlobalDbAdapter::Error> {
        self.backend.insert_template(self.tx, template)
    }

    pub fn update_template(&mut self, key: &[u8], update: DbTemplateUpdate) -> Result<(), TGlobalDbAdapter::Error> {
        self.backend.update_template(self.tx, key, update)
    }

    pub fn template_exists(
        &mut self,
        key: &TemplateAddress,
        status: Option<TemplateStatus>,
    ) -> Result<bool, TGlobalDbAdapter::Error> {
        self.backend.template_exists(self.tx, key, status)
    }

    pub fn set_status(&mut self, key: &TemplateAddress, status: TemplateStatus) -> Result<(), TGlobalDbAdapter::Error> {
        self.backend.set_status(self.tx, key, status)
    }
}

#[derive(Debug, Clone)]
pub struct DbTemplate {
    pub author_public_key: RistrettoPublicKeyBytes,
    pub template_address: TemplateAddress,
    pub template_name: String,
    pub binary_hash: Hash32,
    pub epoch: Epoch,
    pub template_type: DbTemplateType,
    pub code: Option<Vec<u8>>,
    pub url: Option<String>,
    pub status: TemplateStatus,
    pub added_at: PrimitiveDateTime,
}

impl DbTemplate {
    pub fn empty_pending(
        template_address: TemplateAddress,
        author_public_key: RistrettoPublicKeyBytes,
        epoch: Epoch,
    ) -> Self {
        Self {
            author_public_key,
            template_name: String::new(),
            template_address,
            binary_hash: Hash32::zero(),
            status: TemplateStatus::Pending,
            code: None,
            added_at: now(),
            template_type: DbTemplateType::Wasm,
            url: None,
            epoch,
        }
    }
}

fn now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

#[derive(Debug, Clone, Default)]
pub struct DbTemplateUpdate {
    pub author_public_key: Option<RistrettoPublicKeyBytes>,
    pub expected_hash: Option<FixedHash>,
    pub template_name: Option<String>,
    pub template_type: Option<DbTemplateType>,
    pub status: Option<TemplateStatus>,
    pub epoch: Option<Epoch>,
    pub code: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum DbTemplateType {
    Wasm,
}

impl FromStr for DbTemplateType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "wasm" => Ok(DbTemplateType::Wasm),
            _ => Err(()),
        }
    }
}

impl DbTemplateType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DbTemplateType::Wasm => "Wasm",
        }
    }
}

impl Display for DbTemplateType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TemplateStatus {
    /// Template has been registered but has not completed
    #[default]
    New,
    /// Template download has begun but not completed
    Pending,
    /// Template download has completed
    Active,
    /// Template download completed but was invalid
    Invalid,
    /// Template download failed
    DownloadFailed,
    /// Template has been deprecated
    Deprecated,
}

impl TemplateStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, TemplateStatus::Active)
    }

    pub fn is_deprecated(&self) -> bool {
        matches!(self, TemplateStatus::Deprecated)
    }
}

impl FromStr for TemplateStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "new" => Ok(TemplateStatus::New),
            "pending" => Ok(TemplateStatus::Pending),
            "active" => Ok(TemplateStatus::Active),
            "invalid" => Ok(TemplateStatus::Invalid),
            "downloadfailed" => Ok(TemplateStatus::DownloadFailed),
            "deprecated" => Ok(TemplateStatus::Deprecated),
            _ => Err(()),
        }
    }
}

impl TemplateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TemplateStatus::New => "New",
            TemplateStatus::Pending => "Pending",
            TemplateStatus::Active => "Active",
            TemplateStatus::Invalid => "Invalid",
            TemplateStatus::DownloadFailed => "DownloadFailed",
            TemplateStatus::Deprecated => "Deprecated",
        }
    }
}

impl Display for TemplateStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.as_str())
    }
}
