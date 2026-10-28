//! URL liveness: a thread that TCP-connects to every served URL the agents produced, so a card
//! dims when the server behind it dies.

use bevy::prelude::*;
use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Resource, Clone)]
pub struct Probe {
    wanted: Arc<Mutex<Vec<String>>>,
    alive: Arc<Mutex<HashMap<String, bool>>>,
}

impl Default for Probe {
    fn default() -> Self {
        let wanted: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let alive: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(HashMap::new()));
        {
            let (wanted, alive) = (wanted.clone(), alive.clone());
            std::thread::Builder::new()
                .name("url-probe".into())
                .spawn(move || {
                    loop {
                        let urls: Vec<String> = wanted.lock().map(|w| w.clone()).unwrap_or_default();
                        let mut result = HashMap::new();
                        for u in urls {
                            result.insert(u.clone(), reachable(&u));
                        }
                        if let Ok(mut a) = alive.lock() {
                            *a = result;
                        }
                        std::thread::sleep(Duration::from_secs(6));
                    }
                })
                .expect("spawn url-probe");
        }
        Probe { wanted, alive }
    }
}

impl Probe {
    pub fn watch(&self, urls: Vec<String>) {
        if let Ok(mut w) = self.wanted.lock() {
            *w = urls;
        }
    }

    /// None until the first probe; Some(false) means the port refused or timed out.
    pub fn is_alive(&self, url: &str) -> Option<bool> {
        self.alive.lock().ok().and_then(|a| a.get(url).copied())
    }
}

fn host_port(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("https://")
        .map(|r| (r, 443))
        .or_else(|| url.strip_prefix("http://").map(|r| (r, 80)))?;
    let (rest, default_port) = rest;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().unwrap_or(default_port)),
        None => (authority, default_port),
    };
    Some((host.to_string(), port))
}

fn reachable(url: &str) -> bool {
    let Some((host, port)) = host_port(url) else {
        return false;
    };
    let Ok(mut addrs) = (host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    addrs.any(|a| TcpStream::connect_timeout(&a, Duration::from_millis(600)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_and_port() {
        assert_eq!(host_port("http://localhost:3000/app"), Some(("localhost".into(), 3000)));
        assert_eq!(host_port("https://example.com"), Some(("example.com".into(), 443)));
        assert_eq!(host_port("ftp://x"), None);
    }
}
