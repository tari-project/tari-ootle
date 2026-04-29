//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_export]
macro_rules! address {
    ($s:expr) => {
        <$crate::OotleAddress as core::str::FromStr>::from_str($s).expect("Invalid address string literal")
    };
}

#[cfg(test)]
mod tests {
    use ootle_network::Network;

    #[test]
    fn it_produces_a_address_from_string() {
        let addr = address!(
            "otl_loc_1nsy5c5mfn7jgmg5nm3s3m4vr829tgpeehmymkme6k5wszde6wc7zcfwtyyxwn62tqefqyfyjangalt4zrygzwyf8c6c2jtfqyd8dk0gtwcv5x"
        );
        assert_eq!(addr.network(), Network::LocalNet);
    }
}
