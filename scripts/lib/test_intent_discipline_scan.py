"""Exact preset execution exception must not exempt the whole App or View."""
import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.dont_write_bytecode = True
import intent_discipline_scan as scan

class PresetExecutionBoundary(unittest.TestCase):
    def hits(self, path, source):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / path
            target.parent.mkdir(parents=True)
            target.write_text(source)
            output = io.StringIO()
            with patch("sys.argv", ["scan", directory, directory, "src", "--all", "--pane"]):
                with contextlib.redirect_stdout(output):
                    self.assertEqual(scan.main(), 0)
            return output.getvalue()
    def test_owned_execution(self):
        self.assertEqual(self.hits("src/app/preset_editor.rs", "fn apply() { store.save_tab(value); }"), "")
    def test_another_function_in_the_same_file_is_not_exempt(self):
        self.assertIn("[preset]", self.hits("src/app/preset_editor.rs", "fn draw() { store.save_tab(value); }"))
    def test_view_cannot_execute_store_save(self):
        self.assertIn("[preset]", self.hits("src/view/preset.rs", "fn apply() { store.save_tab(value); }"))
    def test_same_function_does_not_allow_structural_mutation(self):
        self.assertIn("[surface]", self.hits("src/app/preset_editor.rs", "fn apply() { engine.split_surface(id); }"))

if __name__ == "__main__":
    unittest.main()
