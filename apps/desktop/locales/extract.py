# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475
"""Lists every tr("…") / trf("…") text in apps/desktop/src into en.json.

Run after adding or changing a text: `python apps/desktop/locales/extract.py`.
The other languages' files keep their entries; a text the code no longer has
is dropped from them (the test in src/i18n.rs refuses stale ones), and a new
text shows in English until it is translated.
"""

import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE.parent / "src"
# rustfmt may break a long call after `tr(`, so whitespace may follow it.
CALL = re.compile(r'(?<![A-Za-z0-9_])trf?\(\s*"((?:[^"\\]|\\.)*)"')


def unescape(text: str) -> str:
    out, i = [], 0
    while i < len(text):
        c = text[i]
        if c == "\\" and i + 1 < len(text):
            nxt = text[i + 1]
            out.append("\n" if nxt == "n" else nxt)
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def texts() -> list[str]:
    found = set()
    for path in sorted(SRC.rglob("*.rs")):
        for match in CALL.finditer(path.read_text(encoding="utf-8")):
            found.add(unescape(match.group(1)))
    found.discard("…")
    return sorted(found)


def write(path: Path, data: dict) -> None:
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def main() -> None:
    used = texts()
    write(HERE / "en.json", {t: t for t in used})
    keep = set(used)
    for path in sorted(HERE.glob("*.json")):
        if path.name == "en.json":
            continue
        data = json.loads(path.read_text(encoding="utf-8") or "{}")
        write(path, {k: v for k, v in sorted(data.items()) if k in keep})
    print(f"{len(used)} texts")


if __name__ == "__main__":
    main()
