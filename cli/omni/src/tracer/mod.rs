use std::fs;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use derive_more::FromStr;
use eyre::OptionExt;
use strum::EnumIs;
use strum::FromRepr;
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing::span;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    clap::ValueEnum,
    FromRepr,
    FromStr,
    EnumIs,
)]
#[repr(u8)]
pub enum TraceLevel {
    None = 0,
    Error = 1,
    Warn = 2,
    #[default]
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl From<TraceLevel> for LevelFilter {
    fn from(value: TraceLevel) -> Self {
        match value {
            TraceLevel::None => LevelFilter::OFF,
            TraceLevel::Error => LevelFilter::ERROR,
            TraceLevel::Warn => LevelFilter::WARN,
            TraceLevel::Info => LevelFilter::INFO,
            TraceLevel::Debug => LevelFilter::DEBUG,
            TraceLevel::Trace => LevelFilter::TRACE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TracerConfig {
    pub stdout_trace_level: TraceLevel,
    pub file_trace_level: TraceLevel,
    pub file_path: Option<PathBuf>,
    pub stderr_trace_enabled: bool,
}

pub struct TracerSubscriber {
    inner: Box<dyn Subscriber + Send + Sync>,
}

impl TracerSubscriber {
    pub fn new(config: &TracerConfig) -> eyre::Result<Self> {
        let mut layers = Vec::new();

        let main_filters = Targets::new()
            .with_target("globset", LevelFilter::OFF)
            .with_target("ignore", LevelFilter::OFF);

        if !config.stdout_trace_level.is_none() {
            let filter: LevelFilter = config.stdout_trace_level.into();
            let stdout_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .pretty()
                .with_file(false)
                .without_time()
                .with_target(false)
                .with_line_number(false)
                .with_filter(main_filters.clone().with_default(filter))
                .boxed();

            layers.push(stdout_layer);
        }

        if !config.stderr_trace_enabled {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(
                    main_filters.clone().with_default(LevelFilter::ERROR),
                )
                .boxed();

            layers.push(stderr_layer);
        }

        if !config.file_trace_level.is_none() {
            let file_path = config
                .file_path
                .as_ref()
                .ok_or_eyre("File path for file trace not set")?;

            fs::create_dir_all(
                file_path.parent().ok_or_eyre("Can't get parent")?,
            )?;

            let filter: LevelFilter = config.file_trace_level.into();

            let file_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_writer(Arc::new(File::create(file_path)?))
                .with_filter(main_filters.clone().with_default(filter))
                .boxed();

            layers.push(file_layer);
        }

        Ok(Self {
            inner: Box::new(Registry::default().with(layers)),
        })
    }
}

impl Subscriber for TracerSubscriber {
    #[inline(always)]
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    #[inline(always)]
    fn register_callsite(
        &self,
        metadata: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        self.inner.register_callsite(metadata)
    }

    #[inline(always)]
    fn clone_span(&self, id: &span::Id) -> span::Id {
        self.inner.clone_span(id)
    }

    #[inline(always)]
    fn try_close(&self, id: span::Id) -> bool {
        self.inner.try_close(id)
    }

    #[inline(always)]
    fn event_enabled(&self, event: &tracing::Event<'_>) -> bool {
        self.inner.event_enabled(event)
    }

    #[inline(always)]
    fn on_register_dispatch(&self, subscriber: &tracing::Dispatch) {
        self.inner.on_register_dispatch(subscriber)
    }

    #[inline(always)]
    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.inner.max_level_hint()
    }

    #[inline(always)]
    unsafe fn downcast_raw(&self, id: std::any::TypeId) -> Option<*const ()> {
        unsafe { self.inner.downcast_raw(id) }
    }

    #[inline(always)]
    fn drop_span(&self, _id: span::Id) {
        #[allow(deprecated)]
        self.inner.drop_span(_id)
    }

    #[inline(always)]
    fn current_span(&self) -> tracing_core::span::Current {
        self.inner.current_span()
    }

    #[inline(always)]
    fn new_span(&self, span: &span::Attributes<'_>) -> span::Id {
        self.inner.new_span(span)
    }

    #[inline(always)]
    fn record(&self, span: &span::Id, values: &span::Record<'_>) {
        self.inner.record(span, values)
    }

    #[inline(always)]
    fn record_follows_from(&self, span: &span::Id, follows: &span::Id) {
        self.inner.record_follows_from(span, follows)
    }

    #[inline(always)]
    fn event(&self, event: &tracing::Event<'_>) {
        self.inner.event(event)
    }

    #[inline(always)]
    fn enter(&self, span: &span::Id) {
        self.inner.enter(span)
    }

    #[inline(always)]
    fn exit(&self, span: &span::Id) {
        self.inner.exit(span)
    }
}
