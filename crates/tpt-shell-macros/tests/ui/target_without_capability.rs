// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::{command, CommandError};

#[command(target(path = "p"))]
fn read(p: String) -> Result<String, CommandError> {
    Ok(p)
}

fn main() {}
