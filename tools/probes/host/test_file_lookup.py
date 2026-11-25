import importlib.util
from pathlib import Path
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[3] / "docker/eden/find_file.py"
SPEC = importlib.util.spec_from_file_location("find_file", SOURCE)
LOOKUP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LOOKUP)


class FileLookupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.roots = [self.base / name for name in ("user", "device", "logs")]
        for root in self.roots:
            root.mkdir()

    def write(self, root, name, text):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_root_priority_and_nested_ascii_name(self):
        self.write(self.roots[0], "nested/PROBE.LOG", "user")
        self.write(self.roots[1], "probe.log", "device")
        self.write(self.roots[2], "probe.log", "logs")
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "user")

    def test_files_precede_nested_matches(self):
        self.write(self.roots[0], "nested/probe.log", "nested")
        self.write(self.roots[0], "PROBE.LOG", "root")
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "root")

    def test_empty_is_distinct_from_missing(self):
        self.assertIsNone(LOOKUP.read_first("probe.log", self.roots))
        self.write(self.roots[1], "probe.log", "")
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "")

    def test_fresh_reads(self):
        path = self.write(self.roots[0], "probe.log", "pending")
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "pending")
        path.write_text("complete", encoding="utf-8")
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "complete")

    def test_literal_name_and_ascii_only(self):
        self.write(self.roots[0], "[probe].log", "literal")
        self.write(self.roots[0], "Ä.log", "uppercase")
        self.assertEqual(LOOKUP.read_first("[probe].log", self.roots), "literal")
        self.assertIsNone(LOOKUP.read_first("ä.log", self.roots))

    def test_missing_root_and_invalid_utf8(self):
        (self.roots[1] / "probe.log").write_bytes(b"before\xffafter")
        self.assertEqual(
            LOOKUP.read_first("probe.log", [self.base / "absent", self.roots[1]]),
            "before\ufffdafter",
        )

    def test_symlinks_stay_inside_root(self):
        outside = self.write(self.base, "outside/secret.log", "outside")
        (self.roots[0] / "probe.log").symlink_to(outside)
        (self.roots[0] / "loop").symlink_to(self.roots[0], target_is_directory=True)
        self.assertIsNone(LOOKUP.read_first("probe.log", self.roots))
        self.assertIsNone(LOOKUP.read_first("secret.log", self.roots))
        inside = self.write(self.roots[1], "actual.txt", "inside")
        (self.roots[1] / "probe.log").symlink_to(inside)
        self.assertEqual(LOOKUP.read_first("probe.log", self.roots), "inside")

    def test_root_symlink_cannot_redirect_lookup(self):
        self.write(self.base, "outside/probe.log", "outside")
        redirected = self.base / "redirected"
        redirected.symlink_to(self.base / "outside", target_is_directory=True)
        self.assertIsNone(LOOKUP.read_first("probe.log", [redirected]))


if __name__ == "__main__":
    unittest.main()
