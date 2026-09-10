//! Windows shell integration. All HWND state and tray resources live on one
//! message thread; slow shell icon extraction happens on a separate worker.
use eframe::egui;
use opencast::shortcut::Shortcut;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};
const ACTIVATE: u32 = WM_APP + 1;
const CHANGE_KEY: u32 = WM_APP + 2;
const EXIT: u32 = WM_APP + 3;
const TRAY: u32 = WM_APP + 4;
const CLASS: &str = "OpenCast.Resident.v2";
const RECORDING: u32 = WM_APP + 5;
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
/// A second invocation only activates the first process; it never creates an indexer.
pub fn acquire_instance(quit: bool, background: bool) -> Result<Option<Instance>, String> {
    unsafe {
        let mutex = CreateMutexW(
            std::ptr::null(),
            0,
            wide("Local\\OpenCast.Resident.v2").as_ptr(),
        );
        if mutex.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            if background && !quit {
                return Ok(None);
            }
            for _ in 0..100 {
                let hwnd = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
                if !hwnd.is_null() {
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, &mut pid);
                    AllowSetForegroundWindow(pid);
                    PostMessageW(hwnd, if quit { EXIT } else { ACTIVATE }, 0, 0);
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            return Err("OpenCast is still starting. Try opening it again in a moment.".into());
        }
        if quit {
            CloseHandle(mutex);
            return Ok(None);
        }
        Ok(Some(Instance(mutex)))
    }
}
#[derive(Debug)]
pub enum Event {
    Shown,
    Settings,
    Quit,
}
struct State {
    app: HWND,
    ctx: egui::Context,
    events: mpsc::Sender<Event>,
    hotkey_id: i32,
    hotkey: Shortcut,
    tray: NOTIFYICONDATAW,
    icon: HICON,
    taskbar_message: u32,
}
impl State {
    fn show(&self, settings: bool) {
        unsafe {
            ShowWindow(self.app, SW_SHOW);
            SetForegroundWindow(self.app);
        }
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let _ = self.events.send(if settings {
            Event::Settings
        } else {
            Event::Shown
        });
        self.ctx.request_repaint();
    }
    fn hide(&self) {
        unsafe {
            ShowWindow(self.app, SW_HIDE);
        }
    }
    fn change_key(&mut self, hwnd: HWND, choice: Shortcut) -> bool {
        if !choice.valid() {
            return false;
        }
        let modifiers = choice.modifiers | MOD_NOREPEAT;
        let next_id = if self.hotkey_id == 1 { 2 } else { 1 };
        // Register first. If occupied, the previously working shortcut survives.
        unsafe {
            if self.hotkey_id != 0 && choice == self.hotkey {
                return true;
            }
            if RegisterHotKey(hwnd, next_id, modifiers, choice.key) == 0 {
                return false;
            }
            if self.hotkey_id != 0 {
                UnregisterHotKey(hwnd, self.hotkey_id);
            }
        }
        self.hotkey_id = next_id;
        self.hotkey = choice;
        true
    }
}
pub struct Resident {
    hwnd: usize,
    app: usize,
    pub events: mpsc::Receiver<Event>,
    pub shortcut_error: Option<String>,
}
impl Resident {
    pub fn new(cc: &eframe::CreationContext<'_>, shortcut: Shortcut) -> Result<Self, String> {
        let RawWindowHandle::Win32(handle) =
            cc.window_handle().map_err(|e| e.to_string())?.as_raw()
        else {
            return Err("No Windows window handle".into());
        };
        let app = handle.hwnd.get() as usize;
        let ctx = cc.egui_ctx.clone();
        let (send, events) = mpsc::channel();
        let (ready, wait) = mpsc::sync_channel(1);
        std::thread::spawn(move || unsafe {
            let class = wide(CLASS);
            let module = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
                return;
            }
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                module,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
                return;
            }
            let icon = LoadImageW(
                module,
                std::ptr::without_provenance::<u16>(1),
                IMAGE_ICON,
                32,
                32,
                LR_DEFAULTCOLOR,
            ) as HICON;
            let mut tray: NOTIFYICONDATAW = std::mem::zeroed();
            tray.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            tray.hWnd = hwnd;
            tray.uID = 1;
            tray.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
            tray.uCallbackMessage = TRAY;
            tray.hIcon = icon;
            for (out, input) in tray
                .szTip
                .iter_mut()
                .zip(wide("OpenCast — click to search"))
            {
                *out = input;
            }
            let mut state = Box::new(State {
                app: app as HWND,
                ctx,
                events: send,
                hotkey_id: 0,
                hotkey: shortcut,
                tray,
                icon,
                taskbar_message: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            });
            let shortcut_error = if state.change_key(hwnd, shortcut) {
                None
            } else {
                Some(format!(
                    "{} is already in use. Choose another shortcut in Settings.",
                    shortcut.label()
                ))
            };
            if Shell_NotifyIconW(NIM_ADD, &state.tray) == 0 {
                if state.hotkey_id != 0 {
                    UnregisterHotKey(hwnd, state.hotkey_id);
                }
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                DestroyWindow(hwnd);
                let _ = ready.send(Err("Could not create the OpenCast tray icon.".into()));
                return;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut State) as isize);
            let _ = ready.send(Ok((hwnd as usize, shortcut_error)));
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // State outlives all callbacks and owns the icon until WM_DESTROY.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        });
        let (hwnd, shortcut_error) = wait.recv().map_err(|e| e.to_string())??;
        Ok(Self {
            hwnd,
            app,
            events,
            shortcut_error,
        })
    }
    pub fn show_settings(&self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, ACTIVATE, 1, 0);
        }
    }
    pub fn recording(&self, active: bool) -> Result<(), String> {
        if unsafe { SendMessageW(self.hwnd as HWND, RECORDING, active as usize, 0) } == 0 {
            Err("Your previous shortcut became unavailable. Please record another.".into())
        } else {
            Ok(())
        }
    }
    pub fn hide(&self) {
        unsafe {
            ShowWindow(self.app as HWND, SW_HIDE);
        }
    }
    pub fn set_hotkey(&self, choice: Shortcut) -> Result<(), String> {
        if unsafe {
            SendMessageW(
                self.hwnd as HWND,
                CHANGE_KEY,
                ((choice.modifiers << 16) | choice.key) as usize,
                0,
            )
        } == 0
        {
            Err("That shortcut is already in use. Your previous shortcut is still active.".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Resident {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // The pointer is installed after CreateWindowEx and remains owned by the
    // message-loop stack until DestroyWindow has finished dispatching callbacks.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if pointer.is_null() {
            return DefWindowProcW(hwnd, msg, w, l);
        }
        let state = &*pointer;
        match msg {
            ACTIVATE => {
                state.show(w == 1);
                0
            }
            CHANGE_KEY => (&mut *pointer).change_key(
                hwnd,
                Shortcut {
                    modifiers: (w as u32) >> 16,
                    key: (w as u32) & 0xffff,
                },
            ) as LRESULT,
            RECORDING => {
                let state = &mut *pointer;
                if w == 1 {
                    if state.hotkey_id != 0 {
                        UnregisterHotKey(hwnd, state.hotkey_id);
                        state.hotkey_id = 0;
                    }
                    1
                } else {
                    state.change_key(hwnd, state.hotkey) as LRESULT
                }
            }
            EXIT => {
                state.show(false);
                let _ = state.events.send(Event::Quit);
                state.ctx.request_repaint();
                0
            }
            WM_HOTKEY => {
                if IsWindowVisible(state.app) != 0 && GetForegroundWindow() == state.app {
                    state.hide();
                } else {
                    state.show(false);
                }
                0
            }
            TRAY => {
                match l as u32 {
                    WM_LBUTTONUP => state.show(false),
                    WM_RBUTTONUP => {
                        let menu = CreatePopupMenu();
                        AppendMenuW(menu, MF_STRING, 1, wide("Open OpenCast").as_ptr());
                        AppendMenuW(menu, MF_STRING, 2, wide("Settings").as_ptr());
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(menu, MF_STRING, 3, wide("Quit OpenCast").as_ptr());
                        let mut pos: POINT = std::mem::zeroed();
                        GetCursorPos(&mut pos);
                        SetForegroundWindow(hwnd);
                        let choice = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_NONOTIFY,
                            pos.x,
                            pos.y,
                            0,
                            hwnd,
                            std::ptr::null(),
                        );
                        DestroyMenu(menu);
                        match choice {
                            1 => state.show(false),
                            2 => state.show(true),
                            3 => {
                                PostMessageW(hwnd, EXIT, 0, 0);
                            }
                            _ => (),
                        }
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                    }
                    _ => (),
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                Shell_NotifyIconW(NIM_DELETE, &state.tray);
                if state.hotkey_id != 0 {
                    UnregisterHotKey(hwnd, state.hotkey_id);
                }
                if !state.icon.is_null() {
                    DestroyIcon(state.icon);
                }
                PostQuitMessage(0);
                0
            }
            message if message != 0 && message == state.taskbar_message => {
                Shell_NotifyIconW(NIM_ADD, &state.tray);
                0
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}

pub fn app_roots() -> Vec<PathBuf> {
    ["APPDATA", "PROGRAMDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("Microsoft/Windows/Start Menu/Programs"))
        .filter(|p| p.is_dir())
        .collect()
}
pub struct Icons {
    requests: mpsc::SyncSender<PathBuf>,
    results: mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>,
    cache: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
}
impl Icons {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, rx) = mpsc::sync_channel::<PathBuf>(64);
        let (tx, results) = mpsc::channel();
        std::thread::spawn(move || unsafe {
            let initialized =
                CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) >= 0;
            while let Ok(path) = rx.recv() {
                let image = shell_icon(&path);
                if tx.send((path, image)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            if initialized {
                CoUninitialize();
            }
        });
        Self {
            requests,
            results,
            cache: Default::default(),
        }
    }
    pub fn get(&mut self, path: &Path, ctx: &egui::Context) -> Option<egui::TextureId> {
        while let Ok((path, image)) = self.results.try_recv() {
            // Ignore results evicted while their shell request was pending.
            if self.cache.contains_key(&path) {
                let texture = image.map(|image| {
                    ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR)
                });
                self.cache.insert(path, texture);
            }
        }
        if !self.cache.contains_key(path) {
            if self.cache.len() >= 512 {
                self.cache.clear();
            }
            if self.requests.try_send(path.to_owned()).is_ok() {
                self.cache.insert(path.to_owned(), None);
            }
        }
        self.cache
            .get(path)
            .and_then(|t| t.as_ref())
            .map(|t| t.id())
    }
}
/// Ask the Shell to resolve executable/shortcut/associated-file icons. Rendering
/// over both black and white recovers alpha for legacy AND-mask icons as well.
fn shell_icon(path: &Path) -> Option<egui::ColorImage> {
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        if SHGetFileInfoW(
            wide(path).as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) == 0
            || info.hIcon.is_null()
        {
            return None;
        }
        let black = render_icon(info.hIcon, 0);
        let white = render_icon(info.hIcon, 255);
        DestroyIcon(info.hIcon);
        let (black, white) = (black?, white?);
        let mut rgba = Vec::with_capacity(32 * 32 * 4);
        for (b, w) in black
            .as_chunks::<4>()
            .0
            .iter()
            .zip(white.as_chunks::<4>().0.iter())
        {
            let alpha = 255u16 - u16::from(w[0].saturating_sub(b[0]));
            for channel in [2, 1, 0] {
                rgba.push(
                    (u16::from(b[channel]) * 255)
                        .checked_div(alpha)
                        .unwrap_or(0)
                        .min(255) as u8,
                );
            }
            rgba.push(alpha as u8);
        }
        Some(egui::ColorImage::from_rgba_unmultiplied([32, 32], &rgba))
    }
}
unsafe fn render_icon(icon: HICON, background: u8) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = 32;
        info.bmiHeader.biHeight = -32;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        let buffer = std::slice::from_raw_parts_mut(bits as *mut u8, 32 * 32 * 4);
        buffer.fill(background);
        let drawn = DrawIconEx(dc, 0, 0, icon, 32, 32, 0, std::ptr::null_mut(), DI_NORMAL) != 0;
        GdiFlush();
        let result = drawn.then(|| buffer.to_vec());
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        result
    }
}
pub fn logo_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_LWIN as i32) < 0 || GetAsyncKeyState(VK_RWIN as i32) < 0 }
}

pub fn reveal(cc: &eframe::CreationContext<'_>) {
    if let Ok(handle) = cc.window_handle()
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        unsafe {
            ShowWindow(handle.hwnd.get() as HWND, SW_SHOW);
        }
    }
    cc.egui_ctx
        .send_viewport_cmd(egui::ViewportCommand::Visible(true));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occupied_shortcut_preserves_previous_registration() {
        let original = Shortcut {
            modifiers: 7,
            key: 134,
        };
        let occupied = Shortcut {
            modifiers: 7,
            key: 135,
        };
        let (ready, wait) = mpsc::sync_channel(1);
        let (release, done) = mpsc::sync_channel(1);
        let blocker = std::thread::spawn(move || unsafe {
            assert_ne!(
                RegisterHotKey(
                    std::ptr::null_mut(),
                    99,
                    occupied.modifiers | MOD_NOREPEAT,
                    occupied.key
                ),
                0
            );
            ready.send(()).unwrap();
            done.recv().unwrap();
            UnregisterHotKey(std::ptr::null_mut(), 99);
        });
        wait.recv().unwrap();
        let (events, _) = mpsc::channel();
        let mut state = State {
            app: std::ptr::null_mut(),
            ctx: egui::Context::default(),
            events,
            hotkey_id: 0,
            hotkey: original,
            tray: unsafe { std::mem::zeroed() },
            icon: std::ptr::null_mut(),
            taskbar_message: 0,
        };
        assert!(state.change_key(std::ptr::null_mut(), original));
        let previous_id = state.hotkey_id;
        assert!(!state.change_key(std::ptr::null_mut(), occupied));
        assert_eq!(state.hotkey, original);
        assert_eq!(state.hotkey_id, previous_id);
        unsafe {
            UnregisterHotKey(std::ptr::null_mut(), state.hotkey_id);
        }
        release.send(()).unwrap();
        blocker.join().unwrap();
    }
    #[test]
    fn shell_icons_have_pixels_and_transparency() {
        unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        }
        let image = shell_icon(&std::env::current_exe().unwrap()).expect("Shell executable icon");
        assert_eq!(image.size, [32, 32]);
        assert!(image.pixels.iter().any(|p| p.a() > 0));
        assert!(image.pixels.iter().any(|p| p.a() < 255));
        unsafe {
            CoUninitialize();
        }
    }
}
