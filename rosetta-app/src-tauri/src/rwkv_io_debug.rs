use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

const DEBUG_ENV: &str = "ROSETTA_RWKV_IO_DEBUG";
const LOG_FILE: &str = "rwkv-io-debug.jsonl";
const PREV_LOG_FILE: &str = "rwkv-io-debug.prev.jsonl";

pub(crate) fn init() {
    if !enabled() {
        return;
    }
    let Some(log_dir) = crate::app_log::logs_dir() else {
        eprintln!("[rwkv-io-debug] logs dir unavailable");
        return;
    };
    if let Err(error) = fs::create_dir_all(&log_dir) {
        eprintln!("[rwkv-io-debug] cannot create logs dir: {error}");
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
        "[rwkv-io-debug] enabled by {DEBUG_ENV}; writing full RWKV inputs/outputs to {}",
        log_path.display()
    );
}

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var(DEBUG_ENV).ok().is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
    })
}

pub(crate) struct RwkvIoDebugRecord<'a> {
    pub provider: &'a str,
    pub context: Option<&'a str>,
    pub endpoint: Option<&'a str>,
    pub source_lang: Option<&'a str>,
    pub target_lang: Option<&'a str>,
    pub status_code: Option<u16>,
    pub ok: bool,
    pub error: Option<&'a str>,
    pub inputs: Vec<&'a str>,
    pub outputs: Vec<&'a str>,
    pub raw_response: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializableRwkvIoDebugRecord<'a> {
    timestamp_ms: u128,
    provider: &'a str,
    context: Option<&'a str>,
    endpoint: Option<&'a str>,
    source_lang: Option<&'a str>,
    target_lang: Option<&'a str>,
    status_code: Option<u16>,
    ok: bool,
    error: Option<&'a str>,
    inputs: Vec<&'a str>,
    outputs: Vec<&'a str>,
    raw_response: Option<&'a str>,
}

pub(crate) fn log_record(record: RwkvIoDebugRecord<'_>) {
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

    let serializable = SerializableRwkvIoDebugRecord {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        provider: record.provider,
        context: record.context,
        endpoint: record.endpoint,
        source_lang: record.source_lang,
        target_lang: record.target_lang,
        status_code: record.status_code,
        ok: record.ok,
        error: record.error,
        inputs: record.inputs,
        outputs: record.outputs,
        raw_response: record.raw_response,
    };
    if let Ok(mut line) = serde_json::to_string(&serializable) {
        line.push('\n');
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::enabled_value;

    #[test]
    fn parses_debug_env_values() {
        assert!(enabled_value("1"));
        assert!(enabled_value("true"));
        assert!(enabled_value("YES"));
        assert!(enabled_value("debug"));
        assert!(!enabled_value("0"));
        assert!(!enabled_value(""));
    }
}

#[cfg(test)]
fn enabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "debug"
    )
}
