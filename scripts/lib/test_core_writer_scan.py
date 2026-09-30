"""Mutation tests for navigation projections versus actual structural writers."""

import contextlib
import io
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
import core_writer_scan as scan


class NavigationWriterBoundary(unittest.TestCase):
    def measure(self, source, path="src/state/navigation.rs"):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for item in scan.APPLY_PATH_FILES + scan.EXCLUDED_PREFIXES:
                target = root / item
                if item.endswith("/"):
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.touch()
            for item, _ in scan.EXCLUDED_FNS:
                target = root / item
                target.parent.mkdir(parents=True, exist_ok=True)
                target.touch()
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source)
            output = io.StringIO()
            with patch("sys.argv", ["core_writer_scan.py", temp]):
                with contextlib.redirect_stdout(output):
                    self.assertEqual(scan.main(), 0)
            return [line.split("\t") for line in output.getvalue().splitlines()]

    def test_selection_map_and_remap_are_not_model_writes(self):
        self.assertEqual(self.measure("""
            impl NavigationState {
                pub fn reconcile(&mut self, workspaces: &[Workspace]) {
                    self.selected_tabs.retain(|id, _| alive(id));
                    self.remap_surface_selection(removed, replacement);
                }
            }
        """), [])

    def test_real_model_write_in_navigation_is_still_detected(self):
        rows = self.measure("""
            impl NavigationState {
                pub fn reconcile(&mut self, engine: &mut CoreState) {
                    engine.workspaces.clear();
                }
            }
        """)
        self.assertEqual([row[0] for row in rows], ["field", "appstate"])
        self.assertTrue(all(row[3] == "reconcile" for row in rows))

    def test_explicit_tab_mutations_are_detected_at_new_application_boundary(self):
        rows = self.measure("""
            fn execute(engine: &mut CoreState) {
                pane.remove_tab(id);
                pane.take_tab(id);
            }
        """, "src/app/structural_exec.rs")
        self.assertEqual([row[0] for row in rows], ["subop", "subop"])
        self.assertEqual([row[5] for row in rows], ["remove_tab", "take_tab"])

    def test_domain_tabs_write_is_detected_beside_remote_mapping(self):
        rows = self.measure("""
            fn retain_workspace(&mut self, pane: &mut Pane) {
                self.remote_tabs.retain(|_, local| alive(local));
                pane.tabs.clear();
            }
        """, "src/app/attach_client.rs")
        self.assertEqual([row[0] for row in rows], ["field"])
        self.assertEqual(rows[0][3], "retain_workspace")


if __name__ == "__main__":
    unittest.main()
