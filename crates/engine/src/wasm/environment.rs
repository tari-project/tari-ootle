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

use std::{
    fmt::{Debug, Formatter},
    sync::{Arc, Mutex, MutexGuard},
};

use tari_template_abi::{TemplateDef, ABI_TEMPLATE_DEF_GLOBAL_NAME};
use wasmer::{
    AsStoreMut,
    AsStoreRef,
    ExportError,
    Instance,
    Memory,
    MemoryAccessError,
    MemoryView,
    TypedFunction,
    WasmPtr,
};

use crate::{
    runtime::RuntimeError,
    wasm::{mem_writer::MemWriter, WasmExecutionError},
};

#[derive(Clone)]
pub struct WasmEnv<T> {
    memory: Option<Memory>,
    state: T,
    mem_alloc: Option<TypedFunction<u32, WasmPtr<u8>>>,
    // mem_free: Option<TypedFunction<u32, ()>>,
    last_panic: Arc<Mutex<Option<String>>>,
    last_engine_error: Arc<Mutex<Option<RuntimeError>>>,
}

impl<T> WasmEnv<T> {
    pub fn new(state: T) -> Self {
        Self {
            memory: None,
            state,
            mem_alloc: None,
            // mem_free: None,
            last_panic: Arc::new(Mutex::new(None)),
            last_engine_error: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn set_last_panic(&self, message: String) {
        *self.last_panic_mut() = Some(message);
    }

    pub(super) fn alloc<S: AsStoreMut>(&self, store: &mut S, len: u32) -> Result<WasmPtr<u8>, WasmExecutionError> {
        let ptr = self.get_mem_alloc_func()?.call(store, len)?;
        if ptr.offset() == 0 {
            return Err(WasmExecutionError::MemoryAllocationFailed);
        }

        Ok(ptr)
    }

    // pub(super) fn free<S: AsStoreMut>(&self, store: &mut S, ptr: WasmPtr<u8>) -> Result<(), WasmExecutionError> {
    //     let mem_free = self
    //         .mem_free
    //         .as_ref()
    //         .ok_or_else(|| WasmExecutionError::MissingAbiFunction { function: "tari_free" })?;
    //     mem_free.call(store, ptr.offset())?;
    //     Ok(())
    // }

    fn last_panic_mut(&self) -> MutexGuard<'_, Option<String>> {
        self.last_panic.lock().expect("last_panic poisoned")
    }

    pub(super) fn take_last_panic_message(&self) -> Option<String> {
        self.last_panic_mut().take()
    }

    fn last_engine_error_mut(&self) -> MutexGuard<'_, Option<RuntimeError>> {
        self.last_engine_error.lock().expect("last_engine_error poisoned")
    }

    pub(super) fn set_last_engine_error(&self, error: RuntimeError) {
        *self.last_engine_error_mut() = Some(error);
    }

    pub(super) fn take_last_engine_error(&self) -> Option<RuntimeError> {
        self.last_engine_error_mut().take()
    }

    pub(super) fn load_template_def<S: AsStoreMut>(
        &self,
        store: &mut S,
        instance: &Instance,
    ) -> Result<TemplateDef, WasmExecutionError> {
        let ptr = instance
            .exports
            .get_global(ABI_TEMPLATE_DEF_GLOBAL_NAME)?
            .get(store)
            .i32()
            .ok_or(WasmExecutionError::ExportError(ExportError::IncompatibleType))? as u32;

        // Load ABI from memory
        // SAFETY: WasmEnv is not used concurrently
        unsafe {
            self.with_memory_with_embedded_len(store, ptr, |data| {
                tari_bor::decode(data).map_err(WasmExecutionError::AbiDecodeError)
            })?
        }
    }

    pub(super) fn memory_writer<'a, S: AsStoreMut>(
        &self,
        store: &'a mut S,
        ptr: WasmPtr<u8>,
    ) -> Result<MemWriter<'a>, WasmExecutionError> {
        let view = self.get_memory()?.view(store);
        Ok(MemWriter::new(ptr, view))
    }

    /// Retrieves a slice of memory at the given pointer and length, and calls the provided callback with that slice.
    /// Returns an error if the pointer and length are out of memory bounds.
    ///
    /// # Safety
    /// This function provides direct access to the memory slice. The caller must ensure that the memory is not
    /// modified while the slice is in use.
    /// It is undefined behaviour to modify the memory contents in any way including by calling a wasm
    /// function that writes to the memory or by resizing the memory.
    pub(super) unsafe fn with_memory_slice<S: AsStoreRef, F: FnMut(&[u8]) -> R, R>(
        &self,
        store: &mut S,
        ptr: WasmPtr<u8>,
        len: u32,
        mut callback: F,
    ) -> Result<R, WasmExecutionError> {
        let memory = self.get_memory()?;
        let view = memory.view(store);

        let slice = view.data_unchecked();

        let start = ptr.offset() as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or(WasmExecutionError::MemoryPointerOutOfRange {
                size: slice.len() as u64,
                pointer: u64::from(ptr.offset()),
                len: u64::from(len),
            })?;

        let slice = slice
            .get(start..end)
            .ok_or(WasmExecutionError::MemoryPointerOutOfRange {
                size: slice.len() as u64,
                pointer: u64::from(ptr.offset()),
                len: u64::from(len),
            })?;

        Ok(callback(slice))
    }

    /// Reads the 4-byte length prefix at the given offset and calls the provided callback with the payload slice
    /// (`offset + 4..offset + 4 + len`) i.e. excluding the length prefix. Returns an error if the length prefix or
    /// payload is out of memory bounds.
    ///
    /// # Safety
    /// This function provides direct access to the memory slice. The caller must ensure that the memory is not
    /// modified while the slice is in use.
    /// It is undefined behaviour to modify the memory contents in any way including by calling a wasm
    /// function that writes to the memory or by resizing the memory.
    pub(super) unsafe fn with_memory_with_embedded_len<S: AsStoreRef, F: FnMut(&[u8]) -> R, R>(
        &self,
        store: &mut S,
        offset: u32,
        mut callback: F,
    ) -> Result<R, WasmExecutionError> {
        let memory = self.get_memory()?;
        let view = memory.view(store);
        let len = read_len_from_memory(&view, offset)?;
        let start = offset
            .checked_add(4)
            .ok_or(WasmExecutionError::MemoryPointerOutOfRange {
                size: view.data_size(),
                pointer: u64::from(offset),
                len: 4,
            })?;
        let end = start
            .checked_add(len)
            .ok_or(WasmExecutionError::MemoryPointerOutOfRange {
                size: view.data_size(),
                pointer: u64::from(start),
                len: u64::from(len),
            })?;

        let slice = view.data_unchecked();
        let slice = slice
            .get(start as usize..end as usize)
            .ok_or(WasmExecutionError::MemoryPointerOutOfRange {
                size: slice.len() as u64,
                pointer: u64::from(start),
                len: u64::from(len),
            })?;

        Ok(callback(slice))
    }

    pub(super) fn memory_size<S: AsStoreRef>(&self, store: &mut S) -> Result<usize, WasmExecutionError> {
        let memory = self.get_memory()?;
        let view = memory.view(store);
        let size = view.data_size();
        usize::try_from(size).map_err(|_| WasmExecutionError::MaxMemorySizeExceeded)
    }

    pub fn state(&self) -> &T {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.state
    }

    fn get_mem_alloc_func(&self) -> Result<&TypedFunction<u32, WasmPtr<u8>>, WasmExecutionError> {
        self.mem_alloc
            .as_ref()
            .ok_or_else(|| WasmExecutionError::MissingAbiFunction { function: "tari_alloc" })
    }

    fn get_memory(&self) -> Result<&Memory, WasmExecutionError> {
        let memory = self.memory.as_ref().ok_or_else(|| WasmExecutionError::MemoryNotSet)?;
        Ok(memory)
    }
}

impl<T> WasmEnv<T> {
    pub fn set_memory(&mut self, memory: Memory) -> &mut Self {
        self.memory = Some(memory);
        self
    }

    pub fn set_alloc_funcs(
        &mut self,
        mem_alloc: TypedFunction<u32, WasmPtr<u8>>,
        // mem_free: TypedFunction<u32, ()>,
    ) -> &mut Self {
        self.mem_alloc = Some(mem_alloc);
        // self.mem_free = Some(mem_free);
        self
    }
}

impl<T: Debug> Debug for WasmEnv<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmEnv")
            .field("memory", &"LazyInit<Memory>")
            .field("tari_alloc", &" LazyInit<NativeFunc<(i32), (i32)>")
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Debug)]
pub struct AllocPtr(u32, u32);

impl AllocPtr {
    pub fn new(offset: u32, len: u32) -> Self {
        Self(offset, len)
    }

    pub fn get(&self) -> u32 {
        self.0
    }

    pub fn len(&self) -> u32 {
        self.1
    }

    pub fn as_wasm_ptr<T>(&self) -> WasmPtr<T> {
        WasmPtr::new(self.get())
    }
}

fn read_len_from_memory(view: &MemoryView, offset: u32) -> Result<u32, MemoryAccessError> {
    let mut buf = [0u8; 4];
    view.read(u64::from(offset), &mut buf)?;
    Ok(u32::from_le_bytes(buf))
}
