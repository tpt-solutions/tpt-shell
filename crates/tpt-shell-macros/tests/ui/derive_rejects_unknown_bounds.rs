// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
#![allow(unused_imports, dead_code, unused_variables)]

use tpt_shell::Contract;

#[derive(Contract)]
struct Entry {
    #[contract(maxlen = 3)]
    name: String,
}

fn main() {}
