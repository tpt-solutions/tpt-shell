#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Checks that the JavaScript runtime in js/runtime/ stays within its size budget (spec §7.2):
# under 4 KB gzipped in total. Only needs gzip.
#
# usage: tools/check-runtime-size.sh [runtime-directory]

set -euo pipefail

budget=4096
dir="${1:-$(dirname "$0")/../js/runtime}"

total=0
for file in "$dir"/*.js; do
  size=$(gzip -9 -n -c "$file" | wc -c)
  echo "$size bytes gzipped  $(basename "$file")"
  total=$((total + size))
done

if [ "$total" -gt "$budget" ]; then
  echo "runtime is $total bytes gzipped, over the $budget byte budget" >&2
  exit 1
fi
echo "runtime is $total bytes gzipped, within the $budget byte budget"
