# Gnirehtet

Reverse tethering for Android — share your computer's internet with an Android
device over USB. No root required.

Works on Linux, Windows and macOS. Relays [TCP] and [UDP] over [IPv4] and
[IPv6].

[TCP]: https://en.wikipedia.org/wiki/Transmission_Control_Protocol
[UDP]: https://en.wikipedia.org/wiki/User_Datagram_Protocol
[IPv4]: https://en.wikipedia.org/wiki/IPv4
[IPv6]: https://en.wikipedia.org/wiki/IPv6

## Quick start

    ./gnirehtet run

The first start opens a prompt on the device to accept the VPN connection.
Press _Ctrl+C_ to stop.

## Requirements

- An Android device with USB debugging enabled
- The `gnirehtet.apk` installed on the device (included in releases)
- That's it — `adb` is auto-downloaded if missing

## Commands

| Command | Description |
|---------|-------------|
| `run` | Connect one device (install + start + relay, stop on Ctrl+C) |
| `start` | Start a client on the device and exit |
| `stop` | Stop the client |
| `install` | Install the APK |
| `autorun` | Auto-connect all present and future devices |
| `relay` | Start only the relay server |
| `tunnel` | Set up the `adb reverse` tunnel |

Run `gnirehtet --help` for all options and flags.

## Environment

- `ADB` — path to `adb` executable
- `GNIREHTET_APK` — path to `gnirehtet.apk`
- `RUST_LOG` — set to `debug` or `trace` for verbose logging

## Build from source

Build the relay (works everywhere, no extra tools):

    scripts/build.sh

Build the APK (requires JDK and Android SDK):

    scripts/build-apk.sh

Or use `make` (Linux/macOS) or `build.bat` (Windows) for the same commands.

Pre-built binaries for all platforms are attached to each
[release](https://github.com/Genymobile/gnirehtet/releases).

## Licence

    Copyright (C) 2017 Genymobile

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
