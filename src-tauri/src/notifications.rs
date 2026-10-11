// Adapted from GeoD Agent 0.2.4, services.rs (MIT), source commit 413b2fc.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use keyring::Entry;
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}
fn error(code: &'static str, message: &str) -> ServiceError {
    ServiceError {
        code,
        message: message.into(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    pub identity_origin: String,
    pub gateway_origin: String,
}
#[derive(Debug, Default)]
struct Flow {
    pending: bool,
    error: Option<String>,
}
#[derive(Debug)]
struct FlowReservation {
    flow: Arc<Mutex<Flow>>,
    transferred: bool,
}
impl FlowReservation {
    fn acquire(flow: &Arc<Mutex<Flow>>) -> Result<Self, ServiceError> {
        let mut current = flow.lock().expect("flow mutex poisoned");
        if current.pending {
            return Err(error("AUTH_IN_PROGRESS", "GeoD 登录流程已在进行"));
        }
        current.pending = true;
        current.error = None;
        Ok(Self {
            flow: Arc::clone(flow),
            transferred: false,
        })
    }

    fn fail(&mut self, message: &str) {
        let mut current = self.flow.lock().expect("flow mutex poisoned");
        current.pending = false;
        current.error = Some(message.into());
        self.transferred = true;
    }

    fn transfer(mut self) {
        self.transferred = true;
    }
}
impl Drop for FlowReservation {
    fn drop(&mut self) {
        if !self.transferred {
            self.fail("登录准备失败，请重试");
        }
    }
}
#[derive(Clone)]
pub struct ServiceState {
    config_path: PathBuf,
    flow: Arc<Mutex<Flow>>,
    credential_lock: Arc<Mutex<()>>,
}
impl ServiceState {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            flow: Arc::new(Mutex::new(Flow::default())),
            credential_lock: Arc::new(Mutex::new(())),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    state: &'static str,
    user_id: Option<String>,
    error: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Tokens {
    identity_origin: String,
    access_token: String,
    refresh_token: String,
    user_id: String,
    access_expires_at: u64,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    user_id: String,
    expires_in: u64,
    scope: String,
    token_type: String,
}
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn credential_account(identity_origin: &str) -> String {
    if identity_origin == "https://geod.laogao.xyz" {
        "geod-oauth".into()
    } else {
        format!(
            "geod-oauth-{:x}",
            Sha256::digest(identity_origin.as_bytes())
        )
    }
}
// The vault entry is shared by installed and development profiles. Its rotating
// token needs one lock for the Windows user, rather than a separate profile lock.
fn lock_credentials(identity_origin: &str) -> Result<fs::File, ServiceError> {
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("GeoD")
        .join("auth-locks");
    fs::create_dir_all(&root).map_err(|_| error("CREDENTIAL_STORE", "无法取得本机授权锁"))?;
    let path = root.join(format!(
        "{:x}.lock",
        Sha256::digest(
            format!(
                "dev.geo-downloader.notifications:{}",
                credential_account(identity_origin)
            )
            .as_bytes()
        )
    ));
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| error("CREDENTIAL_STORE", "无法取得本机授权锁"))?;
    fs2::FileExt::lock_exclusive(&file)
        .map_err(|_| error("CREDENTIAL_STORE", "无法取得本机授权锁"))?;
    Ok(file)
}
fn entry(identity_origin: &str) -> Result<Entry, ServiceError> {
    Entry::new(
        "dev.geo-downloader.notifications",
        &credential_account(identity_origin),
    )
    .map_err(|_| error("CREDENTIAL_STORE", "无法访问系统凭据库"))
}
fn read_tokens(identity_origin: &str) -> Result<Option<Tokens>, ServiceError> {
    let value = match entry(identity_origin)?.get_password() {
        Ok(value) => value,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_) => return Err(error("CREDENTIAL_STORE", "读取系统凭据失败")),
    };
    serde_json::from_str(&value)
        .map(Some)
        .map_err(|_| error("CREDENTIAL_STORE", "系统凭据格式错误"))
}
fn write_tokens(tokens: &Tokens) -> Result<(), ServiceError> {
    entry(&tokens.identity_origin)?
        .set_password(
            &serde_json::to_string(tokens)
                .map_err(|_| error("CREDENTIAL_STORE", "凭据编码失败"))?,
        )
        .map_err(|_| error("CREDENTIAL_STORE", "保存系统凭据失败"))
}
fn delete_tokens(identity_origin: &str) -> Result<(), ServiceError> {
    match entry(identity_origin)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(error("CREDENTIAL_STORE", "移除系统凭据失败")),
    }
}
fn validate_origin(raw: &str) -> Result<String, ServiceError> {
    let url = Url::parse(raw).map_err(|_| error("INVALID_SERVICE_URL", "请输入有效的服务地址"))?;
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
            "服务地址必须是 HTTPS，或本机回环 HTTP",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}
fn validate_config(config: ServiceConfig) -> Result<ServiceConfig, ServiceError> {
    Ok(ServiceConfig {
        identity_origin: validate_origin(&config.identity_origin)?,
        gateway_origin: validate_origin(&config.gateway_origin)?,
    })
}
fn load_config(path: &PathBuf) -> Result<ServiceConfig, ServiceError> {
    #[allow(unused_mut)] // Development overrides below mutate it only in debug builds.
    let mut config: ServiceConfig = match fs::read_to_string(path) {
        Ok(value) => serde_json::from_str(&value)
            .map_err(|_| error("SERVICE_NOT_CONFIGURED", "服务配置损坏"))?,
        Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {
            let identity_origin = std::env::var("GEOD_DESKTOP_IDENTITY_ORIGIN")
                .ok()
                .or_else(|| option_env!("GEOD_DESKTOP_IDENTITY_ORIGIN").map(str::to_string))
                .unwrap_or_else(|| "https://geod.laogao.xyz".to_string());
            let gateway_origin = std::env::var("GEOD_DESKTOP_GATEWAY_ORIGIN")
                .ok()
                .or_else(|| option_env!("GEOD_DESKTOP_GATEWAY_ORIGIN").map(str::to_string))
                .unwrap_or_else(|| "https://geod.laogao.xyz".to_string());
            ServiceConfig {
                identity_origin,
                gateway_origin,
            }
        }
        Err(_) => return Err(error("SERVICE_NOT_CONFIGURED", "读取服务配置失败")),
    };
    // A development preview can use a local gateway while keeping the real login.
    #[cfg(debug_assertions)]
    if let Ok(origin) = std::env::var("GEOD_DESKTOP_DEV_GATEWAY_ORIGIN") {
        let url = reqwest::Url::parse(&origin)
            .map_err(|_| error("SERVICE_NOT_CONFIGURED", "本地网关地址无效"))?;
        if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
            return Err(error("SERVICE_NOT_CONFIGURED", "开发网关必须运行在本机"));
        }
        config.gateway_origin = origin;
    }
    validate_config(config)
}
fn client(origin: &str) -> Result<Client, ServiceError> {
    let local = Url::parse(origin)
        .ok()
        .is_some_and(|u| matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")));
    let builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15));
    let builder = if local { builder.no_proxy() } else { builder };
    builder
        .build()
        .map_err(|_| error("NETWORK_ERROR", "网络客户端初始化失败"))
}
fn ensure_identity_available(origin: &str) -> Result<(), ServiceError> {
    let response = client(origin)?
        .get(format!("{origin}/api/geod/oauth/authorize"))
        .timeout(Duration::from_secs(8))
        .send()
        .map_err(|_| error("IDENTITY_UNAVAILABLE", "GeoD 账号授权服务暂时不可达"))?;
    let status = response.status();
    if status.is_server_error() {
        return Err(error("IDENTITY_UNAVAILABLE", "GeoD 账号授权服务暂时不可达"));
    }
    let value: Value = response
        .json()
        .map_err(|_| error("IDENTITY_UNAVAILABLE", "GeoD 账号授权接口尚未上线"))?;
    if status != reqwest::StatusCode::BAD_REQUEST
        || value.pointer("/error/code").and_then(Value::as_str) != Some("INVALID_OAUTH_REQUEST")
    {
        return Err(error("IDENTITY_UNAVAILABLE", "GeoD 账号授权接口尚未上线"));
    }
    Ok(())
}
fn token_response(value: TokenResponse, identity_origin: String) -> Result<Tokens, ServiceError> {
    let valid_token = |raw: &str| {
        raw.len() == 43
            && raw
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || v == b'_' || v == b'-')
    };
    if !valid_token(&value.access_token)
        || !valid_token(&value.refresh_token)
        || value.user_id.is_empty()
        || value.user_id.len() > 160
        || value.user_id.chars().any(char::is_control)
        || value.expires_in == 0
        || value.expires_in > 86400
        || value.scope != "geod:notifications"
        || value.token_type != "Bearer"
    {
        return Err(error("INVALID_TOKEN_RESPONSE", "身份服务返回了无效授权"));
    }
    Ok(Tokens {
        identity_origin,
        access_token: value.access_token,
        refresh_token: value.refresh_token,
        user_id: value.user_id,
        access_expires_at: unix_seconds() + value.expires_in,
    })
}
fn get_access_token(state: &ServiceState, config: &ServiceConfig) -> Result<String, ServiceError> {
    Ok(authenticated_tokens(state, config)?.access_token)
}
fn authenticated_tokens(
    state: &ServiceState,
    config: &ServiceConfig,
) -> Result<Tokens, ServiceError> {
    let _guard = state
        .credential_lock
        .lock()
        .expect("credential mutex poisoned");
    let _shared = lock_credentials(&config.identity_origin)?;
    // The desktop and its detached background share the same rotating token.
    // Serialize refresh across processes, then reread the current vault value.
    if let Some(parent) = state.config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| error("CREDENTIAL_STORE", "无法创建本机授权目录"))?;
    }
    let refresh_lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(state.config_path.with_extension("credentials.lock"))
        .map_err(|_| error("CREDENTIAL_STORE", "无法取得本机授权锁"))?;
    fs2::FileExt::lock_exclusive(&refresh_lock)
        .map_err(|_| error("CREDENTIAL_STORE", "无法取得本机授权锁"))?;
    let mut tokens = read_tokens(&config.identity_origin)?
        .ok_or_else(|| error("AUTH_REQUIRED", "请先登录 GeoD"))?;
    if tokens.identity_origin != config.identity_origin {
        return Err(error("AUTH_REQUIRED", "请重新登录 GeoD"));
    }
    if tokens.access_expires_at > unix_seconds() + 30 {
        return Ok(tokens);
    }
    let response = client(&config.identity_origin)?
        .post(format!("{}/api/geod/oauth/token", config.identity_origin))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "geod-desktop"),
            ("refresh_token", tokens.refresh_token.as_str()),
        ])
        .send()
        .map_err(|_| error("IDENTITY_UNAVAILABLE", "GeoD 身份服务暂时不可达"))?;
    if !response.status().is_success() {
        if matches!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST
                | reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
        ) {
            // Do not erase credentials replaced by an older installed process
            // while our refresh request was in flight.
            if let Some(latest) = read_tokens(&config.identity_origin)? {
                if latest.refresh_token != tokens.refresh_token
                    || latest.access_token != tokens.access_token
                {
                    if latest.identity_origin == config.identity_origin
                        && latest.access_expires_at > unix_seconds() + 30
                    {
                        return Ok(latest);
                    }
                    return Err(error("IDENTITY_UNAVAILABLE", "本机授权已更新，请稍后重试"));
                }
            }
            delete_tokens(&config.identity_origin)?;
            return Err(error("AUTH_EXPIRED", "GeoD 授权已失效，请重新登录"));
        }
        return Err(error("IDENTITY_UNAVAILABLE", "GeoD 身份服务暂时不可达"));
    }
    let refreshed: TokenResponse = response
        .json()
        .map_err(|_| error("INVALID_TOKEN_RESPONSE", "身份服务返回了无效授权"))?;
    tokens = token_response(refreshed, config.identity_origin.clone())?;
    write_tokens(&tokens)?;
    Ok(tokens)
}

fn auth_status_impl(state: &ServiceState) -> Result<AuthStatus, ServiceError> {
    let flow = state.flow.lock().expect("flow mutex poisoned");
    if flow.pending {
        return Ok(AuthStatus {
            state: "waiting",
            user_id: None,
            error: None,
        });
    }
    let last_error = flow.error.clone();
    drop(flow);
    let config = match load_config(&state.config_path) {
        Ok(config) => config,
        Err(_) => {
            return Ok(AuthStatus {
                state: "unconfigured",
                user_id: None,
                error: last_error,
            })
        }
    };
    let tokens = {
        let _guard = state
            .credential_lock
            .lock()
            .expect("credential mutex poisoned");
        let _shared = lock_credentials(&config.identity_origin)?;
        read_tokens(&config.identity_origin)?
    };
    if let Some(tokens) = tokens {
        if tokens.identity_origin == config.identity_origin {
            let refresh_error = if tokens.access_expires_at <= unix_seconds() + 30 {
                match get_access_token(&state, &config) {
                    Ok(_) => None,
                    Err(cause) if cause.code == "AUTH_EXPIRED" || cause.code == "AUTH_REQUIRED" => {
                        return Ok(AuthStatus {
                            state: "disconnected",
                            user_id: None,
                            error: Some(cause.message),
                        });
                    }
                    Err(cause) => Some(cause.message),
                }
            } else {
                None
            };
            return Ok(AuthStatus {
                state: "connected",
                user_id: Some(tokens.user_id),
                error: refresh_error,
            });
        }
    }
    Ok(AuthStatus {
        state: "disconnected",
        user_id: None,
        error: last_error,
    })
}

fn send_page(stream: &mut std::net::TcpStream, success: bool) {
    let message = if success {
        "GeoD 授权成功，可以返回桌面应用。"
    } else {
        "GeoD 授权失败，请返回桌面应用重试。"
    };
    let body = format!("<!doctype html><meta charset=\"utf-8\"><title>GeoD</title><body style=\"font:16px system-ui;padding:48px\"><h1>{message}</h1></body>");
    let status = if success { "200 OK" } else { "400 Bad Request" };
    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
}
fn read_callback_headers(reader: &mut impl Read) -> Option<String> {
    let mut buffer = [0_u8; 8192];
    let mut length = 0;
    while length < buffer.len() {
        let read = reader.read(&mut buffer[length..]).ok()?;
        if read == 0 {
            return None;
        }
        length += read;
        if let Some(end) = buffer[..length]
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
        {
            return Some(String::from_utf8_lossy(&buffer[..end + 4]).into_owned());
        }
    }
    None
}
fn complete_flow(
    listener: TcpListener,
    expected_state: String,
    verifier: String,
    redirect_uri: String,
    config: ServiceConfig,
    flow: Arc<Mutex<Flow>>,
    credential_lock: Arc<Mutex<()>>,
) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut completed = false;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let request = match read_callback_headers(&mut stream) {
                    Some(request) => request,
                    None => {
                        send_page(&mut stream, false);
                        continue;
                    }
                };
                let first = request.lines().next().unwrap_or("");
                let host = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find_map(|(name, value)| {
                        name.eq_ignore_ascii_case("host").then_some(value.trim())
                    })
                    .unwrap_or("");
                let expected_host = redirect_uri
                    .strip_prefix("http://")
                    .and_then(|part| part.split('/').next())
                    .unwrap_or("");
                let target = first
                    .strip_prefix("GET ")
                    .and_then(|rest| rest.split_once(' '))
                    .map(|(target, _)| target);
                let parsed = target
                    .and_then(|target| Url::parse(&format!("http://{expected_host}{target}")).ok());
                let valid = parsed.as_ref().is_some_and(|url| {
                    url.path() == "/oauth/callback"
                        && url
                            .query_pairs()
                            .any(|(key, value)| key == "state" && value == expected_state)
                }) && host == expected_host
                    && request.contains("HTTP/1.1");
                if !valid {
                    send_page(&mut stream, false);
                    continue;
                }
                let code = parsed
                    .unwrap()
                    .query_pairs()
                    .find(|(key, _)| key == "code")
                    .map(|(_, value)| value.into_owned())
                    .unwrap_or_default();
                let outcome = (|| -> Result<(), ServiceError> {
                    if code.len() != 43
                        || !code
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                    {
                        return Err(error("INVALID_CODE", "授权码无效"));
                    }
                    let response = client(&config.identity_origin)?
                        .post(format!("{}/api/geod/oauth/token", config.identity_origin))
                        .form(&[
                            ("grant_type", "authorization_code"),
                            ("client_id", "geod-desktop"),
                            ("redirect_uri", redirect_uri.as_str()),
                            ("code", code.as_str()),
                            ("code_verifier", verifier.as_str()),
                        ])
                        .send()
                        .map_err(|_| error("IDENTITY_UNAVAILABLE", "GeoD 身份服务暂时不可达"))?;
                    if !response.status().is_success() {
                        return Err(error("INVALID_GRANT", "GeoD 授权码验证失败"));
                    }
                    let value: TokenResponse = response
                        .json()
                        .map_err(|_| error("INVALID_TOKEN_RESPONSE", "身份服务响应无效"))?;
                    let tokens = token_response(value, config.identity_origin.clone())?;
                    let _guard = credential_lock.lock().expect("credential mutex poisoned");
                    let _shared = lock_credentials(&config.identity_origin)?;
                    write_tokens(&tokens)
                })();
                send_page(&mut stream, outcome.is_ok());
                let mut current = flow.lock().expect("flow mutex poisoned");
                current.pending = false;
                current.error = outcome.err().map(|error| error.message);
                completed = true;
                break;
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(_) => break,
        }
    }
    if !completed {
        let mut current = flow.lock().expect("flow mutex poisoned");
        current.pending = false;
        current.error = Some("登录等待已超时，请重试".into());
    }
}

fn auth_begin_impl(app: AppHandle, state: &ServiceState) -> Result<AuthStatus, ServiceError> {
    let config = load_config(&state.config_path)?;
    {
        let _guard = state
            .credential_lock
            .lock()
            .expect("credential mutex poisoned");
        if read_tokens(&config.identity_origin)?.is_some() {
            return Err(error("AUTH_ACTIVE", "请先退出当前 GeoD 账号"));
        }
    }
    let mut reservation = FlowReservation::acquire(&state.flow)?;
    if let Err(cause) = ensure_identity_available(&config.identity_origin) {
        reservation.fail(&cause.message);
        return Err(cause);
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| error("CALLBACK_UNAVAILABLE", "无法建立本机授权回调"))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| error("CALLBACK_UNAVAILABLE", "无法建立本机授权回调"))?;
    let port = listener
        .local_addr()
        .map_err(|_| error("CALLBACK_UNAVAILABLE", "无法取得本机回调地址"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
    let verifier = format!(
        "{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let nonce = Uuid::new_v4().simple().to_string();
    let mut url = Url::parse(&format!(
        "{}/api/geod/oauth/authorize",
        config.identity_origin
    ))
    .map_err(|_| error("INVALID_SERVICE_URL", "身份服务地址无效"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", "geod-desktop")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "geod:notifications")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &nonce);
    if app.opener().open_url(url.as_str(), None::<&str>).is_err() {
        reservation.fail("无法打开系统浏览器");
        return Err(error("BROWSER_UNAVAILABLE", "无法打开系统浏览器"));
    }
    let flow = Arc::clone(&state.flow);
    let credential_lock = Arc::clone(&state.credential_lock);
    thread::Builder::new()
        .name("geod-oauth-callback".into())
        .spawn(move || {
            complete_flow(
                listener,
                nonce,
                verifier,
                redirect_uri,
                config,
                flow,
                credential_lock,
            )
        })
        .map_err(|_| {
            reservation.fail("无法等待本机授权回调");
            error("CALLBACK_UNAVAILABLE", "无法等待本机授权回调")
        })?;
    reservation.transfer();
    Ok(AuthStatus {
        state: "waiting",
        user_id: None,
        error: None,
    })
}
fn auth_logout_impl(state: &ServiceState) -> Result<AuthStatus, ServiceError> {
    if state.flow.lock().expect("flow mutex poisoned").pending {
        return Err(error("AUTH_IN_PROGRESS", "请先完成当前登录流程"));
    }
    let config = load_config(&state.config_path)?;
    let _guard = state
        .credential_lock
        .lock()
        .expect("credential mutex poisoned");
    let _shared = lock_credentials(&config.identity_origin)?;
    if let Some(tokens) = read_tokens(&config.identity_origin)? {
        if tokens.identity_origin == config.identity_origin {
            let _ = client(&config.identity_origin).and_then(|client| {
                client
                    .post(format!("{}/api/geod/oauth/revoke", config.identity_origin))
                    .form(&[
                        ("client_id", "geod-desktop"),
                        ("token", tokens.refresh_token.as_str()),
                    ])
                    .send()
                    .map(|_| ())
                    .map_err(|_| error("IDENTITY_UNAVAILABLE", "GeoD 身份服务暂时不可达"))
            });
        }
    }
    delete_tokens(&config.identity_origin)?;
    Ok(AuthStatus {
        state: "disconnected",
        user_id: None,
        error: None,
    })
}

#[tauri::command]
pub async fn auth_status(state: State<'_, ServiceState>) -> Result<AuthStatus, ServiceError> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || auth_status_impl(&state))
        .await
        .map_err(|_| error("AUTH_INTERRUPTED", "账号状态查询中断"))?
}
#[tauri::command]
pub async fn auth_begin(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<AuthStatus, ServiceError> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || auth_begin_impl(app, &state))
        .await
        .map_err(|_| error("AUTH_INTERRUPTED", "账号授权中断"))?
}
#[tauri::command]
pub async fn auth_logout(state: State<'_, ServiceState>) -> Result<AuthStatus, ServiceError> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || auth_logout_impl(&state))
        .await
        .map_err(|_| error("AUTH_INTERRUPTED", "账号退出中断"))?
}

fn messages_call(
    state: &ServiceState,
    body: Option<Value>,
    version: String,
) -> Result<Value, ServiceError> {
    let config = load_config(&state.config_path)?;
    let token = get_access_token(state, &config)?;
    let client = client(&config.gateway_origin)?;
    let request = match body {
        Some(body) => client
            .post(format!("{}/api/agent/messages/read", config.gateway_origin))
            .json(&body),
        None => client
            .get(format!("{}/api/agent/messages", config.gateway_origin))
            .query(&[("clientVersion", version.as_str()), ("product", "geod")]),
    };
    let response = request
        .bearer_auth(token)
        .send()
        .map_err(|_| error("MESSAGES_UNAVAILABLE", "消息服务暂不可用，请稍后重试。"))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(error("AUTH_EXPIRED", "GeoD 授权已失效，请重新登录"));
    }
    if !status.is_success() {
        return Err(error(
            "MESSAGES_UNAVAILABLE",
            "消息服务暂不可用，请稍后重试。",
        ));
    }
    response
        .json()
        .map_err(|_| error("MESSAGES_RESPONSE", "消息服务响应无效"))
}
#[tauri::command]
pub async fn notifications_list(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<Value, ServiceError> {
    let state = state.inner().clone();
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || messages_call(&state, None, version))
        .await
        .map_err(|_| error("MESSAGES_UNAVAILABLE", "消息查询中断"))?
}
#[tauri::command]
pub async fn notifications_read(
    app: AppHandle,
    state: State<'_, ServiceState>,
    account_id: String,
    message_ids: Vec<String>,
) -> Result<Value, ServiceError> {
    if message_ids.is_empty()
        || message_ids.len() > 100
        || message_ids.iter().any(|id| {
            id.is_empty()
                || id.len() > 100
                || !id
                    .bytes()
                    .all(|v| v.is_ascii_alphanumeric() || v == b'_' || v == b'-')
        })
    {
        return Err(error("MESSAGES_INVALID", "消息标识无效"));
    }
    let state = state.inner().clone();
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move||{
        let config=load_config(&state.config_path)?;
        if authenticated_tokens(&state,&config)?.user_id!=account_id{return Err(error("ACCOUNT_CHANGED","账号已切换，请刷新消息"));}
        messages_call(&state,Some(json!({"accountId":account_id,"clientVersion":version,"product":"geod","messageIds":message_ids})),version)
    }).await.map_err(|_|error("MESSAGES_UNAVAILABLE","消息同步中断"))?
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
    fn service_origins_reject_credentials_paths_and_remote_plaintext() {
        assert_eq!(
            validate_origin("https://geod.laogao.xyz/").unwrap(),
            "https://geod.laogao.xyz"
        );
        for origin in [
            "http://127.0.0.1:4411",
            "http://localhost:4411",
            "http://[::1]:4411",
        ] {
            assert!(validate_origin(origin).is_ok(), "{origin}");
        }
        for origin in [
            "http://example.test",
            "https://u:p@example.test",
            "https://example.test/api",
            "https://example.test?token=secret",
            "https://example.test/#fragment",
        ] {
            assert!(validate_origin(origin).is_err(), "{origin}");
        }
    }
    #[test]
    fn authorization_response_requires_notification_scope_and_valid_tokens() {
        let response = || TokenResponse {
            access_token: "A".repeat(43),
            refresh_token: "B".repeat(43),
            user_id: "account-a".into(),
            expires_in: 900,
            scope: "geod:notifications".into(),
            token_type: "Bearer".into(),
        };
        assert!(token_response(response(), "https://geod.laogao.xyz".into()).is_ok());
        let mut agent = response();
        agent.scope = "geod:agent".into();
        assert!(token_response(agent, "https://geod.laogao.xyz".into()).is_err());
        let mut malformed = response();
        malformed.access_token = "!".repeat(43);
        assert!(token_response(malformed, "https://geod.laogao.xyz".into()).is_err());
    }
    #[test]
    fn callback_reader_is_bounded_and_login_reservations_recover() {
        assert!(read_callback_headers(&mut std::io::Cursor::new(vec![b'x'; 9000])).is_none());
        assert!(read_callback_headers(&mut std::io::Cursor::new(
            b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"
        ))
        .is_some());
        let flow = Arc::new(Mutex::new(Flow::default()));
        {
            let _reserved = FlowReservation::acquire(&flow).unwrap();
            assert!(FlowReservation::acquire(&flow).is_err());
        }
        assert!(!flow.lock().unwrap().pending);
        assert!(FlowReservation::acquire(&flow).is_ok());
        assert_ne!(
            credential_account("http://127.0.0.1:4411"),
            credential_account("https://geod.laogao.xyz")
        );
    }
}
