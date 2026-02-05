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
#![allow(non_snake_case)]

use core::ptr;

pub use tari_template_abi::tari_alloc;

#[global_allocator]
static ALLOC: lol_alloc::AssumeSingleThreaded<lol_alloc::FreeListAllocator> =
    unsafe { lol_alloc::AssumeSingleThreaded::new(lol_alloc::FreeListAllocator::new()) };

#[cfg(feature = "return_null_abi")]
#[unsafe(no_mangle)]
pub static _ABI_TEMPLATE_DEF: [u8; 0] = [];

#[cfg(feature = "return_empty_abi")]
#[unsafe(no_mangle)]
pub static _ABI_TEMPLATE_DEF: [u8; 4] = [0, 0, 0, 0];

#[cfg(not(any(
    feature = "return_empty_abi",
    feature = "return_null_abi",
    feature = "no_template_def"
)))]
#[unsafe(no_mangle)]
pub static _ABI_TEMPLATE_DEF: [u8; 53] = [
    49, 0, 0, 0, 161, 98, 86, 49, 163, 109, 116, 101, 109, 112, 108, 97, 116, 101, 95, 110, 97, 109, 101, 101, 66, 117,
    103, 103, 121, 107, 97, 98, 105, 95, 118, 101, 114, 115, 105, 111, 110, 0, 105, 102, 117, 110, 99, 116, 105, 111,
    110, 115, 128,
];

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Buggy_main(_call_info: *mut u8, _call_info_len: usize) -> *mut u8 {
    ptr::null_mut()
}

unsafe extern "C" {
    pub fn tari_engine(op: i32, input_ptr: *const u8, input_len: usize) -> *mut u8;
    pub fn debug(input_ptr: *const u8, input_len: usize);
    pub fn on_panic(msg_ptr: *const u8, msg_len: u32, line: u32, column: u32);
}

#[cfg(feature = "unexpected_export_function")]
#[unsafe(no_mangle)]
pub extern "C" fn i_shouldnt_be_here() -> *mut u8 {
    ptr::null_mut()
}
