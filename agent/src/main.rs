#![cfg_attr(windows, windows_subsystem = "windows")]
use std::io;
fn main() {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() != Some("run") {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    if let Err(error) = entry() {
        eprintln!("RustClipSync: {error}");
        #[cfg(windows)]
        if std::env::args().nth(1).as_deref() == Some("run") {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    rustclipsync_agent::windows::wide(&error.to_string()).as_ptr(),
                    rustclipsync_agent::windows::wide("RustClipSync").as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}
#[cfg(not(any(windows, feature = "check-windows")))]
fn entry() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the agent requires Windows 11",
    ))
}
#[cfg(any(windows, feature = "check-windows"))]
fn entry() -> io::Result<()> {
    use rustclipsync_agent::{
        config::{Config, Direction},
        windows::{
            clipboard::WindowsClipboard,
            runtime::{self, Control},
            storage::{self, InstanceLock},
            window::Window,
        },
    };
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            mpsc, Arc, Mutex,
        },
    };
    use zeroize::Zeroizing;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_none_or(|arg| arg == "--help") {
        println!("rustclipsync-agent run\nrustclipsync-agent pair <https-url> <account> [--reset]\nPassword is read from the console; it is never a command-line argument. Exit the running agent before pairing.");
        return Ok(());
    }
    let base = PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::other("LOCALAPPDATA is not available"))?,
    );
    let dir = base.join("RustClipSync");
    storage::ensure_private_dir(&dir)?;
    let _lock = InstanceLock::acquire(&dir)?;
    match args[0].as_str() {
        "pair" if args.len() == 3 || (args.len() == 4 && args[3] == "--reset") => {
            let mut config = Config {
                server: args[1].clone(),
                user: args[2].clone(),
                direction: Direction::Both,
                paused: false,
            };
            config.validate()?;
            let existing = storage::read_private(&dir.join("config.json"), 64 * 1024)?;
            if existing.is_some() && args.len() != 4 {
                return Err(io::Error::other(
                    "already configured; --reset explicitly replaces the pairing and pending data",
                ));
            }
            let password = Zeroizing::new(read_password()?);
            runtime::pair(&dir, &config, password.to_string(), args.len() == 4)
                .map_err(|e| io::Error::other(runtime::classify(&e)))?;
            storage::atomic_write(
                &dir.join("config.json"),
                &serde_json::to_vec_pretty(&config)?,
            )?;
            println!("Pairing complete. Start rustclipsync-agent run.");
            Ok(())
        }
        "run" if args.len() == 1 => {
            let bytes = storage::read_private(&dir.join("config.json"), 64 * 1024)?
                .ok_or_else(|| io::Error::other("run pair first"))?;
            let mut config: Config = serde_json::from_slice(&bytes)?;
            config.validate()?;
            let (sender, receiver) = mpsc::sync_channel(32);
            let control = Arc::new(Control {
                stop: AtomicBool::new(false),
                paused: AtomicBool::new(config.paused),
                generation: Arc::new(AtomicU64::new(0)),
                hwnd: AtomicUsize::new(0),
                status: Mutex::new("connecting"),
                wake: sender,
            });
            let window = Window::create(control.clone(), true)?;
            let clipboard = WindowsClipboard::new(window.hwnd(), window.formats()?);
            let worker_control = control.clone();
            let worker = std::thread::spawn(move || {
                runtime::run(dir, config, clipboard, worker_control, receiver)
            });
            let result = window.message_loop();
            control.stop.store(true, Ordering::SeqCst);
            control.notify();
            if worker.join().is_err() {
                return Err(io::Error::other("background worker stopped unexpectedly"));
            }
            result
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid arguments; run --help",
        )),
    }
}
#[cfg(any(windows, feature = "check-windows"))]
fn read_password() -> io::Result<String> {
    use std::io::Write;
    use windows_sys::Win32::System::Console::*;
    struct Restore(windows_sys::Win32::Foundation::HANDLE, u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.0, self.1);
            }
        }
    }
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(input, &mut mode) == 0 {
            return Err(io::Error::other(
                "password input requires an interactive Windows console",
            ));
        }
        if SetConsoleMode(input, mode & !ENABLE_ECHO_INPUT) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _restore = Restore(input, mode);
        eprint!("Password: ");
        io::stderr().flush()?;
        let mut password = String::new();
        io::stdin().read_line(&mut password)?;
        eprintln!();
        while password.ends_with(['\n', '\r']) {
            password.pop();
        }
        if password.is_empty() {
            return Err(io::Error::other("password cannot be empty"));
        }
        Ok(password)
    }
}
