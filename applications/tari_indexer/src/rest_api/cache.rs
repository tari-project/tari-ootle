//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::http::{HeaderMap, HeaderValue, header};

pub struct HttpCacheConfig {
    pub is_public: bool,
    pub max_age: u32,
    pub s_maxage: u32,
    pub stale_while_revalidate: u32,
}
impl HttpCacheConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn to_header_value(&self) -> HeaderValue {
        self.to_header_string()
            .parse()
            .expect("BUG: failed to parse cache control header value")
    }

    pub fn to_header_string(&self) -> String {
        format!(
            "{}, max-age={}, s-maxage={}, stale-while-revalidate={}",
            if self.is_public { "public" } else { "private" },
            self.max_age,
            self.s_maxage,
            self.stale_while_revalidate
        )
    }

    pub fn apply(&self, headers: &mut HeaderMap) {
        headers.insert(header::CACHE_CONTROL, self.to_header_value());
    }

    pub fn with_max_age(mut self, max_age: u32) -> Self {
        self.max_age = max_age;
        self.s_maxage = (max_age / 2).max(1);
        self.stale_while_revalidate = (max_age / 4).max(1);
        self
    }
}

impl Default for HttpCacheConfig {
    fn default() -> Self {
        Self {
            is_public: true,
            max_age: 60,
            s_maxage: 30,
            stale_while_revalidate: 15,
        }
    }
}
