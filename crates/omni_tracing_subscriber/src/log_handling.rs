use tracing_core::{Event, Metadata};
use tracing_log::NormalizeEvent;
use tracing_subscriber::layer::{Context, Filter};

pub(crate) struct LogFilter;

impl<S> Filter<S> for LogFilter {
    // Check dynamic values for Events
    fn event_enabled(&self, event: &Event<'_>, _cx: &Context<'_, S>) -> bool {
        event.is_log()
    }
    fn enabled(&self, _meta: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        true
    }
}
