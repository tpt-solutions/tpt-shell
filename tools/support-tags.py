#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Renders the support tags of the public API (spec §3, CONTRIBUTING "Code rules") as a markdown
# table. Stdlib only, so it runs as `python3 -I tools/support-tags.py`.
#
# A support tag is the first line of a doc block that starts with `Support:` followed by one of
# Full, Partial or Unsupported. The rest of the block (continuation lines included) is the gap
# or note:
#   /// Support: Full.
#   /// Support: Partial. Global hotkeys are X11 only, and the GTK
#   /// backend does not run them under Wayland.
#   //! Support: Unsupported
# A tag on a `///` block applies to the `pub` item below it. A tag in a `//!` block at the top of
# a file applies to the file. A `Support:` line that does not start with one of the three words is
# listed as malformed, so the table never guesses.
#
# Modes:
#   support-tags.py --render [repo-root]   print the table and a summary
#   support-tags.py                        mdBook preprocessor (reads the book on stdin and fills
#                                          {{#support-tags}} in each page)
#   support-tags.py supports html          mdBook asks whether the renderer is supported

import json
import os
import re
import sys

PLACEHOLDER = "{{#support-tags}}"
DOC_LINE = re.compile(r"^\s*//[/!]")
SUPPORT_START = re.compile(r"^Support:\s*(.*)$")
SUPPORT_WORD = re.compile(r"^(Full|Partial|Unsupported)\b[.:;,]?\s*(.*)$")
PUB_ITEM = re.compile(
    r"^\s*pub\s+(?:unsafe\s+|async\s+|const\s+)?(fn|struct|enum|trait|type|const|static|mod)\s+([A-Za-z_][A-Za-z0-9_]*)"
)


def repo_root_from_here():
    return os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))


def rust_files(root):
    for dirpath, dirnames, filenames in os.walk(os.path.join(root, "crates")):
        dirnames[:] = [d for d in dirnames if d not in ("target", "tests")]
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def doc_blocks(lines):
    """Yields (start, end) for each run of consecutive doc-comment lines."""
    i = 0
    while i < len(lines):
        if DOC_LINE.match(lines[i]):
            j = i
            while j < len(lines) and DOC_LINE.match(lines[j]):
                j += 1
            yield i, j
            i = j
        else:
            i += 1


def block_text(lines, start, end):
    # Both `///` and `//!` are three characters long.
    return [line.strip()[3:].strip() for line in lines[start:end]]


def tag_of_block(lines, start, end):
    """Returns ("ok", tag, gap), ("bad", text) or None when the block has no Support paragraph.

    The tag starts a paragraph of the block (a line after an empty doc line counts as a new
    paragraph). Its gap runs to the end of that paragraph.
    """
    texts = block_text(lines, start, end)
    for k, text in enumerate(texts):
        first = SUPPORT_START.match(text)
        if first:
            break
    else:
        return None
    rest = [first.group(1)]
    for text in texts[k + 1 :]:
        if not text:
            break
        rest.append(text)
    body = " ".join(rest).strip()
    parsed = SUPPORT_WORD.match(body)
    if not parsed:
        return ("bad", body)
    return ("ok", parsed.group(1), parsed.group(2).strip())


def scan_file(root, path):
    """Returns (tagged, malformed, untagged) for one file. Tagged rows are (crate, where, tag, gap)."""
    rel = os.path.relpath(path, os.path.join(root, "crates")).replace(os.sep, "/")
    crate = rel.split("/")[0]
    with open(path, encoding="utf-8") as handle:
        lines = handle.read().splitlines()

    tagged, malformed, untagged = [], [], []
    blocks_ending_at = {}
    first_code = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if first_code is None and stripped and not stripped.startswith("//"):
            first_code = index

    for start, end in doc_blocks(lines):
        blocks_ending_at[end] = (start, end)
        if first_code is not None and end > first_code:
            continue  # inner docs inside an item are not file headers
        found = tag_of_block(lines, start, end)
        if found is None:
            continue
        where = f"{rel}:{start + 1}"
        if found[0] == "bad":
            malformed.append((where, found[1]))
        elif lines[start].lstrip().startswith("//!"):
            tagged.append((crate, rel, found[1], found[2]))

    for index, line in enumerate(lines):
        item = PUB_ITEM.match(line)
        if not item:
            continue
        j = index - 1
        while j >= 0 and (not lines[j].strip() or lines[j].strip().startswith("#[")):
            j -= 1
        label = f"{rel}: {item.group(1)} {item.group(2)}"
        block = blocks_ending_at.get(j + 1)
        found = tag_of_block(lines, block[0], block[1]) if block else None
        if found and found[0] == "ok":
            tagged.append((crate, label, found[1], found[2]))
        elif found and found[0] == "bad":
            malformed.append((f"{rel}:{block[0] + 1}", found[1]))
        else:
            untagged.append((crate, label, f"{rel}:{index + 1}"))
    return tagged, malformed, untagged


def scan(root):
    tagged, malformed, untagged = [], [], []
    for path in rust_files(root):
        t, m, u = scan_file(root, path)
        tagged += t
        malformed += m
        untagged += u
    return tagged, malformed, untagged


def render(root):
    tagged, malformed, untagged = scan(root)
    out = ["| Crate | Item | Support | Gap or note |", "|---|---|---|---|"]
    for crate, where, tag, gap in tagged:
        gap = gap.replace("|", "\\|")
        out.append(f"| {crate} | `{where}` | {tag} | {gap} |")
    if not tagged:
        out.append("| - | - | - | No tagged items yet |")
    out.append("")
    counts = {"Full": 0, "Partial": 0, "Unsupported": 0}
    for _, _, tag, _ in tagged:
        counts[tag] += 1
    out.append(
        f"Tagged: {len(tagged)} (Full {counts['Full']}, Partial {counts['Partial']}, "
        f"Unsupported {counts['Unsupported']}). Public items without a tag: {len(untagged)}."
    )
    if malformed:
        out.append("")
        out.append("Malformed support lines (fix the wording):")
        for where, text in malformed:
            out.append(f"- `{where}`: {text}")
    return "\n".join(out) + "\n"


def preprocess(payload):
    _context, book = payload
    table = render(repo_root_from_here())

    def walk(items):
        for item in items:
            if isinstance(item, dict) and "Chapter" in item:
                chapter = item["Chapter"]
                if PLACEHOLDER in chapter.get("content", ""):
                    chapter["content"] = chapter["content"].replace(PLACEHOLDER, table)
                walk(chapter.get("sub_items", []))

    walk(book.get("sections", book.get("items", [])))
    return book


def main(argv):
    if argv[:1] == ["supports"]:
        return 0 if argv[1:2] == ["html"] else 1
    if not argv:
        json.dump(preprocess(json.load(sys.stdin)), sys.stdout)
        return 0
    if argv[0] == "--render":
        root = os.path.abspath(argv[1]) if len(argv) > 1 else repo_root_from_here()
        sys.stdout.write(render(root))
        return 0
    sys.stderr.write("usage: support-tags.py --render [repo-root]\n")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
