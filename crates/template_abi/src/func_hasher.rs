//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::FunctionIdent;

/// The seed used for hashing function names. Selected arbitrarily. (TARI_OOTL)
/// This value must not change, as it would change all function identifiers.
const FUNC_HASHER_SEED: u32 = 0x7A21_0071;

pub fn hash_function_name(func: &str) -> FunctionIdent {
    xxhash_rust::xxh32::xxh32(func.as_bytes(), FUNC_HASHER_SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_deterministic() {
        let func_name = "my_function";
        let hash1 = hash_function_name(func_name);
        let hash2 = hash_function_name(func_name);
        assert_eq!(hash1, hash2, "Hashes for the same function name should be equal");

        let different_func_name = "another_function";
        let hash3 = hash_function_name(different_func_name);
        assert_ne!(hash1, hash3, "Hashes for different function names should not be equal");
    }

    #[test]
    fn it_generates_known_hash() {
        // This test ensures that the hash function remains stable.
        // If you need to change the hashing algorithm or seed, this test will fail.
        // In that case, you must update the expected hash value and consider the breaking change implications.
        let func_name = "my_function";
        let hash = hash_function_name(func_name);
        assert_eq!(
            hash, 3104460920,
            "Function hash for 'my_function' has changed. This is a breaking change."
        );
    }
}
