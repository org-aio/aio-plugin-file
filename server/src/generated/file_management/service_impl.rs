use std::{env, io::ErrorKind, path::PathBuf};

use aio_plugin_file_model::FileItem;
use anyhow::{Context as _, Result, ensure};
use axum::body::Bytes;
use sha2::{Digest as _, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt as _,
    sync::OnceCell,
};
use uuid::Uuid;

use super::{
    model::{DownloadObject, FileQuery, ImageQuery, StoredFile, UploadCommand},
    service::FileService,
    util::{
        FileStorageConfig, is_image_content_type, new_image_token, storage_name, tenant_bucket,
        validate_content_type, validate_filename, validate_image_token, validation,
    },
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS file_objects (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    original_name TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL,
    storage_name TEXT NOT NULL,
    uploaded_by TEXT NOT NULL,
    image_token TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, storage_name)
);
ALTER TABLE file_objects ADD COLUMN IF NOT EXISTS image_token TEXT;
CREATE INDEX IF NOT EXISTS file_objects_tenant_created_idx
    ON file_objects (tenant_id, created_at DESC, id);
CREATE UNIQUE INDEX IF NOT EXISTS file_objects_image_token_idx
    ON file_objects (image_token) WHERE image_token IS NOT NULL;
"#;

type StoredRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
);

/// 图床列表与详情共用的列顺序。
const STORED_COLUMNS: &str = r#"tenant_id, id, original_name, content_type, size_bytes, sha256, uploaded_by,
                      to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
                      image_token, storage_name"#;

#[derive(Debug)]
pub(super) struct FileServiceImpl {
    pool: PgPool,
    config: FileStorageConfig,
    initialized: OnceCell<()>,
}

impl FileServiceImpl {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("AIO_DATABASE_URL")
            .or_else(|_| env::var("AZ_AIO_DATABASE_URL"))
            .context("文件插件缺少 AIO_DATABASE_URL")?;
        Ok(Self {
            pool: PgPoolOptions::new()
                .max_connections(8)
                .connect_lazy(&database_url)
                .context("创建文件数据库连接池失败")?,
            config: FileStorageConfig::from_env()?,
            initialized: OnceCell::new(),
        })
    }

    async fn ensure_initialized(&self) -> Result<()> {
        self.initialized
            .get_or_try_init(|| async {
                fs::create_dir_all(&self.config.root)
                    .await
                    .with_context(|| {
                        format!("创建文件存储目录失败: {}", self.config.root.display())
                    })?;
                let metadata = fs::symlink_metadata(&self.config.root)
                    .await
                    .context("读取文件存储目录失败")?;
                ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "文件存储根路径必须是真实目录"
                );
                sqlx::raw_sql(SCHEMA)
                    .execute(&self.pool)
                    .await
                    .context("创建文件插件数据表失败")?;
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        Ok(())
    }

    async fn tenant_directory(&self, tenant_id: &str) -> Result<PathBuf> {
        let path = self.config.root.join(tenant_bucket(tenant_id));
        match fs::create_dir(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("创建租户文件目录失败: {}", path.display()));
            }
        }
        let metadata = fs::symlink_metadata(&path)
            .await
            .context("读取租户文件目录失败")?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "租户文件目录必须是真实目录"
        );
        Ok(path)
    }

    async fn find(&self, tenant_id: &str, file_id: &str) -> Result<Option<StoredFile>> {
        let sql =
            format!("SELECT {STORED_COLUMNS} FROM file_objects WHERE tenant_id = $1 AND id = $2");
        let row = sqlx::query_as::<_, StoredRow>(&sql)
            .bind(tenant_id)
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await
            .context("读取文件元数据失败")?;
        row.map(stored_from_row).transpose()
    }

    async fn find_by_image_token(&self, token: &str) -> Result<Option<StoredFile>> {
        let sql = format!("SELECT {STORED_COLUMNS} FROM file_objects WHERE image_token = $1");
        let row = sqlx::query_as::<_, StoredRow>(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await
            .context("读取图床图片元数据失败")?;
        row.map(stored_from_row).transpose()
    }

    async fn write_content(&self, tenant_id: &str, file_id: &str, body: &[u8]) -> Result<PathBuf> {
        let directory = self.tenant_directory(tenant_id).await?;
        let name = storage_name(file_id)?;
        let target = directory.join(&name);
        let temporary = directory.join(format!(".{file_id}.upload"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await
            .context("创建上传临时文件失败")?;
        let write_result = async {
            file.write_all(body).await.context("写入上传内容失败")?;
            file.sync_all().await.context("同步上传内容失败")?;
            drop(file);
            fs::rename(&temporary, &target)
                .await
                .context("提交上传内容失败")?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary).await;
            return Err(error);
        }
        Ok(target)
    }

    fn path_for(&self, tenant_id: &str, file_id: &str, persisted_name: &str) -> Result<PathBuf> {
        let expected = storage_name(file_id)?;
        ensure!(
            persisted_name == expected,
            "文件元数据中的存储名称不符合约定"
        );
        Ok(self
            .config
            .root
            .join(tenant_bucket(tenant_id))
            .join(expected))
    }

    #[cfg(test)]
    fn for_test(pool: PgPool, config: FileStorageConfig) -> Self {
        Self {
            pool,
            config,
            initialized: OnceCell::new(),
        }
    }
}

#[async_trait::async_trait]
impl FileService for FileServiceImpl {
    async fn initialize(&self) -> Result<()> {
        self.ensure_initialized().await
    }

    async fn list(&self, tenant_id: &str) -> Result<Vec<FileItem>> {
        self.ensure_initialized().await?;
        let sql = format!(
            "SELECT {STORED_COLUMNS} FROM file_objects
             WHERE tenant_id = $1
             ORDER BY created_at DESC, id"
        );
        let rows = sqlx::query_as::<_, StoredRow>(&sql)
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await
            .context("读取文件列表失败")?;
        rows.into_iter()
            .map(stored_from_row)
            .map(|stored| stored.map(|stored| stored.item))
            .collect()
    }

    async fn upload(&self, command: UploadCommand) -> Result<FileItem> {
        self.ensure_initialized().await?;
        if command.body.len() > self.config.max_bytes {
            return validation(format!("文件不能超过 {} 字节", self.config.max_bytes));
        }
        let filename = validate_filename(&command.filename)?;
        let content_type = validate_content_type(&command.content_type)?;
        let file_id = Uuid::new_v4().to_string();
        let persisted_name = storage_name(&file_id)?;
        let size_bytes = i64::try_from(command.body.len()).context("文件大小超出数据库范围")?;
        let digest = format!("{:x}", Sha256::digest(&command.body));
        // 只有图片才生成公开图床令牌；其他文件不进入公开地址空间。
        let image_token = is_image_content_type(&content_type).then(new_image_token);
        let path = self
            .write_content(&command.tenant_id, &file_id, &command.body)
            .await?;
        let inserted = sqlx::query_scalar::<_, String>(
            r#"INSERT INTO file_objects
                   (id, tenant_id, original_name, content_type, size_bytes, sha256, storage_name, uploaded_by, image_token)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               RETURNING to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#,
        )
        .bind(&file_id)
        .bind(&command.tenant_id)
        .bind(&filename)
        .bind(&content_type)
        .bind(size_bytes)
        .bind(&digest)
        .bind(&persisted_name)
        .bind(&command.user_id)
        .bind(&image_token)
        .fetch_one(&self.pool)
        .await;
        let created_at = match inserted {
            Ok(created_at) => created_at,
            Err(error) => {
                let _ = fs::remove_file(path).await;
                return Err(error).context("保存文件元数据失败");
            }
        };
        Ok(FileItem {
            id: file_id,
            name: filename,
            content_type,
            size_bytes: u64::try_from(size_bytes).context("文件大小无效")?,
            sha256: digest,
            uploaded_by: command.user_id,
            created_at,
            image_token,
        })
    }

    async fn download(&self, query: FileQuery) -> Result<Option<DownloadObject>> {
        self.ensure_initialized().await?;
        let Some(stored) = self.find(&query.tenant_id, &query.file_id).await? else {
            return Ok(None);
        };
        self.read_stored(stored).await.map(Some)
    }

    async fn open_image(&self, query: ImageQuery) -> Result<Option<DownloadObject>> {
        self.ensure_initialized().await?;
        let token = validate_image_token(&query.token)?;
        let Some(stored) = self.find_by_image_token(&token).await? else {
            return Ok(None);
        };
        self.read_stored(stored).await.map(Some)
    }

    async fn delete(&self, query: FileQuery) -> Result<bool> {
        self.ensure_initialized().await?;
        let mut transaction = self.pool.begin().await.context("开始删除事务失败")?;
        let sql = format!(
            "SELECT {STORED_COLUMNS} FROM file_objects
             WHERE tenant_id = $1 AND id = $2
             FOR UPDATE"
        );
        let row = sqlx::query_as::<_, StoredRow>(&sql)
            .bind(&query.tenant_id)
            .bind(&query.file_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("锁定待删除文件失败")?;
        let Some(stored) = row.map(stored_from_row).transpose()? else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let source = self.path_for(&query.tenant_id, &stored.item.id, &stored.storage_name)?;
        let recycle = source.with_file_name(format!(".delete-{}", Uuid::new_v4()));
        fs::rename(&source, &recycle)
            .await
            .context("隔离待删除文件失败")?;
        let deleted = sqlx::query("DELETE FROM file_objects WHERE tenant_id = $1 AND id = $2")
            .bind(&query.tenant_id)
            .bind(&query.file_id)
            .execute(&mut *transaction)
            .await;
        if let Err(error) = deleted {
            let _ = fs::rename(&recycle, &source).await;
            return Err(error).context("删除文件元数据失败");
        }
        if let Err(error) = transaction.commit().await {
            let _ = fs::rename(&recycle, &source).await;
            return Err(error).context("提交删除事务失败");
        }
        fs::remove_file(&recycle)
            .await
            .context("清理已删除文件失败")?;
        Ok(true)
    }

    fn max_file_bytes(&self) -> usize {
        self.config.max_bytes
    }
}

impl FileServiceImpl {
    /// 读取并校验单个文件内容，供租户下载和公开图床共用。
    async fn read_stored(&self, stored: StoredFile) -> Result<DownloadObject> {
        let path = self.path_for(&stored.tenant_id, &stored.item.id, &stored.storage_name)?;
        let body = fs::read(&path)
            .await
            .with_context(|| format!("读取文件内容失败: {}", path.display()))?;
        ensure!(
            body.len() as u64 == stored.item.size_bytes,
            "文件内容长度与元数据不一致"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&body)) == stored.item.sha256,
            "文件内容摘要与元数据不一致"
        );
        Ok(DownloadObject {
            item: stored.item,
            body: Bytes::from(body),
        })
    }
}

fn stored_from_row(row: StoredRow) -> Result<StoredFile> {
    let (
        tenant_id,
        id,
        name,
        content_type,
        size_bytes,
        sha256,
        uploaded_by,
        created_at,
        image_token,
        storage_name,
    ) = row;
    Ok(StoredFile {
        tenant_id,
        item: FileItem {
            id,
            name,
            content_type,
            size_bytes: u64::try_from(size_bytes).context("数据库中的文件大小无效")?,
            sha256,
            uploaded_by,
            created_at,
            image_token,
        },
        storage_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要 AIO_TEST_DATABASE_URL 指向独立 PostgreSQL"]
    async fn persists_content_and_isolates_tenants() -> Result<()> {
        let database_url =
            env::var("AIO_TEST_DATABASE_URL").context("缺少 AIO_TEST_DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await?;
        let temporary = tempfile::tempdir()?;
        let service = FileServiceImpl::for_test(
            pool,
            FileStorageConfig::for_test(temporary.path().to_path_buf(), 1024),
        );
        let suffix = Uuid::new_v4();
        let tenant_a = format!("file-test-a-{suffix}");
        let tenant_b = format!("file-test-b-{suffix}");
        let oversized = service
            .upload(UploadCommand {
                tenant_id: tenant_a.clone(),
                user_id: "test-user".to_owned(),
                filename: "large.bin".to_owned(),
                content_type: "application/octet-stream".to_owned(),
                body: Bytes::from(vec![0_u8; 1025]),
            })
            .await
            .unwrap_err();
        assert!(oversized.to_string().contains("1024"));
        assert!(service.list(&tenant_a).await?.is_empty());
        let uploaded = service
            .upload(UploadCommand {
                tenant_id: tenant_a.clone(),
                user_id: "test-user".to_owned(),
                filename: "hello.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                body: Bytes::from_static(b"hello tenant"),
            })
            .await?;

        assert_eq!(service.list(&tenant_a).await?, vec![uploaded.clone()]);
        assert!(service.list(&tenant_b).await?.is_empty());
        assert!(
            service
                .download(FileQuery {
                    tenant_id: tenant_b.clone(),
                    file_id: uploaded.id.clone(),
                })
                .await?
                .is_none()
        );
        let downloaded = service
            .download(FileQuery {
                tenant_id: tenant_a.clone(),
                file_id: uploaded.id.clone(),
            })
            .await?
            .context("当前租户应能下载文件")?;
        assert_eq!(downloaded.body.as_ref(), b"hello tenant");
        assert!(uploaded.image_token.is_none(), "非图片不应生成图床令牌");

        // 图床令牌只对图片生成，并可通过公开地址在无租户上下文时读取。
        let image = service
            .upload(UploadCommand {
                tenant_id: tenant_a.clone(),
                user_id: "test-user".to_owned(),
                filename: "cover.png".to_owned(),
                content_type: "image/png".to_owned(),
                body: Bytes::from_static(b"\x89PNG\r\n\x1a\n"),
            })
            .await?;
        let token = image.image_token.clone().context("图片应生成图床令牌")?;
        let public = service
            .open_image(ImageQuery {
                token: token.clone(),
            })
            .await?
            .context("图床令牌应能读取图片")?;
        assert_eq!(public.body.as_ref(), b"\x89PNG\r\n\x1a\n");
        assert!(
            service
                .open_image(ImageQuery {
                    token: "f".repeat(64),
                })
                .await?
                .is_none(),
            "未知令牌不应返回图片"
        );

        let content_path =
            service.path_for(&tenant_a, &uploaded.id, &storage_name(&uploaded.id)?)?;
        fs::write(&content_path, b"tampered data").await?;
        assert!(
            service
                .download(FileQuery {
                    tenant_id: tenant_a.clone(),
                    file_id: uploaded.id.clone(),
                })
                .await
                .is_err()
        );
        fs::write(&content_path, b"hello tenant").await?;
        assert!(
            !service
                .delete(FileQuery {
                    tenant_id: tenant_b,
                    file_id: uploaded.id.clone(),
                })
                .await?
        );
        assert!(
            service
                .delete(FileQuery {
                    tenant_id: tenant_a.clone(),
                    file_id: uploaded.id.clone(),
                })
                .await?
        );
        assert!(
            service.open_image(ImageQuery { token }).await?.is_some(),
            "删除其他文件不应影响图床图片"
        );
        assert!(
            service
                .delete(FileQuery {
                    tenant_id: tenant_a.clone(),
                    file_id: image.id,
                })
                .await?
        );
        assert!(service.list(&tenant_a).await?.is_empty());
        Ok(())
    }
}
