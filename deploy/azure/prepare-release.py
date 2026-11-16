"""Build a versioned Linux release from a clean checkout, without cloud access."""

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BINARIES = [
    "flaggo-contract-service",
    "flaggo-decision-service",
    "flaggo-otel-ingestion",
    "flaggo-evidence-materializer",
    "flaggo-async-analysis",
    "flaggo-tetris-playground-analysis",
]


def run(*command, env=None, capture=False):
    return subprocess.run(
        command, cwd=ROOT, env=env, check=True, text=True,
        stdout=subprocess.PIPE if capture else None,
    ).stdout


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def copy_tree(source, destination):
    shutil.copytree(
        source, destination, symlinks=True,
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "linux" or platform.machine() != "x86_64":
        raise RuntimeError("Release preparation requires Linux x86-64")
    if sys.version_info[:2] != (3, 12):
        raise RuntimeError("Release preparation requires CPython 3.12")
    if run("git", "status", "--porcelain", "--untracked-files=no", capture=True).strip():
        raise RuntimeError("Build from a clean committed tree, not preserved website edits")
    commit = run("git", "rev-parse", "HEAD", capture=True).strip()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "bin").mkdir()
    environment = {
        **os.environ,
        "COPILOT_CLI_EXTRACT_DIR": str(output / "copilot"),
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
    }
    for name in [
        "GH_TOKEN", "GITHUB_TOKEN", "COPILOT_GITHUB_TOKEN", "COPILOT_SDK_AUTH_TOKEN",
        "OPENAI_API_KEY", "AZURE_OPENAI_API_KEY", "ANTHROPIC_API_KEY",
    ]:
        environment.pop(name, None)
    run(
        "cargo", "build", "--release", "--locked",
        *[value for name in BINARIES for value in ("--bin", name)], env=environment,
    )
    run(
        "cargo", "build", "-p", "flaggo-analysis-agent", "--features", "copilot",
        "--release", "--locked", "--example", "runtime-probe", env=environment,
    )
    for name in BINARIES:
        shutil.copy2(ROOT / "target/release" / name, output / "bin" / name)
    shutil.copy2(
        ROOT / "target/release/examples/runtime-probe", output / "bin/flaggo-runtime-probe",
    )
    copy_tree(ROOT / "modules/analysis-agent/agent/skills", output / "skills")
    run("npm", "ci", "--ignore-scripts", env=environment)
    run("npm", "run", "build:tetris-playground", env=environment)
    (output / "node/bin").mkdir(parents=True)
    shutil.copy2(shutil.which("node"), output / "node/bin/node")
    for path in [
        "examples/tetris-playground/dist", "examples/tetris-playground/public",
        "examples/tetris-integration/dist", "packages/sdk-typescript", "node_modules",
    ]:
        copy_tree(ROOT / path, output / path)
    for path in [
        "examples/tetris-playground/package.json", "examples/tetris-integration/package.json",
    ]:
        shutil.copy2(ROOT / path, output / path)
    # The application packages use relative workspace links. Preserve their layout.
    copy_tree(ROOT / "deploy/azure", output / "deploy/azure")
    copy_tree(Path(sys.base_prefix), output / "python")
    python = output / "python/bin/python3.12"
    python_environment = {
        **environment,
        "PYTHONHOME": str(output / "python"),
        "LD_LIBRARY_PATH": str(output / "python/lib"),
    }
    lock = ROOT / "apps/async-analysis/requirements-linux.txt"
    shutil.copy2(lock, output / "requirements-linux.txt")
    run(
        str(python), "-m", "pip", "install", "--ignore-installed",
        "--require-hashes", "--only-binary=:all:",
        "--index-url", "https://pypi.org/simple", "-r", str(lock),
        "--report", str(output / "python-wheels.json"), env=python_environment,
    )
    run(
        str(python), str(ROOT / "deploy/azure/verify-python.py"),
        "--output", str(output / "python-runtime.json"), env=python_environment,
    )
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1", capture=True))
    sdk = next(package for package in metadata["packages"] if package["name"] == "github-copilot-sdk")
    sdk_root = Path(sdk["manifest_path"]).parent
    for path in ["LICENSE", "Cargo.lock", "package-lock.json"]:
        shutil.copy2(ROOT / path, output / Path(path).name)
    shutil.copy2(sdk_root / "cli-version-in-process.txt", output / "copilot-version.txt")
    shutil.copy2(sdk_root / "LICENSE", output / "COPILOT-SDK-LICENSE")
    files = []
    for path in sorted(output.rglob("*")):
        if path.is_file() and not path.is_symlink():
            files.append({
                "path": path.relative_to(output).as_posix(),
                "sha256": digest(path), "bytes": path.stat().st_size,
            })
        elif path.is_symlink():
            resolved = path.resolve()
            if not resolved.is_relative_to(output):
                raise RuntimeError(f"Release symlink escapes the artifact: {path}")
    manifest = {
        "format": "flaggo.release/v1",
        "sourceCommit": commit,
        "platform": platform.platform(),
        "python": platform.python_version(),
        "node": run("node", "--version", capture=True).strip(),
        "rust": run("rustc", "--version", capture=True).strip(),
        "copilotSdk": sdk["version"],
        "pythonLockSha256": digest(lock),
        "files": files,
    }
    (output / "release.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
