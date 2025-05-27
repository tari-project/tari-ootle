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
use tari_template_lib_types::serde_helpers;

pub type WorkspaceKey = Vec<u8>;

/// The possible ways to represent an instruction's argument
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum Arg {
    /// The argument is in the transaction execution's workspace, which means it is the result of a previous
    /// instruction
    Workspace(
        #[serde(with = "serde_helpers::dynamic_hex")]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        WorkspaceKey,
    ),
    /// The argument is a value specified in the transaction
    Literal(
        #[serde(with = "serde_helpers::dynamic_hex")]
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        Vec<u8>,
    ),
    // Literal(tari_bor::Value),
}

impl Arg {
    pub fn literal(value: tari_bor::Value) -> Result<Self, tari_bor::BorError> {
        // TODO: Unfortunately, CBOR value does not serialize consistently in JSON so we have to use the byte encoded
        // form for now.
        Ok(Arg::Literal(encode(&value)?))
    }

    pub fn from_type<T: Serialize>(val: &T) -> Result<Self, tari_bor::BorError> {
        Ok(Arg::Literal(encode(val)?))
    }

    pub fn workspace<T: Into<Vec<u8>>>(key: T) -> Self {
        Arg::Workspace(key.into())
    }

    pub fn as_literal_bytes(&self) -> Option<&[u8]> {
        match self {
            Arg::Workspace(_) => None,
            Arg::Literal(bytes) => Some(bytes),
        }
    }
}

#[cfg(test)]
mod tests {
    use tari_bor::decode_exact;
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;
    use crate::args;

    #[derive(Serialize, Deserialize, Debug)]
    struct TestCase {
        bytes: Vec<u8>,
        pk: RistrettoPublicKeyBytes,
    }

    #[test]
    fn decode_encode() {
        let test_case = TestCase {
            bytes: vec![1, 2, 3, 4, 5],
            pk: RistrettoPublicKeyBytes::from([1; 32]),
        };
        let args = args![test_case];
        let json = serde_json::to_string(&args).unwrap();
        let decoded: Vec<Arg> = serde_json::from_str(&json).unwrap();

        let decoded: TestCase = decode_exact(decoded[0].as_literal_bytes().unwrap()).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let json = serde_json::to_value(&args).unwrap();
        assert_eq!(
            json[0]["Literal"].as_str().expect("string"),
            "a265627974657385010203040562706b58200101010101010101010101010101010101010101010101010101010101010101"
        );

        let decoded = tari_bor::decode::<Vec<Arg>>(&tari_bor::encode(&args).unwrap()).unwrap();
        let decoded: TestCase = decode_exact(decoded[0].as_literal_bytes().unwrap()).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);

        let cbor = tari_bor::to_value(&args).unwrap();
        let decoded = tari_bor::from_value::<Vec<Arg>>(&cbor).unwrap();
        let decoded: TestCase = decode_exact(decoded[0].as_literal_bytes().unwrap()).unwrap();
        assert_eq!(test_case.bytes, decoded.bytes);
        assert_eq!(test_case.pk, decoded.pk);
    }
}
