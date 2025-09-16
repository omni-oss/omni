mod config;
pub mod custom_output;
mod trace_level;

pub use config::*;
pub use trace_level::*;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::FormatEvent;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::Format;

use std::fs;
use std::fs::File;
use std::sync::Arc;

use eyre::OptionExt;
use tracing_core::LevelFilter;
use tracing_core::Subscriber;
use tracing_core::span;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::Layer as FmtLayer;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::custom_output::CustomOutput;
use crate::custom_output::CustomOutputFactory;
use crate::custom_output::FormatOption;
use crate::custom_output::FormatOptions;

pub fn noop_subscriber() -> TracerSubscriber {
    TracerSubscriber::new(
        &TracingConfig {
            file_path: None,
            file_trace_level: TraceLevel::None,
            stderr_trace_enabled: false,
            stdout_trace_level: TraceLevel::None,
        },
        vec![],
    )
    .unwrap()
}

pub struct TracerSubscriber {
    inner: Box<dyn Subscriber + Send + Sync>,
}

impl TracerSubscriber {
    pub fn new(
        config: &TracingConfig,
        custom_outputs: Vec<CustomOutput>,
    ) -> eyre::Result<Self> {
        let mut layers = Vec::new();

        let main_filters = Targets::new()
            .with_target("globset", LevelFilter::OFF)
            .with_target("ignore", LevelFilter::OFF);

        if !config.stdout_trace_level.is_none() {
            let filter: LevelFilter = config.stdout_trace_level.into();
            let stdout_layer = tracing_subscriber::fmt::layer()
                .pretty()
                .without_time()
                .with_writer(std::io::stdout)
                .with_ansi(atty::is(atty::Stream::Stdout))
                .with_file(false)
                .with_target(false)
                .with_line_number(false)
                .with_filter(main_filters.clone().with_default(filter))
                .boxed();

            layers.push(stdout_layer);
        }

        if config.stderr_trace_enabled {
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_ansi(atty::is(atty::Stream::Stderr))
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

        if !custom_outputs.is_empty() {
            for output in custom_outputs {
                let out_type = output.config.output_type;
                let filter: LevelFilter = output.config.trace_level.into();
                let layer = match out_type {
                    custom_output::OutputType::Json { options } => {
                        if options.contains(FormatOption::Pretty) {
                            apply_settings(
                                tracing_subscriber::fmt::layer()
                                    .json()
                                    .pretty(),
                                &main_filters,
                                output.factory,
                                filter,
                                options,
                            )
                        } else {
                            apply_settings(
                                tracing_subscriber::fmt::layer().json(),
                                &main_filters,
                                output.factory,
                                filter,
                                options,
                            )
                        }
                    }
                    custom_output::OutputType::Text { options } => {
                        if options.contains(FormatOption::Pretty) {
                            apply_settings(
                                tracing_subscriber::fmt::layer().pretty(),
                                &main_filters,
                                output.factory,
                                filter,
                                options,
                            )
                        } else {
                            apply_settings(
                                tracing_subscriber::fmt::layer(),
                                &main_filters,
                                output.factory,
                                filter,
                                options,
                            )
                        }
                    }
                };

                layers.push(layer);
            }
        }

        Ok(Self {
            inner: Box::new(Registry::default().with(layers)),
        })
    }
}

fn apply_settings<N, L, T, W>(
    layer: FmtLayer<Registry, N, Format<L, T>, W>,
    main_filters: &Targets,
    factory: CustomOutputFactory,
    filter: LevelFilter,
    options: FormatOptions,
) -> Box<dyn tracing_subscriber::Layer<Registry> + Send + Sync + 'static>
where
    W: for<'writer> MakeWriter<'writer> + 'static,
    N: for<'writer> FormatFields<'writer> + 'static,
    Format<L, T>: FormatEvent<Registry, N>,
    Format<L, ()>: FormatEvent<Registry, N>,
    FmtLayer<Registry, N, Format<L, T>, CustomOutputFactory>:
        Layer<Registry> + Send + Sync + 'static,
    FmtLayer<Registry, N, Format<L, ()>, CustomOutputFactory>:
        Layer<Registry> + Send + Sync + 'static,
{
    let layer = layer
        .with_writer(factory)
        .with_line_number(options.contains(FormatOption::WithLineNumber))
        .with_thread_ids(options.contains(FormatOption::WithThreadId))
        .with_thread_names(options.contains(FormatOption::WithThreadName))
        .with_level(options.contains(FormatOption::WithLevel))
        .with_target(options.contains(FormatOption::WithTarget))
        .with_file(options.contains(FormatOption::WithFileName))
        .with_ansi(options.contains(FormatOption::WithAnsi));

    let filters = main_filters.clone().with_default(filter);

    if !options.contains(FormatOption::WithTimestamp) {
        layer.without_time().with_filter(filters).boxed()
    } else {
        layer
            .with_filter(main_filters.clone().with_default(filter))
            .boxed()
    }
}

impl Subscriber for TracerSubscriber {
    #[inline(always)]
    fn enabled(&self, metadata: &tracing_core::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    #[inline(always)]
    fn register_callsite(
        &self,
        metadata: &'static tracing_core::Metadata<'static>,
    ) -> tracing_core::subscriber::Interest {
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
    fn event_enabled(&self, event: &tracing_core::Event<'_>) -> bool {
        self.inner.event_enabled(event)
    }

    #[inline(always)]
    fn on_register_dispatch(&self, subscriber: &tracing_core::Dispatch) {
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
    fn event(&self, event: &tracing_core::Event<'_>) {
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
