//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::memo::Memo;
use tari_transaction_components::transaction_components::{MemoField, memo_field::TxType};

#[test]
fn memo_is_compatible_with_bl_memo_field() {
    let max_len_string = String::from_utf8(vec![b'a'; 254]).unwrap();

    let memo_field = MemoField::new_open_from_string(&max_len_string, TxType::Coinbase).unwrap();
    let memo_bytes = memo_field.to_bytes();

    let memo = Memo::decode_from(&mut memo_bytes.as_slice()).unwrap();
    assert_eq!(
        memo,
        Memo::new_bytes(memo_bytes[1..=Memo::MAX_BYTES_LENGTH].to_vec()).unwrap()
    );

    let u256 = 100_000u64.into();
    let memo_field = MemoField::new_u256(u256);
    let memo_bytes = memo_field.to_bytes();

    let memo = Memo::decode_from(&mut memo_bytes.as_slice()).unwrap();
    let mut buf = [0u8; 32];
    u256.to_little_endian(buf.as_mut());
    assert_eq!(memo, Memo::new_u256(buf));
}
