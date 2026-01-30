//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io;

use wasmer::{MemoryAccessError, MemoryView, WasmPtr};

pub struct MemWriter<'a> {
    ptr: WasmPtr<u8>,
    view: MemoryView<'a>,
}
impl<'a> MemWriter<'a> {
    pub fn new(ptr: WasmPtr<u8>, view: MemoryView<'a>) -> Self {
        Self { ptr, view }
    }

    pub fn write_all_to_mem(&mut self, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.view.write(u64::from(self.ptr.offset()), data)?;
        self.ptr = self.ptr.add_offset(data.len() as u32)?;
        Ok(())
    }
}

impl io::Write for MemWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all_to_mem(buf)
            .map_err(|e| io::Error::other(format!("Wasm memory access error: {:?}", e)))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
