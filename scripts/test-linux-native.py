#!/usr/bin/env python3
"""Run Linux native checks with a disposable clipboard, D-Bus and desktop keyring."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def session():
    subprocess.run(
        ["gnome-keyring-daemon", "--unlock", "--components=secrets"],
        input=b"disposable-test-keyring",
        stdout=subprocess.DEVNULL,
        check=True,
        timeout=15,
    )
    command = ["cargo", "test", "--locked", "--test", "linux_native", "--", "--ignored", "--test-threads=1", "--nocapture"]
    subprocess.run(command, cwd=ROOT, check=True)


def main():
    if len(sys.argv) == 2 and sys.argv[1] == "--session":
        session()
        return
    with tempfile.TemporaryDirectory(prefix="rustclipsync-native-") as temporary:
        base = Path(temporary)
        for name in ("home", "state", "config", "runtime", "data", "cache"):
            (base / name).mkdir(mode=0o700)
        env = dict(os.environ)
        env.update(HOME=str(base / "home"), XDG_STATE_HOME=str(base / "state"), XDG_CONFIG_HOME=str(base / "config"), XDG_RUNTIME_DIR=str(base / "runtime"), RCS_LINUX_DISPOSABLE="1")
        # cargo/rustup must retain the developer toolchain outside the disposable HOME.
        env["CARGO_HOME"] = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
        env["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
        env["XDG_DATA_HOME"] = str(base / "data")
        env["XDG_CACHE_HOME"] = str(base / "cache")
        ui = ROOT / "dist/linux-root/usr/bin/rustclipsync"
        if ui.is_file():
            env["RCS_LINUX_UI_BINARY"] = str(ui)
        env.pop("DBUS_SESSION_BUS_ADDRESS", None)
        if shutil.which("Xvfb"):
            server_command = ["Xvfb", "-displayfd", "1", "-screen", "0", "1024x900x24", "-nolisten", "tcp", "-ac"]
        elif shutil.which("Xwayland") and os.environ.get("WAYLAND_DISPLAY"):
            server_command = ["Xwayland", "-displayfd", "1", "-rootless", "-noreset", "-nolisten", "tcp", "-ac"]
        else:
            raise RuntimeError("Install Xvfb for isolated clipboard tests")
        # Start Xwayland with the original Wayland runtime, then isolate the tests.
        with open(base / "display.log", "wb") as log:
            display = subprocess.Popen(server_command, stdout=subprocess.PIPE, stderr=log)
            try:
                number = display.stdout.readline().decode().strip()
                if not number.isdigit():
                    raise RuntimeError("nested display did not start")
                env["DISPLAY"] = ":" + number
                env.pop("WAYLAND_DISPLAY", None)
                env.pop("GNOME_KEYRING_CONTROL", None)
                subprocess.run(["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()), "--session"], cwd=ROOT, env=env, check=True, timeout=240)
            finally:
                display.terminate()
                try:
                    display.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    display.kill()
                    display.wait()


if __name__ == "__main__":
    main()
