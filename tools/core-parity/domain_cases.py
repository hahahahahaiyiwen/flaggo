"""Domain parity vectors; JSON remains text to preserve duplicates and source order."""

import copy
import json
from pathlib import Path


def cases(root: Path) -> list[dict]:
    requests = []

    def add(identifier, model, document, action="roundtrip"):
        requests.append({
            "id": "domain-" + identifier, "domainModel": model, "domainAction": action,
            "document": document if isinstance(document, str) else json.dumps(document, ensure_ascii=False),
        })

    fixture = json.loads((
        root / "contracts" / "fixtures" / "management" / "decision-contract" /
        "01-validate-valid.json"
    ).read_text(encoding="utf-8"))["request"]["body"]
    add("fixture", "contract", fixture, "digest")
    changed = copy.deepcopy(fixture)
    changed["attributes"].reverse()
    changed["learning"]["evidence"].reverse()
    changed["attributes"][0]["schema"]["description"] = "display only"
    changed["learning"]["evidence"][0]["description"] = "display only"
    changed["authoredExecutable"]["rules"][0]["description"] = "display only"
    changed["learning"]["objective"]["guardrails"][0]["description"] = "display only"
    add("reordered-descriptions", "contract", changed, "digest")
    for name, names in [
        ("utf16", ["\ue000", "\U00010000", "a"]),
        ("ascii", ["z", "a", "b"]),
    ]:
        changed = copy.deepcopy(fixture)
        changed["attributes"] = [
            {"name": name, "schema": {
                "type": "object", "required": names,
                "properties": {"value": {"type": "array", "items": {
                    "type": "object", "required": list(reversed(names)),
                    "description": "ignored", "additionalProperties": False,
                }}},
            }}
            for name in names
        ]
        changed["learning"]["evidence"][0]["correlateBy"] = list(reversed(names))
        add("sorting-" + name, "contract", changed, "digest")

    for name, mutate in [
        ("missing-authority-fields", lambda value: value.update(authority={})),
        ("null-authority-field", lambda value: value["authority"].update(tenant=None)),
        ("null-attribute", lambda value: value.update(attributes=[None])),
        ("missing-name", lambda value: value.pop("name")),
        ("null-name", lambda value: value.update(name=None)),
        ("unknown-contract-property", lambda value: value.update(version=3)),
        ("mis-cased-contract-property", lambda value: value.update(Name="other")),
        ("unknown-schema-property", lambda value: value["result"]["schema"].update(pattern="x")),
        ("null-schema-property", lambda value: value["result"]["schema"].update(properties={"a": None})),
        ("null-rule", lambda value: value["authoredExecutable"].update(rules=[None])),
        ("empty-authored-condition", lambda value: value["authoredExecutable"]["rules"][0].update(when={"condition": ""})),
        ("whitespace-authored-condition", lambda value: value["authoredExecutable"]["rules"][0].update(when={"condition": " "})),
        ("two-authored-when-members", lambda value: value["authoredExecutable"]["rules"][0].update(when={"condition": "x", "expression": "true"})),
        ("null-evidence", lambda value: value["learning"].update(evidence=[None])),
        ("null-correlation-entry", lambda value: value["learning"]["evidence"][0]["source"].update(correlation={"a": None})),
        ("null-correlate-by", lambda value: value["learning"]["evidence"][0].update(correlateBy=[None])),
        ("null-guardrail", lambda value: value["learning"]["objective"].update(guardrails=[None])),
    ]:
        changed = copy.deepcopy(fixture)
        mutate(changed)
        add(name, "contract", changed)

    for kind, extra in [("metric", {"metricKind": "sum", "unit": "ms"}),
                        ("log", {}), ("span", {}), ("spanEvent", {"spanName": "operation"})]:
        changed = copy.deepcopy(fixture)
        changed["learning"]["evidence"][0]["source"] = {
            "kind": kind, "scope": "s", "name": "n", "correlation": {}, **extra,
        }
        add("source-" + kind, "contract", changed, "digest")

    executable = {"contractDigest": "sha256:" + "a" * 64, "rules": [
        {"name": "r", "when": {"expression": "true"}, "return": {"value": {
            "description": "semantic", "null": None, "array": [2, 1],
        }}},
    ]}
    add("executable", "executable", executable, "digest")
    for name, rules in [
        ("null-rule-digest", [None]),
        ("mixed-null-rule-digest", [None, *executable["rules"], None]),
        ("empty-rules-digest", []),
    ]:
        add(name, "executable", {**executable, "rules": rules}, "digest")
    for name, when in [
        ("missing-expression", {}), ("empty-expression", {"expression": ""}),
        ("null-expression", {"expression": None}), ("condition", {"condition": "test"}),
        ("extra-member", {"expression": "true", "extra": True}),
    ]:
        changed = copy.deepcopy(executable)
        changed["rules"][0]["when"] = when
        add("executable-" + name, "executable", changed)
    for name, result in [
        ("literal-null", {"value": None}), ("empty-return", {}),
        ("expression-return", {"expression": "1 + 2"}),
        ("empty-expression-return", {"expression": ""}),
        ("two-return-members", {"value": 1, "expression": "1"}),
    ]:
        changed = copy.deepcopy(executable)
        changed["rules"][0]["return"] = result
        add(name, "executable", changed)
    for name, document in [
        ("attributes", {"attributes": {"x": None, "y": [1, True, "text"]}}),
        ("exposure", {"attributes": {}, "currentExposure": {"exposureId": "test"}}),
        ("null-exposure", {"attributes": {}, "currentExposure": None}),
        ("missing-attributes", {}), ("null-attributes", {"attributes": None}),
        ("unknown-input", {"attributes": {}, "extra": True}),
    ]:
        add("input-" + name, "runtimeInput", document)

    for name, document in [
        ("duplicate", '{"a":1,"a":2}'),
        ("escaped-duplicate", '{"a":1,"\\u0061":2}'),
        ("nested-duplicate", '{"a":[{"x":1,"x":2}]}'),
        ("comment", '{"x":1/*comment*/}'), ("trailing-comma", '[1,]'),
        ("trailing-document", '{} {}'), ("empty-document", ''),
        ("literal-null", 'null'), ("invalid-surrogate", '"\\ud800"'),
        ("invalid-low-surrogate", '"\\udc00"'),
        ("invalid-property-surrogate", '{"\\ud800":1}'),
        ("invalid-escape", '"\\x20"'), ("invalid-hex-escape", '"\\uZZZZ"'),
        ("invalid-property-escape", '{"\\uZZZZ":1}'),
        ("valid-surrogate-pair", '"\\ud83d\\ude00"'),
        ("large-integer", '9007199254740993'),
    ]:
        add("json-" + name, "json", document)
    for depth in (16, 17):
        add(f"json-depth-{depth}", "json", "[" * depth + "0" + "]" * depth)
    for size in (262144, 262145):
        add(f"json-document-bytes-{size}", "json", "0" + " " * (size - 1))
    for size in (16384, 16385):
        add(f"json-string-bytes-{size}", "json", json.dumps("x" * size))
        add(f"json-name-bytes-{size}", "json", json.dumps({"x" * size: 1}))
    for count in (8192, 8193):
        add(f"json-unicode-bytes-{count}", "json", json.dumps("\u00e9" * count, ensure_ascii=False))
    return requests
