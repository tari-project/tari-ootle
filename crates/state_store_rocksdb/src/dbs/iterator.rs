//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Copyright 2020 Tyler Neely
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// TARI MODIFICATIONS:
// - remove allocated buffers on each iteration, instead passing slices that directly reference the data in RocksDB to a
//   "mapper" function. This is safe since we restrict the lifetime of the return type of the mapper to 'static, which
//   forces the caller to return owned data (i.e. no references to the slice). This allows us to avoid unnecessary
//   allocations and copying of data on each iteration, which can significantly improve performance when iterating over
//   large datasets.

use rocksdb::{DBAccess, DBRawIteratorWithThreadMode, Direction, IteratorMode};

/// A standard Rust [`Iterator`] over a database or column family.
///
/// As an alternative, [`DBRawIteratorWithThreadMode`] is a low level wrapper around
/// RocksDB's API, which can provide better performance and more features.
///
/// ```
/// use rocksdb::{DB, Direction, IteratorMode, Options};
///
/// let tempdir = tempfile::Builder::new()
///     .prefix("_path_for_rocksdb_storage2")
///     .tempdir()
///     .expect("Failed to create temporary path for the _path_for_rocksdb_storage2.");
/// let path = tempdir.path();
/// {
///     let db = DB::open_default(path).unwrap();
///     let mut iter = db.iterator(IteratorMode::Start); // Always iterates forward
///     for item in iter {
///         let (key, value) = item.unwrap();
///         println!("Saw {:?} {:?}", key, value);
///     }
///     iter = db.iterator(IteratorMode::End);  // Always iterates backward
///     for item in iter {
///         let (key, value) = item.unwrap();
///         println!("Saw {:?} {:?}", key, value);
///     }
///     iter = db.iterator(IteratorMode::From(b"my key", Direction::Forward)); // From a key in Direction::{forward,reverse}
///     for item in iter {
///         let (key, value) = item.unwrap();
///         println!("Saw {:?} {:?}", key, value);
///     }
///
///     // You can seek with an existing Iterator instance, too
///     iter = db.iterator(IteratorMode::Start);
///     iter.set_mode(IteratorMode::From(b"another key", Direction::Reverse));
///     for item in iter {
///         let (key, value) = item.unwrap();
///         println!("Saw {:?} {:?}", key, value);
///     }
/// }
/// let _ = DB::destroy(&Options::default(), path);
/// ```
pub struct DbRawKeyValueIterator<'a, D: DBAccess, M> {
    raw: DBRawIteratorWithThreadMode<'a, D>,
    map: M,
    direction: Direction,
    done: bool,
}

impl<'a, D: DBAccess, M> DbRawKeyValueIterator<'a, D, M> {
    pub fn new(raw: DBRawIteratorWithThreadMode<'a, D>, mode: IteratorMode, map: M) -> Self {
        let mut rv = Self {
            raw,
            direction: Direction::Forward, // blown away by set_mode()
            map,
            done: false,
        };
        rv.set_mode(mode);
        rv
    }

    pub fn set_mode(&mut self, mode: IteratorMode) {
        self.done = false;
        self.direction = match mode {
            IteratorMode::Start => {
                self.raw.seek_to_first();
                Direction::Forward
            },
            IteratorMode::End => {
                self.raw.seek_to_last();
                Direction::Reverse
            },
            IteratorMode::From(key, Direction::Forward) => {
                self.raw.seek(key);
                Direction::Forward
            },
            IteratorMode::From(key, Direction::Reverse) => {
                self.raw.seek_for_prev(key);
                Direction::Reverse
            },
        };
    }
}

impl<'db, D, M, R> Iterator for DbRawKeyValueIterator<'db, D, M>
where
    D: DBAccess,
    for<'b> M: FnMut(Result<(&'b [u8], &'b [u8]), rocksdb::Error>) -> R,
    // This makes the iterator safe forcing the caller to return owned data (i.e. no references to the slice)
    R: 'static,
{
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        // MODIFICATION: we return slices instead of allocating new buffers on each iteration.
        if self.done {
            return None;
        }

        if let Some((key, value)) = self.raw.item() {
            let ret = (self.map)(Ok((key, value)));
            // We progress the iterator until the map has been applied since rocksdb makes no guarantee
            // about the validity of the slices after calling next()/prev().
            match self.direction {
                Direction::Forward => self.raw.next(),
                Direction::Reverse => self.raw.prev(),
            }

            return Some(ret);
        }

        self.done = true;
        self.raw.status().err().map(Err).map(&mut self.map)
    }
}

impl<D, M, R> std::iter::FusedIterator for DbRawKeyValueIterator<'_, D, M>
where
    D: DBAccess,
    for<'b> M: FnMut(Result<(&'b [u8], &'b [u8]), rocksdb::Error>) -> R,
    R: 'static,
{
}
