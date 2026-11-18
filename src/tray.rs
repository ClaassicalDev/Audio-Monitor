use crate::config::{save_config, AppConfig};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, InvalidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DispatchMessageW, EnumWindows, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetWindowPos, ShowWindow, ShowWindowAsync, SystemParametersInfoW, TrackPopupMenu, HWND_TOP,
    ICONINFO, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, SPI_GETWORKAREA, SWP_SHOWWINDOW,
    SW_HIDE, SW_RESTORE, SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, TPM_BOTTOMALIGN,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAY_CALLBACK: u32 = WM_USER + 101;
const ID_MENU_STUDIO: usize = 1000;
const ID_MENU_FLYOUT: usize = 1001;
const ID_MENU_VOICE_CLARITY: usize = 1002;
const ID_MENU_SPECTRAL_DUCKING: usize = 1003;
const ID_MENU_FOCUS_MODE: usize = 1004;
const ID_MENU_EXIT: usize = 1005;

pub const FLYOUT_WIDTH: i32 = 540;
pub const FLYOUT_HEIGHT: i32 = 600;
pub const STUDIO_WIDTH: i32 = 1140;
pub const STUDIO_HEIGHT: i32 = 740;

pub const ACTION_NONE: u8 = 0;
pub const ACTION_TOGGLE_FLYOUT: u8 = 1;
pub const ACTION_OPEN_STUDIO: u8 = 2;
pub const ACTION_OPEN_FLYOUT: u8 = 3;

pub const REPOS_NONE: u8 = 0;
pub const REPOS_FLYOUT: u8 = 1;
pub const REPOS_STUDIO: u8 = 2;

/// Lock-free atomic action written by tray_wnd_proc so explorer.exe NEVER blocks
static GLOBAL_PENDING_ACTION: AtomicU8 = AtomicU8::new(ACTION_NONE);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct TrayHandle {
    pub exit_requested: Arc<AtomicBool>,
    pub show_requested: Arc<AtomicBool>,
    pub pending_reposition: Arc<AtomicU8>,
    pub main_hwnd: Arc<AtomicIsize>,
    pub flyout_mode: Arc<AtomicBool>,
    pub auto_hide_enabled: Arc<AtomicBool>,
    pub has_gained_focus: Arc<AtomicBool>,
    pub last_shown_ms: Arc<AtomicU64>,
    pub last_hidden_ms: Arc<AtomicU64>,
}

impl TrayHandle {
    pub fn get_main_hwnd(&self) -> Option<HWND> {
        let val = self.main_hwnd.load(Ordering::SeqCst);
        if val != 0 {
            Some(HWND(val as *mut _))
        } else {
            None
        }
    }

    pub fn set_main_hwnd(&self, hwnd: HWND) {
        self.main_hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
    }
}

/// Called ONLY from the main UI thread (`AudioMonitorApp::ui`) that owns `hwnd`!
pub unsafe fn position_window_flyout_on_owner_thread(hwnd: HWND, width: i32, height: i32) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }

    let mut work_area = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work_area as *mut _ as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );

    let x = (work_area.right - width - 12).max(work_area.left + 8);
    let y = (work_area.bottom - height - 12).max(work_area.top + 8);

    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOP),
        x,
        y,
        width,
        height,
        SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
}

/// Called ONLY from the main UI thread (`AudioMonitorApp::ui`) that owns `hwnd`!
pub unsafe fn position_window_studio_on_owner_thread(hwnd: HWND, width: i32, height: i32) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }

    let mut work_area = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work_area as *mut _ as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );

    let screen_w = work_area.right - work_area.left;
    let screen_h = work_area.bottom - work_area.top;

    let x = (work_area.left + (screen_w - width) / 2).max(work_area.left + 8);
    let y = (work_area.top + (screen_h - height) / 2).max(work_area.top + 8);

    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOP),
        x,
        y,
        width,
        height,
        SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
}

/// Generates a crisp 32x32 studio icon (headphones + sound waves in cyan & emerald)
pub fn generate_icon_rgba() -> (Vec<u8>, u32, u32) {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let fx = x as f32;
            let fy = y as f32;

            let dx = (fx - 15.5).abs();
            let dy = (fy - 15.5).abs();
            let dist = dx.max(dy);
            if dist > 15.0 {
                continue;
            }

            rgba[idx] = 17;
            rgba[idx + 1] = 19;
            rgba[idx + 2] = 24;
            rgba[idx + 3] = 255;

            let arc_cx = 15.5;
            let arc_cy = 13.0;
            let arc_dist = ((fx - arc_cx).powi(2) + (fy - arc_cy).powi(2)).sqrt();
            if fy < 14.0 && (arc_dist >= 8.5 && arc_dist <= 11.0) {
                rgba[idx] = 2;
                rgba[idx + 1] = 132;
                rgba[idx + 2] = 199;
                rgba[idx + 3] = 255;
            }

            if fx >= 5.0 && fx <= 8.5 && fy >= 13.0 && fy <= 23.0 {
                rgba[idx] = 56;
                rgba[idx + 1] = 189;
                rgba[idx + 2] = 248;
                rgba[idx + 3] = 255;
            }

            if fx >= 22.5 && fx <= 26.0 && fy >= 13.0 && fy <= 23.0 {
                rgba[idx] = 56;
                rgba[idx + 1] = 189;
                rgba[idx + 2] = 248;
                rgba[idx + 3] = 255;
            }

            if fx >= 11.0 && fx <= 12.0 && fy >= 16.0 && fy <= 20.0 {
                rgba[idx] = 16;
                rgba[idx + 1] = 185;
                rgba[idx + 2] = 129;
                rgba[idx + 3] = 255;
            }
            if fx >= 14.0 && fx <= 16.0 && fy >= 13.0 && fy <= 23.0 {
                rgba[idx] = 52;
                rgba[idx + 1] = 211;
                rgba[idx + 2] = 153;
                rgba[idx + 3] = 255;
            }
            if fx >= 18.0 && fx <= 19.0 && fy >= 15.0 && fy <= 21.0 {
                rgba[idx] = 16;
                rgba[idx + 1] = 185;
                rgba[idx + 2] = 129;
                rgba[idx + 3] = 255;
            }
        }
    }

    (rgba, width as u32, height as u32)
}

/// Convert RGBA buffer into a Win32 HICON
pub unsafe fn create_hicon_from_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    let mut bgra = vec![0u8; rgba.len()];
    for i in (0..rgba.len()).step_by(4) {
        bgra[i] = rgba[i + 2];
        bgra[i + 1] = rgba[i + 1];
        bgra[i + 2] = rgba[i];
        bgra[i + 3] = rgba[i + 3];
    }

    let hbm_color = CreateBitmap(
        width as i32,
        height as i32,
        1,
        32,
        Some(bgra.as_ptr() as *const _),
    );
    if hbm_color.is_invalid() {
        return None;
    }

    let mask_bytes = vec![0u8; (width * height / 8) as usize];
    let hbm_mask = CreateBitmap(
        width as i32,
        height as i32,
        1,
        1,
        Some(mask_bytes.as_ptr() as *const _),
    );
    if hbm_mask.is_invalid() {
        let _ = DeleteObject(hbm_color.into());
        return None;
    }

    let icon_info = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hicon = CreateIconIndirect(&icon_info).ok();
    let _ = DeleteObject(hbm_mask.into());
    let _ = DeleteObject(hbm_color.into());
    hicon
}

/// Finds the main application window by Process ID and Title
pub unsafe fn find_main_window() -> Option<HWND> {
    let mut target_hwnd: Option<HWND> = None;

    unsafe extern "system" fn enum_wnd_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target = &mut *(lparam.0 as *mut Option<HWND>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            let mut title = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title);
            if len > 0 {
                let text = String::from_utf16_lossy(&title[..len as usize]);
                if text.contains("Audio Monitor") {
                    *target = Some(hwnd);
                    return BOOL(0);
                }
            }
        }
        BOOL(1)
    }

    let _ = EnumWindows(
        Some(enum_wnd_proc),
        LPARAM(&mut target_hwnd as *mut _ as isize),
    );
    target_hwnd
}

struct TrayContext {
    config: Arc<RwLock<AppConfig>>,
    exit_requested: Arc<AtomicBool>,
}

static GLOBAL_TRAY_CTX: Mutex<Option<TrayContext>> = Mutex::new(None);

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY_CALLBACK => {
            let event = lparam.0 as u32;
            if event == WM_LBUTTONUP {
                // 100% lock-free & non-blocking: never call SetWindowPos/ShowWindow inside tray_wnd_proc!
                GLOBAL_PENDING_ACTION.store(ACTION_TOGGLE_FLYOUT, Ordering::SeqCst);
            } else if event == WM_LBUTTONDBLCLK {
                GLOBAL_PENDING_ACTION.store(ACTION_OPEN_STUDIO, Ordering::SeqCst);
            } else if event == WM_RBUTTONUP {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);

                let hmenu = CreatePopupMenu().unwrap_or_default();
                if !hmenu.is_invalid() {
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING,
                        ID_MENU_STUDIO,
                        w!("Abrir Ventana Normal (Estudio Completo)"),
                    );
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING,
                        ID_MENU_FLYOUT,
                        w!("Abrir Mezclador Rápido (Popup Bandeja)"),
                    );
                    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);

                    let (vc_active, sd_active, fm_active) = {
                        let ctx_lock = GLOBAL_TRAY_CTX.lock();
                        if let Some(ctx) = ctx_lock.as_ref() {
                            let cfg = ctx.config.read();
                            (
                                cfg.voice_clarity_enabled,
                                cfg.compressor.spectral_ducking,
                                cfg.focus_mode_enabled,
                            )
                        } else {
                            (true, true, true)
                        }
                    };

                    let vc_flag = if vc_active { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING | vc_flag,
                        ID_MENU_VOICE_CLARITY,
                        w!("Voz Clara Discord (ON/OFF)"),
                    );

                    let sd_flag = if sd_active { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING | sd_flag,
                        ID_MENU_SPECTRAL_DUCKING,
                        w!("Ducking Espectral (ON/OFF)"),
                    );

                    let fm_flag = if fm_active { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING | fm_flag,
                        ID_MENU_FOCUS_MODE,
                        w!("Modo Enfoque Prio 2 (ON/OFF)"),
                    );

                    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                    let _ = AppendMenuW(
                        hmenu,
                        MF_STRING,
                        ID_MENU_EXIT,
                        w!("Salir por completo"),
                    );

                    let _ = SetForegroundWindow(hwnd);
                    let _ = TrackPopupMenu(
                        hmenu,
                        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                        pt.x,
                        pt.y,
                        Some(0),
                        hwnd,
                        None,
                    );
                    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                    let _ = DestroyMenu(hmenu);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as usize;
            match id {
                ID_MENU_STUDIO => {
                    GLOBAL_PENDING_ACTION.store(ACTION_OPEN_STUDIO, Ordering::SeqCst);
                }
                ID_MENU_FLYOUT => {
                    GLOBAL_PENDING_ACTION.store(ACTION_OPEN_FLYOUT, Ordering::SeqCst);
                }
                ID_MENU_VOICE_CLARITY => {
                    let ctx_lock = GLOBAL_TRAY_CTX.lock();
                    if let Some(ctx) = ctx_lock.as_ref() {
                        let mut cfg = ctx.config.write();
                        cfg.voice_clarity_enabled = !cfg.voice_clarity_enabled;
                        let _ = save_config(&cfg);
                    }
                }
                ID_MENU_SPECTRAL_DUCKING => {
                    let ctx_lock = GLOBAL_TRAY_CTX.lock();
                    if let Some(ctx) = ctx_lock.as_ref() {
                        let mut cfg = ctx.config.write();
                        cfg.compressor.spectral_ducking = !cfg.compressor.spectral_ducking;
                        let _ = save_config(&cfg);
                    }
                }
                ID_MENU_FOCUS_MODE => {
                    let ctx_lock = GLOBAL_TRAY_CTX.lock();
                    if let Some(ctx) = ctx_lock.as_ref() {
                        let mut cfg = ctx.config.write();
                        cfg.focus_mode_enabled = !cfg.focus_mode_enabled;
                        let _ = save_config(&cfg);
                    }
                }
                ID_MENU_EXIT => {
                    let ctx_lock = GLOBAL_TRAY_CTX.lock();
                    if let Some(ctx) = ctx_lock.as_ref() {
                        ctx.exit_requested.store(true, Ordering::SeqCst);
                    }
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn start_tray(config: Arc<RwLock<AppConfig>>) -> TrayHandle {
    let exit_requested = Arc::new(AtomicBool::new(false));
    let show_requested = Arc::new(AtomicBool::new(false));
    let pending_reposition = Arc::new(AtomicU8::new(REPOS_NONE));
    let main_hwnd = Arc::new(AtomicIsize::new(0));
    let flyout_mode = Arc::new(AtomicBool::new(false));
    let auto_hide_enabled = Arc::new(AtomicBool::new(false));
    let has_gained_focus = Arc::new(AtomicBool::new(false));
    let last_shown_ms = Arc::new(AtomicU64::new(0));
    let last_hidden_ms = Arc::new(AtomicU64::new(0));

    let exit_clone = exit_requested.clone();
    let cfg_clone = config.clone();

    // Asynchronous worker thread: processes GLOBAL_PENDING_ACTION and auto-hide using ONLY ShowWindowAsync/PostMessageW
    let worker_hwnd = main_hwnd.clone();
    let worker_show = show_requested.clone();
    let worker_repos = pending_reposition.clone();
    let worker_flyout = flyout_mode.clone();
    let worker_autohide = auto_hide_enabled.clone();
    let worker_focus = has_gained_focus.clone();
    let worker_shown_ms = last_shown_ms.clone();
    let worker_hidden_ms = last_hidden_ms.clone();
    let worker_exit = exit_requested.clone();

    thread::Builder::new()
        .name("tray-worker".into())
        .spawn(move || {
            while !worker_exit.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_millis(35));

                let mut val = worker_hwnd.load(Ordering::SeqCst);
                if val == 0 {
                    if let Some(found) = unsafe { find_main_window() } {
                        val = found.0 as isize;
                        worker_hwnd.store(val, Ordering::SeqCst);
                    } else {
                        continue;
                    }
                }
                let hwnd = HWND(val as *mut _);

                // 1. Check if the user clicked the tray icon or context menu
                let action = GLOBAL_PENDING_ACTION.swap(ACTION_NONE, Ordering::SeqCst);
                if action != ACTION_NONE {
                    unsafe {
                        match action {
                            ACTION_TOGGLE_FLYOUT => {
                                let is_vis = IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool();
                                if is_vis {
                                    worker_focus.store(false, Ordering::SeqCst);
                                    worker_hidden_ms.store(now_ms(), Ordering::SeqCst);
                                    let _ = ShowWindowAsync(hwnd, SW_HIDE);
                                } else {
                                    let elapsed = now_ms()
                                        .saturating_sub(worker_hidden_ms.load(Ordering::SeqCst));
                                    if elapsed >= 300 {
                                        worker_flyout.store(true, Ordering::SeqCst);
                                        worker_autohide.store(true, Ordering::SeqCst);
                                        worker_focus.store(false, Ordering::SeqCst);
                                        worker_shown_ms.store(now_ms(), Ordering::SeqCst);
                                        worker_repos.store(REPOS_FLYOUT, Ordering::SeqCst);
                                        worker_show.store(true, Ordering::SeqCst);
                                        let _ = ShowWindowAsync(hwnd, SW_SHOW);
                                        let _ = InvalidateRect(Some(hwnd), None, false);
                                        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                                    }
                                }
                            }
                            ACTION_OPEN_FLYOUT => {
                                worker_flyout.store(true, Ordering::SeqCst);
                                worker_autohide.store(true, Ordering::SeqCst);
                                worker_focus.store(false, Ordering::SeqCst);
                                worker_shown_ms.store(now_ms(), Ordering::SeqCst);
                                worker_repos.store(REPOS_FLYOUT, Ordering::SeqCst);
                                worker_show.store(true, Ordering::SeqCst);
                                let _ = ShowWindowAsync(hwnd, SW_SHOW);
                                let _ = InvalidateRect(Some(hwnd), None, false);
                                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                            }
                            ACTION_OPEN_STUDIO => {
                                worker_flyout.store(false, Ordering::SeqCst);
                                worker_autohide.store(false, Ordering::SeqCst);
                                worker_focus.store(false, Ordering::SeqCst);
                                worker_shown_ms.store(now_ms(), Ordering::SeqCst);
                                worker_repos.store(REPOS_STUDIO, Ordering::SeqCst);
                                worker_show.store(true, Ordering::SeqCst);
                                let _ = ShowWindowAsync(hwnd, SW_SHOW);
                                let _ = InvalidateRect(Some(hwnd), None, false);
                                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                            }
                            _ => {}
                        }
                    }
                    continue;
                }

                // 2. Auto-hide on blur (only when in Flyout mode and auto_hide_enabled is true)
                if !worker_flyout.load(Ordering::SeqCst)
                    || !worker_autohide.load(Ordering::SeqCst)
                {
                    continue;
                }

                unsafe {
                    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                        continue;
                    }

                    let fg = GetForegroundWindow();
                    if fg == hwnd {
                        worker_focus.store(true, Ordering::SeqCst);
                        continue;
                    }

                    let elapsed = now_ms().saturating_sub(worker_shown_ms.load(Ordering::SeqCst));
                    let had_focus = worker_focus.load(Ordering::SeqCst);

                    if had_focus && elapsed > 350 && fg.0 != std::ptr::null_mut() {
                        let mut pt = POINT::default();
                        let mut wrect = RECT::default();
                        let _ = GetCursorPos(&mut pt);
                        let _ = GetWindowRect(hwnd, &mut wrect);

                        let cursor_inside = pt.x >= wrect.left
                            && pt.x <= wrect.right
                            && pt.y >= wrect.top
                            && pt.y <= wrect.bottom;

                        if !cursor_inside {
                            worker_focus.store(false, Ordering::SeqCst);
                            worker_hidden_ms.store(now_ms(), Ordering::SeqCst);
                            let _ = ShowWindowAsync(hwnd, SW_HIDE);
                        }
                    }
                }
            }
        })
        .ok();

    thread::Builder::new()
        .name("system-tray".into())
        .spawn(move || unsafe {
            *GLOBAL_TRAY_CTX.lock() = Some(TrayContext {
                config: cfg_clone,
                exit_requested: exit_clone.clone(),
            });

            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = w!("AudioMonitorTrayWindow");

            let wc = WNDCLASSW {
                lpfnWndProc: Some(tray_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("AudioMonitorTrayHidden"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance.into()),
                None,
            );

            let hwnd = match hwnd {
                Ok(h) => h,
                Err(e) => {
                    log::error!("Failed to create tray hidden window: {:?}", e);
                    return;
                }
            };

            let (rgba, w, h) = generate_icon_rgba();
            let hicon = create_hicon_from_rgba(&rgba, w, h).unwrap_or_default();

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAY_CALLBACK,
                hIcon: hicon,
                ..Default::default()
            };

            let tip_utf16: Vec<u16> = "Audio Monitor — Clic Izq: Mezclador Rápido | Clic Der: Menú"
                .encode_utf16()
                .collect();
            let copy_len = tip_utf16.len().min(nid.szTip.len() - 1);
            nid.szTip[..copy_len].copy_from_slice(&tip_utf16[..copy_len]);

            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if exit_clone.load(Ordering::SeqCst) {
                    break;
                }
                let _ = DispatchMessageW(&msg);
            }

            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            if exit_clone.load(Ordering::SeqCst) {
                std::process::exit(0);
            }
        })
        .expect("Failed to start tray thread");

    TrayHandle {
        exit_requested,
        show_requested,
        pending_reposition,
        main_hwnd,
        flyout_mode,
        auto_hide_enabled,
        has_gained_focus,
        last_shown_ms,
        last_hidden_ms,
    }
}
