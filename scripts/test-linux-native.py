#!/usr/bin/env python3
"""Run Linux native checks with a disposable clipboard, D-Bus and desktop keyring."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def session():
    subprocess.run(
        ["gnome-keyring-daemon", "--unlock", "--components=secrets"],
        input=b"disposable-test-keyring",
        stdout=subprocess.DEVNULL,
        check=True,
        timeout=15,
    )
    binary = os.environ.get("RCS_LINUX_TEST_BINARY")
    command = ([binary] if binary else ["cargo", "test", "--locked", "--test", "linux_native", "--"])
    command += ["--ignored", "--test-threads=1", "--nocapture"]
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
        if ui.is_file() and "RCS_LINUX_UI_BINARY" not in env:
            env["RCS_LINUX_UI_BINARY"] = str(ui)
        env.pop("DBUS_SESSION_BUS_ADDRESS", None)
        wayland = "--wayland" in sys.argv
        if wayland:
            if os.geteuid() == 0:
                raise RuntimeError("Run the headless Sway tests as a regular user")
            (base / "sway.conf").write_text("xwayland disable\noutput * mode 1024x768\n")
            env.update(WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            server_command = ["sway", "--unsupported-gpu", "--config", str(base / "sway.conf")]
        elif shutil.which("Xvfb"):
            server_command = ["Xvfb", "-displayfd", "1", "-screen", "0", "1024x900x24", "-nolisten", "tcp", "-ac"]
        elif shutil.which("Xwayland") and os.environ.get("WAYLAND_DISPLAY"):
            server_command = ["Xwayland", "-displayfd", "1", "-rootless", "-noreset", "-nolisten", "tcp", "-ac"]
        else:
            raise RuntimeError("Install Xvfb for isolated clipboard tests")
        # Start Xwayland with the original Wayland runtime, then isolate the tests.
        with open(base / "display.log", "wb") as log:
            display = subprocess.Popen(server_command, stdout=subprocess.PIPE, stderr=log, env=env if wayland else None)
            try:
                if wayland:
                    deadline = time.monotonic() + 10
                    sockets = []
                    while not sockets and time.monotonic() < deadline and display.poll() is None:
                        sockets = [p for p in (base / "runtime").glob("wayland-*") if p.is_socket()]
                        time.sleep(0.05)
                    if not sockets:
                        raise RuntimeError("headless Sway did not start: " + (base / "display.log").read_text()[-2000:])
                    env["WAYLAND_DISPLAY"] = sockets[0].name
                else:
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
