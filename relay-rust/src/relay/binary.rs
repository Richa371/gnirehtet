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

use byteorder::{BigEndian, ByteOrder};
use std::cmp::min;
use std::fmt::Write;

const MAX_STRING_PACKET_SIZE: usize = 20;

#[allow(dead_code)]
pub fn to_byte_array(value: u32) -> [u8; 4] {
    let mut raw = [0u8; 4];
    BigEndian::write_u32(&mut raw, value);
    raw
}

pub fn build_packet_string(data: &[u8]) -> String {
    let mut s = String::new();
    let limit = min(MAX_STRING_PACKET_SIZE, data.len());
    for (i, &byte) in data.iter().take(limit).enumerate() {
        if i != 0 {
            let sep = if (i % 4) == 0 { "  " } else { " " };
            let _ = write!(&mut s, "{}", sep);
        }
        let _ = write!(&mut s, "{:02X}", byte);
    }
    if limit < data.len() {
        let _ = write!(&mut s, "  ... +{} bytes", data.len() - limit);
    }
    s
}
