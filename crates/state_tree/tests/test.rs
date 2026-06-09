//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
// Adapted from https://github.com/radixdlt/radixdlt-scrypto/blob/868ba44ec3b806992864af27c706968c797eb961/radix-engine-stores/src/hash_tree/test.rs

use std::collections::{BTreeSet, HashSet};

use tari_jellyfish::{SPARSE_MERKLE_PLACEHOLDER_HASH, SparseMerkleProofExt, StaleTreeNode, TreeHash, Version};
use tari_ootle_common_types::ToSubstateAddress;
use tari_state_tree::memory_store::MemoryTreeStore;

use crate::support::{HashTreeTester, change, hash_value_from_seed, make_value};
mod support;

#[test]
fn hash_of_next_version_differs_when_value_changed() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(1, Some(30))]);
    let hash_v2 = tester.put_substate_changes(vec![change(1, Some(70))]);
    assert_ne!(hash_v1, hash_v2);
}

#[test]
fn hash_of_next_version_same_when_write_repeated() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(4, Some(30)), change(3, Some(40))]);
    let hash_v2 = tester.put_substate_changes(vec![change(4, Some(30))]);
    assert_eq!(hash_v1, hash_v2);
}

#[test]
fn hash_of_next_version_same_when_write_empty() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(1, Some(30)), change(3, Some(40))]);
    let hash_v2 = tester.put_substate_changes(vec![]);
    assert_eq!(hash_v1, hash_v2);
}

#[test]
fn hash_of_next_version_differs_when_entry_added() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(1, Some(30))]);
    let hash_v2 = tester.put_substate_changes(vec![change(2, Some(30))]);
    assert_ne!(hash_v1, hash_v2);
}

#[test]
fn hash_of_next_version_differs_when_entry_removed() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(1, Some(30)), change(4, Some(20))]);
    let hash_v2 = tester.put_substate_changes(vec![change(1, None)]);
    assert_ne!(hash_v1, hash_v2);
}

#[test]
fn hash_returns_to_same_when_previous_state_restored() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![change(1, Some(30)), change(2, Some(40))]);
    tester.put_substate_changes(vec![change(1, Some(90)), change(2, None), change(3, Some(10))]);
    let hash_v3 = tester.put_substate_changes(vec![change(1, Some(30)), change(2, Some(40)), change(3, None)]);
    assert_eq!(hash_v1, hash_v3);
}

#[test]
fn hash_computed_consistently_after_higher_tier_leafs_deleted() {
    // Compute a "reference" hash of state containing simply [2:3:4, 2:3:5].
    let mut reference_tester = HashTreeTester::new_empty();
    let reference_root = reference_tester.put_substate_changes(vec![change(1, Some(234)), change(2, Some(235))]);

    // Compute a hash of the same state, at which we arrive after deleting some unrelated NodeId.
    let mut tester = HashTreeTester::new_empty();
    tester.put_substate_changes(vec![change(3, Some(162)), change(4, Some(163)), change(1, Some(234))]);
    tester.put_substate_changes(vec![change(3, None), change(4, None)]);
    let root_after_deletes = tester.put_substate_changes(vec![change(2, Some(235))]);

    // We did [3,4,1] - [3,4] + [2] = [1,2] (i.e. same state).
    assert_eq!(root_after_deletes, reference_root);
}

#[test]
fn hash_computed_consistently_after_adding_higher_tier_sibling() {
    // Compute a "reference" hash of state containing simply [1,2,3].
    let mut reference_tester = HashTreeTester::new_empty();
    let reference_root =
        reference_tester.put_substate_changes(vec![change(1, Some(196)), change(2, Some(234)), change(3, Some(235))]);

    // Compute a hash of the same state, at which we arrive after adding some sibling NodeId.
    let mut tester = HashTreeTester::new_empty();
    tester.put_substate_changes(vec![change(2, Some(234))]);
    tester.put_substate_changes(vec![change(1, Some(196))]);
    let root_after_adding_sibling = tester.put_substate_changes(vec![change(3, Some(235))]);

    // We did [2] + [1] + [3] = [1,2,3] (i.e. same state).
    assert_eq!(root_after_adding_sibling, reference_root);
}

#[test]
fn hash_allows_putting_in_same_version() {
    let mut tester_1 = HashTreeTester::new_empty();
    tester_1.put_changes_at_version(None, 1, vec![change(1, Some(30))]);
    tester_1.put_changes_at_version(Some(1), 1, vec![change(2, Some(31))]);
    tester_1.put_changes_at_version(Some(1), 1, vec![change(3, Some(32))]);
    tester_1.put_changes_at_version(Some(1), 1, vec![change(4, Some(33))]);
    tester_1.put_changes_at_version(Some(1), 1, vec![change(5, Some(34))]);
    let hash_1 = tester_1.put_changes_at_version(Some(1), 1, vec![change(6, Some(35))]);
    let mut tester_2 = HashTreeTester::new_empty();
    tester_2.put_changes_at_version(None, 1, vec![
        change(1, Some(30)),
        change(2, Some(31)),
        change(3, Some(32)),
    ]);
    let hash_2 = tester_2.put_changes_at_version(Some(1), 2, vec![
        change(4, Some(33)),
        change(5, Some(34)),
        change(6, Some(35)),
    ]);
    assert_eq!(hash_1, hash_2);
}

#[test]
fn hash_differs_when_states_only_differ_by_node_key() {
    let mut tester_1 = HashTreeTester::new_empty();
    let hash_1 = tester_1.put_substate_changes(vec![change(1, Some(30))]);
    let mut tester_2 = HashTreeTester::new_empty();
    let hash_2 = tester_2.put_substate_changes(vec![change(2, Some(30))]);
    assert_ne!(hash_1, hash_2);
}

#[test]
fn hash_differs_when_states_only_differ_by_value() {
    let mut tester_1 = HashTreeTester::new_empty();
    let hash_1 = tester_1.put_substate_changes(vec![change(1, Some(1))]);
    let mut tester_2 = HashTreeTester::new_empty();
    let hash_2 = tester_2.put_substate_changes(vec![change(1, Some(2))]);
    assert_ne!(hash_1, hash_2);
}

#[test]
fn supports_empty_state() {
    let mut tester = HashTreeTester::new_empty();
    let hash_v1 = tester.put_substate_changes(vec![]);
    assert_eq!(hash_v1, SPARSE_MERKLE_PLACEHOLDER_HASH);
    let hash_v2 = tester.put_substate_changes(vec![change(1, Some(30))]);
    assert_ne!(hash_v2, SPARSE_MERKLE_PLACEHOLDER_HASH);
    let hash_v3 = tester.put_substate_changes(vec![change(1, None)]);
    assert_eq!(hash_v3, SPARSE_MERKLE_PLACEHOLDER_HASH);
}

#[test]
fn records_stale_tree_node_keys() {
    let mut tester = HashTreeTester::new_empty();
    tester.put_substate_changes(vec![change(4, Some(30))]);
    tester.put_substate_changes(vec![change(3, Some(70))]);
    tester.put_substate_changes(vec![change(3, Some(80))]);
    let stale_versions = tester
        .tree_store
        .stale_nodes
        .iter()
        .map(|stale_part| {
            let StaleTreeNode::Node(key) = stale_part else {
                panic!("expected only single node removals");
            };
            key.version()
        })
        .collect::<BTreeSet<Version>>();
    assert_eq!(stale_versions.into_iter().collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn serialized_keys_are_strictly_increasing() {
    let mut tester = HashTreeTester::new(MemoryTreeStore::new(), None);
    tester.put_substate_changes(vec![change(3, Some(90))]);
    let previous_keys = tester.tree_store.nodes.keys().cloned().collect::<HashSet<_>>();
    tester.put_substate_changes(vec![change(1, Some(80))]);
    let min_next_key = tester
        .tree_store
        .nodes
        .keys()
        .filter(|key| !previous_keys.contains(*key))
        .max()
        .unwrap();
    let max_previous_key = previous_keys.iter().max().unwrap();
    assert!(min_next_key > max_previous_key);
}

#[test]
fn proofs() {
    let mut tester = HashTreeTester::new_empty();
    let root_v1 = tester.put_substate_changes(vec![change(1, Some(30))]);
    tester.put_substate_changes(vec![change(2, Some(40))]);
    let root_hash = tester.put_substate_changes(vec![change(3, Some(50))]);

    let tree = tester.create_state_tree();
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(1)).unwrap();
    let hash = hash_value_from_seed(30);
    assert_eq!(proof_value, Some((hash, make_value(1).to_substate_address(), 1)));
    proof.verify_inclusion(&root_hash, &key, &hash).unwrap();
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(2)).unwrap();
    let hash = hash_value_from_seed(40);
    assert_eq!(proof_value, Some((hash, make_value(2).to_substate_address(), 2)));
    proof.verify_inclusion(&root_hash, &key, &hash).unwrap();
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(3)).unwrap();
    let hash = hash_value_from_seed(50);
    assert_eq!(proof_value, Some((hash, make_value(3).to_substate_address(), 3)));
    proof.verify_inclusion(&root_hash, &key, &hash).unwrap();
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(3)).unwrap();
    proof
        .verify_inclusion(&root_hash, &key, &proof_value.unwrap().0)
        .unwrap();

    // Fail cases:
    // Fail to proof exclusion for included value
    let (key, _, proof) = tree.get_proof(3, &make_value(3)).unwrap();
    proof.verify_exclusion(&root_hash, &key).unwrap_err();
    // Fail to proof inclusion for excluded value
    let (key, _, proof) = tree.get_proof(3, &make_value(1)).unwrap();
    let hash = hash_value_from_seed(50);
    proof.verify_inclusion(&root_hash, &key, &hash).unwrap_err();
    // Fail to proof inclusion for old/incorrect merkle root
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(3)).unwrap();
    proof
        .verify_inclusion(&root_v1, &key, &proof_value.unwrap().0)
        .unwrap_err();

    // Exclusion proof
    let (key, proof_value, proof) = tree.get_proof(3, &make_value(4)).unwrap();
    assert!(proof_value.is_none());
    proof.verify_exclusion(&root_hash, &key).unwrap();

    // Fail to verify exclusion proof
    let (key, _, proof) = tree.get_proof(3, &make_value(4)).unwrap();
    let hash = hash_value_from_seed(50);
    proof.verify_inclusion(&root_hash, &key, &hash).unwrap_err();
}

// Builds the shard-group root tree the same way a validator does for a block header: an ephemeral
// tree over the ordered `[global, shard_0, shard_1, ...]` roots. The first shard hosts our substates.
fn shard_group_root_and_proof(shard_root: TreeHash) -> (TreeHash, SparseMerkleProofExt) {
    use tari_state_tree::{compute_merkle_root_for_hashes, compute_proof_for_hashes};
    // [global (empty), our shard, an unrelated sibling shard]
    let ordered_roots = vec![SPARSE_MERKLE_PLACEHOLDER_HASH, shard_root, hash_value_from_seed(99)];
    let group_root = compute_merkle_root_for_hashes(ordered_roots.clone()).unwrap();
    let (_, shard_root_proof) = compute_proof_for_hashes(ordered_roots.into_iter(), shard_root).unwrap();
    (group_root, shard_root_proof)
}

#[test]
fn two_level_substate_inclusion_proof() {
    use tari_state_tree::{SpreadPrefixStateTree, StateTreePayload, SubstateValueProof, memory_store::MemoryTreeStore};

    // Build a shard JMT with the production (SpreadPrefix) key mapper, as a real validator would.
    let mut store = MemoryTreeStore::<StateTreePayload>::new();
    SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(None, 1, vec![change(1, Some(30))])
        .unwrap();
    SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(Some(1), 2, vec![change(2, Some(40))])
        .unwrap();
    let shard_root = SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(Some(2), 3, vec![change(3, Some(50))])
        .unwrap();

    // Level 1: leaf proof for substate make_value(2) against the shard root.
    let (_key, proof_value, leaf_proof) = SpreadPrefixStateTree::new(&mut store)
        .get_proof(3, &make_value(2))
        .unwrap();
    let value_hash = proof_value.unwrap().0;

    // Level 2: shard root within the shard-group root.
    let (group_root, shard_root_proof) = shard_group_root_and_proof(shard_root);

    let proof = SubstateValueProof::new(shard_root, shard_root_proof, leaf_proof);

    // Verifies against the trusted group root.
    proof
        .verify_inclusion(&group_root, &make_value(2), &value_hash)
        .unwrap();
    // A tampered value hash is rejected (binds the value to the committed leaf).
    proof
        .verify_inclusion(&group_root, &make_value(2), &hash_value_from_seed(200))
        .unwrap_err();
    // A wrong group root is rejected (level-2 failure).
    proof
        .verify_inclusion(&hash_value_from_seed(7), &make_value(2), &value_hash)
        .unwrap_err();
    // An included substate cannot be proven absent.
    proof.verify_exclusion(&group_root, &make_value(2)).unwrap_err();
}

#[test]
fn two_level_substate_exclusion_proof() {
    use tari_state_tree::{SpreadPrefixStateTree, StateTreePayload, SubstateValueProof, memory_store::MemoryTreeStore};

    let mut store = MemoryTreeStore::<StateTreePayload>::new();
    SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(None, 1, vec![change(1, Some(30))])
        .unwrap();
    SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(Some(1), 2, vec![change(2, Some(40))])
        .unwrap();
    let shard_root = SpreadPrefixStateTree::new(&mut store)
        .put_substate_changes(Some(2), 3, vec![change(3, Some(50))])
        .unwrap();

    // make_value(4) was never created in this shard - the leaf proof is an exclusion proof.
    let (_key, proof_value, leaf_proof) = SpreadPrefixStateTree::new(&mut store)
        .get_proof(3, &make_value(4))
        .unwrap();
    assert!(proof_value.is_none());

    let (group_root, shard_root_proof) = shard_group_root_and_proof(shard_root);
    let proof = SubstateValueProof::new(shard_root, shard_root_proof, leaf_proof);

    // The absent substate is provably absent under the trusted group root...
    proof.verify_exclusion(&group_root, &make_value(4)).unwrap();
    // ...but cannot be proven present.
    proof
        .verify_inclusion(&group_root, &make_value(4), &hash_value_from_seed(50))
        .unwrap_err();
}
