use std::path::PathBuf;

use omni_tracing_subscriber::{TraceLevel, TracingConfig, TracingSubscriber};

pub fn init_tracing() -> eyre::Result<()> {
    let tracing_config = TracingConfig {
        stderr_trace_enabled: true,
        stdout_trace_level: TraceLevel::Info,
        file_path: Some(PathBuf::from(
            "./.omni/trace/omni-remote-cache-service.log",
        )),
        file_trace_level: TraceLevel::Off,
    };

    let sub = TracingSubscriber::new(&tracing_config, vec![])?;

    tracing::subscriber::set_global_default(sub)?;

    Ok(())
}
