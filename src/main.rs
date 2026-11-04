// =============================================================================
//  WinPick – window overview with live previews and title search (Windows 10/11)
//  Version: 2026-11-04 14:20 (Europe/Berlin)
//
//  - A global hotkey (default Win+J, configurable) shows an overlay on the
//    monitor under the mouse cursor.
//  - Search box on top (focused), settings button next to it; below it live
//    previews (DWM thumbnails) of all open windows in a scrollable grid.
//  - Typing filters by window title (case-insensitive).
//  - Enter brings all matching windows to the front (the first tile ends up
//    in front); with an empty search box only the first tile. Click = this
//    window only, Esc = close.
//  - Settings are stored portably in WinPick.ini next to the EXE
//    (fallback: %APPDATA%\WinPick\WinPick.ini).
//
//  Design rule: never touch other processes' input state. No AttachThreadInput,
//  no injected key presses; WinPick only uses the foreground right it already
//  has from the hotkey or the user's last input.
// =============================================================================
#![windows_subsystem = "windows"]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

use windows::core::{w, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus as gp;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, ReleaseMutex};
use windows::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------
const APP_NAME: &str = "WinPick";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const OVERLAY_CLASS: PCWSTR = w!("WinPickOverlay");
const SETTINGS_CLASS: PCWSTR = w!("WinPickSettings");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

const HOTKEY_ID: i32 = 1;
const IDC_SEARCH: i32 = 1001;
const IDC_BTN_SETTINGS: i32 = 1002;
const WM_APP_TRAY: u32 = WM_APP + 1;
const WM_APP_SHOW: u32 = WM_APP + 2;
const WM_APP_HIDE: u32 = WM_APP + 3;
const IDM_OPEN: usize = 2001;
const IDM_SETTINGS: usize = 2002;
const IDM_ABOUT: usize = 2003;
const IDM_EXIT: usize = 2004;
const IDS_CTRL: i32 = 3001;
const IDS_ALT: i32 = 3002;
const IDS_SHIFT: i32 = 3003;
const IDS_WIN: i32 = 3004;
const IDS_KEY: i32 = 3005;
const IDS_COLS: i32 = 3006;
const IDS_PERCENT: i32 = 3007;
const IDS_MARGIN: i32 = 3008;
const IDS_SWATCH: i32 = 3009;
const IDS_COLORBTN: i32 = 3010;
const IDS_AUTOSTART: i32 = 3011;

const SS_LEFT: u32 = 0x0000;
const SS_SUNKEN: u32 = 0x1000;
const MODS_MASK: u32 = MOD_ALT.0 | MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_WIN.0;

// ----------------------------------------------------------------------------
// Small helpers
// ----------------------------------------------------------------------------
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn hs(s: &str) -> HSTRING {
    HSTRING::from(s)
}
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}
fn cr_r(c: COLORREF) -> u8 {
    (c.0 & 0xFF) as u8
}
fn cr_g(c: COLORREF) -> u8 {
    ((c.0 >> 8) & 0xFF) as u8
}
fn cr_b(c: COLORREF) -> u8 {
    ((c.0 >> 16) & 0xFF) as u8
}
fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}
fn loword(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}
fn hiword(v: usize) -> u32 {
    ((v >> 16) & 0xFFFF) as u32
}
fn x_lparam(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as u16 as i16 as i32
}
fn y_lparam(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as u16 as i16 as i32
}
fn mul_div(a: i32, b: i32, c: i32) -> i32 {
    ((a as i64 * b as i64) / c as i64) as i32
}
unsafe fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    SendMessageW(h, msg, Some(WPARAM(wp)), Some(LPARAM(lp))).0
}
unsafe fn window_text(h: HWND) -> String {
    let len = GetWindowTextLengthW(h);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(h, &mut buf);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
fn lower(s: &str) -> String {
    s.to_lowercase()
}

// ----------------------------------------------------------------------------
// Settings
// ----------------------------------------------------------------------------
#[derive(Clone, Copy)]
struct Settings {
    mods: u32,
    vk: u32,
    bg: COLORREF,
    columns: i32,
    size_percent: i32,
    margin_mm: i32,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { mods: MOD_WIN.0, vk: 'J' as u32, bg: rgb(28, 30, 36), columns: 4, size_percent: 100, margin_mm: 20 }
    }
}

fn exe_path() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
}
fn exe_dir() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
        .unwrap_or_else(|| ".".into())
}
fn appdata_dir() -> String {
    match std::env::var("APPDATA") {
        Ok(a) if !a.is_empty() => format!("{a}\\WinPick"),
        _ => exe_dir(),
    }
}
fn valid_key(vk: u32) -> bool {
    (0x30..=0x39).contains(&vk)
        || (0x41..=0x5A).contains(&vk)
        || (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk)
        || [VK_SPACE.0, VK_OEM_PERIOD.0, VK_OEM_COMMA.0, VK_OEM_MINUS.0, VK_OEM_PLUS.0, VK_TAB.0]
            .contains(&(vk as u16))
}

unsafe fn load_settings(st: &mut State) {
    let local = format!("{}\\WinPick.ini", exe_dir());
    let roam = format!("{}\\WinPick.ini", appdata_dir());
    let exists = |p: &str| std::path::Path::new(p).exists();
    st.ini_path = if !exists(&local) && exists(&roam) { roam } else { local };
    let f = hs(&st.ini_path);
    let d = Settings::default();
    let geti = |k: &str, def: i32| GetPrivateProfileIntW(w!("WinPick"), &hs(k), def, &f) as i32;
    let mut s = Settings {
        mods: geti("HotkeyMods", d.mods as i32) as u32 & MODS_MASK,
        vk: geti("HotkeyKey", d.vk as i32) as u32,
        bg: d.bg,
        columns: geti("Columns", d.columns).clamp(1, 10),
        size_percent: geti("SizePercent", d.size_percent).clamp(30, 100),
        margin_mm: geti("MarginMm", d.margin_mm).clamp(0, 200),
    };
    let mut buf = [0u16; 32];
    let n = GetPrivateProfileStringW(w!("WinPick"), w!("BackgroundColor"), w!(""), Some(&mut buf), &f) as usize;
    let col = String::from_utf16_lossy(&buf[..n]);
    if col.len() == 7 && col.starts_with('#') {
        if let Ok(v) = u32::from_str_radix(&col[1..], 16) {
            s.bg = rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        }
    }
    if s.mods == 0 || !valid_key(s.vk) {
        s.mods = d.mods;
        s.vk = d.vk;
    }
    st.set = s;
}

unsafe fn write_settings_to(s: &Settings, path: &str) -> bool {
    let f = hs(path);
    let put = |k: &str, v: String| WritePrivateProfileStringW(w!("WinPick"), &hs(k), &hs(&v), &f).is_ok();
    put("HotkeyMods", s.mods.to_string())
        && put("HotkeyKey", s.vk.to_string())
        && put("Columns", s.columns.to_string())
        && put("SizePercent", s.size_percent.to_string())
        && put("MarginMm", s.margin_mm.to_string())
        && put("BackgroundColor", format!("#{:02X}{:02X}{:02X}", cr_r(s.bg), cr_g(s.bg), cr_b(s.bg)))
}

unsafe fn save_settings(st: &mut State) {
    if write_settings_to(&st.set, &st.ini_path) {
        return;
    }
    let dir = appdata_dir();
    let _ = std::fs::create_dir_all(&dir);
    let roam = format!("{dir}\\WinPick.ini");
    if write_settings_to(&st.set, &roam) {
        st.ini_path = roam;
        return;
    }
    message(st.settings, "The settings could not be saved.", MB_ICONWARNING);
}

// ----------------------------------------------------------------------------
// Autostart (HKCU\...\Run)
// ----------------------------------------------------------------------------
fn run_value() -> String {
    format!("\"{}\" --autostart", exe_path())
}
unsafe fn get_autostart() -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let r = RegGetValueW(
        HKEY_CURRENT_USER,
        RUN_KEY,
        &hs(APP_NAME),
        RRF_RT_REG_SZ,
        None,
        Some(buf.as_mut_ptr() as *mut c_void),
        Some(&mut size),
    );
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..n]))
}
unsafe fn set_autostart(on: bool) {
    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
        return;
    }
    if on {
        let v = wide(&run_value());
        let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
        let _ = RegSetValueExW(key, &hs(APP_NAME), None, REG_SZ, Some(bytes));
    } else {
        let _ = RegDeleteValueW(key, &hs(APP_NAME));
    }
    let _ = RegCloseKey(key);
}

// ----------------------------------------------------------------------------
// Hotkey names
// ----------------------------------------------------------------------------
unsafe fn key_name(vk: u32) -> String {
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return char::from_u32(vk).map(|c| c.to_string()).unwrap_or_default();
    }
    if (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk) {
        return format!("F{}", vk - VK_F1.0 as u32 + 1);
    }
    match vk as u16 {
        x if x == VK_SPACE.0 => return "Space".into(),
        x if x == VK_TAB.0 => return "Tab".into(),
        x if x == VK_OEM_PERIOD.0 => return ".".into(),
        x if x == VK_OEM_COMMA.0 => return ",".into(),
        x if x == VK_OEM_MINUS.0 => return "-".into(),
        x if x == VK_OEM_PLUS.0 => return "+".into(),
        _ => {}
    }
    let sc = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX);
    let mut lp = ((sc & 0xFF) << 16) as i32;
    if sc & 0xFF00 != 0 {
        lp |= 1 << 24; // extended key
    }
    let mut buf = [0u16; 64];
    let n = GetKeyNameTextW(lp, &mut buf);
    if sc != 0 && n > 0 {
        return String::from_utf16_lossy(&buf[..n as usize]);
    }
    format!("#{vk}")
}
unsafe fn hotkey_text(mods: u32, vk: u32) -> String {
    let mut s = String::new();
    if mods & MOD_WIN.0 != 0 {
        s += "Win+";
    }
    if mods & MOD_CONTROL.0 != 0 {
        s += "Ctrl+";
    }
    if mods & MOD_ALT.0 != 0 {
        s += "Alt+";
    }
    if mods & MOD_SHIFT.0 != 0 {
        s += "Shift+";
    }
    s + &key_name(vk)
}

// ----------------------------------------------------------------------------
// Theme
// ----------------------------------------------------------------------------
#[derive(Default, Clone, Copy)]
struct Theme {
    bg: COLORREF,
    tile: COLORREF,
    tile_hover: COLORREF,
    thumb_bg: COLORREF,
    search: COLORREF,
    button: COLORREF,
    button_down: COLORREF,
    text: COLORREF,
    text_dim: COLORREF,
    accent: COLORREF,
    scroll: COLORREF,
}
fn mix(a: COLORREF, b: COLORREF, t: f64) -> COLORREF {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    rgb(m(cr_r(a), cr_r(b)), m(cr_g(a), cr_g(b)), m(cr_b(a), cr_b(b)))
}
fn is_dark(c: COLORREF) -> bool {
    0.299 * cr_r(c) as f64 + 0.587 * cr_g(c) as f64 + 0.114 * (cr_b(c) as f64) < 140.0
}
unsafe fn build_theme(st: &mut State) {
    let bg = st.set.bg;
    let dark = is_dark(bg);
    let up = if dark { rgb(255, 255, 255) } else { rgb(0, 0, 0) };
    let text = if dark { rgb(238, 238, 240) } else { rgb(24, 24, 28) };
    let mut accent = rgb(0, 120, 215);
    let mut argb = 0u32;
    let mut opaque = BOOL(0);
    if DwmGetColorizationColor(&mut argb, &mut opaque).is_ok() {
        accent = rgb((argb >> 16) as u8, (argb >> 8) as u8, argb as u8);
    }
    st.th = Theme {
        bg,
        tile: mix(bg, up, if dark { 0.07 } else { 0.05 }),
        tile_hover: mix(bg, up, if dark { 0.15 } else { 0.11 }),
        thumb_bg: mix(bg, rgb(0, 0, 0), if dark { 0.28 } else { 0.08 }),
        search: if dark { mix(bg, up, 0.12) } else { mix(bg, rgb(255, 255, 255), 0.85) },
        button: mix(bg, up, if dark { 0.10 } else { 0.07 }),
        button_down: mix(bg, up, if dark { 0.20 } else { 0.15 }),
        text,
        text_dim: mix(text, bg, 0.45),
        accent,
        scroll: mix(bg, text, 0.30),
    };
    if !st.br_search.is_invalid() {
        let _ = DeleteObject(st.br_search.into());
    }
    st.br_search = CreateSolidBrush(st.th.search);
}

// ----------------------------------------------------------------------------
// State (single UI thread)
// ----------------------------------------------------------------------------
struct Item {
    hwnd: HWND,
    title: String,
    lower: String,
    thumb: isize,
    src: SIZE,
    thumb_ok: bool,
    minimized: bool,
    icon: HICON, // own copy (CopyIcon), destroyed in clear_items
}

struct State {
    inst: HINSTANCE,
    set: Settings,
    ini_path: String,
    th: Theme,
    br_search: HBRUSH,
    overlay: HWND,
    edit: HWND,
    btn: HWND,
    settings: HWND,
    old_edit_proc: isize,
    items: Vec<Item>,
    filtered: Vec<usize>,
    scroll: i32,
    hover: i32,
    tracking_mouse: bool,
    dpi: u32,
    f_search: HFONT,
    f_title: HFONT,
    f_small: HFONT,
    f_icon: HFONT,
    f_btn: HFONT,
    search_font_h: i32,
    icon_face: Option<String>,
    rc_search: RECT,
    rc_grid: RECT,
    cell_w: i32,
    cell_h: i32,
    gap: i32,
    pad: i32,
    title_h: i32,
    title_gap: i32,
    nid: NOTIFYICONDATAW,
    msg_taskbar_created: u32,
    hotkey_ok: bool,
    // settings dialog
    set_font: HFONT,
    br_swatch: HBRUSH,
    tmp_color: COLORREF,
    swatch: HWND,
}

static mut STATE: *mut State = null_mut();

/// Access to the UI state. Only ever called on the UI thread; callers never keep
/// the reference across calls that can re-enter a window procedure.
fn st() -> &'static mut State {
    unsafe { &mut *STATE }
}

fn s(v: i32) -> i32 {
    let d = st().dpi;
    mul_div(v, if d == 0 { 96 } else { d as i32 }, 96)
}

// ----------------------------------------------------------------------------
// Fonts
// ----------------------------------------------------------------------------
unsafe extern "system" fn font_enum_proc(_: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, p: LPARAM) -> i32 {
    *(p.0 as *mut bool) = true;
    0
}
unsafe fn font_installed(face: &str) -> bool {
    let dc = GetDC(None);
    let mut lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = c;
    }
    let mut found = false;
    EnumFontFamiliesExW(dc, &lf, Some(font_enum_proc), LPARAM(&mut found as *mut bool as isize), 0);
    ReleaseDC(None, dc);
    found
}
unsafe fn make_font(face: &str, px: i32, weight: i32) -> HFONT {
    CreateFontW(
        -px,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        &hs(face),
    )
}
unsafe fn create_fonts() {
    let stt = st();
    for f in [stt.f_search, stt.f_title, stt.f_small, stt.f_icon, stt.f_btn] {
        if !f.is_invalid() {
            let _ = DeleteObject(f.into());
        }
    }
    if stt.icon_face.is_none() {
        stt.icon_face = Some(if font_installed("Segoe Fluent Icons") {
            "Segoe Fluent Icons".into()
        } else if font_installed("Segoe MDL2 Assets") {
            "Segoe MDL2 Assets".into()
        } else {
            String::new()
        });
    }
    stt.f_search = make_font("Segoe UI", s(20), FW_NORMAL.0 as i32);
    stt.f_title = make_font("Segoe UI", s(14), FW_SEMIBOLD.0 as i32);
    stt.f_small = make_font("Segoe UI", s(13), FW_NORMAL.0 as i32);
    stt.f_btn = make_font("Segoe UI", s(15), FW_NORMAL.0 as i32);
    let face = stt.icon_face.clone().unwrap_or_default();
    stt.f_icon = make_font(if face.is_empty() { "Segoe UI Symbol" } else { &face }, s(17), FW_NORMAL.0 as i32);
    let dc = GetDC(None);
    let old = SelectObject(dc, stt.f_search.into());
    let mut tm = TEXTMETRICW::default();
    let _ = GetTextMetricsW(dc, &mut tm);
    stt.search_font_h = tm.tmHeight;
    SelectObject(dc, old);
    ReleaseDC(None, dc);
    if !stt.edit.is_invalid() {
        send(stt.edit, WM_SETFONT, stt.f_search.0 as usize, 1);
    }
    if !stt.btn.is_invalid() {
        send(stt.btn, WM_SETFONT, stt.f_btn.0 as usize, 1);
    }
}
fn has_icon_font() -> bool {
    st().icon_face.as_deref().map(|f| !f.is_empty()).unwrap_or(false)
}

// ----------------------------------------------------------------------------
// Window enumeration
// ----------------------------------------------------------------------------
unsafe fn window_icon(h: HWND) -> HICON {
    let mut r: usize = 0;
    for kind in [ICON_BIG, ICON_SMALL2, ICON_SMALL] {
        let ok = SendMessageTimeoutW(
            h,
            WM_GETICON,
            WPARAM(kind as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            60,
            Some(&mut r),
        );
        if ok.0 != 0 && r != 0 {
            break;
        }
        r = 0;
    }
    if r == 0 {
        r = GetClassLongPtrW(h, GCLP_HICON);
    }
    if r == 0 {
        r = GetClassLongPtrW(h, GCLP_HICONSM);
    }
    let src = if r != 0 { HICON(r as *mut c_void) } else { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
    // Own copy: the other process may destroy its icon at any time.
    CopyIcon(src).unwrap_or_default()
}

unsafe extern "system" fn enum_proc(h: HWND, _: LPARAM) -> BOOL {
    if !IsWindowVisible(h).as_bool() {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid == GetCurrentProcessId() {
        return true.into();
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let owner = GetWindow(h, GW_OWNER).unwrap_or_default();
    if !owner.is_invalid() && ex & WS_EX_APPWINDOW.0 == 0 {
        return true.into();
    }
    if ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return true.into();
    }
    let mut cloaked = 0i32;
    if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut i32 as *mut c_void, 4).is_ok() && cloaked != 0 {
        return true.into();
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(h, &mut cls);
    let cls = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
    if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return true.into();
    }
    let title = window_text(h);
    if title.is_empty() {
        return true.into();
    }
    st().items.push(Item {
        hwnd: h,
        lower: lower(&title),
        title,
        thumb: 0,
        src: SIZE::default(),
        thumb_ok: false,
        minimized: IsIconic(h).as_bool(),
        icon: window_icon(h),
    });
    true.into()
}

unsafe fn clear_items() {
    let stt = st();
    for it in stt.items.drain(..) {
        if it.thumb != 0 {
            let _ = DwmUnregisterThumbnail(it.thumb);
        }
        if !it.icon.is_invalid() {
            let _ = DestroyIcon(it.icon);
        }
    }
    stt.filtered.clear();
}

unsafe fn enumerate_windows() {
    clear_items();
    let _ = EnumWindows(Some(enum_proc), LPARAM(0)); // z-order: most recently used first
    let ov = st().overlay;
    for it in st().items.iter_mut() {
        if let Ok(t) = DwmRegisterThumbnail(ov, it.hwnd) {
            it.thumb = t;
            if let Ok(sz) = DwmQueryThumbnailSourceSize(t) {
                it.src = sz;
            }
            it.thumb_ok = it.src.cx >= 48 && it.src.cy >= 48;
        }
    }
}

// ----------------------------------------------------------------------------
// Layout
// ----------------------------------------------------------------------------
fn grid_h() -> i32 {
    st().rc_grid.bottom - st().rc_grid.top
}
fn content_h() -> i32 {
    let stt = st();
    let n = stt.filtered.len() as i32;
    if n == 0 {
        return 0;
    }
    let rows = (n as u32).div_ceil(stt.set.columns as u32) as i32;
    rows * stt.cell_h + (rows - 1) * stt.gap
}
fn max_scroll() -> i32 {
    (content_h() - grid_h()).max(0)
}

unsafe fn layout() {
    let stt = st();
    let mut rc = RECT::default();
    let _ = GetClientRect(stt.overlay, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let (p, sh, btn_w) = (s(18), s(48), s(168));
    stt.rc_search = rect(p, p, w - p - btn_w - s(10), p + sh);
    if !stt.btn.is_invalid() {
        let _ = MoveWindow(stt.btn, w - p - btn_w, p, btn_w, sh, true);
    }
    let (el, er) = (stt.rc_search.left + s(48), stt.rc_search.right - s(120));
    if !stt.edit.is_invalid() {
        let _ = MoveWindow(
            stt.edit,
            el,
            stt.rc_search.top + (sh - stt.search_font_h) / 2,
            (er - el).max(s(50)),
            stt.search_font_h,
            true,
        );
    }
    stt.rc_grid = rect(p, p + sh + p, w - p, h - p);
    stt.gap = s(14);
    stt.pad = s(10);
    stt.title_h = s(22);
    stt.title_gap = s(8);
    let cols = stt.set.columns;
    let gw = (stt.rc_grid.right - stt.rc_grid.left) - s(12); // room for the scroll indicator
    stt.cell_w = ((gw - (cols - 1) * stt.gap) / cols).max(s(60));
    let thumb_w = stt.cell_w - 2 * stt.pad;
    let thumb_h = thumb_w * 10 / 16;
    stt.cell_h = stt.pad + stt.title_h + stt.title_gap + thumb_h + stt.pad;
}

fn cell_rect(pos: i32) -> RECT {
    let stt = st();
    let cols = stt.set.columns;
    let (row, col) = (pos / cols, pos % cols);
    let l = stt.rc_grid.left + col * (stt.cell_w + stt.gap);
    let t = stt.rc_grid.top + row * (stt.cell_h + stt.gap) - stt.scroll;
    rect(l, t, l + stt.cell_w, t + stt.cell_h)
}
fn thumb_area(c: &RECT) -> RECT {
    let stt = st();
    rect(c.left + stt.pad, c.top + stt.pad + stt.title_h + stt.title_gap, c.right - stt.pad, c.bottom - stt.pad)
}

fn hit_test(x: i32, y: i32) -> i32 {
    let stt = st();
    let g = stt.rc_grid;
    if x < g.left || x >= g.right || y < g.top || y >= g.bottom {
        return -1;
    }
    let cols = stt.set.columns;
    let (lx, ly) = (x - g.left, y - g.top + stt.scroll);
    let (col, row) = (lx / (stt.cell_w + stt.gap), ly / (stt.cell_h + stt.gap));
    if col < 0 || col >= cols || row < 0 {
        return -1;
    }
    if lx - col * (stt.cell_w + stt.gap) >= stt.cell_w || ly - row * (stt.cell_h + stt.gap) >= stt.cell_h {
        return -1;
    }
    let pos = row * cols + col;
    if (pos as usize) < stt.filtered.len() {
        pos
    } else {
        -1
    }
}

/// Position the DWM thumbnails on their tiles, clipped to the grid area
/// (DWM thumbnails ignore GDI clipping, so source and destination are cut manually).
unsafe fn update_thumbs() {
    let stt = st();
    let mut pos_of = vec![-1i32; stt.items.len()];
    for (p, &i) in stt.filtered.iter().enumerate() {
        pos_of[i] = p as i32;
    }
    let grid = stt.rc_grid;
    for (i, it) in stt.items.iter().enumerate() {
        if it.thumb == 0 {
            continue;
        }
        let mut tp = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_VISIBLE, fVisible: false.into(), ..Default::default() };
        let p = pos_of[i];
        if p >= 0 && it.thumb_ok {
            let a = thumb_area(&cell_rect(p));
            let (aw, ah) = (a.right - a.left, a.bottom - a.top);
            let sc = (aw as f64 / it.src.cx as f64).min(ah as f64 / it.src.cy as f64).min(1.0);
            let dw = ((it.src.cx as f64 * sc).round() as i32).max(1);
            let dh = ((it.src.cy as f64 * sc).round() as i32).max(1);
            let d = rect(a.left + (aw - dw) / 2, a.top + (ah - dh) / 2, a.left + (aw - dw) / 2 + dw, a.top + (ah - dh) / 2 + dh);
            let mut v = RECT::default();
            if IntersectRect(&mut v, &d, &grid).as_bool() {
                tp.dwFlags = DWM_TNP_VISIBLE
                    | DWM_TNP_RECTDESTINATION
                    | DWM_TNP_RECTSOURCE
                    | DWM_TNP_OPACITY
                    | DWM_TNP_SOURCECLIENTAREAONLY;
                tp.fVisible = true.into();
                tp.opacity = 255;
                tp.fSourceClientAreaOnly = false.into();
                tp.rcDestination = v;
                tp.rcSource = rect(
                    mul_div(v.left - d.left, it.src.cx, dw),
                    mul_div(v.top - d.top, it.src.cy, dh),
                    mul_div(v.right - d.left, it.src.cx, dw),
                    mul_div(v.bottom - d.top, it.src.cy, dh),
                );
            }
        }
        let _ = DwmUpdateThumbnailProperties(it.thumb, &tp);
    }
}

unsafe fn apply_filter() {
    let edit = st().edit;
    if edit.is_invalid() {
        return;
    }
    let q = lower(&window_text(edit));
    let stt = st();
    stt.filtered = (0..stt.items.len()).filter(|&i| q.is_empty() || stt.items[i].lower.contains(&q)).collect();
    stt.scroll = 0;
    stt.hover = -1;
    update_thumbs();
    let _ = InvalidateRect(Some(st().overlay), None, false);
}

unsafe fn scroll_by(dy: i32) {
    let ns = (st().scroll + dy).clamp(0, max_scroll());
    if ns == st().scroll {
        return;
    }
    st().scroll = ns;
    update_thumbs();
    let _ = InvalidateRect(Some(st().overlay), None, false);
}

// ----------------------------------------------------------------------------
// Show / hide / activate
// ----------------------------------------------------------------------------
unsafe fn hide_overlay() {
    let ov = st().overlay;
    if ov.is_invalid() || !IsWindowVisible(ov).as_bool() {
        return;
    }
    let _ = ShowWindow(ov, SW_HIDE);
    clear_items();
    st().hover = -1;
    st().tracking_mouse = false;
}

unsafe fn show_overlay() {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut mi);
    let wa = mi.rcWork;
    let (mut dx, mut dy) = (96u32, 96u32);
    if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
        dx = 96;
    }
    if dx != st().dpi || st().f_search.is_invalid() {
        st().dpi = dx;
        create_fonts();
    }
    build_theme(st());

    let (wa_w, wa_h) = (wa.right - wa.left, wa.bottom - wa.top);
    let pct = st().set.size_percent;
    let (ww, wh) = (wa_w * pct / 100, wa_h * pct / 100);
    let mut r = rect(wa.left + (wa_w - ww) / 2, wa.top + (wa_h - wh) / 2, 0, 0);
    r.right = r.left + ww;
    r.bottom = r.top + wh;
    let mut m = mul_div(st().set.margin_mm * 10, st().dpi as i32, 254); // mm -> px
    m = m.min(ww.min(wh) / 2 - s(150));
    if m > 0 {
        let _ = InflateRect(&mut r, -m, -m);
    }

    let ov = st().overlay;
    let _ = SetWindowPos(ov, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
    layout();
    enumerate_windows();
    let _ = SetWindowTextW(st().edit, w!("")); // -> EN_CHANGE -> apply_filter
    apply_filter();
    let _ = ShowWindow(ov, SW_SHOW);
    let _ = SetForegroundWindow(ov);
    if GetForegroundWindow() != ov {
        // Without foreground rights the overlay would sit on top but receive no
        // input. Never force it (no AttachThreadInput, no injected keys) – close.
        hide_overlay();
        return;
    }
    let _ = SetFocus(Some(st().edit));
}

/// `order`: back to front; the last window ends up in front and gets the focus.
/// The overlay is hidden first, so nothing is reordered underneath a topmost
/// window. WinPick still holds the foreground right from the user's Enter/click.
unsafe fn activate_windows(order: Vec<HWND>) {
    let list: Vec<HWND> =
        order.into_iter().filter(|&h| IsWindow(Some(h)).as_bool() && !IsHungAppWindow(h).as_bool()).collect();
    hide_overlay();
    let Some(&front) = list.last() else { return };
    for &h in &list {
        if IsIconic(h).as_bool() {
            let _ = ShowWindowAsync(h, if h == front { SW_RESTORE } else { SW_SHOWNOACTIVATE });
        }
        if h != front {
            let _ = SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    let _ = SetForegroundWindow(front);
}

unsafe fn activate_filtered() {
    let stt = st();
    if stt.filtered.is_empty() {
        let _ = MessageBeep(MB_OK);
        return;
    }
    let query_empty = GetWindowTextLengthW(stt.edit) == 0;
    // Reverse display order: the first tile is raised last and ends up in front,
    // the relative order of the others is kept. Empty search box: first tile only.
    let order: Vec<HWND> = if query_empty {
        vec![stt.items[stt.filtered[0]].hwnd]
    } else {
        stt.filtered.iter().rev().map(|&i| stt.items[i].hwnd).collect()
    };
    activate_windows(order);
}

// ----------------------------------------------------------------------------
// Drawing (GDI+ for anti-aliased shapes, GDI for ClearType text)
// ----------------------------------------------------------------------------
fn argb(c: COLORREF) -> u32 {
    0xFF00_0000 | (cr_r(c) as u32) << 16 | (cr_g(c) as u32) << 8 | cr_b(c) as u32
}

struct Gfx(*mut gp::GpGraphics);
impl Gfx {
    unsafe fn new(dc: HDC) -> Gfx {
        let mut g = null_mut();
        gp::GdipCreateFromHDC(dc, &mut g);
        gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
        gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHalf);
        Gfx(g)
    }
    unsafe fn fill_round(&self, r: &RECT, rad: f32, c: COLORREF, border: Option<(COLORREF, f32)>) {
        let mut path = null_mut();
        gp::GdipCreatePath(gp::FillModeAlternate, &mut path);
        let (x, y) = (r.left as f32, r.top as f32);
        let (w, h) = ((r.right - r.left) as f32 - 1.0, (r.bottom - r.top) as f32 - 1.0);
        let d = (2.0 * rad).min(w.min(h));
        if d < 1.0 {
            gp::GdipAddPathRectangle(path, x, y, w, h);
        } else {
            gp::GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
            gp::GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
            gp::GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
            gp::GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            gp::GdipClosePathFigure(path);
        }
        let mut brush = null_mut();
        gp::GdipCreateSolidFill(argb(c), &mut brush);
        gp::GdipFillPath(self.0, brush as *mut gp::GpBrush, path);
        gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
        if let Some((bc, bw)) = border {
            if bw > 0.0 {
                let mut pen = null_mut();
                gp::GdipCreatePen1(argb(bc), bw, gp::UnitPixel, &mut pen);
                gp::GdipSetPenMode(pen, gp::PenAlignmentInset);
                gp::GdipDrawPath(self.0, pen, path);
                gp::GdipDeletePen(pen);
            }
        }
        gp::GdipDeletePath(path);
    }
    unsafe fn clip(&self, r: &RECT) {
        gp::GdipSetClipRectI(self.0, r.left, r.top, r.right - r.left, r.bottom - r.top, gp::CombineModeReplace);
    }
    unsafe fn reset_clip(&self) {
        gp::GdipResetClip(self.0);
    }
}
impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            gp::GdipDeleteGraphics(self.0);
        }
    }
}

unsafe fn text(dc: HDC, s: &str, mut r: RECT, f: HFONT, c: COLORREF, fmt: DRAW_TEXT_FORMAT) {
    let old = SelectObject(dc, f.into());
    SetTextColor(dc, c);
    SetBkMode(dc, TRANSPARENT);
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    DrawTextW(dc, &mut buf, &mut r, fmt | DT_NOPREFIX);
    SelectObject(dc, old);
}

unsafe fn paint_overlay(hdc: HDC) {
    let stt = st();
    let mut rc = RECT::default();
    let _ = GetClientRect(stt.overlay, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp.into());

    let bg = CreateSolidBrush(stt.th.bg);
    FillRect(mem, &rc, bg);
    let _ = DeleteObject(bg.into());

    let n = stt.filtered.len() as i32;
    let gh = grid_h();
    let grid = stt.rc_grid;
    let th = stt.th;

    // Pass 1: shapes
    {
        let g = Gfx::new(mem);
        g.fill_round(&stt.rc_search, s(10) as f32, th.search, Some((mix(th.search, th.accent, 0.55), s(1).max(1) as f32)));
        g.clip(&grid);
        for p in 0..n {
            let c = cell_rect(p);
            if c.bottom < grid.top || c.top > grid.bottom {
                continue;
            }
            let hov = p == stt.hover;
            g.fill_round(
                &c,
                s(10) as f32,
                if hov { th.tile_hover } else { th.tile },
                if hov { Some((th.accent, s(2) as f32)) } else { None },
            );
            g.fill_round(&thumb_area(&c), s(6) as f32, th.thumb_bg, None);
        }
        g.reset_clip();
        let ms = max_scroll();
        if ms > 0 {
            let content = content_h();
            let tb = s(36).max(gh * gh / content.max(1));
            let ty = grid.top + ((gh - tb) as i64 * stt.scroll as i64 / ms as i64) as i32;
            g.fill_round(&rect(grid.right - s(5), ty, grid.right, ty + tb), s(3) as f32, th.scroll, None);
        }
    }

    // Pass 2: text and icons
    if has_icon_font() {
        let ir = rect(stt.rc_search.left + s(14), stt.rc_search.top, stt.rc_search.left + s(44), stt.rc_search.bottom);
        text(mem, "\u{E721}", ir, stt.f_icon, th.text_dim, DT_SINGLELINE | DT_VCENTER | DT_LEFT);
    }
    let cr = rect(stt.rc_search.right - s(116), stt.rc_search.top, stt.rc_search.right - s(16), stt.rc_search.bottom);
    text(mem, &format!("{} / {}", n, stt.items.len()), cr, stt.f_small, th.text_dim, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);

    IntersectClipRect(mem, grid.left, grid.top, grid.right, grid.bottom);
    let (ico_s, ico_big) = (s(18), s(48));
    for p in 0..n {
        let c = cell_rect(p);
        if c.bottom < grid.top || c.top > grid.bottom {
            continue;
        }
        let it = &stt.items[stt.filtered[p as usize]];
        let iy = c.top + stt.pad + (stt.title_h - ico_s) / 2;
        if !it.icon.is_invalid() {
            let _ = DrawIconEx(mem, c.left + stt.pad, iy, it.icon, ico_s, ico_s, 0, None, DI_NORMAL);
        }
        let tr = rect(c.left + stt.pad + ico_s + s(8), c.top + stt.pad, c.right - stt.pad, c.top + stt.pad + stt.title_h);
        text(mem, &it.title, tr, stt.f_title, th.text, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_LEFT);
        if !it.thumb_ok {
            let a = thumb_area(&c);
            let (cx, cy) = ((a.left + a.right) / 2, (a.top + a.bottom) / 2);
            if !it.icon.is_invalid() {
                let _ = DrawIconEx(mem, cx - ico_big / 2, cy - ico_big / 2 - s(10), it.icon, ico_big, ico_big, 0, None, DI_NORMAL);
            }
            let lr = rect(a.left, cy + ico_big / 2 - s(4), a.right, cy + ico_big / 2 + s(22));
            let label = if it.minimized { "minimized" } else { "no preview" };
            text(mem, label, lr, stt.f_small, th.text_dim, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        }
    }
    SelectClipRgn(mem, None);

    if n == 0 {
        let msg = if stt.items.is_empty() { "No open windows" } else { "No window title contains this text" };
        text(mem, msg, grid, stt.f_btn, th.text_dim, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    }

    let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
}

unsafe fn draw_settings_button(di: &DRAWITEMSTRUCT) {
    let stt = st();
    let r = di.rcItem;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let mem = CreateCompatibleDC(Some(di.hDC));
    let bmp = CreateCompatibleBitmap(di.hDC, w, h);
    let ob = SelectObject(mem, bmp.into());
    let lr = rect(0, 0, w, h);
    let br = CreateSolidBrush(stt.th.bg);
    FillRect(mem, &lr, br);
    let _ = DeleteObject(br.into());
    let down = di.itemState.0 & ODS_SELECTED.0 != 0;
    let focus = di.itemState.0 & ODS_FOCUS.0 != 0;
    {
        let g = Gfx::new(mem);
        g.fill_round(
            &lr,
            s(10) as f32,
            if down { stt.th.button_down } else { stt.th.button },
            if focus { Some((stt.th.accent, s(1) as f32)) } else { None },
        );
    }
    let mut x = s(16);
    if has_icon_font() {
        text(mem, "\u{E713}", rect(x, 0, x + s(26), h), stt.f_icon, stt.th.text, DT_SINGLELINE | DT_VCENTER | DT_LEFT);
        x += s(28);
    }
    text(mem, "Settings", rect(x, 0, w - s(10), h), stt.f_btn, stt.th.text, DT_SINGLELINE | DT_VCENTER | DT_LEFT);
    let _ = BitBlt(di.hDC, r.left, r.top, w, h, Some(mem), 0, 0, SRCCOPY);
    SelectObject(mem, ob);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
}

// ----------------------------------------------------------------------------
// Tray, messages
// ----------------------------------------------------------------------------
unsafe fn message(owner: HWND, text: &str, flags: MESSAGEBOX_STYLE) {
    let owner = if owner.is_invalid() { None } else { Some(owner) };
    MessageBoxW(owner, &hs(text), &hs(APP_NAME), flags | MB_SETFOREGROUND);
}

unsafe fn update_tray(op: NOTIFY_ICON_MESSAGE) {
    let stt = st();
    stt.nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    stt.nid.hWnd = stt.overlay;
    stt.nid.uID = 1;
    stt.nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    stt.nid.uCallbackMessage = WM_APP_TRAY;
    if stt.nid.hIcon.is_invalid() {
        if let Ok(h) = LoadImageW(
            Some(stt.inst),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            IMAGE_FLAGS(0),
        ) {
            stt.nid.hIcon = HICON(h.0);
        }
    }
    let tip = format!(
        "{APP_NAME} – {}",
        if stt.hotkey_ok { hotkey_text(stt.set.mods, stt.set.vk) } else { "hotkey not active".into() }
    );
    stt.nid.szTip = [0; 128];
    for (i, c) in tip.encode_utf16().take(127).enumerate() {
        stt.nid.szTip[i] = c;
    }
    let _ = Shell_NotifyIconW(op, &stt.nid);
}

unsafe fn balloon(title: &str, body: &str) {
    let stt = st();
    let mut nid = stt.nid;
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    nid.szInfoTitle = [0; 64];
    for (i, c) in title.encode_utf16().take(63).enumerate() {
        nid.szInfoTitle[i] = c;
    }
    nid.szInfo = [0; 256];
    for (i, c) in body.encode_utf16().take(255).enumerate() {
        nid.szInfo[i] = c;
    }
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn tray_menu() {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let Ok(m) = CreatePopupMenu() else { return };
    let open = format!("Open overview\t{}", hotkey_text(st().set.mods, st().set.vk));
    let _ = AppendMenuW(m, MF_STRING, IDM_OPEN, &hs(&open));
    let _ = AppendMenuW(m, MF_STRING, IDM_SETTINGS, w!("Settings…"));
    let _ = AppendMenuW(m, MF_STRING, IDM_ABOUT, w!("About"));
    let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(m, MF_STRING, IDM_EXIT, w!("Exit"));
    let _ = SetMenuDefaultItem(m, IDM_OPEN as u32, 0);
    let ov = st().overlay;
    let _ = SetForegroundWindow(ov);
    let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, None, ov, None).0 as usize;
    let _ = PostMessageW(Some(ov), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(m);
    match cmd {
        IDM_OPEN => show_overlay(),
        IDM_SETTINGS => show_settings(),
        IDM_ABOUT => {
            let stt = st();
            let s = format!(
                "{APP_NAME} {APP_VERSION}\n\nWindow overview with title search.\n\nHotkey: {}\nSettings file:\n{}",
                hotkey_text(stt.set.mods, stt.set.vk),
                stt.ini_path
            );
            message(HWND::default(), &s, MB_ICONINFORMATION);
        }
        IDM_EXIT => {
            let _ = DestroyWindow(ov);
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------------
// Search box (subclass)
// ----------------------------------------------------------------------------
unsafe extern "system" fn edit_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_KEYDOWN => {
            let row = st().cell_h + st().gap;
            let page = row.max(grid_h() - row / 2);
            match VIRTUAL_KEY(wp.0 as u16) {
                VK_RETURN => {
                    activate_filtered();
                    return LRESULT(0);
                }
                VK_ESCAPE => {
                    hide_overlay();
                    return LRESULT(0);
                }
                VK_DOWN => {
                    scroll_by(row);
                    return LRESULT(0);
                }
                VK_UP => {
                    scroll_by(-row);
                    return LRESULT(0);
                }
                VK_NEXT => {
                    scroll_by(page);
                    return LRESULT(0);
                }
                VK_PRIOR => {
                    scroll_by(-page);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        WM_CHAR => {
            match wp.0 {
                0x0D | 0x1B => return LRESULT(0),
                1 => {
                    // Ctrl+A
                    send(h, EM_SETSEL, 0, -1);
                    return LRESULT(0);
                }
                0x7F => {
                    // Ctrl+Backspace: delete previous word
                    let (mut a, mut b) = (0u32, 0u32);
                    send(h, EM_GETSEL, &mut a as *mut u32 as usize, &mut b as *mut u32 as isize);
                    let t: Vec<u16> = window_text(h).encode_utf16().collect();
                    let mut k = (a as usize).min(t.len());
                    while k > 0 && t[k - 1] == b' ' as u16 {
                        k -= 1;
                    }
                    while k > 0 && t[k - 1] != b' ' as u16 {
                        k -= 1;
                    }
                    send(h, EM_SETSEL, k, b as isize);
                    let empty = [0u16];
                    send(h, EM_REPLACESEL, 1, empty.as_ptr() as isize);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        WM_MOUSEWHEEL => return SendMessageW(st().overlay, m, Some(wp), Some(lp)),
        _ => {}
    }
    let old: WNDPROC = std::mem::transmute(st().old_edit_proc);
    CallWindowProcW(old, h, m, wp, lp)
}

// ----------------------------------------------------------------------------
// Overlay window procedure
// ----------------------------------------------------------------------------
unsafe extern "system" fn overlay_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m != 0 && m == st().msg_taskbar_created {
        update_tray(NIM_ADD);
        return LRESULT(0);
    }
    match m {
        WM_CREATE => {
            let stt = st();
            stt.overlay = h;
            stt.edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                10,
                10,
                Some(h),
                Some(HMENU(IDC_SEARCH as usize as *mut c_void)),
                Some(stt.inst),
                None,
            )
            .unwrap_or_default();
            stt.old_edit_proc = SetWindowLongPtrW(stt.edit, GWLP_WNDPROC, edit_proc as *const () as isize);
            let cue = wide("Search window titles …  (Enter: bring to front, Esc: close)");
            send(stt.edit, EM_SETCUEBANNER, 1, cue.as_ptr() as isize);
            stt.btn = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("Settings"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_OWNERDRAW as u32),
                0,
                0,
                10,
                10,
                Some(h),
                Some(HMENU(IDC_BTN_SETTINGS as usize as *mut c_void)),
                Some(stt.inst),
                None,
            )
            .unwrap_or_default();
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const c_void,
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
            return LRESULT(0);
        }
        WM_HOTKEY => {
            if wp.0 as i32 == HOTKEY_ID {
                if IsWindowVisible(h).as_bool() {
                    hide_overlay();
                } else {
                    show_overlay();
                }
            }
            return LRESULT(0);
        }
        WM_APP_SHOW => {
            show_overlay();
            return LRESULT(0);
        }
        WM_APP_HIDE => {
            if GetForegroundWindow() != h {
                hide_overlay();
            }
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if loword(wp.0) == WA_INACTIVE && IsWindowVisible(h).as_bool() {
                let _ = PostMessageW(Some(h), WM_APP_HIDE, WPARAM(0), LPARAM(0));
            }
        }
        WM_APP_TRAY => {
            match loword(lp.0 as usize) {
                WM_LBUTTONUP => show_overlay(),
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(),
                _ => {}
            }
            return LRESULT(0);
        }
        WM_COMMAND => {
            let id = loword(wp.0) as i32;
            if id == IDC_SEARCH && hiword(wp.0) == EN_CHANGE {
                apply_filter();
                return LRESULT(0);
            }
            if id == IDC_BTN_SETTINGS {
                show_settings();
                return LRESULT(0);
            }
        }
        WM_CTLCOLOREDIT => {
            if HWND(lp.0 as *mut c_void) == st().edit {
                let dc = HDC(wp.0 as *mut c_void);
                SetTextColor(dc, st().th.text);
                SetBkColor(dc, st().th.search);
                return LRESULT(st().br_search.0 as isize);
            }
        }
        WM_DRAWITEM => {
            let di = &*(lp.0 as *const DRAWITEMSTRUCT);
            if di.CtlID as i32 == IDC_BTN_SETTINGS {
                draw_settings_button(di);
                return LRESULT(1);
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            paint_overlay(dc);
            let _ = EndPaint(h, &ps);
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = (hiword(wp.0) as u16 as i16) as i32;
            scroll_by(-mul_div(delta, (st().cell_h + st().gap) / 2, WHEEL_DELTA as i32));
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            if !st().tracking_mouse {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut t);
                st().tracking_mouse = true;
            }
            let p = hit_test(x_lparam(lp), y_lparam(lp));
            if p != st().hover {
                st().hover = p;
                let _ = InvalidateRect(Some(h), None, false);
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            st().tracking_mouse = false;
            if st().hover != -1 {
                st().hover = -1;
                let _ = InvalidateRect(Some(h), None, false);
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            if loword(lp.0 as usize) == HTCLIENT && HWND(wp.0 as *mut c_void) == h && st().hover >= 0 {
                if let Ok(c) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(c));
                }
                return LRESULT(1);
            }
        }
        WM_LBUTTONUP => {
            let p = hit_test(x_lparam(lp), y_lparam(lp));
            if p >= 0 {
                let hw = st().items[st().filtered[p as usize]].hwnd;
                activate_windows(vec![hw]);
            } else {
                let _ = SetFocus(Some(st().edit));
            }
            return LRESULT(0);
        }
        WM_MBUTTONUP => {
            let _ = SetFocus(Some(st().edit));
            return LRESULT(0);
        }
        WM_DPICHANGED => return LRESULT(0), // position/size are set in show_overlay()
        WM_DESTROY => {
            clear_items();
            let _ = UnregisterHotKey(Some(h), HOTKEY_ID);
            let _ = Shell_NotifyIconW(NIM_DELETE, &st().nid);
            if !st().settings.is_invalid() {
                let _ = DestroyWindow(st().settings);
            }
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(h, m, wp, lp)
}

// ----------------------------------------------------------------------------
// Settings dialog
// ----------------------------------------------------------------------------
unsafe fn set_swatch(c: COLORREF) {
    let stt = st();
    stt.tmp_color = c;
    if !stt.br_swatch.is_invalid() {
        let _ = DeleteObject(stt.br_swatch.into());
    }
    stt.br_swatch = CreateSolidBrush(c);
    if !stt.swatch.is_invalid() {
        let _ = InvalidateRect(Some(stt.swatch), None, true);
    }
}

unsafe fn checked(d: HWND, id: i32) -> bool {
    IsDlgButtonChecked(d, id) == BST_CHECKED.0
}

unsafe fn apply_settings_dialog() {
    let d = st().settings;
    let mut ns = st().set;
    ns.mods = 0;
    if checked(d, IDS_CTRL) {
        ns.mods |= MOD_CONTROL.0;
    }
    if checked(d, IDS_ALT) {
        ns.mods |= MOD_ALT.0;
    }
    if checked(d, IDS_SHIFT) {
        ns.mods |= MOD_SHIFT.0;
    }
    if checked(d, IDS_WIN) {
        ns.mods |= MOD_WIN.0;
    }
    if ns.mods == 0 {
        message(d, "Please select at least one modifier (Ctrl, Alt, Shift or Win).", MB_ICONWARNING);
        return;
    }
    let key_cb = GetDlgItem(Some(d), IDS_KEY).unwrap_or_default();
    let ks = send(key_cb, CB_GETCURSEL, 0, 0);
    if ks < 0 {
        message(d, "Please select a key.", MB_ICONWARNING);
        return;
    }
    ns.vk = send(key_cb, CB_GETITEMDATA, ks as usize, 0) as u32;
    let cs = send(GetDlgItem(Some(d), IDS_COLS).unwrap_or_default(), CB_GETCURSEL, 0, 0);
    ns.columns = if cs < 0 { st().set.columns } else { cs as i32 + 1 };
    let (mut ok1, mut ok2) = (BOOL(0), BOOL(0));
    let pct = GetDlgItemInt(d, IDS_PERCENT, Some(&mut ok1), false) as i32;
    let mar = GetDlgItemInt(d, IDS_MARGIN, Some(&mut ok2), false) as i32;
    if !ok1.as_bool() || !(30..=100).contains(&pct) {
        message(d, "Please enter a window size between 30 and 100 %.", MB_ICONWARNING);
        return;
    }
    if !ok2.as_bool() || !(0..=200).contains(&mar) {
        message(d, "Please enter a margin between 0 and 200 mm.", MB_ICONWARNING);
        return;
    }
    ns.size_percent = pct;
    ns.margin_mm = mar;
    ns.bg = st().tmp_color;

    let ov = st().overlay;
    let cur = st().set;
    if ns.mods != cur.mods || ns.vk != cur.vk || !st().hotkey_ok {
        if st().hotkey_ok {
            let _ = UnregisterHotKey(Some(ov), HOTKEY_ID);
        }
        if RegisterHotKey(Some(ov), HOTKEY_ID, HOT_KEY_MODIFIERS(ns.mods | MOD_NOREPEAT.0), ns.vk).is_ok() {
            st().hotkey_ok = true;
        } else {
            st().hotkey_ok = st().hotkey_ok
                && RegisterHotKey(Some(ov), HOTKEY_ID, HOT_KEY_MODIFIERS(cur.mods | MOD_NOREPEAT.0), cur.vk).is_ok();
            let msg = format!(
                "The hotkey {} is already used by Windows or another program.\n\nPlease choose a different one.",
                hotkey_text(ns.mods, ns.vk)
            );
            message(d, &msg, MB_ICONWARNING);
            return;
        }
    }
    st().set = ns;
    save_settings(st());
    set_autostart(checked(d, IDS_AUTOSTART));
    update_tray(NIM_MODIFY);
    let _ = DestroyWindow(d);
}

unsafe extern "system" fn settings_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_COMMAND => match loword(wp.0) as i32 {
            x if x == IDOK.0 => {
                apply_settings_dialog();
                return LRESULT(0);
            }
            x if x == IDCANCEL.0 => {
                let _ = DestroyWindow(h);
                return LRESULT(0);
            }
            IDS_COLORBTN => {
                static mut CUSTOM: [COLORREF; 16] = [COLORREF(0); 16];
                let custom = &mut *std::ptr::addr_of_mut!(CUSTOM);
                if custom[0].0 == 0 {
                    let presets = [
                        rgb(28, 30, 36),
                        rgb(18, 18, 20),
                        rgb(32, 40, 56),
                        rgb(40, 28, 48),
                        rgb(24, 44, 36),
                        rgb(60, 60, 64),
                        rgb(236, 238, 242),
                        rgb(250, 250, 250),
                    ];
                    custom[..8].copy_from_slice(&presets);
                }
                let mut cc = CHOOSECOLORW {
                    lStructSize: size_of::<CHOOSECOLORW>() as u32,
                    hwndOwner: h,
                    rgbResult: st().tmp_color,
                    lpCustColors: custom.as_mut_ptr(),
                    Flags: CC_RGBINIT | CC_FULLOPEN,
                    ..Default::default()
                };
                if ChooseColorW(&mut cc).as_bool() {
                    set_swatch(cc.rgbResult);
                }
                return LRESULT(0);
            }
            _ => {}
        },
        WM_CTLCOLORSTATIC => {
            if HWND(lp.0 as *mut c_void) == st().swatch {
                return LRESULT(st().br_swatch.0 as isize);
            }
            let dc = HDC(wp.0 as *mut c_void);
            SetBkColor(dc, COLORREF(GetSysColor(COLOR_BTNFACE)));
            SetTextColor(dc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
            return LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize);
        }
        WM_CLOSE => {
            let _ = DestroyWindow(h);
            return LRESULT(0);
        }
        WM_DESTROY => {
            let stt = st();
            stt.settings = HWND::default();
            stt.swatch = HWND::default();
            if !stt.set_font.is_invalid() {
                let _ = DeleteObject(stt.set_font.into());
                stt.set_font = HFONT::default();
            }
            if !stt.br_swatch.is_invalid() {
                let _ = DeleteObject(stt.br_swatch.into());
                stt.br_swatch = HBRUSH::default();
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(h, m, wp, lp)
}

unsafe fn show_settings() {
    if !st().settings.is_invalid() {
        let _ = ShowWindow(st().settings, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(st().settings);
        return;
    }
    hide_overlay();

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut mi);
    let (mut dpi, mut dy) = (96u32, 96u32);
    if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy).is_err() {
        dpi = 96;
    }
    let sc = |v: i32| mul_div(v, dpi as i32, 96);

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let mut rc = rect(0, 0, sc(400), sc(330));
    let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
    let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
    let wa = mi.rcWork;
    let (x, y) = (wa.left + (wa.right - wa.left - ww) / 2, wa.top + (wa.bottom - wa.top - wh) / 2);

    let inst = st().inst;
    let Ok(dlg) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        SETTINGS_CLASS,
        &hs(&format!("{APP_NAME} – Settings")),
        style,
        x,
        y,
        ww,
        wh,
        None,
        None,
        Some(inst),
        None,
    ) else {
        return;
    };
    st().settings = dlg;

    let mut ncm = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    let _ = SystemParametersInfoForDpi(
        SPI_GETNONCLIENTMETRICS.0,
        ncm.cbSize,
        Some(&mut ncm as *mut _ as *mut c_void),
        0,
        dpi,
    );
    st().set_font = CreateFontIndirectW(&ncm.lfMessageFont);
    let font = st().set_font;

    let mk = |cls: PCWSTR, txt: &str, st_: u32, x0: i32, y0: i32, w0: i32, h0: i32, id: i32, ex: u32| -> HWND {
        let c = CreateWindowExW(
            WINDOW_EX_STYLE(ex),
            cls,
            &hs(txt),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(st_),
            sc(x0),
            sc(y0),
            sc(w0),
            sc(h0),
            Some(dlg),
            Some(HMENU(id as isize as *mut c_void)),
            Some(inst),
            None,
        )
        .unwrap_or_default();
        send(c, WM_SETFONT, font.0 as usize, 1);
        c
    };
    let tab = WS_TABSTOP.0;
    let cb = BS_AUTOCHECKBOX as u32 | tab;
    mk(w!("BUTTON"), "Hotkey", BS_GROUPBOX as u32, 12, 8, 376, 88, -1, 0);
    mk(w!("BUTTON"), "Ctrl", cb | WS_GROUP.0, 26, 32, 60, 20, IDS_CTRL, 0);
    mk(w!("BUTTON"), "Alt", cb, 92, 32, 52, 20, IDS_ALT, 0);
    mk(w!("BUTTON"), "Shift", cb, 150, 32, 70, 20, IDS_SHIFT, 0);
    mk(w!("BUTTON"), "Win", cb, 226, 32, 60, 20, IDS_WIN, 0);
    mk(w!("STATIC"), "Key:", SS_LEFT, 26, 64, 60, 20, -1, 0);
    let combo = CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0 | tab;
    let key_cb = mk(w!("COMBOBOX"), "", combo, 92, 60, 140, 300, IDS_KEY, 0);
    mk(w!("STATIC"), "Grid columns:", SS_LEFT, 12, 112, 200, 20, -1, 0);
    let col_cb = mk(w!("COMBOBOX"), "", combo, 230, 108, 80, 300, IDS_COLS, 0);
    let num = ES_NUMBER as u32 | ES_AUTOHSCROLL as u32 | tab;
    mk(w!("STATIC"), "Window size (% of screen):", SS_LEFT, 12, 146, 215, 20, -1, 0);
    mk(w!("EDIT"), "", num, 230, 142, 80, 24, IDS_PERCENT, WS_EX_CLIENTEDGE.0);
    mk(w!("STATIC"), "Additional margin (mm):", SS_LEFT, 12, 180, 215, 20, -1, 0);
    mk(w!("EDIT"), "", num, 230, 176, 80, 24, IDS_MARGIN, WS_EX_CLIENTEDGE.0);
    mk(w!("STATIC"), "Background color:", SS_LEFT, 12, 214, 215, 20, -1, 0);
    let sw = mk(w!("STATIC"), "", SS_SUNKEN, 230, 210, 40, 24, IDS_SWATCH, 0);
    st().swatch = sw;
    mk(w!("BUTTON"), "Change …", BS_PUSHBUTTON as u32 | tab, 278, 209, 92, 26, IDS_COLORBTN, 0);
    mk(w!("BUTTON"), "Start with Windows", cb, 12, 250, 300, 20, IDS_AUTOSTART, 0);
    mk(w!("BUTTON"), "OK", BS_DEFPUSHBUTTON as u32 | tab, 212, 290, 84, 28, IDOK.0, 0);
    mk(w!("BUTTON"), "Cancel", BS_PUSHBUTTON as u32 | tab, 304, 290, 84, 28, IDCANCEL.0, 0);

    let set = st().set;
    let chk = |id: i32, on: bool| {
        let _ = CheckDlgButton(dlg, id, if on { BST_CHECKED } else { BST_UNCHECKED });
    };
    chk(IDS_CTRL, set.mods & MOD_CONTROL.0 != 0);
    chk(IDS_ALT, set.mods & MOD_ALT.0 != 0);
    chk(IDS_SHIFT, set.mods & MOD_SHIFT.0 != 0);
    chk(IDS_WIN, set.mods & MOD_WIN.0 != 0);

    let mut keys: Vec<u32> = (0x41..=0x5A).chain(0x30..=0x39).chain(VK_F1.0 as u32..=VK_F12.0 as u32).collect();
    keys.extend([VK_SPACE.0 as u32, VK_OEM_PERIOD.0 as u32, VK_OEM_COMMA.0 as u32, VK_OEM_MINUS.0 as u32]);
    if !keys.contains(&set.vk) {
        keys.push(set.vk);
    }
    for k in keys {
        let name = wide(&key_name(k));
        let idx = send(key_cb, CB_ADDSTRING, 0, name.as_ptr() as isize);
        send(key_cb, CB_SETITEMDATA, idx as usize, k as isize);
        if k == set.vk {
            send(key_cb, CB_SETCURSEL, idx as usize, 0);
        }
    }
    for c in 1..=10 {
        let t = wide(&c.to_string());
        send(col_cb, CB_ADDSTRING, 0, t.as_ptr() as isize);
    }
    send(col_cb, CB_SETCURSEL, (set.columns - 1) as usize, 0);
    let _ = SetDlgItemInt(dlg, IDS_PERCENT, set.size_percent as u32, false);
    let _ = SetDlgItemInt(dlg, IDS_MARGIN, set.margin_mm as u32, false);
    set_swatch(set.bg);
    chk(IDS_AUTOSTART, get_autostart().is_some());

    let _ = ShowWindow(dlg, SW_SHOW);
    let _ = SetForegroundWindow(dlg);
    let _ = SetFocus(Some(key_cb));
}

// ----------------------------------------------------------------------------
// Entry point
// ----------------------------------------------------------------------------
fn main() {
    unsafe { run() }
}

unsafe fn run() {
    let inst: HINSTANCE = GetModuleHandleW(None).map(|m| m.into()).unwrap_or_default();

    let mutex = CreateMutexW(None, true, w!("Local\\WinPick_SingleInstance_7F3A"));
    if GetLastError() == ERROR_ALREADY_EXISTS {
        // Already running: the running instance shows the overlay.
        if let Ok(other) = FindWindowW(OVERLAY_CLASS, PCWSTR::null()) {
            let mut pid = 0u32;
            GetWindowThreadProcessId(other, Some(&mut pid));
            let _ = AllowSetForegroundWindow(pid);
            let _ = PostMessageW(Some(other), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
        return;
    }

    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let icc = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_WIN95_CLASSES,
    };
    let _ = InitCommonControlsEx(&icc);
    let mut gdip_token = 0usize;
    let gsi = gp::GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
    gp::GdiplusStartup(&mut gdip_token, &gsi, null_mut());

    let state = Box::new(State {
        inst,
        set: Settings::default(),
        ini_path: String::new(),
        th: Theme::default(),
        br_search: HBRUSH::default(),
        overlay: HWND::default(),
        edit: HWND::default(),
        btn: HWND::default(),
        settings: HWND::default(),
        old_edit_proc: 0,
        items: Vec::new(),
        filtered: Vec::new(),
        scroll: 0,
        hover: -1,
        tracking_mouse: false,
        dpi: 0,
        f_search: HFONT::default(),
        f_title: HFONT::default(),
        f_small: HFONT::default(),
        f_icon: HFONT::default(),
        f_btn: HFONT::default(),
        search_font_h: 20,
        icon_face: None,
        rc_search: RECT::default(),
        rc_grid: RECT::default(),
        cell_w: 200,
        cell_h: 160,
        gap: 14,
        pad: 10,
        title_h: 22,
        title_gap: 8,
        nid: NOTIFYICONDATAW::default(),
        msg_taskbar_created: 0,
        hotkey_ok: false,
        set_font: HFONT::default(),
        br_swatch: HBRUSH::default(),
        tmp_color: COLORREF(0),
        swatch: HWND::default(),
    });
    STATE = Box::into_raw(state);

    load_settings(st());
    build_theme(st());

    // Keep the autostart entry in sync if the portable EXE was moved.
    if let Some(cur) = get_autostart() {
        if cur != run_value() {
            set_autostart(true);
        }
    }

    let big_icon = LoadImageW(Some(inst), PCWSTR(std::ptr::without_provenance(1)), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE)
        .map(|h| HICON(h.0))
        .unwrap_or_default();
    let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();

    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(overlay_proc),
        hInstance: inst,
        hCursor: cursor,
        hIcon: big_icon,
        lpszClassName: OVERLAY_CLASS,
        ..Default::default()
    };
    RegisterClassExW(&wc);
    let sc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(settings_proc),
        hInstance: inst,
        hCursor: cursor,
        hIcon: big_icon,
        hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
        lpszClassName: SETTINGS_CLASS,
        ..Default::default()
    };
    RegisterClassExW(&sc);

    st().msg_taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));

    let Ok(ov) = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        OVERLAY_CLASS,
        &hs(APP_NAME),
        WS_POPUP | WS_CLIPCHILDREN,
        0,
        0,
        100,
        100,
        None,
        None,
        Some(inst),
        None,
    ) else {
        return;
    };
    let _ = ChangeWindowMessageFilterEx(ov, st().msg_taskbar_created, MSGFLT_ALLOW, None);

    let set = st().set;
    st().hotkey_ok = RegisterHotKey(Some(ov), HOTKEY_ID, HOT_KEY_MODIFIERS(set.mods | MOD_NOREPEAT.0), set.vk).is_ok();
    update_tray(NIM_ADD);
    if st().hotkey_ok && !std::env::args().any(|a| a == "--autostart") {
        // Tell the user which hotkey is active (it may differ from the default).
        let txt = format!("Press {} to open the window overview.", hotkey_text(set.mods, set.vk));
        balloon("WinPick is running", &txt);
    }
    if !st().hotkey_ok {
        let msg = format!(
            "The hotkey {} is already in use.\n\nPlease choose a different one in the settings.",
            hotkey_text(set.mods, set.vk)
        );
        message(HWND::default(), &msg, MB_ICONWARNING);
        show_settings();
    }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        let dlg = st().settings;
        if !dlg.is_invalid() && IsDialogMessageW(dlg, &msg).as_bool() {
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    gp::GdiplusShutdown(gdip_token);
    if let Ok(m) = mutex {
        let _ = ReleaseMutex(m);
        let _ = CloseHandle(m);
    }
}
