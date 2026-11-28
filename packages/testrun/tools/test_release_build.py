import tempfile
from pathlib import Path
import unittest

from release_build import TARGETS, archive_name, prepared_release, release_notes


class ReleaseBuildTests(unittest.TestCase):
    def test_selects_full_version_entry_and_preserves_subheadings(self):
        changelog = "# Changes\n\n## 1.2.3\n\n### Browser\n\n- New capture\n\n## 1.2.2\n\n- Old fix\n"
        self.assertEqual(release_notes(changelog, "1.2.3"), "### Browser\n\n- New capture\n")
        for notes in ("## 1.2.3\n", changelog + "## 1.2.3\n- Duplicate", "## 1.2.30\n- Other"):
            with self.assertRaises(ValueError):
                release_notes(notes, "1.2.3")

    def test_requires_matching_locked_version(self):
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory)
            (package / "Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
            (package / "Cargo.lock").write_text('[[package]]\nname = "canonic-testrun"\nversion = "1.2.2"\n')
            (package / "CHANGELOG.md").write_text("## 1.2.3\n- New release\n")
            with self.assertRaisesRegex(ValueError, "Cargo.lock"):
                prepared_release(package, "1.2.3")

    def test_assets_are_unique_and_use_platform_archive_formats(self):
        names = {archive_name("1.2.3", target) for target in TARGETS}
        self.assertEqual(len(names), 6)
        self.assertEqual(sum(name.endswith(".zip") for name in names), 2)
        self.assertIn("testrun-1.2.3-aarch64-unknown-linux-gnu.tar.gz", names)


if __name__ == "__main__":
    unittest.main()
