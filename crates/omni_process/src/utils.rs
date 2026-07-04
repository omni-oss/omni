use portable_pty::PtySize;
use terminal_size::{Height, Width};

pub fn get_pty_size() -> PtySize {
    let terminal_size =
        terminal_size::terminal_size().unwrap_or((Width(80), Height(24)));
    PtySize {
        cols: terminal_size.0.0,
        rows: terminal_size.1.0,
        pixel_height: 0,
        pixel_width: 0,
    }
}

pub fn should_use_pty() -> bool {
    // Escape hatch (wins on every platform): OMNI_PTY=1/true/on/yes forces the
    // pty path, OMNI_PTY=0/false/off/no forces the piped path.
    if let Ok(v) = std::env::var("OMNI_PTY") {
        match v.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "off" | "no" => return false,
            "1" | "true" | "on" | "yes" => return true,
            _ => {}
        }
    }

    // ConPTY is unreliable in many Windows environments: child spawns can fail
    // during DLL init with STATUS_DLL_INIT_FAILED (exit code 3221225794), even
    // for plain console programs like cmd.exe. Default to the piped path on
    // Windows so tasks run reliably; opt in with OMNI_PTY=1 where ConPTY works.
    // Note: interactive stdin passthrough to programs that require a real tty
    // (e.g. dev servers) only works under the pty path.
    if cfg!(windows) {
        return false;
    }

    atty::is(atty::Stream::Stdout)
}
