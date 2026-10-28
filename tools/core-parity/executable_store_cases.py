"""Executable-v5 persistence, lifecycle transitions, and v4 migration parity."""

import json
import sqlite3
import tempfile
from contextlib import closing
from pathlib import Path

import store_cases


NOW = "2026-03-03T10:00:00.1234567+00:00"
TIME = "2026-03-01T10:00:00.0000000+00:00"
ZERO = "sha256:" + "0" * 64


def executable(variant=1):
    return {"contractDigest": ZERO, "rules": [{
        "name": "rule" + str(variant), "when": {"expression": " true "},
        "return": {"value": variant},
    }]}


def operations():
    ops = []

    def add(identifier, action, **values):
        ops.append({"id": identifier, "action": action, **values})

    def accept(identifier, default=99, name="persistent.parity"):
        contract = store_cases.contract(default, name)
        contract.pop("authoredExecutable")
        add(identifier, "accept", document=json.dumps(contract),
            createdAt=TIME)

    def put(identifier, variant=1, action="put", **values):
        add(identifier, action, **{
            "contract": "c1", "createdAt": TIME, "document": json.dumps(executable(variant)), **values})

    add("health-before", "health")
    add("initializeContract", "initializeContract")
    add("initialize", "initialize")
    add("initialize-again", "initialize")
    add("health-after", "health")
    accept("c1")
    put("initial")
    put("duplicate", provenance='{"replacement":true}')
    add("immutable", "get", use="initial")
    put("second", 2)
    put("third", 3)
    add("no-active", "active", use="initial")
    add("runtime-no-active", "runtime", contract="c1", input='{"attributes":{"_random":0.5}}')
    add("activate-initial", "activateIfNone", use="initial")
    add("activate-no-change", "activate", use="initial", expectedFrom="initial")
    add("activate-conflict", "activateIfNone", use="second")
    add("activate-wrong-expected", "activate", use="second", expected=ZERO)
    add("activate-missing", "activate", use="initial", executableDigest=ZERO)
    add("activate-binding", "activate", use="initial", contractDigest=ZERO)
    add("replace", "activate", use="second", expectedFrom="initial")
    add("first-inactive", "get", use="initial")
    add("runtime", "runtime", contract="c1", input='{"attributes":{"_random":0.5}}')
    add("history1", "list", use="initial", pageSize=2)
    add("history2", "list", use="initial", pageSize=2, cursorFrom="history1")
    add("candidate-only", "candidates", use="initial", pageSize=200)
    for action in ("list", "candidates"):
        for size in (-1, 0, 1, 200, 201):
            add(action + "-size-" + str(size), action, use="initial", pageSize=size)
        for index, cursor in enumerate(["", "*", "not-a-cursor"]):
            add(action + "-cursor-" + str(index), action, use="initial", pageSize=1, cursor=cursor)
    for name, changes in [
        ("bad-state", {"state": 1}), ("bad-version", {"stateVersion": 1}),
        ("bad-activated", {"activatedAt": NOW}), ("bad-digest", {"executableDigest": ZERO}),
        ("bad-checked", {"checked": {"rules": []}}), ("raw-source", {"raw": True}),
    ]:
        put(name, 20, **changes)
    put("admitted", 4, "admit", cycleId="cycle", provenance='{"manifest":{"n":1.00},"source":"analysis"}')
    put("admission-retry", 4, "admit", cycleId="cycle",
        createdAt="2026-03-01T10:05:00.0000000+00:00")
    put("admission-conflict", 5, "admit", cycleId="cycle")
    put("admission-attempt-conflict", 4, "admit", cycleId="cycle", attemptId="different")
    for name, changes in [
        ("bad-workspace", {"workspaceId": " "}), ("bad-cycle", {"cycleId": ""}),
        ("bad-attempt", {"attemptId": "\u00a0"}), ("bad-name", {"contractName": ""}),
        ("bad-manifest", {"manifestDigest": ZERO.upper()}),
        ("bad-admission-digest", {"admissionContractDigest": ZERO}),
        ("bad-admission-executable", {"admissionExecutableDigest": ZERO}),
        ("bad-admission-time", {"admissionTime": NOW}),
    ]:
        put(name, 6, "admit", **changes)
    put("active-admission-rejected", 2, "admit")
    put("old", 7, "admit")
    put("newest", 8, "admit", createdAt="2026-03-01T10:02:00.0000000+00:00")
    add("pending", "pending", pageSize=1000)
    add("resolve-bad-version", "resolve", snapshotFrom="pending", expectedVersion=-1, resolution=0)
    add("resolve-bad-action", "resolve", snapshotFrom="pending", resolution=2)
    add("resolve-stale-time", "resolve", snapshotFrom="pending", admissionTime=NOW, resolution=0)
    add("resolve-stale-version", "resolve", snapshotFrom="pending", expectedVersion=5, resolution=0)
    add("resolve-stale-active", "resolve", snapshotFrom="pending", expectedActive=None, resolution=0)
    add("resolve-activate", "resolve", snapshotFrom="pending", resolution=0)
    add("resolve-again", "resolve", snapshotFrom="pending", resolution=0)
    put("retry-resolved-admission", 8, "admit", cycleId="newest")
    add("manual-inactive", "activateCandidate", use="old", contract="c1", expectedVersion=0, expectedFrom="newest", activeVersion=3)
    put("manual", 9, "admit")
    put("manual-backlog", 10, "admit")
    add("manual-wrong-token", "activateCandidate", use="manual", contract="c1", expectedVersion=0, expectedFrom="newest", activeVersion=1)
    add("manual-negative", "activateCandidate", use="manual", contract="c1", expectedVersion=-1, expectedFrom="newest", activeVersion=3)
    add("manual-activate", "activateCandidate", use="manual", contract="c1", expectedVersion=0, expectedFrom="newest", activeVersion=3)
    add("manual-superseded", "get", use="manual-backlog")
    put("reject-old", 11, "admit")
    put("reject-newest", 12, "admit", createdAt="2026-03-01T10:03:00.0000000+00:00")
    add("reject-pending", "pending", pageSize=1)
    add("reject", "resolve", snapshotFrom="reject-pending", resolution=1)
    add("reject-again", "resolve", snapshotFrom="reject-pending", resolution=1)
    put("stale-contract", 13, "admit")
    add("stale-pending", "pending", pageSize=1)
    accept("c2", 100)
    add("manual-current-moved", "activateCandidate", use="stale-contract", contract="c1", expectedVersion=0, expectedFrom="manual", activeVersion=4)
    add("supersede-stale", "resolve", snapshotFrom="stale-pending", resolution=0)
    put("no-active-admitted", 14, "admit", contract="c2")
    add("no-active-pending", "pending", pageSize=1)
    add("resolve-initial-activation", "resolve", snapshotFrom="no-active-pending", resolution=0)
    add("runtime-current", "runtime", contract="c2", input='{"attributes":{"_random":0.5}}')
    dynamic = executable(16)
    dynamic["rules"][0]["when"]["expression"] = "attributes.x >= 1"
    dynamic["rules"][0]["return"] = {"expression": "attributes.x + 2"}
    put("dynamic", 16, contract="c2", document=json.dumps(dynamic))
    add("activate-dynamic", "activate", use="dynamic", expectedFrom="no-active-admitted")
    add("runtime-dynamic", "runtime", contract="c2", input='{"attributes":{"_random":0.5,"x":5}}')
    add("dynamic-checked", "get", use="dynamic")
    # A newer stale-version admission must not hide an older current-version candidate.
    put("prefer-current", 17, "admit", contract="c2")
    put("newer-stale", 18, "admit", createdAt=NOW)
    add("current-preference", "pending", pageSize=1)
    add("preference-activate", "resolve", snapshotFrom="current-preference", resolution=0)
    # Independent contract names prove keyset discovery and current-version preference.
    for identifier, name in [("alpha", "alpha.parity"), ("zulu", "zulu.parity")]:
        accept(identifier, name=name)
        put(identifier + "-candidate", 15, "admit", contract=identifier)
    add("pending-first", "pending", pageSize=1)
    add("pending-next", "pending", pageSize=1, afterContractName="alpha.parity")
    for size in (0, 1000, 1001):
        add("pending-size-" + str(size), "pending", pageSize=size)
    for action, values in [
        ("get", {"use": "initial"}), ("active", {"use": "initial"}),
        ("list", {"use": "initial", "pageSize": 1}), ("pending", {"pageSize": 1}),
        ("activate", {"use": "initial"}), ("health", {}),
    ]:
        add("cancel-" + action, action, cancelled=True, **values)
    put("cancel-put", 90, cancelled=True)
    put("cancel-admit", 91, "admit", cancelled=True)
    add("save", "sql", query="CREATE TABLE saved_executables AS SELECT * FROM decision_executables;")
    restore = """
        UPDATE decision_executables SET
            contract_digest=(SELECT s.contract_digest FROM saved_executables s WHERE s.executable_digest=decision_executables.executable_digest),
            executable_json=(SELECT s.executable_json FROM saved_executables s WHERE s.executable_digest=decision_executables.executable_digest),
            checked_executable_json=(SELECT s.checked_executable_json FROM saved_executables s WHERE s.executable_digest=decision_executables.executable_digest),
            provenance_json=(SELECT s.provenance_json FROM saved_executables s WHERE s.executable_digest=decision_executables.executable_digest),
            created_at=(SELECT s.created_at FROM saved_executables s WHERE s.executable_digest=decision_executables.executable_digest);
    """
    for name, assignment in [
        ("checked-null", "checked_executable_json=CAST('null' AS BLOB)"),
        ("checked-json", "checked_executable_json=X'7B'"),
        ("checked-base64", """checked_executable_json=CAST(json_set(CAST(checked_executable_json AS TEXT),'$.rules[0].predicate.checkedAst','!') AS BLOB)"""),
        ("executable-json", "executable_json=X'7B'"),
    ]:
        add("runtime-corrupt-" + name, "sql",
            query="UPDATE decision_executables SET " + assignment + " WHERE lifecycle_state='active';")
        add("runtime-read-" + name, "runtime", contract="c2", input='{"attributes":{"_random":0.5}}')
        add("runtime-restore-" + name, "sql", query=restore)
    for name, assignment in [
        ("contract", "contract_digest='" + ZERO + "'"),
        ("digest", """executable_json=CAST(replace(CAST(executable_json AS TEXT), '"value":2', '"value":22') AS BLOB)"""),
        ("json", "executable_json=X'7B'"),
        ("checked-null", "checked_executable_json=X'6E756C6C'"),
        ("checked-empty", """checked_executable_json=CAST('{"rules":[]}' AS BLOB)"""),
        ("checked-null-rule", """checked_executable_json=CAST('{"rules":[null]}' AS BLOB)"""),
        ("checked-duplicate-valid", """checked_executable_json=CAST('{"rules":[],' || substr(CAST(checked_executable_json AS TEXT),2) AS BLOB)"""),
        ("checked-duplicate-null", """checked_executable_json=CAST('{"rules":null,' || substr(CAST(checked_executable_json AS TEXT),2) AS BLOB)"""),
        ("checked-duplicate-predicate", """checked_executable_json=CAST(replace(CAST(checked_executable_json AS TEXT),'"predicate":{','"predicate":null,"predicate":{') AS BLOB)"""),
        ("checked-duplicate-ast", """checked_executable_json=CAST(replace(CAST(checked_executable_json AS TEXT),'"checkedAst":','"checkedAst":false,"checkedAst":') AS BLOB)"""),
        ("checked-base64", """checked_executable_json=CAST(json_set(CAST(checked_executable_json AS TEXT),'$.rules[0].predicate.checkedAst','!') AS BLOB)"""),
        ("checked-duplicate-base64", """checked_executable_json=CAST(replace(CAST(checked_executable_json AS TEXT),'"checkedAst":','"checkedAst":"!","checkedAst":') AS BLOB)"""),
        ("checked-base64-whitespace", """checked_executable_json=CAST(json_set(CAST(checked_executable_json AS TEXT),'$.rules[0].predicate.checkedAst','Y Q==') AS BLOB)"""),
        ("checked-base64-trailing-bits", """checked_executable_json=CAST(json_set(CAST(checked_executable_json AS TEXT),'$.rules[0].predicate.checkedAst','YR==') AS BLOB)"""),
        ("provenance", "provenance_json=X'7B'"),
        ("time", "created_at='bad'"),
    ]:
        # Select the former active executable for old-contract reads; restore exact bytes each time.
        add("corrupt-" + name, "sql", query=f"UPDATE decision_executables SET {assignment} WHERE lifecycle_state='inactive';")
        add("read-corrupt-" + name, "get", use="second")
        add("restore-" + name, "sql", query=restore)
    for name, encoded in [
        ("empty", ""), ("missing-padding", "YQ"), ("partial-padding", "YQ="),
        ("excess-padding", "YQ==="), ("interior-padding", "Y=Q="),
        ("unicode-space", "Y\u00a0Q=="), ("vertical-tab", "Y\vQ=="),
        ("ascii-whitespace", " \t\r\nYQ== "),
    ]:
        add("base64-" + name, "sql", query=(
            "UPDATE decision_executables SET checked_executable_json=CAST("
            "json_set(CAST(checked_executable_json AS TEXT),'$.rules[0].predicate.checkedAst',"
            f"'{encoded}') AS BLOB) WHERE lifecycle_state='inactive';"))
        add("read-base64-" + name, "get", use="second")
        add("restore-base64-" + name, "sql", query=restore)
    add("unsupported", "sql", query="UPDATE flaggo_schema_versions SET version=6 WHERE component='executable-store';")
    add("unsupported-health", "health")
    add("unsupported-initialize", "initialize")
    add("restore-schema", "sql", query="UPDATE flaggo_schema_versions SET version=5 WHERE component='executable-store';")
    add("bad-schema-type", "sql", query="UPDATE flaggo_schema_versions SET version='invalid' WHERE component='executable-store';")
    add("bad-schema-type-health", "health")
    add("restore-schema-type", "sql", query="UPDATE flaggo_schema_versions SET version=5 WHERE component='executable-store';")
    add("final-health", "health")
    return ops


def request(database, ops, expressions=True):
    return {"id": "executable-store", "storeAction": "executable", "database": str(database),
            "withExpressions": expressions, "now": NOW, "operations": ops}


def snapshot(path):
    with closing(sqlite3.connect(path)) as database:
        return {
            "executables": database.execute("SELECT * FROM decision_executables ORDER BY executable_digest").fetchall(),
            "admissions": database.execute("SELECT * FROM analysis_candidate_admissions ORDER BY workspace_id, cycle_id").fetchall(),
            "contracts": store_cases.snapshot(path),
        }


def operation_pairs(ops, left, right):
    if len(left) != len(ops) or len(right) != len(ops):
        raise ValueError("Executable-store reply count mismatch.")
    pairs = []
    for op, expected, observed in zip(ops, left, right):
        if expected.get("id") != op["id"] or observed.get("id") != op["id"]:
            raise ValueError("Executable-store reply IDs reordered/missing.")
        pairs.append((op["id"], expected, observed))
    return pairs


def validate_reference(replies):
    by_id = {reply["id"]: reply for reply in replies}
    for name, value in [("initial", "created"), ("duplicate", "existing")]:
        if by_id[name].get("value") != value:
            raise RuntimeError("Reference did not exercise immutable executable persistence.")
    for name, outcome in [("resolve-activate", 0), ("reject", 1), ("supersede-stale", 2),
                          ("resolve-stale-active", 3), ("resolve-again", 4)]:
        if by_id[name].get("value", {}).get("outcome") != outcome:
            raise RuntimeError("Reference did not exercise resolution outcome: " + name)
    if by_id["manual-activate"].get("value", {}).get("supersededCandidateCount") != 1:
        raise RuntimeError("Reference manual activation did not supersede admitted backlog.")
    if by_id["runtime-dynamic"].get("value", {}).get("result") != 7:
        raise RuntimeError("Reference did not evaluate a stored dynamic checked executable.")
    if by_id["current-preference"].get("value", {}).get("candidates", [{}])[0].get("admission", {}).get("cycleId") != "prefer-current":
        raise RuntimeError("Reference discovery did not prefer the current contract over newer stale candidates.")
    if by_id["read-corrupt-checked-duplicate-valid"].get("outcome") != "ok":
        raise RuntimeError("Reference did not accept valid repeated checked-DTO members.")
    for name in ("null", "predicate", "ast"):
        if by_id["read-corrupt-checked-duplicate-" + name].get("exceptionType") != "System.Text.Json.JsonException":
            raise RuntimeError("Reference did not validate an earlier repeated checked-DTO member.")


def verify(oracle, candidate):
    import run
    cases, reference, actual = [], {}, {}

    def compare(label, ops, left, right):
        for identifier, expected, observed in operation_pairs(ops, left, right):
            key = label + "-" + identifier
            cases.append({"id": key})
            reference[key] = {"status": "value", "value": expected}
            actual[key] = {"status": "value", "value": observed}

    def invoke(program, path, ops, expressions=True):
        return run.output(program, [request(path, ops, expressions)])["executable-store"]["value"]

    def seeded_operations(program):
        ops = operations()
        contracts = {op["id"]: op["document"] for op in ops if op["action"] == "accept"}
        compile_requests = [{
            "id": op["id"], "decisionAction": "compile", "contract": contracts[op["contract"]],
            "executable": op["document"], "input": '{"attributes":{"_random":0.5}}',
        } for op in ops if op["action"] in ("put", "admit") and "checked" not in op]
        compiled = run.output(program, compile_requests)
        for op in ops:
            if op["id"] in compiled:
                reply = compiled[op["id"]]
                if reply.get("value", {}).get("outcome") != "compiled" or "artifact" not in reply:
                    raise RuntimeError("Could not prepare checked store input: " + op["id"])
                # Byte-identical compiler input for both stores, not a normalization of read output.
                op["checked"] = reply["artifact"]["checkedExecutable"]
        return ops

    with tempfile.TemporaryDirectory(prefix="flaggo-executable-parity-") as directory:
        directory = Path(directory)
        for expressions, artifact_producer, ops in [
            (expressions, producer, seeded_operations(program))
            for expressions in (False, True)
            for producer, program in [("dotnet", oracle), ("rust", candidate)]
        ]:
            label = ("canonical" if expressions else "raw") + "-" + artifact_producer + "-artifacts"
            left, right = directory / (label + "-dotnet.db"), directory / (label + "-rust.db")
            expected, observed = invoke(oracle, left, ops, expressions), invoke(candidate, right, ops, expressions)
            validate_reference(expected)
            compare(label, ops, expected, observed)
            for producer, path in [("dotnet", left), ("rust", right)]:
                before = snapshot(path)
                reads = [{"id": "initialize", "action": "initialize"},
                         {"id": "health", "action": "health"},
                         {"id": "pending", "action": "pending", "pageSize": 1000}]
                reads.extend({"id": "read-" + str(index), "action": "get", "executableDigest": row[0]}
                             for index, row in enumerate(before["executables"]))
                compare(label + "-" + producer + "-restart", reads,
                        invoke(oracle, path, reads), invoke(candidate, path, reads))
                if snapshot(path) != before:
                    raise RuntimeError("Executable restart rewrote persisted state.")
                # Exercise lifecycle writes on both producer databases, not just cross-runtime reads.
                transition_files = [directory / (label + "-" + producer + suffix)
                                    for suffix in ("-transition-dotnet.db", "-transition-rust.db")]
                for destination in transition_files:
                    with closing(sqlite3.connect(path)) as source, closing(sqlite3.connect(destination)) as target:
                        source.backup(target)
                transitions = [
                    {"id": "discover", "action": "pending", "pageSize": 1000},
                    {"id": "activate", "action": "resolve", "snapshotFrom": "discover", "index": 0, "resolution": 0},
                    {"id": "activate-again", "action": "resolve", "snapshotFrom": "discover", "index": 0, "resolution": 0},
                    {"id": "reject", "action": "resolve", "snapshotFrom": "discover", "index": 1, "resolution": 1},
                    {"id": "pending-after", "action": "pending", "pageSize": 1000},
                ]
                transition_expected = invoke(oracle, transition_files[0], transitions)
                transition_actual = invoke(candidate, transition_files[1], transitions)
                if transition_expected[1].get("value", {}).get("outcome") != 0 or transition_expected[3].get("value", {}).get("outcome") != 1:
                    raise RuntimeError("Reference did not transition pre-existing producer admissions.")
                compare(label + "-" + producer + "-transition", transitions, transition_expected, transition_actual)
                if snapshot(transition_files[0]) != snapshot(transition_files[1]):
                    raise RuntimeError("Cross-runtime lifecycle transitions changed different persisted state.")
                # Convert to the actual v4 shape; migrated payload/provenance/lifecycle must be unchanged.
                with closing(sqlite3.connect(path)) as database:
                    database.executescript("""
                        ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_cutoff TEXT NOT NULL DEFAULT '2026-03-01T00:00:00.0000000+00:00';
                        ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_watermark INTEGER NOT NULL DEFAULT 42 CHECK(evidence_watermark > 0);
                        UPDATE flaggo_schema_versions SET version=4 WHERE component='executable-store';
                    """)
                migrated_by = oracle if producer == "rust" else candidate
                migration = invoke(migrated_by, path, reads)
                compare(label + "-" + producer + "-v4-migration", reads, invoke(oracle, path, reads), migration)
                after = snapshot(path)
                if before["executables"] != after["executables"] or before["admissions"] != after["admissions"]:
                    raise RuntimeError("v4 migration rewrote executable payloads, lifecycle, provenance or admission identities.")
                with closing(sqlite3.connect(path)) as database:
                    if database.execute("SELECT version FROM flaggo_schema_versions WHERE component='executable-store'").fetchone() != (5,):
                        raise RuntimeError("v4 migration did not install executable schema v5.")
                # A failed copy must roll back DDL, component version, and admissions.
                with closing(sqlite3.connect(path)) as database:
                    database.executescript("""
                        ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_cutoff TEXT;
                        ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_watermark INTEGER;
                        UPDATE flaggo_schema_versions SET version=4 WHERE component='executable-store';
                        UPDATE analysis_candidate_admissions SET executable_digest='missing' WHERE cycle_id='alpha-candidate';
                    """)
                failed_before = snapshot(path)
                failure = invoke(migrated_by, path, [{"id": "failed-migration", "action": "initialize"}])
                if failure[0].get("sqliteCode") != 19 or snapshot(path) != failed_before:
                    raise RuntimeError("Failed v4 migration did not report/roll back foreign-key failure.")
    report = run.compare(cases, reference, actual)
    report["storageProof"] = {"schemaVersion": 5, "migrationFrom": 4,
                              "checkedArtifactProducers": ["dotnet", "rust"],
                              "restartCombinationsPerArtifactProducer": 4, "migrationRollbackFixtures": 8,
                              "crossRuntimeTransitionFixtures": 8}
    return report
