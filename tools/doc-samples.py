#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Compiles the Rust code samples in the docs (spec §14, "Docs tests: code samples in docs are
# compiled"). Stdlib only: `python3 -I tools/doc-samples.py [output-directory]`.
#
# A sample is a fenced block in docs/src/**/*.md whose info string has both `rust` and
# `compile-check`, for example:
#   ```rust,compile-check
#   use tpt_shell::App;
#   fn main() {}
#   ```
# Each sample becomes one example in a scratch crate outside the workspace, which depends on the
# facade crate by path. A sample without `fn main` gets an empty one appended, so it can hold
# items only. Blocks marked `rust` alone are prose and are not compiled.
#
# The scratch crate is written to the output directory (default target/doc-samples). Its
# Cargo.lock is copied from the workspace, so the samples build against the same versions.
# Exit status: 0 when every sample compiles or there are none; 1 otherwise.

import os
import re
import shutil
import subprocess
import sys

FENCE = re.compile(r"^(\s*)```(.*)$")


def repo_root():
    return os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))


def find_samples(docs_src):
    """Yields (path, line_number, source) for each compile-check block."""
    for dirpath, dirnames, filenames in os.walk(docs_src):
        dirnames.sort()
        for name in sorted(filenames):
            if not name.endswith(".md"):
                continue
            path = os.path.join(dirpath, name)
            with open(path, encoding="utf-8") as handle:
                lines = handle.read().splitlines()
            index = 0
            while index < len(lines):
                opened = FENCE.match(lines[index])
                if not opened or not opened.group(2).strip():
                    index += 1
                    continue
                tokens = re.split(r"[,\s]+", opened.group(2).strip())
                start = index + 1
                index = start
                while index < len(lines) and not FENCE.match(lines[index]):
                    index += 1
                if "rust" in tokens and "compile-check" in tokens:
                    yield path, start, "\n".join(lines[start:index]) + "\n"
                index += 1


def write_crate(out, samples, root):
    examples = os.path.join(out, "examples")
    if os.path.isdir(examples):
        shutil.rmtree(examples)
    os.makedirs(examples)
    facade = os.path.join(root, "crates", "tpt-shell").replace(os.sep, "/")
    with open(os.path.join(out, "Cargo.toml"), "w", encoding="utf-8") as handle:
        handle.write(
            "# Written by tools/doc-samples.py. Not part of the workspace.\n"
            "[package]\n"
            'name = "tpt-shell-doc-samples"\n'
            'version = "0.0.0"\n'
            'edition = "2021"\n'
            "publish = false\n\n"
            "[workspace]\n\n"
            "[dependencies]\n"
            f'tpt-shell = {{ path = "{facade}" }}\n'
        )
    lock = os.path.join(root, "Cargo.lock")
    if os.path.exists(lock):
        shutil.copyfile(lock, os.path.join(out, "Cargo.lock"))
    index = []
    for number, (path, line, source) in enumerate(samples, start=1):
        name = f"sample{number:03d}"
        if not re.search(r"\bfn\s+main\s*\(", source):
            source += "\nfn main() {}\n"
        # A sample often shows one function of several; the others are not an error.
        source = "#![allow(dead_code, unused)]\n" + source
        with open(os.path.join(examples, name + ".rs"), "w", encoding="utf-8") as handle:
            handle.write(source)
        rel = os.path.relpath(path, root).replace(os.sep, "/")
        index.append((name, f"{rel}:{line}"))
    return index


def main(argv):
    root = repo_root()
    out = os.path.abspath(argv[0]) if argv else os.path.join(root, "target", "doc-samples")
    os.makedirs(out, exist_ok=True)
    samples = list(find_samples(os.path.join(root, "docs", "src")))
    if not samples:
        print("no compile-check samples in docs/src yet; nothing to compile")
        return 0

    index = write_crate(out, samples, root)
    for name, where in index:
        print(f"{name}: {where}")
    build = subprocess.run(
        ["cargo", "build", "--examples", "--manifest-path", os.path.join(out, "Cargo.toml")],
        check=False,
    )
    if build.returncode != 0:
        print("a doc sample does not compile; the names above map to the docs lines", file=sys.stderr)
        return 1
    print(f"{len(index)} doc sample(s) compile")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
