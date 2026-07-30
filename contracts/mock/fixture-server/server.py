#!/usr/bin/env python3
"""Fixture-backed mock server for the Flaggo Phase 1 contract.

Selects a golden fixture by the `X-Flaggo-Fixture` request header, verifies that
the incoming method and path match the fixture's recorded request, and replays
the fixture's expected status, headers, and body. It is dependency-free (Python
standard library only) and reads the same golden fixtures the SDK and service
consume, so it never invents responses.

Usage:
    python contracts/mock/fixture-server/server.py --port 8080

Then send a request naming the fixture:
    curl -si -X POST http://localhost:8080/v1/decisions/tetris.dropInterval:decide \
         -H "X-Flaggo-Fixture: decide-active-numeric-strategy" \
         -H "Content-Type: application/json" --data '{}'

Discovery endpoints (no fixture header required):
    GET /_fixtures        -> list every selectable fixture name
    GET /_health          -> mock server liveness
"""
from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import rfc8785
from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT202012

CONTRACTS = Path(__file__).resolve().parents[2]
FIXTURES_DIR = CONTRACTS / "fixtures"
SCHEMAS_DIR = CONTRACTS / "schemas"
FIXTURE_HEADER = "X-Flaggo-Fixture"
SCHEMA_ID_PREFIX = "https://flaggo.dev/contracts/schemas/"
SCHEMA_FILES = [
    "runtime-models-v1.schema.json",
    "management-models-v1.schema.json",
    "decision-definition-bundle-v1.schema.json",
    "problem-details-v1.schema.json",
]

# name -> fixture dict, loaded once at startup
FIXTURES: dict[str, dict] = {}
SCHEMA_REGISTRY = Registry()


def strict_json_loads(value: bytes):
    return json.loads(
        value,
        parse_constant=lambda constant: (_ for _ in ()).throw(
            ValueError(f"non-finite JSON number '{constant}' is forbidden")
        ),
    )


def canonical_json_bytes(value) -> bytes:
    return rfc8785.dumps(value)


def load_schema_registry() -> None:
    global SCHEMA_REGISTRY
    resources = []
    for name in SCHEMA_FILES:
        schema = strict_json_loads((SCHEMAS_DIR / name).read_bytes())
        Draft202012Validator.check_schema(schema)
        resources.append((
            SCHEMA_ID_PREFIX + name,
            Resource.from_contents(schema, default_specification=DRAFT202012),
        ))
    SCHEMA_REGISTRY = Registry().with_resources(resources)


def request_schema_errors(body, schema_ref: str) -> list:
    file_part, separator, pointer = schema_ref.partition("#")
    target = SCHEMA_ID_PREFIX + file_part + (separator + pointer if separator else "")
    validator = Draft202012Validator(
        {"$ref": target},
        registry=SCHEMA_REGISTRY,
        format_checker=FormatChecker(),
    )
    return list(validator.iter_errors(body))


def load_fixtures() -> None:
    load_schema_registry()
    FIXTURES.clear()
    for path in sorted(FIXTURES_DIR.rglob("*.json")):
        with path.open(encoding="utf-8") as fh:
            fx = json.load(fh)
        # Only fixtures that model a real HTTP exchange are selectable.
        if fx.get("sdkLocal") or fx.get("schemaNegative"):
            continue
        FIXTURES[fx["name"]] = fx


def problem_bytes(status: int, code: str, detail: str) -> bytes:
    return json.dumps({
        "type": f"https://flaggo.dev/problems/{code}",
        "title": code.replace("-", " ").capitalize(),
        "status": status,
        "code": code,
        "detail": detail,
    }).encode("utf-8")


class Handler(BaseHTTPRequestHandler):
    server_version = "flaggo-fixture-mock/1.0"

    def log_message(self, fmt, *args):  # quieter logging
        pass

    def _send(self, status: int, headers: dict, body: bytes) -> None:
        self.send_response(status)
        sent_ct = False
        for key, value in headers.items():
            if key.lower() == "content-type":
                sent_ct = True
            self.send_header(key, str(value))
        if not sent_ct:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _send_problem(self, status: int, code: str, detail: str) -> None:
        body = problem_bytes(status, code, detail)
        self._send(status, {"Content-Type": "application/problem+json"}, body)

    def _read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length", 0) or 0)
        return self.rfile.read(length) if length else b""

    def _handle(self) -> None:
        request_bytes = self._read_body()

        if self.path == "/_fixtures" and self.command == "GET":
            body = json.dumps({"fixtures": sorted(FIXTURES.keys())}, indent=2).encode("utf-8")
            self._send(200, {"Content-Type": "application/json"}, body)
            return
        if self.path == "/_health" and self.command == "GET":
            body = json.dumps({"status": "ok", "fixtures": len(FIXTURES)}).encode("utf-8")
            self._send(200, {"Content-Type": "application/json"}, body)
            return

        fixture_name = self.headers.get(FIXTURE_HEADER)
        if not fixture_name:
            self._send_problem(400, "missing-fixture-selector",
                               f"Provide the {FIXTURE_HEADER} header naming a golden fixture. GET /_fixtures to list them.")
            return

        fx = FIXTURES.get(fixture_name)
        if fx is None:
            self._send_problem(404, "fixture-not-found",
                               f"No selectable fixture named '{fixture_name}'.")
            return

        req = fx["request"]
        if self.command != req["method"]:
            self._send_problem(409, "fixture-method-mismatch",
                               f"Fixture '{fixture_name}' expects {req['method']}, got {self.command}.")
            return
        if self.path != req["path"]:
            self._send_problem(409, "fixture-path-mismatch",
                               f"Fixture '{fixture_name}' expects path '{req['path']}', got '{self.path}'.")
            return

        expected_headers = req.get("headers", {})
        for key, expected_value in expected_headers.items():
            actual_value = self.headers.get(key)
            if key.lower() == "authorization":
                matches = bool(actual_value)
            else:
                matches = actual_value == str(expected_value)
            if not matches:
                self._send_problem(
                    400,
                    "fixture-header-mismatch",
                    f"Fixture '{fixture_name}' requires header '{key}'.",
                )
                return

        if "body" in req:
            content_type = self.headers.get("Content-Type", "").split(";", 1)[0].strip().lower()
            if content_type != "application/json":
                self._send_problem(415, "unsupported-media-type", "Request body must use application/json.")
                return
            try:
                request_body = strict_json_loads(request_bytes)
            except (UnicodeDecodeError, ValueError, json.JSONDecodeError) as exc:
                self._send_problem(400, "invalid-json", f"Request body is not strict JSON: {exc}")
                return
            if request_body != req["body"]:
                self._send_problem(
                    409,
                    "fixture-body-mismatch",
                    f"Request body does not match fixture '{fixture_name}'.",
                )
                return
            schema_ref = fx.get("requestSchema")
            if schema_ref:
                errors = request_schema_errors(request_body, schema_ref)
                if errors:
                    self._send_problem(
                        400,
                        "fixture-request-schema-invalid",
                        f"Fixture request violates {schema_ref}: {errors[0].message}",
                    )
                    return
        elif request_bytes:
            self._send_problem(400, "unexpected-request-body", "Selected fixture does not accept a body.")
            return

        exp = fx["expected"]
        headers = dict(exp.get("headers", {}))
        if "body" in exp:
            body = (
                canonical_json_bytes(exp["body"])
                if fx.get("canonicalResponse")
                else json.dumps(exp["body"], separators=(",", ":")).encode("utf-8")
            )
        else:
            body = b""
            headers.pop("Content-Type", None)
        self._send(int(exp["status"]), headers, body)

    do_GET = _handle
    do_POST = _handle
    do_PUT = _handle
    do_DELETE = _handle
    do_PATCH = _handle
    do_HEAD = _handle


def main() -> int:
    parser = argparse.ArgumentParser(description="Flaggo fixture-backed mock server")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()

    load_fixtures()
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"flaggo fixture mock: http://{args.host}:{args.port} ({len(FIXTURES)} fixtures)")
    print(f"select a case with the {FIXTURE_HEADER} header; GET /_fixtures to list.")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
