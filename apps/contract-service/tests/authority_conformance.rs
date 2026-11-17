use std::sync::Arc;

use flaggo_core_conformance::{
    AuthorityFixture, c03_atomic_failure, c03_publication, c04_admission, c05_activation,
};
use flaggo_executable_store::{ContractAuthority, SqliteContractAuthority};
use flaggo_expression::ExpressionCompiler;

struct Fixture {
    _directory: tempfile::TempDir,
    first: Arc<SqliteContractAuthority>,
    second: Arc<SqliteContractAuthority>,
    pool: sqlx::SqlitePool,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("authority.db");
        let first = Arc::new(
            SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
                .await
                .unwrap(),
        );
        first.initialize().await.unwrap();
        let second = Arc::new(
            SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
                .await
                .unwrap(),
        );
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(path),
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            first,
            second,
            pool,
        }
    }

    async fn close(self) {
        self.first.close().await;
        self.second.close().await;
        self.pool.close().await;
    }
}

#[async_trait::async_trait]
impl AuthorityFixture for Fixture {
    fn client(&self) -> Arc<dyn ContractAuthority> {
        self.first.clone()
    }
    fn independent_client(&self) -> Arc<dyn ContractAuthority> {
        self.second.clone()
    }
    async fn fail_publication(&self, enabled: bool) {
        sqlx::raw_sql(if enabled {
            "CREATE TRIGGER fail_publication BEFORE INSERT ON decision_contract_current
             BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;"
        } else {
            "DROP TRIGGER fail_publication;"
        })
        .execute(&self.pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn publication_conforms() {
    let fixture = Fixture::new().await;
    c03_publication(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn publication_rolls_back_all_authority() {
    let fixture = Fixture::new().await;
    c03_atomic_failure(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn admission_conforms_across_independent_clients() {
    let fixture = Fixture::new().await;
    c04_admission(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn activation_conforms_across_independent_clients() {
    let fixture = Fixture::new().await;
    c05_activation(&fixture).await;
    fixture.close().await;
}
