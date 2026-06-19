//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

//! Merklized script tree (MAST) for stealth spend-time scripts (TIP-0006).
//!
//! A stealth output commits to a set of alternative spend conditions via a single 32-byte
//! `script_root`. At spend time the spender reveals one leaf plus an inclusion proof; the engine
//! recomputes the root and checks it against the committed value. Hashing is native-only — a
//! template never builds or verifies a tree.
//!
//! Consensus-critical invariants:
//! - **Domain separation.** Leaves and branches hash under distinct domains ([`EngineHashDomainLabel::SpendScriptLeaf`]
//!   / [`EngineHashDomainLabel::SpendScriptBranch`]), so an internal node can never be reinterpreted as a leaf.
//! - **Lexicographic pair ordering.** A branch hashes its two children in byte order, so an inclusion proof is a bare
//!   list of sibling hashes with no left/right direction bits.
//! - **Canonical set commitment.** Leaf hashes are sorted and must be unique, so the root is a function of the *set* of
//!   leaves, independent of authoring order.
//! - **Promote-unpaired.** An odd node at a level is carried up unchanged. Duplicating it instead would let a tree with
//!   a repeated node collide with a smaller tree's root (CVE-2012-2459).

use borsh::BorshSerialize;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_template_lib::types::Hash32;

use crate::hashing::{EngineHashDomainLabel, hasher32};

/// Hashes a single leaf `(id, payload)` into the leaf domain.
///
/// `id` is the leaf type/version discriminant; `payload` is the leaf body, committed via its
/// canonical Borsh encoding. The fixed-width `id` is chained first, so `(id, payload)` is
/// unambiguous and `id` selects how `payload` is interpreted.
pub fn leaf_hash<P: BorshSerialize + ?Sized>(id: u32, payload: &P) -> Hash32 {
    hasher32(EngineHashDomainLabel::SpendScriptLeaf)
        .chain(&id)
        .chain(payload)
        .result()
}

/// Hashes two child nodes into a branch, ordering them lexicographically so a proof needs no
/// direction bits.
fn branch_hash(a: Hash32, b: Hash32) -> Hash32 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    hasher32(EngineHashDomainLabel::SpendScriptBranch)
        .chain(&lo)
        .chain(&hi)
        .result()
}

/// Folds one level into the next: adjacent pairs become branches, a trailing unpaired node is
/// promoted unchanged.
fn fold_level(level: &[Hash32]) -> Vec<Hash32> {
    level
        .chunks(2)
        .map(|pair| match pair {
            [left, right] => branch_hash(*left, *right),
            [unpaired] => *unpaired,
            _ => unreachable!("chunks(2) yields slices of length 1 or 2"),
        })
        .collect()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MerkleTreeError {
    #[error("script tree must contain at least one leaf")]
    Empty,
    #[error("script tree contains duplicate leaves")]
    DuplicateLeaf,
}

/// A canonical Merklized script tree over a set of distinct leaf hashes.
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// Sorted, unique leaf hashes.
    leaves: Vec<Hash32>,
}

impl MerkleTree {
    /// Builds a tree from leaf hashes, sorting into canonical order. Rejects an empty set or one
    /// containing duplicate leaves.
    pub fn new(leaf_hashes: impl IntoIterator<Item = Hash32>) -> Result<Self, MerkleTreeError> {
        let mut leaves: Vec<Hash32> = leaf_hashes.into_iter().collect();
        if leaves.is_empty() {
            return Err(MerkleTreeError::Empty);
        }
        leaves.sort_unstable();
        if leaves.windows(2).any(|w| w[0] == w[1]) {
            return Err(MerkleTreeError::DuplicateLeaf);
        }
        Ok(Self { leaves })
    }

    /// The committed root. For a single leaf this is the leaf hash itself.
    pub fn root(&self) -> Hash32 {
        let mut level = self.leaves.clone();
        while level.len() > 1 {
            level = fold_level(&level);
        }
        level[0]
    }

    /// Produces an inclusion proof for `leaf`, or `None` if it is not in the tree.
    pub fn proof_for(&self, leaf: Hash32) -> Option<MerkleProof> {
        let mut idx = self.leaves.binary_search(&leaf).ok()?;
        let mut level = self.leaves.clone();
        let mut siblings = Vec::new();
        while level.len() > 1 {
            if let Some(&sibling) = level.get(idx ^ 1) {
                siblings.push(sibling);
            }
            level = fold_level(&level);
            idx /= 2;
        }
        Some(MerkleProof { siblings })
    }
}

/// An inclusion proof: the sibling hashes from a leaf up to the root, bottom-first.
///
/// Carries no direction bits — [`branch_hash`] re-sorts each pair on the way up. The type is pure
/// data (no hashing), so it can move to `tari_template_lib_types` beside the spend witness while the
/// hashing stays here in native code.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, CborLen, Serialize, Deserialize)]
pub struct MerkleProof {
    #[n(0)]
    pub siblings: Vec<Hash32>,
}

impl MerkleProof {
    /// Recomputes the root that `leaf` and this proof attest to. The caller compares the result
    /// against the committed `script_root`.
    pub fn compute_root(&self, leaf: Hash32) -> Hash32 {
        self.siblings
            .iter()
            .fold(leaf, |acc, &sibling| branch_hash(acc, sibling))
    }
}

/// Verifies that `leaf` is committed in `root` under `proof`.
pub fn verify_inclusion(leaf: Hash32, proof: &MerkleProof, root: Hash32) -> bool {
    proof.compute_root(leaf) == root
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct, deterministic leaves for tree-shape tests.
    fn leaves(n: u32) -> Vec<Hash32> {
        (0..n).map(|i| leaf_hash(i, &i.to_le_bytes())).collect()
    }

    fn tree(n: u32) -> MerkleTree {
        MerkleTree::new(leaves(n)).unwrap()
    }

    #[test]
    fn round_trips_for_every_leaf_across_sizes() {
        // 3, 5 and 7 exercise the promote-unpaired path at multiple levels.
        for n in [1u32, 2, 3, 4, 5, 7, 8, 16, 31] {
            let t = tree(n);
            let root = t.root();
            for leaf in leaves(n) {
                let proof = t.proof_for(leaf).expect("leaf is in the tree");
                assert!(
                    verify_inclusion(leaf, &proof, root),
                    "n={n} failed to verify a committed leaf"
                );
            }
        }
    }

    #[test]
    fn single_leaf_root_is_the_leaf_with_an_empty_proof() {
        let only = leaf_hash(0, b"only");
        let t = MerkleTree::new([only]).unwrap();
        assert_eq!(t.root(), only);
        let proof = t.proof_for(only).unwrap();
        assert!(proof.siblings.is_empty());
        assert!(verify_inclusion(only, &proof, only));
    }

    #[test]
    fn root_is_independent_of_authoring_order() {
        let mut ls = leaves(6);
        let forward = MerkleTree::new(ls.clone()).unwrap().root();
        ls.reverse();
        let reversed = MerkleTree::new(ls).unwrap().root();
        assert_eq!(forward, reversed);
    }

    #[test]
    fn leaf_and_branch_domains_are_separated() {
        let a = leaf_hash(0, b"x");
        let b = leaf_hash(1, b"y");
        // A two-leaf root is a branch; it must not collide with a leaf hash of the same raw bytes.
        let root = MerkleTree::new([a, b]).unwrap().root();
        assert_ne!(root, leaf_hash(0, b"x"));
        assert_ne!(root, leaf_hash(1, b"y"));
    }

    #[test]
    fn non_member_has_no_proof() {
        let t = tree(4);
        let outsider = leaf_hash(999, b"not in tree");
        assert!(t.proof_for(outsider).is_none());
    }

    #[test]
    fn tampered_sibling_fails_verification() {
        let t = tree(5);
        let root = t.root();
        let leaf = leaves(5)[0];
        let mut proof = t.proof_for(leaf).unwrap();
        let mut bytes = proof.siblings[0].into_array();
        bytes[0] ^= 0xff;
        proof.siblings[0] = Hash32::from_array(bytes);
        assert!(!verify_inclusion(leaf, &proof, root));
    }

    #[test]
    fn wrong_root_fails_verification() {
        let t = tree(4);
        let leaf = leaves(4)[2];
        let proof = t.proof_for(leaf).unwrap();
        assert!(!verify_inclusion(leaf, &proof, Hash32::zero()));
    }

    #[test]
    fn rejects_empty_and_duplicate() {
        assert_eq!(MerkleTree::new([]).unwrap_err(), MerkleTreeError::Empty);
        let dup = leaf_hash(0, b"same");
        assert_eq!(MerkleTree::new([dup, dup]).unwrap_err(), MerkleTreeError::DuplicateLeaf);
    }

    #[test]
    fn proof_round_trips_through_cbor() {
        let t = tree(7);
        let leaf = leaves(7)[3];
        let proof = t.proof_for(leaf).unwrap();
        let encoded = minicbor::to_vec(&proof).unwrap();
        let decoded: MerkleProof = minicbor::decode(&encoded).unwrap();
        assert_eq!(proof, decoded);
        assert!(verify_inclusion(leaf, &decoded, t.root()));
    }
}
