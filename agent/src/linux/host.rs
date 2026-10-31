use super::{
    clipboard::PlatformClipboard,
    pipe,
    storage::{self, InstanceLock},
};
use crate::{
    config::Config,
    runtime::{self, Control},
};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    sync::{atomic::Ordering, mpsc, Arc},
    time::Duration,
};

pub fn entry() -> io::Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let dir = if args.first().is_some_and(|a| a == "--data-dir") {
        if args.len() < 3 {
            return Err(io::Error::other(
                "--data-dir requires an absolute directory and command",
            ));
        }
        let dir = PathBuf::from(args.remove(1));
        args.remove(0);
        if !dir.is_absolute() {
            return Err(io::Error::other("profile path must be absolute"));
        }
        dir
    } else {
        storage::data_dir()?
    };
    match args.first().map(String::as_str) {
        None => {
            let exe = std::env::current_exe()?;
            let sibling = exe.with_file_name("rustclipsync");
            let ui = if sibling.is_file() {
                sibling
            } else {
                PathBuf::from("rustclipsync")
            };
            std::process::Command::new(ui).spawn()?;
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!("rustclipsync-agent [--data-dir <absolute-path>] <command>\n  pair <https-url> <account> [--reset] [--password-stdin]\n  run | autostart\n  rpc < request.json\n  socket-path\n  capabilities\nPasswords are read privately from stdin or the terminal; never pass them as arguments.");
            return Ok(());
        }
        Some("capabilities") if args.len() == 1 => {
            println!(
                "{}",
                serde_json::json!({"platform":"linux", "clipboard":"wayland-data-control-or-x11", "keyStore":"secret-service", "ipc":"unix-socket", "shortcuts":"desktop-activation-commands"})
            );
            return Ok(());
        }
        Some("socket-path") if args.len() == 1 => {
            println!("{}", pipe::socket_path(&dir)?.display());
            return Ok(());
        }
        Some("rpc") if args.len() == 1 => {
            let mut bytes = Vec::new();
            io::stdin()
                .take((crate::ipc::MAX_FRAME + 1) as u64)
                .read_to_end(&mut bytes)?;
            println!("{}", pipe::call(&dir, &bytes)?);
            return Ok(());
        }
        _ => {}
    }
    storage::ensure_private_dir(&dir)?;
    let _lock = InstanceLock::acquire(&dir)?;
    match args[0].as_str() {
        "pair"
            if (3..=5).contains(&args.len())
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
            let reset = args[3..].iter().any(|a| a == "--reset");
            if existing.is_some() && !reset {
                return Err(io::Error::other(
                    "already configured; --reset explicitly replaces pairing and pending data",
                ));
            }
            let password = if args[3..].iter().any(|a| a == "--password-stdin") {
                crate::credentials::read_secret(io::stdin().lock())?
            } else {
                read_password()?
            };
            runtime::pair(&dir, &config, password.to_string(), reset)
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
            let (tx, rx) = mpsc::sync_channel(32);
            let control = Arc::new(Control::new(&config, tx));
            let (clipboard, monitor) = PlatformClipboard::start(control.clone())?;
            let server = pipe::Server::start(&dir, control.clone())?;
            let mut signals = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT])?;
            let signal_handle = signals.handle();
            let shared = control.clone();
            let signal_thread = std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    shared.stop.store(true, Ordering::SeqCst);
                    shared.notify();
                }
            });
            let shared = control.clone();
            let worker =
                std::thread::spawn(move || runtime::run(dir, config, clipboard, shared, rx));
            while !control.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
            control.notify();
            drop(monitor);
            drop(server);
            signal_handle.close();
            let _ = signal_thread.join();
            worker
                .join()
                .map_err(|_| io::Error::other("background worker stopped unexpectedly"))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid arguments; run --help",
        )),
    }
}
fn read_password() -> io::Result<zeroize::Zeroizing<String>> {
    struct Restore(libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
            }
        }
    }
    let mut mode = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut mode) } != 0 {
        return Err(io::Error::other(
            "use --password-stdin when no terminal is available",
        ));
    }
    let _restore = Restore(mode);
    mode.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &mode) } != 0 {
        return Err(io::Error::last_os_error());
    }
    eprint!("Password: ");
    io::stderr().flush()?;
    let mut password = zeroize::Zeroizing::new(String::new());
    io::stdin().read_line(&mut password)?;
    eprintln!();
    while password.ends_with(['\r', '\n']) {
        password.pop();
    }
    if password.is_empty() || password.len() > 8192 {
        return Err(io::Error::other("invalid password length"));
    }
    Ok(password)
}
