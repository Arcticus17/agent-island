//! OpenCode 的显式事件通道：直接问它自己的 HTTP 服务，而不是猜日志。
//!
//! 依据官方文档（https://opencode.ai/docs/server/）与本机 opencode 1.19.4 实测：
//!
//! ```text
//! GET /global/health   → {"healthy":true,"version":"1.19.4"}
//! GET /session         → [{ id, slug, projectID, directory, title, time:{created,updated}, … }]
//! GET /session/status  → { [sessionID]: SessionStatus }   // 空闲时是 {}
//! GET /event           → SSE：data: {"id":…,"type":"server.connected","properties":{}}
//! ```
//!
//! 这三条足以把身份变成"服务自己说的"：
//! - `directory` 是会话的工作目录（可与进程 cwd 对齐）；
//! - `session/status` 里出现某个 session，说明它正在忙 → 这就是当前该确认的会话。
//!
//! 客户端只用 `std::net::TcpStream` 手写 HTTP/1.1（只支持 GET + Connection: close + chunked），
//! 不引第三方依赖。

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenCodeHealth {
    pub healthy: bool,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenCodeSession {
    pub id: String,
    pub directory: Option<String>,
    pub title: Option<String>,
}

pub(crate) fn parse_health(body: &str) -> Option<OpenCodeHealth> {
    let value: Value = serde_json::from_str(body.trim()).ok()?;
    Some(OpenCodeHealth {
        healthy: value.get("healthy").and_then(Value::as_bool)?,
        version: value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    })
}

pub(crate) fn parse_sessions(body: &str) -> Vec<OpenCodeSession> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(body.trim()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.to_string();
            Some(OpenCodeSession {
                id,
                directory: item
                    .get("directory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                title: item.get("title").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect()
}

/// `GET /session/status` 返回 `{ [sessionID]: SessionStatus }`，空闲时是 `{}`。
/// 因此"出现在这张表里"就等价于"正在忙"——不需要猜 SessionStatus 的内部结构。
pub(crate) fn busy_session(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body.trim()).ok()?;
    let object = value.as_object()?;
    object
        .keys()
        .find(|key| key.starts_with("ses_"))
        .cloned()
}

/// 手写 HTTP GET（只用于 127.0.0.1 上的本机服务）。
pub(crate) fn http_get(host: &str, port: u16, path: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect((host, port))
        .map_err(|error| format!("连接 OpenCode 服务失败：{error}"))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("请求 OpenCode 失败：{error}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("读取 OpenCode 响应失败：{error}"))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "OpenCode 响应缺少头部".to_string())?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        let status = head.lines().next().unwrap_or("").to_string();
        return Err(format!("OpenCode 返回非 200：{status}"));
    }
    if head.to_lowercase().contains("transfer-encoding: chunked") {
        return Ok(dechunk(body));
    }
    Ok(body.to_string())
}

/// 解 chunked 编码（SSE 会用到；JSON 端点通常是 Content-Length）。
pub(crate) fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some((size_line, tail)) = rest.split_once("\r\n") else {
            break;
        };
        let Ok(size) = usize::from_str_radix(size_line.trim(), 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        if tail.len() < size {
            out.push_str(tail);
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].trim_start_matches("\r\n");
    }
    out
}

/// 环境变量里配置的 OpenCode 服务地址（例如 `http://127.0.0.1:4199`）。
pub(crate) fn configured_endpoint() -> Option<(String, u16)> {
    let raw = std::env::var("AGENT_ISLAND_OPENCODE_URL").ok()?;
    let raw = raw.trim();
    let without_scheme = raw.strip_prefix("http://").unwrap_or(raw);
    let (host, port) = without_scheme.split_once(':')?;
    let port: u16 = port.trim_end_matches('/').parse().ok()?;
    Some((host.to_string(), port))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_health_and_session_shapes() {
        let health = parse_health(r#"{"healthy":true,"version":"1.19.4"}"#).unwrap();
        assert!(health.healthy);
        assert_eq!(health.version, "1.19.4");
        assert!(parse_health("not json").is_none());

        // 形状取自真实响应；id/路径换成中性值，避免把本机会话信息带进仓库
        let sessions = parse_sessions(
            r#"[{"id":"ses_example_a","slug":"happy-rocket","directory":"C:\\work\\demo","title":"New session"},{"id":"ses_example_b","directory":"D:\\work\\other"}]"#,
        );
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "ses_example_a");
        assert_eq!(sessions[0].directory.as_deref(), Some("C:\\work\\demo"));
        assert_eq!(sessions[1].title, None);
    }

    #[test]
    fn an_empty_status_map_means_idle() {
        assert_eq!(busy_session("{}"), None);
        assert_eq!(busy_session(r#"{"ses_abc":{"type":"busy"}}"#).as_deref(), Some("ses_abc"));
        // 非 session 的键不算
        assert_eq!(busy_session(r#"{"version":"1.19.4"}"#), None);
    }

    #[test]
    fn dechunks_sse_payloads() {
        // 载荷 "data: {\"type\":\"server.connected\"}\r\n\r\n" 是 37 字节 = 0x25
        let body = "25\r\ndata: {\"type\":\"server.connected\"}\r\n\r\n0\r\n\r\n";
        assert_eq!(dechunk(body), "data: {\"type\":\"server.connected\"}\r\n\r\n");
    }

    #[test]
    fn parses_the_configured_endpoint() {
        std::env::set_var("AGENT_ISLAND_OPENCODE_URL", "http://127.0.0.1:4199");
        assert_eq!(
            configured_endpoint(),
            Some(("127.0.0.1".to_string(), 4199))
        );
        std::env::remove_var("AGENT_ISLAND_OPENCODE_URL");
        assert_eq!(configured_endpoint(), None);
    }

    /// 本机装了 opencode 时可以手动跑：`cargo test -- --ignored opencode_live_probe`
    #[test]
    #[ignore]
    fn opencode_live_probe() {
        let (host, port) = configured_endpoint().expect("AGENT_ISLAND_OPENCODE_URL");
        let health = http_get(&host, port, "/global/health").expect("health");
        let parsed = parse_health(&health).expect("parse health");
        println!("health={parsed:?}");
        let sessions = http_get(&host, port, "/session").expect("sessions");
        let parsed_sessions = parse_sessions(&sessions);
        println!("sessions={}", parsed_sessions.len());
        for session in parsed_sessions.iter().take(3) {
            println!("  {} {:?} {:?}", session.id, session.directory, session.title);
        }
        let status = http_get(&host, port, "/session/status").expect("status");
        println!("busy={:?}", busy_session(&status));
    }
}


