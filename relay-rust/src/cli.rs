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

use clap::{CommandFactory, Parser, Subcommand};
use log::*;

use crate::commands;
use crate::execution_error;
use crate::execution_error::CommandExecutionError;

const TAG: &str = "Main";

#[derive(Parser)]
#[command(name = "gnirehtet", version, about = "Reverse tethering for Android")]
struct Cli {
    /// Path to log file (appends; defaults to stderr/stdout)
    #[arg(long, global = true)]
    log_file: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install the client on the Android device and exit
    Install {
        /// Device serial (required if multiple devices connected via adb)
        serial: Option<String>,
    },
    /// Uninstall the client from the Android device and exit
    Uninstall {
        /// Device serial
        serial: Option<String>,
    },
    /// Uninstall then install the client
    Reinstall {
        /// Device serial
        serial: Option<String>,
    },
    /// Enable reverse tethering: install, start client, start relay, cleanup on Ctrl+C
    Run {
        /// Device serial
        serial: Option<String>,
        /// DNS server(s) (comma-separated)
        #[arg(short = 'd')]
        dns: Option<String>,
        /// Routes to reverse tether (comma-separated, e.g. 0.0.0.0/0)
        #[arg(short = 'r')]
        routes: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
        /// HTTP proxy (host:port)
        #[arg(short = 'x')]
        proxy: Option<String>,
        /// Proxy exclusion list (comma-separated domains)
        #[arg(short = 'e')]
        proxy_exclusions: Option<String>,
        /// Stop client on device disconnect
        #[arg(short = 's')]
        stop_on_disconnect: bool,
        /// MTU value for the VPN interface
        #[arg(long, default_value_t = 0x4000)]
        mtu: u16,
        /// Package name(s) to allow (repeatable)
        #[arg(long)]
        allow_app: Vec<String>,
        /// Package name(s) to deny (repeatable)
        #[arg(long)]
        deny_app: Vec<String>,
        /// SOCKS5 proxy (host:port)
        #[arg(long)]
        socks5: Option<String>,
    },
    /// Enable reverse tethering for all devices (monitor + auto-start + relay)
    Autorun {
        /// DNS server(s)
        #[arg(short = 'd')]
        dns: Option<String>,
        /// Routes
        #[arg(short = 'r')]
        routes: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
        /// Stop client on device disconnect
        #[arg(short = 's')]
        stop_on_disconnect: bool,
        /// MTU value for the VPN interface
        #[arg(long, default_value_t = 0x4000)]
        mtu: u16,
        /// Allow ADB-over-WiFi (TCP) devices (by default only USB devices are monitored)
        #[arg(long)]
        allow_wifi: bool,
        /// SOCKS5 proxy (host:port)
        #[arg(long)]
        socks5: Option<String>,
    },
    /// Start a client on the Android device and exit
    Start {
        /// Device serial
        serial: Option<String>,
        /// DNS server(s) (comma-separated)
        #[arg(short = 'd')]
        dns: Option<String>,
        /// Routes (comma-separated)
        #[arg(short = 'r')]
        routes: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
        /// HTTP proxy (host:port)
        #[arg(short = 'x')]
        proxy: Option<String>,
        /// Proxy exclusion list (comma-separated domains)
        #[arg(short = 'e')]
        proxy_exclusions: Option<String>,
        /// MTU value for the VPN interface
        #[arg(long, default_value_t = 0x4000)]
        mtu: u16,
        /// Package name(s) to allow (repeatable)
        #[arg(long)]
        allow_app: Vec<String>,
        /// Package name(s) to deny (repeatable)
        #[arg(long)]
        deny_app: Vec<String>,
        /// SOCKS5 proxy (host:port)
        #[arg(long)]
        socks5: Option<String>,
    },
    /// Listen for device connections and start a client on every detected device
    Autostart {
        /// DNS server(s)
        #[arg(short = 'd')]
        dns: Option<String>,
        /// Routes
        #[arg(short = 'r')]
        routes: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
        /// MTU value for the VPN interface
        #[arg(long, default_value_t = 0x4000)]
        mtu: u16,
        /// Allow ADB-over-WiFi (TCP) devices (by default only USB devices are monitored)
        #[arg(long)]
        allow_wifi: bool,
        /// SOCKS5 proxy (host:port)
        #[arg(long)]
        socks5: Option<String>,
    },
    /// Stop the client on the Android device and exit
    Stop {
        /// Device serial
        serial: Option<String>,
    },
    /// Stop then start the client
    Restart {
        /// Device serial
        serial: Option<String>,
        /// DNS server(s)
        #[arg(short = 'd')]
        dns: Option<String>,
        /// Routes
        #[arg(short = 'r')]
        routes: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
    },
    /// Set up the 'adb reverse' tunnel
    Tunnel {
        /// Device serial
        serial: Option<String>,
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
    },
    /// Start the relay server in the current terminal
    Relay {
        /// Relay server port
        #[arg(short = 'p', default_value_t = 31416)]
        port: u16,
    },
}

impl Commands {
    fn execute(&self) -> Result<(), CommandExecutionError> {
        match self {
            Commands::Install { serial } => commands::cmd_install(serial.as_deref()),
            Commands::Uninstall { serial } => commands::cmd_uninstall(serial.as_deref()),
            Commands::Reinstall { serial } => commands::cmd_reinstall(serial.as_deref()),
            Commands::Run {
                serial,
                dns,
                routes,
                port,
                proxy,
                proxy_exclusions,
                stop_on_disconnect,
                mtu,
                allow_app,
                deny_app,
                socks5,
            } => commands::cmd_run(serial.as_deref(), dns.as_deref(), routes.as_deref(), *port, proxy.as_deref(), proxy_exclusions.as_deref(), *stop_on_disconnect, *mtu, allow_app, deny_app, socks5.as_deref()),
            Commands::Autorun {
                dns,
                routes,
                port,
                stop_on_disconnect,
                mtu,
                allow_wifi,
                socks5,
            } => commands::cmd_autorun(dns.as_deref(), routes.as_deref(), *port, *stop_on_disconnect, *mtu, *allow_wifi, socks5.as_deref()),
            Commands::Start {
                serial,
                dns,
                routes,
                port,
                proxy,
                proxy_exclusions,
                mtu,
                allow_app,
                deny_app,
                socks5,
            } => commands::cmd_start(
                serial.as_deref(),
                dns.as_deref(),
                routes.as_deref(),
                *port,
                proxy.as_deref(),
                proxy_exclusions.as_deref(),
                *mtu,
                allow_app,
                deny_app,
                socks5.as_deref(),
            ),
            Commands::Autostart { dns, routes, port, mtu, allow_wifi, socks5 } => {
                commands::cmd_autostart(dns.as_deref(), routes.as_deref(), *port, *mtu, *allow_wifi, socks5.as_deref())
            }
            Commands::Stop { serial } => commands::cmd_stop(serial.as_deref()),
            Commands::Restart {
                serial,
                dns,
                routes,
                port,
            } => commands::cmd_restart(
                serial.as_deref(),
                dns.as_deref(),
                routes.as_deref(),
                *port,
            ),
            Commands::Tunnel { serial, port } => commands::cmd_tunnel(serial.as_deref(), *port),
            Commands::Relay { port } => commands::cmd_relay(*port),
        }
    }
}

pub fn run() {
    let raw: Vec<String> = std::env::args().collect();

    // Handle the deprecated "rt" alias
    if raw.len() > 1 && raw[1] == "rt" {
        error!(
            target: TAG,
            "The 'rt' command has been renamed to 'run'. Try 'gnirehtet run' instead."
        );
        std::process::exit(1);
    }

    // No arguments: show interactive prompt
    if raw.len() <= 1 {
        interactive_prompt();
        return;
    }

    let cli = Cli::parse();
    if let Err(ref err) = cli.command.execute() {
        execution_error::print_error(err);
        std::process::exit(3);
    }
}

fn interactive_prompt() {
    eprintln!();
    eprintln!("gnirehtet {} \u{2014} Reverse tethering for Android", env!("CARGO_PKG_VERSION"));
    eprintln!();
    loop {
        eprintln!("Choose an action:");
        eprintln!("  [1] Exit");
        eprintln!("  [2] View all commands and options (--help)");
        eprintln!("  [3] Run with recommended settings (not yet configured)");
        eprintln!();
        eprint!("Enter choice [1]: ");

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            break;
        }
        match input.trim() {
            "" | "1" => {
                eprintln!("Exiting.");
                std::process::exit(0);
            }
            "2" => {
                // Print clap help
                let _ = Cli::command().print_help();
                eprintln!();
                eprintln!();
                continue;
            }
            "3" => {
                // Auto-detect DNS and MTU, then run autorun
                let dns = commands::detect_system_dns().join(",");
                let mtu = commands::detect_mtu();
                eprintln!("Starting autorun with: DNS={}  MTU={}", dns, mtu);
                let result = commands::cmd_autorun(Some(&dns), None, 31416, false, mtu, false, None);
                if let Err(ref err) = result {
                    execution_error::print_error(err);
                }
                std::process::exit(result.is_err() as i32);
            }
            _ => {
                eprintln!("Invalid choice. Enter 1, 2, or 3.");
                eprintln!();
            }
        }
    }
}

pub fn get_log_file() -> Option<String> {
    let raw: Vec<String> = std::env::args().collect();
    // Simple manual parsing for --log-file before the subcommand
    for i in 1..raw.len() {
        if raw[i] == "--log-file"
            && i + 1 < raw.len() {
                return Some(raw[i + 1].clone());
            }
        if raw[i].starts_with("--log-file=") {
            return Some(raw[i]["--log-file=".len()..].to_string());
        }
        // Stop at the subcommand
        if !raw[i].starts_with('-') {
            break;
        }
    }
    None
}
