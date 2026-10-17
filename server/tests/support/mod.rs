use aio_plugin_file_server::{FileServiceImpl, PUBLIC_IMAGE_TYPES, hosting};
use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use az_plugin_process::Host;
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// 只替换 broker 的底层对象源，HTTP、Unix SDK、可信入口与 PostgreSQL 都是真实实现。
#[derive(Default)]
pub struct Objects {
    pub values: Mutex<HashMap<String, Vec<u8>>>,
    pub fail_write_reply: AtomicBool,
    pub fail_remove: AtomicBool,
    pub hold_write: AtomicBool,
    pub writing: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}
#[derive(Deserialize)]
struct Key {
    key: String,
}
fn authenticated(headers: &HeaderMap) -> bool {
    headers
        .get("x-aio-token")
        .is_some_and(|value| value == "file-test-ingress-token-32-characters")
}
async fn read(
    State(objects): State<Arc<Objects>>,
    Query(key): Query<Key>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match objects.values.lock() {
        Ok(values) => match values.get(&key.key) {
            Some(value) => value.clone().into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
async fn write(
    State(objects): State<Arc<Objects>>,
    Query(key): Query<Key>,
    headers: HeaderMap,
    bytes: Bytes,
) -> StatusCode {
    if !authenticated(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    {
        let Ok(mut values) = objects.values.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR;
        };
        values.insert(key.key, bytes.to_vec());
    }
    if objects.hold_write.swap(false, Ordering::SeqCst) {
        objects.writing.notify_one();
        objects.release.notified().await;
    }
    if objects.fail_write_reply.swap(false, Ordering::SeqCst) {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::NO_CONTENT
    }
}
async fn remove(
    State(objects): State<Arc<Objects>>,
    Query(key): Query<Key>,
    headers: HeaderMap,
) -> StatusCode {
    if !authenticated(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    if objects.fail_remove.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let Ok(mut values) = objects.values.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    values.remove(&key.key);
    StatusCode::NO_CONTENT
}

pub struct Fixture {
    pub service: Arc<FileServiceImpl>,
    pub objects: Arc<Objects>,
    pub admin: PgPool,
    pub schema: String,
    pub client: reqwest::Client,
    pub origin: String,
    pub host: Host,
    pub database: String,
    pub public_base: String,
    pub role: String,
    pub source: String,
    temporary: tempfile::TempDir,
    servers: Vec<tokio::task::JoinHandle<()>>,
}
impl Fixture {
    pub async fn new() -> Result<Self> {
        let database =
            std::env::var("AIO_FILE_TEST_DATABASE_URL").context("缺少独立文件验收数据库")?;
        let admin = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database)
            .await?;
        let name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&admin)
            .await?;
        ensure!(name == "aio_file_test", "仅允许独立 aio_file_test 数据库");
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let schema = format!("p_{suffix}");
        let role = format!("r_{suffix}");
        let password = uuid::Uuid::new_v4().simple().to_string();
        let mut tx = admin.begin().await?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!("CREATE ROLE {role} LOGIN PASSWORD '{password}'"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!("SET LOCAL search_path TO {schema}"))
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(include_str!("../../migrations/001_files.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!("GRANT USAGE ON SCHEMA {schema} TO {role}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA {schema} TO {role}"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::query(&format!("ALTER ROLE {role} SET search_path TO {schema}"))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let mut restricted = reqwest::Url::parse(&database)?;
        restricted
            .set_username(&role)
            .map_err(|_| anyhow::anyhow!("数据库用户名无效"))?;
        restricted
            .set_password(Some(&password))
            .map_err(|_| anyhow::anyhow!("数据库密码无效"))?;
        let database = restricted.to_string();
        let temporary = tempfile::tempdir()?;
        let broker = temporary.path().join("broker.sock");
        let listener = tokio::net::UnixListener::bind(&broker)?;
        let objects = Arc::new(Objects::default());
        let router = Router::new()
            .route("/storage", get(read).put(write).delete(remove))
            .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
            .with_state(objects.clone());
        let broker_task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let source = uuid::Uuid::new_v4().to_string();
        let public_base = format!("/api/runtime/components/public/{source}/tenant-a");
        let config = json!({"abi_version":2,"tenant_id":"tenant-a","database_url":database,"encryption_key":null,
            "storage":true,"public_base":public_base,"public_routes":[{"path":"/i/{token}","content_types":PUBLIC_IMAGE_TYPES,"max_bytes":104857600}],
            "ingress_token":"file-test-ingress-token-32-characters","broker_socket":broker,"endpoints":[],"services":[]});
        let host = Host::from_bytes(
            &serde_json::to_vec(&config)?,
            temporary.path().join("plugin.sock"),
        )?;
        let service = Arc::new(FileServiceImpl::new(
            &database,
            host.storage()?,
            "tenant-a".into(),
            public_base.clone(),
        )?);
        service.verify_host_database().await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let router = hosting::router(&host, service.clone())?;
        let http_task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            service,
            objects,
            admin,
            schema,
            client: reqwest::Client::builder().no_proxy().build()?,
            origin,
            host,
            database,
            public_base,
            role,
            source,
            temporary,
            servers: vec![broker_task, http_task],
        })
    }
    pub fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        user: &str,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.origin))
            .header("x-aio-token", "file-test-ingress-token-32-characters")
            .header("x-aio-tenant-id", "tenant-a")
            .header("x-aio-user-id", user)
            .header("x-aio-context", "file-context")
    }
    pub fn public(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("{}{path}", self.origin))
            .header("x-aio-token", "file-test-ingress-token-32-characters")
            .header("x-aio-tenant-id", "tenant-a")
            .header("x-aio-public", "1")
    }
    pub fn reopened(&self) -> Result<FileServiceImpl> {
        FileServiceImpl::new(
            &self.database,
            self.host.storage()?,
            "tenant-a".into(),
            self.public_base.clone(),
        )
    }
    pub async fn ready_cleanup(&self, id: &str) -> Result<()> {
        sqlx::query(&format!("UPDATE {}.file_objects SET cleanup_after=now()-interval '1 second' WHERE id=$1 AND state='deleting'",self.schema)).bind(id).execute(&self.admin).await?;
        Ok(())
    }
    pub async fn close(self) -> Result<()> {
        for server in &self.servers {
            server.abort();
        }
        self.service.shutdown().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await?;
        sqlx::query(&format!("DROP ROLE {}", self.role))
            .execute(&self.admin)
            .await?;
        self.admin.close().await;
        self.temporary.close()?;
        Ok(())
    }
}
