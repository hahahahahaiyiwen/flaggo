import importlib.util
import copy
import hashlib
import io
import json
import sqlite3
import tempfile
import tarfile
import unittest
import zipfile
from contextlib import closing, nullcontext
from pathlib import Path
from unittest.mock import Mock, patch


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gateway = load("gateway", "gateway.py")
state_tool = load("state_tool", "state-tool.py")
readiness = load("readiness", "wait-ready.py")
reuse = load("reuse", "reuse-release.py")


class PreparationTests(unittest.TestCase):
    def test_artifact_replay_rejects_mismatched_run_workflow_or_failed_build(self):
        evidence = json.loads(Path(__file__).with_name("release.provenance.json").read_text())
        workflow = evidence["sourceWorkflow"]
        run = {
            "id": workflow["runId"], "workflow_id": workflow["id"], "path": workflow["path"],
            "run_attempt": 1, "head_sha": evidence["sourceCommit"], "head_branch": workflow["branch"],
            "repository": {"full_name": reuse.REPOSITORY},
            "head_repository": {"full_name": reuse.REPOSITORY},
            "event": "push", "status": "completed", "conclusion": "failure",
        }
        artifact = {
            "id": evidence["artifact"]["id"], "name": evidence["artifact"]["name"],
            "size_in_bytes": evidence["artifact"]["sizeBytes"],
            "digest": "sha256:" + evidence["artifact"]["zipSha256FromGitHub"], "expired": False,
            "workflow_run": {"id": run["id"], "head_sha": run["head_sha"], "head_branch": run["head_branch"]},
        }
        workflow_file = {"sha": workflow["gitBlob"], "path": workflow["path"]}
        job = {
            "id": workflow["releaseJobId"], "run_id": run["id"], "head_sha": run["head_sha"],
            "name": "linux-release", "status": "completed", "conclusion": "failure",
            "steps": [
                {"name": "Build committed versioned artifacts", "conclusion": "success"},
                {"name": "Relocate and probe without credentials or maintainer caches", "conclusion": "success"},
                {"name": "Exercise installed private services and offline recovery on the runner", "conclusion": "failure"},
                {"name": "Run actions/upload-artifact@v4", "conclusion": "success"},
            ],
        }
        reuse.verify_metadata(evidence, run, artifact, workflow_file, job)
        for index, key, value in [
            (0, "head_sha", "wrong"), (0, "workflow_id", -1),
            (1, "expired", True), (1, "digest", "wrong"), (2, "sha", "wrong"),
            (3, "steps", []),
        ]:
            with self.subTest(key=key):
                values = copy.deepcopy([run, artifact, workflow_file, job])
                values[index][key] = value
                with self.assertRaises(RuntimeError):
                    reuse.verify_metadata(evidence, *values)

    def test_artifact_replay_checks_all_digest_layers_before_extracting_release(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = json.dumps({
                "format": "flaggo.release/v1", "sourceCommit": "source", "pythonLockSha256": "lock",
            }).encode()
            tar_bytes = io.BytesIO()
            with tarfile.open(fileobj=tar_bytes, mode="w:gz") as archive:
                member = tarfile.TarInfo("flaggo-relocated/release.json")
                member.size = len(manifest)
                archive.addfile(member, io.BytesIO(manifest))
            zipped = root / "input.zip"
            with zipfile.ZipFile(zipped, "w") as archive:
                archive.writestr("_temp/flaggo-release.tar.gz", tar_bytes.getvalue())
            expected = {
                "sizeBytes": zipped.stat().st_size, "zipSha256FromGitHub": reuse.digest(zipped),
                "archiveSha256VerifiedAfterDownload": hashlib.sha256(tar_bytes.getvalue()).hexdigest(),
                "manifestSha256VerifiedAfterDownload": hashlib.sha256(manifest).hexdigest(),
            }
            evidence = {"sourceCommit": "source", "runtime": {"pythonLockSha256": "lock"}, "artifact": expected}
            for key in ["zipSha256FromGitHub", "archiveSha256VerifiedAfterDownload",
                        "manifestSha256VerifiedAfterDownload"]:
                with self.subTest(key=key):
                    changed = copy.deepcopy(evidence)
                    changed["artifact"][key] = "wrong"
                    with self.assertRaisesRegex(RuntimeError, "checksum"):
                        reuse.extract_release(changed, zipped, root)
                    self.assertFalse((root / "flaggo-relocated").exists())
            self.assertEqual(reuse.extract_release(evidence, zipped, root),
                             expected["manifestSha256VerifiedAfterDownload"])
            self.assertEqual((root / "flaggo-relocated/release.json").read_bytes(), manifest)

    def test_readiness_retries_startup_connection_resets_then_requires_success(self):
        url = "http://127.0.0.1:3000/api/health"
        with patch.object(readiness.sys, "argv", ["wait-ready.py", url]), \
                patch.object(readiness.urllib.request, "urlopen", side_effect=[
                    ConnectionResetError("starting"), readiness.urllib.error.URLError("not ready"),
                    nullcontext(Mock(status=200)),
                ]) as request, patch.object(readiness.time, "sleep") as pause:
            readiness.main()
            self.assertEqual(request.call_count, 3)
            self.assertEqual(pause.call_count, 2)

    def test_readiness_persistent_connection_reset_fails_with_the_url_and_cause(self):
        url = "http://127.0.0.1:3000/api/health"
        error = ConnectionResetError("still unavailable")
        with patch.object(readiness.sys, "argv", ["wait-ready.py", url]), \
                patch.object(readiness.urllib.request, "urlopen", side_effect=error) as request, \
                patch.object(readiness.time, "sleep") as pause:
            with self.assertRaisesRegex(RuntimeError, url) as failure:
                readiness.main()
            self.assertIs(failure.exception.__cause__, error)
            self.assertEqual(request.call_count, 30)
            self.assertEqual(pause.call_count, 29)

    def test_gateway_refreshes_real_pid_changes_and_preserves_degraded_status(self):
        start = Mock(side_effect=lambda env: Mock(poll=lambda: None))
        process = gateway.Gateway(start)
        pids = {"flaggo-contract-service": 100, "flaggo-async-analysis": 200}
        process.refresh(pids)
        first = process.child
        process.refresh(pids)
        process.refresh({**pids, "flaggo-async-analysis": 0})
        self.assertEqual(start.call_count, 1)
        process.refresh({**pids, "flaggo-async-analysis": 201})
        first.terminate.assert_called_once()
        self.assertEqual(
            json.loads(start.call_args.args[0]["FLAGGO_PLAYGROUND_SERVICE_PROCESSES"]),
            {**pids, "flaggo-async-analysis": 201},
        )
        self.assertEqual(start.call_count, 2)

    def test_recovery_round_trip_preserves_authority_and_rejects_tampering(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            state = root / "state"
            state.mkdir()
            with closing(sqlite3.connect(state / "flaggo.db")) as db:
                db.execute("PRAGMA journal_mode=WAL")
                db.execute("CREATE TABLE authority (digest TEXT NOT NULL)")
                db.execute("INSERT INTO authority VALUES ('sha256:accepted')")
                db.commit()
            workspace = state / "analysis-workspaces" / "sealed"
            workspace.mkdir(parents=True)
            (workspace / "evidence.json").write_text('{"saved":"artifact"}', encoding="utf-8")
            (workspace / "copilot-home").write_text("sealed artifact, not an auth cache", encoding="utf-8")
            (state / "copilot-home").mkdir()
            (state / "copilot-home" / "do-not-export").write_text("private-cache", encoding="utf-8")
            release = root / "release"
            release.mkdir()
            (release / "release.json").write_text(json.dumps({
                "format": "flaggo.release/v1", "sourceCommit": "test", "files": [],
            }), encoding="utf-8")
            release_hash = state_tool.digest(release / "release.json")
            config = root / "config"
            config.mkdir()
            (config / "analysis.env").write_text("DO_NOT_EXPORT=credential", encoding="utf-8")
            stopped = Mock()
            archive = root / "backup"
            backup_hash = state_tool.backup(state, archive, release, config, release_hash, stopped)
            self.assertFalse((archive / "state/copilot-home").exists())
            self.assertTrue((archive / "state/analysis-workspaces/sealed/copilot-home").is_file())
            self.assertNotIn("DO_NOT_EXPORT", (archive / "backup.json").read_text())
            restored = root / "restored"
            state_tool.restore(archive, restored, backup_hash, release_hash, stopped)
            with closing(sqlite3.connect(restored / "flaggo.db")) as db:
                self.assertEqual(db.execute("SELECT digest FROM authority").fetchone()[0], "sha256:accepted")
            self.assertEqual((restored / "analysis-workspaces/sealed/evidence.json").read_bytes(),
                             (workspace / "evidence.json").read_bytes())
            with self.assertRaisesRegex(ValueError, "matching release"):
                state_tool.restore(archive, root / "wrong-release", backup_hash, "wrong", stopped)
            with self.assertRaisesRegex(ValueError, "never overwrite"):
                state_tool.restore(archive, restored, backup_hash, release_hash, stopped)
            (archive / "state/flaggo.db").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "mismatch"):
                state_tool.restore(archive, root / "tampered", backup_hash, release_hash, stopped)
            self.assertFalse((root / "tampered").exists())
            self.assertGreaterEqual(stopped.call_count, 4)
            (state / ".flaggo-state.json").write_text(json.dumps({
                "format": "flaggo.state/v1", "kind": "disposable-smoke", "stateId": "exact",
            }))
            result = state_tool.reset(
                state, root / "reset-archive", release, config, release_hash, "exact", stopped,
            )
            self.assertNotEqual(result["newStateId"], "exact")
            self.assertFalse((state / "flaggo.db").exists())
            self.assertTrue((root / "reset-archive/quiesced-original/flaggo.db").is_file())
            self.assertTrue((state / "copilot-home/do-not-export").is_file())
            self.assertFalse((root / "reset-archive/quiesced-original/copilot-home").exists())

    def test_reset_requires_exact_disposable_identity_before_moving_anything(self):
        with tempfile.TemporaryDirectory() as temporary:
            state = Path(temporary).resolve()
            marker = state / ".flaggo-state.json"
            marker.write_text(json.dumps({"kind": "persistent", "stateId": "operator-id"}))
            with self.assertRaisesRegex(ValueError, "exact disposable"):
                state_tool.reset(state, state / "archive", state, state, "hash", "operator-id", Mock())
            self.assertEqual(json.loads(marker.read_text())["kind"], "persistent")
            self.assertFalse((state / "archive").exists())

    def test_recovery_rejects_extra_files_and_manifest_path_traversal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "state").mkdir()
            (root / "state/unrecorded").write_text("unexpected")
            with self.assertRaisesRegex(ValueError, "differs"):
                state_tool.verify_state_copy(root, {"files": []})
            with self.assertRaisesRegex(ValueError, "Invalid manifest path"):
                state_tool.contained_file(root, "../escape")


if __name__ == "__main__":
    unittest.main()
