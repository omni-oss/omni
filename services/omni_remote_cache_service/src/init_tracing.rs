use std::path::PathBuf;

use omni_tracing_subscriber::{Level, TracingConfig, TracingSubscriber};

pub fn init_tracing() -> eyre::Result<()> {
    let tracing_config = TracingConfig {
        stderr_filter: Some("info".to_string()),
        file_path: Some(PathBuf::from(
            "./.omni/trace/omni-remote-cache-service.log",
        )),
        file_level: Level::Off,
    };

    let sub = TracingSubscriber::new(&tracing_config, vec![])?;

    tracing::subscriber::set_global_default(sub)?;

    Ok(())
}
