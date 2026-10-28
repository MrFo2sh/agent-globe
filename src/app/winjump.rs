//! Bring the editor window that owns a session to the front. Windows only; a no-op elsewhere.
//! Matching is by window title: VS Code puts the workspace folder name in it.

/// The folder name a session's editor window shows in its title.
pub fn needle_for(cwd: &str, project: &str) -> String {
    let base = cwd
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if !base.is_empty() {
        return base.to_string();
    }
    // Project slugs look like `d--software-factory`: drive, two dashes, then the path with
    // separators replaced by dashes. Keep the last path segment.
    let rest = project.split_once("--").map(|(_, r)| r).unwrap_or(project);
    rest.to_string()
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// True when a window title belongs to a workspace named `needle`.
pub fn title_matches(title: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let t = title.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    if t.contains(&n) {
        return true;
    }
    // Slugs lose dots and spaces; compare the squashed forms as a fallback.
    let (ts, ns) = (squash(title), squash(needle));
    !ns.is_empty() && ts.contains(&ns)
}

/// Score a candidate window: editors win over browsers and terminals that merely mention the name.
pub fn title_score(title: &str) -> u8 {
    let t = title.to_ascii_lowercase();
    if t.contains("visual studio code") || t.contains("cursor") || t.contains("windsurf") {
        3
    } else if t.contains("code") {
        2
    } else {
        1
    }
}

#[cfg(windows)]
pub fn focus(needle: &str) -> bool {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the Vec we passed below and outlives the enumeration.
        let out = unsafe { &mut *(lparam as *mut Vec<(isize, String)>) };
        // SAFETY: plain Win32 queries on a window handle handed to us by EnumWindows.
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            // Asking one of our own windows for its title sends a message to the main thread,
            // which is busy running this very frame: skip them or the app deadlocks.
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == GetCurrentProcessId() {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 {
                out.push((hwnd as isize, String::from_utf16_lossy(&buf[..n as usize])));
            }
        }
        1
    }

    let mut titles: Vec<(isize, String)> = Vec::new();
    // SAFETY: the callback only reads window titles; the Vec lives for the whole call.
    unsafe {
        EnumWindows(Some(collect), &mut titles as *mut _ as LPARAM);
    }
    let best = titles
        .iter()
        .filter(|(_, t)| title_matches(t, needle))
        .max_by_key(|(_, t)| title_score(t));
    let Some((hwnd, _)) = best else { return false };
    let hwnd = *hwnd as HWND;
    // SAFETY: a handle we just enumerated; restoring and raising a window is harmless if it is gone.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd) != 0
    }
}

#[cfg(not(windows))]
pub fn focus(_needle: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needle_prefers_cwd_then_project_slug() {
        assert_eq!(
            needle_for("D:\\software-factory", "d--software-factory"),
            "software-factory"
        );
        assert_eq!(needle_for("/home/me/my.app/", ""), "my.app");
        assert_eq!(needle_for("", "d--software-factory"), "software-factory");
        assert_eq!(needle_for("", "d--education-2026-sem-2"), "education-2026-sem-2");
    }

    #[test]
    fn titles_match_loosely_and_editors_rank_first() {
        assert!(title_matches(
            "ui.rs - software-factory - Visual Studio Code",
            "software-factory"
        ));
        assert!(title_matches("main.py - my app - Visual Studio Code", "my.app"));
        assert!(!title_matches("Agent Globe", "software-factory"));
        assert!(!title_matches("anything", ""));
        assert!(title_score("x - Visual Studio Code") > title_score("software-factory - Google Chrome"));
    }
}
