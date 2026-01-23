//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::runtime::StateTracker;

pub trait RuntimeModule<TStore>: Send + Sync {
    fn on_initialize(&self, _track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_runtime_call(
        &self,
        _track: &mut StateTracker<TStore>,
        _call: &'static str,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_template_loaded(
        &self,
        _track: &mut StateTracker<TStore>,
        _bytes_loaded: usize,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_before_finalize(&self, _track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        Ok(())
    }

    fn on_runtime_event(
        &self,
        _track: &mut StateTracker<TStore>,
        _call: &RuntimeEvent,
    ) -> Result<(), RuntimeModuleError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    SignatureVerified,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeModuleError {
    #[error("BOR error: {0}")]
    Bor(#[from] tari_bor::BorError),
    #[error("Overflow error: {0}")]
    Overflow(String),
}
