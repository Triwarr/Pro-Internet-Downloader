//! Pro Internet Manager: system tray icon (Win32 Shell_NotifyIcon, no extra crates besides
//! windows-sys, which eframe/winit already pull in) and a single-instance guard.
//!
//! A hidden message-only window on its own thread owns the tray icon:
//! - left click / double click / Enter: show the main window again
//! - right click: menu with "Open Pro Internet Manager" and "Exit"
//! A second start of the exe posts WM_PIM_SHOW to that window and quits.
//! No balloons, toasts or other notifications are ever shown.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICONDATAW,
    NOTIFYICON_VERSION_4,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowExW,
    GetCursorPos, GetMessageW, GetSystemMetrics, IsIconic, IsWindowVisible, LoadImageW, PostMessageW, PostQuitMessage, RegisterClassW,
    AllowSetForegroundWindow, RegisterWindowMessageW, SetForegroundWindow, ShowWindowAsync, TrackPopupMenu, TranslateMessage, HWND_MESSAGE, IMAGE_ICON,
    LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, SW_RESTORE, SW_SHOW, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    WNDCLASSW,
};

const CLASS_NAME: &str = "ProInternetManagerTrayWindow";
const MUTEX_NAME: &str = "Local\\ProInternetManager_Restyle_SingleInstance";

/// Test copies on another API port (hidden PIM_API_PORT) get their own mutex and tray class,
/// so they never talk to the installed app. Normal starts use the fixed names.
fn instance_suffix() -> String {
    if crate::api_port() == crate::API_PORT { String::new() } else { format!("_{}", crate::api_port()) }
}
const WM_PIM_TRAY: u32 = WM_APP + 1;
const WM_PIM_SHOW: u32 = WM_APP + 2;
const NIN_SELECT: u32 = 0x0400;
const NIN_KEYSELECT: u32 = 0x0401;
const CMD_OPEN: usize = 1;
const CMD_EXIT: usize = 2;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static ICON_ADDED: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);
static CTX: OnceLock<egui::Context> = OnceLock::new();
static DONE: OnceLock<Mutex<Option<std::sync::mpsc::Receiver<()>>>> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Single instance: returns false when another (non-demo) instance already runs.
/// The running instance is asked to show its window.
pub fn acquire_single_instance() -> bool {
    let name = wide(&format!("{MUTEX_NAME}{}", instance_suffix()));
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        if handle.is_null() {
            crate::pimlog::warn("Single-instance mutex could not be created; continuing without it.");
            return true;
        }
        if !already {
            // Keep the handle open for the lifetime of the process.
            return true;
        }
        let class = wide(&format!("{CLASS_NAME}{}", instance_suffix()));
        // The running instance may still be starting: look for its tray window for up to 3 s.
        // PostMessageW never waits, so this second copy cannot hang on a busy first instance.
        let mut other = std::ptr::null_mut();
        for _ in 0..30 {
            other = FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
            if !other.is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if other.is_null() {
            crate::pimlog::warn("Another Pro Internet Manager instance holds the mutex but its tray window was not found; this instance exits.");
        } else {
            // We were started by the user, so we may hand the foreground right to the running copy.
            AllowSetForegroundWindow(u32::MAX);
            PostMessageW(other, WM_PIM_SHOW, 0, 0);
            crate::pimlog::info("Second start: asked the running instance to show its window; this instance exits.");
        }
        false
    }
}

pub fn is_running() -> bool {
    ICON_ADDED.load(Ordering::SeqCst)
}

pub fn is_hidden() -> bool {
    HIDDEN.load(Ordering::SeqCst)
}

pub fn take_exit_request() -> bool {
    EXIT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Hide the main window to the tray (called by the app on close / --minimized).
pub fn hide(ctx: &egui::Context) {
    HIDDEN.store(true, Ordering::SeqCst);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
}

/// Show and focus the main window (from the tray thread or the app).
pub fn show_main() {
    HIDDEN.store(false, Ordering::SeqCst);
    if let Some(ctx) = CTX.get() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
    let hwnd = MAIN_HWND.load(Ordering::SeqCst) as HWND;
    if !hwnd.is_null() {
        unsafe {
            ShowWindowAsync(hwnd, SW_SHOW);
            if IsIconic(hwnd) != 0 {
                ShowWindowAsync(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
}

static USE_GUID: AtomicBool = AtomicBool::new(false);

/// Stable tray GUID per exe path (Windows ties a tray GUID to one exe path).
fn icon_guid() -> windows_sys::core::GUID {
    let path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let fnv = |seed: u64| {
        let mut h: u64 = 0xcbf29ce484222325 ^ seed;
        for b in format!("PIM-tray|{path}").bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let mut v = ((fnv(0) as u128) << 64) | fnv(0x9e3779b97f4a7c15) as u128;
    // RFC 4122 version 5 / variant bits.
    v = (v & !(0xF000u128 << 64)) | (0x5000u128 << 64);
    v = (v & !(0xC000u128 << 48)) | (0x8000u128 << 48);
    windows_sys::core::GUID::from_u128(v)
}

fn add_icon(hwnd: HWND) -> bool {
    unsafe {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = WM_PIM_TRAY;
        // Resource 1 = assets\ProInternetManager.ico, embedded by build.rs.
        data.hIcon = LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            1usize as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR,
        ) as _;
        if data.hIcon.is_null() {
            crate::pimlog::warn("Tray: exe icon resource not found; the tray icon may look empty.");
        }
        for (index, unit) in "Pro Internet Manager".encode_utf16().enumerate() {
            data.szTip[index] = unit;
        }
        // 0.2.4: the icon is identified by a GUID derived from the exe path. A copy that was
        // killed or crashed leaves a "ghost" icon behind; deleting that GUID first removes it,
        // so a restart never shows two icons. Falls back to the plain id if Windows refuses.
        let mut guid_data = data;
        guid_data.uFlags |= NIF_GUID;
        guid_data.guidItem = icon_guid();
        Shell_NotifyIconW(NIM_DELETE, &guid_data);
        if Shell_NotifyIconW(NIM_ADD, &guid_data) != 0 {
            USE_GUID.store(true, Ordering::SeqCst);
            data = guid_data;
        } else {
            USE_GUID.store(false, Ordering::SeqCst);
            crate::pimlog::warn(format!("Tray: GUID icon refused (error {}), using the classic id.", GetLastError()));
            if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                crate::pimlog::error(format!("Tray: Shell_NotifyIcon(NIM_ADD) failed (error {}).", GetLastError()));
                return false;
            }
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &data);
        true
    }
}

fn delete_icon(hwnd: HWND) {
    unsafe {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        if USE_GUID.load(Ordering::SeqCst) {
            data.uFlags = NIF_GUID;
            data.guidItem = icon_guid();
        }
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
    ICON_ADDED.store(false, Ordering::SeqCst);
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            crate::pimlog::error("Tray: CreatePopupMenu failed.");
            return;
        }
        let open = wide("Open Pro Internet Manager");
        let exit = wide("Exit");
        AppendMenuW(menu, MF_STRING, CMD_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, CMD_EXIT, exit.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Required so the menu closes when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, point.x, point.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match command as usize {
            CMD_OPEN => {
                crate::pimlog::info("Tray menu: Open Pro Internet Manager");
                show_main();
            }
            CMD_EXIT => {
                crate::pimlog::info("Tray menu: Exit requested");
                EXIT_REQUESTED.store(true, Ordering::SeqCst);
                if let Some(ctx) = CTX.get() {
                    ctx.request_repaint();
                }
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PIM_TRAY => {
            // NOTIFYICON_VERSION_4: the event is in LOWORD(lParam).
            let event = (lparam as u32) & 0xFFFF;
            match event {
                NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_main(),
                WM_CONTEXTMENU | WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_PIM_SHOW => {
            show_main();
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            delete_icon(hwnd);
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => {
            let taskbar = TASKBAR_CREATED.load(Ordering::SeqCst) as u32;
            if taskbar != 0 && msg == taskbar {
                // Explorer restarted: put the icon back.
                crate::pimlog::info("Tray: Explorer restarted, re-adding the tray icon.");
                ICON_ADDED.store(add_icon(hwnd), Ordering::SeqCst);
                return 0;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

/// Start the tray thread. `main_hwnd` is the eframe window (0 if unknown).
pub fn start(ctx: egui::Context, main_hwnd: isize) {
    let _ = CTX.set(ctx);
    MAIN_HWND.store(main_hwnd, Ordering::SeqCst);
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
    let _ = DONE.set(Mutex::new(Some(done_rx)));
    thread::spawn(move || {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide(&format!("{CLASS_NAME}{}", instance_suffix()));
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
            let title = wide("Pro Internet Manager tray");
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                crate::pimlog::error(format!("Tray: hidden tray window could not be created (error {}).", GetLastError()));
                let _ = ready_tx.send(false);
                let _ = done_tx.send(());
                return;
            }
            TRAY_HWND.store(hwnd as isize, Ordering::SeqCst);
            let tb = wide("TaskbarCreated");
            TASKBAR_CREATED.store(RegisterWindowMessageW(tb.as_ptr()) as isize, Ordering::SeqCst);
            let ok = add_icon(hwnd);
            ICON_ADDED.store(ok, Ordering::SeqCst);
            if ok {
                crate::pimlog::info("Tray icon added (left click: show window, right click: menu).");
            }
            let _ = ready_tx.send(ok);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            TRAY_HWND.store(0, Ordering::SeqCst);
        }
        let _ = done_tx.send(());
    });
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(true) => {}
        Ok(false) => crate::pimlog::warn("Tray icon unavailable: closing the window will exit the app."),
        Err(_) => crate::pimlog::warn("Tray thread did not report back within 3 s."),
    }
}

/// Remove the tray icon and stop the tray thread (waits up to 1 s).
pub fn shutdown() {
    let hwnd = TRAY_HWND.load(Ordering::SeqCst) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
    if let Some(lock) = DONE.get() {
        if let Some(rx) = lock.lock().ok().and_then(|mut g| g.take()) {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }
}

/// `--tray-selftest`: checks from outside the window that the tray window exists.
pub fn tray_window_exists() -> bool {
    let class = wide(&format!("{CLASS_NAME}{}", instance_suffix()));
    unsafe { !FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null()).is_null() }
}

/// Win32 visibility of the main eframe window (for self-tests and logging).
pub fn main_window_visible() -> bool {
    let hwnd = MAIN_HWND.load(Ordering::SeqCst) as HWND;
    !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) != 0 }
}
