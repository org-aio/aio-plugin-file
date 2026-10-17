//! 上传元数据先于对象写入持久化，重复索引只能写入同一内容。
use super::{
    FileServiceImpl,
    model::{FileQuery, StoredFile},
    service_impl::COLUMNS,
    util::{
        FileDomainError, chunk_key, is_image_content_type, new_image_token, validate_content_type,
        validate_filename, validation,
    },
};
use aio_plugin_file_model::{
    BeginUpload, DEFAULT_MAX_FILE_BYTES, FILE_CHUNK_BYTES, FileItem, UploadSession,
};
use anyhow::{Context, Result, ensure};
use axum::body::Bytes;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};

pub(super) async fn begin(
    service: &FileServiceImpl,
    tenant: &str,
    user: &str,
    request: BeginUpload,
) -> Result<UploadSession> {
    service.check_tenant(tenant)?;
    let name = validate_filename(&request.name)?;
    let content_type = validate_content_type(&request.content_type)?;
    if request.size_bytes > DEFAULT_MAX_FILE_BYTES {
        return validation("文件不能超过 100 MiB");
    }
    ensure!(!user.is_empty(), "上传者不能为空");
    let mut transaction = service.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('file-upload:' || $1,0))")
        .bind(tenant)
        .execute(&mut *transaction)
        .await?;
    let (all, own):(i64,i64)=sqlx::query_as("SELECT count(*),count(*) FILTER (WHERE uploaded_by=$2) FROM file_objects WHERE tenant_id=$1 AND state='uploading'")
        .bind(tenant).bind(user).fetch_one(&mut *transaction).await?;
    if all >= 32 || own >= 8 {
        return Err(FileDomainError::Conflict("未完成上传过多，请完成或取消后重试".into()).into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO file_objects (id,tenant_id,original_name,content_type,size_bytes,uploaded_by) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(&id).bind(tenant).bind(name).bind(content_type).bind(i64::try_from(request.size_bytes)?).bind(user).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(UploadSession {
        id,
        chunk_bytes: FILE_CHUNK_BYTES,
        chunk_count: request.size_bytes.div_ceil(FILE_CHUNK_BYTES) as u32,
        received: Vec::new(),
    })
}

pub(super) async fn status(
    service: &FileServiceImpl,
    query: FileQuery,
    user: &str,
) -> Result<UploadSession> {
    let file = service
        .find(&query)
        .await?
        .ok_or(FileDomainError::NotFound)?;
    service.upload_owner(&file, user)?;
    let received:Vec<i32>=sqlx::query_scalar("SELECT chunk_index FROM file_chunks WHERE tenant_id=$1 AND file_id=$2 AND stored ORDER BY chunk_index")
        .bind(&service.tenant).bind(&file.item.id).fetch_all(&service.pool).await?;
    Ok(UploadSession {
        id: file.item.id,
        chunk_bytes: FILE_CHUNK_BYTES,
        chunk_count: file.item.size_bytes.div_ceil(FILE_CHUNK_BYTES) as u32,
        received: received.into_iter().map(|index| index as u32).collect(),
    })
}

async fn lock_file(
    service: &FileServiceImpl,
    transaction: &mut Transaction<'_, Postgres>,
    query: &FileQuery,
    user: &str,
) -> Result<StoredFile> {
    service.check_tenant(&query.tenant_id)?;
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM file_objects WHERE tenant_id=$1 AND id=$2 FOR UPDATE"
    ))
    .bind(&query.tenant_id)
    .bind(&query.file_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(FileDomainError::NotFound)?;
    let file = service.stored(row)?;
    service.upload_owner(&file, user)?;
    Ok(file)
}

pub(super) fn chunk_size(size: u64, index: u32) -> Result<u64> {
    let start = u64::from(index) * FILE_CHUNK_BYTES;
    if start >= size {
        return validation("上传分块索引无效");
    }
    Ok((size - start).min(FILE_CHUNK_BYTES))
}

pub(super) async fn put(
    service: &FileServiceImpl,
    query: FileQuery,
    user: &str,
    index: u32,
    body: Bytes,
) -> Result<()> {
    let digest = format!("{:x}", Sha256::digest(&body));
    let mut transaction = service.pool.begin().await?;
    let file = lock_file(service, &mut transaction, &query, user).await?;
    if body.len() as u64 != chunk_size(file.item.size_bytes, index)? {
        return validation("上传分块长度与声明不一致");
    }
    let existing:Option<(String,bool)>=sqlx::query_as("SELECT sha256,stored FROM file_chunks WHERE tenant_id=$1 AND file_id=$2 AND chunk_index=$3")
        .bind(&query.tenant_id).bind(&query.file_id).bind(index as i32).fetch_optional(&mut *transaction).await?;
    if let Some((previous, stored)) = existing {
        if previous != digest {
            return Err(
                FileDomainError::Conflict("同一分块不能更换内容，请重新上传".into()).into(),
            );
        }
        if stored {
            return Ok(());
        }
    } else {
        ensure!(file.state == "uploading", "已完成文件不能新增分块");
        sqlx::query(
            "INSERT INTO file_chunks (tenant_id,file_id,chunk_index,sha256) VALUES($1,$2,$3,$4)",
        )
        .bind(&query.tenant_id)
        .bind(&query.file_id)
        .bind(index as i32)
        .bind(&digest)
        .execute(&mut *transaction)
        .await?;
    }
    // 对象可能在 HTTP 取消后才写完；摘要先提交，后续重试绝不能覆盖成其他内容。
    transaction.commit().await?;
    let mut transaction = service.pool.begin().await?;
    let file = lock_file(service, &mut transaction, &query, user).await?;
    if file.state == "ready" {
        return Ok(());
    }
    ensure!(file.state == "uploading", "上传状态已变化");
    service
        .storage
        .write(&chunk_key(&query.file_id, index), body.to_vec())
        .await?;
    sqlx::query(
        "UPDATE file_chunks SET stored=true WHERE tenant_id=$1 AND file_id=$2 AND chunk_index=$3",
    )
    .bind(&query.tenant_id)
    .bind(&query.file_id)
    .bind(index as i32)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) async fn complete(
    service: &FileServiceImpl,
    query: FileQuery,
    user: &str,
) -> Result<FileItem> {
    let mut transaction = service.pool.begin().await?;
    let mut file = lock_file(service, &mut transaction, &query, user).await?;
    if file.state == "ready" {
        return Ok(file.item);
    }
    let chunks:Vec<(i32,String)>=sqlx::query_as("SELECT chunk_index,sha256 FROM file_chunks WHERE tenant_id=$1 AND file_id=$2 AND stored ORDER BY chunk_index")
        .bind(&query.tenant_id).bind(&query.file_id).fetch_all(&mut *transaction).await?;
    if chunks.len() as u64 != file.item.size_bytes.div_ceil(FILE_CHUNK_BYTES) {
        return Err(FileDomainError::Conflict("上传分块尚未齐全".into()).into());
    }
    let mut digest = Sha256::new();
    for (expected, (index, hash)) in chunks.into_iter().enumerate() {
        ensure!(index as usize == expected, "上传分块索引不连续");
        let bytes = service
            .storage
            .read(&chunk_key(&query.file_id, index as u32))
            .await?
            .context("上传分块丢失")?;
        verify_chunk(&bytes, file.item.size_bytes, index as u32, &hash)?;
        digest.update(&bytes);
    }
    let actual = format!("{:x}", digest.finalize());
    ensure!(
        file.item.sha256.is_empty() || file.item.sha256 == actual,
        "文件摘要与导入声明不一致"
    );
    let imported = !file.item.sha256.is_empty();
    file.item.sha256 = actual;
    if !imported {
        file.item.image_token =
            is_image_content_type(&file.item.content_type).then(new_image_token);
    }
    file.item.public_path = file
        .item
        .image_token
        .as_ref()
        .map(|token| format!("{}/i/{token}", service.public_base));
    sqlx::query("UPDATE file_objects SET state='ready',sha256=$3,image_token=$4 WHERE tenant_id=$1 AND id=$2")
        .bind(&query.tenant_id).bind(&query.file_id).bind(&file.item.sha256).bind(&file.item.image_token).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(file.item)
}

pub(super) fn verify_chunk(bytes: &[u8], size: u64, index: u32, expected: &str) -> Result<()> {
    ensure!(
        bytes.len() as u64 == chunk_size(size, index)?,
        "文件分块长度不一致"
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == expected,
        "文件分块摘要不一致"
    );
    Ok(())
}
