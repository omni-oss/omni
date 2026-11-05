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

fn buffer() -> std::sync::MutexGuard<'static, Vec<u8>> {
    BUFFER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Buffers stderr traces until [`resume_stderr`] is called. The gate is
/// process-wide, and repeated calls do not require matching resumes.
pub fn pause_stderr() {
    let _guard = buffer();
    PAUSED.store(true, Ordering::SeqCst);
}

/// Writes buffered traces to stderr and resumes writing directly. Write errors
/// are ignored and the buffer is discarded even if the write fails.
pub fn resume_stderr() {
    let mut guard = buffer();
    if PAUSED.swap(false, Ordering::SeqCst) {
        let buffered = std::mem::take(&mut *guard);
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
        let mut guard = buffer();
        if PAUSED.load(Ordering::SeqCst) {
            guard.extend_from_slice(buf);
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
