//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

fn format_rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn setup_logger() {
    let _ignore = fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {} {}",
                format_rfc3339_now(),
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
