#![cfg_attr(windows, windows_subsystem = "windows")]
use std::io;
fn main() {
    #[cfg(windows)]
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--help" || arg == "pair")
        && !std::env::args().any(|arg| arg == "--password-stdin")
    {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    if let Err(error) = entry() {
        eprintln!("RustClipSync: {error}");
        #[cfg(windows)]
        if std::env::args().nth(1).is_none_or(|arg| arg == "run") {
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
#[cfg(all(target_os = "linux", not(feature = "check-windows")))]
fn entry() -> io::Result<()> {
    rustclipsync_agent::linux::host::entry()
}
#[cfg(not(any(windows, target_os = "linux", feature = "check-windows")))]
fn entry() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the agent requires Windows or Linux",
    ))
}
#[cfg(any(windows, feature = "check-windows"))]
fn entry() -> io::Result<()> {
    use rustclipsync_agent::{
        config::Config,
        windows::{
            clipboard::WindowsClipboard,
            runtime::{self, Control},
            storage::{self, InstanceLock},
            window::Window,
        },
    };
    use std::{
        path::PathBuf,
        sync::{atomic::Ordering, mpsc, Arc},
    };
    use zeroize::Zeroizing;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let ui = std::env::current_exe()?.with_file_name("RustClipSync.exe");
        if ui.is_file() {
            std::process::Command::new(ui).spawn()?;
            return Ok(());
        }
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(std::ptr::null_mut(),rustclipsync_agent::windows::wide("此文件是 RustClipSync 后台组件。请运行 RustClipSync 安装程序，或从命令行执行 pair 和 run。\nThis is the background component. Use the RustClipSync installer or the pair/run commands.").as_ptr(),rustclipsync_agent::windows::wide("RustClipSync").as_ptr(),windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION);
        }
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--help") {
        println!("rustclipsync-agent run\nrustclipsync-agent rpc < request.json\nrustclipsync-agent pair <https-url> <account> [--reset] [--password-stdin]\nPassword is read from the console or the GUI's anonymous stdin pipe, never command-line arguments.");
        return Ok(());
    }
    if args[0] == "rpc" && args.len() == 1 {
        use std::io::Read;
        let mut bytes = Vec::new();
        io::stdin()
            .take((rustclipsync_agent::ipc::MAX_FRAME + 1) as u64)
            .read_to_end(&mut bytes)?;
        let response = rustclipsync_agent::windows::pipe::call(&bytes)?;
        println!("{}", serde_json::to_string(&response)?);
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
        "pair"
            if args.len() >= 3
                && args.len() <= 5
                && args[3..]
                    .iter()
                    .all(|a| a == "--reset" || a == "--password-stdin") =>
        {
            let mut config = Config::new(args[1].clone(), args[2].clone());
            config.validate()?;
            let existing = storage::read_private(&dir.join("config.json"), 64 * 1024)?;
            if let Some(ref bytes) = existing {
                let mut previous: Config = serde_json::from_slice(bytes)?;
                previous.validate()?;
                if previous.identity() == config.identity() {
                    config = previous;
                }
            }

            if existing.is_some() && !args[3..].iter().any(|a| a == "--reset") {
                return Err(io::Error::other(
                    "already configured; --reset explicitly replaces the pairing and pending data",
                ));
            }
            let password = if args[3..].iter().any(|a| a == "--password-stdin") {
                rustclipsync_agent::credentials::read_secret(io::stdin().lock())?
            } else {
                Zeroizing::new(read_password()?)
            };
            runtime::pair(
                &dir,
                &config,
                password.to_string(),
                args[3..].iter().any(|a| a == "--reset"),
            )
            .map_err(|e| io::Error::other(runtime::classify(&e)))?;
            storage::atomic_write(
                &dir.join("config.json"),
                &serde_json::to_vec_pretty(&config)?,
            )?;
            println!("Pairing complete. Start rustclipsync-agent run.");
            Ok(())
        }
        "run" | "autostart" if args.len() == 1 => {
            if args[0] == "autostart" && !dir.join("config.json").is_file() {
                return Ok(());
            }
            let bytes = storage::read_private(&dir.join("config.json"), 64 * 1024)?
                .ok_or_else(|| io::Error::other("run pair first"))?;
            let mut config: Config = serde_json::from_slice(&bytes)?;
            config.validate()?;
            let (sender, receiver) = mpsc::sync_channel(32);
            let control = Arc::new(Control::new(&config, sender));
            let window = Window::create(control.clone(), true)?;
            let clipboard = WindowsClipboard::new(window.hwnd(), window.formats()?);
            let _pipe = rustclipsync_agent::windows::pipe::Server::start(control.clone())?;
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
