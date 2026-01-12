//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use jmt::storage::NodeKey;

pub fn write_node_key<W: fmt::Write>(f: &mut W, node_key: &NodeKey) -> fmt::Result {
    write!(f, "{}:", node_key.version())?;
    for n in node_key.nibble_path().nibbles() {
        // NOTE: we know the nibble is represented as a u8, so we can safely cast it
        write!(f, "{:02x}", n.as_usize() as u8)?;
    }
    Ok(())
}
