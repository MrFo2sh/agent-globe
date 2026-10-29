//! Connections between files, from three sources:
//! - static imports, read from the source (Rust `mod`/`use`, TypeScript and JavaScript `import`
//!   and `require`, Python `import`), resolved against the file list;
//! - temporal coupling, from git history: files that change in the same commits;
//! - (live, in the app) files the same unit touched in the same turn.
//!
//! Pure helpers; the index thread calls them and the scene draws the result.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkKind {
    Import,
    CoChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Relative paths with forward slashes, `a < b` for co-change (undirected), importer → imported for imports.
    pub a: String,
    pub b: String,
    pub kind: LinkKind,
    pub weight: u32,
}

/// Files whose text is worth scanning for imports, by extension.
pub fn scannable(path: &str) -> bool {
    matches!(
        ext(path).as_str(),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "py" | "vue" | "svelte"
    )
}

fn ext(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn dir_of(rel: &str) -> &str {
    rel.rfind('/').map(|i| &rel[..i]).unwrap_or("")
}

/// `dir/../x` folded, forward slashes, no leading `./`.
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            x => out.push(x),
        }
    }
    out.join("/")
}

/// The import specifiers in one file: Rust module paths, JS/TS module strings, Python modules.
pub fn specifiers(path: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    match ext(path).as_str() {
        "rs" => {
            for line in text.lines() {
                let l = line.trim_start();
                if let Some(rest) = l.strip_prefix("mod ").or_else(|| l.strip_prefix("pub mod ")) {
                    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() && rest.trim_end().ends_with(';') {
                        out.push(format!("mod:{name}"));
                    }
                } else if let Some(rest) = l.strip_prefix("use ").or_else(|| l.strip_prefix("pub use ")) {
                    // `use crate::a::b::{c, d};` → crate::a::b ; `use super::x;` → super::x
                    let head: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                        .collect();
                    if head.starts_with("crate::") || head.starts_with("super::") || head.starts_with("self::") {
                        out.push(format!("use:{}", head.trim_end_matches("::")));
                    }
                }
            }
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "vue" | "svelte" => {
            for line in text.lines() {
                let l = line.trim_start();
                let is_import = l.starts_with("import ") || l.starts_with("export ") || l.contains("require(");
                if !is_import {
                    continue;
                }
                for q in ['\'', '"'] {
                    let mut rest = l;
                    while let Some(i) = rest.find(q) {
                        let after = &rest[i + 1..];
                        if let Some(j) = after.find(q) {
                            let spec = &after[..j];
                            if spec.starts_with('.') {
                                out.push(format!("js:{spec}"));
                            }
                            rest = &after[j + 1..];
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        "py" => {
            for line in text.lines() {
                let l = line.trim_start();
                if let Some(rest) = l.strip_prefix("from ") {
                    let module: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
                    out.push(format!("py:{module}"));
                } else if let Some(rest) = l.strip_prefix("import ") {
                    for m in rest.split(',') {
                        let module: String = m.trim().chars().take_while(|c| !c.is_whitespace()).collect();
                        if !module.is_empty() {
                            out.push(format!("py:{module}"));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

/// The project file a specifier points at, if any.
pub fn resolve(from: &str, spec: &str, files: &HashSet<String>) -> Option<String> {
    let dir = dir_of(from);
    let has = |p: &str| files.contains(p).then(|| p.to_string());
    if let Some(name) = spec.strip_prefix("mod:") {
        // `mod x;` beside `x.rs`, `x/mod.rs`, or (from `a.rs`) `a/x.rs`
        let stem = Path::new(from).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let candidates = [
            format!("{dir}/{name}.rs"),
            format!("{dir}/{name}/mod.rs"),
            format!("{dir}/{stem}/{name}.rs"),
            format!("{dir}/{stem}/{name}/mod.rs"),
        ];
        return candidates.iter().map(|c| normalize(c)).find_map(|c| has(&c));
    }
    if let Some(path) = spec.strip_prefix("use:") {
        let parts: Vec<&str> = path.split("::").collect();
        let (base, rest): (String, &[&str]) = match parts.first().copied() {
            Some("crate") => {
                // the crate root is the nearest `src` above the file, else the file's top folder
                let root = from.find("src/").map(|i| &from[..i + 3]).unwrap_or("src");
                (root.to_string(), &parts[1..])
            }
            Some("super") => (dir_of(dir).to_string(), &parts[1..]),
            Some("self") => (dir.to_string(), &parts[1..]),
            _ => return None,
        };
        // longest prefix that is a file: a/b/c → a/b/c.rs, a/b/c/mod.rs, a/b.rs, …
        for n in (1..=rest.len()).rev() {
            let joined = rest[..n].join("/");
            let candidates = [format!("{base}/{joined}.rs"), format!("{base}/{joined}/mod.rs")];
            if let Some(f) = candidates.iter().map(|c| normalize(c)).find_map(|c| has(&c)) {
                return Some(f);
            }
        }
        return None;
    }
    if let Some(rel) = spec.strip_prefix("js:") {
        let target = normalize(&format!("{dir}/{rel}"));
        let exts = [
            "", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".vue", ".svelte", ".json",
        ];
        for e in exts {
            if let Some(f) = has(&format!("{target}{e}")) {
                return Some(f);
            }
        }
        for e in ["ts", "tsx", "js", "jsx", "mjs"] {
            if let Some(f) = has(&format!("{target}/index.{e}")) {
                return Some(f);
            }
        }
        return None;
    }
    if let Some(module) = spec.strip_prefix("py:") {
        let dots = module.chars().take_while(|c| *c == '.').count();
        let name = &module[dots..];
        let mut base = dir.to_string();
        for _ in 1..dots.max(1) {
            base = dir_of(&base).to_string();
        }
        let joined = name.replace('.', "/");
        let candidates: Vec<String> = if dots > 0 {
            vec![format!("{base}/{joined}.py"), format!("{base}/{joined}/__init__.py")]
        } else {
            vec![
                format!("{joined}.py"),
                format!("{joined}/__init__.py"),
                format!("{dir}/{joined}.py"),
                format!("src/{joined}.py"),
            ]
        };
        return candidates.iter().map(|c| normalize(c)).find_map(|c| has(&c));
    }
    None
}

/// Import links for a set of files, given a reader for their text.
pub fn import_links(files: &[String], read: impl Fn(&str) -> Option<String>) -> Vec<Link> {
    let set: HashSet<String> = files.iter().cloned().collect();
    let mut out = Vec::new();
    for f in files {
        if !scannable(f) {
            continue;
        }
        let Some(text) = read(f) else { continue };
        for spec in specifiers(f, &text) {
            if let Some(to) = resolve(f, &spec, &set)
                && to != *f
            {
                out.push(Link {
                    a: f.clone(),
                    b: to,
                    kind: LinkKind::Import,
                    weight: 1,
                });
            }
        }
    }
    out.sort_by(|x, y| (&x.a, &x.b).cmp(&(&y.a, &y.b)));
    out.dedup_by(|x, y| x.a == y.a && x.b == y.b);
    out
}

/// Temporal coupling from `git log --name-only --format=%H`: pairs of files that changed in the
/// same commit, counted. Commits touching more than `max_files` are skipped (reorganisations
/// couple everything with everything), and only pairs seen at least `min_count` times are kept.
pub fn co_change_links(log: &str, max_files: usize, min_count: u32) -> Vec<Link> {
    let mut pairs: HashMap<(String, String), u32> = HashMap::new();
    let mut commit: Vec<String> = Vec::new();
    let flush = |commit: &mut Vec<String>, pairs: &mut HashMap<(String, String), u32>| {
        if commit.len() >= 2 && commit.len() <= max_files {
            commit.sort();
            commit.dedup();
            for i in 0..commit.len() {
                for j in i + 1..commit.len() {
                    *pairs.entry((commit[i].clone(), commit[j].clone())).or_insert(0) += 1;
                }
            }
        }
        commit.clear();
    };
    for line in log.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let is_hash = l.len() >= 7 && l.chars().all(|c| c.is_ascii_hexdigit());
        if is_hash {
            flush(&mut commit, &mut pairs);
        } else {
            commit.push(l.replace('\\', "/"));
        }
    }
    flush(&mut commit, &mut pairs);
    let mut out: Vec<Link> = pairs
        .into_iter()
        .filter(|(_, n)| *n >= min_count)
        .map(|((a, b), n)| Link {
            a,
            b,
            kind: LinkKind::CoChange,
            weight: n,
        })
        .collect();
    out.sort_by(|x, y| y.weight.cmp(&x.weight).then_with(|| (&x.a, &x.b).cmp(&(&y.a, &y.b))));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rust_mod_and_use_resolve_to_files() {
        let fs = files(&[
            "src/main.rs",
            "src/app/mod.rs",
            "src/app/ui.rs",
            "src/data/model.rs",
            "src/data/mod.rs",
        ]);
        let specs = specifiers(
            "src/app/mod.rs",
            "mod ui;\npub mod scene;\nuse super::data::model::Event;\nuse crate::data::model::{Session, Event};\nuse std::fs;\n",
        );
        assert!(specs.contains(&"mod:ui".to_string()));
        assert!(specs.contains(&"use:crate::data::model".to_string()));
        assert!(!specs.iter().any(|s| s.contains("std")));
        assert_eq!(
            resolve("src/app/mod.rs", "mod:ui", &fs).as_deref(),
            Some("src/app/ui.rs")
        );
        assert_eq!(
            resolve("src/app/mod.rs", "use:crate::data::model", &fs).as_deref(),
            Some("src/data/model.rs")
        );
        assert_eq!(
            resolve("src/app/ui.rs", "use:super::data::model", &fs).as_deref(),
            Some("src/data/model.rs")
        );
        assert_eq!(
            resolve("src/main.rs", "mod:app", &fs).as_deref(),
            Some("src/app/mod.rs")
        );
    }

    #[test]
    fn javascript_relative_imports_and_python_modules_resolve() {
        let fs = files(&[
            "proxy/src/server.js",
            "proxy/src/db.js",
            "proxy/src/lib/index.ts",
            "pkg/__init__.py",
            "pkg/util.py",
            "pkg/sub/a.py",
        ]);
        let specs = specifiers(
            "proxy/src/server.js",
            "import x from './db.js';\nconst y = require('./lib');\nimport z from 'express';\n",
        );
        assert_eq!(specs, vec!["js:./db.js", "js:./lib"]);
        assert_eq!(
            resolve("proxy/src/server.js", "js:./db.js", &fs).as_deref(),
            Some("proxy/src/db.js")
        );
        assert_eq!(
            resolve("proxy/src/server.js", "js:./lib", &fs).as_deref(),
            Some("proxy/src/lib/index.ts")
        );
        assert_eq!(
            resolve("pkg/sub/a.py", "py:..util", &fs).as_deref(),
            Some("pkg/util.py")
        );
        assert_eq!(
            resolve("pkg/sub/a.py", "py:pkg.util", &fs).as_deref(),
            Some("pkg/util.py")
        );
        assert_eq!(resolve("pkg/sub/a.py", "py:os", &fs), None);
    }

    #[test]
    fn co_change_counts_pairs_and_skips_big_commits() {
        let log = "abc1234\nsrc/a.rs\nsrc/b.rs\n\ndef5678\nsrc/a.rs\nsrc/b.rs\nsrc/c.rs\n\n9999999\n";
        let mut big = String::from("fffffff\n");
        for i in 0..40 {
            big.push_str(&format!("f{i}.rs\n"));
        }
        let links = co_change_links(&format!("{log}{big}"), 30, 2);
        assert_eq!(links.len(), 1, "only a and b changed together twice: {links:?}");
        assert_eq!(
            (links[0].a.as_str(), links[0].b.as_str(), links[0].weight),
            ("src/a.rs", "src/b.rs", 2)
        );
        assert_eq!(links[0].kind, LinkKind::CoChange);
    }

    #[test]
    fn import_links_dedupe_and_skip_self() {
        let fs = vec!["src/main.rs".to_string(), "src/app.rs".to_string()];
        let links = import_links(&fs, |p| match p {
            "src/main.rs" => Some("mod app;\nuse crate::app;\nuse crate::app::X;\n".to_string()),
            _ => Some(String::new()),
        });
        assert_eq!(links.len(), 1);
        assert_eq!(
            (links[0].a.as_str(), links[0].b.as_str()),
            ("src/main.rs", "src/app.rs")
        );
    }
}
