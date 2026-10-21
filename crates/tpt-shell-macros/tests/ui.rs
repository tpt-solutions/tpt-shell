// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Compile-fail tests for the macros: each file in `ui/` must fail to compile with the error
//! recorded beside it in `ui/*.stderr`. Regenerate the recordings with
//! `TRYBUILD=overwrite cargo test -p tpt-shell-macros --test ui` and review the diff.

#[test]
fn compile_fail() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
