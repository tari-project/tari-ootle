//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Decoder, Encode, data::Type, decode};
use tari_bor::adapters::boxed_slice;
use tari_template_abi::rust::prelude::*;

use super::ViewableBalanceProof;
use crate::{
    EncryptedData,
    FunctionName,
    Hash32,
    TemplateAddress,
    access_rules::AccessRule,
    bytes::Bytes,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
};

/// An unspent output that does not reveal the value and the owner of the coin it represents.
///
/// Unspent outputs contain:
/// - **commitment** - the Pedersen commitment k.G + v.H
/// - **sender_public_nonce** - the sender-provided public nonce that is used as part of a DH key exchange to generate
///   the decryption key for the encrypted data.
/// - **encrypted_data** - the encrypted data that contains the encrypted mask and value.
/// - **viewable_balance_proof** - an optional verifiable balance proof that must be provided and valid if the view key
///   is enabled for a resource.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct UnspentOutput {
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
    /// Public nonce (R) that was used to generate the commitment mask
    #[n(1)]
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    /// Encrypted mask and value for the recipient.
    #[n(2)]
    pub encrypted_data: EncryptedData,
    #[n(3)]
    pub minimum_value_promise: u64,
    /// If the view key is enabled for a given resource, this proof MUST be provided, otherwise it MUST NOT.
    #[n(4)]
    pub viewable_balance_proof: Option<ViewableBalanceProof>,
}

/// How a stealth output is authorised at spend time (TIP-0006): a key path, a committed condition tree, or both.
///
/// Modelled as an enum rather than a pair of `Option`s so the unspendable `{no key, no conditions}` state is
/// unrepresentable — by construction in memory and at the decode boundary, with no runtime invariant to enforce.
///
/// - **Key** — a one-time "stealth" public key; spendable by proving ownership of it (a signature, via the
///   transaction's auth scope).
/// - **Script** — the Merkle root (MAST) committing the set of alternative [`SpendCondition`] leaves; spendable by
///   revealing one leaf plus an inclusion proof.
/// - **KeyAndScript** — either path is admissible; the per-input `SpendWitness` selects which.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum SpendAuthorization {
    #[n(0)]
    Key(#[n(0)] RistrettoPublicKeyBytes),
    #[n(1)]
    Script(#[n(0)] Hash32),
    #[n(2)]
    KeyAndScript {
        #[n(0)]
        spend_key: RistrettoPublicKeyBytes,
        #[n(1)]
        condition_root: Hash32,
    },
}

impl SpendAuthorization {
    /// The one-time key authorising a key-path spend, if this output has a key path.
    pub fn spend_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::Key(key) | Self::KeyAndScript { spend_key: key, .. } => Some(key),
            Self::Script(_) => None,
        }
    }

    /// The committed condition-tree root authorising a script-path spend, if this output has a condition tree.
    pub fn condition_root(&self) -> Option<&Hash32> {
        match self {
            Self::Script(root) |
            Self::KeyAndScript {
                condition_root: root, ..
            } => Some(root),
            Self::Key(_) => None,
        }
    }
}

/// A stealth unspent output, authorised at spend time per its [`SpendAuthorization`] (TIP-0006).
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct StealthUnspentOutput {
    #[n(0)]
    pub output: UnspentOutput,
    #[n(1)]
    pub auth: SpendAuthorization,
    #[n(2)]
    pub tag: UtxoTag,
}

impl StealthUnspentOutput {
    pub fn commitment(&self) -> &PedersenCommitmentBytes {
        &self.output.commitment
    }
}

/// A spend condition leaf (v0) committed in a [`StealthUnspentOutput::condition_root`] tree. A script-path spend
/// reveals one leaf and an inclusion proof; the engine recomputes the root and, on a match, evaluates the leaf.
///
/// `Decode` is hand-written (see below) rather than derived: the `All` variant is self-recursive, so a derived decode
/// would recurse on the call stack and a deeply nested adversarial payload could overflow it during decode — before
/// any validation runs. The hand-written decode threads a depth counter bounded by [`tari_bor::MAX_DECODE_DEPTH`].
#[derive(Debug, Clone, Encode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SpendCondition {
    /// Spend is gated on a native access rule evaluated against the transaction's auth scope.
    #[n(0)]
    AccessRule(#[n(0)] AccessRule),
    /// Spend is gated on a stateless WASM predicate over the spending transfer. The referenced template function
    /// introspects the spending `StealthTransferStatement` and rejects the spend by panicking.
    #[n(1)]
    TemplateFunction(#[n(0)] TemplateFunction),
    /// Spend is gated on a native [`BuiltinPredicate`] — a consensus-fixed primitive (timelock, covenant, hashlock)
    /// that needs no deployed template and is evaluated natively over the spending transfer.
    #[n(2)]
    Builtin(#[n(0)] BuiltinPredicate),
    /// Conjunction: the spend is admissible only if every nested condition holds (logical AND). The condition tree
    /// itself is the disjunction (each leaf is an alternative spend path), so this is the only combinator a leaf needs.
    /// A conjunction must be non-empty and flat — a nested `All` is rejected at spend time, bounding evaluation depth.
    #[n(3)]
    All(
        #[n(0)]
        #[cbor(with = "boxed_slice")]
        Box<[SpendCondition]>,
    ),
}

impl SpendCondition {
    pub const fn as_template_function(&self) -> Option<&TemplateFunction> {
        match self {
            Self::TemplateFunction(func) => Some(func),
            _ => None,
        }
    }

    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All(_))
    }

    pub const fn is_template_function(&self) -> bool {
        matches!(self, Self::TemplateFunction(_))
    }

    /// Whether this condition is a builtin that reads the witness `data` blob as its own complete input. Such a
    /// builtin owns the whole blob, so it must be the sole `data` consumer in its leaf (enforced at spend time).
    pub const fn is_data_owning_builtin(&self) -> bool {
        matches!(self, Self::Builtin(predicate) if predicate.consumes_data())
    }
}

impl<'b, C> Decode<'b, C> for SpendCondition {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, decode::Error> {
        decode_spend_condition(d, ctx, 0)
    }
}

fn decode_spend_condition<'b, C>(
    d: &mut Decoder<'b>,
    ctx: &mut C,
    depth: usize,
) -> Result<SpendCondition, decode::Error> {
    if depth >= tari_bor::MAX_DECODE_DEPTH {
        return Err(decode::Error::message(
            "SpendCondition nesting exceeds the maximum decode depth",
        ));
    }

    // minicbor's derived enum framing is a definite 2-element array of `[variant index, body]`.
    let pos = d.position();
    if d.array()? != Some(2) {
        return Err(decode::Error::message("expected SpendCondition enum (2-element array)").at(pos));
    }
    let variant_pos = d.position();
    match d.i64()? {
        0 => decode_variant_field(d, ctx, |d, ctx| AccessRule::decode(d, ctx)).map(SpendCondition::AccessRule),
        1 => decode_variant_field(d, ctx, |d, ctx| TemplateFunction::decode(d, ctx))
            .map(SpendCondition::TemplateFunction),
        2 => decode_variant_field(d, ctx, |d, ctx| BuiltinPredicate::decode(d, ctx)).map(SpendCondition::Builtin),
        3 => decode_variant_field(d, ctx, |d, ctx| decode_condition_slice(d, ctx, depth + 1)).map(SpendCondition::All),
        n => Err(decode::Error::unknown_variant(n).at(variant_pos)),
    }
}

// Decodes one `All` boxed slice, recursing at the incremented depth. Reuses the `boxed_slice` adapter's element loop
// (and its `MAX_PREALLOC` cap) via a depth-threading decoder.
fn decode_condition_slice<'b, C>(
    d: &mut Decoder<'b>,
    ctx: &mut C,
    depth: usize,
) -> Result<Box<[SpendCondition]>, decode::Error> {
    boxed_slice::decode_with_fn(d, ctx, |d, ctx| decode_spend_condition(d, ctx, depth))
}

// Every `SpendCondition` variant carries a single field at index 0. minicbor's derive encodes a variant body as an
// array; read the field at position 0 and skip any trailing positions, handling the indefinite-length form.
fn decode_variant_field<'b, C, T>(
    d: &mut Decoder<'b>,
    ctx: &mut C,
    decode_field: impl FnOnce(&mut Decoder<'b>, &mut C) -> Result<T, decode::Error>,
) -> Result<T, decode::Error> {
    let pos = d.position();
    match d.array()? {
        Some(0) => Err(decode::Error::missing_value(0).at(pos)),
        Some(len) => {
            let field = decode_field(d, ctx)?;
            for _ in 1..len {
                d.skip()?;
            }
            Ok(field)
        },
        None => {
            if matches!(d.datatype()?, Type::Break) {
                return Err(decode::Error::missing_value(0).at(pos));
            }
            let field = decode_field(d, ctx)?;
            while !matches!(d.datatype()?, Type::Break) {
                d.skip()?;
            }
            d.skip()?;
            Ok(field)
        },
    }
}

/// A stateless WASM predicate that gates the spend of a stealth UTXO.
///
/// The condition fully commits to `{template, function, args}`, so a spender cannot substitute a different predicate.
/// Templates are immutable substates, so the referenced code cannot change once the output is committed.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct TemplateFunction {
    /// The template providing the predicate.
    #[n(0)]
    pub template: TemplateAddress,
    /// The stateless (`is_mut == false`) predicate function on that template.
    #[n(1)]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub function: FunctionName,
    /// Bound parameters, positional — one CBOR-encoded value per leading (non-`SpendContext`) parameter, matching the
    /// engine's `Vec<Bytes>` call ABI. Baked into the condition at creation; the spender cannot alter them.
    #[n(2)]
    pub args: Vec<Bytes>,
}

impl TemplateFunction {
    pub fn new(template: TemplateAddress, function: FunctionName, args: Vec<Bytes>) -> Self {
        Self {
            template,
            function,
            args,
        }
    }
}

/// A native, consensus-fixed spend predicate committed as a [`SpendCondition::Builtin`] leaf (TIP-0006).
///
/// Unlike a [`TemplateFunction`], a builtin requires no deployed template and is evaluated natively by the engine —
/// the canonical semantics live in trusted core code, so the set is append-only and a shipped variant is never
/// resemanticised. Each variant is a pure boolean predicate over the spending transfer; rejecting a spend leaves its
/// inputs unspent. A predicate that needs spender-supplied data reads the witness `data` blob
/// ([`SpendWitness::ScriptPath`](super::SpendWitness::ScriptPath)) as its complete raw input — see
/// [`BuiltinPredicate::consumes_data`] and the sole-consumer rule it implies.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum BuiltinPredicate {
    /// Absolute epoch lock: admissible only once the current epoch is at or after `unlock_epoch`.
    #[n(0)]
    AfterEpoch(#[n(0)] u64),
    /// Absolute epoch deadline: admissible only while the current epoch is strictly before `deadline_epoch`.
    #[n(1)]
    BeforeEpoch(#[n(0)] u64),
    /// "Stay in the vault" covenant: every stealth output of the transfer must preserve the invoking `condition_root`,
    /// and there must be at least one such output.
    #[n(2)]
    OutputPreservesCondition,
    /// Value-flow covenant: at least one stealth output must commit `condition_root` and promise at least `min_value`.
    #[n(3)]
    OutputTo {
        #[n(0)]
        condition_root: Hash32,
        #[n(1)]
        min_value: u64,
    },
    /// Value-conservation covenant (TIP-0006 Option A/C): the invoking partition's committed value is conserved into
    /// outputs carrying its `condition_root`, save for an exact cleartext outflow of at most `max_revealed`. A
    /// `max_revealed` of zero admits no cleartext escape (full conservation).
    #[n(4)]
    BalancePreserved(#[n(0)] u64),
    /// Hashlock: the witness `data` blob must be a preimage whose `alg` digest equals `hash`. As a data-consuming
    /// builtin it reads the entire blob as raw bytes, so it must be the sole `data` consumer in its leaf.
    ///
    /// A bare hashlock is satisfiable by anyone who learns the preimage; pair it with an [`AccessRule`] inside an
    /// [`SpendCondition::All`] to bind the claim to a key (the standard HTLC construction).
    #[n(5)]
    HashLock {
        #[n(0)]
        hash: Hash32,
        #[n(1)]
        alg: HashAlg,
    },
}

impl BuiltinPredicate {
    /// Whether this predicate reads the witness `data` blob (as raw bytes) as its complete input. Such a predicate
    /// cannot know the blob's shape relative to siblings, so it owns the whole blob and must be the sole `data`
    /// consumer in its leaf.
    pub const fn consumes_data(&self) -> bool {
        match self {
            Self::HashLock { .. } => true,
            Self::AfterEpoch(_) |
            Self::BeforeEpoch(_) |
            Self::OutputPreservesCondition |
            Self::OutputTo { .. } |
            Self::BalancePreserved(_) => false,
        }
    }
}

/// The digest used by a [`BuiltinPredicate::HashLock`]. The preimage is hashed with no domain separation so a hashlock
/// can interoperate with an external chain's HTLC (e.g. Bitcoin's `SHA256`).
#[derive(Debug, Clone, Copy, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub enum HashAlg {
    #[n(0)]
    Blake2b256,
    #[n(1)]
    Sha256,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> SpendCondition {
        SpendCondition::Builtin(BuiltinPredicate::AfterEpoch(0))
    }

    // Wraps `leaf()` in `depth` nested `All` conjunctions.
    fn nested_all(depth: usize) -> SpendCondition {
        let mut cond = leaf();
        for _ in 0..depth {
            cond = SpendCondition::All(Box::new([cond]));
        }
        cond
    }

    #[test]
    fn spend_condition_roundtrips_all_variants() {
        let cases = [
            SpendCondition::AccessRule(AccessRule::AllowAll),
            SpendCondition::TemplateFunction(TemplateFunction::new(
                Hash32::from_array([0u8; 32]),
                FunctionName::try_from("transfer").unwrap(),
                vec![Bytes::from_vec(vec![1, 2, 3])],
            )),
            SpendCondition::Builtin(BuiltinPredicate::AfterEpoch(7)),
            SpendCondition::All(Box::new([leaf(), SpendCondition::AccessRule(AccessRule::AllowAll)])),
            nested_all(4),
        ];
        for cond in cases {
            let bytes = tari_bor::encode(&cond).unwrap();
            let decoded: SpendCondition = tari_bor::decode(&bytes).unwrap();
            assert_eq!(decoded, cond, "hand-written decode must match derived encode");
        }
    }

    #[test]
    fn spend_condition_decodes_up_to_max_depth() {
        // One level below the limit decodes and round-trips; at the limit it is rejected.
        let ok = nested_all(tari_bor::MAX_DECODE_DEPTH - 1);
        let bytes = tari_bor::encode(&ok).unwrap();
        assert_eq!(tari_bor::decode::<SpendCondition>(&bytes).unwrap(), ok);

        let too_deep = nested_all(tari_bor::MAX_DECODE_DEPTH);
        let bytes = tari_bor::encode(&too_deep).unwrap();
        assert!(tari_bor::decode::<SpendCondition>(&bytes).is_err());
    }

    #[test]
    fn spend_condition_rejects_deeply_nested_payload_without_overflow() {
        // A tiny-per-level payload that a recursive decode without a depth bound would overflow the stack on (a process
        // abort). Each `All` level is the 4 bytes `82 03 81 81` (variant index 3). The depth guard rejects it as a
        // clean error long before the recursion can overflow.
        let bytes: Vec<u8> = (0..100_000).flat_map(|_| [0x82u8, 0x03, 0x81, 0x81]).collect();
        assert!(tari_bor::decode::<SpendCondition>(&bytes).is_err());
    }
}
