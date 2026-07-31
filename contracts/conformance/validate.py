#!/usr/bin/env python3
"""Phase 1 executable contract validator.

Validates, with no network access:
  1. Every JSON/OpenAPI document under contracts/ parses.
  2. The four JSON Schemas are valid Draft 2020-12.
  3. Runtime and management OpenAPI 3.1 documents are structurally valid and
     every local $ref (internal and external-file) resolves on disk.
  4. The fixture manifest indexes every fixture file and every fixture file is
     indexed (no drift).
  5. All 43 required golden scenarios are covered by at least one fixture.
  6. Fixture request/response bodies validate against their referenced schemas;
     schema-negative fixtures are rejected as intended.

Exit code 0 on success, 1 on any failure.

Usage:
    python contracts/conformance/validate.py
"""
from __future__ import annotations

import json
import base64
import hashlib
import re
import sys
from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
from decimal import Decimal
from pathlib import Path
from threading import Lock

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
CANONICALIZATION_VECTORS = CONTRACTS / "conformance" / "canonicalization-vectors-v1.json"
SEMANTIC_DIGEST_VECTORS = CONTRACTS / "conformance" / "semantic-digest-vectors-v1.json"
STRICT_JSON_VECTORS = CONTRACTS / "conformance" / "strict-json-vectors-v1.json"

SCHEMA_ID_PREFIX = "https://flaggo.dev/contracts/schemas/"
REQUIRED_SCENARIO_COUNT = 43

SCHEMA_FILES = [
    "runtime-models-v1.schema.json",
    "management-models-v1.schema.json",
    "decision-definition-bundle-v1.schema.json",
    "problem-details-v1.schema.json",
]


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
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"schema does not parse: {path}: {exc}")
            continue
        try:
            Draft202012Validator.check_schema(schema)
            rep.check(True, "")
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"invalid Draft 2020-12 schema {name}: {exc}")
            continue
        uri = SCHEMA_ID_PREFIX + name
        resources.append((uri, Resource.from_contents(schema, default_specification=DRAFT202012)))
    return Registry().with_resources(resources)


def validator_for_ref(schema_ref: str, registry: Registry) -> Draft202012Validator:
    """Return a validator for a manifest-style ref: '<file>' or '<file>#/$defs/<Def>'."""
    if "#" in schema_ref:
        file_part, pointer = schema_ref.split("#", 1)
        target = SCHEMA_ID_PREFIX + file_part + "#" + pointer
    else:
        target = SCHEMA_ID_PREFIX + schema_ref
    return Draft202012Validator(
        {"$ref": target},
        registry=registry,
        format_checker=FormatChecker(),
    )


def validate_body(body, schema_ref: str, registry: Registry, rep: Report, ctx: str) -> None:
    try:
        validator = validator_for_ref(schema_ref, registry)
        errors = sorted(validator.iter_errors(body), key=lambda e: list(e.path))
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{ctx}: could not validate against {schema_ref}: {exc}")
        return
    if errors:
        first = errors[0]
        loc = "/".join(str(p) for p in first.path)
        rep.fail(f"{ctx}: body invalid against {schema_ref} at '/{loc}': {first.message}")
    else:
        rep.check(True, "")


def expect_rejected(body, schema_ref: str, registry: Registry, rep: Report, ctx: str) -> None:
    try:
        validator = validator_for_ref(schema_ref, registry)
        ok = validator.is_valid(body)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{ctx}: could not run negative validation against {schema_ref}: {exc}")
        return
    if ok:
        rep.fail(f"{ctx}: body was expected to be REJECTED by {schema_ref} but validated")
    else:
        rep.check(True, "")


# --------------------------------------------------------------------------- #
# OpenAPI structural validation and local ref resolution
# --------------------------------------------------------------------------- #

def resolve_pointer(doc, pointer: str):
    node = doc
    for raw in pointer.split("/"):
        if raw == "":
            continue
        token = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(node, list):
            node = node[int(token)]
        elif isinstance(node, dict):
            node = node[token]
        else:
            raise KeyError(pointer)
    return node


def replace_pointer(doc, pointer: str, value) -> None:
    tokens = [
        raw.replace("~1", "/").replace("~0", "~")
        for raw in pointer.split("/")
        if raw
    ]
    parent = doc
    for token in tokens[:-1]:
        parent = parent[int(token)] if isinstance(parent, list) else parent[token]
    final = tokens[-1]
    if isinstance(parent, list):
        parent[int(final)] = value
    else:
        parent[final] = value


def collect_refs(node, acc: list[str]) -> None:
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                acc.append(value)
            else:
                collect_refs(value, acc)
    elif isinstance(node, list):
        for item in node:
            collect_refs(item, acc)


def absolutize_external_refs(node, base_dir: Path) -> None:
    """Rewrite external refs in a validation copy for the OpenAPI validator."""
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


def check_ref_resolves(ref: str, doc, base_dir: Path, rep: Report, ctx: str) -> None:
    file_part, _, pointer = ref.partition("#")
    if file_part == "":
        target_doc = doc
        target_path = None
    else:
        target_path = (base_dir / file_part).resolve()
        if not target_path.exists():
            rep.fail(f"{ctx}: $ref target file not found: {ref}")
            return
        try:
            target_doc = load_json(target_path)
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"{ctx}: $ref target does not parse: {ref}: {exc}")
            return
    if pointer:
        try:
            resolve_pointer(target_doc, pointer)
        except Exception:  # noqa: BLE001
            rep.fail(f"{ctx}: $ref pointer does not resolve: {ref}")
            return
    rep.check(True, "")


def validate_openapi(path: Path, rep: Report) -> None:
    ctx = f"openapi:{path.name}"
    try:
        doc = load_json(path)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{ctx}: does not parse as JSON/YAML: {exc}")
        return
    version = str(doc.get("openapi", ""))
    rep.check(version.startswith("3.1"), f"{ctx}: openapi version must be 3.1.x, got '{version}'")
    try:
        validation_doc = deepcopy(doc)
        absolutize_external_refs(validation_doc, path.parent)
        validate_spec(validation_doc)
        rep.check(True, "")
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{ctx}: OpenAPI 3.1 semantic validation failed: {exc}")
    rep.check(bool(doc.get("info", {}).get("title")), f"{ctx}: info.title required")
    paths = doc.get("paths")
    rep.check(isinstance(paths, dict) and len(paths) > 0, f"{ctx}: at least one path required")
    seen_op_ids: set[str] = set()
    for route, item in (paths or {}).items():
        for method, op in item.items():
            if method not in {"get", "put", "post", "delete", "patch", "options", "head"}:
                continue
            op_id = op.get("operationId")
            rep.check(bool(op_id), f"{ctx}: {method.upper()} {route} missing operationId")
            if op_id:
                rep.check(op_id not in seen_op_ids, f"{ctx}: duplicate operationId '{op_id}'")
                seen_op_ids.add(op_id)
            rep.check(bool(op.get("responses")), f"{ctx}: {method.upper()} {route} missing responses")
    refs: list[str] = []
    collect_refs(doc, refs)
    for ref in sorted(set(refs)):
        check_ref_resolves(ref, doc, path.parent, rep, ctx)


def load_openapi_operations() -> list[dict]:
    operations = []
    for name in ["flaggo-runtime-v1.yaml", "flaggo-management-v1.yaml"]:
        doc = load_json(OPENAPI_DIR / name)
        for route, item in doc["paths"].items():
            pattern = "^" + re.sub(r"\{[^}]+\}", r"[^/]+", route) + "$"
            for method, operation in item.items():
                if method not in {"get", "put", "post", "delete", "patch", "options", "head"}:
                    continue
                scopes = {
                    scope
                    for requirement in operation.get("security", doc.get("security", []))
                    for scope in requirement.get("oauth2", [])
                }
                operations.append({
                    "method": method.upper(),
                    "route": route,
                    "pattern": re.compile(pattern),
                    "responses": set(operation["responses"]),
                    "scopes": scopes,
                })
    return operations


def match_operation(method: str, path: str, operations: list[dict]) -> dict | None:
    return next(
        (
            operation
            for operation in operations
            if operation["method"] == method and operation["pattern"].fullmatch(path)
        ),
        None,
    )


# --------------------------------------------------------------------------- #
# Fixtures + manifest
# --------------------------------------------------------------------------- #

FIXTURE_REQUIRED_KEYS = {"name", "scenario", "invariant", "request", "expected"}


def canonical_json_bytes(value) -> bytes:
    return rfc8785.dumps(value)


def _sorted_signal_refs(values: list[dict]) -> list[dict]:
    by_key: dict[str, dict] = {}
    for value in values:
        key = value["key"]
        if key in by_key:
            raise ValueError(f"duplicate signal reference: {key}")
        by_key[key] = {"key": key}
    return [by_key[key] for key in sorted(by_key)]


def _collect_signal_keys(definition: dict) -> set[str]:
    keys: set[str] = set()
    signal_roles = definition.get("signals", {})
    for role in ("evidence", "guardrails"):
        role_keys = [ref["key"] for ref in signal_roles.get(role, [])]
        if len(role_keys) != len(set(role_keys)):
            raise ValueError(f"duplicate {role} signal reference")
        keys.update(role_keys)
    input_keys = [ref["key"] for ref in definition.get("inference", {}).get("inputs", [])]
    if len(input_keys) != len(set(input_keys)):
        raise ValueError("duplicate inference signal reference")
    keys.update(input_keys)

    def visit(node) -> None:
        if isinstance(node, dict):
            signal = node.get("signal")
            if isinstance(signal, dict) and isinstance(signal.get("key"), str):
                keys.add(signal["key"])
            for value in node.values():
                visit(value)
        elif isinstance(node, list):
            for value in node:
                visit(value)

    visit(definition.get("intent"))
    return keys


def normalize_definition(definition: dict, *, semantic_identity: bool = True) -> dict:
    normalized = deepcopy(definition)
    for key in ("contractDigest", "revision", "schemaDigest"):
        normalized.pop(key, None)
    if semantic_identity:
        normalized.pop("definitionId", None)
        normalized.pop("owner", None)

    signal_keys = _collect_signal_keys(normalized)
    signal_roles = normalized.get("signals")
    if signal_roles is not None or signal_keys:
        signal_roles = signal_roles or {}
        for role in ("evidence", "guardrails"):
            if role in signal_roles:
                signal_roles[role] = _sorted_signal_refs(signal_roles[role])
        if signal_keys:
            signal_roles["allowed"] = [{"key": key} for key in sorted(signal_keys)]
        else:
            signal_roles.pop("allowed", None)
        if signal_roles:
            normalized["signals"] = signal_roles
        else:
            normalized.pop("signals", None)

    inference = normalized.get("inference")
    if inference and "inputs" in inference:
        inference["inputs"] = _sorted_signal_refs(inference["inputs"])

    policy = normalized.get("policy")
    if policy and policy.get("kind") == "inline":
        if policy.get("clientFallback", {}).get("requiredEvidenceUnavailable") == "forbid":
            policy.pop("clientFallback", None)
        constraints = policy.get("constraints", [])
        kinds = [constraint["kind"] for constraint in constraints]
        if len(kinds) != len(set(kinds)):
            raise ValueError("duplicate inline policy constraint kind")
        policy["constraints"] = sorted(
            constraints,
            key=lambda constraint: (constraint["kind"], canonical_json_bytes(constraint)),
        )
    return normalized


def contract_digest(definition: dict) -> str:
    normalized = normalize_definition(definition, semantic_identity=True)
    return f"sha256:{hashlib.sha256(canonical_json_bytes(normalized)).hexdigest()}"


def normalize_bundle(bundle: dict) -> dict:
    normalized = deepcopy(bundle)
    declarations: dict[str, dict] = {}
    for declaration in normalized.get("signals", []):
        declaration = deepcopy(declaration)
        declaration.pop("schemaDigest", None)
        key = declaration["key"]
        if key in declarations:
            raise ValueError(f"duplicate signal declaration: {key}")
        declarations[key] = declaration
    if "signals" in normalized:
        normalized["signals"] = [declarations[key] for key in sorted(declarations)]

    definitions: dict[str, dict] = {}
    definition_digests: dict[str, str] = {}
    for definition in normalized["definitions"]:
        key = definition["key"]
        candidate = normalize_definition(definition, semantic_identity=False)
        digest = contract_digest(definition)
        if key in definitions:
            if definition_digests[key] != digest:
                raise ValueError(f"conflicting duplicate decision key: {key}")
            if canonical_json_bytes(definitions[key]) != canonical_json_bytes(candidate):
                raise ValueError(f"metadata-conflicting duplicate decision key: {key}")
            continue
        definitions[key] = candidate
        definition_digests[key] = digest
    normalized["definitions"] = [definitions[key] for key in sorted(definitions)]
    return normalized


def bundle_digest(bundle: dict) -> str:
    normalized = normalize_bundle(bundle)
    return f"sha256:{hashlib.sha256(canonical_json_bytes(normalized)).hexdigest()}"


def definition_value_contract_errors(definition: dict) -> set[str]:
    action = definition["actionSpace"]
    fallback = definition["fallback"]["value"]
    default = action["default"]
    value_type = definition["valueType"]
    errors: set[str] = set()
    if value_type == "number":
        minimum, maximum = action["min"], action["max"]
        step = action.get("step")
        if minimum > maximum:
            errors.add("invalid-bounds")
        if not minimum <= default <= maximum:
            errors.add("invalid-default")
        if not minimum <= fallback <= maximum:
            errors.add("invalid-fallback")
        if step is not None:
            if step <= 0:
                errors.add("invalid-step")
            elif minimum <= maximum:
                for label, value in (("invalid-default-step", default), ("invalid-fallback-step", fallback)):
                    offset = (Decimal(str(value)) - Decimal(str(minimum))) / Decimal(str(step))
                    if offset != offset.to_integral_value():
                        errors.add(label)
    elif value_type == "string":
        allowed = action.get("allowedValues")
        if allowed is not None:
            if default not in allowed:
                errors.add("invalid-default")
            if fallback not in allowed:
                errors.add("invalid-fallback")
    return errors


def validate_canonicalization_vectors(rep: Report) -> None:
    document = load_json(CANONICALIZATION_VECTORS)
    vectors = document.get("vectors", [])
    rep.check(bool(vectors), "canonicalization vector set is empty")
    for vector in vectors:
        try:
            actual = canonical_json_bytes(vector["input"]).decode("utf-8")
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"canonicalization vector '{vector.get('name')}' failed: {exc}")
            continue
        rep.check(
            actual == vector["expected"],
            f"canonicalization vector '{vector.get('name')}' mismatch: {actual!r}",
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


def validate_semantic_digest_vectors(rep: Report) -> None:
    document = load_json(SEMANTIC_DIGEST_VECTORS)
    definition_cases = document.get("definitionCases", [])
    inequivalent_definition_cases = document.get("inequivalentDefinitionCases", [])
    bundle_cases = document.get("bundleCases", [])
    value_contract_cases = document.get("valueContractCases", [])
    normalization_error_cases = document.get("normalizationErrorCases", [])
    rep.check(bool(definition_cases), "semantic definition digest vector set is empty")
    rep.check(
        bool(inequivalent_definition_cases),
        "inequivalent semantic definition digest vector set is empty",
    )
    rep.check(bool(bundle_cases), "semantic bundle digest vector set is empty")
    rep.check(bool(value_contract_cases), "value contract vector set is empty")
    rep.check(bool(normalization_error_cases), "normalization error vector set is empty")

    for case in definition_cases:
        digests = [contract_digest(variant["definition"]) for variant in case["variants"]]
        rep.check(
            len(set(digests)) == 1,
            f"semantic definition vector '{case['name']}' variants diverged: {digests}",
        )
        rep.check(
            digests[0] == case["expectedContractDigest"],
            f"semantic definition vector '{case['name']}' digest mismatch: {digests[0]}",
        )

    for case in inequivalent_definition_cases:
        left_digest = contract_digest(case["left"])
        right_digest = contract_digest(case["right"])
        rep.check(
            left_digest == case["expectedLeftDigest"],
            f"semantic definition vector '{case['name']}' left digest mismatch: {left_digest}",
        )
        rep.check(
            right_digest == case["expectedRightDigest"],
            f"semantic definition vector '{case['name']}' right digest mismatch: {right_digest}",
        )
        rep.check(
            left_digest != right_digest,
            f"semantic definition vector '{case['name']}' collapsed distinct definitions",
        )

    for case in bundle_cases:
        digests = [bundle_digest(variant["bundle"]) for variant in case["variants"]]
        rep.check(
            len(set(digests)) == 1,
            f"semantic bundle vector '{case['name']}' variants diverged: {digests}",
        )
        rep.check(
            digests[0] == case["expectedBundleDigest"],
            f"semantic bundle vector '{case['name']}' digest mismatch: {digests[0]}",
        )
        definitions = case["variants"][0]["bundle"]["definitions"]
        actual_contracts = {definition["key"]: contract_digest(definition) for definition in definitions}
        rep.check(
            actual_contracts == case["expectedContractDigests"],
            f"semantic bundle vector '{case['name']}' per-definition digests mismatch",
        )

    for case in value_contract_cases:
        actual = definition_value_contract_errors(case["definition"])
        expected = set(case["expectedErrors"])
        rep.check(
            actual == expected,
            f"value contract vector '{case['name']}' errors mismatch: {sorted(actual)}",
        )

    for case in normalization_error_cases:
        try:
            normalize_bundle(case["bundle"])
        except ValueError as exc:
            rep.check(
                case["expectedError"] in str(exc),
                f"normalization error vector '{case['name']}' returned unexpected error: {exc}",
            )
        else:
            rep.fail(f"normalization error vector '{case['name']}' was accepted")


class StartupRegistrationError(RuntimeError):
    def __init__(self, code: str) -> None:
        super().__init__(f"startup registration failed: {code}")
        self.code = code


class StartupRegistrationHarness:
    """Minimal executable model for startup apply and binding initialization."""

    def __init__(self) -> None:
        self._lock = Lock()
        self._results: dict[str, tuple[bytes, dict]] = {}
        self.binding: dict | None = None
        self.startup_error: str | None = None
        self.write_count = 0

    def apply(self, fixture: dict) -> dict:
        request = fixture["request"]
        key = request["headers"]["Idempotency-Key"]
        fingerprint = canonical_json_bytes(request["body"])
        exchange = {
            "status": fixture["expected"]["status"],
            "body": fixture["expected"]["body"],
        }
        with self._lock:
            retained = self._results.get(key)
            if retained is not None:
                retained_fingerprint, retained_exchange = retained
                if retained_fingerprint != fingerprint:
                    raise RuntimeError("idempotency key reused with a different canonical body")
                return deepcopy(retained_exchange)
            self._results[key] = (fingerprint, deepcopy(exchange))
            self.write_count += 1
            if exchange["status"] == 200:
                self.binding = deepcopy(exchange["body"]["acceptedDefinitions"])
                self.startup_error = None
            else:
                self.binding = None
                self.startup_error = exchange["body"].get("code", exchange["body"].get("status"))
            return deepcopy(exchange)

    def initialize_sdk(self) -> dict:
        if self.binding is None:
            raise StartupRegistrationError(self.startup_error or "missing-runtime-binding")
        return deepcopy(self.binding)


def execute_stateful_scenarios(fixtures: list[dict], rep: Report) -> int:
    executed = 0
    for fixture in fixtures:
        scenario = fixture.get("statefulScenario")
        if not scenario:
            continue
        kind = scenario.get("kind")
        harness = StartupRegistrationHarness()
        if kind == "concurrent-startup-apply":
            copies = scenario.get("requestCopies")
            rep.check(
                isinstance(copies, int) and copies >= 2,
                "concurrent-startup-apply requires at least two request copies",
            )
            if not isinstance(copies, int) or copies < 2:
                continue
            with ThreadPoolExecutor(max_workers=copies) as executor:
                results = list(executor.map(lambda _: harness.apply(fixture), range(copies)))
            first = canonical_json_bytes(results[0])
            rep.check(
                all(canonical_json_bytes(result) == first for result in results[1:]),
                "scenario 19 concurrent apply calls returned different receipts",
            )
            rep.check(harness.write_count == 1, "scenario 19 performed more than one registry write")
            rep.check(
                harness.initialize_sdk() == results[0]["body"]["acceptedDefinitions"],
                "scenario 19 SDK binding differs from the converged receipt",
            )
            reordered = deepcopy(fixture)
            reordered["request"]["body"]["signals"].reverse()
            try:
                harness.apply(reordered)
            except RuntimeError as exc:
                rep.check(
                    "different canonical body" in str(exc),
                    "scenario 19 returned the wrong idempotency conflict",
                )
            else:
                rep.fail("scenario 19 replayed a semantically equivalent but byte-distinct request")
        elif kind == "approval-blocked-startup":
            result = harness.apply(fixture)
            rep.check(
                result["status"] == 202 and result["body"].get("status") == "requires-approval",
                "scenario 20 did not execute a requires-approval apply result",
            )
            try:
                harness.initialize_sdk()
            except StartupRegistrationError as exc:
                rep.check(harness.binding is None, "scenario 20 retained a runtime binding")
                rep.check(exc.code == "requires-approval", "scenario 20 raised the wrong approval startup error")
            else:
                rep.fail("scenario 20 initialized the SDK without an accepted runtime binding")
        elif kind == "invalid-registration-blocked-startup":
            result = harness.apply(fixture)
            rep.check(
                result["status"] == 422 and result["body"].get("code") == "invalid-bundle",
                "scenario 20 did not execute an invalid-bundle apply result",
            )
            try:
                harness.initialize_sdk()
            except StartupRegistrationError as exc:
                rep.check(harness.binding is None, "scenario 20 retained a binding after invalid registration")
                rep.check(exc.code == "invalid-bundle", "scenario 20 raised the wrong invalid startup error")
            else:
                rep.fail("scenario 20 initialized the SDK after invalid registration")
        else:
            rep.fail(f"unknown stateful scenario kind: {kind}")
            continue
        executed += 1
    return executed


def validate_issue_correlations(fx: dict, rep: Report, ctx: str) -> None:
    request_body = fx.get("request", {}).get("body")
    response_body = fx.get("expected", {}).get("body")
    if not isinstance(request_body, dict) or not isinstance(response_body, dict):
        return
    issues = response_body.get("issues", [])
    if not issues:
        return

    declared_signals = {
        signal.get("key")
        for signal in request_body.get("signals", [])
        if isinstance(signal, dict)
    }
    for issue in issues:
        code = issue.get("code")
        pointer = issue.get("path")
        try:
            offending = resolve_pointer(request_body, pointer)
        except Exception:  # noqa: BLE001
            rep.fail(f"{ctx}: issue '{code}' path does not resolve in request: {pointer}")
            continue

        if code == "unknown-signal":
            signal_key = issue.get("signalKey")
            rep.check(
                isinstance(offending, dict)
                and offending.get("key") == signal_key
                and signal_key not in declared_signals,
                f"{ctx}: unknown-signal does not identify an undeclared request reference",
            )
        elif code == "invalid-signal-schema":
            value_range = offending.get("range") if isinstance(offending, dict) else None
            rep.check(
                isinstance(value_range, list)
                and len(value_range) == 2
                and value_range[0] > value_range[1],
                f"{ctx}: invalid-signal-schema path does not identify a descending range",
            )
        elif code == "invalid-definition":
            rep.check(
                isinstance(offending, dict) and bool(definition_value_contract_errors(offending)),
                f"{ctx}: invalid-definition path does not identify invalid action-space bounds",
            )
        elif code == "invalid-objective":
            signal_key = (
                offending.get("signal", {}).get("key")
                if isinstance(offending, dict)
                else None
            )
            rep.check(
                signal_key not in declared_signals,
                f"{ctx}: invalid-objective path does not identify an undeclared objective signal",
            )
        elif code == "invalid-policy":
            constraints = offending.get("constraints", []) if isinstance(offending, dict) else []
            rep.check(
                any(
                    constraint.get("kind") == "number-bounds"
                    and constraint.get("min") > constraint.get("max")
                    for constraint in constraints
                ),
                f"{ctx}: invalid-policy path does not identify contradictory number bounds",
            )
        elif code == "invalid-strategy":
            live_inputs = offending.get("liveInputs", []) if isinstance(offending, dict) else []
            rep.check(
                "missingInput" in live_inputs,
                f"{ctx}: invalid-strategy path does not identify an unavailable live input",
            )


def validate_snapshot_digest(fx: dict, rep: Report, ctx: str) -> None:
    if fx.get("name") != "approval-snapshot-digest-headers":
        return
    body = fx["expected"]["body"]
    headers = fx["expected"]["headers"]
    normalized_body = normalize_bundle(body)
    rep.check(body == normalized_body, f"{ctx}: snapshot body is not normalized canonical bundle content")
    digest = hashlib.sha256(canonical_json_bytes(normalized_body)).digest()
    project_digest = f"sha256:{digest.hex()}"
    etag = f'"{project_digest}"'
    content_digest = f"sha-256=:{base64.b64encode(digest).decode('ascii')}:"
    declared = fx.get("digest", {})
    rep.check(fx.get("canonicalResponse") is True, f"{ctx}: snapshot must request canonical byte replay")
    rep.check(declared.get("bundleDigest") == project_digest, f"{ctx}: bundleDigest does not hash canonical body")
    rep.check(declared.get("etag") == etag, f"{ctx}: digest.etag does not hash canonical body")
    rep.check(declared.get("contentDigest") == content_digest, f"{ctx}: digest.contentDigest does not hash canonical body")
    rep.check(headers.get("ETag") == etag, f"{ctx}: ETag does not hash canonical body")
    rep.check(headers.get("Content-Digest") == content_digest, f"{ctx}: Content-Digest does not hash canonical body")


def validate_bundle_digest(fx: dict, rep: Report, ctx: str) -> None:
    request_body = fx.get("request", {}).get("body")
    if (
        not isinstance(request_body, dict)
        or request_body.get("format") != "flaggo.decision-definition-bundle/v1"
    ):
        return
    project_digest = bundle_digest(request_body)
    response_digest = fx.get("expected", {}).get("body", {}).get("bundleDigest")
    if response_digest is not None:
        rep.check(
            response_digest == project_digest,
            f"{ctx}: response bundleDigest does not hash the canonical request body",
        )
    declared_digest = fx.get("digest", {}).get("bundleDigest")
    if declared_digest is not None:
        rep.check(
            declared_digest == project_digest,
            f"{ctx}: fixture bundleDigest does not hash the canonical request body",
        )
    response_body = fx.get("expected", {}).get("body", {})
    definitions = {definition["key"]: definition for definition in request_body["definitions"]}
    successful_status = response_body.get("status") in {"approved", "requires-approval", "valid"}
    if successful_status:
        for key, definition in definitions.items():
            errors = definition_value_contract_errors(definition)
            rep.check(
                not errors,
                f"{ctx}: successful bundle contains invalid value contract for '{key}': {sorted(errors)}",
            )
    for field in ("validatedDefinitions", "acceptedDefinitions"):
        declared_definitions = response_body.get(field, {})
        for key, identity in declared_definitions.items():
            if key not in definitions:
                rep.fail(f"{ctx}: {field} contains unknown decision key '{key}'")
                continue
            expected_digest = contract_digest(definitions[key])
            rep.check(
                identity.get("contractDigest") == expected_digest,
                f"{ctx}: {field}.{key}.contractDigest does not hash normalized definition semantics",
            )


def validate_fixture(
    fx_path: Path,
    registry: Registry,
    operations: list[dict],
    rep: Report,
) -> dict | None:
    ctx = f"fixture:{fx_path.relative_to(CONTRACTS)}"
    try:
        fx = load_json(fx_path)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"{ctx}: does not parse: {exc}")
        return None
    missing = FIXTURE_REQUIRED_KEYS - set(fx)
    rep.check(not missing, f"{ctx}: missing keys {sorted(missing)}")
    if missing:
        return fx

    req = fx["request"]
    rep.check(bool(req.get("method")), f"{ctx}: request.method required")
    rep.check(bool(req.get("path")), f"{ctx}: request.path required")
    rep.check(isinstance(req.get("headers", {}), dict), f"{ctx}: request.headers must be an object")
    exp = fx["expected"]
    if not fx.get("sdkLocal"):
        rep.check(isinstance(exp.get("status"), int), f"{ctx}: expected.status required")
        rep.check(isinstance(exp.get("headers", {}), dict), f"{ctx}: expected.headers must be an object")
    if not fx.get("sdkLocal") and not fx.get("schemaNegative"):
        operation = match_operation(req.get("method"), req.get("path"), operations)
        rep.check(operation is not None, f"{ctx}: request does not match an OpenAPI operation")
        if operation is not None:
            status = str(exp.get("status"))
            rep.check(
                status in operation["responses"] or "default" in operation["responses"],
                f"{ctx}: HTTP {status} is absent from OpenAPI responses for {operation['method']} {operation['route']}",
            )

    # Positive request body validation
    req_schema = fx.get("requestSchema")
    if req_schema and "body" in req:
        validate_body(req["body"], req_schema, registry, rep, f"{ctx} request")

    # Positive response body validation
    resp_schema = fx.get("responseSchema")
    if resp_schema and isinstance(exp, dict) and "body" in exp:
        validate_body(exp["body"], resp_schema, registry, rep, f"{ctx} expected")

    # Optional documented SDK client-fallback outcome
    if "sdkOutcome" in fx:
        oref = fx.get("sdkOutcomeSchema")
        rep.check(bool(oref), f"{ctx}: sdkOutcome requires sdkOutcomeSchema")
        if oref:
            validate_body(fx["sdkOutcome"], oref, registry, rep, f"{ctx} sdkOutcome")

    # Schema-negative fixtures: every sample must be rejected
    if fx.get("schemaNegative"):
        samples = fx.get("rejectedSamples", [])
        rep.check(len(samples) > 0, f"{ctx}: schemaNegative fixture needs rejectedSamples")
        for i, sample in enumerate(samples):
            sref = sample.get("schemaRef")
            rep.check(bool(sref), f"{ctx}: rejectedSamples[{i}] needs schemaRef")
            if sref:
                expect_rejected(sample.get("body"), sref, registry, rep, f"{ctx} rejectedSamples[{i}]")
        for i, mutation in enumerate(fx.get("rejectedFixtureMutations", [])):
            source_path = CONTRACTS / "fixtures" / mutation["fixture"]
            source = load_json(source_path)
            body = deepcopy(resolve_pointer(source, mutation.get("sourcePointer", "/expected/body")))
            replace_pointer(body, mutation["path"], mutation.get("value"))
            expect_rejected(
                body,
                mutation["schemaRef"],
                registry,
                rep,
                f"{ctx} rejectedFixtureMutations[{i}]",
            )
        for i, mutation in enumerate(fx.get("acceptedFixtureMutations", [])):
            source_path = CONTRACTS / "fixtures" / mutation["fixture"]
            source = load_json(source_path)
            body = deepcopy(resolve_pointer(source, mutation.get("sourcePointer", "/expected/body")))
            replace_pointer(body, mutation["path"], mutation.get("value"))
            validate_body(
                body,
                mutation["schemaRef"],
                registry,
                rep,
                f"{ctx} acceptedFixtureMutations[{i}]",
            )
        for i, raw_json in enumerate(fx.get("rejectedJsonDocuments", [])):
            try:
                json.loads(
                    raw_json,
                    parse_constant=lambda value: (_ for _ in ()).throw(
                        ValueError(f"non-finite JSON number '{value}' is forbidden")
                    ),
                )
            except ValueError:
                rep.check(True, "")
            else:
                rep.fail(f"{ctx}: rejectedJsonDocuments[{i}] parsed as strict JSON")

    validate_issue_correlations(fx, rep, ctx)
    validate_bundle_digest(fx, rep, ctx)
    validate_snapshot_digest(fx, rep, ctx)
    return fx


def collect_identity_tuples(node, identities: list[tuple[str, str, str]]) -> None:
    if isinstance(node, dict):
        if all(isinstance(node.get(key), str) for key in ("definitionId", "revision", "contractDigest")):
            identities.append((node["definitionId"], node["revision"], node["contractDigest"]))
        for value in node.values():
            collect_identity_tuples(value, identities)
    elif isinstance(node, list):
        for value in node:
            collect_identity_tuples(value, identities)


def validate_identity_consistency(fixtures: list[dict], rep: Report) -> None:
    accepted_by_revision: dict[tuple[str, str], set[str]] = {}
    for fixture in fixtures:
        expected = fixture.get("expected", {})
        response_body = expected.get("body", {})
        if expected.get("status") != 200 or response_body.get("status") == "invalid":
            continue
        identities: list[tuple[str, str, str]] = []
        collect_identity_tuples(fixture.get("request", {}).get("body"), identities)
        collect_identity_tuples(response_body, identities)
        for definition_id, revision, digest in identities:
            accepted_by_revision.setdefault((definition_id, revision), set()).add(digest)

        declared_digest = fixture.get("digest", {}).get("contractDigest")
        identity_digests = {digest for _, _, digest in identities}
        if declared_digest is not None:
            rep.check(
                len(identity_digests) == 1 and declared_digest in identity_digests,
                f"fixture:{fixture.get('name')}: digest.contractDigest does not match its successful identity",
            )

    for (definition_id, revision), digests in accepted_by_revision.items():
        rep.check(
            len(digests) == 1,
            f"successful fixtures assign multiple contract digests to {definition_id} + {revision}: {sorted(digests)}",
        )

    accepted = {
        identity: next(iter(digests))
        for identity, digests in accepted_by_revision.items()
        if len(digests) == 1
    }
    for fixture in fixtures:
        if fixture.get("schemaNegative") or fixture.get("intentionalIdentityConflict"):
            continue
        identities: list[tuple[str, str, str]] = []
        collect_identity_tuples(fixture.get("request", {}).get("body"), identities)
        collect_identity_tuples(fixture.get("expected", {}).get("body"), identities)
        collect_identity_tuples(fixture.get("sdkOutcome"), identities)
        for definition_id, revision, digest in identities:
            expected_digest = accepted.get((definition_id, revision))
            if expected_digest is not None:
                rep.check(
                    digest == expected_digest,
                    f"fixture:{fixture.get('name')}: stale contract digest for {definition_id} + {revision}",
                )


def main() -> int:
    rep = Report()

    # 1. every JSON/openapi document parses (schemas, openapi, fixtures, manifest)
    doc_paths = (
        list(SCHEMAS_DIR.glob("*.json"))
        + list(OPENAPI_DIR.glob("*.yaml"))
        + list(FIXTURES_DIR.rglob("*.json"))
        + [MANIFEST, CANONICALIZATION_VECTORS, SEMANTIC_DIGEST_VECTORS, STRICT_JSON_VECTORS]
    )
    for p in doc_paths:
        try:
            load_json(p)
            rep.check(True, "")
        except Exception as exc:  # noqa: BLE001
            rep.fail(f"parse error: {p}: {exc}")

    # 2/3. schemas -> registry
    registry = build_registry(rep)
    validate_canonicalization_vectors(rep)
    validate_strict_json_vectors(rep)
    validate_semantic_digest_vectors(rep)

    # 4. OpenAPI documents
    for name in ["flaggo-runtime-v1.yaml", "flaggo-management-v1.yaml"]:
        validate_openapi(OPENAPI_DIR / name, rep)
    operations = load_openapi_operations()

    # 5. manifest + coverage + drift
    try:
        manifest = load_json(MANIFEST)
    except Exception as exc:  # noqa: BLE001
        rep.fail(f"manifest does not parse: {exc}")
        print_report(rep)
        return 1

    cases = manifest.get("cases", [])
    rep.check(len(cases) > 0, "manifest has no cases")
    rep.check(
        manifest.get("totalCases") == len(cases),
        f"manifest totalCases is {manifest.get('totalCases')}, expected {len(cases)}",
    )

    indexed_paths: set[Path] = set()
    names: set[str] = set()
    covered: set[int] = set()
    manifest_by_path: dict[Path, dict] = {}
    for case in cases:
        name = case.get("name")
        rep.check(bool(name), "manifest case missing name")
        if name:
            rep.check(name not in names, f"duplicate case name in manifest: {name}")
            names.add(name)
        rel = case.get("file")
        rep.check(bool(rel), f"manifest case '{name}' missing file")
        if rel:
            fpath = (CONTRACTS / rel).resolve()
            rep.check(fpath.exists(), f"manifest case '{name}' file not found: {rel}")
            indexed_paths.add(fpath)
            manifest_by_path[fpath] = case
        for sc in case.get("scenarios", []):
            covered.add(int(sc))

    on_disk = {p.resolve() for p in FIXTURES_DIR.rglob("*.json")}
    missing_from_manifest = on_disk - indexed_paths
    for p in sorted(missing_from_manifest):
        rep.fail(f"fixture on disk not indexed in manifest: {p.relative_to(CONTRACTS)}")

    required = set(range(1, REQUIRED_SCENARIO_COUNT + 1))
    uncovered = required - covered
    rep.check(not uncovered, f"required scenarios not covered by fixtures: {sorted(uncovered)}")

    # 6. validate every fixture body
    fixtures_validated = 0
    loaded_fixtures: list[dict] = []
    for p in sorted(on_disk):
        fixture = validate_fixture(p, registry, operations, rep)
        if fixture is not None:
            loaded_fixtures.append(fixture)
            declared_scenarios = set(fixture.get("scenarios", [fixture.get("scenario")]))
            manifest_scenarios = set(manifest_by_path.get(p, {}).get("scenarios", []))
            rep.check(
                declared_scenarios == manifest_scenarios,
                f"fixture:{p.relative_to(CONTRACTS)}: declared scenarios {sorted(declared_scenarios)} "
                f"do not match manifest {sorted(manifest_scenarios)}",
            )
        fixtures_validated += 1

    secured_operations = [operation for operation in operations if operation["scopes"]]
    for operation in secured_operations:
        covered_by_scope_fixture = any(
            fixture.get("expected", {}).get("status") == 403
            and fixture.get("expected", {}).get("body", {}).get("code") == "insufficient-scope"
            and (
                matched := match_operation(
                    fixture.get("request", {}).get("method"),
                    fixture.get("request", {}).get("path"),
                    operations,
                )
            ) is operation
            for fixture in loaded_fixtures
        )
        rep.check(
            covered_by_scope_fixture,
            f"secured operation {operation['method']} {operation['route']} lacks a 403 insufficient-scope fixture",
        )

    stateful_scenarios = execute_stateful_scenarios(loaded_fixtures, rep)
    rep.check(
        stateful_scenarios == 3,
        "scenario 19 and both scenario 20 rejection paths must execute statefully",
    )
    validate_identity_consistency(loaded_fixtures, rep)

    print_report(rep, extra={
        "schemas": len(SCHEMA_FILES),
        "openapi_docs": 2,
        "fixtures": fixtures_validated,
        "manifest_cases": len(cases),
        "scenarios_covered": len(covered & required),
        "stateful_scenarios": stateful_scenarios,
    })
    return 1 if rep.errors else 0


def print_report(rep: Report, extra: dict | None = None) -> None:
    print("=" * 68)
    print("Flaggo Phase 1 contract validation")
    print("=" * 68)
    if extra:
        for k, v in extra.items():
            print(f"  {k:20s}: {v}")
    print(f"  checks_run          : {rep.checks}")
    if rep.errors:
        print(f"\nFAILED with {len(rep.errors)} error(s):")
        for e in rep.errors:
            print(f"  - {e}")
    else:
        print("\nAll checks passed.")


if __name__ == "__main__":
    sys.exit(main())
