use std::sync::Arc;

use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use super::store::{LogEntryBuilder, LogStore};
use super::types::*;

pub struct LoggingLayer {
    store: Arc<LogStore>,
    service: LogService,
}

impl LoggingLayer {
    pub fn new(store: Arc<LogStore>, service: LogService) -> Self {
        Self { store, service }
    }
}

impl<S> Layer<S> for LoggingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();

        // Map tracing level to our LogLevel
        let level = match *metadata.level() {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        };

        // Filter: only capture INFO+ from external crates, all levels from hr_* crates
        let module_path = metadata.module_path().unwrap_or("");
        let target = metadata.target();
        let is_hr_crate = module_path.starts_with("hr_")
            || module_path.starts_with("homeroute")
            || target.starts_with("hr_")
            || target.starts_with("homeroute");

        if !is_hr_crate && level < LogLevel::Info {
            return; // Skip TRACE/DEBUG from external crates (tokio, tungstenite, etc.)
        }

        // Extract fields
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.message.unwrap_or_default();

        // Skip health check requests to avoid log pollution
        if message.contains("/api/health") || message.contains("/health") {
            if let Some(ref data) = visitor.data {
                if data
                    .get("http.path")
                    .and_then(|v| v.as_str())
                    .map(|p| p == "/api/health" || p == "/health")
                    .unwrap_or(false)
                {
                    return;
                }
            }
        }

        // Parse crate_name from module_path: "hr_api::routes::backup" -> "hr-api"
        // Also check visitor data for log compat fields (log crate → tracing bridge)
        let raw_module = metadata.module_path().unwrap_or("");
        let effective_module = if raw_module.is_empty() || raw_module == "unknown" {
            // Try to get from log compat fields
            visitor
                .log_module_path
                .as_deref()
                .or(Some(metadata.target()))
                .unwrap_or("unknown")
        } else {
            raw_module
        };
        let module_path_str = effective_module.to_string();
        let crate_name = module_path_str
            .split("::")
            .next()
            .unwrap_or("unknown")
            .replace('_', "-");

        // Get function name from current span
        let function = ctx
            .current_span()
            .id()
            .and_then(|id| ctx.span(id))
            .map(|span| span.name().to_string());

        // Determine category based on structured fields
        let category = if visitor.has_http_fields {
            LogCategory::HttpRequest
        } else if visitor.has_ipc_fields {
            LogCategory::IpcCall
        } else if level == LogLevel::Error {
            LogCategory::Error
        } else {
            LogCategory::System
        };

        let data = visitor.data.map(serde_json::Value::Object);

        // Use log compat file/line if available and native ones are missing
        let file = metadata
            .file()
            .map(|s| s.to_string())
            .or_else(|| visitor.log_file.clone());
        let line = metadata.line().or(visitor.log_line);

        let builder = LogEntryBuilder {
            service: self.service,
            level,
            category,
            message,
            data,
            request_id: visitor.request_id,
            user_id: visitor.user_id,
            source: LogSource {
                crate_name,
                module: module_path_str,
                function,
                file,
                line,
            },
        };

        self.store.push(builder);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    data: Option<serde_json::Map<String, serde_json::Value>>,
    has_http_fields: bool,
    has_ipc_fields: bool,
    request_id: Option<String>,
    user_id: Option<String>,
    // Fields from log crate compat (tracing-log bridge)
    log_module_path: Option<String>,
    log_file: Option<String>,
    log_line: Option<u32>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        if name == "message" {
            self.message = Some(format!("{:?}", value));
            // Clean surrounding quotes if present
            if let Some(ref mut msg) = self.message {
                if msg.starts_with('"') && msg.ends_with('"') && msg.len() >= 2 {
                    *msg = msg[1..msg.len() - 1].to_string();
                }
            }
            return;
        }

        // Skip log compat metadata fields
        if name.starts_with("log.") {
            return;
        }

        if name.starts_with("http.") {
            self.has_http_fields = true;
        }
        if name.starts_with("ipc.") {
            self.has_ipc_fields = true;
        }
        if name == "request_id" {
            self.request_id = Some(format!("{:?}", value));
        }
        if name == "user_id" {
            self.user_id = Some(format!("{:?}", value));
        }

        let data = self.data.get_or_insert_with(serde_json::Map::new);
        data.insert(
            name.to_string(),
            serde_json::Value::String(format!("{:?}", value)),
        );
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == "message" {
            self.message = Some(value.to_string());
            return;
        }
        // Capture log compat fields but don't put them in data
        if name == "log.module_path" {
            self.log_module_path = Some(value.to_string());
            return;
        }
        if name == "log.file" {
            self.log_file = Some(value.to_string());
            return;
        }
        if name == "log.target" || name == "log.line" {
            return; // Skip these metadata fields from data
        }

        if name.starts_with("http.") {
            self.has_http_fields = true;
        }
        if name.starts_with("ipc.") {
            self.has_ipc_fields = true;
        }
        if name == "request_id" {
            self.request_id = Some(value.to_string());
        }
        if name == "user_id" {
            self.user_id = Some(value.to_string());
        }

        let data = self.data.get_or_insert_with(serde_json::Map::new);
        data.insert(
            name.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let name = field.name();
        if name == "log.line" {
            self.log_line = Some(value as u32);
            return;
        }
        if name.starts_with("log.") {
            return;
        }
        if name.starts_with("http.") {
            self.has_http_fields = true;
        }
        let data = self.data.get_or_insert_with(serde_json::Map::new);
        data.insert(
            name.to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let name = field.name();
        if name.starts_with("log.") {
            return;
        }
        if name.starts_with("http.") {
            self.has_http_fields = true;
        }
        let data = self.data.get_or_insert_with(serde_json::Map::new);
        data.insert(
            name.to_string(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let name = field.name();
        let data = self.data.get_or_insert_with(serde_json::Map::new);
        if let Some(n) = serde_json::Number::from_f64(value) {
            data.insert(name.to_string(), serde_json::Value::Number(n));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let name = field.name();
        let data = self.data.get_or_insert_with(serde_json::Map::new);
        data.insert(name.to_string(), serde_json::Value::Bool(value));
    }
}
