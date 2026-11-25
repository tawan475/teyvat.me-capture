# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475

import base64
import importlib.util
import io
import json
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path

# The script lives in .github/scripts, which is not an importable package.
_PATH = Path(__file__).resolve().parent / ".github" / "scripts" / "keys_watch.py"
_SPEC = importlib.util.spec_from_file_location("keys_watch", _PATH)
watch = importlib.util.module_from_spec(_SPEC)
sys.modules["keys_watch"] = watch  # dataclasses look their module up here
_SPEC.loader.exec_module(watch)


def key(key_id: int, fill: int = 0xA5) -> bytes:
    """A made-up key: its id big-endian, then a filler byte."""
    return key_id.to_bytes(2, "big") + bytes([fill]) * (watch.KEY_LEN - 2)


def table(**entries: bytes) -> bytes:
    """`table(_10=key(10))` -> the JSON text of {"10": base64}."""
    return json.dumps({name.lstrip("_"): base64.b64encode(value).decode() for name, value in entries.items()}).encode()


def b64(value: bytes) -> str:
    return base64.b64encode(value).decode()


OURS = table(_10=key(10), _20=key(20), _2701=key(2701))


class ParseTest(unittest.TestCase):
    def test_reads_ids_and_keys(self):
        parsed = watch.parse_table(OURS, "ours")
        self.assertEqual(sorted(parsed), [10, 20, 2701])
        self.assertEqual(parsed[20], key(20))

    def test_the_real_table_parses(self):
        parsed = watch.parse_table(watch.OURS.read_bytes(), "ours")
        self.assertIn(2701, parsed)
        for key_id, value in parsed.items():
            self.assertEqual(int.from_bytes(value[:2], "big"), key_id)

    def assert_malformed(self, raw, fragment):
        with self.assertRaises(watch.TableError) as caught:
            watch.parse_table(raw, "t")
        self.assertIn(fragment, str(caught.exception))

    def test_malformed_tables_are_errors(self):
        good = b64(key(1))
        self.assert_malformed(b"\xff\xfe", "not UTF-8")
        self.assert_malformed(b"{", "not JSON")
        self.assert_malformed(b"[]", "not a JSON object")
        self.assert_malformed(b"{}", "holds no keys")
        self.assert_malformed(json.dumps({"abc": good}), "not a decimal")
        self.assert_malformed(json.dumps({"-1": good}), "not a decimal")
        self.assert_malformed(json.dumps({"01": good}), "not a decimal")
        self.assert_malformed(json.dumps({" 1": good}), "not a decimal")
        self.assert_malformed(json.dumps({"65536": good}), "not a decimal")
        self.assert_malformed(json.dumps({"1": 5}), "not a string")
        self.assert_malformed(json.dumps({"1": "not base64!"}), "not base64")
        self.assert_malformed(json.dumps({"1": b64(b"short")}), "5 bytes")
        self.assert_malformed(json.dumps({"1": b64(key(1) + b"x")}), "4097 bytes")
        self.assert_malformed('{"1": "%s", "1": "%s"}' % (good, good), "appears twice")

    def test_the_edges_of_the_id_range_are_ids(self):
        parsed = watch.parse_table(json.dumps({"0": b64(key(0)), "65535": b64(key(65535))}), "t")
        self.assertEqual(sorted(parsed), [0, 65535])


class CompareTest(unittest.TestCase):
    def test_same_tables_have_no_differences(self):
        ours = watch.parse_table(OURS, "ours")
        result = watch.compare(ours, dict(ours))
        self.assertFalse(result.has_differences)

    def test_ids_only_we_have_are_fine(self):
        ours = watch.parse_table(OURS, "ours")
        public = {10: key(10), 20: key(20)}
        result = watch.compare(ours, public)
        self.assertFalse(result.has_differences)
        self.assertEqual(result.only_ours, [2701])

    def test_missing_and_different_ids_are_reported(self):
        ours = watch.parse_table(OURS, "ours")
        public = {10: key(10), 20: key(20, fill=0x5A), 30: key(30), 5: key(5)}
        result = watch.compare(ours, public)
        self.assertTrue(result.has_differences)
        self.assertEqual(result.missing, [5, 30])
        self.assertEqual(result.different, [20])
        self.assertEqual(result.only_ours, [2701])


class RunTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.ours = Path(self.tmp.name) / "gi.json"
        self.ours.write_bytes(OURS)
        self.report = Path(self.tmp.name) / "report.md"

    def run_with(self, get, ours=None):
        out = io.StringIO()
        code = watch.run(ours or self.ours, get=get, report_path=self.report, out=out)
        return code, out.getvalue()

    def assert_no_key_material(self, text):
        for key_id in (5, 10, 20, 30, 2701):
            for fill in (0xA5, 0x5A):
                value = key(key_id, fill)
                encoded = b64(value)
                # Any run of the base64 or hex text, not just the whole key.
                self.assertNotIn(encoded[:16], text)
                self.assertNotIn(encoded[-16:], text)
                self.assertNotIn(value.hex()[:16], text)
                self.assertNotIn(value[2:18].hex(), text)

    def test_nothing_to_do(self):
        code, out = self.run_with(lambda url: table(_10=key(10), _20=key(20)))
        self.assertEqual(code, watch.EXIT_NOTHING)
        self.assertIn("nothing to do", out)
        self.assertFalse(self.report.exists())
        self.assert_no_key_material(out)

    def test_differences_found(self):
        public = table(_10=key(10), _20=key(20, fill=0x5A), _30=key(30), _5=key(5))
        code, out = self.run_with(lambda url: public)
        self.assertEqual(code, watch.EXIT_DIFFERENCES)
        self.assertIn("differences found", out)
        written = self.report.read_text(encoding="utf-8")
        self.assertIn("lack: 5, 30", written)
        self.assertIn("different keys: 20", written)
        self.assertNotIn("2701", written)
        self.assert_no_key_material(out)
        self.assert_no_key_material(written)

    def test_the_public_list_is_read_from_its_url(self):
        asked = []
        self.run_with(lambda url: asked.append(url) or OURS)
        self.assertEqual(asked, [watch.PUBLIC_URL])

    def test_a_network_error_is_not_checked(self):
        def offline(url):
            raise urllib.error.URLError("no route")

        code, out = self.run_with(offline)
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.assertIn("could not check", out)
        self.assertNotIn("nothing to do", out)

    def test_an_http_error_is_not_checked(self):
        def missing(url):
            raise urllib.error.HTTPError(url, 404, "Not Found", {}, io.BytesIO(b""))

        code, out = self.run_with(missing)
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.assertIn("404", out)

    def test_a_malformed_public_list_is_not_checked(self):
        for raw in (b"<html>rate limited</html>", b"{}", json.dumps({"10": b64(b"short")}).encode()):
            code, out = self.run_with(lambda url: raw)
            self.assertEqual(code, watch.EXIT_UNCHECKED, raw)
            self.assertIn("could not check", out)

    def test_a_wrong_length_public_key_never_shows_its_bytes(self):
        long_key = key(10) + b"\x01"
        code, out = self.run_with(lambda url: json.dumps({"10": b64(long_key)}).encode())
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.assertNotIn(b64(long_key)[:16], out)
        self.assert_no_key_material(out)

    def test_a_missing_or_malformed_own_table_is_not_checked(self):
        code, out = self.run_with(lambda url: OURS, ours=Path(self.tmp.name) / "absent.json")
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.ours.write_text('{"10": "x"}', encoding="utf-8")
        code, out = self.run_with(lambda url: OURS)
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.assertIn("our keys", out)

    def test_main_turns_a_crash_into_not_checked(self):
        original = watch.run

        def crash(*args, **kwargs):
            raise KeyError("boom")

        watch.run = crash
        try:
            out = io.StringIO()
            stdout, sys.stdout = sys.stdout, out
            try:
                code = watch.main([])
            finally:
                sys.stdout = stdout
        finally:
            watch.run = original
        self.assertEqual(code, watch.EXIT_UNCHECKED)
        self.assertIn("could not check", out.getvalue())

    def test_exit_codes_are_distinct(self):
        codes = {watch.EXIT_NOTHING, watch.EXIT_DIFFERENCES, watch.EXIT_UNCHECKED}
        self.assertEqual(len(codes), 3)
        self.assertNotIn(1, codes)  # Python's own code for an uncaught exception


if __name__ == "__main__":
    unittest.main()
