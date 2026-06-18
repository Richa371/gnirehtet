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

use log::*;
use std::thread;
use std::time::Duration;

use crate::adb::{ensure_adb, exec_adb, get_apk_path, must_install_client};
use crate::execution_error::{Cmd, CommandExecutionError, ProcessIoError, ProcessStatusError};
use crate::adb_monitor::AdbMonitor;
use relaylib::relay::tcp_connection;

const TAG: &str = "Main";

/// Detect system DNS servers by parsing OS-specific configuration.
pub fn detect_system_dns() -> Vec<String> {
    // Linux: parse /etc/resolv.conf for nameserver entries
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
            let servers: Vec<String> = content
                .lines()
                .filter_map(|line| line.trim().strip_prefix("nameserver "))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && *s != "127.0.0.1" && *s != "::1")
                .collect();
            if !servers.is_empty() {
                return servers;
            }
        }
    }
    // macOS: use scutil --dns
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("scutil").arg("--dns").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let servers: Vec<String> = stdout
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("nameserver[")
                        .and_then(|s| s.split(']').nth(1))
                })
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && *s != "127.0.0.1" && *s != "::1")
                .collect();
            if !servers.is_empty() {
                return servers;
            }
        }
    }
    // Windows: use powershell to query DNS client server addresses
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-Command", "(Get-DnsClientServerAddress).ServerAddresses"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let servers: Vec<String> = stdout
                .split_whitespace()
                .filter(|s| {
                    !s.is_empty()
                        && *s != "127.0.0.1"
                        && *s != "::1"
                        && *s != "0.0.0.0"
                })
                .map(|s| s.to_string())
                .collect();
            if !servers.is_empty() {
                return servers;
            }
        }
    }
    // Fallback
    vec!["8.8.8.8".to_string()]
}

/// Detect the MTU of the default route interface; falls back to 16384.
pub fn detect_mtu() -> u16 {
    // Try to detect interface MTU; fall back to 16384
    #[cfg(target_os = "linux")]
    {
        // Use `ip route` to find the default route's MTU
        if let Ok(output) = std::process::Command::new("ip")
            .args(["route", "show", "default"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse "default via X dev Y ... mtu N"
            for word in stdout.split_whitespace() {
                if let Some(mtu_str) = word.strip_prefix("mtu")
                    && let Ok(mtu) = mtu_str.trim().parse::<u16>() {
                        return mtu.max(1280); // minimum MTU for IPv6
                    }
            }
        }
    }
    0x4000 // default 16384
}

pub fn cmd_install(serial: Option<&str>) -> Result<(), CommandExecutionError> {
    let apk_path = get_apk_path();
    info!(target: TAG, "Installing gnirehtet client...");
    let adb = crate::adb::get_adb_path();
    let args = crate::adb::create_adb_args(serial, vec!["install", "-r", &apk_path]);
    let cmd_obj = Cmd::new(adb.clone(), args.clone());
    match std::process::Command::new(&adb).args(&args).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                Ok(())
            } else {
                if stderr.contains("INSTALL_FAILED") {
                    eprintln!(
                        "Tip: Make sure USB debugging is enabled on your device and check for a confirmation dialog on the device screen."
                    );
                }
                Err(ProcessStatusError::new(cmd_obj, output.status).into())
            }
        }
        Err(err) => Err(ProcessIoError::new(cmd_obj, err).into()),
    }
}

pub fn cmd_uninstall(serial: Option<&str>) -> Result<(), CommandExecutionError> {
    info!(target: TAG, "Uninstalling gnirehtet client...");
    exec_adb(serial, vec!["uninstall", "com.genymobile.gnirehtet"])
}

pub fn cmd_reinstall(serial: Option<&str>) -> Result<(), CommandExecutionError> {
    cmd_uninstall(serial)?;
    cmd_install(serial)?;
    Ok(())
}

pub fn cmd_stop(serial: Option<&str>) -> Result<(), CommandExecutionError> {
    info!(target: TAG, "Stopping client...");
    exec_adb(
        serial,
        vec![
            "shell",
            "am",
            "start",
            "-a",
            "com.genymobile.gnirehtet.STOP",
            "-n",
            "com.genymobile.gnirehtet/.GnirehtetActivity",
        ],
    )
}

pub fn cmd_tunnel(serial: Option<&str>, port: u16) -> Result<(), CommandExecutionError> {
    exec_adb(
        serial,
        vec![
            "reverse",
            "localabstract:gnirehtet",
            format!("tcp:{}", port).as_str(),
        ],
    )
}

pub fn cmd_relay(port: u16) -> Result<(), CommandExecutionError> {
    info!(target: TAG, "Starting relay server on port {}...", port);
    relaylib::relay(port)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_start(
    serial: Option<&str>,
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
    proxy: Option<&str>,
    proxy_exclusions: Option<&str>,
    mtu: u16,
    allow_apps: &[String],
    deny_apps: &[String],
    socks5: Option<&str>,
) -> Result<(), CommandExecutionError> {
    ensure_adb();
    if must_install_client(serial)? {
        cmd_install(serial)?;
        thread::sleep(Duration::from_millis(500));
    }

    info!(target: TAG, "Starting client...");
    cmd_tunnel(serial, port)?;

    let mut adb_args: Vec<String> = vec![
        "shell".into(),
        "am".into(),
        "start".into(),
        "-a".into(),
        "com.genymobile.gnirehtet.START".into(),
        "-n".into(),
        "com.genymobile.gnirehtet/.GnirehtetActivity".into(),
    ];
    if let Some(dns_servers) = dns_servers {
        adb_args.push("--esa".into());
        adb_args.push("dnsServers".into());
        adb_args.push(dns_servers.into());
    }
    if let Some(routes) = routes {
        adb_args.push("--esa".into());
        adb_args.push("routes".into());
        adb_args.push(routes.into());
    }
    if let Some(proxy) = proxy {
        adb_args.push("--es".into());
        adb_args.push("proxyHostPort".into());
        adb_args.push(proxy.into());
    }
    if let Some(exclusions) = proxy_exclusions {
        adb_args.push("--esa".into());
        adb_args.push("proxyExclusionList".into());
        adb_args.push(exclusions.into());
    }
    if let Some(socks5_host_port) = socks5 {
        adb_args.push("--es".into());
        adb_args.push("socks5Proxy".into());
        adb_args.push(socks5_host_port.into());
    }
    adb_args.push("--ei".into());
    adb_args.push("mtu".into());
    adb_args.push(mtu.to_string());
    if !allow_apps.is_empty() {
        adb_args.push("--esa".into());
        adb_args.push("allowApps".into());
        adb_args.push(allow_apps.join(","));
    }
    if !deny_apps.is_empty() {
        adb_args.push("--esa".into());
        adb_args.push("denyApps".into());
        adb_args.push(deny_apps.join(","));
    }
    exec_adb(serial, adb_args)
}

pub fn cmd_autostart(
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
    mtu: u16,
    allow_wifi: bool,
    socks5: Option<&str>,
) -> Result<(), CommandExecutionError> {
    let start_dns_servers = dns_servers.map(String::from);
    let start_routes = routes.map(String::from);
    let start_socks5 = socks5.map(String::from);
    let mut adb_monitor = AdbMonitor::new(Box::new(move |serial: &str| {
        let dns_servers = start_dns_servers.as_ref().map(String::as_ref);
        let routes = start_routes.as_ref().map(String::as_ref);
        let socks5 = start_socks5.as_ref().map(String::as_ref);
        async_start(Some(serial), dns_servers, routes, port, None, None, mtu, &[], &[], socks5)
    }));
    adb_monitor.set_usb_only(!allow_wifi);
    adb_monitor.monitor();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn async_start(
    serial: Option<&str>,
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
    proxy: Option<&str>,
    proxy_exclusions: Option<&str>,
    mtu: u16,
    allow_apps: &[String],
    deny_apps: &[String],
    socks5: Option<&str>,
) {
    let start_serial = serial.map(String::from);
    let start_dns_servers = dns_servers.map(String::from);
    let start_routes = routes.map(String::from);
    let start_proxy = proxy.map(String::from);
    let start_exclusions = proxy_exclusions.map(String::from);
    let start_socks5 = socks5.map(String::from);
    let allow_apps_owned = allow_apps.to_vec();
    let deny_apps_owned = deny_apps.to_vec();
    thread::spawn(move || {
        let serial = start_serial.as_ref().map(String::as_ref);
        let dns_servers = start_dns_servers.as_ref().map(String::as_ref);
        let routes = start_routes.as_ref().map(String::as_ref);
        let proxy = start_proxy.as_ref().map(String::as_ref);
        let exclusions = start_exclusions.as_ref().map(String::as_ref);
        let socks5 = start_socks5.as_ref().map(String::as_ref);
        if let Err(err) = cmd_start(serial, dns_servers, routes, port, proxy, exclusions, mtu, &allow_apps_owned, &deny_apps_owned, socks5) {
            crate::execution_error::print_error(&err);
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_run(
    serial: Option<&str>,
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
    proxy: Option<&str>,
    proxy_exclusions: Option<&str>,
    _stop_on_disconnect: bool,
    mtu: u16,
    allow_apps: &[String],
    deny_apps: &[String],
    socks5: Option<&str>,
) -> Result<(), CommandExecutionError> {
    if let Some(proxy) = socks5
        && let Ok(addr) = proxy.parse::<std::net::SocketAddr>() {
            let _ = tcp_connection::SOCKS5_PROXY.set(addr);
        }
    async_start(serial, dns_servers, routes, port, proxy, proxy_exclusions, mtu, allow_apps, deny_apps, socks5);

    let ctrlc_serial = serial.map(String::from);
    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        CommandExecutionError::Io(std::io::Error::other(e))
    })?;

    rt.block_on(async {
        tokio::select! {
            result = tokio::task::spawn_blocking(move || cmd_relay(port)) => {
                result.map_err(|e| {
                    CommandExecutionError::Io(std::io::Error::other(e))
                })?
            }
            _ = tokio::signal::ctrl_c() => {
                info!(target: TAG, "Interrupted");
                if let Err(err) = cmd_stop(ctrlc_serial.as_deref()) {
                    error!(target: TAG, "Cannot stop client: {}", err);
                }
                std::process::exit(0);
            }
        }
    })
}

pub fn cmd_autorun(
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
    _stop_on_disconnect: bool,
    mtu: u16,
    allow_wifi: bool,
    socks5: Option<&str>,
) -> Result<(), CommandExecutionError> {
    if let Some(proxy) = socks5
        && let Ok(addr) = proxy.parse::<std::net::SocketAddr>() {
            let _ = tcp_connection::SOCKS5_PROXY.set(addr);
        }
    {
        let autostart_dns_servers = dns_servers.map(String::from);
        let autostart_routes = routes.map(String::from);
        let autostart_socks5 = socks5.map(String::from);
        thread::spawn(move || {
            let dns_servers = autostart_dns_servers.as_ref().map(String::as_ref);
            let routes = autostart_routes.as_ref().map(String::as_ref);
            let socks5 = autostart_socks5.as_ref().map(String::as_ref);
            if let Err(err) = cmd_autostart(dns_servers, routes, port, mtu, allow_wifi, socks5) {
                error!(target: TAG, "Cannot auto start clients: {}", err);
            }
        });
    }

    cmd_relay(port)
}

pub fn cmd_restart(
    serial: Option<&str>,
    dns_servers: Option<&str>,
    routes: Option<&str>,
    port: u16,
) -> Result<(), CommandExecutionError> {
    cmd_stop(serial)?;
    cmd_start(serial, dns_servers, routes, port, None, None, 0x4000, &[], &[], None)?;
    Ok(())
}
