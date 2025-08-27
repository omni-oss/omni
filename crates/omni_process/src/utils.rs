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
    !cfg!(windows) && atty::is(atty::Stream::Stdout)
}
