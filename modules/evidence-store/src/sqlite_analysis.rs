use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    str::FromStr,
};

use async_trait::async_trait;
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlparser::{
    ast::{Expr, ObjectName, Statement, Visit, Visitor},
    dialect::SQLiteDialect,
    parser::Parser,
};
use sqlx::{
    Column, QueryBuilder, Row, Sqlite, SqlitePool, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow, SqliteSynchronous},
};

use crate::{
    EvidenceAnalysisStore, EvidenceQueryCapabilities, EvidenceQueryRequest, EvidenceQueryResult,
    EvidenceQueryScope, EvidenceSourceSelector, EvidenceStoreError, ObservationWatermark,
};

const COMPONENT_NAME: &str = "evidence-store";
const SCHEMA_VERSION: i64 = 4;
const LOGICAL_VIEW: &str = "observations";
const SAFE_FUNCTIONS: &[&str] = &[
    "abs",
    "avg",
    "coalesce",
    "count",
    "json_extract",
    "json_type",
    "lower",
    "max",
    "min",
    "nullif",
    "round",
    "sum",
    "total",
    "upper",
];
pub const SQLITE_EVIDENCE_QUERY_CAPABILITIES: EvidenceQueryCapabilities =
    EvidenceQueryCapabilities {
        dialect: "sqlite",
        statements: &["SELECT", "WITH (non-recursive)"],
        relations: &["observations", "named CTEs"],
        features: &[
            "aggregation",
            "arithmetic",
            "CASE expressions",
            "grouping",
            "JSON extraction",
            "ordering",
        ],
        functions: SAFE_FUNCTIONS,
    };

#[derive(Clone)]
pub struct SqliteEvidenceAnalysisStore {
    pool: SqlitePool,
}

impl SqliteEvidenceAnalysisStore {
    pub async fn connect(database_url: &str) -> Result<Self, EvidenceStoreError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(EvidenceStoreError::unavailable)?
            .create_if_missing(false)
            .read_only(true)
            .foreign_keys(true)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("PRAGMA query_only = ON")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        let found: Option<i64> =
            sqlx::query_scalar("SELECT version FROM flaggo_schema_versions WHERE component = ?")
                .bind(COMPONENT_NAME)
                .fetch_optional(&pool)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        if found != Some(SCHEMA_VERSION) {
            pool.close().await;
            return Err(EvidenceStoreError::UnsupportedSchemaVersion {
                component: COMPONENT_NAME,
                found: found.unwrap_or_default(),
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[async_trait]
impl EvidenceAnalysisStore for SqliteEvidenceAnalysisStore {
    fn query_capabilities(&self) -> EvidenceQueryCapabilities {
        SQLITE_EVIDENCE_QUERY_CAPABILITIES
    }

    async fn capture_watermark(&self) -> Result<ObservationWatermark, EvidenceStoreError> {
        let value: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(observation_sequence), 0) FROM evidence_observations",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        Ok(ObservationWatermark::new(u64::try_from(value).map_err(
            |_| EvidenceStoreError::CorruptData("negative observation sequence".to_owned()),
        )?))
    }

    async fn query(
        &self,
        scope: &EvidenceQueryScope,
        request: EvidenceQueryRequest,
    ) -> Result<EvidenceQueryResult, EvidenceStoreError> {
        validate_scope(scope)?;
        if request.limits.max_bytes == 0 {
            return Err(EvidenceStoreError::InvalidQuery(
                "max_bytes must be greater than zero".to_owned(),
            ));
        }
        let normalized_sql = validate_sql(&request.sql)?;
        let cutoff = i64::try_from(request.cutoff_unix_nano).map_err(|_| {
            EvidenceStoreError::InvalidQuery("cutoff exceeds SQLite integer range".to_owned())
        })?;
        let watermark = i64::try_from(request.watermark.get()).map_err(|_| {
            EvidenceStoreError::InvalidQuery("watermark exceeds SQLite integer range".to_owned())
        })?;
        let mut builder = QueryBuilder::<Sqlite>::new(
            "WITH observations AS (
                SELECT observation_sequence,
                       observation_id,
                       logical_source_id,
                       content_digest,
                       signal_type,
                       instrumentation_scope,
                       signal_name,
                       metric_kind,
                       metric_unit,
                       parent_span_name,
                       CAST(observed_at_unix_nano AS INTEGER) AS observed_at_unix_nano,
                       observed_time_source,
                       protocol_kind,
                       decision_id,
                       contract_digest,
                       CAST(payload_json AS TEXT) AS payload_json
                FROM evidence_observations
                WHERE tenant = ",
        );
        builder
            .push_bind(&scope.authority.tenant)
            .push(" AND application = ")
            .push_bind(&scope.authority.application)
            .push(" AND environment = ")
            .push_bind(&scope.authority.environment)
            .push(" AND (contract_digest IS NULL OR contract_digest = ")
            .push_bind(&scope.contract_digest)
            .push(") AND CAST(observed_at_unix_nano AS INTEGER) <= ")
            .push_bind(cutoff)
            .push(" AND observation_sequence <= ")
            .push_bind(watermark)
            .push(" AND (");
        for (index, source) in scope.sources.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            push_source(&mut builder, source);
        }
        builder
            .push(")) SELECT * FROM (")
            .push(&normalized_sql)
            .push(") AS analysis_result LIMIT ")
            .push_bind(i64::from(request.limits.max_rows.get()) + 1);

        let query = builder.build();
        let future = async {
            let mut stream = query.fetch(&self.pool);
            let mut columns = Vec::new();
            let mut rows = Vec::new();
            let mut observation_ids = BTreeSet::new();
            let mut result_bytes = 0usize;
            while let Some(row) = stream
                .try_next()
                .await
                .map_err(EvidenceStoreError::unavailable)?
            {
                if rows.len() >= request.limits.max_rows.get() as usize {
                    return Err(EvidenceStoreError::QueryLimit("row"));
                }
                if columns.is_empty() {
                    columns = row
                        .columns()
                        .iter()
                        .map(|column| column.name().to_owned())
                        .collect();
                }
                let decoded = decode_row(&row)?;
                if let Some(Value::String(observation_id)) = decoded.get("observation_id") {
                    observation_ids.insert(observation_id.clone());
                }
                result_bytes = result_bytes
                    .checked_add(
                        serde_json::to_vec(&decoded)
                            .expect("JSON values must serialize")
                            .len(),
                    )
                    .ok_or(EvidenceStoreError::QueryLimit("byte"))?;
                if result_bytes > request.limits.max_bytes {
                    return Err(EvidenceStoreError::QueryLimit("byte"));
                }
                rows.push(decoded);
            }
            Ok(EvidenceQueryResult {
                normalized_sql: normalized_sql.clone(),
                columns,
                rows,
                observation_ids: observation_ids.into_iter().collect(),
                result_bytes,
            })
        };
        tokio::time::timeout(request.limits.timeout, future)
            .await
            .map_err(|_| EvidenceStoreError::QueryLimit("time"))?
    }
}

fn validate_scope(scope: &EvidenceQueryScope) -> Result<(), EvidenceStoreError> {
    if scope.sources.is_empty() {
        return Err(EvidenceStoreError::InvalidQuery(
            "at least one contract evidence source is required".to_owned(),
        ));
    }
    if scope.contract_digest.len() != 71
        || !scope.contract_digest.starts_with("sha256:")
        || !scope.contract_digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(EvidenceStoreError::InvalidQuery(
            "contract digest must be a lowercase sha256 digest".to_owned(),
        ));
    }
    Ok(())
}

fn validate_sql(sql: &str) -> Result<String, EvidenceStoreError> {
    if sql.trim().is_empty() {
        return Err(EvidenceStoreError::InvalidQuery(
            "SQL must not be empty".to_owned(),
        ));
    }
    let statements = Parser::parse_sql(&SQLiteDialect {}, sql)
        .map_err(|error| EvidenceStoreError::InvalidQuery(error.to_string()))?;
    let [statement] = statements.as_slice() else {
        return Err(EvidenceStoreError::InvalidQuery(
            "exactly one query is required".to_owned(),
        ));
    };
    let Statement::Query(query) = statement else {
        return Err(EvidenceStoreError::InvalidQuery(
            "only SELECT or WITH queries are allowed".to_owned(),
        ));
    };
    let mut allowed_relations = BTreeSet::from([LOGICAL_VIEW.to_owned()]);
    if let Some(with) = &query.with {
        if with.recursive {
            return Err(EvidenceStoreError::InvalidQuery(
                "recursive common table expressions are not allowed".to_owned(),
            ));
        }
        for cte in &with.cte_tables {
            let name = cte.alias.name.value.to_ascii_lowercase();
            if name == LOGICAL_VIEW || !allowed_relations.insert(name) {
                return Err(EvidenceStoreError::InvalidQuery(
                    "common table expression names must be unique and cannot shadow observations"
                        .to_owned(),
                ));
            }
        }
    }
    let mut validator = ReadOnlyVisitor { allowed_relations };
    if let ControlFlow::Break(message) = statement.visit(&mut validator) {
        return Err(EvidenceStoreError::InvalidQuery(message));
    }
    Ok(statement.to_string())
}

struct ReadOnlyVisitor {
    allowed_relations: BTreeSet<String>,
}

impl Visitor for ReadOnlyVisitor {
    type Break = String;

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        let name = relation.to_string().to_ascii_lowercase();
        if self.allowed_relations.contains(&name) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(format!(
                "relation '{name}' is not available; query the observations view"
            ))
        }
    }

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(function) = expression {
            let name = function.name.to_string().to_ascii_lowercase();
            if !SAFE_FUNCTIONS.contains(&name.as_str()) {
                return ControlFlow::Break(format!("function '{name}' is not allowed"));
            }
        }
        ControlFlow::Continue(())
    }
}

fn push_source(builder: &mut QueryBuilder<Sqlite>, source: &EvidenceSourceSelector) {
    builder
        .push("(signal_type = ")
        .push_bind(source.signal.as_str())
        .push(" AND instrumentation_scope = ")
        .push_bind(&source.instrumentation_scope)
        .push(" AND signal_name = ")
        .push_bind(&source.signal_name);
    push_optional(builder, "metric_kind", source.metric_kind.as_deref());
    push_optional(builder, "metric_unit", source.metric_unit.as_deref());
    push_optional(
        builder,
        "parent_span_name",
        source.parent_span_name.as_deref(),
    );
    builder.push(")");
}

fn push_optional(builder: &mut QueryBuilder<Sqlite>, column: &str, value: Option<&str>) {
    builder.push(" AND ").push(column);
    if let Some(value) = value {
        builder.push(" = ").push_bind(value);
    } else {
        builder.push(" IS NULL");
    }
}

fn decode_row(row: &SqliteRow) -> Result<BTreeMap<String, Value>, EvidenceStoreError> {
    let mut decoded = BTreeMap::new();
    for (index, column) in row.columns().iter().enumerate() {
        let raw = row
            .try_get_raw(index)
            .map_err(EvidenceStoreError::unavailable)?;
        let value = if raw.is_null() {
            Value::Null
        } else {
            match raw.type_info().name() {
                "INTEGER" => Value::from(
                    row.try_get::<i64, _>(index)
                        .map_err(EvidenceStoreError::unavailable)?,
                ),
                "REAL" => Value::from(
                    row.try_get::<f64, _>(index)
                        .map_err(EvidenceStoreError::unavailable)?,
                ),
                "TEXT" => Value::from(
                    row.try_get::<String, _>(index)
                        .map_err(EvidenceStoreError::unavailable)?,
                ),
                "BLOB" => {
                    return Err(EvidenceStoreError::InvalidQuery(
                        "binary query results are not supported; select text or numeric values"
                            .to_owned(),
                    ));
                }
                data_type => {
                    return Err(EvidenceStoreError::CorruptData(format!(
                        "unsupported SQLite result type '{data_type}'"
                    )));
                }
            }
        };
        decoded.insert(column.name().to_owned(), value);
    }
    Ok(decoded)
}
