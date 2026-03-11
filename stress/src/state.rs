use log::{Level, Log, Metadata, Record};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SharedState {
    pub active_users: AtomicUsize,
    pub total_pings_sent: AtomicUsize,
    pub latency_tx: flume::Sender<u64>,

    pub log_error: AtomicUsize,
    pub log_warn: AtomicUsize,
    pub log_info: AtomicUsize,
    pub log_debug: AtomicUsize,

    pub log_tx: flume::Sender<String>,
}

impl SharedState {
    pub fn new() -> (Self, flume::Receiver<u64>, flume::Receiver<String>) {
        let (tx, rx) = flume::unbounded();
        let (log_tx, log_rx) = flume::unbounded();
        (
            Self {
                active_users: AtomicUsize::new(0),
                total_pings_sent: AtomicUsize::new(0),
                latency_tx: tx,
                log_error: AtomicUsize::new(0),
                log_warn: AtomicUsize::new(0),
                log_info: AtomicUsize::new(0),
                log_debug: AtomicUsize::new(0),
                log_tx,
            },
            rx,
            log_rx,
        )
    }

    pub fn add_latency(&self, latency_ms: u64) {
        let _ = self.latency_tx.send(latency_ms);
    }
}

pub struct TuiLogger {
    pub state: Arc<SharedState>,
}

impl Log for TuiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match record.level() {
                Level::Error => self.state.log_error.fetch_add(1, Ordering::Relaxed),
                Level::Warn => self.state.log_warn.fetch_add(1, Ordering::Relaxed),
                Level::Info => self.state.log_info.fetch_add(1, Ordering::Relaxed),
                Level::Debug | Level::Trace => self.state.log_debug.fetch_add(1, Ordering::Relaxed),
            };

            let log_line = format!("[{}] {}", record.level(), record.args());
            let _ = self.state.log_tx.send(log_line);
        }
    }

    fn flush(&self) {}
}
