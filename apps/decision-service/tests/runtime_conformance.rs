use flaggo_contract_store::SqliteContractVersionStore;
use flaggo_core_conformance::runtime::{RuntimeFixture, c02_exact_runtime};
use flaggo_decision::{ActiveExecutable, ActiveExecutableReader, Cancellation, StoreReadError};
use flaggo_executable_store::{ContractAuthority, SqliteContractAuthority, SqliteExecutableStore};
use flaggo_expression::ExpressionCompiler;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Reader {
    store: SqliteExecutableStore,
    unavailable: Arc<AtomicBool>,
}

impl ActiveExecutableReader for Reader {
    async fn get_active(
        &self,
        digest: &str,
        cancellation: Cancellation,
    ) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(StoreReadError::Dependency(
                std::io::Error::other("injected authority outage").into(),
            ));
        }
        ActiveExecutableReader::get_active(&self.store, digest, cancellation).await
    }
}

struct Fixture {
    authority: Arc<SqliteContractAuthority>,
    unavailable: Arc<AtomicBool>,
    pool: sqlx::SqlitePool,
}

#[async_trait::async_trait]
impl RuntimeFixture for Fixture {
    type Contracts = SqliteContractVersionStore;
    type Executables = Reader;
    fn authority(&self) -> Arc<dyn ContractAuthority> {
        self.authority.clone()
    }
    fn readers(&self) -> (Self::Contracts, Self::Executables) {
        (
            self.authority.contracts().clone(),
            Reader {
                store: self.authority.executables().clone(),
                unavailable: self.unavailable.clone(),
            },
        )
    }
    fn unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::SeqCst);
    }
    async fn corrupt_active(&self, digest: &str) {
        sqlx::query("UPDATE decision_executables SET checked_executable_json = ? WHERE contract_digest = ? AND lifecycle_state = 'active'")
            .bind(b"null".as_slice()).bind(digest).execute(&self.pool).await.unwrap();
    }
}

#[tokio::test]
async fn sqlite_runtime_conforms_with_cached_and_fresh_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.db");
    let authority = Arc::new(
        SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
            .await
            .unwrap(),
    );
    authority.initialize().await.unwrap();
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(path))
            .await
            .unwrap();
    let fixture = Fixture {
        authority,
        pool,
        unavailable: Arc::new(AtomicBool::new(false)),
    };
    c02_exact_runtime(&fixture).await;
    fixture.authority.close().await;
    fixture.pool.close().await;
}
