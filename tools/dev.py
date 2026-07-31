#!/usr/bin/env python3
"""Portable local-development entrypoint for the Flaggo monorepo."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VALIDATOR = ROOT / "contracts" / "conformance" / "validate.py"
MOCK_SERVER = ROOT / "contracts" / "mock" / "fixture-server" / "server.py"


def run(command: list[str]) -> int:
    return subprocess.run(command, cwd=ROOT, check=False).returncode


def check() -> int:
    return run([sys.executable, str(VALIDATOR)])


def serve(host: str, port: int) -> int:
    return run(
        [
            sys.executable,
            str(MOCK_SERVER),
            "--host",
            host,
            "--port",
            str(port),
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("check", help="Run the executable contract gate.")

    serve_parser = subcommands.add_parser(
        "serve",
        help="Start the fixture-backed local API.",
    )
    serve_parser.add_argument(
        "--host",
        default=os.getenv("FLAGGO_HOST", "127.0.0.1"),
    )
    serve_parser.add_argument(
        "--port",
        type=int,
        default=int(os.getenv("FLAGGO_PORT", "8080")),
    )

    args = parser.parse_args()
    if args.command == "check":
        return check()
    return serve(args.host, args.port)


if __name__ == "__main__":
    raise SystemExit(main())
