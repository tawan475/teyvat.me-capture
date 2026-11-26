#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475

"""Compare our dispatch keys with the public list and name any we lack.

The game encrypts its login exchange with a "dispatch key" picked by a 16-bit
id; `apps/desktop/keys/gi.json` maps each id to its 4,096-byte key (base64).
A game update can bring a new id, and a login under an id we lack decrypts
nothing. The public list we compare with is `keys/gi.json` in
juliuskreutz/stardb-exporter, read with one plain GET (read-only: nothing is
ever sent there).

.github/workflows/keys-watch.yml runs this once a day and opens one issue
(label `dispatch-keys`) when it finds a difference.

Differences reported: ids the public list has and we lack, and ids both have
with different keys. Ids only we have are fine (2701, the Bilibili server's,
is one). Output names ids only, never key bytes.

Exit codes: 0 = nothing to do, 3 = differences found, 2 = could not check
(network error, malformed table). Any other code is a crash: also unchecked.

Usage: python .github/scripts/keys_watch.py [--ours PATH] [--report PATH]
  --report writes the issue text (Markdown) when there are differences.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import json
import re
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

PUBLIC_URL = "https://raw.githubusercontent.com/juliuskreutz/stardb-exporter/HEAD/keys/gi.json"
PUBLIC_PAGE = "https://github.com/juliuskreutz/stardb-exporter/blob/HEAD/keys/gi.json"
OURS = Path(__file__).resolve().parents[2] / "apps" / "desktop" / "keys" / "gi.json"
USER_AGENT = "keys-watch"
KEY_LEN = 4096
MAX_ID = 65535

EXIT_NOTHING = 0
EXIT_UNCHECKED = 2
EXIT_DIFFERENCES = 3

# A canonical decimal id: no sign, no leading zero, no spaces.
ID_TEXT = re.compile(r"0|[1-9][0-9]{0,4}")


class TableError(Exception):
    """A table that cannot be read as a keys table. Its message never holds key
    material."""


def parse_table(raw: bytes | str, name: str) -> dict[int, bytes]:
    """A keys table: a JSON object of decimal ids (0-65535) to base64 text that
    decodes to exactly KEY_LEN bytes. Anything else raises TableError."""
    if isinstance(raw, bytes):
        try:
            raw = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            raise TableError(f"{name}: not UTF-8 text") from error

    def no_duplicates(pairs: list) -> dict:
        seen: dict = {}
        for key, value in pairs:
            if key in seen:
                shown = key if ID_TEXT.fullmatch(key) else "(not shown)"
                raise TableError(f"{name}: id {shown} appears twice")
            seen[key] = value
        return seen

    try:
        data = json.loads(raw, object_pairs_hook=no_duplicates)
    except json.JSONDecodeError as error:
        # The position only: the message's own text quotes nothing of the input.
        raise TableError(f"{name}: not JSON (line {error.lineno}, column {error.colno})") from None
    if not isinstance(data, dict):
        raise TableError(f"{name}: not a JSON object")
    if not data:
        raise TableError(f"{name}: holds no keys")

    table: dict[int, bytes] = {}
    for key, value in data.items():
        label = key if ID_TEXT.fullmatch(key) else "(not shown)"
        if not ID_TEXT.fullmatch(key) or int(key) > MAX_ID:
            # An id that is not an id could be anything: never echo more than its length.
            raise TableError(f"{name}: an entry's id is not a decimal 0-{MAX_ID} ({len(key)} chars)")
        if not isinstance(value, str):
            raise TableError(f"{name}: id {label}: the key is not a string")
        try:
            decoded = base64.b64decode(value, validate=True)
        except (binascii.Error, ValueError):
            raise TableError(f"{name}: id {label}: the key is not base64") from None
        if len(decoded) != KEY_LEN:
            raise TableError(f"{name}: id {label}: the key is {len(decoded)} bytes, not {KEY_LEN}")
        table[int(key)] = decoded
    return table


@dataclass(frozen=True)
class Comparison:
    missing: list[int]  # the public list has them, we don't
    different: list[int]  # both have them, the bytes differ
    only_ours: list[int]  # fine

    @property
    def has_differences(self) -> bool:
        return bool(self.missing or self.different)


def compare(ours: dict[int, bytes], public: dict[int, bytes]) -> Comparison:
    return Comparison(
        missing=sorted(set(public) - set(ours)),
        different=sorted(i for i in set(public) & set(ours) if public[i] != ours[i]),
        only_ours=sorted(set(ours) - set(public)),
    )


def ids(values: list[int]) -> str:
    return ", ".join(str(value) for value in values) or "none"


def summary(result: Comparison) -> list[str]:
    return [
        f"ids the public list has and we lack: {ids(result.missing)}",
        f"ids both have with different keys: {ids(result.different)}",
        f"ids only we have (fine): {ids(result.only_ours)}",
    ]


def report(result: Comparison) -> str:
    """The issue text: ids only."""
    lines = [
        "The public dispatch key list differs from `apps/desktop/keys/gi.json`.",
        "",
    ]
    if result.missing:
        lines.append(f"- Ids the public list has and we lack: {ids(result.missing)}")
    if result.different:
        lines.append(f"- Ids both have with different keys: {ids(result.different)}")
    lines += [
        "",
        "A login under a missing id decrypts nothing. Check the ids against the",
        f"public list ({PUBLIC_PAGE}), add or update the entries in",
        "`apps/desktop/keys/gi.json` (each key's first two bytes, big-endian, are",
        "its id), and release.",
        "",
        "Opened by `.github/workflows/keys-watch.yml`.",
    ]
    return "\n".join(lines) + "\n"


# `fetch`'s signature: (url) -> body bytes; raises OSError or ValueError on failure.
Fetch = Callable[[str], bytes]


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        if response.status != 200:
            raise ValueError(f"GET {url} answered {response.status}")
        return response.read()


def run(ours_path: Path, get: Fetch = fetch, report_path: Path | None = None, out=None) -> int:
    out = out or sys.stdout
    try:
        ours = parse_table(ours_path.read_bytes(), "our keys")
    except OSError as error:
        print(f"could not check: our keys: {error.strerror or error}", file=out)
        return EXIT_UNCHECKED
    except TableError as error:
        print(f"could not check: {error}", file=out)
        return EXIT_UNCHECKED
    try:
        public = parse_table(get(PUBLIC_URL), "the public list")
    except urllib.error.HTTPError as error:
        print(f"could not check: GET {PUBLIC_URL} failed with {error.code}", file=out)
        return EXIT_UNCHECKED
    except (OSError, ValueError) as error:
        # URLError and timeouts are OSErrors; their text names the host, not data.
        print(f"could not check: GET {PUBLIC_URL} failed: {type(error).__name__}", file=out)
        return EXIT_UNCHECKED
    except TableError as error:
        print(f"could not check: {error}", file=out)
        return EXIT_UNCHECKED

    result = compare(ours, public)
    print(f"checked {len(ours)} of our keys against {len(public)} public ones", file=out)
    for line in summary(result):
        print(line, file=out)
    if not result.has_differences:
        print("nothing to do", file=out)
        return EXIT_NOTHING
    print("differences found", file=out)
    if report_path is not None:
        report_path.write_text(report(result), encoding="utf-8")
    return EXIT_DIFFERENCES


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ours", type=Path, default=OURS, help="our keys table")
    parser.add_argument("--report", type=Path, help="write the issue text here on differences")
    args = parser.parse_args(argv)
    try:
        return run(args.ours, report_path=args.report)
    except Exception as error:  # noqa: BLE001 - an unexpected failure is "not checked", never silence
        print(f"could not check: {type(error).__name__}", file=sys.stdout)
        return EXIT_UNCHECKED


if __name__ == "__main__":
    sys.exit(main())
