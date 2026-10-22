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

# A passing test's output stays in a collapsed group. A failing test's output is printed in the
# open (GitHub hides group contents in a pasted log) and raised as an error annotation, and the
# names of all failures are repeated at the end.
failed=0
failed_names=""
first_crash=""
gdb_symbols_done=0
out="$(mktemp)"
while IFS= read -r name; do
  "$bin" --exact "$name" --nocapture >"$out" 2>&1
  rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "::group::$name"
    cat "$out"
    echo "RESULT: $name ok"
    echo "::endgroup::"
    continue
  fi
  failed=$((failed + 1))
  failed_names="$failed_names $name"
  if grep -q "test result: ok. 1 passed" "$out"; then
    # The test body finished and the harness reported success; the process died afterwards, in an
    # exit-time handler (a WebKit object torn down by atexit). A real failure, but a different one
    # from a test that aborts in its body.
    label="PASSED, THEN ABORTED AT PROCESS EXIT"
  else
    label="FAILED"
  fi
  echo "RESULT: $name $label (exit code $rc)"
  tail -n 80 "$out"
  # The first GLib, GTK or WebKit complaint, for the annotation and for a quick read.
  first="$(grep -m1 -E 'ERROR|CRITICAL|WARNING|panicked|assert|Error' "$out" | cut -c1-300)"
  echo "::error title=$name $label (exit code $rc)::${first:-no message}"
  if [ "$rc" -ge 128 ] || grep -q "test result: ok. 1 passed" "$out"; then
    [ -n "$first_crash" ] || first_crash="$name"
    echo "native backtrace follows"
    if command -v gdb >/dev/null 2>&1; then
      if [ "$gdb_symbols_done" -eq 0 ]; then
        # Only the first crash pays for debug symbols: gdb fetches them from debuginfod, which is
        # slow, so the whole gdb run is capped. Later crashes get the cheap backtrace.
        gdb_symbols_done=1
        echo "(first crash: resolving symbols through debuginfod, capped at 8 minutes)"
        DEBUGINFOD_URLS="https://debuginfod.ubuntu.com" G_MESSAGES_DEBUG=all \
          timeout 480 gdb -batch -ex "set pagination off" -ex "set debuginfod enabled on" \
          -ex run -ex "bt full" -ex "thread apply all bt" \
          --args "$bin" --exact "$name" --nocapture >"$out.gdb" 2>&1
        frame_count=14
      else
        G_MESSAGES_DEBUG=all gdb -batch -ex "set pagination off" -ex run -ex "bt full" \
          -ex "thread apply all bt" --args "$bin" --exact "$name" --nocapture >"$out.gdb" 2>&1
        frame_count=25
      fi
      tail -n 200 "$out.gdb"
      # The stack frames of the crashing thread and the GLib messages, as one annotation, so the
      # cause can be read without opening the log.
      frames="$(grep -E '^#[0-9]+ ' "$out.gdb" | head -n "$frame_count" | cut -c1-200 | sed 's/%/%25/g' | paste -sd '|')"
      glib="$(grep -E 'GLib|Gtk|Gdk|WebKit|\*\*' "$out.gdb" | head -n 6 | cut -c1-200 | sed 's/%/%25/g' | paste -sd '|')"
      echo "::error title=$name backtrace::frames: ${frames:-none} || messages: ${glib:-none}"
    else
      echo "gdb is not installed"
    fi
  fi
done < <("$bin" --list 2>/dev/null | sed -n 's/: test$//p')
rm -f "$out" "$out.gdb"

# A cheap experiment that needs no symbols: run the first crashing test again with different
# WebKit environment settings and with an explicit WebKit context owned by the adapter
# (TPT_WEBKIT_CONTEXT=own), and say which, if any, removes the abort.
if [ -n "$first_crash" ]; then
  echo "== experiment on $first_crash =="
  results=""
  run_variant() {
    label="$1"
    shift
    env "$@" timeout 120 "$bin" --exact "$first_crash" --nocapture >"$out.exp" 2>&1
    vrc=$?
    if [ "$vrc" -eq 0 ]; then
      verdict="no abort (exit 0)"
    else
      verdict="exit $vrc"
    fi
    results="$results$(printf '%-40s %s' "$label" "$verdict")
"
  }
  run_variant "baseline (job environment)" TPT_X=1
  run_variant "compositing off only" -u WEBKIT_DISABLE_DMABUF_RENDERER
  run_variant "dmabuf off only" -u WEBKIT_DISABLE_COMPOSITING_MODE
  run_variant "neither switch" -u WEBKIT_DISABLE_COMPOSITING_MODE -u WEBKIT_DISABLE_DMABUF_RENDERER
  run_variant "own WebKit context (adapter-owned)" TPT_WEBKIT_CONTEXT=own
  echo "variant                                  result"
  printf '%s' "$results"
  summary="$(printf '%s' "$results" | sed 's/  */ /g' | paste -sd '|')"
  echo "::notice title=exit-abort experiment on $first_crash::$summary"
  rm -f "$out.exp"
fi

echo "tests that failed: $failed"
[ -z "$failed_names" ] || echo "failed:$failed_names"
[ "$failed" -eq 0 ]
