//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Uses CBOR encoding. This means that any changes to field names etc effect the preimage of a hash.
/// The user must make sure this is acceptable for the specific use case. e.g. the component body, nfts etc.
pub(crate) fn serialize_cbor_value<W: borsh::io::Write>(
    obj: &tari_bor::Value,
    writer: &mut W,
) -> Result<(), borsh::io::Error> {
    tari_bor::encode_into_std_writer(obj, writer).map_err(borsh::io::Error::other)
}
