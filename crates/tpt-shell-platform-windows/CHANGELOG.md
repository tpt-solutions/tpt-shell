# Changelog

All notable changes to `tpt-shell-platform-windows` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Win32 windows hosting WebView2 pages, with the platform's own COM bindings, and the app origin (`https://tpt-app.localhost`) for pages served from a window's asset source.
- Strict Content-Security-Policy on every page the shell serves. Inline script and style are blocked.
- Remote navigation is blocked unless a window lists the URL prefix in `allow_navigation`.
- Developer tools and the context menu are off in release builds. Debug builds keep them on.
- Grants that need consent ask in a native message box owned by the requesting window. The default answer is No.
- Native Yes/No confirmation for agent actions, and a Ctrl+Alt+Shift+K kill-switch hotkey (Partial).
- Keystore with DPAPI: one encrypted file per name, per user. Holds the audit checkpoint key.
- Balloon notifications and a tray icon, with `App::tray`.
- Single instance by named mutex and `WM_COPYDATA`.
- Deep-link registration.
- Window placement saved and restored, and off-screen windows brought back.
- Timers on the UI thread (`WindowsBackend::every` and `cancel`).
- Transparent windows, drag regions (Partial), dark title bar, Mica, Acrylic and Tabbed backdrops.
- Message box, and a multi-select open dialog.
- Colour scheme driven through WebView2. High contrast, reduced motion and reduced transparency reach the page as `data-` attributes, and update when the user changes a setting.
- Message handler per window (`set_message_handler`). The reply is posted back to the page.
- Serial ports on COM1 to COM999, and raw spooler printing through winspool.
- Low-level key hook for kiosk key chords (best effort).
- Pinned WebView2 runtime folder, optional.
- Page runtime: `window.tpt`, `tpt.on`, and `tpt.caps` (engine `webview2` and its version).
- Blobs, with a 64 MiB limit, served from an app URL.
- UI Automation and native accessibility tree support for the test driver.

### Changed
- The Windows CLI install step writes a per-user Run entry through `tpt-shell install-service`.
