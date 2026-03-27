//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use serde::{Deserialize, Serialize};

use crate::{
    rust::{boxed::Box, ops, string::String, vec::Vec},
    version::WasmAbiVersion,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TemplateDef {
    V1(TemplateDefV1),
}

impl TemplateDef {
    pub fn template_name(&self) -> &str {
        match self {
            TemplateDef::V1(def) => def.template_name.as_str(),
        }
    }

    pub fn abi_version(&self) -> u16 {
        match self {
            TemplateDef::V1(def) => def.abi_version,
        }
    }

    pub fn get_function(&self, name: &str) -> Option<&FunctionDef> {
        match self {
            TemplateDef::V1(def) => def.get_function(name),
        }
    }

    pub fn functions(&self) -> &[FunctionDef] {
        match self {
            TemplateDef::V1(def) => &def.functions,
        }
    }

    /// Encodes the template definition with a length prefix, which is required for passing data to the engine in wasm.
    /// The length prefix is a 4-byte little-endian integer representing the total length of the encoded data (including
    /// the prefix itself).
    #[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
    pub fn encode_for_wasm_embedding(&self) -> Result<Vec<u8>, tari_bor::BorError> {
        use std::io::Write;

        use tari_bor::{encode_into_writer, encoded_len};

        use crate::WASM_PTR_SIZE;
        let data_len = encoded_len(self)?;
        // for the length prefix
        let mut buf = Vec::with_capacity(data_len + WASM_PTR_SIZE);
        let full_len = data_len + WASM_PTR_SIZE;
        let writer = &mut buf;
        // length prefix
        writer.write_all(&(full_len as u32).to_le_bytes())?;
        encode_into_writer(self, writer)?;
        Ok(buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TemplateDefV1 {
    pub template_name: String,
    pub abi_version: WasmAbiVersion,
    pub functions: Vec<FunctionDef>,
}

impl TemplateDefV1 {
    pub fn get_function(&self, name: &str) -> Option<&FunctionDef> {
        self.functions.iter().find(|f| f.name.as_str() == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FunctionDef {
    pub name: String,
    pub arguments: Vec<ArgDef>,
    pub output: Type,
    pub is_mut: bool,
    #[serde(default, skip_serializing_if = "ops::Not::not")]
    pub is_migration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ArgDef {
    pub name: String,
    pub arg_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Type {
    #[default]
    Unit,
    Bool,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    String,
    Vec(Box<Type>),
    Tuple(Vec<Type>),
    Other {
        name: String,
    },
    Option(Box<Type>),
}

impl Type {
    pub fn other(&self) -> Option<&str> {
        match self {
            Type::Other { name } => Some(name),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Unit => write!(f, "Unit"),
            Type::Bool => write!(f, "Bool"),
            Type::I8 => write!(f, "I8"),
            Type::I16 => write!(f, "I16"),
            Type::I32 => write!(f, "I32"),
            Type::I64 => write!(f, "I64"),
            Type::I128 => write!(f, "I128"),
            Type::U8 => write!(f, "U8"),
            Type::U16 => write!(f, "U16"),
            Type::U32 => write!(f, "U32"),
            Type::U64 => write!(f, "U64"),
            Type::U128 => write!(f, "U128"),
            Type::String => write!(f, "String"),
            Type::Vec(t) => write!(f, "Vec<{}>", t),
            Type::Option(t) => write!(f, "Option<{}>", t),
            Type::Tuple(types) => {
                let type_list = types.iter().map(|t| format!("{:?}", t)).collect::<Vec<_>>().join(",");
                write!(f, "Tuple<{}>", type_list)
            },
            Type::Other { name } => write!(f, "{}", name),
        }
    }
}
