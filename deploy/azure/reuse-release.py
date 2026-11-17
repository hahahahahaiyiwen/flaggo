"""Verify and extract the one reviewed Linux release; never build or deploy."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile


REPOSITORY = "hahahahahaiyiwen/flaggo"
ROOT = Path(__file__).resolve().parents[2]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def gh(endpoint):
    return json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{REPOSITORY}/{endpoint}"], text=True,
    ))


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_metadata(evidence, run, artifact, workflow_file, job):
    source = evidence["sourceCommit"]
    workflow = evidence["sourceWorkflow"]
    expected_artifact = evidence["artifact"]
    require(run["id"] == workflow["runId"]
            and run["workflow_id"] == workflow["id"]
            and run["path"] == workflow["path"]
            and run["run_attempt"] == workflow["runAttempt"]
            and run["head_sha"] == source and run["head_branch"] == workflow["branch"]
            and run["repository"]["full_name"] == REPOSITORY
            and run["head_repository"]["full_name"] == REPOSITORY
            and run["event"] == workflow["event"] and run["status"] == "completed"
            and run["conclusion"] == evidence["runConclusion"],
            "Retained run/source/workflow identity mismatch")
    require(workflow_file["sha"] == workflow["gitBlob"]
            and workflow_file["path"] == workflow["path"],
            "Retained workflow content does not match the reviewed source")
    require(artifact["id"] == expected_artifact["id"]
            and artifact["name"] == expected_artifact["name"]
            and artifact["size_in_bytes"] == expected_artifact["sizeBytes"]
            and artifact["digest"] == "sha256:" + expected_artifact["zipSha256FromGitHub"]
            and not artifact["expired"]
            and artifact["workflow_run"]["id"] == run["id"]
            and artifact["workflow_run"]["head_sha"] == source
            and artifact["workflow_run"]["head_branch"] == workflow["branch"],
            "Retained artifact identity/digest/availability mismatch")
    require(job["id"] == workflow["releaseJobId"] and job["run_id"] == run["id"]
            and job["head_sha"] == source and job["name"] == "linux-release"
            and job["status"] == "completed" and job["conclusion"] == "failure",
            "Retained release job identity mismatch")
    steps = {step["name"]: step["conclusion"] for step in job["steps"]}
    require(steps.get("Build committed versioned artifacts") == "success"
            and steps.get("Relocate and probe without credentials or maintainer caches") == "success"
            and steps.get("Exercise installed private services and offline recovery on the runner") == "failure"
            and steps.get("Run actions/upload-artifact@v4") == "success",
            "The reviewed packaging/probe/upload steps did not succeed")


def release_member(member, destination):
    filtered = tarfile.data_filter(member, destination)
    if member.issym() or member.islnk():
        root = Path(destination).resolve()
        origin = (root / member.name).parent if member.issym() else root
        require((origin / member.linkname).resolve().is_relative_to(root),
                "Original release link target escapes extraction directory")
        # data_filter may normalize "./" away, changing manifest-covered link bytes.
        filtered = filtered.replace(linkname=member.linkname)
    return filtered


def extract_release(evidence, archive_zip, output):
    expected = evidence["artifact"]
    require(archive_zip.stat().st_size == expected["sizeBytes"]
            and digest(archive_zip) == expected["zipSha256FromGitHub"],
            "Downloaded artifact ZIP size/checksum mismatch")
    archive_tar = output / "flaggo-release.tar.gz"
    with zipfile.ZipFile(archive_zip) as zipped:
        require(zipped.namelist().count("flaggo-release.tar.gz") == 1,
                "Reviewed artifact ZIP layout mismatch")
        with zipped.open("flaggo-release.tar.gz") as source, archive_tar.open("wb") as target:
            shutil.copyfileobj(source, target)
    require(digest(archive_tar) == expected["archiveSha256VerifiedAfterDownload"],
            "Sealed Linux archive checksum mismatch")
    with tarfile.open(archive_tar, "r:gz") as archive:
        with archive.extractfile("flaggo-relocated/release.json") as stream:
            manifest_bytes = stream.read()
        require(hashlib.sha256(manifest_bytes).hexdigest()
                == expected["manifestSha256VerifiedAfterDownload"],
                "Sealed release manifest checksum mismatch")
        manifest = json.loads(manifest_bytes)
        require(manifest["format"] == "flaggo.release/v1"
                and manifest["sourceCommit"] == evidence["sourceCommit"]
                and manifest["pythonLockSha256"] == evidence["runtime"]["pythonLockSha256"],
                "Sealed release source/lock identity mismatch")
        require(all(Path(member.name).parts[0] == "flaggo-relocated"
                    for member in archive.getmembers()), "Unexpected archive root")
        archive.extractall(output, filter=release_member)
    return hashlib.sha256(manifest_bytes).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    require(os.environ.get("GITHUB_ACTIONS") == "true"
            and os.environ.get("GITHUB_REPOSITORY") == REPOSITORY,
            "Only the reviewed repository's Actions runner may reuse this release")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    require(head == os.environ.get("GITHUB_SHA"), "Test-driver checkout identity mismatch")
    require(not subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], text=True,
    ).strip(), "Test-driver tracked files must be clean")
    evidence = json.loads(
        (ROOT / "deploy/azure/release.provenance.json").read_text()
    )["artifactReplayPin"]
    workflow = evidence["sourceWorkflow"]
    run = gh(f"actions/runs/{workflow['runId']}")
    artifact = gh(f"actions/artifacts/{evidence['artifact']['id']}")
    workflow_file = gh(f"contents/{workflow['path']}?ref={evidence['sourceCommit']}")
    job = gh(f"actions/jobs/{workflow['releaseJobId']}")
    verify_metadata(evidence, run, artifact, workflow_file, job)
    replay_run = gh(f"actions/runs/{os.environ['GITHUB_RUN_ID']}")
    require(replay_run["head_sha"] == head and replay_run["event"] == "workflow_dispatch"
            and replay_run["workflow_id"] == workflow["id"]
            and replay_run["path"] == workflow["path"] and replay_run["run_attempt"] == 1
            and replay_run["head_branch"] == workflow["branch"],
            "Current artifact-only dispatch provenance mismatch")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    archive_zip = output / "retained-artifact.zip"
    with archive_zip.open("wb") as stream:
        subprocess.run(["gh", "api",
                        f"repos/{REPOSITORY}/actions/artifacts/{artifact['id']}/zip"],
                       stdout=stream, check=True)
    manifest_hash = extract_release(evidence, archive_zip, output)
    proof = {
        "format": "flaggo.artifact-replay/v1",
        "sourceCommit": evidence["sourceCommit"],
        "testDriverCommit": head,
        "sourceWorkflow": workflow,
        "artifact": evidence["artifact"],
        "replayRunId": replay_run["id"],
        "testDriverWorkflowBlob": subprocess.check_output(
            ["git", "rev-parse", f"HEAD:{workflow['path']}"], text=True,
        ).strip(),
        "downloadedZipSha256": digest(archive_zip),
        "extractedManifestSha256": manifest_hash,
        "buildPerformed": False,
        "azureDeployed": False,
        "providerCredentialsConnected": False,
    }
    (output / "artifact-input-provenance.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof))


if __name__ == "__main__":
    main()
