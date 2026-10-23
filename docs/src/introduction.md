# Introduction

tpt-shell is a webview app shell for Rust. It hosts web UIs in the operating system's own webview and gives them a typed, validated, permissioned and audited channel to Rust code.

tpt-shell has no telemetry. The shell does not collect, store for upload, or send usage data, and it opens no sockets unless a feature you enable needs one.

Start with the [Quickstart](quickstart.md) and the [Concepts](concepts.md). The [Cookbook](cookbook.md) has short recipes, the guides cover kiosk, devices, audit, agents, profiles and compliance, and the [Security guide](security.md) says what is protected by default. Each page marks what is built and what is not. Check [Platform notes](platform-notes.md) before you choose a platform: Windows is the platform where opening windows has been verified; macOS and Linux have not yet been verified to open one.

Some pages are still unfinished. Where a feature is not built, the page says so and names its item in [todo.md](../../todo.md).
