"""Decision runtime vectors and cross-runtime executable restart/corruption gates."""

import copy
import json
import expression_cases


def contract():
    return {
        "authority": {"tenant": "local", "application": "parity", "environment": "test"},
        "name": "runtime.parity", "expression_syntax": "flaggo.cel/v1",
        "attributes": [
            {"name": name, "schema": (
                {"type": "array", "items": {"type": "integer"}} if kind == "array" else
                {"type": "object", "properties": {"extra": {"type": "integer"}},
                 "additionalProperties": False} if kind == "object" else {"type": kind})}
            for name, kind in [("x", "integer"), ("y", "integer"), ("n", "number"),
                               ("label", "string"), ("items", "array"), ("object", "object")]
        ],
        "result": {"schema": {"type": "integer"}, "default": 99},
    }


def rule(name="r", predicate="true", result=None):
    return {"name": name, "when": {"expression": predicate},
            "return": {"value": 7} if result is None else result}


def request(identifier, rules=None, *, model=None, attributes=None, action="runtime", **extra):
    return {
        "id": "decision-" + identifier, "decisionAction": action,
        "contract": json.dumps(model or contract(), ensure_ascii=False),
        "executable": json.dumps({
            "contractDigest": "sha256:" + "a" * 64,
            "rules": [rule()] if rules is None else rules,
        }, ensure_ascii=False),
        "input": json.dumps({"attributes": {"_random": 0.5, **(attributes or {})}},
                            ensure_ascii=False),
        **extra,
    }


def cases():
    cases = [
        request("first-match", [rule("first"), rule("second")]),
        request("later-match", [rule("false", "false"), rule("second")]),
        request("overflow-literal", [rule(predicate="1e400 > 0.0")]),
        request("empty", []),
        request("false-default", [rule(predicate="false")]),
        request("missing-predicate", [rule(predicate="attributes.x > 0")]),
        request("missing-return", [
            rule("skip", "true", {"expression": "attributes.y"}), rule("next")]),
        request("missing-return-before-failing-predicate", [
            rule("skip", "1 / attributes.x > 0", {"expression": "attributes.y"}), rule("next")],
                attributes={"x": 0}),
        request("missing-optional-reference-even-unreachable", [
            rule("skip", "true || attributes.x > 0"), rule("next")]),
        request("return-missing-even-dead-branch", [
            rule("skip", "true", {"expression": "true ? 1 : attributes.x"}), rule("next")]),
        request("dynamic", [rule(result={"expression": "attributes.x + attributes.y"})],
                attributes={"x": 3, "y": 5}),
        request("predicate-failure", [rule(predicate="1 / attributes.x > 0"), rule("next")],
                attributes={"x": 0}),
        request("return-failure", [rule(result={"expression": "1 / attributes.x"}), rule("next")],
                attributes={"x": 0}),
        request("overflow-return", [rule(result={"expression": "attributes.x + 1"})],
                attributes={"x": 9223372036854775807}),
        request("dynamic-nonboolean-predicate", [
            rule(predicate="attributes.object.extra"), rule("next")],
                attributes={"object": {"extra": 1}}),
        request("cancel-rule", cancelled=True),
        request("cancel-missing-attribute", [rule(predicate="attributes.x > 0")], cancelled=True),
        request("cancel-empty-default", [], cancelled=True),
        request("current-exposure"),
    ]
    for index, (left, right) in enumerate([
        ({"type": "integer"}, {"type": "string"}),
        ({"type": "array", "items": {"type": "integer"}},
         {"type": "array", "items": {"type": "string"}}),
        ({"type": "integer"}, {"type": "integer"}),
    ]):
        model = contract()
        model["attributes"] = [
            {"name": "choose", "schema": {"type": "boolean"}},
            {"name": "a", "schema": {"type": "array", "items": left}},
            {"name": "b", "schema": {"type": "array", "items": right}},
        ]
        model["result"] = {"schema": {"type": "array", "items": left}, "default": []}
        cases.append(request("conditional-container-" + str(index), [
            rule(result={"expression": "attributes.choose ? attributes.a : attributes.b"}),
        ], model=model, attributes={"choose": False, "a": [], "b": []}))
    for case in expression_cases.nullable_cases():
        if case["resultType"] == "boolean":
            cases.append(request(case["id"], [rule(predicate=case["expression"])]))
    for name, value in [
        ("missing", None), ("null", None), ("string", "0.5"), ("boolean", True),
        ("negative", -0.1), ("one", 1), ("above-one", 2), ("zero", 0),
        ("negative-zero", -0.0), ("below-one", 0.9999999999999999),
    ]:
        case = request("random-" + name)
        attributes = {} if name == "missing" else {"_random": value}
        case["input"] = json.dumps({"attributes": attributes})
        cases.append(case)
    for name, value in [
        ("undeclared", 1), ("_unknown", 2), ("slash~/attribute", 3),
        ("x", 1.5), ("y", "bad"), ("items", "bad"), ("object", []),
    ]:
        cases.append(request("input-" + name, attributes={name: value}))
    case = request("input-issue-order")
    case["input"] = json.dumps({"attributes": {
        "unknown": 3, "_extra": False, "x": "bad", "items": False,
    }, "currentExposure": {"exposureId": ""}})
    cases.append(case)
    for length in (0, 256, 257):
        for token in ("a", "\u00e9"):
            case = request(f"exposure-{ord(token)}-{length}")
            case["input"] = json.dumps({
                "attributes": {"_random": 0.5}, "currentExposure": {"exposureId": token * length},
            })
            cases.append(case)
    for role in ("contract", "active"):
        for fault in ("missing", "integrity", "dependency", "timeout", "cancelled"):
            cases.append(request(f"{role}-fault-{fault}", **{role + "Fault": fault}))
    cases.extend([
        request("identity-before-stores", contractName="bad name", contractFault="integrity"),
        request("digest-before-stores", contractDigest="bad", contractFault="integrity"),
        request("unknown-name", contractName="unknown.name"),
        request("unknown-digest", contractDigest="sha256:" + "b" * 64),
        request("input-before-active", attributes={"x": "bad"}, activeFault="integrity"),
        request("contract-before-input", attributes={"x": "bad"}, contractFault="missing"),
    ])
    for schema, default, expression, attributes in [
        ({"type": "integer", "minimum": 0}, 0, "attributes.x", {"x": -1}),
        ({"type": "integer", "maximum": 5}, 0, "attributes.x", {"x": 6}),
        ({"type": "string", "maxLength": 1}, "", "attributes.label", {"label": "long"}),
        ({"type": "array", "items": {"type": "integer"}}, [], "attributes.items", {"items": [1, "x"]}),
        ({"type": "object", "properties": {}, "additionalProperties": False},
         {}, "attributes.object", {"object": {"extra": 1}}),
        ({"type": "number"}, 0, "attributes.n / 0.0", {"n": 1}),
        ({"type": "number"}, 0, "attributes.n / 0.0", {"n": 0}),
        ({"type": "array", "items": {"type": "number"}}, [],
         "[attributes.n / 0.0]", {"n": 1}),
        ({"type": "object", "properties": {"n": {"type": "number"}},
          "additionalProperties": False}, {},
         "{'n': attributes.n / 0.0}", {"n": 1}),
    ]:
        model = contract()
        model["result"] = {"schema": schema, "default": default}
        cases.append(request("result-" + str(len(cases)),
                             [rule(result={"expression": expression})],
                             model=model, attributes=attributes))
    for count in (256, 257):
        cases.append(request("rule-limit-" + str(count),
                             [rule("r" + str(index), "false") for index in range(count)]))
    for name, rules in [
        ("duplicate", [rule(), rule()]), ("bad-name", [rule("bad name")]),
        ("bad-predicate", [rule(predicate="1")]),
        ("bad-return-type", [rule(result={"expression": "'wrong'"})]),
        ("bad-literal", [rule(result={"value": "wrong"})]),
        ("undeclared-expression", [rule(predicate="attributes.unknown == 1")]),
        ("syntax-error", [rule(predicate="true +")]),
        ("empty-when", [{"name": "r", "when": {}, "return": {"value": 1}}]),
    ]:
        cases.append(request("compile-" + name, rules, action="compile"))
    for kind, default, returned in [
        ("boolean", False, True), ("string", "", "\u00e9"),
        ("array", [], [1, 2]), ("object", {}, {"a": [1, 2]}),
        ("number", 0.0, 9007199254740992), ("integer", 0, -9223372036854775808),
    ]:
        model = contract()
        model["result"] = {"schema": {"type": kind}, "default": default}
        if kind == "array":
            model["result"]["schema"]["items"] = {"type": "integer"}
        if kind == "object":
            model["result"]["schema"].update(properties={
                "a": {"type": "array", "items": {"type": "integer"}},
            }, additionalProperties=False)
        cases.append(request("literal-" + kind, [rule(result={"value": returned})], model=model))
        cases.append(request("default-" + kind, [], model=model))
    changed = contract()
    changed["result"]["default"] = "invalid"
    cases.append(request("invalid-default-result", [], action="evaluate",
                         evaluationContract=json.dumps(changed)))
    cases.append(request("random-rule", [rule("random", "attributes._random < 0.75")]))
    cases.append(request("floating-native-fault",
                         [rule(result={"expression": "attributes.n * 1.7976931348623157e308"})],
                         model={**contract(), "result": {"schema": {"type": "number"}, "default": 0}},
                         attributes={"n": 2}))
    for case in list(cases):
        if case["decisionAction"] == "runtime" and not any(
            key in case for key in ("contractFault", "activeFault", "contractName", "contractDigest")
        ):
            cases.append({**case, "id": case["id"] + "-evaluator", "decisionAction": "evaluate"})
    return cases


def restart_cases(requests, responses):
    restarted = []
    for case in requests:
        response = responses[case["id"]]
        if response["status"] != "value" or response["value"].get("outcome") != "decision":
            continue
        artifact = response.get("artifact")
        if not isinstance(artifact, dict) or not all(
            name in artifact for name in ("executable", "executableDigest", "checkedExecutable")
        ):
            raise ValueError(f"Decision {case['id']} is missing its persisted executable artifact.")
        restarted.append({**case, "artifact": copy.deepcopy(artifact),
                          "executable": "invalid JSON; admission source is unavailable after restart"})
    return restarted


def validate_reference(responses):
    for identifier, outcome in {
        "first-match": "decision", "later-match": "decision", "empty": "decision",
        "missing-return": "decision", "missing-return-before-failing-predicate": "decision",
        "predicate-failure": "error", "return-failure": "error", "random-missing": "error",
        "rule-limit-256": "decision", "rule-limit-257": "compile-invalid",
        "cancel-rule": "error", "cancel-empty-default": "decision",
        "literal-object": "decision", "literal-array": "decision",
        "invalid-default-result": "error", "floating-native-fault": "error",
    }.items():
        response = responses["decision-" + identifier]
        if response.get("value", {}).get("outcome") != outcome:
            raise RuntimeError(f"Reference did not exercise required decision outcome: {identifier}")
    first = responses["decision-first-match"]["value"]["decision"]
    if first["result"] != 7 or first["evaluation"] != {"source": "rule", "rule": "first"}:
        raise RuntimeError("The reference did not preserve first-match rule precedence.")
    skipped = responses["decision-missing-return-before-failing-predicate"]["value"]["decision"]
    if skipped["evaluation"] != {"source": "rule", "rule": "next"}:
        raise RuntimeError("The reference evaluated a rule with a missing return attribute.")


def corruption_cases(restarted):
    original = next(case for case in restarted if case["id"] == "decision-first-match")
    cases = []

    def add(name, change):
        case = copy.deepcopy(original)
        case.update(id="decision-corrupt-" + name, decisionAction="evaluate")
        change(case["artifact"])
        cases.append(case)

    add("digest", lambda value: value.update(executableDigest="sha256:" + "0" * 64))
    add("binding", lambda value: value["executable"].update(contractDigest="sha256:" + "0" * 64))
    add("kind", lambda value: value["executable"].update(kind="unknown"))
    add("count", lambda value: value["checkedExecutable"]["rules"].pop())
    add("name", lambda value: value["checkedExecutable"]["rules"][0].update(name="other"))
    add("canonical", lambda value: value["checkedExecutable"]["rules"][0]["predicate"].update(
        canonicalExpression="false"))
    add("ast", lambda value: value["checkedExecutable"]["rules"][0]["predicate"].update(checkedAst="AA=="))
    add("ast-digest", lambda value: value["checkedExecutable"]["rules"][0]["predicate"].update(
        checkedAstDigest="sha256:" + "0" * 64))
    add("refs", lambda value: value["checkedExecutable"]["rules"][0]["predicate"].update(
        referencedAttributes=["x"]))
    add("unexpected-return", lambda value: value["checkedExecutable"]["rules"][0].update(
        result=copy.deepcopy(value["checkedExecutable"]["rules"][0]["predicate"])))
    return cases
