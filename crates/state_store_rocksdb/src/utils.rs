//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::io;

use tari_ootle_storage::time::{PrimitiveDateTime, UtcDateTime};

pub(crate) fn read_to_fixed<const SZ: usize, T, R>(reader: &mut R) -> Option<T>
where
    [u8; SZ]: Into<T>,
    R: io::Read,
{
    let mut array = [0u8; SZ];
    reader.read_exact(&mut array).ok()?;
    Some(array.into())
}

pub(crate) fn read_n_bytes<R: io::Read>(reader: &mut R, n: usize) -> Option<Vec<u8>> {
    let mut vec = vec![0u8; n];
    reader.read_exact(&mut vec[..]).ok()?;
    Some(vec)
}

pub(crate) fn now() -> PrimitiveDateTime {
    let now = UtcDateTime::now();
    PrimitiveDateTime::new(now.date(), now.time())
}
