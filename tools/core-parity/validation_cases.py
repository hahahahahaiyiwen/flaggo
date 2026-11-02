"""Compare validation results, including issue ordering, messages and native faults."""

import copy
import json
from pathlib import Path


def cases(root: Path) -> list[dict]:
    requests = []
    fixture = json.loads((
        root / "contracts" / "fixtures" / "management" / "decision-contract" /
        "01-validate-valid.json"
    ).read_text(encoding="utf-8"))
    contract = fixture["request"]["body"]
    digest = fixture["expected"]["body"]["contractDigest"]

    def add(identifier, model, document, *, value=None, value_action=False, supplied_contract=None):
        request = {
            "id": "validation-" + identifier, "domainModel": model,
            "domainAction": "validateValue" if value_action else "validate",
            "document": document if isinstance(document, str) else json.dumps(document, ensure_ascii=False),
        }
        if value_action:
            request["validationValue"] = json.dumps(value, ensure_ascii=False)
        if supplied_contract is not None:
            request["validationContract"] = json.dumps(supplied_contract, ensure_ascii=False)
        requests.append(request)

    add("valid-contract", "contract", contract)
    integer_contract_prefix = (
        '{"authority":{"tenant":"local","application":"parity","environment":"test"},'
        '"name":"integer.zero","expression_syntax":"flaggo.cel/v1",'
        '"attributes":[{"name":"n","schema":{"type":"integer"}}],'
        '"result":{"schema":{"type":"integer"},"default":'
    )
    integer_contract = json.loads(integer_contract_prefix + '0}}')
    # Raw text is required: Python integers and parsed JSON erase lexical -0.
    for name, token in [("zero", "0"), ("negative-zero", "-0"), ("decimal-zero", "-0.0"),
                        ("exponent-zero", "-0e0"), ("signed-exponent-zero", "-0E+0")]:
        add("lexical-" + name, "schema", {"type": "integer"}, value_action=True)
        requests[-1]["validationValue"] = token
        add("default-" + name, "contract", integer_contract_prefix + token + '}}')
        add("input-" + name, "runtimeInput", '{"attributes":{"_random":0,"n":' + token + '}}',
            supplied_contract=integer_contract)
        add("return-" + name, "contract", integer_contract_prefix + '0},'
            '"authoredExecutable":{"rules":[{"name":"r","when":{"expression":"true"},'
            '"return":{"value":' + token + '}}]}}')
    for name, mutate in [
        ("empty-authority", lambda c: c.update(authority={})),
        ("bad-name", lambda c: c.update(name="9invalid")),
        ("trailing-lf-name", lambda c: c.update(name="accepted\n")),
        ("trailing-crlf-name", lambda c: c.update(name="rejected\r\n")),
        ("long-name", lambda c: c.update(name="a" * 129)),
        ("bad-syntax", lambda c: c.update(expression_syntax="other")),
        ("bad-attribute-name", lambda c: c["attributes"][0].update(name="_private")),
        ("attribute-trailing-lf", lambda c: c["attributes"][0].update(name="accepted\n")),
        ("duplicate-attribute", lambda c: c["attributes"].append(copy.deepcopy(c["attributes"][0]))),
        ("null-attribute", lambda c: c["attributes"].append(None)),
        ("invalid-default", lambda c: c["result"].update(default="wrong")),
        ("invalid-result-schema", lambda c: c["result"].update(schema={"type": "integer", "minimum": 900, "maximum": 100, "multipleOf": 0})),
        ("empty-authored", lambda c: c["authoredExecutable"].update(rules=[])),
        ("null-authored-rule", lambda c: c["authoredExecutable"]["rules"].append(None)),
        ("duplicate-rule", lambda c: c["authoredExecutable"]["rules"].append(copy.deepcopy(c["authoredExecutable"]["rules"][0]))),
        ("bad-member-name", lambda c: c["authoredExecutable"]["rules"][0].update(name="_invalid")),
        ("whitespace-condition", lambda c: c["authoredExecutable"]["rules"][0].update(when={"condition": "\u00a0\t"})),
        ("natural-language-condition", lambda c: c["authoredExecutable"]["rules"][0].update(when={"condition": "high pressure"})),
        ("whitespace-expression", lambda c: c["authoredExecutable"]["rules"][0].update(when={"expression": " \t"})),
        ("expression-byte-limit", lambda c: c["authoredExecutable"]["rules"][0].update(when={"expression": "x" * 4096})),
        ("expression-over-byte-limit", lambda c: c["authoredExecutable"]["rules"][0].update(when={"expression": "\u00e9" * 2049})),
        ("bad-literal-return", lambda c: c["authoredExecutable"]["rules"][0].update(**{"return": {"value": None}})),
        ("unsupported-policy", lambda c: c["learning"]["policy"].update(mode="manual")),
        ("empty-evidence", lambda c: c["learning"].update(evidence=[])),
        ("null-evidence", lambda c: c["learning"]["evidence"].append(None)),
        ("duplicate-evidence-and-source", lambda c: c["learning"]["evidence"].append(copy.deepcopy(c["learning"]["evidence"][0]))),
        ("unknown-evidence-attribute", lambda c: c["learning"]["evidence"][0].update(attribute="missing")),
        ("bad-correlations", lambda c: c["learning"]["evidence"][0].update(correlateBy=[None, "", "z", "a", "z"])),
        ("unexpected-correlations", lambda c: c["learning"]["evidence"][0]["source"].update(correlation={
            "z/~": None, "a": {"location": "bad", "attribute": ""}})),
        ("invalid-metric", lambda c: c["learning"]["evidence"][0]["source"].update(metricKind="invalid", unit="x" * 129, scope="", name="")),
        ("metric-unit-utf16-bound", lambda c: c["learning"]["evidence"][0]["source"].update(unit="\U0001f600" * 64)),
        ("metric-unit-over-utf16-bound", lambda c: c["learning"]["evidence"][0]["source"].update(unit="\U0001f600" * 65)),
        ("parent-span-wrong-source", lambda c: c["learning"]["evidence"][0]["source"]["correlation"]["session_id"].update(location="parentSpan")),
        ("unknown-primary", lambda c: c["learning"]["objective"]["primary"].update(evidence="missing", direction="invalid")),
        ("null-guardrail", lambda c: c["learning"]["objective"]["guardrails"].append(None)),
        ("duplicate-guardrail", lambda c: c["learning"]["objective"]["guardrails"].append(copy.deepcopy(c["learning"]["objective"]["guardrails"][0]))),
        ("blank-guardrail", lambda c: c["learning"]["objective"]["guardrails"][0].update(expression=" ")),
        ("unchecked-guardrail-syntax", lambda c: c["learning"]["objective"]["guardrails"][0].update(expression="not parsed {")),
        ("guardrail-no-base-byte-limit", lambda c: c["learning"]["objective"]["guardrails"][0].update(expression="x" * 4097)),
    ]:
        changed = copy.deepcopy(contract)
        mutate(changed)
        add("contract-" + name, "contract", changed)
    for size in (128, 129):
        changed = copy.deepcopy(contract)
        changed["attributes"] = [{"name": f"a{i}", "schema": {"type": "integer"}} for i in range(size)]
        changed.pop("learning")
        add(f"attribute-count-{size}", "contract", changed)
    for interval in [
        "PT15M", "P1Y2M3W4DT5H6M7.25S", "P1D", "PT0.1S", "P0D", "PT0S",
        "P", "PT", "P1YT", "P1.5D", "PT1.5H", "PT1M1H", "P1D\n", "P1D\r\n",
        "P\u0661Y1M", "P\u0661D", "P\U0001d7d9Y1M",
    ]:
        changed = copy.deepcopy(contract)
        changed["learning"]["policy"]["evaluate"]["interval"] = interval
        add("interval-" + str(len(requests)), "contract", changed)
    for source_kind, extra in [
        ("log", {}), ("span", {}), ("spanEvent", {"spanName": ""}),
        ("spanEvent", {"spanName": "parent"}),
    ]:
        changed = copy.deepcopy(contract)
        changed["learning"]["evidence"][0]["source"] = {
            "kind": source_kind, "scope": "\U0001f600" * 128, "name": "signal",
            "correlation": {"session_id": {"location": "parentSpan", "attribute": "session"}},
            **extra,
        }
        add("source-validation-" + str(len(requests)), "contract", changed)
    changed = copy.deepcopy(contract)
    repeated = copy.deepcopy(changed["learning"]["evidence"][0])
    repeated.update(name="different", correlateBy=[])
    repeated["source"]["correlation"] = {}
    changed["learning"]["evidence"].append(repeated)
    add("source-identity-ignores-correlation-mapping", "contract", changed)

    keywords = {
        "minimum": -1, "maximum": 2, "exclusiveMinimum": 0.5, "exclusiveMaximum": 2.5,
        "multipleOf": 0.5, "minLength": 1, "maxLength": 2,
        "items": {"type": "boolean"}, "minItems": 1, "maxItems": 2, "uniqueItems": False,
        "properties": {}, "required": [], "additionalProperties": False,
        "minProperties": 1, "maxProperties": 2, "description": "metadata",
    }
    for kind in ("null", "boolean", "integer", "number", "string", "array", "object", "unsupported"):
        baseline = {"type": kind}
        if kind == "array":
            baseline["items"] = {"type": "integer"}
        if kind == "object":
            baseline.update(properties={}, additionalProperties=False)
        add("schema-" + kind, "schema", baseline)
        for name, value in keywords.items():
            add(f"schema-{kind}-{name}", "schema", {**baseline, name: value})
    for name, schema in [
        ("array-missing-items", {"type": "array"}),
        ("object-open", {"type": "object", "properties": {"z": None}}),
        ("object-null-properties", {"type": "object", "properties": None, "additionalProperties": False}),
        ("object-nested-errors", {"type": "object", "properties": {
            "z/~": None, "b": {"type": "invalid"}, "a": {"type": "array"}},
            "additionalProperties": False, "required": ["missing", None, "", "missing"]}),
        ("bounds-priority", {"type": "number", "minimum": 0, "exclusiveMinimum": 10, "maximum": 5}),
        ("integer-keywords", {"type": "integer", **{key: 0.1 for key in ("minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf")}}),
    ]:
        add("schema-" + name, "schema", schema)
    for kind, lower, upper, maximum in [
        ("string", "minLength", "maxLength", 16384),
        ("array", "minItems", "maxItems", 256),
        ("object", "minProperties", "maxProperties", 256),
    ]:
        for minimum, max_value in [(0, maximum), (-1, 1), (1, -1), (2, 1), (maximum + 1, maximum + 1)]:
            add(f"range-{kind}-{minimum}-{max_value}", "schema", {"type": kind, lower: minimum, upper: max_value})

    for kind in ("null", "boolean", "integer", "number", "string", "array", "object", "unsupported"):
        for index, value in enumerate([None, True, 1, 1.0, "x", [], {}]):
            add(f"value-type-{kind}-{index}", "schema", {"type": kind}, value=value, value_action=True)
    for index, value in enumerate([-(2**63), 2**63 - 1, 2**63, 1.0, 0.0, -0.0, 1e0, 0.1]):
        add(f"int64-value-{index}", "schema", {"type": "integer"}, value=value, value_action=True)
    for multiple in (0.1, 0.3, 1e-5, 1e16, 1e17, 0.0, -2.0):
        for value in (0.3, 1.0):
            add(f"multiple-{multiple}-{value}", "schema", {"type": "number", "multipleOf": multiple}, value=value, value_action=True)
    for multiple in (1e-7, 1e-5, 1e-4, 1e16, 1e17, 1.2345678901234567e20):
        add(f"multiple-format-{multiple}", "schema", {"type": "number", "multipleOf": multiple},
            value=multiple * 1.5, value_action=True)
    for index, value in enumerate([-1, 0, 0.5, 1, 2, 3]):
        add(f"numeric-bounds-{index}", "schema", {"type": "number", "minimum": 0, "maximum": 2,
            "exclusiveMinimum": 0, "exclusiveMaximum": 2}, value=value, value_action=True)
    for index, value in enumerate(["", "a", "\U0001f600", "e\u0301", "a" * 16385]):
        add(f"string-runes-{index}", "schema", {"type": "string", "minLength": 1, "maxLength": 1}, value=value, value_action=True)
    for index, value in enumerate([[1, 1.0], [9007199254740992, 9007199254740993],
                                  [{"a": 1, "b": 2}, {"b": 2, "a": 1}], [1, "1"],
                                  list(range(256)), list(range(257))]):
        add(f"array-unique-{index}", "schema", {"type": "array", "uniqueItems": True}, value=value, value_action=True)
    for index, value in enumerate([[], [1], [1, "bad", None]]):
        add(f"array-items-{index}", "schema", {"type": "array", "items": {"type": "integer"}, "minItems": 1, "maxItems": 2}, value=value, value_action=True)
    for name, schema, value in [
        ("order-and-pointers", {"type": "object", "properties": {"z/~": {"type": "integer"}, "b": {"type": "boolean"}}},
         {"z/~": "bad", "unknown": 1, "b": 2, "a": 3}),
        ("required-order", {"type": "object", "required": ["z/~", "a"]}, {}),
        ("null-required-fault", {"type": "object", "required": [None]}, {}),
        ("null-property-fault", {"type": "object", "properties": {"x": None}}, {"x": 1}),
        ("object-count-limit", {"type": "object", "properties": {}}, {str(i): 0 for i in range(257)}),
    ]:
        add("object-" + name, "schema", schema, value=value, value_action=True)

    for index, random in enumerate([None, False, 0, -0.0, -1, 0.9999999999999999, 1, "0.5"]):
        add(f"random-{index}", "runtimeInput", {"attributes": {"_random": random}}, supplied_contract=contract)
    add("missing-random", "runtimeInput", {"attributes": {}}, supplied_contract=contract)
    add("runtime-order", "runtimeInput", {"attributes": {"z/~": 1, "_unknown": 1, "a": 1, "current_level": 1.0, "_random": 0}}, supplied_contract=contract)
    for null_attribute in (False, True):
        changed = copy.deepcopy(contract)
        changed["attributes"].append(None if null_attribute else copy.deepcopy(changed["attributes"][0]))
        add(f"runtime-invalid-contract-fault-{null_attribute}", "runtimeInput", {"attributes": {"_random": 0}}, supplied_contract=changed)
    for exposure in ("", "x" * 256, "x" * 257, "\U0001f600" * 64, "\U0001f600" * 65):
        add(f"exposure-{len(requests)}", "runtimeInput", {"attributes": {"_random": 0}, "currentExposure": {"exposureId": exposure}}, supplied_contract=contract)
    for size in (129, 130):
        add(f"runtime-count-{size}", "runtimeInput", {"attributes": {"_random": 0, **{f"a{i}": 0 for i in range(size - 1)}}}, supplied_contract=contract)

    executable = {"contractDigest": digest, "rules": [{"name": "r", "when": {"expression": "true"}, "return": {"value": 250}}]}
    add("valid-executable", "executable", executable, supplied_contract=contract)
    for name, mutate in [
        ("wrong-digest-kind", lambda e: e.update(contractDigest="wrong", kind="other")),
        ("no-rules", lambda e: e.update(rules=[])),
        ("null-rule-fault", lambda e: e.update(rules=[None])),
        ("duplicate-rules", lambda e: e["rules"].append(copy.deepcopy(e["rules"][0]))),
        ("missing-predicate", lambda e: e["rules"][0].update(when={})),
        ("blank-predicate", lambda e: e["rules"][0].update(when={"expression": ""})),
        ("predicate-too-large", lambda e: e["rules"][0].update(when={"expression": "x" * 4097})),
        ("blank-return", lambda e: e["rules"][0].update(**{"return": {"expression": " "}})),
        ("bad-return", lambda e: e["rules"][0].update(**{"return": {"value": 99}})),
    ]:
        changed = copy.deepcopy(executable)
        mutate(changed)
        add("executable-" + name, "executable", changed, supplied_contract=contract)
    for size in (256, 257):
        changed = copy.deepcopy(executable)
        changed["rules"] = [{**copy.deepcopy(executable["rules"][0]), "name": f"r{i}"} for i in range(size)]
        add(f"rule-count-{size}", "executable", changed, supplied_contract=contract)
    invalid = copy.deepcopy(contract)
    invalid["name"] = "invalid name"
    add("executable-invalid-contract-short-circuit", "executable", {"contractDigest": "wrong", "kind": "other", "rules": [None]}, supplied_contract=invalid)
    return requests
