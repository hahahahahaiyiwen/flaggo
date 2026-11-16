"""Verify releases and make quiesced, checksum-verified offline recovery copies."""

import argparse
from contextlib import closing
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import uuid
from pathlib import Path

UNITS = [
    "flaggo-gateway", "flaggo-smoke-analysis", "flaggo@async-analysis",
    "flaggo@evidence-materializer", "flaggo@otel-ingestion",
    "flaggo@decision-service", "flaggo@contract-service", "flaggo-observability",
]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def contained_file(root, relative):
    path = root / relative
    if not relative or Path(relative).is_absolute() or ".." in Path(relative).parts:
        raise ValueError(f"Invalid manifest path: {relative}")
    if not path.resolve().is_relative_to(root.resolve()) or path.is_symlink():
        raise ValueError(f"Manifest path escapes its root or is a symlink: {relative}")
    return path


def verify(root, manifest):
    for item in manifest["files"]:
        path = contained_file(root, item["path"])
        if path.stat().st_size != item["bytes"] or digest(path) != item["sha256"]:
            raise ValueError(f"Artifact checksum/size mismatch: {path}")


def release(root, expected):
    if digest(root / "release.json") != expected:
        raise ValueError("Installed release manifest does not match the approved hash")
    manifest = read_json(root / "release.json")
    if manifest["format"] != "flaggo.release/v1":
        raise ValueError("Unsupported release manifest")
    verify(root, manifest)
    for link in manifest.get("symlinks", []):
        path = root / link["path"]
        if (Path(link["path"]).is_absolute() or ".." in Path(link["path"]).parts
                or not path.is_symlink() or os.readlink(path) != link["target"]
                or not path.resolve().is_relative_to(root.resolve())):
            raise ValueError(f"Release symlink mismatch: {path}")
    return manifest


def assert_stopped():
    if os.name != "posix" or os.geteuid() != 0:
        raise RuntimeError("Quiesced host operations require Linux root")
    for unit in UNITS:
        result = subprocess.run(
            ["systemctl", "show", unit, "--property=MainPID", "--value"],
            check=True, capture_output=True, text=True,
        )
        if int(result.stdout.strip()) != 0:
            raise RuntimeError(f"Stop all writers before recovery operations: {unit}")
    # Include other host/container processes with open state handles.
    for process in Path("/proc").iterdir():
        if not process.name.isdecimal() or int(process.name) == os.getpid():
            continue
        try:
            descriptors = list((process / "fd").iterdir())
        except FileNotFoundError:
            continue
        for descriptor in descriptors:
            try:
                target = os.readlink(descriptor)
            except FileNotFoundError:
                continue
            if target == "/var/lib/flaggo" or target.startswith("/var/lib/flaggo/"):
                raise RuntimeError(f"Process {process.name} still holds a state handle")


def verify_state_copy(root, manifest):
    paths = list((root / "state").rglob("*"))
    if any(path.is_symlink() for path in paths):
        raise ValueError("A recovery copy must not contain symlinks")
    expected = {item["path"] for item in manifest["files"]}
    actual = {path.relative_to(root).as_posix() for path in paths if path.is_file()}
    if actual != expected:
        raise ValueError("The recovery tree differs from its manifest")
    verify(root, manifest)


def backup(state, output, release_root, config, expected, stopped=assert_stopped):
    stopped()
    release_manifest = release(release_root, expected)
    if not config.is_dir() or not list(config.glob("*.env")):
        raise ValueError("Backup requires the actual installed environment configuration directory")
    if output.resolve().is_relative_to(state.resolve()):
        raise ValueError("Backup destination must be outside the state tree")
    with closing(sqlite3.connect(f"{(state / 'flaggo.db').as_uri()}?mode=rw", uri=True)) as db:
        result = db.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        if result[0] != 0 or db.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise RuntimeError("SQLite checkpoint/integrity verification failed")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    for path in state.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"Recovery state must not contain symlinks: {path}")
    shutil.copytree(
        state, output / "state",
        ignore=lambda directory, _names: {"copilot-home"} if Path(directory) == state else set(),
    )
    files = [
        {"path": path.relative_to(output).as_posix(), "bytes": path.stat().st_size,
         "sha256": digest(path)}
        for path in sorted((output / "state").rglob("*")) if path.is_file()
    ]
    manifest = {
        "format": "flaggo.backup/v1", "releaseSha256": expected,
        "sourceCommit": release_manifest["sourceCommit"],
        "configuration": [
            {"path": path.name, "sha256": digest(path)}
            for path in sorted(config.glob("*.env"))
        ],
        "files": files,
        "excluded": ["copilot-home", "configuration values and provider credentials"],
    }
    stopped()
    verify_state_copy(output, manifest)
    (output / "backup.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return digest(output / "backup.json")


def restore(source, destination, expected, release_hash, stopped=assert_stopped):
    stopped()
    if digest(source / "backup.json") != expected:
        raise ValueError("Backup manifest does not match the approved hash")
    manifest = read_json(source / "backup.json")
    if manifest["format"] != "flaggo.backup/v1" or manifest["releaseSha256"] != release_hash:
        raise ValueError("Restore requires the backup's exact matching release")
    verify_state_copy(source, manifest)
    if destination.exists():
        raise ValueError("Restore into a new, separate empty path, never overwrite live state")
    shutil.copytree(source / "state", destination)
    with closing(sqlite3.connect(f"{(destination / 'flaggo.db').as_uri()}?mode=ro", uri=True)) as db:
        if db.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise RuntimeError("Restored database failed integrity verification")
    stopped()


def reset(state, archive, release_root, config, release_hash, state_id, stopped=assert_stopped):
    stopped()
    identity = read_json(state / ".flaggo-state.json")
    if (identity.get("format") != "flaggo.state/v1"
            or identity.get("kind") != "disposable-smoke" or identity.get("stateId") != state_id):
        raise ValueError("Reset requires the exact disposable smoke state identity")
    names = {
        "flaggo.db", "flaggo.db-wal", "flaggo.db-shm", "analysis-workspaces",
        "lgtm", ".flaggo-state.json",
    }
    if any(path.name not in names | {"lost+found", "copilot-home"} for path in state.iterdir()):
        raise ValueError("Unexpected state entries: reset requires operator review")
    backup_hash = backup(state, archive, release_root, config, release_hash, stopped)
    originals = archive / "quiesced-original"
    originals.mkdir(mode=0o700)
    for name in sorted(names):
        path = state / name
        if path.exists():
            path.rename(originals / name)
    identity["stateId"] = str(uuid.uuid4())
    (state / ".flaggo-state.json").write_text(json.dumps(identity) + "\n", encoding="utf-8")
    return {"backupSha256": backup_hash, "newStateId": identity["stateId"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    check = commands.add_parser("verify-release")
    check.add_argument("--release", type=Path, required=True)
    check.add_argument("--release-sha256", required=True)
    save = commands.add_parser("backup")
    save.add_argument("--state", type=Path, default=Path("/var/lib/flaggo"))
    save.add_argument("--output", type=Path, required=True)
    save.add_argument("--release", type=Path, required=True)
    save.add_argument("--config", type=Path, default=Path("/etc/flaggo"))
    save.add_argument("--release-sha256", required=True)
    load = commands.add_parser("restore")
    load.add_argument("--source", type=Path, required=True)
    load.add_argument("--destination", type=Path, required=True)
    load.add_argument("--backup-sha256", required=True)
    load.add_argument("--release-sha256", required=True)
    clear = commands.add_parser("reset-disposable")
    clear.add_argument("--archive", type=Path, required=True)
    clear.add_argument("--release", type=Path, required=True)
    clear.add_argument("--config", type=Path, default=Path("/etc/flaggo"))
    clear.add_argument("--release-sha256", required=True)
    clear.add_argument("--confirm-state-id", required=True)
    args = parser.parse_args()
    if args.action == "verify-release":
        release(args.release.resolve(), args.release_sha256)
        print("Release hashes verified")
    elif args.action == "backup":
        if args.state.resolve() != Path("/var/lib/flaggo"):
            raise ValueError("Online host backup only supports the configured state mount")
        if not os.path.ismount(args.state):
            raise RuntimeError("The configured state filesystem is not mounted")
        print(backup(
            args.state.resolve(), args.output.resolve(), args.release.resolve(),
            args.config.resolve(), args.release_sha256,
        ))
    elif args.action == "restore":
        restore(
            args.source.resolve(), args.destination.resolve(),
            args.backup_sha256, args.release_sha256,
        )
        print("Restored offline copy verified; no live state was switched")
    elif args.action == "reset-disposable":
        if not os.path.ismount("/var/lib/flaggo"):
            raise RuntimeError("The configured state filesystem is not mounted")
        print(json.dumps(reset(
            Path("/var/lib/flaggo"), args.archive.resolve(), args.release.resolve(),
            args.config.resolve(), args.release_sha256, args.confirm_state_id,
        )))


if __name__ == "__main__":
    main()
