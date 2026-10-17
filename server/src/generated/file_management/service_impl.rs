use super::{
    model::{DownloadObject, FileQuery, ImageQuery, StoredFile},
    service::FileService,
    uploads,
    util::{FileDomainError, validate_image_token},
};
use aio_plugin_file_model::{BeginUpload, DEFAULT_MAX_FILE_BYTES, FileItem, UploadSession};
use anyhow::{Context, Result, ensure};
use axum::body::Bytes;
use az_plugin_process::ObjectStorage;
use sqlx::{
    PgPool, Row,
    postgres::{PgPoolOptions, PgRow},
};

pub(super) const COLUMNS: &str = r#"id, original_name, content_type, size_bytes, sha256, uploaded_by,
    to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS created,
    image_token, state, expires_at <= now() AS expired"#;

/// 正式实现仅消费宿主受限数据库与对象客户端，不持有宿主文件系统路径。
pub struct FileServiceImpl {
    pub(super) pool: PgPool,
    pub(super) storage: ObjectStorage,
    pub(super) tenant: String,
    pub(super) public_base: String,
}
impl FileServiceImpl {
    /// 进程入口从宿主配置构建服务，所有业务操作仍检查租户绑定。
    pub fn new(
        database: &str,
        storage: ObjectStorage,
        tenant: String,
        public_base: String,
    ) -> Result<Self> {
        ensure!(!tenant.is_empty(), "文件租户不能为空");
        ensure!(
            public_base.starts_with("/api/runtime/components/public/")
                && !public_base.ends_with('/'),
            "文件公开路径绑定无效"
        );
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_lazy(database)?;
        Ok(Self {
            pool,
            storage,
            tenant,
            public_base,
        })
    }
    /// 宿主负责结构迁移；拒绝带管理权限的数据库和混入其他租户的数据。
    pub async fn verify_host_database(&self) -> Result<()> {
        let safe: bool=sqlx::query_scalar("SELECT NOT (rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls) AND current_schema() ~ '^p_[a-f0-9]+$' AND NOT has_schema_privilege(current_user, current_schema(), 'CREATE') FROM pg_roles WHERE rolname=current_user").fetch_one(&self.pool).await?;
        ensure!(safe, "文件进程必须使用宿主发放的受限数据库角色");
        let other: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_objects WHERE tenant_id<>$1 UNION ALL SELECT 1 FROM file_chunks WHERE tenant_id<>$1)").bind(&self.tenant).fetch_one(&self.pool).await.context("宿主文件结构未就绪")?;
        ensure!(!other, "文件数据库包含其他租户数据");
        Ok(())
    }
    /// 停止前须先取消并等待清理任务，关闭不删除业务数据。
    pub async fn shutdown(&self) {
        self.pool.close().await;
    }
    pub(super) fn check_tenant(&self, tenant: &str) -> Result<()> {
        ensure!(tenant == self.tenant, "文件租户绑定不匹配");
        Ok(())
    }
    pub(super) fn stored(&self, row: PgRow) -> Result<StoredFile> {
        let image_token: Option<String> = row.try_get("image_token")?;
        let public_path = image_token
            .as_ref()
            .map(|token| format!("{}/i/{token}", self.public_base));
        Ok(StoredFile {
            item: FileItem {
                id: row.try_get("id")?,
                name: row.try_get("original_name")?,
                content_type: row.try_get("content_type")?,
                size_bytes: u64::try_from(row.try_get::<i64, _>("size_bytes")?)?,
                sha256: row
                    .try_get::<Option<String>, _>("sha256")?
                    .unwrap_or_default(),
                uploaded_by: row.try_get("uploaded_by")?,
                created_at: row.try_get("created")?,
                image_token,
                public_path,
            },
            state: row.try_get("state")?,
            expired: row.try_get("expired")?,
        })
    }
    pub(super) async fn find(&self, query: &FileQuery) -> Result<Option<StoredFile>> {
        self.check_tenant(&query.tenant_id)?;
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM file_objects WHERE tenant_id=$1 AND id=$2"
        ))
        .bind(&self.tenant)
        .bind(&query.file_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| self.stored(row)).transpose()
    }
    pub(super) fn upload_owner(&self, stored: &StoredFile, user: &str) -> Result<()> {
        if stored.item.uploaded_by != user || stored.state == "deleting" {
            return Err(FileDomainError::NotFound.into());
        }
        if stored.state == "uploading" && stored.expired {
            return Err(FileDomainError::Conflict("上传已过期".into()).into());
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl FileService for FileServiceImpl {
    async fn list(&self, tenant: &str) -> Result<Vec<FileItem>> {
        self.check_tenant(tenant)?;
        let rows=sqlx::query(&format!("SELECT {COLUMNS} FROM file_objects WHERE tenant_id=$1 AND state='ready' ORDER BY created_at DESC,id"))
            .bind(tenant).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| self.stored(row).map(|record| record.item))
            .collect()
    }
    async fn begin(&self, tenant: &str, user: &str, request: BeginUpload) -> Result<UploadSession> {
        uploads::begin(self, tenant, user, request).await
    }
    async fn upload_status(&self, query: FileQuery, user: &str) -> Result<UploadSession> {
        uploads::status(self, query, user).await
    }
    async fn put_chunk(&self, query: FileQuery, user: &str, index: u32, body: Bytes) -> Result<()> {
        uploads::put(self, query, user, index, body).await
    }
    async fn complete(&self, query: FileQuery, user: &str) -> Result<FileItem> {
        uploads::complete(self, query, user).await
    }
    async fn cancel_upload(&self, query: FileQuery, user: &str) -> Result<bool> {
        self.tombstone(query, Some(user)).await
    }
    async fn delete(&self, query: FileQuery) -> Result<bool> {
        self.tombstone(query, None).await
    }
    async fn download(&self, query: FileQuery) -> Result<Option<DownloadObject>> {
        let Some(stored) = self
            .find(&query)
            .await?
            .filter(|file| file.state == "ready")
        else {
            return Ok(None);
        };
        self.stream(stored).await.map(Some)
    }
    async fn open_image(&self, query: ImageQuery) -> Result<Option<DownloadObject>> {
        self.check_tenant(&query.tenant_id)?;
        let token = validate_image_token(&query.token)?;
        let row=sqlx::query(&format!("SELECT {COLUMNS} FROM file_objects WHERE tenant_id=$1 AND image_token=$2 AND state='ready'"))
            .bind(&self.tenant).bind(token).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        self.stream(self.stored(row)?).await.map(Some)
    }
    fn max_file_bytes(&self) -> u64 {
        DEFAULT_MAX_FILE_BYTES
    }
}
