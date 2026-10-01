"""A moved/split file must remain measurable even without a size exemption."""
from pathlib import Path
import sys
import tempfile
import unittest
sys.dont_write_bytecode = True
from frozen_corpus import validate

class FrozenCorpus(unittest.TestCase):
    def fixture(self, root):
        (root / "parent.rs").write_text("fn parent() {}")
        (root / "child.rs").write_text("fn child() {}")
        (root / ".complexity-file-allowlist").write_text("parent.rs\n")
        (root / ".complexity-frozen-files").write_text("parent.rs\nchild.rs\n")
    def test_split_child_remains_in_aggregate_without_exemption(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); self.fixture(root)
            self.assertEqual(validate(root), ["parent.rs", "child.rs"])
    def test_missing_moved_file_does_not_silently_lower_sum(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); self.fixture(root); (root / "child.rs").unlink()
            with self.assertRaises(ValueError): validate(root)
    def test_exception_cannot_be_omitted_from_aggregate(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); self.fixture(root)
            (root / ".complexity-frozen-files").write_text("child.rs\n")
            with self.assertRaises(ValueError): validate(root)
    def test_duplicate_cannot_inflate_the_baseline(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); self.fixture(root)
            (root / ".complexity-frozen-files").write_text("parent.rs\nparent.rs\n")
            with self.assertRaises(ValueError): validate(root)

if __name__ == "__main__": unittest.main()
