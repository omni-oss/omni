use std::collections::HashMap;

use async_trait::async_trait;
use bridge_rpc_core::{
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use bridge_rpc_utils::server::read_request_as_json;
use log::Log;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct LogService<L> {
    logger: L,
}

impl LogService<&'static dyn Log> {
    pub fn new_with_default_logger() -> Self {
        Self::new(log::logger())
    }
}

impl<L: Log + 'static> LogService<L> {
    pub fn new(logger: L) -> Self {
        Self { logger }
    }
}

#[async_trait]
impl<L: Log + 'static> Service for LogService<L> {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let reader = context.request.into_reader();

        let (json, _trailers) = read_request_as_json::<LogRecord>(reader)
            .await
            .map_err(ServiceError::custom_error)?;

        if let Some(fields) = json.fields {
            log::log!(
                logger: &self.logger,
                target: &json.target.join("::"),
                json.level.to_log_level(),
                fields:serde,
                timestamp = json.timestamp;
                "{}",
                json.message,
            );
        } else {
            log::log!(
                logger: &self.logger,
                target: &json.target.join("::"),
                json.level.to_log_level(),
                timestamp = json.timestamp;
                "{}",
                json.message
            );
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct LogRecord {
    pub level: LogLevel,
    pub target: Vec<String>,
    pub message: String,
    pub fields: Option<HashMap<String, serde_json::Value>>,
    pub timestamp: u64,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn to_log_level(self) -> log::Level {
        match self {
            LogLevel::Error => log::Level::Error,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Info => log::Level::Info,
            LogLevel::Debug => log::Level::Debug,
            LogLevel::Trace => log::Level::Trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bridge_rpc_core::service::Service;
    use log::{Level, Log, Metadata, Record};
    use serde_json::json;

    use super::LogService;
    use crate::services::test_harness::ServiceContextBuilder;

    /// A snapshot of a single [`log::Record`] captured by
    /// [`CapturingLogger`].
    #[derive(Debug, Clone)]
    struct CapturedRecord {
        level: Level,
        target: String,
        message: String,
    }

    /// A [`Log`] implementation that simply pushes every record it sees into
    /// a shared, lock-protected vector. Cheap to clone (the storage is
    /// reference-counted), and intended to be cloned once into the service
    /// under test while the original handle is kept by the test for
    /// inspection.
    #[derive(Clone, Default)]
    struct CapturingLogger {
        records: Arc<Mutex<Vec<CapturedRecord>>>,
    }

    impl CapturingLogger {
        fn snapshot(&self) -> Vec<CapturedRecord> {
            self.records.lock().unwrap().clone()
        }
    }

    impl Log for CapturingLogger {
        fn enabled(&self, _: &Metadata) -> bool {
            true
        }

        fn log(&self, record: &Record) {
            self.records.lock().unwrap().push(CapturedRecord {
                level: record.level(),
                target: record.target().to_string(),
                message: record.args().to_string(),
            });
        }

        fn flush(&self) {}
    }

    async fn run_log_service(
        body: serde_json::Value,
    ) -> (
        CapturingLogger,
        Result<(), bridge_rpc_core::service_error::ServiceError>,
    ) {
        let logger = CapturingLogger::default();
        let service = LogService::new(logger.clone());

        let (ctx, _awaiter) = ServiceContextBuilder::new("/log")
            .with_body_json(&body)
            .build()
            .await;

        let result = service.run(ctx).await;

        (logger, result)
    }

    #[tokio::test]
    async fn logs_message_without_fields() {
        let body = json!({
            "level": "info",
            "target": ["my", "module"],
            "message": "hello world",
            "timestamp": 1_700_000_000u64,
        });

        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");

        let records = logger.snapshot();
        assert_eq!(records.len(), 1, "expected exactly one record");
        assert_eq!(records[0].level, Level::Info);
        assert_eq!(records[0].target, "my::module");
        assert_eq!(records[0].message, "hello world");
    }

    #[tokio::test]
    async fn logs_message_with_fields() {
        let body = json!({
            "level": "warn",
            "target": ["with", "fields"],
            "message": "something happened",
            "timestamp": 1_700_000_001u64,
            "fields": {
                "user_id": 42,
                "action": "login",
            },
        });

        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");

        let records = logger.snapshot();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, Level::Warn);
        assert_eq!(records[0].target, "with::fields");
        assert_eq!(records[0].message, "something happened");
    }

    #[tokio::test]
    async fn logs_message_with_empty_fields_object() {
        // An empty `fields` map still goes through the `Some(fields)` branch.
        let body = json!({
            "level": "debug",
            "target": ["empty", "fields"],
            "message": "no extra fields",
            "timestamp": 1_700_000_002u64,
            "fields": {},
        });

        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");

        let records = logger.snapshot();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, Level::Debug);
        assert_eq!(records[0].target, "empty::fields");
        assert_eq!(records[0].message, "no extra fields");
    }

    #[tokio::test]
    async fn maps_log_level_error() {
        let body = json!({
            "level": "error",
            "target": ["t"],
            "message": "e",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].level, Level::Error);
    }

    #[tokio::test]
    async fn maps_log_level_warn() {
        let body = json!({
            "level": "warn",
            "target": ["t"],
            "message": "w",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].level, Level::Warn);
    }

    #[tokio::test]
    async fn maps_log_level_info() {
        let body = json!({
            "level": "info",
            "target": ["t"],
            "message": "i",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].level, Level::Info);
    }

    #[tokio::test]
    async fn maps_log_level_debug() {
        let body = json!({
            "level": "debug",
            "target": ["t"],
            "message": "d",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].level, Level::Debug);
    }

    #[tokio::test]
    async fn maps_log_level_trace() {
        let body = json!({
            "level": "trace",
            "target": ["t"],
            "message": "tr",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].level, Level::Trace);
    }

    #[tokio::test]
    async fn target_with_single_segment_has_no_separator() {
        let body = json!({
            "level": "info",
            "target": ["only"],
            "message": "m",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].target, "only");
    }

    #[tokio::test]
    async fn target_with_many_segments_is_joined_with_double_colon() {
        let body = json!({
            "level": "info",
            "target": ["a", "b", "c", "d"],
            "message": "m",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].target, "a::b::c::d");
    }

    #[tokio::test]
    async fn empty_target_array_produces_empty_target_string() {
        let body = json!({
            "level": "info",
            "target": [],
            "message": "m",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;
        result.expect("service should succeed");
        assert_eq!(logger.snapshot()[0].target, "");
    }

    #[tokio::test]
    async fn returns_error_on_invalid_json_body() {
        let logger = CapturingLogger::default();
        let service = LogService::new(logger.clone());

        let (ctx, _awaiter) = ServiceContextBuilder::new("/log")
            .with_body_bytes(b"not valid json".to_vec())
            .build()
            .await;

        let result = service.run(ctx).await;
        assert!(
            result.is_err(),
            "expected service to error on invalid JSON body"
        );
        assert!(
            logger.snapshot().is_empty(),
            "no records should have been captured"
        );
    }

    #[tokio::test]
    async fn returns_error_on_unknown_log_level() {
        let body = json!({
            "level": "fatal",
            "target": ["t"],
            "message": "m",
            "timestamp": 1u64,
        });
        let (logger, result) = run_log_service(body).await;

        assert!(
            result.is_err(),
            "expected service to error on unknown log level"
        );
        assert!(
            logger.snapshot().is_empty(),
            "no records should have been captured"
        );
    }

    #[tokio::test]
    async fn default_logger_constructor_compiles_and_runs() {
        // We can't observe what the global logger does without installing
        // a custom one (which would conflict with other tests), but we can
        // at least verify that the convenience constructor produces a
        // working service that completes without erroring on a valid
        // request.
        let service = LogService::new_with_default_logger();

        let body = json!({
            "level": "info",
            "target": ["default", "logger"],
            "message": "hi",
            "timestamp": 1u64,
        });

        let (ctx, _awaiter) = ServiceContextBuilder::new("/log")
            .with_body_json(&body)
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");
    }

    #[tokio::test]
    async fn does_not_emit_response_frames() {
        let body = json!({
            "level": "info",
            "target": ["t"],
            "message": "m",
            "timestamp": 1u64,
        });

        let logger = CapturingLogger::default();
        let service = LogService::new(logger.clone());

        let (ctx, mut awaiter) = ServiceContextBuilder::new("/log")
            .with_body_json(&body)
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");

        // The log service consumes the request and returns without ever
        // starting a response, so the response channel should be drained
        // and closed.
        assert!(
            awaiter.is_drained(),
            "log service should not produce any response frames"
        );
    }
}
