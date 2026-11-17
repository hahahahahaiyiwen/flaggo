"""Stage allowlisted diagnostics from the credential-free installed runner only."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

SCRATCH_FILES = (
    "flaggo-replay-input/artifact-input-provenance.json",
    "installed-service-runtime.json", "installed-service-python.json",
    "installed-browser-result.json", "installed-recovery-status.json",
    "installed-recovery-result.json", "installed-result.json", "installed-restart.json",
    "installed-reset.json", "installed-journal.log", "installed-final-state.body",
    "installed-final-state-http.json", "installed-final-state-error.log",
    "analysis-stopped-parity.json", "restored-parity.json",
)
BROWSER_FILES = (
    "adaptive-evaluation.json", "correlation-state.json", "analysis-actions.json",
    "service-telemetry.json", "trace.zip", "test-failed-1.png", "error-context.md",
)


def source_file(root, relative):
    path = root
    for part in Path(relative).parts:
        if part in ("..", "") or Path(part).is_absolute():
            raise ValueError(f"Invalid diagnostic path: {relative}")
        path = path / part
        if path.is_symlink():
            raise ValueError(f"Diagnostic symlink refused: {relative}")
    if not path.exists():
        return None
    if not path.is_file():
        raise ValueError(f"Diagnostic is not a regular file: {relative}")
    return path


def collect(scratch, repository, uid, gid):
    output = scratch / "flaggo-diagnostics"
    output.mkdir(mode=0o700)
    if os.name == "posix":
        os.chown(output, uid, gid)
    report = {"files": [], "missing": [], "errors": []}

    def write(relative, data):
        destination = output / relative
        destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        destination.write_bytes(data)
        destination.chmod(0o600)
        if os.name == "posix":
            os.chown(destination, uid, gid)
            for parent in destination.parents:
                if parent == output:
                    break
                os.chown(parent, uid, gid)
        report["files"].append({
            "path": str(relative), "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })

    def copy(root, relative, destination):
        try:
            path = source_file(root, relative)
            if path is None:
                report["missing"].append(str(destination))
            else:
                write(destination, path.read_bytes())
        except (OSError, ValueError) as error:
            report["errors"].append(f"{destination}: {error}")

    for relative in SCRATCH_FILES:
        copy(scratch, relative, Path(relative).name)
    try:
        backup = source_file(scratch, "state-backup/backup.json")
        if backup is not None:
            data = backup.read_bytes()
            manifest = json.loads(data)
            write("backup-proof.json", (json.dumps({
                "format": manifest["format"], "releaseSha256": manifest["releaseSha256"],
                "sourceCommit": manifest["sourceCommit"],
                "backupSha256": hashlib.sha256(data).hexdigest(),
                "fileCount": len(manifest["files"]),
            }, indent=2) + "\n").encode())
        else:
            report["missing"].append("backup-proof.json")
    except (OSError, ValueError, KeyError) as error:
        report["errors"].append(f"backup-proof.json: {error}")

    browser = repository / "examples/tetris-playground/test-results"
    try:
        if browser.is_symlink():
            raise ValueError("Browser diagnostic root must not be a symlink")
        if browser.exists():
            copy(browser, ".last-run.json", "browser/.last-run.json")
            for directory in sorted(browser.iterdir()):
                if directory.is_symlink():
                    raise ValueError("Browser diagnostic directory must not be a symlink")
                if directory.is_dir():
                    for name in BROWSER_FILES:
                        copy(browser, directory.relative_to(browser) / name,
                             Path("browser") / directory.name / name)
    except (OSError, ValueError) as error:
        report["errors"].append(f"browser: {error}")
    write("diagnostic-index.json", (json.dumps(report, indent=2) + "\n").encode())
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("repository", type=Path)
    parser.add_argument("uid", type=int)
    parser.add_argument("gid", type=int)
    args = parser.parse_args()
    if os.name != "posix" or os.geteuid() != 0 or os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Collection requires root on the credential-free Actions runner")
    if args.uid <= 0 or args.gid <= 0:
        raise ValueError("Diagnostics must belong to the unprivileged runner")
    report = collect(args.scratch.resolve(), args.repository.resolve(), args.uid, args.gid)
    for error in report["errors"]:
        print(error, file=sys.stderr)
    if report["errors"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
