#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475

"""Rename the project: every "logbook" in file contents and paths.

"Logbook" is a working name. Once the final one is chosen:

    python tools/rename.py <new-name>      # e.g. python tools/rename.py almanac

It replaces logbook / Logbook / LOGBOOK (and logbook_ / logbook- forms, which
are the same word) in every tracked text file and renames tracked paths that
contain it, then prints what changed. Review the diff, run `python check.py`,
and fix by hand what reads oddly (prose such as "a logbook of ...").

Names that are a contract with the tracker or that installed copies of
irminsul already stored (`irminsul` OAuth client, `/irminsul/*` routes, the
`irminsul` credential, `Irminsul` app id, task and GOOD source) contain no
"logbook" and are left alone.
"""

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main():
    if len(sys.argv) != 2 or not sys.argv[1].isidentifier() or not sys.argv[1].islower():
        sys.exit("usage: rename.py <new-name>   (lowercase letters, digits, underscores)")
    new = sys.argv[1]
    pairs = [("LOGBOOK", new.upper()), ("Logbook", new[0].upper() + new[1:]), ("logbook", new)]

    tracked = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, check=True
    ).stdout.decode("utf-8").split("\0")
    changed = 0
    for rel in filter(None, tracked):
        path = ROOT / rel
        if path.resolve() == Path(__file__).resolve():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            continue
        updated = text
        for old, repl in pairs:
            updated = updated.replace(old, repl)
        if updated != text:
            path.write_bytes(updated.encode("utf-8"))
            changed += 1

    moved = 0
    for rel in sorted(filter(None, tracked), key=len, reverse=True):
        target = rel
        for old, repl in pairs:
            target = target.replace(old, repl)
        if target != rel and (ROOT / rel).exists():
            (ROOT / target).parent.mkdir(parents=True, exist_ok=True)
            subprocess.run(["git", "-C", str(ROOT), "mv", rel, target], check=True)
            moved += 1
    print(f"{changed} files edited, {moved} paths renamed")


if __name__ == "__main__":
    main()
