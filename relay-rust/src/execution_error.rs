use std::error;
use std::fmt;
use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

// ANSI color codes for terminal output
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// Severity level for user-facing messages.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Yellow — something is suboptimal but recoverable (latency, packet loss).
    Warning,
    /// Red — something failed and needs user action (disconnect, missing ADB).
    Error,
}

#[derive(Debug)]
pub enum CommandExecutionError {
    ProcessIo(ProcessIoError),
    ProcessStatus(ProcessStatusError),
    Io(io::Error),
}

impl CommandExecutionError {
    pub fn severity(&self) -> Severity {
        match self {
            CommandExecutionError::Io(err) => {
                // Connection resets and timeouts are warnings; real I/O faults are errors
                match err.kind() {
                    io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::WouldBlock => Severity::Warning,
                    _ => Severity::Error,
                }
            }
            CommandExecutionError::ProcessStatus(err) => {
                // ADB exit code 1 often means device not found or command rejected
                if matches!(err.termination, Termination::Value(1)) {
                    Severity::Warning
                } else {
                    Severity::Error
                }
            }
            CommandExecutionError::ProcessIo(_) => Severity::Error,
        }
    }

    /// Return a human-readable suggestion for fixing this error.
    pub fn suggestion(&self) -> &'static str {
        match self {
            CommandExecutionError::ProcessIo(err) => {
                if err.error.kind() == io::ErrorKind::NotFound {
                    "Install adb or set the ADB env var"
                } else {
                    "Run `adb start-server`. Try a different USB cable or port"
                }
            }
            CommandExecutionError::ProcessStatus(err) => match err.termination {
                Termination::Value(1) => {
                    "Check `adb devices` shows your device and USB debugging is enabled"
                }
                Termination::Value(_) => {
                    "Restart ADB: `adb kill-server && adb start-server`"
                }
                #[cfg(unix)]
                Termination::Signal(_) => {
                    "ADB was killed by the system. Restart the daemon"
                }
            },
            CommandExecutionError::Io(err) => match err.kind() {
                io::ErrorKind::NotFound => {
                    "APK not found. Download from releases, build with `make apk`, or set GNIREHTET_APK"
                }
                io::ErrorKind::ConnectionRefused => {
                    "Port in use or no device connected. Use `-p PORT` to change it"
                }
                io::ErrorKind::TimedOut => {
                    "High latency or device disconnected. Check the cable"
                }
                io::ErrorKind::ConnectionReset => {
                    "Connection reset. Cable issue or device unplugged"
                }
                _ => "Check USB connection and try again",
            },
        }
    }
}

#[derive(Debug)]
pub struct ProcessStatusError {
    cmd: Cmd,
    termination: Termination,
}

#[derive(Debug)]
pub struct ProcessIoError {
    cmd: Cmd,
    pub error: io::Error,
}

#[derive(Debug)]
pub struct Cmd {
    command: String,
    args: Vec<String>,
}

#[derive(Debug)]
pub enum Termination {
    Value(i32),
    #[cfg(unix)]
    Signal(i32),
}

impl Termination {
    fn from(status: ExitStatus) -> Self {
        match status.code() {
            Some(code) => Termination::Value(code),
            #[cfg(unix)]
            None => Termination::Signal(status.signal().unwrap_or(-1)),
            #[cfg(not(unix))]
            None => Termination::Value(-1),
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {:?}", self.command, self.args)
    }
}

impl Cmd {
    pub fn new<S1, S2>(command: S1, args: Vec<S2>) -> Cmd
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        Self {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect::<Vec<_>>(),
        }
    }
}

impl ProcessStatusError {
    pub fn new(cmd: Cmd, status: ExitStatus) -> Self {
        Self {
            cmd,
            termination: Termination::from(status),
        }
    }
}

impl fmt::Display for ProcessStatusError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.termination {
            Termination::Value(code) => {
                write!(f, "{} returned {}", self.cmd, code)
            }
            #[cfg(unix)]
            Termination::Signal(sig) => {
                write!(f, "{} killed by signal {}", self.cmd, sig)
            }
        }
    }
}

impl error::Error for ProcessStatusError {}

impl ProcessIoError {
    pub fn new(cmd: Cmd, error: io::Error) -> Self {
        Self { cmd, error }
    }
}

impl fmt::Display for ProcessIoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} failed: {}", self.cmd, self.error)
    }
}

impl error::Error for ProcessIoError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.error)
    }
}

impl fmt::Display for CommandExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CommandExecutionError::ProcessIo(ref err) => err.fmt(f),
            CommandExecutionError::ProcessStatus(ref err) => err.fmt(f),
            CommandExecutionError::Io(ref err) => write!(f, "IO: {}", err),
        }
    }
}

impl error::Error for CommandExecutionError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            CommandExecutionError::ProcessIo(ref err) => Some(err),
            CommandExecutionError::ProcessStatus(ref err) => Some(err),
            CommandExecutionError::Io(ref err) => Some(err),
        }
    }
}

impl From<ProcessIoError> for CommandExecutionError {
    fn from(error: ProcessIoError) -> Self {
        CommandExecutionError::ProcessIo(error)
    }
}

impl From<ProcessStatusError> for CommandExecutionError {
    fn from(error: ProcessStatusError) -> Self {
        CommandExecutionError::ProcessStatus(error)
    }
}

impl From<io::Error> for CommandExecutionError {
    fn from(error: io::Error) -> Self {
        CommandExecutionError::Io(error)
    }
}

/// Print a formatted error with a suggestion. One line, no label.
pub fn print_error(err: &CommandExecutionError) {
    let severity_mark = if err.severity() == Severity::Error { "!" } else { "?" };
    let prefix = if err.severity() == Severity::Error { RED } else { YELLOW };
    eprintln!("{}{} {}. {}{}", prefix, severity_mark, err, err.suggestion(), RESET);
}
