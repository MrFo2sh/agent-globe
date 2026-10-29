//! Plan usage, the same numbers the Claude Code extension shows: five-hour session, weekly, and
//! per-model limits. Read from Anthropic's OAuth usage endpoint with the Claude Code login stored on
//! this machine. The token is read when needed, kept in memory, never logged or written anywhere.

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const POLL: Duration = Duration::from_secs(90);
/// After a 429 the endpoint wants a longer pause.
const BACKOFF: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub key: String,
    pub label: String,
    /// Percent used, 0 to 100.
    pub utilization: f32,
    /// Unix milliseconds when the window resets, if the reply said.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub limits: Vec<Limit>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageState {
    /// No Claude Code login on this machine (or the file has another shape).
    NoLogin,
    /// Something failed; short reason for the HUD tooltip.
    Error(String),
    Ready(Usage),
}

#[derive(Resource, Clone)]
pub struct UsageWatch {
    state: Arc<Mutex<Option<UsageState>>>,
}

/// Human label for a limit key as the endpoint names it.
pub fn label_for(key: &str) -> String {
    match key {
        "five_hour" => "Session (5hr)".into(),
        "seven_day" => "Weekly (7 day)".into(),
        k => {
            // seven_day_opus -> "Opus limit"; anything else -> words with the first letter up
            let rest = k.strip_prefix("seven_day_").unwrap_or(k);
            let mut words: Vec<String> = rest
                .split('_')
                .filter(|w| !w.is_empty())
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect();
            if k.starts_with("seven_day_") {
                words.push("limit".into());
            }
            words.join(" ")
        }
    }
}

/// Short label for the HUD.
pub fn short_label(key: &str) -> String {
    match key {
        "five_hour" => "5h".into(),
        "seven_day" => "7d".into(),
        k => label_for(k).replace(" limit", ""),
    }
}

fn parse_time(v: &serde_json::Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(if n > 10_000_000_000 { n } else { n * 1000 });
    }
    let s = v.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Every object in the reply that carries a `utilization`, in a stable order: session, weekly,
/// then the rest alphabetically.
pub fn parse_usage(body: &str, now: i64) -> Result<Usage, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    let obj = v.as_object().ok_or("reply is not an object")?;
    let mut limits: Vec<Limit> = obj
        .iter()
        .filter_map(|(k, val)| {
            let o = val.as_object()?;
            let util = o.get("utilization")?.as_f64()? as f32;
            Some(Limit {
                key: k.clone(),
                label: label_for(k),
                utilization: util.clamp(0.0, 100.0),
                resets_at: o.get("resets_at").and_then(parse_time),
            })
        })
        .collect();
    let rank = |k: &str| match k {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    };
    limits.sort_by(|a, b| rank(&a.key).cmp(&rank(&b.key)).then(a.key.cmp(&b.key)));
    if limits.is_empty() {
        let keys: Vec<&String> = obj.keys().collect();
        return Err(format!("no limits in reply (keys: {keys:?})"));
    }
    Ok(Usage {
        limits,
        fetched_at: now,
    })
}

/// "Resets at 8:30 AM" today, otherwise "Resets Wed, Nov 4 at 7:00 AM".
pub fn resets_text(resets_at: Option<i64>, now: i64) -> String {
    let Some(ms) = resets_at else { return String::new() };
    let Some(t) = chrono::DateTime::from_timestamp_millis(ms) else {
        return String::new();
    };
    let t = t.with_timezone(&chrono::Local);
    let today = chrono::DateTime::from_timestamp_millis(now)
        .map(|n| n.with_timezone(&chrono::Local).date_naive())
        .unwrap_or_else(|| t.date_naive());
    if t.date_naive() == today {
        format!("Resets at {}", t.format("%-I:%M %p"))
    } else {
        format!("Resets {}", t.format("%a, %b %-d at %-I:%M %p"))
    }
}

/// The access token of the Claude Code login, read fresh each time so a re-login is picked up.
fn access_token(claude_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(claude_dir.join(".credentials.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn fetch(claude_dir: &Path) -> UsageState {
    let Some(token) = access_token(claude_dir) else {
        return UsageState::NoLogin;
    };
    let resp = ureq::get(USAGE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(15))
        .call();
    match resp {
        Ok(r) => match r.into_string() {
            Ok(body) => match parse_usage(&body, chrono::Utc::now().timestamp_millis()) {
                Ok(u) => UsageState::Ready(u),
                Err(e) => UsageState::Error(e),
            },
            Err(e) => UsageState::Error(format!("read failed: {e}")),
        },
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            UsageState::Error("login expired: run `claude` and sign in again".into())
        }
        Err(ureq::Error::Status(code, _)) => UsageState::Error(format!("usage endpoint answered {code}")),
        Err(e) => UsageState::Error(format!("no connection: {e}")),
    }
}

impl UsageWatch {
    /// Start polling once a minute in the background.
    pub fn start(claude_dir: PathBuf) -> Self {
        let state: Arc<Mutex<Option<UsageState>>> = Arc::new(Mutex::new(None));
        let shared = state.clone();
        std::thread::Builder::new()
            .name("usage".into())
            .spawn(move || {
                loop {
                    let next = fetch(&claude_dir);
                    let mut wait = POLL;
                    if let UsageState::Error(e) = &next {
                        warn!("usage: {e}");
                        if e.contains("429") {
                            wait = BACKOFF;
                        }
                    }
                    if let Ok(mut s) = shared.lock() {
                        // A passing error keeps the last good reading on screen.
                        let keep_old = matches!(next, UsageState::Error(_)) && matches!(*s, Some(UsageState::Ready(_)));
                        if !keep_old {
                            *s = Some(next);
                        }
                    }
                    std::thread::sleep(wait);
                }
            })
            .expect("spawn usage");
        UsageWatch { state }
    }

    pub fn snapshot(&self) -> Option<UsageState> {
        self.state.lock().ok().and_then(|s| s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_endpoint_keys() {
        assert_eq!(label_for("five_hour"), "Session (5hr)");
        assert_eq!(label_for("seven_day"), "Weekly (7 day)");
        assert_eq!(label_for("seven_day_opus"), "Opus limit");
        assert_eq!(label_for("seven_day_fable"), "Fable limit");
        assert_eq!(short_label("seven_day_fable"), "Fable");
        assert_eq!(short_label("five_hour"), "5h");
    }

    #[test]
    fn replies_are_parsed_in_a_stable_order_with_reset_times() {
        let body = r#"{"seven_day_fable":{"utilization":67.0,"resets_at":"2026-11-04T05:00:00Z"},
            "five_hour":{"utilization":44.2,"resets_at":"2026-10-29T06:30:00Z"},
            "seven_day":{"utilization":33,"resets_at":"2026-11-04T05:00:00Z"},
            "extra":"ignored","nested":{"no_util":1}}"#;
        let u = parse_usage(body, 0).unwrap();
        let keys: Vec<&str> = u.limits.iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, ["five_hour", "seven_day", "seven_day_fable"]);
        assert!((u.limits[0].utilization - 44.2).abs() < 1e-4);
        assert!(u.limits[0].resets_at.is_some());
        assert!(parse_usage(r#"{"a":1}"#, 0).is_err());
        assert!(parse_usage("nope", 0).is_err());
    }

    #[test]
    fn reset_text_is_short_for_today_and_dated_otherwise() {
        let now = chrono::Local::now();
        let later_today = now + chrono::Duration::minutes(5);
        let text = resets_text(Some(later_today.timestamp_millis()), now.timestamp_millis());
        assert!(text.starts_with("Resets at "), "{text}");
        let next_week = now + chrono::Duration::days(6);
        let text = resets_text(Some(next_week.timestamp_millis()), now.timestamp_millis());
        assert!(text.starts_with("Resets ") && text.contains(" at "), "{text}");
        assert_eq!(resets_text(None, 0), "");
    }
}
