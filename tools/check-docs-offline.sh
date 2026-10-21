#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Checks that a built mdBook site loads nothing from the network (spec §15, todo: "Offline docs
# bundle verified"). Links in the prose may point to the web; resources the page loads (scripts,
# stylesheets, images, fonts, CSS imports) may not. Run it after `mdbook build`.
#
# usage: tools/check-docs-offline.sh [book-directory]

set -euo pipefail

book="${1:-$(dirname "$0")/../docs/book}"
if [ ! -d "$book" ]; then
  echo "no built book at $book; run mdbook build first" >&2
  exit 1
fi

# Resource-loading tags and CSS, on one line each. mdBook writes these tags on single lines.
patterns=(
  '<(script|img|iframe|link|source|embed|object)[^>]*(src|href|data)="?https?://'
  '@import[^;]*https?://'
  'url\((["'"'"']?)https?://'
  'fetch\((["'"'"'])https?://'
)

found=0
for pattern in "${patterns[@]}"; do
  if matches=$(grep -rEn --include='*.html' --include='*.css' --include='*.js' -e "$pattern" "$book" 2>/dev/null); then
    echo "$matches"
    found=1
  fi
done

if [ "$found" -ne 0 ]; then
  echo "the book loads resources from the network (listed above)" >&2
  exit 1
fi

# A preprocessor that fails can leave its placeholder in the page. Treat that as a failure too.
if leftovers=$(grep -rln '{{#' "$book" --include=*.html 2>/dev/null); then
  echo "$leftovers"
  echo "unrendered mdBook placeholders in the pages listed above (a preprocessor did not run)" >&2
  exit 1
fi

pages=$(find "$book" -name '*.html' | wc -l | tr -d ' ')
echo "offline check passed: $pages page(s) load no network resources, no placeholders left"
