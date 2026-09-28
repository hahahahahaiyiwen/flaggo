#!/usr/bin/env python3
"""Offline conformance gate for the v3 management and runtime contracts."""
from __future__ import annotations

import hashlib
import json
import re
import sys
from copy import deepcopy
from pathlib import Path

import rfc8785
from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate_spec
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT202012

CONTRACTS = Path(__file__).resolve().parents[1]
SCHEMAS_DIR = CONTRACTS / "schemas"
OPENAPI_DIR = CONTRACTS / "openapi"
FIXTURES_DIR = CONTRACTS / "fixtures"
MANIFEST = CONTRACTS / "conformance" / "fixture-manifest-v1.json"
CANONICALIZATION_VECTORS = (
    CONTRACTS / "conformance" / "canonicalization-vectors-v1.json"
)
STRICT_JSON_VECTORS = CONTRACTS / "conformance" / "strict-json-vectors-v1.json"

SCHEMA_ID_PREFIX = "https://flaggo.dev/contracts/schemas/"
SCHEMA_FILES = [
    "runtime-models-v3.schema.json",
    "management-models-v3.schema.json",
    "problem-details-v3.schema.json",
]
OPENAPI_FILES = [
    "flaggo-runtime-v3.yaml",
    "flaggo-management-v3.yaml",
]
FIXTURE_REQUIRED_KEYS = {"name", "scenario", "invariant", "request", "expected"}
DIGEST_PATTERN = re.compile(r"^sha256:[0-9a-f]{64}$")
RUNTIME_PATH_PATTERN = re.compile(
    r"^/v3/decision-contracts/[^/]+/versions/(?P<digest>sha256:[0-9a-f]{64})/decisions$"
)
EXACT_VERSION_PATH_PATTERN = re.compile(
    r"^/v3/decision-contracts/[^/]+/versions/(?P<digest>sha256:[0-9a-f]{64})$"
)


class Report:
    def __init__(self) -> None:
        self.errors: list[str] = []
        self.checks = 0

    def check(self, ok: bool, message: str) -> bool:
        self.checks += 1
        if not ok:
            self.errors.append(message)
        return ok

    def fail(self, message: str) -> None:
        self.errors.append(message)


def reject_duplicate_object_keys(pairs: list[tuple[str, object]]) -> dict:
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON object key '{key}'")
        value[key] = item
    return value


def strict_json_loads(value: str):
    return json.loads(
        value,
        object_pairs_hook=reject_duplicate_object_keys,
        parse_constant=lambda constant: (_ for _ in ()).throw(
            ValueError(f"non-finite JSON number '{constant}' is forbidden")
        ),
    )


def load_json(path: Path):
    return strict_json_loads(path.read_text(encoding="utf-8"))


def build_registry(rep: Report) -> Registry:
    resources = []
    for name in SCHEMA_FILES:
        path = SCHEMAS_DIR / name
        if not path.exists():
            rep.fail(f"missing schema file: {path}")
            continue
        try:
            schema = load_json(path)
            Draft202012Validator.check_schema(schema)
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"invalid Draft 2020-12 schema {name}: {exc}")
            continue
        rep.check(True, "")
        resources.append(
            (
                SCHEMA_ID_PREFIX + name,
                Resource.from_contents(schema, default_specification=DRAFT202012),
            )
        )
    return Registry().with_resources(resources)


def validator_for_ref(
    schema_ref: str,
    registry: Registry,
) -> Draft202012Validator:
    file_part, separator, pointer = schema_ref.partition("#")
    target = SCHEMA_ID_PREFIX + file_part
    if separator:
        target += "#" + pointer
    return Draft202012Validator(
        {"$ref": target},
        registry=registry,
        format_checker=FormatChecker(),
    )


def validate_body(
    body,
    schema_ref: str,
    registry: Registry,
    rep: Report,
    context: str,
) -> None:
    try:
        errors = sorted(
            validator_for_ref(schema_ref, registry).iter_errors(body),
            key=lambda error: list(error.path),
        )
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{context}: could not validate against {schema_ref}: {exc}")
        return
    if errors:
        first = errors[0]
        location = "/".join(str(part) for part in first.path)
        rep.fail(
            f"{context}: body invalid against {schema_ref} at "
            f"'/{location}': {first.message}"
        )
    else:
        rep.check(True, "")


def expect_rejected(
    body,
    schema_ref: str,
    registry: Registry,
    rep: Report,
    context: str,
) -> None:
    try:
        accepted = validator_for_ref(schema_ref, registry).is_valid(body)
    except Exception as exc:  # noqa: BLE001
        rep.fail(
            f"{context}: could not run negative validation against "
            f"{schema_ref}: {exc}"
        )
        return
    rep.check(
        not accepted,
        f"{context}: body was expected to be rejected by {schema_ref}",
    )


def resolve_pointer(document, pointer: str):
    node = document
    for raw in pointer.split("/"):
        if not raw:
            continue
        token = raw.replace("~1", "/").replace("~0", "~")
        node = node[int(token)] if isinstance(node, list) else node[token]
    return node


def replace_pointer(document, pointer: str, value) -> None:
    tokens = [
        raw.replace("~1", "/").replace("~0", "~")
        for raw in pointer.split("/")
        if raw
    ]
    parent = document
    for token in tokens[:-1]:
        parent = parent[int(token)] if isinstance(parent, list) else parent[token]
    final = tokens[-1]
    if isinstance(parent, list):
        parent[int(final)] = value
    else:
        parent[final] = value


def collect_refs(node, refs: list[str]) -> None:
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                refs.append(value)
            else:
                collect_refs(value, refs)
    elif isinstance(node, list):
        for item in node:
            collect_refs(item, refs)


def absolutize_external_refs(node, base_dir: Path) -> None:
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and not ref.startswith("#"):
            file_part, separator, pointer = ref.partition("#")
            absolute = (base_dir / file_part).resolve().as_uri()
            node["$ref"] = absolute + (separator + pointer if separator else "")
        for value in node.values():
            absolutize_external_refs(value, base_dir)
    elif isinstance(node, list):
        for item in node:
            absolutize_external_refs(item, base_dir)


def check_ref_resolves(
    ref: str,
    document,
    base_dir: Path,
    rep: Report,
    context: str,
) -> None:
    file_part, _, pointer = ref.partition("#")
    if file_part:
        target_path = (base_dir / file_part).resolve()
        if not target_path.exists():
            rep.fail(f"{context}: $ref target file not found: {ref}")
            return
        try:
            target = load_json(target_path)
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"{context}: $ref target does not parse: {ref}: {exc}")
            return
    else:
        target = document
    if pointer:
        try:
            resolve_pointer(target, pointer)
        except Exception:  # noqa: BLE001
            rep.fail(f"{context}: $ref pointer does not resolve: {ref}")
            return
    rep.check(True, "")


def validate_openapi(path: Path, rep: Report) -> None:
    context = f"openapi:{path.name}"
    try:
        document = load_json(path)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{context}: does not parse: {exc}")
        return

    version = str(document.get("openapi", ""))
    rep.check(
        version.startswith("3.1"),
        f"{context}: OpenAPI version must be 3.1.x, got '{version}'",
    )
    try:
        validation_document = deepcopy(document)
        absolutize_external_refs(validation_document, path.parent)
        validate_spec(validation_document)
        rep.check(True, "")
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{context}: OpenAPI 3.1 validation failed: {exc}")

    paths = document.get("paths")
    rep.check(
        isinstance(paths, dict) and bool(paths),
        f"{context}: at least one path is required",
    )
    operation_ids: set[str] = set()
    for route, item in (paths or {}).items():
        for method, operation in item.items():
            if method not in {
                "get",
                "put",
                "post",
                "delete",
                "patch",
                "options",
                "head",
            }:
                continue
            operation_id = operation.get("operationId")
            rep.check(
                bool(operation_id),
                f"{context}: {method.upper()} {route} missing operationId",
            )
            if operation_id:
                rep.check(
                    operation_id not in operation_ids,
                    f"{context}: duplicate operationId '{operation_id}'",
                )
                operation_ids.add(operation_id)
            rep.check(
                bool(operation.get("responses")),
                f"{context}: {method.upper()} {route} missing responses",
            )

    refs: list[str] = []
    collect_refs(document, refs)
    for ref in sorted(set(refs)):
        check_ref_resolves(ref, document, path.parent, rep, context)


def load_openapi_operations() -> list[dict]:
    operations = []
    for name in OPENAPI_FILES:
        document = load_json(OPENAPI_DIR / name)
        for route, item in document["paths"].items():
            pattern = "^" + re.sub(r"\{[^}]+\}", r"[^/]+", route) + "$"
            for method, operation in item.items():
                if method not in {
                    "get",
                    "put",
                    "post",
                    "delete",
                    "patch",
                    "options",
                    "head",
                }:
                    continue
                scopes = {
                    scope
                    for requirement in operation.get(
                        "security",
                        document.get("security", []),
                    )
                    for scope in requirement.get("oauth2", [])
                }
                operations.append(
                    {
                        "method": method.upper(),
                        "route": route,
                        "pattern": re.compile(pattern),
                        "responses": set(operation["responses"]),
                        "scopes": scopes,
                    }
                )
    return operations


def match_operation(
    method: str,
    path: str,
    operations: list[dict],
) -> dict | None:
    return next(
        (
            operation
            for operation in operations
            if operation["method"] == method
            and operation["pattern"].fullmatch(path)
        ),
        None,
    )


def canonical_json_bytes(value) -> bytes:
    return rfc8785.dumps(value)


def validate_canonicalization_vectors(rep: Report) -> None:
    vectors = load_json(CANONICALIZATION_VECTORS).get("vectors", [])
    rep.check(bool(vectors), "canonicalization vector set is empty")
    for vector in vectors:
        try:
            actual = canonical_json_bytes(vector["input"]).decode("utf-8")
        except Exception as exc:  # noqa: BLE001
            rep.fail(
                f"canonicalization vector '{vector.get('name')}' failed: {exc}"
            )
            continue
        rep.check(
            actual == vector["expected"],
            f"canonicalization vector '{vector.get('name')}' mismatch: "
            f"{actual!r}",
        )


def validate_strict_json_vectors(rep: Report) -> None:
    vectors = load_json(STRICT_JSON_VECTORS).get("vectors", [])
    rep.check(bool(vectors), "strict JSON vector set is empty")
    for vector in vectors:
        try:
            strict_json_loads(vector["input"])
        except ValueError:
            accepted = False
        else:
            accepted = True
        rep.check(
            accepted == vector["accepted"],
            f"strict JSON vector '{vector['name']}' acceptance mismatch",
        )


def normalize_contract(contract: dict) -> dict:
    normalized = deepcopy(contract)

    def normalize_schema(schema: dict) -> None:
        schema.pop("description", None)
        if isinstance(schema.get("items"), dict):
            normalize_schema(schema["items"])
        for property_schema in schema.get("properties", {}).values():
            normalize_schema(property_schema)

    for attribute in normalized.get("attributes", []):
        normalize_schema(attribute["schema"])
    normalize_schema(normalized["result"]["schema"])
    for rule in normalized.get("authoredExecutable", {}).get("rules", []):
        rule.pop("description", None)
    for evidence in normalized.get("learning", {}).get("evidence", []):
        evidence.pop("description", None)
    for guardrail in (
        normalized.get("learning", {})
        .get("objective", {})
        .get("guardrails", [])
    ):
        guardrail.pop("description", None)

    normalized["attributes"] = sorted(
        normalized.get("attributes", []),
        key=lambda item: item["name"],
    )
    learning = normalized.get("learning")
    if learning is not None:
        learning["evidence"] = sorted(
            learning.get("evidence", []),
            key=lambda item: item["name"],
        )
        for item in learning["evidence"]:
            item["correlateBy"] = sorted(item.get("correlateBy", []))
    return normalized


def contract_digest(contract: dict) -> str:
    canonical = canonical_json_bytes(normalize_contract(contract))
    return f"sha256:{hashlib.sha256(canonical).hexdigest()}"


def validate_issue_paths(fixture: dict, rep: Report, context: str) -> None:
    request_body = fixture.get("request", {}).get("body")
    response_body = fixture.get("expected", {}).get("body")
    if not isinstance(request_body, dict) or not isinstance(response_body, dict):
        return
    for issue in response_body.get("issues", []):
        pointer = issue.get("path")
        if not isinstance(pointer, str):
            continue
        try:
            resolve_pointer(request_body, pointer)
        except Exception:  # noqa: BLE001
            rep.fail(
                f"{context}: issue '{issue.get('code')}' path does not "
                f"resolve in the request: {pointer}"
            )
        else:
            rep.check(True, "")


def validate_fixture(
    path: Path,
    registry: Registry,
    operations: list[dict],
    rep: Report,
) -> dict | None:
    context = f"fixture:{path.relative_to(CONTRACTS)}"
    try:
        fixture = load_json(path)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{context}: does not parse: {exc}")
        return None

    missing = FIXTURE_REQUIRED_KEYS - set(fixture)
    rep.check(not missing, f"{context}: missing keys {sorted(missing)}")
    if missing:
        return fixture

    request = fixture["request"]
    expected = fixture["expected"]
    rep.check(bool(request.get("method")), f"{context}: request.method required")
    rep.check(bool(request.get("path")), f"{context}: request.path required")
    rep.check(
        isinstance(request.get("headers", {}), dict),
        f"{context}: request.headers must be an object",
    )
    if not fixture.get("sdkLocal"):
        rep.check(
            isinstance(expected.get("status"), int),
            f"{context}: expected.status required",
        )
        rep.check(
            isinstance(expected.get("headers", {}), dict),
            f"{context}: expected.headers must be an object",
        )
        if not fixture.get("schemaNegative"):
            correlation_id = expected.get("headers", {}).get(
                "X-Flaggo-Correlation-Id"
            )
            rep.check(
                bool(correlation_id),
                f"{context}: every HTTP response requires "
                "X-Flaggo-Correlation-Id",
            )
            supplied = request.get("headers", {}).get("X-Flaggo-Correlation-Id")
            if supplied is not None:
                rep.check(
                    correlation_id == supplied,
                    f"{context}: response must echo the supplied "
                    "X-Flaggo-Correlation-Id",
                )

    if not fixture.get("sdkLocal") and not fixture.get("schemaNegative"):
        operation = match_operation(
            request.get("method"),
            request.get("path"),
            operations,
        )
        rep.check(
            operation is not None,
            f"{context}: request does not match an OpenAPI operation",
        )
        if operation is not None:
            status = str(expected.get("status"))
            rep.check(
                status in operation["responses"]
                or "default" in operation["responses"],
                f"{context}: HTTP {status} is absent from responses for "
                f"{operation['method']} {operation['route']}",
            )

    rep.check(
        fixture.get("protocol", "json") == "json",
        f"{context}: unsupported fixture protocol",
    )

    request_schema = fixture.get("requestSchema")
    if request_schema and "body" in request:
        validate_body(
            request["body"],
            request_schema,
            registry,
            rep,
            f"{context} request",
        )

    response_schema = fixture.get("responseSchema")
    if response_schema and "body" in expected:
        validate_body(
            expected["body"],
            response_schema,
            registry,
            rep,
            f"{context} expected",
        )

    if fixture.get("schemaNegative"):
        samples = fixture.get("rejectedSamples", [])
        rep.check(
            bool(samples),
            f"{context}: schemaNegative fixture needs rejectedSamples",
        )
        for index, sample in enumerate(samples):
            schema_ref = sample.get("schemaRef")
            rep.check(
                bool(schema_ref),
                f"{context}: rejectedSamples[{index}] needs schemaRef",
            )
            if schema_ref:
                expect_rejected(
                    sample.get("body"),
                    schema_ref,
                    registry,
                    rep,
                    f"{context} rejectedSamples[{index}]",
                )

        for index, mutation in enumerate(
            fixture.get("rejectedFixtureMutations", [])
        ):
            source = load_json(FIXTURES_DIR / mutation["fixture"])
            body = deepcopy(
                resolve_pointer(
                    source,
                    mutation.get("sourcePointer", "/expected/body"),
                )
            )
            replace_pointer(body, mutation["path"], mutation.get("value"))
            expect_rejected(
                body,
                mutation["schemaRef"],
                registry,
                rep,
                f"{context} rejectedFixtureMutations[{index}]",
            )

        for index, mutation in enumerate(
            fixture.get("acceptedFixtureMutations", [])
        ):
            source = load_json(FIXTURES_DIR / mutation["fixture"])
            body = deepcopy(
                resolve_pointer(
                    source,
                    mutation.get("sourcePointer", "/expected/body"),
                )
            )
            replace_pointer(body, mutation["path"], mutation.get("value"))
            validate_body(
                body,
                mutation["schemaRef"],
                registry,
                rep,
                f"{context} acceptedFixtureMutations[{index}]",
            )

        for index, raw_json in enumerate(
            fixture.get("rejectedJsonDocuments", [])
        ):
            try:
                strict_json_loads(raw_json)
            except ValueError:
                rep.check(True, "")
            else:
                rep.fail(
                    f"{context}: rejectedJsonDocuments[{index}] parsed as "
                    "strict JSON"
                )

    validate_issue_paths(fixture, rep, context)
    return fixture


def iter_contract_versions(node):
    if isinstance(node, dict):
        if {
            "name",
            "contractDigest",
            "activeExecutableDigest",
            "contract",
        }.issubset(node):
            yield node
        for value in node.values():
            yield from iter_contract_versions(value)
    elif isinstance(node, list):
        for value in node:
            yield from iter_contract_versions(value)


def validate_v3_identities(fixtures: list[dict], rep: Report) -> None:
    for fixture in fixtures:
        context = f"fixture:{fixture.get('name')}"
        request = fixture.get("request", {})
        expected = fixture.get("expected", {})
        request_body = request.get("body")
        response_body = expected.get("body")

        if (
            isinstance(request_body, dict)
            and {"name", "expression_syntax", "result"}.issubset(request_body)
            and isinstance(response_body, dict)
            and response_body.get("status") == "valid"
        ):
            rep.check(
                response_body.get("contractDigest")
                == contract_digest(request_body),
                f"{context}: validation digest does not identify the "
                "canonical DecisionContract",
            )

        for version in iter_contract_versions(response_body):
            contract = version["contract"]
            rep.check(
                version["name"] == contract.get("name"),
                f"{context}: version name differs from contract name",
            )
            rep.check(
                version["contractDigest"] == contract_digest(contract),
                f"{context}: version digest does not identify its "
                "canonical DecisionContract",
            )
            rep.check(
                bool(
                    DIGEST_PATTERN.fullmatch(
                        version["activeExecutableDigest"]
                    )
                ),
                f"{context}: active executable digest is not canonical",
            )

        runtime_match = RUNTIME_PATH_PATTERN.fullmatch(
            request.get("path", "")
        )
        if (
            runtime_match
            and expected.get("status") == 200
            and isinstance(response_body, dict)
        ):
            rep.check(
                response_body.get("contractDigest")
                == runtime_match.group("digest"),
                f"{context}: runtime response digest differs from the "
                "exact-version route",
            )

        exact_match = EXACT_VERSION_PATH_PATTERN.fullmatch(
            request.get("path", "")
        )
        if (
            exact_match
            and expected.get("status") == 200
            and isinstance(response_body, dict)
        ):
            rep.check(
                response_body.get("contractDigest")
                == exact_match.group("digest"),
                f"{context}: exact-version response digest differs from "
                "the route",
            )

        location = expected.get("headers", {}).get("Location")
        if (
            request.get("method") == "PUT"
            and expected.get("status") == 201
            and isinstance(response_body, dict)
        ):
            expected_location = (
                f"/v3/decision-contracts/{response_body.get('name')}"
                f"/versions/{response_body.get('contractDigest')}"
            )
            rep.check(
                location == expected_location,
                f"{context}: Location does not identify the created version",
            )


def main() -> int:
    rep = Report()

    document_paths = (
        list(SCHEMAS_DIR.glob("*.json"))
        + list(OPENAPI_DIR.glob("*.yaml"))
        + list(FIXTURES_DIR.rglob("*.json"))
        + [MANIFEST, CANONICALIZATION_VECTORS, STRICT_JSON_VECTORS]
    )
    for path in document_paths:
        try:
            load_json(path)
            rep.check(True, "")
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"parse error: {path}: {exc}")

    registry = build_registry(rep)
    validate_canonicalization_vectors(rep)
    validate_strict_json_vectors(rep)

    for name in OPENAPI_FILES:
        validate_openapi(OPENAPI_DIR / name, rep)
    operations = load_openapi_operations()

    try:
        manifest = load_json(MANIFEST)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"manifest does not parse: {exc}")
        print_report(rep)
        return 1

    cases = manifest.get("cases", [])
    rep.check(bool(cases), "manifest has no cases")
    rep.check(
        manifest.get("totalCases") == len(cases),
        f"manifest totalCases is {manifest.get('totalCases')}, "
        f"expected {len(cases)}",
    )

    indexed_paths: set[Path] = set()
    manifest_by_path: dict[Path, dict] = {}
    names: set[str] = set()
    covered: set[int] = set()
    for case in cases:
        name = case.get("name")
        rep.check(bool(name), "manifest case missing name")
        if name:
            rep.check(
                name not in names,
                f"duplicate case name in manifest: {name}",
            )
            names.add(name)
        relative = case.get("file")
        rep.check(bool(relative), f"manifest case '{name}' missing file")
        if relative:
            path = (CONTRACTS / relative).resolve()
            rep.check(
                path.exists(),
                f"manifest case '{name}' file not found: {relative}",
            )
            rep.check(
                path not in indexed_paths,
                f"fixture indexed more than once: {relative}",
            )
            indexed_paths.add(path)
            manifest_by_path[path] = case
        covered.update(int(scenario) for scenario in case.get("scenarios", []))

    on_disk = {path.resolve() for path in FIXTURES_DIR.rglob("*.json")}
    for path in sorted(on_disk - indexed_paths):
        rep.fail(
            "fixture on disk not indexed in manifest: "
            f"{path.relative_to(CONTRACTS)}"
        )
    for path in sorted(indexed_paths - on_disk):
        rep.fail(
            "manifest indexes a missing fixture: "
            f"{path.relative_to(CONTRACTS)}"
        )

    required = set(manifest.get("coveredScenarios", []))
    rep.check(
        manifest.get("requiredScenarioCount") == len(required),
        "manifest requiredScenarioCount does not match coveredScenarios",
    )
    rep.check(
        not (required - covered),
        f"required scenarios not covered by fixtures: "
        f"{sorted(required - covered)}",
    )

    loaded_fixtures: list[dict] = []
    for path in sorted(on_disk):
        fixture = validate_fixture(path, registry, operations, rep)
        if fixture is None:
            continue
        loaded_fixtures.append(fixture)
        declared = set(
            fixture.get("scenarios", [fixture.get("scenario")])
        )
        indexed = set(manifest_by_path.get(path, {}).get("scenarios", []))
        rep.check(
            declared == indexed,
            f"fixture:{path.relative_to(CONTRACTS)}: declared scenarios "
            f"{sorted(declared)} do not match manifest {sorted(indexed)}",
        )

    for operation in (
        operation for operation in operations if operation["scopes"]
    ):
        covered_by_scope_fixture = any(
            fixture.get("expected", {}).get("status") == 403
            and fixture.get("expected", {}).get("body", {}).get("type")
            == "https://flaggo.dev/problems/insufficient-scope"
            and match_operation(
                fixture.get("request", {}).get("method"),
                fixture.get("request", {}).get("path"),
                operations,
            )
            is operation
            for fixture in loaded_fixtures
        )
        rep.check(
            covered_by_scope_fixture,
            f"secured operation {operation['method']} {operation['route']} "
            "lacks a 403 insufficient-scope fixture",
        )

    validate_v3_identities(loaded_fixtures, rep)

    print_report(
        rep,
        {
            "schemas": len(SCHEMA_FILES),
            "openapi_docs": len(OPENAPI_FILES),
            "fixtures": len(on_disk),
            "manifest_cases": len(cases),
            "scenarios_covered": len(covered & required),
        },
    )
    return 1 if rep.errors else 0


def print_report(rep: Report, extra: dict | None = None) -> None:
    print("=" * 68)
    print("Flaggo v3 contract validation")
    print("=" * 68)
    if extra:
        for key, value in extra.items():
            print(f"  {key:20s}: {value}")
    print(f"  checks_run          : {rep.checks}")
    if rep.errors:
        print(f"\nFAILED with {len(rep.errors)} error(s):")
        for error in rep.errors:
            print(f"  - {error}")
    else:
        print("\nAll checks passed.")


if __name__ == "__main__":
    sys.exit(main())
