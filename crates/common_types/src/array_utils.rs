//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub fn copy_fixed_checked<const SZ: usize, T>(bytes: &[u8]) -> Option<T>
where [u8; SZ]: Into<T> {
    if bytes.len() != SZ {
        return None;
    }
    let mut array = [0u8; SZ];
    array.copy_from_slice(&bytes[..SZ]);
    Some(array.into())
}
