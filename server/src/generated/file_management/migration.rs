//! 运维导入保留旧数据；每个文件独立提交，失败后可重新核对并续传。
use super::{
    FileService, FileServiceImpl,
    model::FileQuery,
    service_impl::COLUMNS,
    uploads::verify_chunk,
    util::{
        chunk_key, is_image_content_type, tenant_bucket, validate_content_type, validate_filename,
        validate_image_token,
    },
};
use aio_plugin_file_model::{DEFAULT_MAX_FILE_BYTES, FILE_CHUNK_BYTES, FileItem};
use anyhow::{Context, Result, ensure};
use axum::body::Bytes;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// 导出新旧公开路径映射，部署方据此更新引用，不保留旧宿主路由别名。
#[derive(Debug, Serialize)]
pub struct PublicLink {
    pub old_path: String,
    pub new_path: String,
}
#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub copied: usize,
    pub verified: usize,
    pub links: Vec<PublicLink>,
}
struct Legacy {
    item: FileItem,
    storage_name: String,
}

/// 部署者先停止旧文件服务。目标 schema 与角色来自权威绑定表，内容仅通过授权 broker 写入。
pub async fn copy_legacy(
    host: &PgPool,
    bindings: &PgPool,
    root: &Path,
    service: &FileServiceImpl,
) -> Result<MigrationReport> {
    service.verify_host_database().await?;
    let metadata = tokio::fs::symlink_metadata(root)
        .await
        .context("旧文件根目录不存在")?;
    ensure!(
        root.is_absolute() && metadata.is_dir() && !metadata.file_type().is_symlink(),
        "旧文件根路径必须是真实绝对目录"
    );
    let source: String = sqlx::query_scalar("SELECT id::text FROM component_sources WHERE git=$1")
        .bind("https://github.com/org-aio/aio-plugin-file.git")
        .fetch_optional(host)
        .await?
        .context("请先发布并安装文件整包")?;
    let mut binding = bindings.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *binding)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('aio-plugin-bindings',0))")
        .execute(&mut *binding)
        .await?;
    let expected:(String,String)=sqlx::query_as("SELECT schema_name,role_name FROM aio_plugin_host.database_bindings WHERE source_id=$1 AND tenant_id=$2")
        .bind(&source).bind(&service.tenant).fetch_optional(&mut *binding).await?.context("文件租户尚无权威数据库绑定")?;
    let actual: (String, String) = sqlx::query_as("SELECT current_schema(),current_user")
        .fetch_one(&service.pool)
        .await?;
    ensure!(expected == actual, "文件进程数据库与权威绑定不匹配");
    ensure!(
        service.public_base
            == format!(
                "/api/runtime/components/public/{source}/{}",
                urlencoding::encode(&service.tenant)
            ),
        "文件公开路径与权威来源不匹配"
    );
    let mut old = host.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *old)
        .await?;
    sqlx::query("LOCK TABLE public.file_objects IN SHARE MODE")
        .execute(&mut *old)
        .await
        .context("锁定旧文件表失败，请先停止旧宿主")?;
    let rows=sqlx::query(r#"SELECT id,original_name,content_type,size_bytes,sha256,storage_name,uploaded_by,image_token,
        to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS created
        FROM public.file_objects WHERE tenant_id=$1 ORDER BY id"#).bind(&service.tenant).fetch_all(&mut *old).await?;
    let mut report = MigrationReport {
        copied: 0,
        verified: 0,
        links: Vec::new(),
    };
    for row in rows {
        let image_token: Option<String> = row.try_get("image_token")?;
        let public_path = image_token
            .as_ref()
            .map(|token| format!("{}/i/{token}", service.public_base));
        let legacy = Legacy {
            item: FileItem {
                id: row.try_get("id")?,
                name: row.try_get("original_name")?,
                content_type: row.try_get("content_type")?,
                size_bytes: u64::try_from(row.try_get::<i64, _>("size_bytes")?)?,
                sha256: row.try_get("sha256")?,
                uploaded_by: row.try_get("uploaded_by")?,
                created_at: row.try_get("created")?,
                image_token,
                public_path,
            },
            storage_name: row.try_get("storage_name")?,
        };
        if copy_one(service, root, &legacy).await? {
            report.copied += 1;
        } else {
            report.verified += 1;
        }
        if let (Some(token), Some(path)) = (&legacy.item.image_token, &legacy.item.public_path) {
            report.links.push(PublicLink {
                old_path: format!("/i/{token}"),
                new_path: path.clone(),
            });
        }
    }
    old.rollback().await?;
    binding.rollback().await?;
    Ok(report)
}
async fn copy_one(service: &FileServiceImpl, root: &Path, legacy: &Legacy) -> Result<bool> {
    let item = &legacy.item;
    let id = uuid::Uuid::parse_str(&item.id).context("旧文件 ID 无效")?;
    ensure!(
        item.id == id.to_string() && legacy.storage_name == format!("{id}.blob"),
        "旧文件路径与 ID 不匹配"
    );
    ensure!(
        item.size_bytes <= DEFAULT_MAX_FILE_BYTES,
        "旧文件超过 100 MiB，拒绝丢失内容"
    );
    ensure!(
        validate_filename(&item.name)? == item.name
            && validate_content_type(&item.content_type)? == item.content_type,
        "旧文件元数据无效"
    );
    ensure!(
        item.sha256.len() == 64
            && item
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
        "旧文件摘要无效"
    );
    if let Some(token) = &item.image_token {
        ensure!(validate_image_token(token)? == *token, "旧图片令牌无效");
        ensure!(
            is_image_content_type(&item.content_type),
            "旧图片类型 {} 尚未在整包声明，拒绝丢失公开链接",
            item.content_type
        );
    }
    let directory = root.join(tenant_bucket(&service.tenant));
    let metadata = tokio::fs::symlink_metadata(&directory).await?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "旧租户目录不能是符号链接"
    );
    let path = directory.join(&legacy.storage_name);
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .await
        .context("打开旧文件失败")?;
    let metadata = file.metadata().await?;
    ensure!(
        metadata.is_file() && metadata.len() == item.size_bytes,
        "旧文件长度不匹配"
    );
    // 先验证原件，再登记上传，损坏原件不能污染不可变分块意图。
    let mut verified = Sha256::new();
    let mut remaining = item.size_bytes;
    let mut buffer = vec![0u8; FILE_CHUNK_BYTES as usize];
    while remaining > 0 {
        let size = remaining.min(FILE_CHUNK_BYTES) as usize;
        file.read_exact(&mut buffer[..size]).await?;
        verified.update(&buffer[..size]);
        remaining -= size as u64;
    }
    let mut extra = [0u8; 1];
    ensure!(file.read(&mut extra).await? == 0, "旧文件在校验中增长");
    ensure!(
        format!("{:x}", verified.finalize()) == item.sha256,
        "旧文件 {} 内容摘要不匹配",
        item.id
    );
    file.rewind().await?;
    let mut transaction = service.pool.begin().await?;
    sqlx::query("INSERT INTO file_objects (id,tenant_id,original_name,content_type,size_bytes,sha256,uploaded_by,image_token,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::text::timestamptz) ON CONFLICT(id) DO NOTHING")
        .bind(&item.id).bind(&service.tenant).bind(&item.name).bind(&item.content_type).bind(item.size_bytes as i64).bind(&item.sha256).bind(&item.uploaded_by).bind(&item.image_token).bind(&item.created_at).execute(&mut *transaction).await?;
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM file_objects WHERE tenant_id=$1 AND id=$2 FOR UPDATE"
    ))
    .bind(&service.tenant)
    .bind(&item.id)
    .fetch_optional(&mut *transaction)
    .await?
    .context("文件 ID 与其他租户冲突")?;
    let target = service.stored(row)?;
    ensure!(
        target.item == *item && target.state != "deleting",
        "文件 {} 已有不同元数据或删除意图，拒绝覆盖",
        item.id
    );
    let pending = target.state == "uploading";
    if pending {
        sqlx::query("UPDATE file_objects SET expires_at=now()+interval '24 hours' WHERE tenant_id=$1 AND id=$2").bind(&service.tenant).bind(&item.id).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    let mut hash = Sha256::new();
    for index in 0..item.size_bytes.div_ceil(FILE_CHUNK_BYTES) {
        let size = (item.size_bytes - index * FILE_CHUNK_BYTES).min(FILE_CHUNK_BYTES) as usize;
        let mut bytes = vec![0u8; size];
        file.read_exact(&mut bytes)
            .await
            .context("旧文件读取中断")?;
        hash.update(&bytes);
        let expected = format!("{:x}", Sha256::digest(&bytes));
        if pending {
            service
                .put_chunk(
                    FileQuery {
                        tenant_id: service.tenant.clone(),
                        file_id: item.id.clone(),
                    },
                    &item.uploaded_by,
                    index as u32,
                    Bytes::from(bytes),
                )
                .await?;
        }
        let intent:Option<(String,bool)>=sqlx::query_as("SELECT sha256,stored FROM file_chunks WHERE tenant_id=$1 AND file_id=$2 AND chunk_index=$3")
            .bind(&service.tenant).bind(&item.id).bind(index as i32).fetch_optional(&service.pool).await?;
        ensure!(
            intent == Some((expected.clone(), true)),
            "迁移目标分块元数据不匹配"
        );
        let stored = service
            .storage
            .read(&chunk_key(&item.id, index as u32))
            .await?
            .context("迁移分块丢失")?;
        verify_chunk(&stored, item.size_bytes, index as u32, &expected)?;
    }
    let mut extra = [0u8; 1];
    ensure!(file.read(&mut extra).await? == 0, "旧文件在迁移中增长");
    ensure!(
        format!("{:x}", hash.finalize()) == item.sha256,
        "旧文件 {} 内容摘要不匹配",
        item.id
    );
    // 成功读取与核对全部对象后才使文件可见；原图床令牌、时间和作者保持不变。
    if pending {
        let changed=sqlx::query("UPDATE file_objects SET state='ready' WHERE tenant_id=$1 AND id=$2 AND state='uploading'").bind(&service.tenant).bind(&item.id).execute(&service.pool).await?.rows_affected();
        ensure!(changed == 1, "迁移期间上传状态改变，请核对后重试");
    }
    Ok(pending)
}
