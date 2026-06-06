//! 轻量异常指标追踪系统
//!
//! JSONL 文件存储，mpsc channel 解耦，fire-and-forget 写入。

use chrono::Utc;
use serde::Serialize;
use std::sync::LazyLock;
use tokio::sync::mpsc;

/// 字符串截断上限（字符级，CJK 安全）
const TRUNCATE_LIMIT: usize = 500;

/// 指标事件
#[derive(Debug, Serialize)]
struct MetricEvent {
    /// ISO 8601 毫秒时间戳
    ts: String,
    /// session_id
    #[serde(skip_serializing_if = "Option::is_none")]
    sid: Option<String>,
    /// run_id（当前 ReAct 循环标识）
    #[serde(skip_serializing_if = "Option::is_none")]
    rid: Option<String>,
    /// 事件名（点分层级）
    event: String,
    /// 事件附加数据
    data: serde_json::Value,
}

/// 全局 channel sender
static METRICS_TX: LazyLock<mpsc::UnboundedSender<MetricEvent>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(metrics_writer(rx));
    tx
});

/// 发射一个指标事件。fire-and-forget，不阻塞调用方。
///
/// `data` 中所有字符串值会被截断到 500 字符。
pub fn emit(event: &str, data: serde_json::Value, sid: Option<&str>, rid: Option<&str>) {
    let data = truncate_json_strings(data);
    let evt = MetricEvent {
        ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        sid: sid.map(|s| s.to_owned()),
        rid: rid.map(|s| s.to_owned()),
        event: event.to_owned(),
        data,
    };
    if METRICS_TX.send(evt).is_err() {
        tracing::warn!(event, "metrics channel send failed (writer dropped)");
    }
}

/// 获取当前进程 RSS（MB），Unix only
pub fn current_rss_mb() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        if ret == 0 {
            #[cfg(target_os = "macos")]
            let rss_kb = (usage.ru_maxrss / 1024) as u64;
            #[cfg(not(target_os = "macos"))]
            let rss_kb = usage.ru_maxrss as u64;
            return Some(rss_kb / 1024);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// 单 writer task：消费 channel，追加写入 JSONL 文件
async fn metrics_writer(mut rx: mpsc::UnboundedReceiver<MetricEvent>) {
    let base_dir = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".peri")
        .join("metrics");

    if let Err(e) = tokio::fs::create_dir_all(&base_dir).await {
        tracing::warn!(path = %base_dir.display(), error = %e, "无法创建 metrics 目录");
        return;
    }

    let mut current_date = today();
    let path = base_dir.join(format!("{current_date}.jsonl"));
    let file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "无法打开 metrics 文件");
            return;
        }
    };
    use tokio::io::AsyncWriteExt;
    let mut writer = tokio::io::BufWriter::new(file);

    while let Some(evt) = rx.recv().await {
        let date = today();
        if date != current_date {
            let _ = writer.flush().await;
            let path = base_dir.join(format!("{date}.jsonl"));
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(f) => {
                    writer = tokio::io::BufWriter::new(f);
                    current_date = date;
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "无法切换 metrics 文件");
                    return;
                }
            }
        }

        match serde_json::to_string(&evt) {
            Ok(line) => {
                if let Err(e) = writer.write_all(line.as_bytes()).await {
                    tracing::warn!(error = %e, "metrics write failed");
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    tracing::warn!(error = %e, "metrics newline write failed");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "metrics serialize failed");
            }
        }
    }

    let _ = writer.flush().await;
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 递归截断 JSON 中所有字符串值到 TRUNCATE_LIMIT 字符
fn truncate_json_strings(val: serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            if s.chars().count() > TRUNCATE_LIMIT {
                serde_json::Value::String(s.chars().take(TRUNCATE_LIMIT).collect())
            } else {
                serde_json::Value::String(s)
            }
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, truncate_json_strings(v)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(truncate_json_strings).collect())
        }
        other => other,
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
