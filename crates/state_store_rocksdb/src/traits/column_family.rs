//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::codecs::DbCodec;

pub trait Cf {
    type Key;

    type KeyCodec: Default + DbCodec<Self::Key>;
    type Value;
    type ValueCodec: Default + DbCodec<Self::Value>;

    fn name() -> &'static str;

    fn as_name(&self) -> &'static str {
        Self::name()
    }

    fn key_codec() -> Self::KeyCodec {
        Self::KeyCodec::default()
    }

    fn value_codec() -> Self::ValueCodec {
        Self::ValueCodec::default()
    }
}

pub trait QueryCf {
    type Cf: Cf;
    type Key;

    type KeyCodec: Default + DbCodec<Self::Key>;

    fn make_cf_key_codec() -> <Self::Cf as Cf>::KeyCodec {
        <Self::Cf as Cf>::KeyCodec::default()
    }

    fn make_cf_value_codec() -> <Self::Cf as Cf>::ValueCodec {
        <Self::Cf as Cf>::ValueCodec::default()
    }
}

impl<T: QueryCf> Cf for T {
    type Key = T::Key;
    type KeyCodec = T::KeyCodec;
    type Value = <T::Cf as Cf>::Value;
    type ValueCodec = <T::Cf as Cf>::ValueCodec;

    fn name() -> &'static str {
        <T::Cf as Cf>::name()
    }
}
