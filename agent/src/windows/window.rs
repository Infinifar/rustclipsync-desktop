use super::{
    clipboard::Formats,
    runtime::{Control, STATUS_CHANGED, UPDATE_SHORTCUTS, WORKER_STOPPED},
    shortcuts::Hotkeys,
    wide,
};
use std::{
    cell::RefCell,
    io, mem, ptr,
    sync::{atomic::Ordering, Arc},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener},
        LibraryLoader::GetModuleHandleW,
    },
    UI::{Shell::*, WindowsAndMessaging::*},
};
const TRAY_CALLBACK: u32 = WM_APP + 1;
const OPEN: usize = 1000;
const PAUSE: usize = 1001;
const QUIT: usize = 1002;
struct Context {
    control: Arc<Control>,
    taskbar: u32,
    hotkeys: RefCell<Hotkeys>,
}
pub struct Window {
    hwnd: HWND,
    _context: Box<Context>,
}
impl Window {
    pub fn create(control: Arc<Control>, show_tray: bool) -> io::Result<Self> {
        unsafe {
            let class = wide("RustClipSync.Agent.Window");
            let instance = GetModuleHandleW(ptr::null());
            let mut wc: WNDCLASSW = mem::zeroed();
            wc.lpfnWndProc = Some(procedure);
            wc.hInstance = instance;
            wc.hIcon = LoadIconW(instance, 1usize as _);
            wc.lpszClassName = class.as_ptr();
            if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(io::Error::last_os_error());
            }
            let mut context = Box::new(Context {
                control,
                hotkeys: RefCell::new(Hotkeys::default()),
                taskbar: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            });
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("RustClipSync").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                (&mut *context as *mut Context).cast(),
            );
            if hwnd.is_null() {
                return Err(io::Error::last_os_error());
            }
            context
                .control
                .signals
                .hwnd
                .store(hwnd as usize, Ordering::SeqCst);
            let binding_failed = context
                .hotkeys
                .borrow_mut()
                .replace(hwnd as usize, &context.control.shortcuts())
                .is_err();
            context
                .control
                .shortcut_error
                .store(binding_failed, Ordering::SeqCst);
            if AddClipboardFormatListener(hwnd) == 0 {
                DestroyWindow(hwnd);
                return Err(io::Error::last_os_error());
            }
            let window = Self {
                hwnd,
                _context: context,
            };
            if show_tray && !window.tray(NIM_ADD) {
                return Err(io::Error::other("could not create notification area icon"));
            }
            Ok(window)
        }
    }
    pub fn hwnd(&self) -> usize {
        self.hwnd as usize
    }
    pub fn formats(&self) -> io::Result<Formats> {
        Formats::register()
    }
    fn tray(&self, action: u32) -> bool {
        unsafe { tray(self.hwnd, &self._context.control, action) }
    }
    pub fn message_loop(&self) -> io::Result<()> {
        unsafe {
            let mut message: MSG = mem::zeroed();
            loop {
                let result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
                if result == 0 {
                    return Ok(());
                }
                if result == -1 {
                    return Err(io::Error::last_os_error());
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            self.tray(NIM_DELETE);
            self._context.hotkeys.borrow_mut().clear(self.hwnd as usize);
            RemoveClipboardFormatListener(self.hwnd);
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let context = &*pointer;
    let control = &context.control;
    if context.taskbar != 0 && message == context.taskbar {
        tray(hwnd, control, NIM_ADD);
        return 0;
    }
    match message {
        UPDATE_SHORTCUTS => {
            let request = control
                .signals
                .shortcut_request
                .lock()
                .ok()
                .and_then(|mut r| r.take());
            if let Some(request) = request {
                if !request.cancelled.load(Ordering::SeqCst) {
                    let previous = control.shortcuts();
                    let result = context
                        .hotkeys
                        .borrow_mut()
                        .replace(hwnd as usize, &request.settings);
                    if request.cancelled.load(Ordering::SeqCst) {
                        let failed = context
                            .hotkeys
                            .borrow_mut()
                            .replace(hwnd as usize, &previous)
                            .is_err();
                        control.shortcut_error.store(failed, Ordering::SeqCst);
                    } else {
                        if result.is_ok() {
                            control.shortcut_error.store(false, Ordering::SeqCst);
                        }
                        let _ = request.reply.send(result);
                    }
                }
            }
            0
        }
        WM_HOTKEY => {
            if let Some(action) = context.hotkeys.borrow().action(wparam) {
                open_ui(Some(action));
            }
            0
        }
        WM_CLIPBOARDUPDATE => {
            control.generation.fetch_add(1, Ordering::SeqCst);
            control.notify();
            0
        }
        WM_POWERBROADCAST => {
            control.notify();
            1
        }
        STATUS_CHANGED => {
            tray(hwnd, control, NIM_MODIFY);
            0
        }
        TRAY_CALLBACK if lparam as u32 == WM_LBUTTONDBLCLK => {
            open_settings();
            0
        }
        TRAY_CALLBACK if matches!(lparam as u32, WM_RBUTTONUP | WM_CONTEXTMENU) => {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return 0;
            }
            AppendMenuW(menu, MF_STRING, OPEN, wide("打开 RustClipSync").as_ptr());
            let paused = control.paused.load(Ordering::SeqCst);
            AppendMenuW(
                menu,
                MF_STRING,
                PAUSE,
                wide(if paused {
                    "恢复同步"
                } else {
                    "暂停同步"
                })
                .as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, QUIT, wide("退出 RustClipSync").as_ptr());
            let mut point = mem::zeroed();
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
            let selected = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            ) as usize;
            DestroyMenu(menu);
            PostMessageW(hwnd, WM_NULL, 0, 0);
            if selected == OPEN {
                open_settings();
            }
            if selected == PAUSE {
                control.paused.store(!paused, Ordering::SeqCst);
                control.generation.fetch_add(1, Ordering::SeqCst);
                control.notify();
            }
            if selected == QUIT {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_CLOSE => {
            control.stop.store(true, Ordering::SeqCst);
            control.generation.fetch_add(1, Ordering::SeqCst);
            control.set_status("stopping");
            control.notify();
            0
        }
        WORKER_STOPPED => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            context.hotkeys.borrow_mut().clear(hwnd as usize);
            tray(hwnd, control, NIM_DELETE);
            RemoveClipboardFormatListener(hwnd);
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
unsafe fn tray(hwnd: HWND, control: &Control, action: u32) -> bool {
    let mut data: NOTIFYICONDATAW = mem::zeroed();
    data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = TRAY_CALLBACK;
    data.hIcon = LoadIconW(GetModuleHandleW(ptr::null()), 1usize as _);
    if data.hIcon.is_null() {
        data.hIcon = LoadIconW(ptr::null_mut(), IDI_APPLICATION);
    }
    let state = control.status.lock().map(|v| *v).unwrap_or("error");
    let tooltip = wide(&format!("RustClipSync: {state}"));
    let len = tooltip.len().min(data.szTip.len() - 1);
    data.szTip[..len].copy_from_slice(&tooltip[..len]);
    Shell_NotifyIconW(action, &data) != 0
}

fn open_settings() {
    open_ui(None);
}
fn open_ui(action: Option<&str>) {
    if let Ok(exe) = std::env::current_exe() {
        let ui = exe.with_file_name("RustClipSync.exe");
        if ui.is_file() {
            let mut command = std::process::Command::new(ui);
            if let Some(action) = action {
                command.arg(action);
            }
            let _ = command.spawn();
        }
    }
}
