//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Codec-wide nesting bound applied to untrusted input before any typed decode runs.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use minicbor::{Decoder, data::Type, decode};

/// Rejects input nested deeper than `max_depth`.
///
/// The walk is iterative and reads only item heads, so it costs a fraction of the decode it guards
/// and is itself immune to the recursion it rejects.
pub fn check_nesting_depth(input: &[u8], max_depth: usize) -> Result<(), decode::Error> {
    if walk(&mut Decoder::new(input), max_depth).unwrap_or(true) {
        return Ok(());
    }
    Err(decode::Error::message("maximum CBOR nesting depth exceeded"))
}

/// Walks the heads of the first item in `d`, returning whether it stays within `max_depth`.
///
/// Malformed input is reported as `Err` and treated by the caller as within bound: the decode that
/// follows produces a parse error far more specific than anything this walk could say.
fn walk(d: &mut Decoder<'_>, max_depth: usize) -> Result<bool, decode::Error> {
    // Items still to read per open container, innermost last: `Some(n)` for a definite-length
    // container, `None` for an indefinite-length one that ends at a break byte. The outermost frame
    // is the single top-level item, so an item's nesting depth is `stack.len() - 1`.
    let mut stack: Vec<Option<u64>> = Vec::with_capacity(16);
    stack.push(Some(1));

    loop {
        let Some(frame) = stack.last().copied() else {
            return Ok(true);
        };
        match frame {
            Some(0) => {
                stack.pop();
                continue;
            },
            Some(_) => {},
            None => {
                if matches!(d.datatype()?, Type::Break) {
                    d.skip()?;
                    stack.pop();
                    continue;
                }
            },
        }

        // An item is about to be read, and the frames open around it are its nesting. Checked here
        // rather than where a container is opened, so an empty container at the bound is accepted
        // exactly as a scalar there is.
        if stack.len() - 1 > max_depth {
            return Ok(false);
        }

        if let Some(Some(n)) = stack.last_mut() {
            *n -= 1;
        }

        let remaining = match d.datatype()? {
            Type::Array | Type::ArrayIndef => d.array()?,
            // A map's key and value each nest at the same depth, so one frame covers both.
            Type::Map | Type::MapIndef => d.map()?.map(|n| n.saturating_mul(2)),
            Type::Tag => {
                d.tag()?;
                Some(1)
            },
            // Scalars, including indefinite-length byte and text strings, whose chunks do not nest.
            _ => {
                d.skip()?;
                continue;
            },
        };

        stack.push(remaining);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Any bound will do for exercising the walk; the real one is the caller's.
    const MAX: usize = 256;

    #[test]
    fn scalars_and_flat_containers_pass() {
        for input in [
            &[0x00][..],                                                 // 0
            &[0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff][..], // a 64-bit negative
            &[0x80][..],                                                 // []
            &[0xa0][..],                                                 // {}
            &[0x83, 0x01, 0x02, 0x03][..],                               // [1, 2, 3]
            &[0xa1, 0x01, 0x02][..],                                     // {1: 2}
            &[0x9f, 0x01, 0xff][..],                                     // [_ 1]
            &[0xbf, 0x01, 0x02, 0xff][..],                               // {_ 1: 2}
            &[0x43, 0x01, 0x02, 0x03][..],                               // h'010203'
            &[0x5f, 0x41, 0x01, 0x41, 0x02, 0xff][..],                   // (_ h'01', h'02')
            &[0xc0, 0x01][..],                                           // 0(1)
        ] {
            assert!(check_nesting_depth(input, MAX).is_ok(), "rejected {input:x?}");
        }
    }

    #[test]
    fn trailing_bytes_after_the_first_item_are_not_walked() {
        // A flat item followed by a nesting bomb: only the first item is this walk's business.
        let mut input = vec![0x00];
        input.extend(core::iter::repeat_n(0x81u8, 10_000));
        assert!(check_nesting_depth(&input, MAX).is_ok());
    }

    #[test]
    fn nesting_at_the_bound_passes_and_one_level_deeper_is_rejected() {
        for head in [
            0x81u8, // array of 1
            0xa1,   // map of 1
            0x9f,   // indefinite array
            0xc0,   // tag
        ] {
            let at_bound: Vec<u8> = core::iter::repeat_n(head, MAX).chain(core::iter::once(0x00)).collect();
            assert!(check_nesting_depth(&at_bound, MAX).is_ok(), "head {head:#x} at bound");

            let over_bound: Vec<u8> = core::iter::repeat_n(head, MAX + 1)
                .chain(core::iter::once(0x00))
                .collect();
            assert!(
                check_nesting_depth(&over_bound, MAX).is_err(),
                "head {head:#x} over bound"
            );
        }
    }

    #[test]
    fn an_empty_container_at_the_bound_is_accepted_as_a_scalar_there_is() {
        let containers: Vec<u8> = core::iter::repeat_n(0x81u8, MAX).collect();
        for innermost in [0x00u8, 0x80, 0xa0] {
            let mut input = containers.clone();
            input.push(innermost);
            assert!(check_nesting_depth(&input, MAX).is_ok(), "innermost {innermost:#x}");
        }
    }

    #[test]
    fn a_nesting_bomb_is_rejected_without_recursing() {
        let bomb = vec![0x81u8; 1_000_000];
        assert!(check_nesting_depth(&bomb, MAX).is_err());
    }

    #[test]
    fn malformed_input_is_left_for_the_decoder_to_reject() {
        for input in [
            &[][..],     // no input at all
            &[0x81][..], // array of 1 with no element
            &[0xff][..], // a stray break
            &[0x1c][..], // a reserved additional-information value
        ] {
            assert!(check_nesting_depth(input, MAX).is_ok(), "claimed {input:x?} for itself");
        }
    }
}
