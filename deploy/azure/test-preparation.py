import importlib.util
import copy
import hashlib
import io
import json
import os
import shutil
import sqlite3
import subprocess
import sys
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
diagnostics = load("diagnostics", "collect-diagnostics.py")


class PreparationTests(unittest.TestCase):
    def test_browser_failure_does_not_skip_recovery_or_turn_overall_status_green(self):
        windows_bash = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Git" / "bin" / "bash.exe"
        bash = str(windows_bash) if os.name == "nt" and windows_bash.exists() else shutil.which("bash")
        self.assertIsNotNone(bash, "Bash is required to validate installed-check orchestration")
        helper = Path(__file__).with_name("installed-checks.sh").read_text()
        for browser_status, recovery_status in [(0, 0), (7, 0), (0, 9), (7, 9)]:
            with self.subTest(browser=browser_status, recovery=recovery_status), \
                    tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                script = helper + f"""
browser_check() {{ return {browser_status}; }}
recovery_check() {{
  printf started > recovery.started
  (exit {recovery_status})
  printf completed > recovery.completed
}}
run_installed_checks browser_check recovery_check .
printf passed > overall.passed
"""
                result = subprocess.run([bash, "-ec", script], cwd=root, capture_output=True, text=True)
                self.assertEqual(result.returncode, recovery_status or browser_status, result.stderr)
                self.assertEqual(json.loads((root / "installed-browser-result.json").read_text()),
                                 {"passed": browser_status == 0, "exitCode": browser_status})
                self.assertEqual(json.loads((root / "installed-recovery-status.json").read_text()),
                                 {"passed": recovery_status == 0, "exitCode": recovery_status})
                self.assertTrue((root / "recovery.started").exists())
                self.assertEqual((root / "recovery.completed").exists(), recovery_status == 0)
                self.assertEqual((root / "overall.passed").exists(),
                                 browser_status == 0 and recovery_status == 0)
                diagnostics.collect(root, root, os.getuid() if os.name == "posix" else 0,
                                    os.getgid() if os.name == "posix" else 0)
                staged = root / "flaggo-diagnostics"
                for name in ("installed-browser-result.json", "installed-recovery-status.json"):
                    self.assertEqual((staged / name).read_bytes(), (root / name).read_bytes())

    def test_diagnostics_only_stage_allowlisted_proofs_without_private_backup_values(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            private = root / "state-backup"
            private.mkdir(mode=0o700)
            (private / "backup.json").write_text(json.dumps({
                "format": "flaggo.backup/v1", "releaseSha256": "release",
                "sourceCommit": "source", "files": [{"path": "private-evidence-name"}],
                "configuration": [{"secret": "DO_NOT_EXPORT"}],
            }))
            (private / "flaggo.db").write_bytes(b"PRIVATE_DATABASE")
            (root / "installed-secrets.json").write_text("NOT_ALLOWLISTED")
            (root / "installed-reset.json").write_text('{"newStateId":"fresh"}')
            browser = root / "examples/tetris-playground/test-results/example"
            browser.mkdir(parents=True)
            (browser / "correlation-state.json").write_text('{"polls":[{"actual":"body"}]}')
            (browser / "private.env").write_text("DO_NOT_EXPORT")
            owner = private.stat()
            uid, gid = (1000, 1000) if os.name == "posix" and os.geteuid() == 0 else (
                os.getuid() if os.name == "posix" else 0, os.getgid() if os.name == "posix" else 0,
            )
            result = diagnostics.collect(root, root, uid, gid)
            self.assertEqual(result["errors"], [])
            staged = root / "flaggo-diagnostics"
            self.assertEqual((staged / "installed-reset.json").read_bytes(),
                             (root / "installed-reset.json").read_bytes())
            self.assertEqual((staged / "browser/example/correlation-state.json").read_bytes(),
                             (browser / "correlation-state.json").read_bytes())
            proof = json.loads((staged / "backup-proof.json").read_text())
            self.assertEqual(proof["backupSha256"], state_tool.digest(private / "backup.json"))
            self.assertEqual(proof["fileCount"], 1)
            for path in staged.rglob("*"):
                if path.is_file():
                    self.assertNotIn(b"DO_NOT_EXPORT", path.read_bytes())
                    self.assertNotIn(b"PRIVATE_DATABASE", path.read_bytes())
                    self.assertNotIn(b"private-evidence-name", path.read_bytes())
                    self.assertNotIn(b"NOT_ALLOWLISTED", path.read_bytes())
                    if os.name == "posix":
                        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                        self.assertEqual(path.stat().st_uid, uid)
            self.assertEqual(private.stat().st_mode, owner.st_mode)
            self.assertEqual(private.stat().st_uid, owner.st_uid)
            if os.name == "posix":
                self.assertEqual(staged.stat().st_mode & 0o777, 0o700)
                self.assertEqual(staged.stat().st_uid, uid)
                if os.geteuid() == 0:
                    os.chown(root, uid, gid)
                    subprocess.run([
                        sys.executable, "-c",
                        "from pathlib import Path; import sys\n"
                        "root=Path(sys.argv[1])\n"
                        "for p in (root/'flaggo-diagnostics').rglob('*'):\n"
                        " if p.is_file(): p.read_bytes()\n"
                        "try: (root/'state-backup/backup.json').read_bytes()\n"
                        "except PermissionError: pass\n"
                        "else: raise AssertionError('private backup became readable')\n",
                        str(root),
                    ], user=uid, group=gid, check=True, cwd=root)

    @unittest.skipUnless(os.name == "posix", "POSIX symlink diagnostics")
    def test_diagnostics_refuse_symlinked_allowlisted_file_without_following_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "private").write_text("secret")
            (root / "installed-reset.json").symlink_to(root / "private")
            result = diagnostics.collect(root, root, os.getuid(), os.getgid())
            self.assertTrue(any("symlink refused" in error for error in result["errors"]))
            self.assertFalse((root / "flaggo-diagnostics/installed-reset.json").exists())

    def test_artifact_filter_preserves_exact_safe_link_text_and_rejects_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            member = tarfile.TarInfo("flaggo-relocated/python/python")
            member.type = tarfile.SYMTYPE
            member.linkname = "./bin/python3.12"
            self.assertEqual(reuse.release_member(member, temporary).linkname,
                             "./bin/python3.12")
            member.linkname = "../../../escape"
            with self.assertRaises((tarfile.LinkOutsideDestinationError, RuntimeError)):
                reuse.release_member(member, temporary)

    def test_artifact_replay_rejects_mismatched_run_workflow_or_failed_build(self):
        evidence = json.loads(
            Path(__file__).with_name("release.provenance.json").read_text()
        )["artifactReplayPin"]
        workflow = evidence["sourceWorkflow"]
        run = {
            "id": workflow["runId"], "workflow_id": workflow["id"], "path": workflow["path"],
            "run_attempt": 1, "head_sha": evidence["sourceCommit"], "head_branch": workflow["branch"],
            "repository": {"full_name": reuse.REPOSITORY},
            "head_repository": {"full_name": reuse.REPOSITORY},
            "event": workflow["event"], "status": "completed", "conclusion": "failure",
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
            (0, "id", -1), (0, "head_sha", "wrong"), (0, "workflow_id", -1),
            (0, "event", "push"), (0, "run_attempt", 2),
            (0, "path", "wrong"), (0, "head_branch", "wrong"),
            (1, "id", -1), (1, "expired", True), (1, "digest", "wrong"),
            (1, "workflow_run", {**artifact["workflow_run"], "head_sha": "wrong"}),
            (2, "sha", "wrong"), (3, "id", -1), (3, "steps", []),
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

    def test_reset_rejects_cross_filesystem_archive_before_backup_or_source_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            state = root / "state"
            state.mkdir()
            (state / ".flaggo-state.json").write_text(json.dumps({
                "format": "flaggo.state/v1", "kind": "disposable-smoke", "stateId": "exact",
            }))
            (state / "flaggo.db").write_bytes(b"must not open or checkpoint this database")
            (state / "flaggo.db-wal").write_bytes(b"must not alter WAL")
            snapshot = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in state.iterdir()}
            for archive, message in [(state / "archive", "outside"), (root, "new path")]:
                with self.subTest(archive=archive):
                    with self.assertRaisesRegex(ValueError, message):
                        state_tool.reset(state, archive, root, root, "hash", "exact", Mock())
                    self.assertEqual(snapshot, {p.name: (p.read_bytes(), p.stat().st_mtime_ns)
                                                for p in state.iterdir()})
            other = root / "other-volume"
            other.mkdir()
            original_stat = Path.stat

            def other_device(path, *args, **kwargs):
                result = original_stat(path, *args, **kwargs)
                if path == other:
                    fields = list(result)
                    fields[2] += 1
                    return os.stat_result(fields)
                return result

            with patch.object(Path, "stat", other_device):
                with self.assertRaisesRegex(ValueError, "same filesystem"):
                    state_tool.reset(state, other / "new/archive", root, root, "hash", "exact", Mock())
            self.assertFalse((other / "new").exists())
            self.assertEqual(snapshot, {p.name: (p.read_bytes(), p.stat().st_mtime_ns)
                                        for p in state.iterdir()})
            if os.name == "posix" and Path("/dev/shm").is_dir():
                if Path("/dev/shm").stat().st_dev != state.stat().st_dev:
                    with tempfile.TemporaryDirectory(dir="/dev/shm") as separate_mount:
                        archive = Path(separate_mount) / "archive"
                        with self.assertRaisesRegex(ValueError, "same filesystem"):
                            state_tool.reset(state, archive, root, root, "hash", "exact", Mock())
                        self.assertFalse(archive.exists())
                        self.assertEqual(snapshot, {p.name: (p.read_bytes(), p.stat().st_mtime_ns)
                                                    for p in state.iterdir()})

    def test_reset_cli_cannot_redirect_to_unrelated_state(self):
        with patch.object(sys, "argv", ["state-tool.py", "reset-disposable", "--state", ".",
                                       "--archive", "archive", "--release", ".", "--release-sha256",
                                       "hash", "--confirm-state-id", "exact"]), \
                patch.object(state_tool.os.path, "ismount", return_value=True), \
                patch.object(Path, "samefile", return_value=False), \
                patch.object(state_tool, "reset") as reset:
            with self.assertRaisesRegex(ValueError, "exact configured state mount"):
                state_tool.main()
            reset.assert_not_called()

    @unittest.skipUnless(os.environ.get("FLAGGO_TEST_BIND_MOUNTS") == "1",
                         "Requires explicitly enabled isolated Linux mount capability")
    def test_reset_bind_alias_rejected_but_common_mount_archive_succeeds(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = root / "state"
            alias = root / "alias"
            state.mkdir(mode=0o700)
            alias.mkdir()
            marker = {"format": "flaggo.state/v1", "kind": "disposable-smoke", "stateId": "exact"}
            (state / ".flaggo-state.json").write_text(json.dumps(marker))
            workspace = state / "analysis-workspaces" / "sealed"
            workspace.mkdir(parents=True)
            (workspace / "evidence bytes.bin").write_bytes(b"accepted workspace\x00\xff")
            (state / "copilot-home").mkdir(mode=0o700)
            (state / "copilot-home/private-cache").write_bytes(b"do not replace")
            with closing(sqlite3.connect(state / "flaggo.db")) as db:
                db.execute("CREATE TABLE authority (digest TEXT)")
                db.execute("INSERT INTO authority VALUES ('sha256:accepted')")
                db.commit()
            release = root / "release"
            release.mkdir()
            (release / "release.json").write_text(json.dumps({
                "format": "flaggo.release/v1", "sourceCommit": "test", "files": [],
            }))
            config = root / "config"
            config.mkdir()
            (config / "analysis.env").write_text("EXAMPLE=value")
            release_hash = state_tool.digest(release / "release.json")
            archive = root / "archive"
            subprocess.run(["mount", "--bind", str(state), str(alias)], check=True)
            try:
                self.assertTrue(alias.samefile(state))
                self.assertEqual(alias.stat().st_dev, archive.parent.stat().st_dev)
                snapshot = {p.relative_to(state): p.read_bytes() for p in state.rglob("*")
                            if p.is_file()}
                with self.assertRaisesRegex(ValueError, "same filesystem and mount"):
                    state_tool.reset(alias, archive, release, config, release_hash, "exact", Mock())
                self.assertEqual(snapshot, {p.relative_to(state): p.read_bytes()
                                            for p in state.rglob("*") if p.is_file()})
                self.assertFalse(archive.exists())
                child = subprocess.Popen([
                    sys.executable, "-c",
                    "import sys,time; f=open(sys.argv[1]); print('ready',flush=True); time.sleep(60)",
                    str(state / "flaggo.db"),
                ], stdout=subprocess.PIPE, text=True)
                try:
                    self.assertEqual(child.stdout.readline().strip(), "ready")
                    with patch.object(state_tool.subprocess, "run", return_value=Mock(stdout="0\n")):
                        with self.assertRaisesRegex(RuntimeError, "state handle"):
                            state_tool.assert_stopped(state)
                finally:
                    child.terminate()
                    child.wait()
                    child.stdout.close()
                result = state_tool.reset(
                    state, archive, release, config, release_hash, "exact", Mock(),
                )
                self.assertFalse((alias / "flaggo.db").exists())
                self.assertNotEqual(json.loads((alias / ".flaggo-state.json").read_text())["stateId"],
                                    "exact")
                self.assertEqual(state_tool.digest(archive / "backup.json"), result["backupSha256"])
                self.assertEqual((archive / "quiesced-original").stat().st_mode & 0o777, 0o700)
                self.assertEqual((archive / "quiesced-original").stat().st_uid, 0)
                restored = root / "restored"
                state_tool.restore(archive, restored, result["backupSha256"], release_hash, Mock())
                root.chmod(0o711)
                import pwd
                try:
                    owner = pwd.getpwnam("flaggo")
                except KeyError:
                    subprocess.run(["useradd", "--system", "--no-create-home", "flaggo"], check=True)
                    owner = pwd.getpwnam("flaggo")
                os.chown(state, owner.pw_uid, owner.pw_gid)
                mount_identity = (alias.stat().st_dev, alias.stat().st_ino)
                mount_domain = state_tool.rename_domain(alias)
                self.assertNotEqual(mount_domain, state_tool.rename_domain(state))
                helper = Path(__file__).with_name("installed-checks.sh").read_text()

                def restore_copy(source, destination):
                    return subprocess.run(
                        ["bash", "-euo", "pipefail", "-c", helper + """
restore_installed_state "$1" "$2" "$3"
printf completed
""", "restore-test", str(source), str(destination), owner.pw_name],
                        capture_output=True, text=True,
                    )

                failed_source = root / "incomplete-copy"
                failed_source.mkdir()
                before_marker = (alias / ".flaggo-state.json").read_bytes()
                failed = restore_copy(failed_source, alias)
                self.assertNotEqual(failed.returncode, 0)
                self.assertNotIn("completed", failed.stdout)
                self.assertEqual((alias / ".flaggo-state.json").read_bytes(), before_marker)
                self.assertFalse((alias / "flaggo.db").exists())
                blocked = root / "blocked"
                blocked.mkdir(mode=0o700)
                failed = restore_copy(restored, blocked)
                self.assertNotEqual(failed.returncode, 0)
                self.assertNotIn("completed", failed.stdout)
                self.assertEqual(list(blocked.iterdir()), [])
                successful = restore_copy(restored, alias)
                self.assertEqual(successful.returncode, 0, successful.stderr)
                self.assertEqual(successful.stdout, "completed")
                self.assertEqual((alias.stat().st_dev, alias.stat().st_ino), mount_identity)
                self.assertEqual(state_tool.rename_domain(alias), mount_domain)
                self.assertEqual(alias.stat().st_mode & 0o777, 0o700)
                self.assertEqual((alias / "flaggo.db").stat().st_uid, owner.pw_uid)
                self.assertEqual(json.loads((alias / ".flaggo-state.json").read_text()), marker)
                with closing(sqlite3.connect(alias / "flaggo.db")) as db:
                    self.assertEqual(db.execute("SELECT digest FROM authority").fetchone()[0],
                                     "sha256:accepted")
                self.assertEqual(restored.stat().st_uid, 0)
                self.assertEqual(archive.stat().st_uid, 0)
                self.assertEqual((alias / "analysis-workspaces/sealed/evidence bytes.bin").read_bytes(),
                                 b"accepted workspace\x00\xff")
                self.assertEqual((alias / "copilot-home/private-cache").read_bytes(), b"do not replace")
                self.assertEqual((restored / "analysis-workspaces/sealed/evidence bytes.bin").stat().st_uid,
                                 0)
            finally:
                subprocess.run(["umount", str(alias)], check=True)

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
