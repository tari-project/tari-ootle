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

#[cfg(target_arch = "wasm32")]
mod wasm;

use minicbor::{CborLen, Decode, Encode};
#[cfg(target_arch = "wasm32")]
pub use wasm::*;
#[cfg(not(target_arch = "wasm32"))]
mod non_wasm;
#[cfg(not(target_arch = "wasm32"))]
pub use non_wasm::*;
use tari_bor::{decode_exact, encode_into_writer, encoded_len};

use crate::{
    ops::EngineOp,
    rust::{
        alloc_internal::{Layout, alloc, dealloc},
        prelude::*,
        vec::Vec,
    },
};

const USIZE_ALIGN: usize = align_of::<usize>();
const USIZE_SIZE: usize = size_of::<usize>();

// Do not compile if target is wasm and pointer size is not 32 bits
#[cfg(all(target_arch = "wasm32", not(target_pointer_width = "32")))]
compile_error!("This crate only supports wasm32 with 32-bit pointers");

pub fn wrap_ptr(v: Vec<u8>) -> *mut u8 {
    let p = Box::leak(v.into_boxed_slice());
    p.as_mut_ptr()
}

/// Calls the engine with the given operation and input, returning the decoded output.
pub fn call_engine<T, U>(op: EngineOp, input: &T) -> U
where
    T: Encode<()> + CborLen<()> + ?Sized,
    U: for<'b> Decode<'b, ()>,
{
    let len = encoded_len(&input);
    let mut encoded_input = Vec::with_capacity(len);
    encode_into_writer(input, &mut encoded_input).unwrap();
    let len = encoded_input.len();
    let input_ptr = encoded_input.as_mut_ptr();
    let result_ptr = unsafe { tari_engine(op.as_i32(), input_ptr, len) };
    if result_ptr.is_null() {
        // Terse by design: the engine identifies the refused call on its own side, and a formatted
        // panic pulls `core::fmt::Arguments` into every published template.
        panic!("ENGCALL_NULL");
    }
    // SAFETY: The pointer returned by `tari_engine` is valid and points to a length-prefixed block of memory allocated
    // by `tari_alloc`.
    let owned = unsafe { OwnedData::owned_from_ptr(result_ptr) };
    // Decode the output data, skipping the length prefix
    decode_exact(owned.data()).unwrap_or_else(|e| panic!("DECODEFAIL op {} input len: {}: {}", op.as_i32(), len, e))
}

/// Takes ownership of a length-prefixed block of memory allocated by `tari_alloc` and provides access to the data.
/// This is used when WASM is responsible for freeing memory.
pub struct OwnedData {
    ptr: *mut u8,
    len: usize,
}

impl OwnedData {
    /// Take ownership of the entire allocation (len, data)
    ///
    /// # Safety
    /// Caller must ensure that ptr is a valid pointer to a length-prefixed block of memory allocated by `tari_alloc`
    /// i.e. with an embedded length prefix `size_of::<usize>()`
    pub unsafe fn owned_from_ptr(ptr: *mut u8) -> Self {
        if ptr.is_null() {
            return Self {
                ptr: core::ptr::null_mut(),
                len: 0,
            };
        }
        // SAFETY: The pointer is assumed to be valid and points to a length-prefixed block of memory allocated
        // by `tari_alloc`.
        unsafe {
            let len_offset = ptr.sub(USIZE_SIZE).cast::<usize>();
            // SAFETY: pointer allocated by tari_alloc is usize-aligned
            let alloc_size = len_offset.read();
            let len = alloc_size.checked_sub(USIZE_SIZE).expect("BADALLOC");
            Self { ptr, len }
        }
    }

    pub fn data(&self) -> &[u8] {
        if self.ptr.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: `self.ptr` points to the payload after the length prefix of an active `tari_alloc` allocation of
        // total size `USIZE_SIZE + self.len`.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for OwnedData {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` is either null or a valid pointer allocated by `tari_alloc`.
        unsafe {
            tari_free(self.ptr);
        }
    }
}

/// Allocates a length-prefixed block of memory containing the encoded value and returns a pointer to that value.
/// This memory should be freed using `tari_free`.
pub fn alloc_and_encode<T: Encode<()> + CborLen<()> + ?Sized>(val: &T) -> *mut u8 {
    let len = encoded_len(val);
    let ptr = internal_alloc(len);
    // SAFETY: `internal_alloc(len)` returns a valid writable pointer to `len` bytes.
    let mut buf = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    if encode_into_writer(val, &mut buf).is_err() {
        // SAFETY: ptr was allocated by internal_alloc above and has not been freed or returned.
        unsafe {
            tari_free(ptr);
        }
        panic!("ENCDFAIL");
    }
    ptr
}

fn internal_alloc(size: usize) -> *mut u8 {
    let alloc_size = size.checked_add(USIZE_SIZE).expect("ALLOC_OVERFLOW");
    let layout = Layout::from_size_align(alloc_size, USIZE_ALIGN).expect("ALLOC_LAYOUT");
    unsafe {
        // memory layout: [size] [allocation]
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("ALLOC_OOM");
        }

        // SAFETY: ptr is usize-aligned and we've allocated sufficient memory
        ptr.cast::<usize>().write(alloc_size);

        // Return a pointer to the start of the allocation (after the length prefix)
        ptr.add(USIZE_SIZE)
    }
}

/// Allocates a length-prefixed block of memory of length `len` + 4 bytes.
#[unsafe(no_mangle)]
pub extern "C" fn tari_alloc(size: usize) -> *mut u8 {
    internal_alloc(size)
}

/// Frees a block of memory allocated by `tari_alloc`.
///
/// # Safety
/// Caller must ensure that ptr must be a valid pointer to a block of memory allocated by `tari_alloc` i.e. with an
/// embedded length prefix `size_of::<usize>()` bytes before the pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tari_free(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe {
            // read the length prefix to determine the size of the allocation
            let alloc_ptr = ptr.sub(USIZE_SIZE);
            let alloc_size = alloc_ptr.cast::<usize>().read();

            // SAFETY: Caller must ensure that ptr is a valid pointer to a block of memory allocated by `tari_alloc`.
            let layout = Layout::from_size_align_unchecked(alloc_size, USIZE_ALIGN);
            dealloc(alloc_ptr, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_data_null_ptr_returns_empty_slice_and_drops_safely() {
        let owned = unsafe { OwnedData::owned_from_ptr(core::ptr::null_mut()) };
        assert!(owned.data().is_empty());
    }

    #[test]
    fn alloc_and_encode_roundtrips_through_owned_data() {
        let payload = vec![1u32, 2, 3, 42];
        let ptr = alloc_and_encode(&payload);
        assert!(!ptr.is_null());

        let owned = unsafe { OwnedData::owned_from_ptr(ptr) };
        let decoded: Vec<u32> = decode_exact(owned.data()).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn tari_alloc_zero_size_roundtrips_through_owned_data() {
        let ptr = tari_alloc(0);
        assert!(!ptr.is_null());
        let owned = unsafe { OwnedData::owned_from_ptr(ptr) };
        assert!(owned.data().is_empty());
    }

    #[test]
    #[should_panic(expected = "ALLOC_OVERFLOW")]
    fn internal_alloc_rejects_size_overflow() {
        let _ = internal_alloc(usize::MAX);
    }
}
