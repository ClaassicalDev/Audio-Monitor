use std::collections::{HashMap, HashSet};
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, GW_OWNER,
};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub exe_name: String,
}

#[derive(Debug, Clone)]
pub struct RunningProcess {
    #[allow(dead_code)]
    pub pid: u32,
    pub exe_name: String,
    pub window_title: Option<String>,
    pub is_system: bool,
}

pub const SYSTEM_PROCESSES: &[&str] = &[
    "svchost.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "dllhost.exe",
    "backgroundtaskhost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "sihost.exe",
    "taskhostw.exe",
    "explorer.exe",
    "dwm.exe",
    "csrss.exe",
    "lsass.exe",
    "services.exe",
    "smss.exe",
    "wininit.exe",
    "winlogon.exe",
    "wmiprvse.exe",
    "spoolsv.exe",
    "fontdrvhost.exe",
    "audiodg.exe",
    "securityhealthsystray.exe",
    "securityhealthservice.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "audiomonitor.exe",
    "system",
    "registry",
    "ctfmon.exe",
    "smartscreen.exe",
    "searchapp.exe",
    "searchindexer.exe",
    "igfxem.exe",
    "nvcontainer.exe",
    "dasHost.exe",
    "compattelrunner.exe",
];

/// Takes a snapshot of all running processes in the system.
pub fn get_all_processes() -> Vec<ProcessInfo> {
    let mut processes = Vec::new();

    unsafe {
        let snapshot: HANDLE = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return processes,
        };

        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]);

                processes.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    exe_name,
                });

                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    processes
}

/// Enumerates visible top-level windows and gathers their window titles mapped by Process ID.
pub fn get_window_titles() -> HashMap<u32, String> {
    let mut titles: HashMap<u32, String> = HashMap::new();
    unsafe {
        let lparam = LPARAM(&mut titles as *mut HashMap<u32, String> as isize);
        let _ = EnumWindows(Some(enum_windows_callback), lparam);
    }
    titles
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let length = GetWindowTextLengthW(hwnd);
    if length == 0 {
        return BOOL(1);
    }

    // Skip windows with owner (child popups, tooltips, flyouts)
    if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
        if owner != HWND::default() {
            return BOOL(1);
        }
    }

    let mut buf = vec![0u16; (length + 1) as usize];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len > 0 {
        let text = String::from_utf16_lossy(&buf[..len as usize]);
        let trimmed = text.trim();
        if !trimmed.is_empty()
            && trimmed != "Default IME"
            && trimmed != "MSCTFIME UI"
            && trimmed != "Program Manager"
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                let map = &mut *(lparam.0 as *mut HashMap<u32, String>);
                // Prefer keeping a title, or replacing if the new one is more descriptive
                map.entry(pid).or_insert_with(|| trimmed.to_string());
            }
        }
    }

    BOOL(1)
}

/// Resolves the Root PID for each executable name.
/// For multi-process applications (Chrome, Discord, Brave, Opera, Spotify, etc.),
/// the Root PID is the top-level process whose parent process does NOT share
/// the same executable name.
pub fn resolve_root_pids(processes: &[ProcessInfo]) -> HashMap<String, u32> {
    // Group processes by lower-case executable name
    let mut by_name: HashMap<String, Vec<&ProcessInfo>> = HashMap::new();
    for p in processes {
        let key = p.exe_name.to_lowercase();
        by_name.entry(key).or_default().push(p);
    }

    let mut root_pids = HashMap::new();

    for (name_lower, procs) in by_name {
        let pids_in_group: HashSet<u32> = procs.iter().map(|p| p.pid).collect();

        // A root process is one whose parent PID is not in the group
        let mut roots: Vec<&ProcessInfo> = procs
            .iter()
            .copied()
            .filter(|p| !pids_in_group.contains(&p.parent_pid))
            .collect();

        if roots.is_empty() {
            // In case of circular parentage or unusual trees, fallback to smallest PID
            if let Some(min_p) = procs.iter().min_by_key(|p| p.pid) {
                root_pids.insert(name_lower, min_p.pid);
            }
        } else {
            // Sort by PID to pick the oldest/main root
            roots.sort_by_key(|p| p.pid);
            root_pids.insert(name_lower, roots[0].pid);
        }
    }

    root_pids
}

/// Smart match for executable name against running root PIDs.
/// Resilient against `.exe` suffix, casing, or partial matching when added without `.exe`.
pub fn find_root_pid(target_exe: &str, root_pids: &HashMap<String, u32>) -> Option<u32> {
    let target = target_exe.trim();
    if target.is_empty() {
        return None;
    }

    let target_lower = target.to_lowercase();
    let target_clean = target_lower.trim_end_matches(".exe");

    // 1. Direct case-insensitive match (e.g. "Overwatch.exe" -> "overwatch.exe")
    if let Some(&pid) = root_pids.get(&target_lower) {
        return Some(pid);
    }

    // 2. Exact match with .exe added
    let target_with_exe = format!("{}.exe", target_clean);
    if let Some(&pid) = root_pids.get(&target_with_exe) {
        return Some(pid);
    }

    // 3. Match without .exe in root_pids keys
    for (exe, &pid) in root_pids {
        let exe_clean = exe.trim_end_matches(".exe");
        if exe_clean == target_clean {
            return Some(pid);
        }
    }

    // 4. Starts-with or contains fallback if user typed friendly name (e.g. "Overwatch" -> "Overwatch.exe")
    for (exe, &pid) in root_pids {
        let exe_clean = exe.trim_end_matches(".exe");
        if exe_clean.starts_with(target_clean) || exe_clean.contains(target_clean) {
            return Some(pid);
        }
    }

    None
}

/// Returns all user-facing executables with window titles and system process markers.
pub fn get_user_processes(processes: &[ProcessInfo]) -> Vec<RunningProcess> {
    let system_ignore: HashSet<&str> = SYSTEM_PROCESSES.iter().copied().collect();
    let root_pids = resolve_root_pids(processes);
    let window_titles = get_window_titles();

    // Map each exe to any window title discovered in its process tree
    let mut exe_to_title: HashMap<String, String> = HashMap::new();
    for p in processes {
        if let Some(title) = window_titles.get(&p.pid) {
            let key = p.exe_name.to_lowercase();
            // If title is more informative, prefer it
            exe_to_title
                .entry(key)
                .and_modify(|existing| {
                    if title.len() > existing.len() {
                        *existing = title.clone();
                    }
                })
                .or_insert_with(|| title.clone());
        }
    }

    let mut list = Vec::new();
    let mut seen_names = HashSet::new();

    for p in processes {
        let lower = p.exe_name.to_lowercase();
        if seen_names.insert(lower.clone()) {
            if let Some(&root_pid) = root_pids.get(&lower) {
                let is_system = system_ignore.contains(lower.as_str());
                let title = exe_to_title.get(&lower).cloned();

                list.push(RunningProcess {
                    pid: root_pid,
                    exe_name: p.exe_name.clone(),
                    window_title: title,
                    is_system,
                });
            }
        }
    }

    // Sort: apps with a visible window title appear first (alphabetically by title),
    // followed by background apps (alphabetically by exe name).
    list.sort_by(|a, b| match (&a.window_title, &b.window_title) {
        (Some(ta), Some(tb)) => ta.to_lowercase().cmp(&tb.to_lowercase()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.exe_name.to_lowercase().cmp(&b.exe_name.to_lowercase()),
    });

    list
}
