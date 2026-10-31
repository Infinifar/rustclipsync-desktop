use super::{
    clipboard::Formats,
    runtime::{Control, STATUS_CHANGED, WORKER_STOPPED},
    wide,
};
use std::{
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
const PAUSE: usize = 1001;
const QUIT: usize = 1002;
struct Context {
    control: Arc<Control>,
    taskbar: u32,
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
            wc.lpszClassName = class.as_ptr();
            if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(io::Error::last_os_error());
            }
            let mut context = Box::new(Context {
                control,
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
            context.control.hwnd.store(hwnd as usize, Ordering::SeqCst);
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
        TRAY_CALLBACK if matches!(lparam as u32, WM_RBUTTONUP | WM_CONTEXTMENU) => {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return 0;
            }
            let paused = control.paused.load(Ordering::SeqCst);
            AppendMenuW(
                menu,
                MF_STRING,
                PAUSE,
                wide(if paused {
                    "Resume synchronization"
                } else {
                    "Pause synchronization"
                })
                .as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, QUIT, wide("Exit RustClipSync").as_ptr());
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
    data.hIcon = LoadIconW(ptr::null_mut(), IDI_APPLICATION);
    let state = control.status.lock().map(|v| *v).unwrap_or("error");
    let tooltip = wide(&format!("RustClipSync: {state}"));
    let len = tooltip.len().min(data.szTip.len() - 1);
    data.szTip[..len].copy_from_slice(&tooltip[..len]);
    Shell_NotifyIconW(action, &data) != 0
}
