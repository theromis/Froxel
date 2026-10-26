#!/usr/bin/env python3
"""Print a version's CHANGELOG.md section as plain text for the stores.

    tools/release_notes.py 0.3.1

Headings are dropped, list items become "• " lines with their wrapped lines
joined, and links, backticks, and bold marks are reduced to their text. A
version without a section prints nothing.
"""

import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parents[1]
version = sys.argv[1].lstrip("v")
section, keep = [], False
for line in (root / "CHANGELOG.md").read_text().splitlines():
    if line.startswith("## "):
        keep = (line[3:].split() or [""])[0] == version
        continue
    if keep:
        section.append(line)

items = []
for line in section:
    if line.startswith("- "):
        items.append(line[2:].strip())
    elif line.startswith("  ") and items:
        items[-1] += " " + line.strip()
for item in items:
    item = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", item)
    print("• " + re.sub(r"\*\*|`", "", item))
