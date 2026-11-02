pub const SCHEMA_VERSION: i64 = 5;

pub(crate) const SCHEMA: &str = "
INSERT INTO flaggo_schema_versions(component, version) VALUES ('executable-store', 5)
ON CONFLICT(component) DO UPDATE SET version = excluded.version;
CREATE TABLE IF NOT EXISTS decision_executables (
    executable_digest TEXT PRIMARY KEY,
    contract_digest TEXT NOT NULL,
    executable_json BLOB NOT NULL,
    checked_executable_json BLOB NOT NULL,
    provenance_json BLOB NULL,
    lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN ('candidate', 'active', 'inactive')),
    state_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    activated_at TEXT NULL
);
CREATE INDEX IF NOT EXISTS ix_executables_contract ON decision_executables(contract_digest);
CREATE UNIQUE INDEX IF NOT EXISTS ux_executables_one_active
ON decision_executables(contract_digest) WHERE lifecycle_state = 'active';
CREATE INDEX IF NOT EXISTS ix_executables_analysis_candidates
ON decision_executables(executable_digest, state_version) WHERE lifecycle_state = 'candidate';
CREATE TABLE IF NOT EXISTS analysis_candidate_admissions (
    workspace_id TEXT NOT NULL, cycle_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
    contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL, executable_digest TEXT NOT NULL,
    analysis_manifest_digest TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY(workspace_id, cycle_id),
    FOREIGN KEY(executable_digest) REFERENCES decision_executables(executable_digest)
);
CREATE INDEX IF NOT EXISTS ix_analysis_candidates_contract
ON analysis_candidate_admissions(contract_name, contract_digest, created_at);
CREATE INDEX IF NOT EXISTS ix_analysis_candidates_executable
ON analysis_candidate_admissions(executable_digest, contract_name, contract_digest, created_at DESC);
";

pub(crate) const MIGRATE_FOUR: &str = "
CREATE TABLE analysis_candidate_admissions_v5 (
    workspace_id TEXT NOT NULL, cycle_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
    contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL, executable_digest TEXT NOT NULL,
    analysis_manifest_digest TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY(workspace_id, cycle_id),
    FOREIGN KEY(executable_digest) REFERENCES decision_executables(executable_digest)
);
INSERT INTO analysis_candidate_admissions_v5 (
    workspace_id, cycle_id, attempt_id, contract_name, contract_digest,
    executable_digest, analysis_manifest_digest, created_at
)
SELECT workspace_id, cycle_id, attempt_id, contract_name, contract_digest,
       executable_digest, analysis_manifest_digest, created_at FROM analysis_candidate_admissions;
DROP TABLE analysis_candidate_admissions;
ALTER TABLE analysis_candidate_admissions_v5 RENAME TO analysis_candidate_admissions;
";
