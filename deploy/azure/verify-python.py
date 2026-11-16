"""Verify the supported scientific environment and record its actual identity."""

import argparse
import importlib
import importlib.metadata
import json
import platform
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if sys.implementation.name != "cpython" or sys.version_info[:2] != (3, 12):
        raise RuntimeError("The backend scientific environment requires CPython 3.12")
    if sys.platform != "linux" or platform.machine() != "x86_64":
        raise RuntimeError("The backend scientific environment requires Linux x86-64")
    packages = {}
    for distribution, module in [
        ("numpy", "numpy"),
        ("pandas", "pandas"),
        ("scipy", "scipy"),
        ("scikit-learn", "sklearn"),
        ("matplotlib", "matplotlib"),
        ("seaborn", "seaborn"),
    ]:
        importlib.import_module(module)
        packages[distribution] = importlib.metadata.version(distribution)
    args.output.write_text(
        json.dumps(
            {
                "implementation": sys.implementation.name,
                "python": platform.python_version(),
                "platform": platform.platform(),
                "machine": platform.machine(),
                "packages": packages,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
