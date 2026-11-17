use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::core::settings::LogLevelSetting;

const RECENT_BUFFER_CAPACITY: usize = 300;

#[derive(Clone, Debug, Serialize)]
pub struct LogEvent {
    pub seq: u64,
    pub message: String,
    pub level: String,
    pub target: String,
    pub source: String,
    pub task_id: Option<String>,
    pub instance_id: Option<String>,
    pub timestamp: String,
}

pub static LOG_TX: OnceLock<mpsc::Sender<LogEvent>> = OnceLock::new();
static RECENT_LOG_BUFFER: LazyLock<Mutex<VecDeque<LogEvent>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(RECENT_BUFFER_CAPACITY)));
static LOG_SEQ: AtomicU64 = AtomicU64::new(1);
static BROADCAST_LEVEL: AtomicU8 = AtomicU8::new(2); // Default: INFO

fn level_to_rank(level: &tracing::Level) -> u8 {
    match *level {
        tracing::Level::ERROR => 0,
        tracing::Level::WARN => 1,
        tracing::Level::INFO => 2,
        tracing::Level::DEBUG => 3,
        tracing::Level::TRACE => 4,
    }
}

fn setting_to_rank(setting: LogLevelSetting) -> u8 {
    match setting {
        LogLevelSetting::Error => 0,
        LogLevelSetting::Warn => 1,
        LogLevelSetting::Info => 2,
        LogLevelSetting::Debug => 3,
        LogLevelSetting::Trace => 4,
    }
}

/// Updates the maximum log level forwarded to the frontend UI over IPC.
pub fn set_broadcast_level(level: LogLevelSetting) {
    BROADCAST_LEVEL.store(setting_to_rank(level), Ordering::Relaxed);
}

/// Returns buffered recent logs (including early startup logs before the UI mounted).
pub fn get_recent_logs() -> Vec<LogEvent> {
    match RECENT_LOG_BUFFER.lock() {
        Ok(guard) => guard.iter().cloned().collect(),
        Err(_) => Vec::new(),
    }
}

pub struct FrontendBroadcastLayer {
    custom_tx: Option<mpsc::Sender<LogEvent>>,
    custom_level: Option<u8>,
}

impl Default for FrontendBroadcastLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl FrontendBroadcastLayer {
    pub fn new() -> Self {
        Self {
            custom_tx: None,
            custom_level: None,
        }
    }

    #[cfg(test)]
    pub fn for_test(tx: mpsc::Sender<LogEvent>, level: LogLevelSetting) -> Self {
        Self {
            custom_tx: Some(tx),
            custom_level: Some(setting_to_rank(level)),
        }
    }

    fn effective_rank(&self) -> u8 {
        self.custom_level
            .unwrap_or_else(|| BROADCAST_LEVEL.load(Ordering::Relaxed))
    }
}

#[derive(Default, Clone)]
struct SpanContextData {
    task_id: Option<String>,
    instance_id: Option<String>,
}

fn strip_outer_quotes(mut val: String) -> String {
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        val = val[1..val.len() - 1].to_string();
    }
    val
}

#[derive(Default)]
struct SpanFieldVisitor {
    task_id: Option<String>,
    instance_id: Option<String>,
}

impl Visit for SpanFieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "task_id" {
            self.task_id = Some(value.to_string());
        } else if field.name() == "instance_id" {
            self.instance_id = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "task_id" {
            self.task_id = Some(strip_outer_quotes(format!("{:?}", value)));
        } else if field.name() == "instance_id" {
            self.instance_id = Some(strip_outer_quotes(format!("{:?}", value)));
        }
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    task_id: Option<String>,
    instance_id: Option<String>,
    frontend_origin: bool,
    extra_fields: Vec<String>,
}

impl Visit for EventVisitor {
    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "frontend_origin" {
            self.frontend_origin = value;
        } else if !field.name().starts_with("log.") {
            self.extra_fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if !field.name().starts_with("log.") {
            self.extra_fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if !field.name().starts_with("log.") {
            self.extra_fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if !field.name().starts_with("log.") {
            self.extra_fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "task_id" => self.task_id = Some(value.to_string()),
            "instance_id" => self.instance_id = Some(value.to_string()),
            name if !name.starts_with("log.") => {
                self.extra_fields.push(format!("{}={}", name, value));
            }
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => {
                self.message = Some(format!("{:?}", value));
            }
            "task_id" => {
                self.task_id = Some(strip_outer_quotes(format!("{:?}", value)));
            }
            "instance_id" => {
                self.instance_id = Some(strip_outer_quotes(format!("{:?}", value)));
            }
            "frontend_origin" => {}
            name if !name.starts_with("log.") => {
                self.extra_fields.push(format!("{}={:?}", name, value));
            }
            _ => {}
        }
    }
}

impl<S> Layer<S> for FrontendBroadcastLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = SpanFieldVisitor::default();
        attrs.record(&mut visitor);
        if visitor.task_id.is_some() || visitor.instance_id.is_some() {
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(SpanContextData {
                    task_id: visitor.task_id,
                    instance_id: visitor.instance_id,
                });
            }
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = SpanFieldVisitor::default();
        values.record(&mut visitor);
        if visitor.task_id.is_some() || visitor.instance_id.is_some() {
            if let Some(span) = ctx.span(id) {
                let mut exts = span.extensions_mut();
                if let Some(existing) = exts.get_mut::<SpanContextData>() {
                    if visitor.task_id.is_some() {
                        existing.task_id = visitor.task_id;
                    }
                    if visitor.instance_id.is_some() {
                        existing.instance_id = visitor.instance_id;
                    }
                } else {
                    exts.insert(SpanContextData {
                        task_id: visitor.task_id,
                        instance_id: visitor.instance_id,
                    });
                }
            }
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        if level_to_rank(event.metadata().level()) > self.effective_rank() {
            return;
        }

        let target = event.metadata().target();
        if target != "frontend" && !target.starts_with("dawnland_launcher_lib") {
            return;
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        // Frontend-originated logs written to disk are already appended on the JS side.
        // Skip rebroadcasting them to avoid echo loops and duplicate UI entries.
        if visitor.frontend_origin {
            return;
        }

        let mut task_id = visitor.task_id;
        let mut instance_id = visitor.instance_id;

        if task_id.is_none() || instance_id.is_none() {
            if let Some(scope) = ctx.event_scope(event) {
                for span in scope {
                    if let Some(data) = span.extensions().get::<SpanContextData>() {
                        if task_id.is_none() {
                            task_id = data.task_id.clone();
                        }
                        if instance_id.is_none() {
                            instance_id = data.instance_id.clone();
                        }
                        if task_id.is_some() && instance_id.is_some() {
                            break;
                        }
                    }
                }
            }
        }

        let mut raw_message = visitor.message.unwrap_or_default();
        if !visitor.extra_fields.is_empty() {
            let fields_str = visitor.extra_fields.join(", ");
            if raw_message.is_empty() {
                raw_message = fields_str;
            } else {
                raw_message = format!("{raw_message} ({fields_str})");
            }
        }

        let message = super::redact::redact_sensitive(&raw_message);
        let short_target = target
            .strip_prefix("dawnland_launcher_lib::")
            .unwrap_or(target)
            .to_string();

        let log_event = LogEvent {
            seq: LOG_SEQ.fetch_add(1, Ordering::Relaxed),
            message,
            level: event.metadata().level().to_string(),
            target: short_target,
            source: "rust".to_string(),
            task_id,
            instance_id,
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        if let Some(ref test_tx) = self.custom_tx {
            let _ = test_tx.try_send(log_event);
            return;
        }

        if let Ok(mut buf) = RECENT_LOG_BUFFER.lock() {
            if buf.len() >= RECENT_BUFFER_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(log_event.clone());
        }

        if let Some(tx) = LOG_TX.get() {
            let _ = tx.try_send(log_event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Instrument;
    use tracing_subscriber::layer::SubscriberExt;

    #[tokio::test]
    async fn test_frontend_layer_extracts_task_id_from_span_and_skips_frontend_origin() {
        let (tx, mut rx) = mpsc::channel(16);
        let layer = FrontendBroadcastLayer::for_test(tx, LogLevelSetting::Debug);
        let subscriber = tracing_subscriber::registry().with(layer);
        let dispatch = tracing::Dispatch::new(subscriber);

        let _guard = tracing::dispatcher::set_default(&dispatch);
        let span = tracing::info_span!("task", task_id = "task-abc-123", instance_id = "inst-1");

        async {
            tracing::info!("Step started");
            tracing::warn!(count = 3, "Retrying download");
            tracing::error!(frontend_origin = true, "Should not rebroadcast");
            tracing::debug!("Debug detail included");
        }
        .instrument(span)
        .await;

        let ev1 = rx.try_recv().expect("first event");
        assert_eq!(ev1.task_id.as_deref(), Some("task-abc-123"));
        assert_eq!(ev1.instance_id.as_deref(), Some("inst-1"));
        assert_eq!(ev1.message, "Step started");
        assert_eq!(ev1.source, "rust");

        let ev2 = rx.try_recv().expect("second event");
        assert_eq!(ev2.task_id.as_deref(), Some("task-abc-123"));
        assert_eq!(ev2.message, "Retrying download (count=3)");

        let ev3 = rx.try_recv().expect("debug event should follow frontend_origin skip");
        assert_eq!(ev3.level, "DEBUG");
        assert_eq!(ev3.message, "Debug detail included");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_frontend_layer_redacts_tokens_and_filters_levels() {
        let (tx, mut rx) = mpsc::channel(16);
        let layer = FrontendBroadcastLayer::for_test(tx, LogLevelSetting::Info);
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("Hidden debug log");
            tracing::info!("Token: Bearer secret_jwt_token_value");
        });

        let ev = rx.try_recv().expect("info event");
        assert!(ev.message.contains("Bearer ***"));
        assert!(!ev.message.contains("secret_jwt_token_value"));
        assert!(rx.try_recv().is_err());
    }
}
