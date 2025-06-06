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

use serde::{Deserialize, Serialize};
use tari_bor::encode;
use tari_template_abi::rust::fmt;
use tari_template_lib_types::serde_helpers;

pub type WorkspaceId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct WorkspaceOffsetId {
    id: WorkspaceId,
    offset: Option<usize>,
}

impl WorkspaceOffsetId {
    pub fn new(id: WorkspaceId) -> Self {
        Self { id, offset: None }
    }

    pub fn with_offset(self, offset: usize) -> Self {
        Self {
            id: self.id,
            offset: Some(offset),
        }
    }

    pub fn with_offset_opt(self, offset: Option<usize>) -> Self {
        Self { id: self.id, offset }
    }

    /// The workspace ID
    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    /// The offset within the workspace, if provided. Offset refers to the index of an array or field/map entry
    /// within a workspace item.
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }
}

impl fmt::Display for WorkspaceOffsetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(offset) = self.offset {
            write!(f, "{}.{}", self.id, offset)
        } else {
            write!(f, "{}", self.id)
        }
    }
}

/// Represents an argument that can be passed to a transaction instruction. Either a literal value or a reference to a
/// item on the runtime's workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum InstructionArg {
    /// The argument is in the transaction execution's workspace, which means it is the result of a previous
    /// instruction
    Workspace(#[cfg_attr(feature = "ts", ts(type = "number"))] WorkspaceOffsetId),
    /// The argument is a value specified in the transaction
    Literal(
        #[serde(with = "serde_helpers::dynamic_hex")]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        Vec<u8>,
    ),
    // Literal(tari_bor::Value),
}

impl InstructionArg {
    pub fn literal(value: tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        // TODO: Unfortunately, CBOR value does not serialize consistently in JSON so we have to use the byte encoded
        // form for now.
        Ok(Self::Literal(encode(&value)?))
    }

    pub fn from_type<T: Serialize>(val: &T) -> Result<Self, tari_bor::BorError> {
        Ok(Self::Literal(encode(val)?))
    }

    pub fn workspace(id: WorkspaceId, offset: Option<usize>) -> Self {
        Self::workspace_offset(WorkspaceOffsetId::new(id).with_offset_opt(offset))
    }

    pub fn workspace_offset(id: WorkspaceOffsetId) -> Self {
        Self::Workspace(id)
    }

    pub fn as_literal_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Literal(bytes) => Some(bytes),
            Self::Workspace(_) => None,
        }
    }
}
