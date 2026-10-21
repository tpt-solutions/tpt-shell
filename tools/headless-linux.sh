#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Runs a command under a headless display on Linux (spec §15: Xvfb, and Wayland headless where
# possible).
#
#   xvfb     runs the command under Xvfb with xvfb-run. This is the full setup.
#   wayland  starts a headless Weston compositor, points WAYLAND_DISPLAY at it and forces GTK to
#            use Wayland. PARTIAL: it proves that a Wayland client can connect and draw, not that
#            a user's compositor behaves the same. The GTK backend (M5) is the first real user.
#
# usage: tools/headless-linux.sh xvfb|wayland <command> [arguments...]

set -euo pipefail

usage() {
  echo "usage: tools/headless-linux.sh xvfb|wayland <command> [arguments...]" >&2
  exit 2
}

[ "$#" -ge 2 ] || usage
mode="$1"
shift

case "$mode" in
  xvfb)
    if ! command -v xvfb-run >/dev/null 2>&1; then
      echo "xvfb-run is not installed (apt install xvfb xauth)" >&2
      exit 1
    fi
    exec xvfb-run -a -s "-screen 0 1280x1024x24" "$@"
    ;;
  wayland)
    if ! command -v weston >/dev/null 2>&1; then
      echo "weston is not installed (apt install weston)" >&2
      exit 1
    fi
    runtime="$(mktemp -d)"
    chmod 700 "$runtime"
    socket="tpt-shell-test"
    log="$runtime/weston.log"
    trap 'kill "${weston_pid:-0}" 2>/dev/null || true; rm -rf "$runtime"' EXIT
    XDG_RUNTIME_DIR="$runtime" weston --backend=headless --socket="$socket" --idle-time=0 \
      >"$log" 2>&1 &
    weston_pid=$!
    for _ in $(seq 1 50); do
      [ -S "$runtime/$socket" ] && break
      sleep 0.2
    done
    if [ ! -S "$runtime/$socket" ]; then
      cat "$log" >&2
      echo "weston did not create the Wayland socket" >&2
      exit 1
    fi
    export XDG_RUNTIME_DIR="$runtime"
    export WAYLAND_DISPLAY="$socket"
    export GDK_BACKEND=wayland
    "$@"
    ;;
  *)
    usage
    ;;
esac
