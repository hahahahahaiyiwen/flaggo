"""Opt-in real-host HTTP migration gates with fail-closed result accounting."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path


PREFIX = "FLAGGO_HTTP_PARITY "
FORMAT = "flaggo.http-parity/v1"
MINIMUM_COMPARISONS = {"contract": 39, "decision": 29}
PRODUCERS = {"dotnet", "rust"}


def parse_results(stdout: str) -> dict:
    observed = {}
    for line in stdout.splitlines():
        if not line.startswith(PREFIX):
            continue
        record = json.loads(line[len(PREFIX):])
        if (not isinstance(record, dict)
                or set(record) != {"format", "service", "producer", "comparisons"}
                or record["format"] != FORMAT
                or not isinstance(record["service"], str)
                or record["service"] not in MINIMUM_COMPARISONS
                or not isinstance(record["producer"], str)
                or record["producer"] not in PRODUCERS
                or type(record["comparisons"]) is not int
                or record["comparisons"] < MINIMUM_COMPARISONS[record["service"]]):
            raise ValueError(f"Invalid HTTP parity result: {record!r}")
        key = (record["service"], record["producer"])
        if key in observed:
            raise ValueError(f"Duplicate HTTP parity result: {key}")
        observed[key] = record["comparisons"]
    expected = {(service, producer) for service in MINIMUM_COMPARISONS for producer in PRODUCERS}
    if observed.keys() != expected:
        raise ValueError(f"Missing HTTP parity results: {sorted(expected - observed.keys())}")
    services = {
        service: {producer: observed[service, producer] for producer in sorted(PRODUCERS)}
        for service in MINIMUM_COMPARISONS
    }
    count = sum(observed.values())
    return {"status": "passed", "cases": count, "matched": count, "mismatches": [],
            "services": services}


def _run(command: list[str], root: Path, log, *, env=None) -> str:
    log.write(f"$ {subprocess.list2cmdline(command)}\n")
    log.flush()
    result = subprocess.run(command, cwd=root, text=True, encoding="utf-8",
                            capture_output=True, check=False, env=env)
    log.write(result.stdout + "\n" + result.stderr + "\n")
    log.flush()
    if result.returncode != 0:
        raise RuntimeError(
            f"HTTP acceptance command exited {result.returncode}: "
            f"{subprocess.list2cmdline(command)}; see {log.name}")
    return result.stdout


def verify(root: Path, log_path: Path, *, enabled: bool, build_reference: bool,
           reference_root: Path | None = None) -> dict:
    if not enabled:
        return {"status": "not-requested", "reason": "Select --profile --http to run live HTTP gates."}
    if reference_root is None:
        raise ValueError("Live HTTP parity requires an explicit historical reference checkout.")
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as log:
        if build_reference:
            for service, project in [
                ("contract", "Flaggo.ContractService"), ("decision", "Flaggo.DecisionService"),
            ]:
                _run(["dotnet", "build",
                      str(reference_root / "apps" / f"{service}-service" / "src" / project / f"{project}.csproj"),
                      "--configuration", "Release", "--nologo", "--verbosity", "quiet"],
                     reference_root, log)
        stdout = _run([
            "cargo", "test", "--locked", "--color", "never",
            "-p", "flaggo-contract-service", "-p", "flaggo-decision-service",
            "--test", "http", "actual_dotnet_", "--", "--ignored", "--nocapture",
        ], root, log, env={**os.environ, "FLAGGO_DOTNET_REFERENCE_ROOT": str(reference_root)})
        result = parse_results(stdout)
    result["log"] = str(log_path)
    return result
