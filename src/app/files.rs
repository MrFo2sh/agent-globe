//! The file system beside the session: a lazy tree of the session's folder, and a fuzzy finder
//! (the card's search box, or Ctrl+P from anywhere) over an index built in the background with
//! the gitignore respected. Clicking a file opens it in the editor card; a right-click reveals it
//! in VS Code.

use agent_globe::data::links::{self, Link};
use bevy::prelude::*;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the editor card does with a file, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Code,
    Markdown,
    Pdf,
    Docx,
    Image,
}

pub fn kind_of(path: &str) -> DocKind {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "mdx" => DocKind::Markdown,
        "pdf" => DocKind::Pdf,
        "docx" => DocKind::Docx,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => DocKind::Image,
        _ => DocKind::Code,
    }
}

/// Monaco's language id for a file, by extension.
pub fn language_of(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" | "jsx" => "typescript",
        "js" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" => "json",
        "toml" => "ini",
        "yml" | "yaml" => "yaml",
        "md" | "markdown" | "mdx" => "markdown",
        "html" | "htm" | "vue" | "svelte" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "less" => "less",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "rb" => "ruby",
        "swift" => "swift",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" | "psm1" => "powershell",
        "bat" | "cmd" => "bat",
        "sql" => "sql",
        "xml" | "svg" | "csproj" | "plist" => "xml",
        "dockerfile" => "dockerfile",
        "lua" => "lua",
        "wgsl" | "glsl" | "hlsl" => "cpp",
        "ini" | "cfg" | "conf" | "env" => "ini",
        _ => "plaintext",
    }
}

/// Git repositories nested under `root`, up to `depth` folders down (the root itself excluded).
fn nested_repos(root: &Path, depth: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            if p.join(".git").exists() {
                out.push(p);
            } else {
                walk(&p, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, depth, &mut out);
    out.sort();
    out
}

/// Imports from the source (files up to 256 KB, the first 4,000 scannable ones) and co-change
/// pairs from the last 500 commits of every repository, kept to pairs of files the index knows.
fn connections(root: &Path, files: &[String], repos: &[(PathBuf, String)]) -> Vec<Link> {
    let scan: Vec<String> = files
        .iter()
        .filter(|p| links::scannable(p))
        .take(4_000)
        .cloned()
        .collect();
    let all: Vec<String> = files.to_vec();
    let known: std::collections::HashSet<&str> = all.iter().map(|s| s.as_str()).collect();
    let mut out = links::import_links(&all, |rel| {
        if !scan.iter().any(|s| s == rel) {
            return None;
        }
        let p = root.join(rel);
        let len = std::fs::metadata(&p).ok()?.len();
        if len > 262_144 {
            return None;
        }
        std::fs::read_to_string(&p).ok()
    });
    for (base, prefix) in repos {
        let mut git = std::process::Command::new("git");
        git.args(["log", "--name-only", "--format=%H", "-n", "500"])
            .current_dir(base);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            git.creation_flags(0x0800_0000);
        }
        if let Ok(o) = git.output()
            && o.status.success()
        {
            let log = String::from_utf8_lossy(&o.stdout);
            out.extend(
                links::co_change_links(&log, 30, 2)
                    .into_iter()
                    .map(|l| Link {
                        a: format!("{prefix}{}", l.a),
                        b: format!("{prefix}{}", l.b),
                        ..l
                    })
                    .filter(|l| known.contains(l.a.as_str()) && known.contains(l.b.as_str()))
                    .take(6_000),
            );
        }
    }
    out
}

/// Folders the tree and the index never enter: build output, dependencies, VCS internals.
const SKIP: [&str; 9] = [
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    ".next",
    ".cache",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// One session folder's index: every file path (relative, forward slashes) for the fuzzy
/// finder, built once in a thread, plus a short-lived cache of directory listings for the tree.
#[derive(Resource, Default)]
pub struct FileIndex {
    root: String,
    entries: Arc<Mutex<Vec<String>>>,
    building: Arc<AtomicBool>,
    /// Bumped after every rebuild, so the scene knows when to lay the files out again.
    generation: Arc<AtomicU64>,
    /// Connections between files: imports read from the source, and git co-change pairs.
    links: Arc<Mutex<Vec<Link>>>,
    built: bool,
    /// Walk options: dotfiles and gitignored files in or out; the build was made with `built_with`.
    show_hidden: bool,
    show_ignored: bool,
    built_with: (bool, bool),
    dirs: HashMap<String, (Instant, Vec<Entry>)>,
    matcher: SkimMatcherV2,
}

impl FileIndex {
    /// Point the index at a folder; a new folder (or `force`) rebuilds it in the background.
    /// Which files the walk admits; a change rebuilds the index on the next `ensure`.
    pub fn configure(&mut self, show_hidden: bool, show_ignored: bool) {
        self.show_hidden = show_hidden;
        self.show_ignored = show_ignored;
    }

    pub fn ensure(&mut self, root: &str, force: bool) {
        if root.is_empty() {
            return;
        }
        let opts = (self.show_hidden, self.show_ignored);
        if self.root == root
            && self.built_with == opts
            && (self.built || self.building.load(Ordering::Relaxed))
            && !force
        {
            return;
        }
        self.root = root.to_string();
        // Built (or being built) for this root and these options: only a new root, new options
        // or `force` start another walk. Before 0.29.2 this stayed false until someone searched,
        // so every frame started a full rebuild and a `git log` per repository.
        self.built = true;
        self.built_with = opts;
        self.dirs.clear();
        self.building.store(true, Ordering::Relaxed);
        let (entries, building, generation) = (self.entries.clone(), self.building.clone(), self.generation.clone());
        let links = self.links.clone();
        let (show_hidden, show_ignored) = opts;
        let root = PathBuf::from(root);
        std::thread::Builder::new()
            .name("file-index".into())
            .spawn(move || {
                // The folder itself, then every git repository nested in it (a parent's
                // gitignore often hides them, as a factory hides its products), each walked
                // with its own ignore rules.
                let mut repos: Vec<(PathBuf, String)> = vec![(root.clone(), String::new())];
                for nested in nested_repos(&root, 3) {
                    let prefix = nested
                        .strip_prefix(&root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    repos.push((nested, format!("{prefix}/")));
                }
                let mut out: Vec<String> = Vec::new();
                for (base, prefix) in &repos {
                    let walker = ignore::WalkBuilder::new(base)
                        .hidden(!show_hidden)
                        .git_ignore(!show_ignored)
                        .git_exclude(!show_ignored)
                        .max_depth(Some(14))
                        .filter_entry(|e| !e.file_name().to_str().is_some_and(|n| SKIP.contains(&n)))
                        .build();
                    for e in walker.flatten() {
                        if e.file_type().is_some_and(|t| t.is_file())
                            && let Ok(rel) = e.path().strip_prefix(base)
                        {
                            out.push(format!("{prefix}{}", rel.to_string_lossy().replace('\\', "/")));
                            if out.len() >= 60_000 {
                                break;
                            }
                        }
                    }
                }
                out.sort();
                out.dedup();
                if let Ok(mut v) = entries.lock() {
                    *v = out.clone();
                }
                generation.fetch_add(1, Ordering::Relaxed);
                building.store(false, Ordering::Relaxed);
                // Connections come after the list, so the tree and the finder never wait for them.
                let found = connections(&root, &out, &repos);
                if let Ok(mut l) = links.lock() {
                    *l = found;
                }
                generation.fetch_add(1, Ordering::Relaxed);
            })
            .expect("spawn file index");
    }

    /// The connections found by the last rebuild.
    pub fn links_snapshot(&self) -> Vec<Link> {
        self.links.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// The first `cap` indexed paths (relative, forward slashes), sorted.
    pub fn snapshot(&self, cap: usize) -> Vec<String> {
        self.entries
            .lock()
            .map(|v| v.iter().take(cap).cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_building(&self) -> bool {
        self.building.load(Ordering::Relaxed)
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Fuzzy matches for `query`, best first: (relative path, matched char positions).
    pub fn search(&mut self, query: &str, limit: usize) -> Vec<(String, Vec<usize>)> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let Ok(entries) = self.entries.lock() else {
            return Vec::new();
        };
        let mut scored: Vec<(i64, &String, Vec<usize>)> = entries
            .iter()
            .filter_map(|p| self.matcher.fuzzy_indices(p, q).map(|(s, idx)| (s, p, idx)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, p, idx)| (p.clone(), idx))
            .collect()
    }

    /// The listing of one folder, dirs first, hidden and skipped folders left out; cached for
    /// a few seconds so the tree can be drawn every frame.
    pub fn list(&mut self, dir: &str) -> Vec<Entry> {
        if let Some((at, v)) = self.dirs.get(dir)
            && at.elapsed() < Duration::from_secs(5)
        {
            return v.clone();
        }
        let mut out: Vec<Entry> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if (name.starts_with('.') && !self.show_hidden) || SKIP.contains(&name.as_str()) {
                    continue;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let path = e.path().to_string_lossy().to_string();
                out.push(Entry { name, path, is_dir });
            }
        }
        out.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.dirs.insert(dir.to_string(), (Instant::now(), out.clone()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_languages_follow_the_extension() {
        assert_eq!(kind_of("docs/COURSE.md"), DocKind::Markdown);
        assert_eq!(kind_of("paper.PDF"), DocKind::Pdf);
        assert_eq!(kind_of("thesis.docx"), DocKind::Docx);
        assert_eq!(kind_of("shot.png"), DocKind::Image);
        assert_eq!(kind_of("src/app/ui.rs"), DocKind::Code);
        assert_eq!(language_of("src/app/ui.rs"), "rust");
        assert_eq!(language_of("proxy/src/server.js"), "javascript");
        assert_eq!(language_of("Cargo.toml"), "ini");
        assert_eq!(language_of("README"), "plaintext");
    }

    #[test]
    fn index_finds_files_fuzzily() {
        let dir = std::env::temp_dir().join(format!("ag-index-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("src/app"));
        std::fs::write(dir.join("src/app/ui.rs"), "x").unwrap();
        std::fs::write(dir.join("src/app/terminal.rs"), "x").unwrap();
        std::fs::write(dir.join("README.md"), "x").unwrap();
        let mut index = FileIndex::default();
        index.ensure(&dir.to_string_lossy(), true);
        let deadline = Instant::now() + Duration::from_secs(5);
        while index.is_building() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(index.len(), 3);
        // one build bumps the generation twice (the list, then its connections): let it settle
        std::thread::sleep(Duration::from_millis(1500));
        let generation = index.generation();
        index.ensure(&dir.to_string_lossy(), false);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(index.generation(), generation, "an unchanged root must not rebuild");
        assert!(!index.is_building());
        let hits = index.search("term", 5);
        assert_eq!(hits[0].0, "src/app/terminal.rs");
        assert!(!hits[0].1.is_empty(), "matched positions light up in the list");
        let list = index.list(&dir.to_string_lossy());
        assert_eq!(list[0].name, "src", "folders first");
        let _ = std::fs::remove_dir_all(dir);
    }
}
