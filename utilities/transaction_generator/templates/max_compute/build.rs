//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

fn main() {
    tari_ootle_template_build::TemplateMetadataBuilder::new()
        .build()
        .expect("Failed to build template metadata");
}
