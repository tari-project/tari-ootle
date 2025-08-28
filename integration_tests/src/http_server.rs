//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use httpmock::MockServer;

pub struct MockHttpServer {
    server: MockServer,
}

impl MockHttpServer {
    pub async fn connect() -> Self {
        Self {
            server: MockServer::start_async().await,
        }
    }

    pub async fn publish_file(&self, url_path: String, file_path: String) -> Mock<'_> {
        let mock = self
            .server
            .mock_async(|when, then| {
                when.path(format!("/{}", url_path));
                then.status(200).body_from_file(file_path);
            })
            .await;

        let url = self.server.url(url_path);
        Mock { mock, url }
    }
}

impl fmt::Debug for MockHttpServer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MockHttpServer")
    }
}

pub struct Mock<'a> {
    pub mock: httpmock::Mock<'a>,
    pub url: String,
}

// impl Drop for Mock<'_> {
//     fn drop(&mut self) {
//         self.mock.delete();
//     }
// }
