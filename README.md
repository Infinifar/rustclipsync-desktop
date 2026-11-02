<p><img src="assets/omaclipsync.svg" alt="RustClipSync icon" width="64" height="64"></p>

Version **0.5.0** uses rustclipsync-core **0.4.0** and wire protocol **3** at `/api/...`. Pair it with RustClipSync Server **3.0.0**; see the [release notes](releases/v0.5.0.md). Older clients need server 2.0.0.

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

- **Text sync:** bidirectional, upload-only or download-only. After going offline, the server retains at most one eligible item per device within the administrator-configured reconnect window. Persistent cursors and message IDs prevent repeated processing. Text items can be up to 10 MiB.
- **Sensitive inbox:** masked by default, with reveal, copy, delete and a separate window that can stay on top. Automatically writing sensitive text to the system clipboard is off by default.
- **Encrypted history:** search, pagination, favorites and cleanup. History is off by default; when enabled, retention defaults to 7 days, with configurable retention and entry limits. Sensitive text is excluded from ordinary history.
- **Desktop integration:** themes, font size and login startup. Windows provides a tray icon and global shortcuts; Linux supports desktop shortcuts for opening history or the sensitive inbox.
- **UI resource management:** Windows pages are created on first use. Main-window polling pauses when minimized and resumes immediately when restored. Unchanged history pages are cached by storage instance, revision and query.

## Direction reporting and protocol compatibility

This client requires protocol **3** (`X-RCS-Protocol: 3`); update it together with a matching server. Older clients must continue using their older server. A protocol mismatch stops synchronization and displays an upgrade message instead of retrying credentials.

At connection/reconnection, the shared core reports upload-only, receive-only or bidirectional mode and waits for a server ACK before receiving. Direction changes are reported immediately, using the WebSocket control channel with an authenticated HTTP fallback. Local direction and sensitive-content filters remain active when a report fails; the UI marks the direction as unconfirmed. Pausing keeps the chosen direction and pairing while blocking sending and receiving. Manual send/receive obey the same direction and pause state.

Ordinary UI or background-process closure is offline, not logout. Explicit re-pairing or account replacement must confirm revocation of the previous direction session before replacing the local identity; a failed revocation is shown as unconfirmed. Server-side inactive direction state expires after 30 days. Reconnect backfill is only the latest eligible per-device message, after the latest direction ACK. Its retention window belongs to the server administration page, including the disable setting; the client does not choose or hardcode a backfill window. See the [direction protocol](https://github.com/Infinifar/rustclipsync-server/blob/main/docs/DIRECTION-PROTOCOL.md).

Mobile apps and Fcitx input methods are released separately and are not part of this release batch. Verify their protocol-3 support before switching a shared server. Omaclipsync **0.4.0** is the matching Omarchy plugin release.

## Privacy

**Sync text marked as sensitive** is off by default. Some applications do not mark sensitive content, so this setting cannot identify all sensitive information.

Local state and history are encrypted, with the master key protected by Windows DPAPI or Linux Secret Service. Passwords are not stored in configuration files or command-line arguments. Logs and ordinary status responses exclude clipboard text. HTTPS protects transport, but the server can still read synced content: end-to-end encryption is not implemented. Deleting an inbox item affects only this device and does not reset the receive cursor.

## Development and releases

[Desktop CI / Release](.github/workflows/desktop.yml) is the unified build entry point. It validates Windows and four Linux distributions in parallel, then publishes all installers and checksums in one GitHub Release after all checks pass.

To publish a new version, update the version metadata and add its release notes in English on the main branch. Other commits run CI without publishing a new version; manual builds are also available in Actions. Updates to an existing release notes file are synced to the published release. See the [release process (Chinese)](RELEASING.md), [validation records (Chinese)](VALIDATION.md) and [agent development notes (Chinese)](agent/README.md).

Related projects: [Server](https://github.com/Infinifar/rustclipsync-server) · [Protocol](https://github.com/Infinifar/rustclipsync-server/blob/main/PROTOCOL.md) · [Android keyboard](https://github.com/Infinifar/fcitx5-android-bibi-keyboard) · [Omaclipsync](https://github.com/Infinifar/omaclipsync)

Licensed under MIT. Third-party components are listed in [THIRD-PARTY.md](THIRD-PARTY.md).

### Moving away from an older server

Upgrade server/client versions as a set. If the old server has no direction-revocation API, first revoke the old device token using that server's administrator/older client. Stop the background agent, then explicitly discard only this local pairing:

```bash
rustclipsync-agent forget-pairing --confirm-remote-revocation-unconfirmed
```

This destroys the local device token, pending outbox and sensitive inbox; encrypted history and key-store entries remain. It does not prove or perform remote revocation: the old server may still authorize that token until its administrator revokes it. Pair with the new server afterwards. A protocol error is never treated as successful revocation.

HTTP fallback has a 30-second online lease, separate from the administration page's activity badge. A server backfill window of zero disables offline storage/replay, while eligible online devices still receive live updates.
