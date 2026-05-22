//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Host-side mirrors of the WASM builtin templates' component state structs.
//!
//! Genesis bootstrap and test fixtures need to construct `ComponentBody`
//! payloads off-chain. Without these mirrors, callers hand-roll CBOR `Value`
//! trees that have to match the WASM template's wire format exactly — and
//! silently break (`unexpected type map at position 0: expected array`) the
//! moment a field is reordered or the codec swaps encoding strategies.
//!
//! Each struct here must remain wire-equivalent with its `#[template]`
//! counterpart in `crates/template_builtin/templates/<name>/`. The tests in
//! this module pin that invariant against the on-chain encoding.

use minicbor::{CborLen, Decode, Encode};
use tari_template_lib_types::VaultId;

/// Mirror of `XtrFaucet { vault: Vault }` in `templates/xtr_faucet/`.
///
/// `Vault` is `#[cbor(transparent)]` over `VaultId`, so the host-side
/// encoding of this struct is byte-identical to the WASM template's.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
pub struct XtrFaucetState {
    #[n(0)]
    pub vault: VaultId,
}

/// Mirror of `NftFaucet { serial_number: u64 }` in `templates/nft_faucet/`.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
pub struct NftFaucetState {
    #[n(0)]
    pub serial_number: u64,
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::ObjectKey;

    use super::*;

    /// Encoding `XtrFaucetState` must match a 1-element CBOR array whose only
    /// element is the bare `VaultId` — the shape the WASM template decodes.
    #[test]
    fn xtr_faucet_state_wire_format_matches_one_element_array_of_vault() {
        let vault = VaultId::new(ObjectKey::from_array([0xABu8; 32]));
        let from_struct = minicbor::to_vec(&XtrFaucetState { vault }).unwrap();
        let expected = minicbor::to_vec([vault]).unwrap();
        assert_eq!(from_struct, expected);
    }

    #[test]
    fn nft_faucet_state_wire_format_matches_one_element_array_of_uint() {
        let from_struct = minicbor::to_vec(&NftFaucetState { serial_number: 42 }).unwrap();
        let expected = minicbor::to_vec([42u64]).unwrap();
        assert_eq!(from_struct, expected);
    }
}
