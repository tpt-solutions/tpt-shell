// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::{command, CommandError};
use tpt_shell::__private::ipc::jobs::JobContext;

#[command]
fn scan(job: &JobContext) -> Result<u32, CommandError> {
    Ok(1)
}

fn main() {}
