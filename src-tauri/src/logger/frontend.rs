use serde::Serialize;
use tracing::Subscriber;
use tracing_subscriber::{layer::Context, Layer};
use tokio::sync::mpsc;
use std::sync::OnceLock;
use tracing::field::{Field, Visit};

#[derive(Clone, Serialize)]
pub struct LogEvent {
    pub message: String,
    pub level: String,
    pub task_id: Option<String>,
    pub timestamp: String,
}

pub static LOG_TX: OnceLock<mpsc::UnboundedSender<LogEvent>> = OnceLock::new();

pub struct FrontendBroadcastLayer;

struct EventVisitor {
    message: Option<String>,
    task_id: Option<String>,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        } else if field.name() == "task_id" {
            let mut val = format!("{:?}", value);
            if val.starts_with('"') && val.ends_with('"') {
                val = val[1..val.len()-1].to_string();
            }
            self.task_id = Some(val);
        }
    }
    
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else if field.name() == "task_id" {
            self.task_id = Some(value.to_string());
        }
    }
}

impl<S> Layer<S> for FrontendBroadcastLayer
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target();
        // Broadcast explicitly targeted logs OR any log from our own library
        if target != "frontend" && !target.starts_with("dawnland_launcher_lib") {
            return;
        }

        let mut visitor = EventVisitor {
            message: None,
            task_id: None,
        };
        event.record(&mut visitor);

        let message = visitor.message.unwrap_or_default();
        let task_id = visitor.task_id;
        
        let message = if message.starts_with('"') && message.ends_with('"') {
            message[1..message.len()-1].to_string()
        } else {
            message
        };

        if let Some(tx) = LOG_TX.get() {
            let _ = tx.send(LogEvent {
                message,
                level: event.metadata().level().to_string(),
                task_id,
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            });
        }
    }
}
