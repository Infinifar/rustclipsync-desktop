#!/usr/bin/env python3
"""Package the same staged client with native distribution dependency metadata."""
import argparse
import hashlib
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / "agent/Cargo.toml").read_text())["package"]["version"]


def run(*args):
    subprocess.run(args, check=True, cwd=ROOT)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("format", choices=["deb", "rpm", "arch", "tar"])
    parser.add_argument("--distro", required=True, help="Distribution and release used to compile these binaries")
    options = parser.parse_args()
    stage = ROOT / "dist/linux-root"
    output = ROOT / "dist/linux-packages"
    output.mkdir(parents=True, exist_ok=True)
    if not (stage / "usr/bin/rustclipsync").is_file() or not (stage / "usr/libexec/rustclipsync/rustclipsync-agent").is_file():
        raise SystemExit("Run bash scripts/build-linux.sh --stage first")
    prefix = f"rustclipsync-desktop-{VERSION}-{options.distro}-x86_64"
    if options.format == "deb":
        control = stage / "DEBIAN"
        control.mkdir(exist_ok=True)
        (control / "control").write_text(f"""Package: rustclipsync-desktop
Version: {VERSION}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Infinifar <infinifar@users.noreply.github.com>
Depends: libdbus-1-3, libqt6core6 (>= 6.4), libqt6gui6 (>= 6.4), libqt6network6 (>= 6.4), libqt6qml6 (>= 6.4), libqt6quick6 (>= 6.4), libqt6quickcontrols2-6 (>= 6.4), qml6-module-qtquick, qml6-module-qtquick-controls, qml6-module-qtquick-layouts, qml6-module-qtquick-window, qml6-module-qtquick-templates, qml6-module-qtqml-workerscript, qml6-module-qtqml-models, qt6-wayland
Recommends: gnome-keyring | kwallet6
Description: RustClipSync native Linux desktop client
 Lightweight Rust clipboard synchronization with an independent Qt 6/QML UI.
 Built on {options.distro}; requires an unlocked Secret Service provider.
""")
        # Derive libc/libstdc++ floors from the actual artifacts, not the packager's guess.
        result = subprocess.run(["dpkg-shlibdeps", "--ignore-missing-info", "-O", "-e" + str(stage / "usr/bin/rustclipsync"), "-e" + str(stage / "usr/libexec/rustclipsync/rustclipsync-agent")], cwd=ROOT / "packaging/linux/debian", capture_output=True, text=True, check=True)
        native = next(line.split("=", 1)[1] for line in result.stdout.splitlines() if line.startswith("shlibs:Depends="))
        content = (control / "control").read_text().replace("Depends: ", "Depends: " + native + ", ", 1)
        (control / "control").write_text(content)
        run("dpkg-deb", "--root-owner-group", "--build", str(stage), str(output / (prefix + ".deb")))
        shutil.rmtree(control)
    elif options.format == "rpm":
        build = ROOT / "build/rpm"
        for name in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
            (build / name).mkdir(parents=True, exist_ok=True)
        spec = (ROOT / "packaging/linux/rustclipsync.spec.in").read_text().replace("@VERSION@", VERSION).replace("@STAGE@", str(stage))
        specfile = build / "SPECS/rustclipsync.spec"
        specfile.write_text(spec)
        run("rpmbuild", "--define", "_topdir " + str(build), "--define", "_build_id_links none", "-bb", str(specfile))
        for rpm in (build / "RPMS").rglob("*.rpm"):
            shutil.copy2(rpm, output / (prefix + ".rpm"))
    elif options.format == "arch":
        # A pacman package includes all file/directory members in .MTREE.
        files = [p for p in stage.rglob("*") if p.is_file()]
        (stage / ".PKGINFO").write_text(f"pkgname = rustclipsync-desktop\npkgver = {VERSION}-1\npkgdesc = RustClipSync native Qt 6 and Rust client\nurl = https://github.com/Infinifar/rustclipsync-desktop\nbuilddate = 0\npackager = Infinifar\nsize = {sum(p.stat().st_size for p in files)}\narch = x86_64\nlicense = MIT\ndepend = qt6-base\ndepend = qt6-declarative\ndepend = qt6-wayland\ndepend = dbus\noptdepend = gnome-keyring: Secret Service key storage\noptdepend = kwallet: Secret Service key storage\n")
        mtree = subprocess.run(["bsdtar", "-czf", "-", "--format=mtree", "--options=!all,use-set,type,uid,gid,mode,time,size,sha256", "-C", str(stage), "."], check=True, capture_output=True).stdout
        (stage / ".MTREE").write_bytes(mtree)
        run("bsdtar", "--zstd", "-cf", str(output / (prefix + ".pkg.tar.zst")), "-C", str(stage), ".")
        (stage / ".PKGINFO").unlink()
        (stage / ".MTREE").unlink()
    else:
        with tarfile.open(output / (prefix + ".tar.gz"), "w:gz") as archive:
            archive.add(stage / "usr", arcname="usr")
    packages = sorted(p for p in output.iterdir() if p.is_file() and p.name != "SHA256SUMS")
    (output / "SHA256SUMS").write_text("".join(hashlib.sha256(p.read_bytes()).hexdigest() + "  " + p.name + "\n" for p in packages))


if __name__ == "__main__":
    main()
