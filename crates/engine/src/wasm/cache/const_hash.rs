//  Copyright 2022. The Tari Project
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

//! A hash that runs in a `const` context.
//!
//! [`ENGINE_FINGERPRINT`](super::ENGINE_FINGERPRINT) folds several source files into one
//! 64-bit value. Doing it at compile time keeps the digest a constant and keeps the text it reads
//! out of the binary — the bytes are consumed during const evaluation and nothing references them
//! afterwards.
//!
//! FNV-1a with a final avalanche mix, because no cryptographic property is wanted. Nothing outside
//! this build chooses the input: the job is to separate this project's own configurations from one
//! another, not to resist someone searching for a collision.

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Start a digest over a domain separator.
pub(crate) const fn init(domain: &[u8]) -> u64 {
    bytes(FNV_OFFSET_BASIS, domain)
}

const fn bytes(mut state: u64, input: &[u8]) -> u64 {
    let mut i = 0;
    while i < input.len() {
        state ^= input[i] as u64;
        state = state.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    state
}

/// Absorb a length-prefixed part, so that content moved from one part to another still moves the
/// digest.
pub(crate) const fn part(state: u64, input: &[u8]) -> u64 {
    bytes(bytes(state, &(input.len() as u64).to_le_bytes()), input)
}

/// Absorb a length-prefixed run of values, so that a value moved from one run to an adjacent one
/// still moves the digest.
pub(crate) const fn values(mut state: u64, input: &[u128]) -> u64 {
    state = bytes(state, &(input.len() as u64).to_le_bytes());
    let mut i = 0;
    while i < input.len() {
        state = bytes(state, &input[i].to_le_bytes());
        i += 1;
    }
    state
}

/// Avalanche the accumulator, so that inputs differing in one byte differ everywhere.
pub(crate) const fn finish(mut state: u64) -> u64 {
    state = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    state = (state ^ (state >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    state ^ (state >> 31)
}

pub(crate) const fn to_hex(value: u64) -> [u8; 16] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        out[15 - i] = DIGITS[((value >> (i * 4)) & 0xf) as usize];
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_boundary_is_not_a_byte_boundary() {
        // Without the length prefix these two would be the same stream of bytes.
        let split = part(part(init(b"d"), b"ab"), b"c");
        let joined = part(init(b"d"), b"abc");
        assert_ne!(finish(split), finish(joined));
    }

    #[test]
    fn one_flipped_byte_changes_most_of_the_digest() {
        let a = finish(part(init(b"d"), b"the quick brown fox"));
        let b = finish(part(init(b"d"), b"the quick brown fox!"));
        assert!((a ^ b).count_ones() > 16, "avalanche is too weak: {:#x}", a ^ b);
    }

    #[test]
    fn hex_is_the_lower_case_big_endian_rendering() {
        assert_eq!(&to_hex(0x0123_4567_89ab_cdef), b"0123456789abcdef");
        assert_eq!(&to_hex(0), b"0000000000000000");
    }
}
