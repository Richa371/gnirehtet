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

mod adb;
mod adb_monitor;
mod cli;
mod commands;
mod execution_error;
mod logger;

fn main() {
    let log_file = cli::get_log_file();
    if let Err(e) = logger::init(log_file.as_deref()) {
        eprintln!("Failed to initialize logger: {}", e);
    }
    adb::ensure_adb();
    cli::run();
}
