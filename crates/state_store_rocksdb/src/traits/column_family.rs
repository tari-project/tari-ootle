//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::codecs::{DbCodec, PrefixCodec, Prefixed};

pub trait Cf {
    type Key: 'static;

    type Prefix: Prefixed;

    type KeyCodec: Default + DbCodec<Self::Key>;
    type Value: 'static;
    type ValueCodec: Default + DbCodec<Self::Value>;

    fn name() -> &'static str;

    fn as_name(&self) -> &'static str {
        Self::name()
    }

    fn key_codec() -> PrefixCodec<Self::Prefix, Self::KeyCodec> {
        PrefixCodec::<Self::Prefix, Self::KeyCodec>::default()
    }

    fn value_codec() -> Self::ValueCodec {
        Self::ValueCodec::default()
    }

    fn key_prefix() -> Option<u8> {
        Self::Prefix::prefix()
    }
}

pub type PrefixedCodec<TCf> = PrefixCodec<<TCf as Cf>::Prefix, <TCf as Cf>::KeyCodec>;

pub trait QueryCf {
    type Cf: Cf;
    // TODO: ideally we dont restrict to 'static here since that is antithetical to a query
    type Key: 'static;

    type KeyCodec: Default + DbCodec<Self::Key>;

    fn make_cf_key_codec() -> PrefixCodec<<Self::Cf as Cf>::Prefix, <Self::Cf as Cf>::KeyCodec> {
        Self::Cf::key_codec()
    }

    fn make_cf_value_codec() -> <Self::Cf as Cf>::ValueCodec {
        Self::Cf::value_codec()
    }
}

impl<T: QueryCf> Cf for T {
    type Key = T::Key;
    type KeyCodec = T::KeyCodec;
    type Prefix = <T::Cf as Cf>::Prefix;
    type Value = <T::Cf as Cf>::Value;
    type ValueCodec = <T::Cf as Cf>::ValueCodec;

    fn name() -> &'static str {
        <T::Cf as Cf>::name()
    }
}
