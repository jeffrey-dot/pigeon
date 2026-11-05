//! Private configuration for real PTY tests; user rc files never enter the host.

use std::path::Path;
use std::process::Command;

pub fn command(socket: &str, program: &str) -> Command {
    let home = Path::new(socket).with_extension("home");
    std::fs::create_dir_all(home.join(".config")).expect("private config directory");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pigeon-server"));
    command.env_clear();
    command.env("HOME", &home);
    command.env("XDG_CONFIG_HOME", home.join(".config"));
    command.env("SHELL", "/bin/sh");
    command.env("TERM", "xterm-256color");
    command.env(
        "PATH",
        std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
    );
    if let Some(locale) = std::env::var_os("LANG") {
        command.env("LANG", locale);
    }
    // Diagnostics and the host's own test knobs pass through; everything else is cleared.
    for name in ["PIGEON_DEBUG", "PIGEON_IDLE_MS", "PIGEON_HELLO_MS"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.args(["--socket", socket, "--shell", program]);
    command
}

pub fn cleanup(socket: &str) {
    let _ = std::fs::remove_dir_all(Path::new(socket).with_extension("home"));
}
