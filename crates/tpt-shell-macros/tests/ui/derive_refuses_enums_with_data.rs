// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::Contract;

#[derive(Contract)]
enum Mode {
    Read(u8),
}

fn main() {}
