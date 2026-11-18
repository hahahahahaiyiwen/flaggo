"""Build a separate, locked consumer of one immutable product revision."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess


def run(*args, cwd):
    subprocess.run(args, cwd=cwd, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, help="Public Git URL, or file URL for explicitly local verification")
    parser.add_argument("--revision", required=True, help="Full 40-character immutable Git commit")
    parser.add_argument("--output", required=True, type=Path, help="New directory for consumer source, lockfile and results")
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.revision):
        parser.error("--revision must be a full lowercase commit SHA")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    crates = [
        "flaggo-contract", "flaggo-expression", "flaggo-decision",
        "flaggo-contract-application", "flaggo-decision-http",
        "flaggo-executable-store", "flaggo-core-conformance", "flaggo-analysis-workspace",
        "flaggo-evidence-store", "flaggo-raw-otlp-inbox",
        "flaggo-service-hosting",
        "flaggo-otlp-http",
        "flaggo-analysis-application", "flaggo-analysis-execution",
        "flaggo-evidence-materialization",
    ]
    manifest = '[package]\nname = "flaggo-pinned-consumer"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\n\n[dependencies]\n'
    for crate in crates:
        manifest += f'{crate} = {{ git = {json.dumps(args.repository)}, rev = "{args.revision}", default-features = false }}\n'
    manifest += 'tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread"] }\n'
    manifest += 'serde_json = "1.0.151"\n'
    (output / "Cargo.toml").write_text(manifest, encoding="utf-8")
    (output / "src").mkdir()
    shutil.copyfile(Path(__file__).with_name("main.rs"), output / "src" / "main.rs")
    run("cargo", "generate-lockfile", cwd=output)
    run("cargo", "run", "--locked", "--quiet", cwd=output)
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=output, text=True
    ))
    products = [item for item in metadata["packages"] if item["name"].startswith("flaggo-") and item["name"] != "flaggo-pinned-consumer"]
    for product in products:
        if not (product["source"] or "").endswith("#" + args.revision):
            raise RuntimeError(f"Unpinned product dependency: {product['name']}")
    forbidden = [item["name"] for item in metadata["packages"] if item["name"].startswith(("sqlx", "libsqlite3"))]
    if forbidden:
        raise RuntimeError(f"Portable consumer unexpectedly depends on SQLite: {forbidden}")
    apps = {"flaggo-async-analysis", "flaggo-contract-service", "flaggo-decision-service",
            "flaggo-otel-ingestion", "flaggo-evidence-materializer"}
    if apps.intersection(item["name"] for item in products):
        raise RuntimeError("Portable consumer unexpectedly depends on an app composition root")
    report = {
        "repository": args.repository, "revision": args.revision,
        "scope": "local-git-consumer" if args.repository.startswith("file:") else "remote-git-consumer",
        "result": "passed", "productPackages": sorted(item["name"] for item in products),
        "wire": "v4", "authorityCapability": "flaggo.authority/v3",
        "authorityPolicy": "flaggo.authority.policy/v1",
        "policyFixtures": "flaggo.authority-policy-conformance/v1",
        "securityCapability": "flaggo.security.resource/v1",
        "evidenceProfile": "flaggo.evidence.sqlite/v1",
        "projectionCapability": "flaggo.evidence.projection/v2",
        "conformanceFixtures": "flaggo.authority-conformance/v3",
        "admissionFixtures": "flaggo.admission-conformance/v1",
        "canonicalFixtures": "flaggo.canonical-conformance/v1",
        "runtimeFixtures": "flaggo.runtime-conformance/v1",
        "workspaceCapability": "flaggo.workspace/v2",
        "executionCapability": "flaggo.execution/v1",
        "workspaceFixtures": "flaggo.workspace-conformance/v1",
        "distributedQualification": False,
    }
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
