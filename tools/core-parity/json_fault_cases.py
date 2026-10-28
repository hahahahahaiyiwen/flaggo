"""Keep raw number lexemes and distinguish JSON rejection from native faults."""

import copy
import json
from pathlib import Path


def cases(root: Path) -> list[dict]:
    fixture = json.loads((
        root / "contracts" / "fixtures" / "management" / "decision-contract" /
        "01-validate-valid.json"
    ).read_text(encoding="utf-8"))
    contract = fixture["request"]["body"]
    requests = []

    def add(name, model, document, action="roundtrip", **extra):
        requests.append({
            "id": "json-fault-" + name, "domainModel": model, "domainAction": action,
            "document": document if isinstance(document, str) else json.dumps(document),
            **extra,
        })

    for name, source in [
        ("missing", {}), ("late", {"scope": "s", "kind": "log", "name": "n", "correlation": {}}),
        ("unknown", {"kind": "other"}), ("null-kind", {"kind": None}),
        ("integer-kind", {"kind": 1}), ("array-kind", {"kind": []}),
        ("null-source", None), ("array-source", []), ("string-source", "log"),
        ("missing-member", {"kind": "log", "scope": "s", "correlation": {}}),
        ("unknown-member-first", {"extra": True, "kind": "log"}),
        ("unknown-member-after", {"kind": "log", "extra": True}),
        ("metadata-first", {"$id": "a", "kind": "log"}),
        ("metadata-unknown-first", {"$other": "a", "kind": "log"}),
        ("metadata-after-kind", {"kind": "log", "$id": "a"}),
        ("metadata-reference", {"$ref": "a", "kind": "log"}),
        ("metadata-values", {"$values": [], "kind": "log"}),
        ("metadata-default-type", {"$type": "log", "kind": "log"}),
        ("metadata-dollar-kind", {"$kind": "log", "kind": "log"}),
        ("valid", {"kind": "log", "scope": "s", "name": "n", "correlation": {}}),
    ]:
        for action in ["roundtrip", "validate", "digest"]:
            changed = copy.deepcopy(contract)
            changed["learning"]["evidence"][0]["source"] = source
            add(f"source-{name}-{action}", "contract", changed, action)
    for name, text in [
        ("error-before-source", '{"name":false,"learning":__LEARNING__}'),
        ("error-after-source", '{"learning":__LEARNING__,"name":false}'),
    ]:
        learning = copy.deepcopy(contract["learning"])
        learning["evidence"][0]["source"] = {}
        add(name, "contract", text.replace("__LEARNING__", json.dumps(learning)))
    changed = copy.deepcopy(contract)
    changed["learning"]["evidence"][0]["source"] = {}
    add("duplicate-before-polymorphism", "contract", json.dumps(changed).replace(
        '"source": {}', '"source": {"kind":"log","kind":"metric"}'))
    for token in ["1e400", "-1e400", "1e-400", "1.7976931348623157e308",
                  "1.7976931348623159e308"]:
        name = token.replace("-", "negative-").replace(".", "_")
        for raw in [token, '{"n":[' + token + ']}']:
            add(f"number-{name}-{len(raw)}", "json", raw, "rawRoundtrip")
        for kind in ["number", "integer", "string"]:
            add(f"value-{kind}-{name}", "schema", {"type": kind}, "validateValue",
                validationValue=token)
        for action in ["validate", "digest"]:
            changed = copy.deepcopy(contract)
            changed["result"]["default"] = "__NUMBER__"
            add(f"default-{name}-{action}", "contract",
                json.dumps(changed).replace('"__NUMBER__"', token), action)
            changed = copy.deepcopy(contract)
            changed["authoredExecutable"]["rules"][0]["return"] = {"value": "__NUMBER__"}
            add(f"return-{name}-{action}", "contract",
                json.dumps(changed).replace('"__NUMBER__"', token), action)
            executable = {
                "contractDigest": fixture["expected"]["body"]["contractDigest"],
                "rules": [{"name": "r", "when": {"expression": "true"},
                           "return": {"value": "__NUMBER__"}}],
            }
            add(f"executable-{name}-{action}", "executable",
                json.dumps(executable).replace('"__NUMBER__"', token), action,
                validationContract=json.dumps(contract))
        for action in ["roundtrip", "validate"]:
            add(f"bound-{name}-{action}", "schema",
                '{"type":"number","minimum":' + token + '}', action)
        add(f"input-{name}", "runtimeInput", '{"attributes":{"_random":' + token + '}}',
            "validate", validationContract=json.dumps(contract))
        for keyword in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf"]:
            add(f"bound-{keyword}-{name}", "schema",
                '{"type":"number","' + keyword + '":' + token + '}', "validate")
        for action in ["validate", "digest"]:
            changed = copy.deepcopy(contract)
            changed["attributes"][0]["schema"]["minimum"] = "__NUMBER__"
            add(f"expression-bound-{name}-{action}", "contract",
                json.dumps(changed).replace('"__NUMBER__"', token), action, withExpressions=True)
    add("unique-overflow", "schema", {"type": "array", "uniqueItems": True},
        "validateValue", validationValue="[1e400,1e400]")
    add("unique-nested-overflow", "schema", {"type": "array", "uniqueItems": True},
        "validateValue", validationValue='[{"n":1e400}]')
    return requests
