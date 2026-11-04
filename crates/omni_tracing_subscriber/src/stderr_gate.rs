//! A stderr writer that can be paused while a live display owns the terminal.
//!
//! A progress bar or alternate-screen TUI is corrupted by stray stderr lines.
//! While paused, trace lines are buffered; resuming flushes them so nothing is
//! lost. `--trace-file` is the supported route for traces during such a run.

use std::io::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static PAUSED: AtomicBool = AtomicBool::new(false);
static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Buffers stderr traces until [`resume_stderr`] is called.
pub fn pause_stderr() {
    PAUSED.store(true, Ordering::SeqCst);
}

/// Flushes anything buffered while paused and resumes writing directly.
pub fn resume_stderr() {
    if PAUSED.swap(false, Ordering::SeqCst) {
        let buffered = std::mem::take(&mut *BUFFER.lock().unwrap());
        if !buffered.is_empty() {
            let _ = io::stderr().write_all(&buffered);
        }
    }
}

pub fn stderr_paused() -> bool {
    PAUSED.load(Ordering::SeqCst)
}

#[derive(Clone, Copy, Default)]
pub(crate) struct GatedStderr;

impl Write for GatedStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if PAUSED.load(Ordering::SeqCst) {
            BUFFER.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        } else {
            io::stderr().write(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if PAUSED.load(Ordering::SeqCst) {
            Ok(())
        } else {
            io::stderr().flush()
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for GatedStderr {
    type Writer = GatedStderr;

    fn make_writer(&'a self) -> Self::Writer {
        GatedStderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_writes_are_buffered_then_flushed() {
        pause_stderr();
        let mut w = GatedStderr;
        w.write_all(b"held").unwrap();
        assert!(stderr_paused());
        assert_eq!(&*BUFFER.lock().unwrap(), b"held");
        resume_stderr();
        assert!(!stderr_paused());
        assert!(BUFFER.lock().unwrap().is_empty());
    }
}
