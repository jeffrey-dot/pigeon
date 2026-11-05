#!/usr/bin/env python3
"""Mutation fixtures for the architecture guard; no product files are changed."""
import copy
import unittest

from architecture import manifest_errors, source_errors


class ArchitectureTests(unittest.TestCase):
    def test_runtime_alias_and_qualified_use_are_rejected_outside_bin(self):
        for code in ["use tokio as runtime;", "tokio::spawn(work);", "use portable_pty::{Child};"]:
            self.assertTrue(source_errors("core/src/pane.rs", code))
            self.assertFalse(source_errors("core/src/bin/pane.rs", code))
        self.assertFalse(source_errors("core/src/pane.rs", "// tokio::spawn(work);\nuse std::sync::Arc;"))

    def test_swift_import_forms_cannot_cross_ui_seams(self):
        for code in ["import AppKit", "@preconcurrency import AppKit", "import class AppKit.NSView"]:
            self.assertTrue(source_errors("clients/macos/Sources/PigeonCore/Adapter.swift", code))
        self.assertTrue(source_errors("clients/macos/Sources/PigeonApp/Other.swift", "import CoreText"))
        self.assertFalse(source_errors("clients/macos/Sources/PigeonApp/Terminal/CoreTextRenderer.swift", "import CoreText"))

    def test_renamed_and_target_specific_dependencies_are_checked(self):
        package = {"name": "pigeon-core", "manifest_path": "/repo/core/Cargo.toml",
                   "features": {"host": ["dep:tokio", "dep:nix", "dep:portable-pty"]},
                   "dependencies": [{"name": "serde", "kind": None, "optional": False, "target": None}]}
        self.assertEqual(manifest_errors([package]), [])
        bad = copy.deepcopy(package)
        bad["dependencies"][0].update(name="objc2", rename="portable_ui", target="cfg(target_os = macos)")
        self.assertEqual(len(manifest_errors([bad])), 2)
        bad = copy.deepcopy(package)
        bad["dependencies"][0]["name"] = "tokio"
        self.assertTrue(manifest_errors([bad]))

    def test_client_cannot_enable_core_internals(self):
        package = {"name": "pigeon-linux", "manifest_path": "/repo/clients/linux/Cargo.toml",
                   "dependencies": [{"name": "pigeon-core", "uses_default_features": False, "features": []}]}
        self.assertEqual(manifest_errors([package]), [])
        for key, value in [("uses_default_features", True), ("features", ["internals"])]:
            bad = copy.deepcopy(package)
            bad["dependencies"][0][key] = value
            self.assertTrue(manifest_errors([bad]))


if __name__ == "__main__":
    unittest.main()
