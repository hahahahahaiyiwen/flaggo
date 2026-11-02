"""Explicit, immutable historical oracle; never a default service dependency."""

from pathlib import Path
import subprocess


REVISION = "c4532a103d68a6659ca8ea94e9a9595c7265d14e"


def resolve(root: Path, candidate: Path) -> Path:
    reference = candidate.resolve(strict=True)
    if not reference.is_dir() or reference.is_relative_to(root.resolve()):
        raise ValueError("The historical reference must be a separate checkout outside this tree.")
    for relative in (
        "tools/core-parity/dotnet/Flaggo.CoreParity.csproj",
        "apps/contract-service/src/Flaggo.ContractService/Flaggo.ContractService.csproj",
        "apps/decision-service/src/Flaggo.DecisionService/Flaggo.DecisionService.csproj",
    ):
        if not (reference / relative).is_file():
            raise ValueError(f"Historical reference is missing {relative}.")
    revision = subprocess.run(
        ["git", "-C", str(reference), "rev-parse", "HEAD"],
        text=True, encoding="utf-8", capture_output=True, check=True,
    ).stdout.strip()
    if revision != REVISION:
        raise ValueError(f"Historical reference must be pinned to {REVISION}; found {revision}.")
    changed = subprocess.run(
        ["git", "-C", str(reference), "status", "--porcelain", "--untracked-files=normal"],
        text=True, encoding="utf-8", capture_output=True, check=True,
    ).stdout.strip()
    if changed:
        raise ValueError("Historical reference has modified or untracked files; use a clean checkout.")
    return reference
