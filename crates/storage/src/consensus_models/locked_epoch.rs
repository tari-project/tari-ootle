//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use minicbor::{CborLen, Decode, Encode};
use tari_engine_types::fees::ExhaustBurnRate;
use tari_ootle_common_types::Epoch;
use tari_template_lib_types::Hash32;

/// The epoch a transaction executes in, and everything about that epoch its execution depends on.
///
/// Every field is read off the block being proposed or voted on, so a replica executes against what
/// the block commits to rather than against its own view of the epoch. That is what makes an
/// execution reproducible by a node that reaches the block later, including one that joined by state
/// sync and never witnessed the epoch open.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LockedEpoch {
    #[n(0)]
    epoch: Epoch,
    #[n(1)]
    hash: Hash32,
    /// The share of collected fees burned rather than paid to leaders, for the whole of `epoch`.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[serde(with = "exhaust_burn_rate_bps")]
    #[n(2)]
    #[cbor(with = "exhaust_burn_rate_codec")]
    exhaust_burn_rate: ExhaustBurnRate,
}

impl LockedEpoch {
    pub fn new(epoch: Epoch, hash: Hash32, exhaust_burn_rate: ExhaustBurnRate) -> Self {
        Self {
            epoch,
            hash,
            exhaust_burn_rate,
        }
    }

    pub fn hash(&self) -> &Hash32 {
        &self.hash
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn exhaust_burn_rate(&self) -> ExhaustBurnRate {
        self.exhaust_burn_rate
    }

    pub fn destructure(self) -> (Epoch, Hash32, ExhaustBurnRate) {
        (self.epoch, self.hash, self.exhaust_burn_rate)
    }
}

impl Display for LockedEpoch {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LockedEpoch({}, {}, burn {}bps)",
            self.epoch,
            self.hash,
            self.exhaust_burn_rate.as_bps()
        )
    }
}

/// Reads and writes the rate as its basis points, so a value above the ceiling is a deserialization
/// error naming the rate rather than something [`ExhaustBurnRate::new`] would panic on.
// `serde(with)` fixes these signatures, so the reference is not this module's to choose.
#[allow(clippy::trivially_copy_pass_by_ref)]
mod exhaust_burn_rate_bps {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use super::*;

    pub fn serialize<S: Serializer>(rate: &ExhaustBurnRate, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u16(rate.as_bps())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ExhaustBurnRate, D::Error> {
        let bps = u16::deserialize(d)?;
        ExhaustBurnRate::try_new(bps)
            .ok_or_else(|| D::Error::custom(format!("exhaust burn rate {bps} is above the ceiling")))
    }
}

/// Encodes the rate as its basis points, so a value above the ceiling is a decode error naming the
/// rate rather than something [`ExhaustBurnRate::new`] would panic on.
// `cbor(with)` fixes these signatures, so the reference is not this module's to choose.
#[allow(clippy::trivially_copy_pass_by_ref)]
mod exhaust_burn_rate_codec {
    use super::*;

    pub fn encode<C, W: minicbor::encode::Write>(
        rate: &ExhaustBurnRate,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.u16(rate.as_bps())?;
        Ok(())
    }

    pub fn decode<'b, C>(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<ExhaustBurnRate, minicbor::decode::Error> {
        let bps = d.u16()?;
        ExhaustBurnRate::try_new(bps)
            .ok_or_else(|| minicbor::decode::Error::message(format!("exhaust burn rate {bps} is above the ceiling")))
    }

    pub fn cbor_len<C>(rate: &ExhaustBurnRate, ctx: &mut C) -> usize {
        minicbor::CborLen::cbor_len(&rate.as_bps(), ctx)
    }
}
