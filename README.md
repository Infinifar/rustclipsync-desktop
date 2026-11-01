<p><img src="assets/omaclipsync.svg" alt="RustClipSync icon" width="64" height="64"></p>

# RustClipSync Desktop

English | [简体中文](README-zh.md)

A lightweight text clipboard sync client for Windows 11 and Linux, built on [rustclipsync-core](https://github.com/Infinifar/rustclipsync-core). The desktop UI uses WinUI 3 on Windows and Qt 6/QML on Linux. The Rust background agent keeps syncing after you close the UI.

## Download and setup

Download the installer for your platform from the [latest release](https://github.com/Infinifar/rustclipsync-desktop/releases/latest). Each release includes a combined `SHA256SUMS` file.

| Platform | Package |
| --- | --- |
| Windows 11 x64 | `RustClipSync-<version>-setup-x64.exe`, installed per user |
| Debian 12 / Ubuntu 24.04 baseline | Distribution-specific `.deb` |
| Fedora 43 baseline | `.rpm` |
| Arch / Omarchy | `.pkg.tar.zst` |

After installation, open Settings and enter your HTTPS server URL, username and password to pair. See the [Linux guide (Chinese)](LINUX.md) for installation commands, desktop requirements and shortcut configuration. Wayland requires data-control support; GNOME/Wayland sessions without this protocol are currently unsupported.

Windows uses shared system runtimes and provides official Microsoft download links when dependencies are missing. See the [Windows installation guide (Chinese)](WINDOWS.md). Windows installers are currently unsigned; see the [code signing policy (Chinese)](CODE-SIGNING.md).

## Features

- **Text sync:** bidirectional, upload-only or download-only. After going offline, only the latest item is synced. Persistent cursors and message IDs prevent repeated processing. Text items can be up to 10 MiB.
- **Sensitive inbox:** masked by default, with reveal, copy, delete and a separate window that can stay on top. Automatically writing sensitive text to the system clipboard is off by default.
- **Encrypted history:** search, pagination, favorites and cleanup. History is off by default; when enabled, retention defaults to 7 days, with configurable retention and entry limits. Sensitive text is excluded from ordinary history.
- **Desktop integration:** themes, font size and login startup. Windows provides a tray icon and global shortcuts; Linux supports desktop shortcuts for opening history or the sensitive inbox.
- **UI resource management:** Windows pages are created on first use. Main-window polling pauses when minimized and resumes immediately when restored. Unchanged history pages are cached by storage instance, revision and query.

## Privacy

**Sync text marked as sensitive** is off by default. Some applications do not mark sensitive content, so this setting cannot identify all sensitive information.

Local state and history are encrypted, with the master key protected by Windows DPAPI or Linux Secret Service. Passwords are not stored in configuration files or command-line arguments. Logs and ordinary status responses exclude clipboard text. HTTPS protects transport, but the server can still read synced content: end-to-end encryption is not implemented. Deleting an inbox item affects only this device and does not reset the receive cursor.

## Development and releases

[Desktop CI / Release](.github/workflows/desktop.yml) is the unified build entry point. It validates Windows and four Linux distributions in parallel, then publishes all installers and checksums in one GitHub Release after all checks pass.

To publish a new version, update the version metadata and add its release notes in English on the main branch. Other commits run CI without publishing a new version; manual builds are also available in Actions. Updates to an existing release notes file are synced to the published release. See the [release process (Chinese)](RELEASING.md), [validation records (Chinese)](VALIDATION.md) and [agent development notes (Chinese)](agent/README.md).

Related projects: [Server](https://github.com/Infinifar/rustclipsync-server) · [Protocol](https://github.com/Infinifar/rustclipsync-server/blob/main/PROTOCOL-V1.md) · [Android keyboard](https://github.com/Infinifar/fcitx5-android-bibi-keyboard) · [Omaclipsync](https://github.com/Infinifar/omaclipsync)

Licensed under MIT. Third-party components are listed in [THIRD-PARTY.md](THIRD-PARTY.md).
