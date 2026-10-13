//! Ordered, task-scoped credential failover. Tokens never appear in task logs.
use crate::settings::{AppSettings, SettingsManager};
use std::sync::{Arc, Mutex, OnceLock, Weak};

type Logger = Arc<dyn Fn(&str) + Send + Sync>;
static LIVE_POOLS: OnceLock<Mutex<Vec<Weak<TiandituTokens>>>> = OnceLock::new();

pub fn normalize_tokens(primary: Option<&str>, backups: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in primary
        .into_iter()
        .chain(backups.iter().map(String::as_str))
    {
        let token = token.trim();
        if !token.is_empty() && !tokens.iter().any(|value| value == token) {
            tokens.push(token.to_owned());
        }
    }
    tokens
}

fn configured_tokens(settings: &AppSettings, fallback: Option<&str>) -> Vec<String> {
    let tokens = normalize_tokens(
        settings.tianditu_token.as_deref(),
        &settings.tianditu_backup_tokens,
    );
    if tokens.is_empty() {
        // Explicit request credentials remain supported for CLI callers. Desktop
        // resume supplies the freshly saved primary, including an explicit clear.
        let fallback = fallback.map(str::trim).filter(|token| !token.is_empty());
        normalize_tokens(
            Some(fallback.unwrap_or(crate::config::TIANDITU_DEFAULT_TOKEN)),
            &[],
        )
    } else {
        tokens
    }
}

pub fn is_tianditu_url(template: &str) -> bool {
    reqwest::Url::parse(&template.replace("{s}", "0"))
        .ok()
        .and_then(|url| {
            url.host_str().map(|host| {
                let host = host.trim_end_matches('.').to_ascii_lowercase();
                host == "tianditu.gov.cn" || host.ends_with(".tianditu.gov.cn")
            })
        })
        .unwrap_or(false)
}

// Keep fixed URL parameters and replace only tk. Never send credentials to a
// host merely containing the provider's name in its path or query string.
pub fn token_url(url: &str, token: &str) -> String {
    let mut parsed = reqwest::Url::parse(url).expect("validated tile URL");
    let pairs: Vec<_> = parsed
        .query_pairs()
        .filter(|(key, _)| key != "tk")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    parsed
        .query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("tk", token);
    parsed.to_string()
}

struct State {
    tokens: Vec<String>,
    active: usize,
    generation: u64,
}

// Do not derive Debug: this value contains a credential.
#[derive(Clone)]
pub struct Selection {
    pub token: String,
    index: usize,
    generation: u64,
}

pub struct TiandituTokens {
    state: Mutex<State>,
    logger: Logger,
}

impl TiandituTokens {
    pub fn new(tokens: Vec<String>, logger: Logger) -> Self {
        Self {
            state: Mutex::new(State {
                tokens,
                active: 0,
                generation: 0,
            }),
            logger,
        }
    }

    /// One pool for all zooms and annotation layers of a task; setting saves
    /// update live pools. Completed files are unaffected by credential changes.
    pub fn for_task(fallback: Option<&str>, logger: Logger) -> Arc<Self> {
        let mut registry = LIVE_POOLS.get_or_init(Default::default).lock().unwrap();
        let settings = SettingsManager::new()
            .and_then(|manager| manager.get())
            .unwrap_or_default();
        let pool = Arc::new(Self::new(configured_tokens(&settings, fallback), logger));
        registry.retain(|pool| pool.strong_count() > 0);
        registry.push(Arc::downgrade(&pool));
        pool
    }

    pub fn current(&self) -> Result<Selection, String> {
        let state = self.state.lock().unwrap();
        state
            .tokens
            .get(state.active)
            .map(|token| Selection {
                token: token.clone(),
                index: state.active,
                generation: state.generation,
            })
            .ok_or_else(|| Self::exhausted(state.tokens.len()))
    }

    fn exhausted(count: usize) -> String {
        format!("TILE_TOKENS_EXHAUSTED: 天地图的 {} 个 Token 均不可用或额度不足；任务数据已保留。请修改并保存 Token 后继续任务，或稍后重试。", count)
    }

    pub fn reject(&self, used: &Selection, reason: &str) -> Result<(), String> {
        let message = {
            let mut state = self.state.lock().unwrap();
            // Late errors from concurrent requests must not exhaust the next
            // token, or reject a newly saved credential configuration.
            if used.generation != state.generation || used.index != state.active {
                return Ok(());
            }
            state.active += 1;
            if state.active >= state.tokens.len() {
                let error = Self::exhausted(state.tokens.len());
                drop(state);
                (self.logger)(&error);
                return Err(error);
            }
            format!(
                "天地图 {}，自动切换至第 {}/{} 个 Token，继续下载",
                reason,
                state.active + 1,
                state.tokens.len()
            )
        };
        (self.logger)(&message);
        Ok(())
    }

    fn replace(&self, tokens: Vec<String>) {
        let count = tokens.len();
        {
            let mut state = self.state.lock().unwrap();
            // Saving unrelated settings must not reactivate exhausted tokens.
            if state.tokens == tokens {
                return;
            }
            state.tokens = tokens;
            state.active = 0;
            state.generation = state.generation.wrapping_add(1);
        }
        (self.logger)(&format!(
            "天地图 Token 设置已更新，后续请求使用最新配置（{} 个 Token）",
            count
        ));
    }
}

/// Call only after settings have been successfully persisted.
pub fn settings_saved(settings: &AppSettings) {
    let pools = {
        let mut registry = LIVE_POOLS.get_or_init(Default::default).lock().unwrap();
        registry.retain(|pool| pool.strong_count() > 0);
        registry
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>()
    };
    let tokens = configured_tokens(settings, None);
    for pool in pools {
        pool.replace(tokens.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_unique_tokens_and_legacy_settings() {
        let settings: AppSettings = serde_json::from_str(r#"{"tianditu_token":" old "}"#).unwrap();
        assert_eq!(configured_tokens(&settings, Some("stale")), vec!["old"]);
        assert_eq!(
            normalize_tokens(
                Some(" first "),
                &["second".into(), "first".into(), " ".into()]
            ),
            vec!["first", "second"]
        );
        assert_eq!(configured_tokens(&AppSettings::default(), None).len(), 1);
        assert_eq!(
            configured_tokens(&AppSettings::default(), Some(" ")),
            vec![crate::config::TIANDITU_DEFAULT_TOKEN]
        );
    }

    #[test]
    fn successfully_saved_settings_update_registered_live_tasks_and_clear_old_tokens() {
        let pool = Arc::new(TiandituTokens::new(
            vec!["stale-request".into()],
            Arc::new(|_| {}),
        ));
        LIVE_POOLS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .push(Arc::downgrade(&pool));
        let stale = pool.current().unwrap();
        let mut settings = AppSettings::default();
        settings.tianditu_token = Some("fresh-primary".into());
        settings.tianditu_backup_tokens = vec!["fresh-backup".into()];
        settings_saved(&settings);
        pool.reject(&stale, "HTTP 429").unwrap();
        assert_eq!(pool.current().unwrap().token, "fresh-primary");
        let fresh = pool.current().unwrap();
        pool.reject(&fresh, "HTTP 429").unwrap();
        settings_saved(&settings); // unrelated setting save retains the active backup
        assert_eq!(pool.current().unwrap().token, "fresh-backup");
        settings_saved(&AppSettings::default());
        assert_eq!(
            pool.current().unwrap().token,
            crate::config::TIANDITU_DEFAULT_TOKEN
        );
    }

    #[test]
    fn concurrent_failures_only_advance_once_and_saved_tokens_supersede_in_flight_errors() {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let output = logs.clone();
        let pool = Arc::new(TiandituTokens::new(
            vec!["secret-a".into(), "secret-b".into(), "secret-c".into()],
            Arc::new(move |line| output.lock().unwrap().push(line.to_owned())),
        ));
        let used = pool.current().unwrap();
        let handles: Vec<_> = (0..30)
            .map(|_| {
                let used = used.clone();
                let pool = pool.clone();
                std::thread::spawn(move || pool.reject(&used, "HTTP 429").unwrap())
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(pool.current().unwrap().token, "secret-b");
        assert_eq!(logs.lock().unwrap().len(), 1);
        pool.replace(vec!["new-a".into(), "new-b".into()]);
        let used = pool.current().unwrap();
        pool.reject(&used, "HTTP 401").unwrap();
        for _ in 0..30 {
            pool.reject(&used, "HTTP 429").unwrap();
        }
        assert_eq!(pool.current().unwrap().token, "new-b");
        pool.replace(vec!["saved".into()]);
        pool.reject(&used, "HTTP 429").unwrap();
        assert_eq!(pool.current().unwrap().token, "saved");
        let used = pool.current().unwrap();
        assert!(pool
            .reject(&used, "HTTP 429")
            .unwrap_err()
            .starts_with("TILE_TOKENS_EXHAUSTED:"));
        pool.replace(vec!["saved".into()]);
        assert!(pool.current().is_err());
        assert!(logs
            .lock()
            .unwrap()
            .iter()
            .all(|line| !line.contains("secret-")
                && !line.contains("new-a")
                && !line.contains("saved")));
    }

    #[test]
    fn only_provider_hosts_and_tk_are_changed() {
        assert!(is_tianditu_url(
            "https://t{s}.tianditu.gov.cn/img_w/wmts?tk=old"
        ));
        for url in [
            "https://evil.test/tianditu.gov.cn",
            "https://tianditu.gov.cn.evil.test/a",
            "https://tianditu.gov.cn@evil.test/a",
        ] {
            assert!(!is_tianditu_url(url));
        }
        let url = token_url(
            "https://t0.tianditu.gov.cn/wmts?x=2&tk=old&hl=en&tk=duplicate",
            "new",
        );
        let parsed = reqwest::Url::parse(&url).unwrap();
        let pairs: Vec<_> = parsed.query_pairs().collect();
        assert_eq!(
            pairs,
            vec![
                ("x".into(), "2".into()),
                ("hl".into(), "en".into()),
                ("tk".into(), "new".into())
            ]
        );
    }
}
