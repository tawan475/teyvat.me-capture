# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475
"""Refresh game-data/: the pinned game data files the build scripts read.

    python game-data/pin.py --from <folder>       # a client data set's folder (the decoder's output)
    python game-data/pin.py --release latest      # or one released in genshin-game-data (needs `gh`)
    python game-data/pin.py --release client-7.1.0-ea10d22a

A client data set is the game's own data files, decoded from the installed
client into Dimbreath's file layout (ExcelBinOutput/, TextMap/,
BinOutput/Scene/Point/); genshin-game-data publishes each as a release. This
keeps the files `teyvatme-game-data` reads (FILES, plus the point file of
every scene WorldAreaConfigData names), with TextMap_MediumEN cut down to the
text hashes the kept tables mention, each gzipped reproducibly (same input,
same bytes) as `game-data/<path>.gz`, and writes game-data/manifest.json with
the data set's id. Files the new data set no longer has are removed.

The data set id is genshin-game-data's (scripts/lib/client-data.ts): 40 hex
digits of the SHA-256 over the sorted `path<TAB>sha256` lines of all its
ExcelBinOutput/, TextMap/ and BinOutput/ JSON files.
"""

import argparse
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANIFEST = "manifest.json"
DATA_FOLDERS = ("ExcelBinOutput", "TextMap", "BinOutput")
RELEASE_REPO = os.environ.get("TEYVATME_GAME_DATA_REPO", "tawan475/genshin-game-data")

# What crates/game-data reads (lib.rs `get_json_file`), the exploration tables, and tables whose ids tell
# collection messages apart.
EXCEL = [
    "AvatarExcelConfigData",
    "AvatarSkillDepotExcelConfigData",
    "AvatarSkillExcelConfigData",
    "ConstValueExcelConfigData",
    "EquipAffixExcelConfigData",
    "MaterialExcelConfigData",
    "ReliquaryAffixExcelConfigData",
    "ReliquaryExcelConfigData",
    "ReliquaryMainPropExcelConfigData",
    "ReliquarySetExcelConfigData",
    "WeaponExcelConfigData",
    "CityConfigData",
    "ReputationCityExcelConfigData",
    "OfferingOpenStateConfigData",
    "WorldAreaConfigData",
    # Told apart by their ids: recipes (CookDataNotify has shape twins) and Aranara collections.
    "CookRecipeExcelConfigData",
    "AranaraCollectionExcelConfigData",
    # The crafting, forging and processing unlock lists are lone id lists: told apart by these ids.
    "CombineExcelConfigData",
    "ForgeExcelConfigData",
    "CompoundExcelConfigData",
]
TEXT_MAP = "TextMap/TextMap_MediumEN.json"
WORLD_AREAS = "ExcelBinOutput/WorldAreaConfigData.json"
POINTS = "BinOutput/Scene/Point/scene{}_point.json"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def data_set_id(root):
    """genshin-game-data's id of the data set in `root` (every JSON file of DATA_FOLDERS)."""
    lines = []
    for folder in DATA_FOLDERS:
        base = root / folder
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.json")):
            lines.append((path.relative_to(root).as_posix(), sha256(path.read_bytes())))
    if not lines:
        raise SystemExit(f"{root} holds no {', '.join(DATA_FOLDERS)} files")
    text = "".join(f"{p}\t{h}\n" for p, h in sorted(lines))
    return sha256(text.encode())[:40]


def numbers(value, out):
    """Every integer anywhere in a JSON value (the text hashes a table mentions are among them)."""
    if isinstance(value, bool):
        return out
    if isinstance(value, int):
        out.add(value)
    elif isinstance(value, dict):
        for v in value.values():
            numbers(v, out)
    elif isinstance(value, list):
        for v in value:
            numbers(v, out)
    return out


def scenes(world_areas):
    return sorted({row["sceneID"] for row in world_areas if isinstance(row.get("sceneID"), int)})


def gz(data):
    """gzip with no name and no time in the header: the same input gives the same bytes."""
    return gzip.compress(data, compresslevel=9, mtime=0)


def select(root):
    """{path: bytes} to pin from a data set folder."""
    out = {}
    for name in EXCEL:
        path = f"ExcelBinOutput/{name}.json"
        file = root / path
        if not file.exists():
            raise SystemExit(f"{file} is missing: the data set lacks a table the crate reads")
        out[path] = file.read_bytes()
    hashes = set()
    for path, data in out.items():
        numbers(json.loads(data), hashes)
    text = json.loads((root / TEXT_MAP).read_bytes())
    kept = {k: v for k, v in text.items() if k.isdigit() and int(k) in hashes}
    out[TEXT_MAP] = (json.dumps(kept, ensure_ascii=False, indent=2) + "\n").encode()
    for scene in scenes(json.loads(out[WORLD_AREAS])):
        path = POINTS.format(scene)
        if (root / path).exists():
            out[path] = (root / path).read_bytes()
    return out


def write(files, info, target=HERE):
    """Replaces the pinned files in `target` with `files`; returns what changed."""
    old = set()
    for folder in DATA_FOLDERS:
        base = target / folder
        if base.is_dir():
            old |= {p.relative_to(target).as_posix() for p in base.rglob("*.json.gz")}
    changed = []
    for path, data in sorted(files.items()):
        dest = target / f"{path}.gz"
        packed = gz(data)
        if dest.exists() and dest.read_bytes() == packed:
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(packed)
        changed.append(path)
    for stale in sorted(old - {f"{p}.gz" for p in files}):
        (target / stale).unlink()
        changed.append(f"{stale} (removed)")
    manifest = dict(info, files={p: sha256(d) for p, d in sorted(files.items())})
    text = json.dumps(manifest, indent=2) + "\n"
    if not (target / MANIFEST).exists() or (target / MANIFEST).read_text(encoding="utf-8") != text:
        (target / MANIFEST).write_text(text, encoding="utf-8", newline="\n")
        changed.append(MANIFEST)
    return changed


def gh(*args):
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def download(tag, dest):
    """A released data set into `dest` as a folder (only the files `select` reads, gunzipped)."""
    if tag == "latest":
        releases = json.loads(gh("api", f"repos/{RELEASE_REPO}/releases?per_page=100"))
        ours = [r for r in releases if r["tag_name"].startswith("client-") and not r["draft"] and r["published_at"]]
        if not ours:
            raise SystemExit(f"{RELEASE_REPO} has no client data release")
        tag = max(ours, key=lambda r: r["published_at"])["tag_name"]
    gh("release", "download", tag, "--repo", RELEASE_REPO, "--pattern", MANIFEST, "--dir", str(dest))
    released = json.loads((dest / MANIFEST).read_text(encoding="utf-8"))
    wanted = [p for p in released["files"] if p.startswith(("ExcelBinOutput/", "BinOutput/Scene/Point/")) or p == TEXT_MAP]
    for path in wanted:
        asset = path.replace("/", ".") + ".gz"
        gh("release", "download", tag, "--repo", RELEASE_REPO, "--pattern", asset, "--dir", str(dest))
        data = gzip.decompress((dest / asset).read_bytes())
        if sha256(data) != released["files"][path]["sha256"]:
            raise SystemExit(f"{asset} of {tag}: wrong contents")
        (dest / path).parent.mkdir(parents=True, exist_ok=True)
        (dest / path).write_bytes(data)
        (dest / asset).unlink()
    return tag, released


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--from", dest="folder", help="a client data set's folder (the decoder's output)")
    source.add_argument("--release", help="a genshin-game-data client data release tag, or latest")
    args = parser.parse_args(argv)
    with tempfile.TemporaryDirectory() as tmp:
        if args.folder:
            root = Path(args.folder)
            info_file = root / "client.json"
            client = json.loads(info_file.read_text(encoding="utf-8")) if info_file.exists() else {}
            data_id = data_set_id(root)
            info = {"id": data_id, "client": client.get("title", ""), "gameVersion": client.get("gameVersion", "")}
        else:
            root = Path(tmp)
            tag, released = download(args.release, root)
            data_id = released["id"]
            info = {"id": data_id, "client": released["title"], "gameVersion": released["gameVersion"], "release": tag}
        files = select(root)
        changed = write(files, info)
    total = sum(p.stat().st_size for p in HERE.rglob("*.gz"))
    print(f"data set {data_id} ({info['client']}): {len(files)} files, {total / 1e6:.1f} MB pinned")
    for line in changed:
        print("  " + line)
    if not changed:
        print("  nothing changed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
