"""Production expression adapters, syntax-only canonicalization and cancellation."""

import copy
import json
from pathlib import Path


def review_cases() -> list[dict]:
    requests = [{
        "id": "overflow-literal-" + str(index), "expression": source,
        "resultType": "boolean", "compileOnly": False,
    } for index, source in enumerate([
        "1e400 > 0.0", "-1e400 < 0.0", "1e400 == 1e400", "1e400 > 1.0",
    ])]
    for index, (left, right) in enumerate([
        ({"type": "integer"}, {"type": "string"}),
        ({"type": "array", "items": {"type": "integer"}},
         {"type": "array", "items": {"type": "string"}}),
        ({"type": "integer"}, {"type": "integer"}),
        (None, {"type": "integer"}),
    ]):
        requests.append({
            "id": "conditional-container-" + str(index),
            "expression": "size(attributes.choose ? attributes.a : attributes.b)",
            "schemas": {
                "choose": {"type": "boolean"},
                "a": {"type": "array", **({"items": left} if left else {})},
                "b": {"type": "array", "items": right},
            },
            "attributes": {"choose": True, "a": [], "b": []},
            "resultType": "integer", "compileOnly": False,
        })
    return requests


def lexical_cases() -> list[str]:
    return [
        "true & false", "true | false", "a = b", "a @ b", "not valid { @",
        "'unterminated", "b'unterminated", "'''unterminated", "`unterminated",
        "'\\q'", "'\\x0'", "'\\uZZZZ'", "0x", "0xG", "1e", "1e+", "1.",
        "1.e2", "1..a", "1.0u", "true # false",
        "'' '\\q'", "not valid { '\\q'", "'\\uD800'", "'\\U00110000'",
        "b'\\u0041'", "'\\12'", "b'\\400'", "attributes.``", "attributes.`😀`",
        "1 `field`", "'😀\\q'", "'\\x😀'", "'a\nb'", "b'a\nb'",
    ]


def syntax_cases() -> list[dict]:
    sources = [
        "sum( evidence.recovery_failure )<= 0",
        "unknown( evidence.a, evidence.b )",
        "(true ? evidence.a : evidence.b).value",
        "{'b': 2, 'a': [1, 2]}['a']",
        "[1, 2, 3]",
        "1 in [1, 2]",
        "has(evidence.value)",
        "evidence.value.startsWith('x')",
        "Example{value: 1}",
        "1u",
        "b'abc'",
        "b'\\x00\\xff'",
        "attributes.`a/b`",
        "attributes.`in`",
        "a + (b - c)",
        "(a + b) * c",
        "a ? b : c ? d : e",
        "not valid {",
        "",
        "x" * 4097,
        "has(1)",
        "[1, 2].map(x, x + 1)",
        "[1, 2].filter(x, x > 0)",
        "[1, 2].all(x, x > 0)",
        "[1, 2].exists(x, x > 0)",
        "[1, 2].exists_one(x, x > 0)",
        "evidence.items.map(x, x.items.filter(y, y > 0))",
        "[,]", "{,}", "Type{,}", "[1,]", "{1: 2,}", "Type{field: 1,}",
        ".qualified.Type{field: [1, 2]}",
        "--1", "---1", "!!true", "!!!true", "--attributes.current_level",
        "-9223372036854775808", "-0x8000000000000000", "-0.0",
        "1e400 > 0.0", "-1e400 < 0.0",
        "a ? (b ? c : d) : e", "a ? b ? c : d : e",
        "(", "[", "{", "Type{", "f(", "f(1,)", "[1,,2]", "{1}", "Type{1:2}",
        "a.", "a[", "a[]", "a.?b", "a[?1]", "[?1]", "{?1:2}", "Type{?field:1}",
        "1 +", "1 ** 2", "1 / / 2", "!-1", "-!true",
        "9223372036854775808", "18446744073709551616u",
        "(" * 249 + "1" + ")" * 249,
        "(" * 250 + "1" + ")" * 250,
        " + ".join(["1"] * 1000),
        " + ".join(["1"] * 1001),
        r"""r'\q'""", r"""br'\x41'""", r"""rb'\x41'""",
        r"""'\?\'\"\`'""", r"""b'\303\251'""", r"""'\u00e9\U0001f600'""",
        "'''first\nsecond'''", "b'😀é'",
    ]
    sources.extend(lexical_cases())
    sources.extend(f"b'\\x{byte:02x}'" for byte in range(256))
    return [{"id": f"syntax-{index}", "syntaxExpression": source}
            for index, source in enumerate(sources)]


def compiler_cases(root: Path) -> list[dict]:
    fixture = json.loads((
        root / "contracts" / "fixtures" / "management" / "decision-contract" /
        "01-validate-valid.json"
    ).read_text(encoding="utf-8"))
    original = fixture["request"]["body"]
    requests = []

    def add(identifier, model, document, action="validate", supplied_contract=None):
        request = {
            "id": "compiler-" + identifier, "domainModel": model, "domainAction": action,
            "withExpressions": True, "document": json.dumps(document, ensure_ascii=False),
        }
        if supplied_contract is not None:
            request["validationContract"] = json.dumps(supplied_contract, ensure_ascii=False)
        requests.append(request)

    formatted = copy.deepcopy(original)
    rules = formatted["authoredExecutable"]["rules"]
    rules[0]["when"]["expression"] = " attributes.board_pressure>= 0.8 "
    rules[1]["return"]["expression"] = "1000.0-double( attributes.current_level )*25.0"
    formatted["learning"]["objective"]["guardrails"][0]["expression"] = (
        "sum( evidence.recovery_failure )<= 0"
    )
    for name, contract in [("fixture", original), ("formatted", formatted)]:
        for action in ("validate", "digest", "canonicalize"):
            add(f"{name}-{action}", "contract", contract, action)
    for name, expression in [
        ("opaque-guardrail", "unknown(evidence.a, evidence.b)"),
        ("structured-guardrail", "{'a': [1, 2]}['a']"),
        ("macro-guardrail", "evidence.items.map(x, x + 1)"),
    ]:
        changed = copy.deepcopy(original)
        changed["learning"]["objective"]["guardrails"][0]["expression"] = expression
        add(name + "-validate", "contract", changed)
        add(name + "-digest", "contract", changed, "digest")

    for name, expression in [
        ("undeclared", "attributes.missing"),
        ("predicate-type", "1"),
        ("unknown-function", "unknown(1)"),
        ("list", "[1, 2]"),
        ("root-identifier", "evidence"),
        ("parser-diagnostic", "not valid {"),
        ("overload-diagnostic", 'attributes.board_pressure + "x"'),
        ("receiver-profile", "attributes.board_pressure.map(x, x)"),
        ("unknown-before-undeclared", "unknown(attributes.missing)"),
        ("unknown-before-type-error", 'unknown(1 + "x")'),
        ("profile-before-type-error", '(1 + "x") == [1]'),
        ("declaration-before-type-error", '(1 + "x") == attributes.missing'),
        ("sibling-type-errors", '(1 + "x") == (true + 2)'),
        ("multiline-type-error", 'true &&\n("😀" + 1 == 2)'),
        ("utf16-type-error", '"😀" + 1 == 0'),
        ("nested-type-error", '(1 + "x") + 2 == 0'),
        ("arity", "int(1, 2) == 0"),
        ("zero-arity", "bool()"),
        ("unary-overload", '!1'),
        ("empty-group", "()"),
        ("missing-right-paren", "(true"),
        ("argument-trailing-comma", "int(1,)"),
        ("empty-index", "attributes.level[]"),
        ("optional-select", "attributes.?level"),
        ("optional-index", "attributes[?1]"),
        ("optional-list", "[?1]"),
        ("numeric-overflow", "9223372036854775808"),
        ("uint-overflow", "18446744073709551616u"),
        ("uint-profile", "1u"),
        ("bytes-profile", "b'x'"),
        ("mixed-unary", "!-1"),
        ("nested-middle-ternary", "true ? true ? true : false : false"),
        ("ast-depth", " + ".join(["1"] * 33)),
        ("static-cost", '"' + "a" * 1001 + '" == "' + "a" * 1001 + '"'),
        ("unbounded-cost", "attributes.board_pressure == 0.0 && string(1) == '1'"),
        ("result-before-cost", "string(1) + 'x'"),
    ]:
        changed = copy.deepcopy(original)
        changed["authoredExecutable"]["rules"][0]["when"] = {"expression": expression}
        add(name, "contract", changed)

    for index, expression in enumerate(lexical_cases()):
        changed = copy.deepcopy(original)
        changed["authoredExecutable"]["rules"][0]["when"] = {"expression": expression}
        add(f"lexical-{index}", "contract", changed)

    for index, expression in enumerate([
        "attributes.extra_list + 1 == 0",
        "attributes.extra_map + 1 == 0",
        "attributes.extra_list",
        "attributes.extra_list == attributes.extra_map",
        "(true ? attributes.extra_list : attributes.extra_list) + 1 == 0",
    ]):
        changed = copy.deepcopy(original)
        changed["attributes"].extend([
            {"name": "extra_list", "schema": {"type": "array", "items": {"type": "integer"}}},
            {"name": "extra_map", "schema": {"type": "object", "properties": {}, "additionalProperties": False}},
        ])
        changed["authoredExecutable"]["rules"][0]["when"] = {"expression": expression}
        add(f"container-{index}", "contract", changed)

    executable = {
        "contractDigest": fixture["expected"]["body"]["contractDigest"],
        "rules": [{
            "name": "r", "when": {"expression": "attributes.current_level>= 1"},
            "return": {"expression": "double( attributes.current_level )+ 100.0"},
        }],
    }
    add("executable", "executable", executable, supplied_contract=original)
    add("executable-digest", "executable", executable, "digest")
    changed = copy.deepcopy(executable)
    changed["rules"][0]["when"] = {"expression": "attributes.missing"}
    add("executable-undeclared", "executable", changed, supplied_contract=original)
    for source in ("1e400 > 0.0", "-1e400 < 0.0"):
        changed = copy.deepcopy(original)
        changed["authoredExecutable"]["rules"][0]["when"] = {"expression": source}
        add("overflow-validate-" + source[0], "contract", changed)
        add("overflow-digest-" + source[0], "contract", changed, "digest")
    for index, case in enumerate(review_cases()[4:6]):
        changed = copy.deepcopy(original)
        changed["attributes"].extend(
            {"name": name, "schema": schema} for name, schema in case["schemas"].items())
        changed["authoredExecutable"]["rules"][0]["when"] = {
            "expression": case["expression"] + " > 0"}
        add("conditional-container-" + str(index), "contract", changed)
    return requests


def cancellation_cases() -> list[dict]:
    return [{
        "id": f"cancelled-{index}", "expression": expression,
        "schemas": {"level": {"type": "integer"}}, "attributes": {"level": 3},
        "resultType": kind, "compileOnly": False, "emitChecked": True, "cancelled": True,
    } for index, (expression, kind) in enumerate([
        ("1", "integer"),
        ("attributes.level + 1", "integer"),
        ("false && (1 / 0 == 1)", "boolean"),
    ])]
