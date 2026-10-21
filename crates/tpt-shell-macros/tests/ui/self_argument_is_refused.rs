// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

#![allow(dead_code)]
use tpt_shell::{command, CommandError};

struct App;

impl App {
    #[command]
    fn scan(&self) -> Result<u32, CommandError> {
        Ok(1)
    }
}

fn main() {}
