// Classic GeoD exposes public product notices only. No account or credential handling.
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, io::Read, path::PathBuf, time::Duration};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceError {
    code: &'static str,
    message: String,
}
fn error(code: &'static str, message: &str) -> ServiceError {
    ServiceError {
        code,
        message: message.into(),
    }
}
#[derive(Clone)]
pub struct ServiceState {
    config_path: PathBuf,
}
impl ServiceState {
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    gateway_origin: String,
}
fn validate_origin(raw: &str) -> Result<String, ServiceError> {
    let url = Url::parse(raw).map_err(|_| error("INVALID_SERVICE_URL", "公告服务地址无效"))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(error(
            "INVALID_SERVICE_URL",
            "公告服务仅支持 HTTPS 或本机回环 HTTP",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
fn gateway_origin(state: &ServiceState) -> Result<String, ServiceError> {
    let origin = match fs::read_to_string(&state.config_path) {
        Ok(value) => {
            serde_json::from_str::<Config>(&value)
                .map_err(|_| error("INVALID_SERVICE_URL", "公告服务配置无效"))?
                .gateway_origin
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::env::var("GEOD_DESKTOP_GATEWAY_ORIGIN")
                .ok()
                .or_else(|| option_env!("GEOD_DESKTOP_GATEWAY_ORIGIN").map(str::to_string))
                .unwrap_or_else(|| "https://geod.laogao.xyz".into())
        }
        Err(_) => return Err(error("INVALID_SERVICE_URL", "无法读取公告服务配置")),
    };
    validate_origin(&origin)
}
fn validate_public_feed(value: Value) -> Result<Value, ServiceError> {
    if value.get("accountId") != Some(&Value::Null)
        || value.get("product").and_then(Value::as_str) != Some("geod")
        || value.get("schemaVersion").and_then(Value::as_u64) != Some(1)
    {
        return Err(error("MESSAGES_RESPONSE", "公共公告响应无效"));
    }
    Ok(value)
}
fn fetch_public_notices(origin: &str, version: &str) -> Result<Value, ServiceError> {
    let local = Url::parse(origin)
        .ok()
        .is_some_and(|u| matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")));
    let builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15));
    let client = (if local { builder.no_proxy() } else { builder })
        .build()
        .map_err(|_| error("MESSAGES_UNAVAILABLE", "公告网络连接初始化失败"))?;
    let response = client
        .get(format!("{origin}/api/agent/messages/public"))
        .query(&[("product", "geod"), ("clientVersion", version)])
        .send()
        .map_err(|_| error("MESSAGES_UNAVAILABLE", "公告服务暂不可用，请稍后重试"))?;
    if !response.status().is_success() {
        return Err(error(
            "MESSAGES_UNAVAILABLE",
            "公告服务暂不可用，请稍后重试",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(8_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| error("MESSAGES_RESPONSE", "公共公告响应无效"))?;
    if bytes.len() > 8_000_000 {
        return Err(error("MESSAGES_RESPONSE", "公共公告响应过大"));
    }
    validate_public_feed(
        serde_json::from_slice(&bytes)
            .map_err(|_| error("MESSAGES_RESPONSE", "公共公告响应无效"))?,
    )
}
#[tauri::command]
pub async fn notifications_list(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<Value, ServiceError> {
    let state = state.inner().clone();
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        fetch_public_notices(&gateway_origin(&state)?, &version)
    })
    .await
    .map_err(|_| error("MESSAGES_UNAVAILABLE", "公共公告查询中断"))?
}
#[tauri::command]
pub fn notifications_open_link(app: AppHandle, url: String) -> Result<(), ServiceError> {
    let parsed = Url::parse(&url).map_err(|_| error("MESSAGE_LINK", "消息链接无效"))?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || url.len() > 2000
    {
        return Err(error("MESSAGE_LINK", "消息链接仅支持安全 HTTPS 地址"));
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|_| error("MESSAGE_LINK", "无法打开消息链接"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_service_origins_are_safe() {
        for origin in [
            "https://geod.laogao.xyz",
            "http://127.0.0.1:4411",
            "http://[::1]:4411",
        ] {
            assert!(validate_origin(origin).is_ok());
        }
        for origin in [
            "http://example.test",
            "https://u:p@example.test",
            "https://example.test/api",
            "https://example.test?token=x",
        ] {
            assert!(validate_origin(origin).is_err());
        }
    }
    #[test]
    fn private_account_responses_are_rejected() {
        assert!(validate_public_feed(
            serde_json::json!({"schemaVersion":1,"product":"geod","accountId":null})
        )
        .is_ok());
        assert!(validate_public_feed(
            serde_json::json!({"schemaVersion":1,"product":"geod","accountId":"account"})
        )
        .is_err());
        assert!(validate_public_feed(
            serde_json::json!({"schemaVersion":1,"product":"geod-agent","accountId":null})
        )
        .is_err());
    }
    #[test]
    fn public_http_never_sends_credentials() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut data = [0u8; 8192];
            let count = socket.read(&mut data).unwrap();
            let request = String::from_utf8_lossy(&data[..count]);
            assert!(request
                .starts_with("GET /api/agent/messages/public?product=geod&clientVersion=3.6.11 "));
            assert!(!request.to_lowercase().contains("authorization:"));
            assert!(!request.to_lowercase().contains("cookie:"));
            let body = r#"{"schemaVersion":1,"product":"geod","accountId":null,"items":[]}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        assert!(fetch_public_notices(&origin, "3.6.11").is_ok());
        server.join().unwrap();
    }
}
