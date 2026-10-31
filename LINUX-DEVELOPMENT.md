# Linux desktop implementation

The existing `rustclipsync-core` source snapshot remains the protocol authority.
This work changes the desktop host, not the server protocol or shared core.

## Completed implementation

1. Session orchestration and encrypted local history live in shared agent modules.
   Windows messages, DPAPI, ACLs, clipboard, tray and hotkeys remain in `windows/`.
2. The Linux host provides private XDG storage, Secret Service key storage, bounded local
   Unix-socket IPC, regular Wayland/X11 clipboard access and desktop activation.
3. An independent Qt 6/QML interface follows the existing desktop composition:
   centered Home/Appearance/Shortcuts/Settings navigation, sync controls, an always
   visible masked sensitive inbox, and searchable encrypted history. Qt is a thin
   presentation host; network requests and state transitions stay in Rust.
4. Native DEB/RPM/Arch packaging and a Linux CI matrix keep
   the existing native Windows build, clipboard, IPC and WinUI installation tests.

## Verification boundaries

- Tests use isolated storage, a disposable D-Bus/keyring and nested display servers;
  never pair to production or change the user's active clipboard.
- A package build does not establish support for every desktop environment.
  Detect missing Wayland data-control support and fail visibly; do not silently
  use XWayland as a background clipboard substitute.
- GNOME/Wayland without data-control needs a separate supported integration.
- Define native package dependency floors. Do not call a tarball or AppImage
  universally compatible with older glibc systems.
- Preserve Windows configuration, state format, named-pipe contract and UI behavior.

## UI direction

Preserve the existing RustClipSync identity and Omaclipsync layout, using stock
Qt Quick Controls with system/light/dark palettes and scalable text. Existing
Windows 0.2.3 home screenshots in `dist/windows-0.2.3/` are the composition
reference, not assets to ship. Use the existing logo, real controls, clear empty
states, one scroll owner per page, and no sensitive text in status/notifications.
Verify a normal desktop window and a narrow window, light/dark themes and the
separate masked inbox window. No pixel-identical claim across UI toolkits.

Final checks and platform limitations are recorded in [VALIDATION.md](VALIDATION.md).
