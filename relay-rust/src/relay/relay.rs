/*
 * Copyright (C) 2017 Genymobile
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! The relay engine: runs on tokio's multi-threaded runtime, accepting reverse-tunnel
//! connections from Android devices and relaying IP packets to/from the internet.

use log::*;
use std::io;

use super::client::Client;

const TAG: &str = "Relay";

pub struct Relay {
    port: u16,
}

impl Relay {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Start the relay server. Creates a tokio runtime and enters the async accept loop.
    /// Accepted clients are handed off to dedicated OS threads.
    pub fn run(&self) -> io::Result<()> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.run_async())
    }

    async fn run_async(&self) -> io::Result<()> {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;
        info!(target: TAG, "Relay server started on port {}", self.port);
        loop {
            let (stream, peer) = listener.accept().await?;
            debug!(target: TAG, "New connection from {}", peer);
            let std_stream = stream.into_std()?;
            std::thread::spawn(move || {
                Client::run_blocking(std_stream);
            });
        }
    }
}
