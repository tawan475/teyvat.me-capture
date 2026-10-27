# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475

import gzip
import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

# The script lives in game-data/, which is not an importable package.
_ROOT = Path(__file__).resolve().parent
_SPEC = importlib.util.spec_from_file_location("pin", _ROOT / "game-data" / "pin.py")
pin = importlib.util.module_from_spec(_SPEC)
sys.modules["pin"] = pin
_SPEC.loader.exec_module(pin)


def write_json(root, path, value):
    file = root / path
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(value), encoding="utf-8")


def data_set(root):
    """A client data set folder with every table the crate reads."""
    for name in pin.EXCEL:
        write_json(root, f"ExcelBinOutput/{name}.json", [{"id": 1, "nameTextMapHash": 111}])
    write_json(root, pin.WORLD_AREAS, [{"areaID1": 1, "sceneID": 3}, {"areaID1": 2, "sceneID": 9}])
    write_json(root, pin.TEXT_MAP, {"111": "Kept", "222": "Not mentioned", "1": "Also kept: an id"})
    write_json(root, "TextMap/TextMapCHS.json", {"111": "x"})
    write_json(root, pin.POINTS.format(3), {"points": {"1": {}}})
    write_json(root, pin.POINTS.format(4), {"points": {"2": {}}})


class SelectTest(unittest.TestCase):
    def test_keeps_the_tables_the_mentioned_text_and_the_world_scenes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_set(root)
            files = pin.select(root)
            self.assertIn("ExcelBinOutput/AvatarExcelConfigData.json", files)
            self.assertNotIn("TextMap/TextMapCHS.json", files)
            self.assertEqual(json.loads(files[pin.TEXT_MAP]), {"111": "Kept", "1": "Also kept: an id"})
            # scene 3 is named by WorldAreaConfigData and exists; 9 is named but has no file; 4 is not named
            self.assertIn(pin.POINTS.format(3), files)
            self.assertNotIn(pin.POINTS.format(9), files)
            self.assertNotIn(pin.POINTS.format(4), files)

    def test_a_missing_table_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_set(root)
            (root / "ExcelBinOutput/WeaponExcelConfigData.json").unlink()
            with self.assertRaises(SystemExit):
                pin.select(root)


class IdTest(unittest.TestCase):
    def test_names_the_data_set_by_its_data_files_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data_set(root)
            first = pin.data_set_id(root)
            self.assertRegex(first, "^[0-9a-f]{40}$")
            (root / "client.json").write_text("{}", encoding="utf-8")
            self.assertEqual(pin.data_set_id(root), first)
            write_json(root, pin.POINTS.format(4), {"points": {}})
            self.assertNotEqual(pin.data_set_id(root), first)

    def test_matches_genshin_game_data(self):
        # scripts/lib/client-data.ts manifestId: sha256 over sorted "path<TAB>sha256\n" lines, 40 digits
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_json(root, "ExcelBinOutput/B.json", [2])
            write_json(root, "ExcelBinOutput/A.json", [1])
            lines = "".join(
                f"ExcelBinOutput/{n}.json\t{hashlib.sha256((root / 'ExcelBinOutput' / f'{n}.json').read_bytes()).hexdigest()}\n"
                for n in ("A", "B")
            )
            self.assertEqual(pin.data_set_id(root), hashlib.sha256(lines.encode()).hexdigest()[:40])


class WriteTest(unittest.TestCase):
    def test_reproducible_and_removes_what_the_data_set_lost(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp)
            files = {"ExcelBinOutput/A.json": b"[1]", pin.POINTS.format(3): b"{}"}
            info = {"id": "a" * 40}
            self.assertEqual(len(pin.write(files, info, target)), 3)
            first = (target / "ExcelBinOutput/A.json.gz").read_bytes()
            self.assertEqual(pin.write(files, info, target), [])
            self.assertEqual((target / "ExcelBinOutput/A.json.gz").read_bytes(), first)
            changed = pin.write({"ExcelBinOutput/A.json": b"[1]"}, info, target)
            self.assertIn(f"{pin.POINTS.format(3)}.gz (removed)", changed)
            self.assertFalse((target / f"{pin.POINTS.format(3)}.gz").exists())


class CommittedPinTest(unittest.TestCase):
    """game-data/ as committed: every listed file is there with the listed contents, and nothing else."""

    def test_manifest_matches_the_files(self):
        manifest = json.loads((pin.HERE / pin.MANIFEST).read_text(encoding="utf-8"))
        self.assertRegex(manifest["id"], "^[0-9a-f]{40}$")
        present = {p.relative_to(pin.HERE).as_posix()[: -len(".gz")] for p in pin.HERE.rglob("*.json.gz")}
        self.assertEqual(present, set(manifest["files"]))
        for path, digest in manifest["files"].items():
            data = gzip.decompress((pin.HERE / f"{path}.gz").read_bytes())
            self.assertEqual(hashlib.sha256(data).hexdigest(), digest, path)
        for name in pin.EXCEL:
            self.assertIn(f"ExcelBinOutput/{name}.json", manifest["files"])
        self.assertIn(pin.TEXT_MAP, manifest["files"])


if __name__ == "__main__":
    unittest.main()
