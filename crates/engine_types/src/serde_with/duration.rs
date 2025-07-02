//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod seconds {
    //! Helper module for serialising configuration variables from `Duration` to integers representing seconds and back.
    //! Use this converter by employing
    use std::time::Duration;

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
    //! use tari_engine_types::serde_with::duration::optional_seconds;
    //! ...
    //! #[serde(with="optional_seconds")]
    //! pub my_var: Option<Duration>
    //! ```
    use std::time::Duration;

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
            Some(d) => s.serialize_u64(d.as_secs()),
            None => s.serialize_none(),
        }
    }
}
