#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Runs every test of tpt-shell-platform-linux in its own process, so a native abort (SIGABRT from
# GTK, WebKit or GLib, which kills the whole test process without a Rust message) does not hide
# the other tests. A test that dies from a signal is run again under gdb to print a native
# backtrace, with GLib messages switched on. Meant to run inside tools/headless-linux.sh.
#
# usage: tools/linux-adapter-tests.sh        (from the workspace root)

set -uo pipefail

# A session bus silences "Cannot autolaunch D-Bus" and lets GTK and WebKit find their services.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ -z "${TPT_IN_DBUS_SESSION:-}" ] \
  && command -v dbus-run-session >/dev/null 2>&1; then
  export TPT_IN_DBUS_SESSION=1
  exec dbus-run-session -- bash "$0" "$@"
fi

bin="$(cargo test -p tpt-shell-platform-linux --no-run --message-format=json 2>/dev/null \
  | python3 -c '
import json, sys
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "tpt_shell_platform_linux" and m.get("profile", {}).get("test"):
        print(m["executable"])
')"
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
  echo "could not find the test binary" >&2
  exit 1
fi
echo "test binary: $bin"

failed=0
while IFS= read -r name; do
  echo "::group::$name"
  "$bin" --exact "$name" --nocapture
  rc=$?
  if [ "$rc" -ne 0 ]; then
    failed=$((failed + 1))
    echo "RESULT: $name FAILED (exit code $rc)"
    if [ "$rc" -ge 128 ]; then
      echo "killed by signal $((rc - 128)); native backtrace follows"
      if command -v gdb >/dev/null 2>&1; then
        G_MESSAGES_DEBUG=all gdb -batch -ex "set pagination off" -ex run -ex "bt full" \
          -ex "thread apply all bt" --args "$bin" --exact "$name" --nocapture 2>&1 | tail -n 200
      else
        echo "gdb is not installed"
      fi
    fi
  else
    echo "RESULT: $name ok"
  fi
  echo "::endgroup::"
done < <("$bin" --list 2>/dev/null | sed -n 's/: test$//p')

echo "tests that failed: $failed"
[ "$failed" -eq 0 ]
