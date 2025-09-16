use derive_builder::Builder;
use derive_new::new;
use enumflags2::{BitFlags, bitflags};
use parking_lot::Mutex;
use std::sync::Arc;
use tracing_subscriber::fmt::MakeWriter;

use crate::TraceLevel;

#[derive(Clone)]
pub struct CustomOutput {
    pub(crate) config: CustomOutputConfig,
    pub(crate) factory: CustomOutputFactory,
}

impl CustomOutput {
    pub fn new_instance<W>(config: CustomOutputConfig, writer: W) -> Self
    where
        W: std::io::Write + Send + Sync + 'static,
    {
        Self {
            config,
            factory: CustomOutputFactory::new_instance(writer),
        }
    }

    pub fn new_factory<W, F>(config: CustomOutputConfig, factory: F) -> Self
    where
        W: std::io::Write + Send + Sync + 'static,
        F: Fn() -> W + Send + Sync + 'static,
    {
        Self {
            config,
            factory: CustomOutputFactory::new_factory(factory),
        }
    }
}

#[derive(Debug, Clone, Copy, Builder)]
#[builder(setter(into, strip_option))]
pub struct CustomOutputConfig {
    pub trace_level: TraceLevel,
    pub output_type: OutputType,
}

#[derive(Debug, Clone, Copy, new)]
pub enum OutputType {
    Json { options: FormatOptions },
    Text { options: FormatOptions },
}

impl Default for OutputType {
    fn default() -> Self {
        Self::Text {
            options: FormatOptions::default(),
        }
    }
}

#[bitflags(default = Pretty |  WithAnsi | WithTimestamp | WithLevel | WithThreadId | WithThreadName )]
#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum FormatOption {
    Pretty,
    WithLineNumber,
    WithTarget,
    WithAnsi,
    WithTimestamp,
    WithLevel,
    WithThreadId,
    WithThreadName,
    WithFileName,
}

pub type FormatOptions = BitFlags<FormatOption>;

#[derive(Clone)]
pub struct CustomOutputFactory {
    func: Arc<dyn (Fn() -> CustomOutputWriter) + Send + Sync>,
}

impl CustomOutputFactory {
    pub fn new_instance(
        writer: impl std::io::Write + Send + Sync + 'static,
    ) -> Self {
        let instance = Arc::new(Mutex::new(writer));

        Self {
            func: Arc::new(move || {
                CustomOutputWriter::SharedInstance(instance.clone())
            }),
        }
    }

    pub fn new_factory<
        W: std::io::Write + Send + Sync + 'static,
        F: Fn() -> W + Send + Sync + 'static,
    >(
        factory: F,
    ) -> Self {
        Self {
            func: Arc::new(move || {
                CustomOutputWriter::NewInstance(Box::new(factory()))
            }),
        }
    }
}

pub enum CustomOutputWriter {
    SharedInstance(Arc<Mutex<dyn std::io::Write + Send + Sync>>),
    NewInstance(Box<dyn std::io::Write + Send + Sync>),
}

impl std::io::Write for CustomOutputWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            CustomOutputWriter::SharedInstance(w) => w.lock().write(buf),
            CustomOutputWriter::NewInstance(w) => w.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            CustomOutputWriter::SharedInstance(w) => w.lock().flush(),
            CustomOutputWriter::NewInstance(w) => w.flush(),
        }
    }
}

impl<'a> MakeWriter<'a> for CustomOutputFactory {
    type Writer = CustomOutputWriter;

    fn make_writer(&'a self) -> Self::Writer {
        (self.func)()
    }
}
