//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod seconds {
    //! Helper module for serialising configuration variables from `Duration` to integers representing seconds and back.
    //! Use this converter by employing
    use core::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where D: Deserializer<'de> {
        Ok(Duration::from_secs(u64::deserialize(deserializer)?))
    }

    pub fn serialize<S>(duration: &Duration, s: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        s.serialize_u64(duration.as_secs())
    }
}

pub mod optional_seconds {
    //! Helper module for serialising configuration variables from `Duration` to integers representing seconds and back.
    //! Use this converter by employing
    //! ```ignore
    //! use ootle_serde::duration::optional_seconds;
    //! ...
    //! #[serde(with="optional_seconds")]
    //! pub my_var: Option<Duration>
    //! ```
    use core::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where D: Deserializer<'de> {
        match Option::<u64>::deserialize(deserializer)? {
            Some(d) => Ok(Some(Duration::from_secs(d))),
            None => Ok(None),
        }
    }

    pub fn serialize<S>(duration: &Option<Duration>, s: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        match duration {
            Some(d) => s.serialize_some(&d.as_secs()),
            None => s.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Config {
        #[serde(with = "super::seconds")]
        timeout: Duration,
        #[serde(with = "super::optional_seconds")]
        optional_timeout: Option<Duration>,
    }

    #[test]
    fn round_trips_duration_in_json_and_binary() {
        for sample in [
            Config {
                timeout: Duration::from_secs(30),
                optional_timeout: Some(Duration::from_secs(120)),
            },
            Config {
                timeout: Duration::from_secs(0),
                optional_timeout: None,
            },
        ] {
            let json = serde_json::to_string(&sample).unwrap();
            let decoded_json: Config = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded_json, sample);

            let binary = tari_bor::serde_codec::to_vec(&sample).unwrap();
            let decoded_bin: Config = tari_bor::serde_codec::from_slice(&binary).unwrap();
            assert_eq!(decoded_bin, sample);
        }
    }
}
