//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// We cannot currently use the "borsh" feature in indexmap with the "derive" feature of borsh as this creates a cyclic
// dependency.
// See https://github.com/bkchr/proc-macro-crate/issues/37#issuecomment-2476386861 for details.
pub mod indexmap {

    use borsh::BorshSerialize;
    use indexmap::IndexMap;

    pub fn serialize<K: borsh::ser::BorshSerialize, V: borsh::ser::BorshSerialize, W: borsh::io::Write>(
        obj: &IndexMap<K, V>,
        writer: &mut W,
    ) -> Result<(), borsh::io::Error> {
        let len = obj.len() as u64;
        len.serialize(writer)?;
        for (key, value) in obj {
            key.serialize(writer)?;
            value.serialize(writer)?;
        }
        Ok(())
    }
}
