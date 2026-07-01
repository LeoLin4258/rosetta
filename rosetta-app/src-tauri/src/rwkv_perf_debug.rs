use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

const PERF_ENV: &str = "ROSETTA_RWKV_PERF_DEBUG";
const LOG_FILE: &str = "rwkv-performance.jsonl";
const PREV_LOG_FILE: &str = "rwkv-performance.prev.jsonl";

pub(crate) fn init() {
    if !enabled() {
        return;
    }
    let Some(log_dir) = crate::app_log::logs_dir() else {
        eprintln!("[rwkv-perf] logs dir unavailable");
        return;
    };
    if let Err(error) = fs::create_dir_all(&log_dir) {
        eprintln!("[rwkv-perf] cannot create logs dir: {error}");
        return;
    }

    let log_path = log_dir.join(LOG_FILE);
    let prev_path = log_dir.join(PREV_LOG_FILE);
    if log_path.exists() {
        let _ = fs::rename(&log_path, &prev_path);
    }
    let _ = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path);
    eprintln!(
        "[rwkv-perf] enabled; writing privacy-safe RWKV timing summaries to {}",
        log_path.display()
    );
}

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_enabled(PERF_ENV) || env_enabled("ROSETTA_RWKV_IO_DEBUG"))
}

fn env_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| enabled_value(&value))
}

#[derive(Debug)]
pub(crate) struct RwkvPerfRecord<'a> {
    pub provider: &'a str,
    pub context: Option<&'a str>,
    pub endpoint: Option<&'a str>,
    pub source_lang: Option<&'a str>,
    pub target_lang: Option<&'a str>,
    pub batch_size: usize,
    pub input_chars: u64,
    pub output_chars: u64,
    pub status_code: Option<u16>,
    pub ok: bool,
    pub error: Option<&'a str>,
    pub prepare_request_ms: u64,
    pub http_send_ms: u64,
    pub response_read_ms: u64,
    pub response_parse_ms: u64,
    pub latency_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializableRwkvPerfRecord<'a> {
    timestamp_ms: u128,
    provider: &'a str,
    context: Option<&'a str>,
    endpoint: Option<&'a str>,
    source_lang: Option<&'a str>,
    target_lang: Option<&'a str>,
    batch_size: usize,
    input_chars: u64,
    output_chars: u64,
    status_code: Option<u16>,
    ok: bool,
    error: Option<&'a str>,
    prepare_request_ms: u64,
    http_send_ms: u64,
    response_read_ms: u64,
    response_parse_ms: u64,
    latency_ms: u64,
}

pub(crate) fn log_record(record: RwkvPerfRecord<'_>) {
    if !enabled() {
        return;
    }
    let Some(log_dir) = crate::app_log::logs_dir() else {
        return;
    };
    if fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let path = log_dir.join(LOG_FILE);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let serializable = SerializableRwkvPerfRecord {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        provider: record.provider,
        context: record.context,
        endpoint: record.endpoint,
        source_lang: record.source_lang,
        target_lang: record.target_lang,
        batch_size: record.batch_size,
        input_chars: record.input_chars,
        output_chars: record.output_chars,
        status_code: record.status_code,
        ok: record.ok,
        error: record.error,
        prepare_request_ms: record.prepare_request_ms,
        http_send_ms: record.http_send_ms,
        response_read_ms: record.response_read_ms,
        response_parse_ms: record.response_parse_ms,
        latency_ms: record.latency_ms,
    };
    if let Ok(mut line) = serde_json::to_string(&serializable) {
        line.push('\n');
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
pub(crate) fn enabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "debug"
    )
}

#[cfg(not(test))]
fn enabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "debug"
    )
}

#[cfg(test)]
mod tests {
    use super::enabled_value;

    #[test]
    fn parses_perf_debug_env_values() {
        assert!(enabled_value("1"));
        assert!(enabled_value("true"));
        assert!(enabled_value("YES"));
        assert!(enabled_value("debug"));
        assert!(!enabled_value("0"));
        assert!(!enabled_value(""));
    }
}
