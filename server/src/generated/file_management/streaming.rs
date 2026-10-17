//! 下载按块校验并保留背压，删除后的下一块读取会停止。
use super::{
    FileServiceImpl,
    model::{DownloadObject, StoredFile},
    uploads::verify_chunk,
    util::chunk_key,
};
use aio_plugin_file_model::FILE_CHUNK_BYTES;
use anyhow::{Context, Result, ensure};
use axum::body::{Body, Bytes};
use futures_util::stream::try_unfold;
use sha2::{Digest, Sha256};

impl FileServiceImpl {
    pub(super) async fn stream(&self, stored: StoredFile) -> Result<DownloadObject> {
        let chunks:Vec<(i32,String)>=sqlx::query_as("SELECT chunk_index,sha256 FROM file_chunks WHERE tenant_id=$1 AND file_id=$2 AND stored ORDER BY chunk_index")
            .bind(&self.tenant).bind(&stored.item.id).fetch_all(&self.pool).await?;
        ensure!(
            chunks.len() as u64 == stored.item.size_bytes.div_ceil(FILE_CHUNK_BYTES),
            "文件分块不完整"
        );
        let storage = self.storage.clone();
        let pool = self.pool.clone();
        let tenant = self.tenant.clone();
        let item = stored.item.clone();
        let body = try_unfold(
            (chunks.into_iter(), 0u32, Sha256::new()),
            move |(mut chunks, index, mut digest)| {
                let storage = storage.clone();
                let pool = pool.clone();
                let tenant = tenant.clone();
                let item = item.clone();
                async move {
                    let Some((actual, hash)) = chunks.next() else {
                        ensure!(
                            format!("{:x}", digest.finalize()) == item.sha256,
                            "文件整体摘要不一致"
                        );
                        return Ok::<_, anyhow::Error>(None);
                    };
                    ensure!(actual == index as i32, "文件分块索引不连续");
                    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_objects WHERE tenant_id=$1 AND id=$2 AND state='ready')")
                    .bind(tenant).bind(&item.id).fetch_one(&pool).await?;
                    ensure!(active, "文件已删除");
                    let bytes = storage
                        .read(&chunk_key(&item.id, index))
                        .await?
                        .context("文件分块丢失")?;
                    verify_chunk(&bytes, item.size_bytes, index, &hash)?;
                    digest.update(&bytes);
                    if chunks.len() == 0 {
                        ensure!(
                            format!("{:x}", digest.clone().finalize()) == item.sha256,
                            "文件整体摘要不一致"
                        );
                    }
                    Ok(Some((Bytes::from(bytes), (chunks, index + 1, digest))))
                }
            },
        );
        Ok(DownloadObject {
            item: stored.item,
            body: Body::from_stream(body),
        })
    }
}
