// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Linux backend for tpt-shell (spec §6.3).
//!
//! GTK and WebKitGTK are loaded with `dlopen` at run time, so the shell builds without their
//! development packages and can report a missing library clearly. This crate has the loader.
//! The GTK and WebKit calls are not written yet, and no Linux machine has run this code.

pub mod loader;
