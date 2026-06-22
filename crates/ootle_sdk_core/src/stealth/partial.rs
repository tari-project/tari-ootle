//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The opaque output of stealth statement assembly.
//!
//! [`StealthPartialTransaction`] pairs the assembled [`UnsignedTransaction`] with the
//! [`StealthSignatureRequirementsState`] the sign/seal path needs to drive signing/sealing. Like
//! [`crate::inputs::PartialTransaction`], it is a **handle**, not a wire record — it deliberately
//! does **not** derive `Serialize`/`Deserialize` (the signing state never crosses the boundary; the
//! core does the signing and emits submit-ready bytes).

use tari_ootle_transaction::UnsignedTransaction;

use crate::inputs::StealthSignerEntry;

/// The signing-requirements state accumulated by the input resolver and finalized here at assembly.
///
/// **Opaque / not `Serialize`** — this never crosses the boundary. The sign/seal path consumes it to
/// select the signing keys (account-key seal, stealth `c+k` seal, or an ephemeral key). No boundary
/// mirror is introduced: the core performs the signing itself, so no host needs to inspect the
/// requirements. If a host ever needs to, a read-only view can be added then without touching this
/// internal type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StealthSignatureRequirementsState {
    /// Whether the account key must seal the transaction (set from `intent.revealed_input_amount > 0`
    /// or `intent.pay_fee_from_revealed`). When `true`, every spend signer is a *required* signer (no
    /// implicit seal promotion) and the seal is the account key.
    pub must_sign_with_account_key: bool,
    /// The explicit stealth seal signer — the first non-account-key-signed input promoted to seal
    /// with its one-time `c+k` key. `None` when the account key seals, or when there are no inputs
    /// (ephemeral seal).
    pub seal_signer: Option<StealthSignerEntry>,
    /// All other required signers, in resolution order. Each must authorize spending its input with
    /// its stealth-derived key.
    pub other_signers: Vec<StealthSignerEntry>,
}

impl StealthSignatureRequirementsState {
    /// The transaction may be sealed with a freshly-generated **ephemeral** key (maximizing privacy)
    /// only when nothing forces a specific signer — no account-key seal, no seal signer, and no other
    /// required signers.
    pub fn can_sign_with_ephemeral_key(&self) -> bool {
        !self.must_sign_with_account_key && self.other_signers.is_empty() && self.seal_signer.is_none()
    }

    /// Iterate the required signers that still need an authorization signature, skipping the first
    /// one when it was implicitly promoted to the seal signer
    /// (`must_sign_with_account_key == false` AND no explicit `seal_signer` was set — the seal *is*
    /// the first required signer in that case, so it must not be double-counted).
    pub fn other_signers_iter(&self) -> impl Iterator<Item = &StealthSignerEntry> {
        let skip = usize::from(!self.must_sign_with_account_key && self.seal_signer.is_none());
        self.other_signers.iter().skip(skip)
    }
}

/// The opaque assembly output: the assembled unsigned transaction plus the signing requirements the
/// sign/seal path consumes. No `Serialize` — matches the [`crate::inputs::PartialTransaction`] handle
/// pattern.
#[derive(Debug)]
pub struct StealthPartialTransaction {
    pub(crate) unsigned: UnsignedTransaction,
    pub(crate) sig_reqs: StealthSignatureRequirementsState,
}

impl StealthPartialTransaction {
    /// Borrows the assembled unsigned transaction (the sign/seal path signs/seals/encodes this).
    pub fn unsigned(&self) -> &UnsignedTransaction {
        &self.unsigned
    }

    /// Consumes the handle, yielding the unsigned transaction and the signing requirements.
    pub fn into_parts(self) -> (UnsignedTransaction, StealthSignatureRequirementsState) {
        (self.unsigned, self.sig_reqs)
    }

    /// Borrows the signing requirements (the sign/seal path drives key selection from these).
    pub fn signature_requirements(&self) -> &StealthSignatureRequirementsState {
        &self.sig_reqs
    }
}
