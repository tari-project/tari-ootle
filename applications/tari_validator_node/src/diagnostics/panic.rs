//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use tari_ootle_common_types::diag_event;
use tari_ootle_storage::DiagnosticEventStore;

type Recorder = Box<dyn Fn(&str, &str) + Send + Sync>;

static PANIC_RECORDER: OnceLock<Recorder> = OnceLock::new();
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Lets the process-wide panic hook write a `node.panic` event.
///
/// The write goes straight to the store rather than through [`DiagnosticsHandle`], because by the
/// time a panic reaches the hook the writer task may already be gone.
///
/// [`DiagnosticsHandle`]: super::DiagnosticsHandle
pub fn install_panic_recorder<TStore>(store: TStore)
where TStore: DiagnosticEventStore + Send + Sync + 'static {
    let _ignore = PANIC_RECORDER.set(Box::new(move |location, message| {
        let event = diag_event!(error, "node.panic", "Panicked at {location}: {message}",
            location => location,
            message => message,
            thread => std::thread::current().name().unwrap_or("<unnamed>")
        );
        let _ignore = store.diagnostic_events_append(&[event]);
    }));
}

/// Records a panic, if a recorder has been installed.
///
/// Only the first panic is recorded: a panic raised from inside the store while recording would
/// re-enter here and could deadlock on the same lock it panicked holding.
pub fn record_panic(location: &str, message: &str) {
    let Some(recorder) = PANIC_RECORDER.get() else {
        return;
    };
    if RECORDING.swap(true, Ordering::SeqCst) {
        return;
    }
    recorder(location, message);
}
