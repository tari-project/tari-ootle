//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::{Range, RangeFrom, RangeTo};

/// A subset of RangeBounds that are possible to query in RocksDB.
pub enum QueryRange<B> {
    // start..end
    Exclusive { start: B, end: B },
    // start..
    From { start: B },
    // ..end
    To { end: B },
}

impl<B> From<Range<B>> for QueryRange<B> {
    fn from(range: Range<B>) -> Self {
        QueryRange::Exclusive {
            start: range.start,
            end: range.end,
        }
    }
}

impl<B> From<RangeFrom<B>> for QueryRange<B> {
    fn from(range: RangeFrom<B>) -> Self {
        QueryRange::From { start: range.start }
    }
}

impl<B> From<RangeTo<B>> for QueryRange<B> {
    fn from(range: RangeTo<B>) -> Self {
        QueryRange::To { end: range.end }
    }
}
