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
use std::process;
use std::sync::OnceLock;

use crate::execution_error::{Cmd, CommandExecutionError, ProcessIoError, ProcessStatusError};

const TAG: &str = "Adb";

pub const REQUIRED_APK_VERSION_CODE: &str = "10";

static ADB_PATH: OnceLock<String> = OnceLock::new();

/// Ensure ADB is available — either from PATH, the `ADB` env var, or
/// downloaded and extracted automatically.
pub fn ensure_adb() {
    if ADB_PATH.get().is_some() {
        return;
    }

    // 1. Check ADB env var
    if let Some(env_adb) = std::env::var_os("ADB") {
        let path = match env_adb.into_string() {
            Ok(p) => p,
            Err(_) => {
                warn!(target: TAG, "ADB env var contains invalid UTF-8, ignoring");
                "adb".to_string()
            }
        };
        info!(target: TAG, "Using ADB from environment: {}", path);
        let _ = ADB_PATH.set(path);
        return;
    }

    // 2. Try running `adb version`
    if let Ok(status) = process::Command::new("adb").arg("version").status()
        && status.success() {
            info!(target: TAG, "ADB found in PATH");
            let _ = ADB_PATH.set("adb".to_string());
            return;
        }

    // 3. Download and extract ADB automatically
    info!(target: TAG, "ADB not found in PATH, downloading...");
    match download_and_extract_adb() {
        Ok(path) => {
            let _ = ADB_PATH.set(path);
        }
        Err(e) => {
            error!(target: TAG, "Failed to download ADB: {}", e);
            // Fall back to bare "adb" — a proper error will surface later
            let _ = ADB_PATH.set("adb".to_string());
        }
    }
}

/// Return the OS-specific platform string used in the download URL.
fn get_platform() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        compile_error!("Unsupported platform");
    }
}

/// Download platform-tools and extract ADB (and Windows DLLs) next to the
/// gnirehtet binary.
fn download_and_extract_adb() -> Result<String, String> {
    let platform = get_platform();

    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Cannot determine binary path: {}", e))?
        .parent()
        .ok_or_else(|| "Cannot get binary directory".to_string())?
        .to_path_buf();

    #[cfg(unix)]
    let adb_binary = exe_dir.join("adb");
    #[cfg(windows)]
    let adb_binary = exe_dir.join("adb.exe");

    if adb_binary.exists() {
        info!(target: TAG, "ADB already present at {}", adb_binary.display());
        return Ok(adb_binary.to_string_lossy().to_string());
    }

    let url = format!(
        "https://dl.google.com/android/repository/platform-tools-latest-{}.zip",
        platform
    );

    let tmp_dir = std::env::temp_dir().join("gnirehtet-adb");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir {}: {}", tmp_dir.display(), e))?;
    let zip_path = tmp_dir.join("platform-tools-latest.zip");

    info!(target: TAG, "Downloading ADB from {}...", url);
    download(&url, &zip_path)?;

    info!(target: TAG, "Extracting ADB to {}...", exe_dir.display());
    extract_adb(&zip_path, &exe_dir, platform)?;

    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_dir(&tmp_dir);

    if !adb_binary.exists() {
        return Err(format!(
            "ADB binary not found at {} after extraction",
            adb_binary.display()
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&adb_binary, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    println!("ADB extracted to {}", adb_binary.display());

    #[cfg(target_os = "linux")]
    if is_command_available("apt") {
        println!("Tip: install system-wide with `sudo apt install adb`");
    }

    Ok(adb_binary.to_string_lossy().to_string())
}

/// Download a file via `curl`. Keeps dependencies minimal.
fn download(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("curl")
        .args(["-fLo", &dest.to_string_lossy(), url])
        .status()
        .map_err(|e| format!("Failed to run curl: {}", e))?;
    if !status.success() {
        return Err("Download failed".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
/// Check whether a command is available on the system (`which`).
fn is_command_available(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Extract only the ADB-related files from the platform-tools zip.
fn extract_adb(
    zip_path: &std::path::Path,
    dest_dir: &std::path::Path,
    platform: &str,
) -> Result<(), String> {
    let files_to_extract: Vec<&str> = if platform == "windows" {
        vec![
            "platform-tools/adb.exe",
            "platform-tools/AdbWinApi.dll",
            "platform-tools/AdbWinUsbApi.dll",
        ]
    } else {
        vec!["platform-tools/adb"]
    };

    // unzip -o <zip> <file1> <file2> ... -d <dest>
    let mut args = vec![
        "-o".to_string(),
        zip_path.to_string_lossy().to_string(),
    ];
    for f in &files_to_extract {
        args.push(f.to_string());
    }
    args.push("-d".to_string());
    args.push(dest_dir.to_string_lossy().to_string());

    let status = std::process::Command::new("unzip")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to run unzip: {}", e))?;
    if !status.success() {
        return Err("Extraction failed".into());
    }
    Ok(())
}

/// Return the path to the ADB binary (resolved by `ensure_adb()` or via
/// environment / fallback).
pub fn get_adb_path() -> String {
    ADB_PATH.get().cloned().unwrap_or_else(|| {
        // Backward-compatible fallback if ensure_adb() wasn't called.
        if let Some(env_adb) = std::env::var_os("ADB") {
            env_adb.into_string().unwrap_or_else(|_| { warn!(target: TAG, "Invalid ADB value"); "adb".to_string() })
        } else {
            "adb".to_string()
        }
    })
}

pub fn get_apk_path() -> String {
    if let Some(env_adb) = std::env::var_os("GNIREHTET_APK") {
        env_adb.into_string().unwrap_or_else(|_| { warn!(target: TAG, "Invalid GNIREHTET_APK value"); "gnirehtet.apk".to_string() })
    } else {
        "gnirehtet.apk".to_string()
    }
}

pub fn create_adb_args<S: Into<String>>(serial: Option<&str>, args: Vec<S>) -> Vec<String> {
    let mut command = Vec::<String>::new();
    if let Some(serial) = serial {
        command.push("-s".into());
        command.push(serial.to_string());
    }
    for arg in args {
        command.push(arg.into());
    }
    command
}

pub fn exec_adb<S: Into<String>>(
    serial: Option<&str>,
    args: Vec<S>,
) -> Result<(), CommandExecutionError> {
    let adb_args = create_adb_args(serial, args);
    let adb = get_adb_path();
    debug!(target: TAG, "Execute: {:?} {:?}", adb, adb_args);
    match process::Command::new(&adb).args(&adb_args[..]).status() {
        Ok(exit_status) => {
            if exit_status.success() {
                Ok(())
            } else {
                let cmd = Cmd::new(adb, adb_args);
                Err(ProcessStatusError::new(cmd, exit_status).into())
            }
        }
        Err(err) => {
            let cmd = Cmd::new(adb, adb_args);
            Err(ProcessIoError::new(cmd, err).into())
        }
    }
}

pub fn must_install_client(serial: Option<&str>) -> Result<bool, CommandExecutionError> {
    info!(target: TAG, "Checking gnirehtet client...");
    let args = create_adb_args(
        serial,
        vec!["shell", "dumpsys", "package", "com.genymobile.gnirehtet"],
    );
    let adb = get_adb_path();
    debug!(target: TAG, "Execute: {:?} {:?}", adb, args);
    match process::Command::new(&adb).args(&args[..]).output() {
        Ok(output) => {
            if output.status.success() {
                let dumpsys = String::from_utf8_lossy(&output.stdout[..]);
                if let Some(index) = dumpsys.find("    versionCode=") {
                    let start = index + 16;
                    if let Some(end) = dumpsys[start..].find(' ') {
                        let installed_version_code = &dumpsys[start..start + end];
                        Ok(installed_version_code != REQUIRED_APK_VERSION_CODE)
                    } else {
                        Ok(true)
                    }
                } else {
                    Ok(true)
                }
            } else {
                let cmd = Cmd::new(adb, args);
                Err(ProcessStatusError::new(cmd, output.status).into())
            }
        }
        Err(err) => {
            let cmd = Cmd::new(adb, args);
            Err(ProcessIoError::new(cmd, err).into())
        }
    }
}
