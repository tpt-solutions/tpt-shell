// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::{command, CommandError};

#[command(job)]
fn scan(n: u32) -> Result<u32, CommandError> {
    Ok(n)
}

fn main() {}
