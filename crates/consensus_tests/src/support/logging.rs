//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Current UTC time in RFC 3339, with fractional seconds only when they are non-zero.
fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc();
    // Rfc3339 formatting fails only for years outside 0..=9999.
    now.format(&Rfc3339).unwrap_or_else(|_| now.to_string())
}

pub fn setup_logger() {
    let _ignore = fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {} {}",
                now_rfc3339(),
                record.target().strip_prefix("tari::ootle::consensus::hotstuff::").unwrap_or(record.target()),
                record.level(),
                message
            ))
        })
        // Add blanket level filter -
        .level(log::LevelFilter::Debug)
        // Output to stdout, files, and other Dispatch configurations
        .chain(std::io::stdout())
        // .chain(fern::log_file("output.log").unwrap())
        // Apply globally
        .apply();
}
