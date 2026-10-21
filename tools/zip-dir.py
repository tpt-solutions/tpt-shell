#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Zips a directory with its own name as the single top-level folder (spec §10.8, bundle zip).
# Stdlib only. Entries are sorted and carry a fixed timestamp (1980-01-01), so the same tree
# always gives the same bytes. Run it as `python3 -I tools/zip-dir.py <directory> <output.zip>`.

import os
import sys
import zipfile

FIXED_TIME = (1980, 1, 1, 0, 0, 0)


def main(argv):
    if len(argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    src = os.path.abspath(argv[1])
    out = os.path.abspath(argv[2])
    if not os.path.isdir(src):
        print(f"not a directory: {src}", file=sys.stderr)
        return 1
    base = os.path.dirname(src)
    files = []
    for root, dirs, names in os.walk(src):
        dirs.sort()
        for name in sorted(names):
            files.append(os.path.join(root, name))
    tmp = out + ".tmp"
    with zipfile.ZipFile(tmp, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            arc = os.path.relpath(path, base).replace(os.sep, "/")
            info = zipfile.ZipInfo(arc, date_time=FIXED_TIME)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (os.stat(path).st_mode & 0xFFFF) << 16
            with open(path, "rb") as handle:
                archive.writestr(info, handle.read())
    os.replace(tmp, out)
    print(f"wrote {len(files)} files to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
