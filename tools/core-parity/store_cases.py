"""Real SQLite contract-store parity, including cross-runtime existing databases."""

import base64
import json
import sqlite3
import tempfile
from contextlib import closing
from pathlib import Path


def contract(default=99, name="persistent.parity"):
    return {
        "authority": {"tenant": "local", "application": "parity", "environment": "test"},
        "name": name, "expression_syntax": "flaggo.cel/v1",
        "attributes": [{"name": "x", "schema": {"type": "integer", "description": "original"}}],
        "result": {"schema": {"type": "integer"}, "default": default},
        "authoredExecutable": {"rules": [{
            "name": "r", "when": {"expression": " attributes.x >= 1 "},
            "return": {"expression": "attributes.x+ 2"},
        }]},
    }


def operations():
    result = []

    def add(identifier, action, **values):
        result.append({"id": identifier, "action": action, **values})

    def put(identifier, model, time="2026-03-01T10:00:00.0000000+00:00", **values):
        add(identifier, "put", document=json.dumps(model, ensure_ascii=False), acceptedAt=time, **values)

    add("health-before", "health")
    add("initialize", "initialize")
    add("initialize-again", "initialize")
    add("health-after", "health")
    add("current-before", "current", contractName="persistent.parity")
    put("v1", contract())
    put("v2", contract(100), "2026-03-01T10:01:00.1234567+00:00")
    put("v3", contract(101), "2026-03-01T12:01:00.1234567+02:00")
    duplicate = contract()
    duplicate["attributes"][0]["schema"]["description"] = "replacement should not overwrite"
    put("duplicate", duplicate, "2026-03-01T10:09:00.0000000+00:00")
    add("immutable", "get", use="v1")
    add("current-still-empty", "current", use="v1")
    add("set-current", "setCurrent", use="v2")
    add("current", "current", use="v1")
    add("page1", "list", use="v1", pageSize=2)
    add("page2", "list", use="v1", pageSize=2, cursorFrom="page1")
    add("catalog", "catalog")
    add("missing-version", "get", use="v1", contractDigest="sha256:" + "0" * 64)
    add("missing-current-update", "setCurrent", use="v1", contractDigest="sha256:" + "0" * 64)
    add("current-after-failed-update", "current", use="v1")
    for size in (-1, 0, 1, 200, 201):
        add("page-size-" + str(size), "list", use="v1", pageSize=size)
    stamp = "2026-03-01T10:01:00.1234567+00:00"
    digest = "sha256:" + "a" * 64
    valid = base64.urlsafe_b64encode((stamp + "\n" + digest).encode()).decode().rstrip("=")
    for index, cursor in enumerate([
        "", "*", "not-a-cursor", valid, valid + " ", valid + "    ",
        "\n" + valid + "\n\n\n", valid + "====",
        *[base64.b64encode((time + "\n" + digest).encode()).decode() for time in (
            "2026-03-01T10:01:00+00:00", "2026-03-01T10:01:00.123456+00:00",
            "2026-03-01T10:01:00.12345678+00:00", "2026-03-01T10:01:00.1234567Z",
            "0000-03-01T10:01:00.1234567+00:00", "2026-03-01T10:01:00.1234567+14:01",
            "2026-03-01T10:01:60.1234567+00:00",
        )],
        base64.b64encode((stamp + "\nsha256:" + "A" * 64).encode()).decode(),
        base64.b64encode((stamp + "\n" + digest + "\n").encode()).decode(),
        base64.b64encode(stamp.encode()).decode(),
    ]):
        add("cursor-" + str(index), "list", use="v1", pageSize=1, cursor=cursor)
    conflicting = contract()
    conflicting["authority"]["tenant"] = "different"
    put("conflicting-owner", conflicting)
    put("wrong-digest", contract(200), contractDigest="sha256:" + "0" * 64)
    for index, name in enumerate([" leading", "trailing ", "final\n", "contains space"]):
        put("name-" + str(index), contract(name=name))
    for field in ("tenant", "application", "environment"):
        for value in (None, "", " \t\n", "\u00a0"):
            model = contract()
            if value is None:
                model["authority"].pop(field)
            else:
                model["authority"][field] = value
            put("scope-" + str(len(result)), model)
    for action, values in [
        ("get", {"use": "v1"}), ("current", {"use": "v1"}),
        ("list", {"use": "v1", "pageSize": 1}), ("health", {}),
        ("setCurrent", {"use": "v3"}),
    ]:
        add("cancel-" + action, action, cancelled=True, **values)
    put("cancel-put", contract(201), cancelled=True)
    add("after-cancel", "current", use="v1")
    add("health-before-corruption", "health")
    add("save-copy", "sql", query="CREATE TABLE saved_versions AS SELECT * FROM decision_contract_versions;")
    for identifier, query in [
        ("name", """UPDATE decision_contract_versions SET contract_json =
            CAST(replace(CAST(contract_json AS TEXT), '"name":"persistent.parity"', '"name":"other.name"') AS BLOB)
            WHERE contract_name='persistent.parity';"""),
        ("digest", """UPDATE decision_contract_versions SET contract_json =
            CAST(replace(CAST(contract_json AS TEXT), '"default":99', '"default":199') AS BLOB)
            WHERE contract_name='persistent.parity';"""),
        ("json", "UPDATE decision_contract_versions SET contract_json = X'7B' WHERE contract_name='persistent.parity';"),
        ("infinite-schema", """UPDATE decision_contract_versions SET contract_json =
            CAST(replace(CAST(contract_json AS TEXT), '"type":"integer"',
                         '"type":"integer","minimum":1e400') AS BLOB)
            WHERE contract_name='persistent.parity';"""),
        ("time", "UPDATE decision_contract_versions SET accepted_at = 'bad' WHERE contract_name='persistent.parity';"),
    ]:
        add("corrupt-" + identifier, "sql", query=query)
        add("read-corrupt-" + identifier, "get", use="v1")
        add("restore-" + identifier, "sql", query="""
            UPDATE decision_contract_versions SET
                accepted_at = (SELECT saved.accepted_at FROM saved_versions saved
                    WHERE saved.contract_name = decision_contract_versions.contract_name
                      AND saved.contract_digest = decision_contract_versions.contract_digest),
                contract_json = (SELECT saved.contract_json FROM saved_versions saved
                    WHERE saved.contract_name = decision_contract_versions.contract_name
                      AND saved.contract_digest = decision_contract_versions.contract_digest);
        """)
    add("unsupported-schema", "sql", query="UPDATE flaggo_schema_versions SET version=4 WHERE component='contract-store';")
    add("unsupported-health", "health")
    add("unsupported-initialize", "initialize")
    add("restore-schema", "sql", query="UPDATE flaggo_schema_versions SET version=3 WHERE component='contract-store';")
    add("final-health", "health")
    add("final-read", "get", use="v1")
    return result


def request(database, expressions, ops, identifier="contract-store"):
    return {"id": identifier, "storeAction": "contract", "database": str(database),
            "withExpressions": expressions, "operations": ops}


def snapshot(path):
    with closing(sqlite3.connect(path)) as database:
        return {
            "versions": database.execute(
                "SELECT contract_name, contract_digest, accepted_at, contract_json "
                "FROM decision_contract_versions ORDER BY contract_name, contract_digest").fetchall(),
            "resources": database.execute(
                "SELECT * FROM decision_contract_resources ORDER BY contract_name").fetchall(),
            "current": database.execute(
                "SELECT * FROM decision_contract_current ORDER BY contract_name").fetchall(),
            "schema": database.execute(
                "SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name").fetchall(),
            "components": database.execute("SELECT * FROM flaggo_schema_versions ORDER BY component").fetchall(),
        }


def verify(oracle, candidate):
    import run
    cases, reference, actual = [], {}, {}

    def compare_sequence(identifier, ops, expected, observed):
        if len(expected) != len(ops) or len(observed) != len(ops):
            raise ValueError("Store reply counts do not match operation counts.")
        for operation, left, right in zip(ops, expected, observed):
            if left.get("id") != operation["id"] or right.get("id") != operation["id"]:
                raise ValueError("Store operation IDs are reordered or missing.")
            key = identifier + "-" + operation["id"]
            cases.append({"id": key})
            reference[key] = {"status": "value", "value": left}
            actual[key] = {"status": "value", "value": right}

    with tempfile.TemporaryDirectory(prefix="flaggo-contract-parity-") as directory:
        directory = Path(directory)
        for expressions in (False, True):
            label = "canonical" if expressions else "raw"
            left, right = directory / (label + "-dotnet.db"), directory / (label + "-rust.db")
            ops = operations()
            expected = run.output(oracle, [request(left, expressions, ops)])["contract-store"]["value"]
            observed = run.output(candidate, [request(right, expressions, ops)])["contract-store"]["value"]
            by_id = {reply["id"]: reply for reply in expected}
            if by_id["v1"].get("value") != "created" or by_id["duplicate"].get("value") != "existing":
                raise RuntimeError("Reference store did not exercise immutable persistence.")
            if by_id["health-before"]["value"] is not False or by_id["final-health"]["value"] is not True:
                raise RuntimeError("Reference store did not preserve schema/health boundaries.")
            compare_sequence(label, ops, expected, observed)
            for producer, database in [("dotnet", left), ("rust", right)]:
                before = snapshot(database)
                reads = [{"id": "reinitialize", "action": "initialize"}, *[
                    {"id": "read-" + str(index), "action": "get",
                     "contractName": name, "contractDigest": digest}
                    for index, (name, digest, _, _) in enumerate(before["versions"])
                ]]
                reads.extend([{"id": "catalog", "action": "catalog"}, {"id": "health", "action": "health"}])
                first = run.output(oracle, [request(database, False, reads)])["contract-store"]["value"]
                second = run.output(candidate, [request(database, False, reads)])["contract-store"]["value"]
                compare_sequence(label + "-" + producer + "-restart", reads, first, second)
                if snapshot(database) != before:
                    raise RuntimeError("Restart reads rewrote existing contract bytes or state.")
        existing = directory / "existing-v5.db"
        bootstrap = [{"id": "executable-schema", "action": "initializeExecutable"}]
        run.output(oracle, [request(existing, True, bootstrap)])
        with closing(sqlite3.connect(existing)) as database:
            before_schema = database.execute(
                "SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name").fetchall()
        ops = [op for op in operations() if op["id"] in {
            "initialize", "v1", "set-current", "current", "catalog", "health-after",
        }]
        ops = [{**op, "use": "v1"} if op["action"] == "setCurrent" else op for op in ops]
        initialized = run.output(candidate, [request(existing, True, ops)])["contract-store"]["value"]
        if any(reply["outcome"] != "ok" for reply in initialized):
            raise RuntimeError("Rust could not initialize/write contracts alongside executable schema v5.")
        before = snapshot(existing)
        if len(before["versions"]) != 1 or len(before["current"]) != 1:
            raise RuntimeError("Existing-schema fixture did not persist and select a contract version.")
        reads = [{"id": "catalog", "action": "catalog"}, {"id": "health", "action": "health"}]
        compare_sequence("existing-v5", reads,
                         run.output(oracle, [request(existing, True, reads)])["contract-store"]["value"],
                         run.output(candidate, [request(existing, True, reads)])["contract-store"]["value"])
        with closing(sqlite3.connect(existing)) as database:
            after_schema = database.execute(
                "SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name").fetchall()
        if not set(before_schema).issubset(after_schema) or ("executable-store", 5) not in before["components"]:
            raise RuntimeError("Contract initialization changed existing executable schema v5.")
    report = run.compare(cases, reference, actual)
    report["storageProof"] = {
        "contractSchemaVersion": 3, "existingExecutableSchemaVersion": 5,
        "producerConsumerCombinations": 4, "reinitializationPreservesRawBytes": True,
    }
    return report
