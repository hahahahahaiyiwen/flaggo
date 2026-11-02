import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import reference


class HistoricalReferenceTests(unittest.TestCase):
    def fixture(self, path):
        for relative in (
            "tools/core-parity/dotnet/Flaggo.CoreParity.csproj",
            "apps/contract-service/src/Flaggo.ContractService/Flaggo.ContractService.csproj",
            "apps/decision-service/src/Flaggo.DecisionService/Flaggo.DecisionService.csproj",
        ):
            file = path / relative
            file.parent.mkdir(parents=True, exist_ok=True)
            file.touch()

    def test_rejects_missing_current_tree_and_incomplete_references_before_git(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch("reference.subprocess.run") as command:
                for path in [root, root / "inside", root / "missing"]:
                    with self.subTest(path=path):
                        with self.assertRaises((ValueError, FileNotFoundError)):
                            reference.resolve(root, path)
                incomplete = root / "oracle"
                incomplete.mkdir()
                with self.assertRaisesRegex(ValueError, "missing"):
                    reference.resolve(root / "current", incomplete)
            command.assert_not_called()

    def test_accepts_only_pinned_clean_checkout_and_returns_absolute_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            oracle = root / "oracle"
            self.fixture(oracle)
            for revision, status, expected in [
                (reference.REVISION, "", None),
                ("0" * 40, "", "pinned"),
                (reference.REVISION, " M Program.cs", "modified or untracked"),
                (reference.REVISION, "?? Directory.Build.props", "modified or untracked"),
            ]:
                with self.subTest(revision=revision, status=status):
                    with patch("reference.subprocess.run", side_effect=[
                        subprocess.CompletedProcess([], 0, revision + "\n", ""),
                        subprocess.CompletedProcess([], 0, status, ""),
                    ]):
                        if expected:
                            with self.assertRaisesRegex(ValueError, expected):
                                reference.resolve(root / "current", oracle)
                        else:
                            self.assertEqual(oracle.resolve(),
                                             reference.resolve(root / "current", oracle))

    def test_git_failure_is_not_treated_as_clean(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            oracle = root / "oracle"
            self.fixture(oracle)
            with patch("reference.subprocess.run",
                       side_effect=subprocess.CalledProcessError(128, ["git"])):
                with self.assertRaises(subprocess.CalledProcessError):
                    reference.resolve(root / "current", oracle)
