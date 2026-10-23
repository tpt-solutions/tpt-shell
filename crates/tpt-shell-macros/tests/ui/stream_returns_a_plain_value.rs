// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::{command, CommandError};

#[command(kind = "stream")]
fn items(n: u32) -> u32 {
    n
}

fn main() {}
