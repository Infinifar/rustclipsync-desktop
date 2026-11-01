import hashlib
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def fixture(self, missing=None, corrupt=None, duplicate=None):
        source = self.root / "artifacts"
        for index, name in enumerate(release.expected_assets("0.3.0")):
            if name == missing:
                continue
            folder = source / str(index) / "nested"
            folder.mkdir(parents=True)
            content = ("fixture " + name).encode()
            (folder / name).write_bytes(content + (b"tampered" if name == corrupt else b""))
            (folder / "SHA256SUMS").write_text("\ufeff" + hashlib.sha256(content).hexdigest() + "  " + name + "\n")
            if name == duplicate:
                other = source / "duplicate"
                other.mkdir()
                shutil.copy(folder / name, other / name)
        return source

    def test_complete_verified_set_has_one_combined_manifest(self):
        output = release.prepare_assets(self.fixture(), self.root / "release", "0.3.0")
        self.assertEqual(set(release.checksums(output / "SHA256SUMS")), set(release.expected_assets("0.3.0")))
        self.assertEqual(len(list(output.iterdir())), 6)

    def test_missing_platform_never_creates_partial_release(self):
        source = self.fixture(missing=release.expected_assets("0.3.0")[1])
        with self.assertRaises(ValueError):
            release.prepare_assets(source, self.root / "release", "0.3.0")
        self.assertFalse((self.root / "release").exists())

    def test_tampered_installer_is_rejected(self):
        with self.assertRaises(ValueError):
            release.prepare_assets(self.fixture(corrupt=release.expected_assets("0.3.0")[0]), self.root / "release", "0.3.0")

    def test_duplicate_artifacts_are_rejected(self):
        with self.assertRaises(ValueError):
            release.prepare_assets(self.fixture(duplicate=release.expected_assets("0.3.0")[2]), self.root / "release", "0.3.0")

    def test_mixed_ui_and_agent_versions_are_rejected(self):
        version = release.check_versions()
        for name in release.VERSION_FILES:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(release.ROOT / name, target)
        self.assertEqual(release.check_versions(self.root), version)
        path = self.root / "ui/linux/main.cpp"
        path.write_text(path.read_text().replace(f'setApplicationVersion("{version}")', 'setApplicationVersion("999.0.0")'))
        with self.assertRaises(ValueError):
            release.check_versions(self.root)

    def test_main_publishes_new_version_only_and_pull_requests_never_publish(self):
        (self.root / "releases").mkdir()
        (self.root / "releases/v0.3.0.md").write_text("Release notes")
        environment = {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/main", "GITHUB_REPOSITORY": "owner/repo", "GITHUB_SHA": "new"}
        with patch.object(release, "tag_commit", return_value=None):
            self.assertTrue(release.plan("0.3.0", environment, self.root))
        with patch.object(release, "tag_commit", return_value="older"):
            self.assertFalse(release.plan("0.3.0", environment, self.root))
        environment["GITHUB_EVENT_NAME"] = "pull_request"
        with patch.object(release, "tag_commit", side_effect=AssertionError("PR must not query publication")):
            self.assertFalse(release.plan("0.3.0", environment, self.root))

    def test_mismatched_tag_and_arbitrary_publish_branch_are_rejected(self):
        for event, reference in [("push", "refs/tags/v9.9.9"), ("workflow_dispatch", "refs/heads/feature")]:
            with self.assertRaises(ValueError):
                release.plan("0.3.0", {"GITHUB_EVENT_NAME": event, "GITHUB_REF": reference, "REQUEST_PUBLISH": "true"}, self.root)

    def test_cleanup_preserves_success_active_cancelled_and_current_runs(self):
        deleted = []
        records = [
            {"id": 1, "status": "completed", "conclusion": "failure"},
            {"id": 2, "status": "in_progress", "conclusion": None},
            {"id": 3, "status": "completed", "conclusion": "success"},
            {"id": 4, "status": "completed", "conclusion": "cancelled"},
            {"id": 5, "status": "completed", "conclusion": "failure"},
        ]
        def fake_api(path, method="GET", **kwargs):
            if method == "DELETE":
                deleted.append(path)
                return None
            if "/actions/runs?" in path:
                return [{"workflow_runs": records}]
            if path.endswith("/artifacts?per_page=100"):
                return [{"artifacts": [{"id": 101}]}]
            return records[int(path.rsplit("/", 1)[1]) - 1]
        with patch.object(release, "api", side_effect=fake_api):
            release.cleanup_failed({"GITHUB_REPOSITORY": "owner/repo", "GITHUB_RUN_ID": "5"})
        self.assertEqual(deleted, ["repos/owner/repo/actions/artifacts/101", "repos/owner/repo/actions/runs/1"])

    def test_release_lookup_includes_unpublished_drafts(self):
        draft = {"id": 12, "tag_name": "v0.3.0", "draft": True}
        with patch.object(release, "api", return_value=[[{"tag_name": "v0.2.3", "draft": False}], [draft]]) as query:
            self.assertEqual(release.find_release("owner/repo", "v0.3.0"), draft)
            self.assertIn("/releases?", query.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
