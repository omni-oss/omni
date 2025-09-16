use crossbeam_channel::Sender;
use derive_new::new;
use std::io::{self, Write};

#[derive(Clone, Debug, new)]
pub struct InMemoryTracer {
    tx: Sender<Vec<u8>>,
}

impl Write for InMemoryTracer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx
            .send(buf.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
