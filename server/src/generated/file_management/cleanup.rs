//! 删除先提交墓碑，等待宿主对象操作截止时间后再清理全部声明键。
use super::{FileServiceImpl, model::FileQuery};
use aio_plugin_file_model::FILE_CHUNK_BYTES;
use anyhow::Result;

impl FileServiceImpl {
    pub(super) async fn tombstone(&self, query: FileQuery, owner: Option<&str>) -> Result<bool> {
        self.check_tenant(&query.tenant_id)?;
        let changed=sqlx::query("UPDATE file_objects SET state='deleting',cleanup_after=coalesce(cleanup_after,now()+interval '60 seconds') WHERE tenant_id=$1 AND id=$2 AND (($3::text IS NULL AND state IN ('ready','deleting')) OR ($3 IS NOT NULL AND uploaded_by=$3 AND state IN ('uploading','deleting')))")
            .bind(&query.tenant_id).bind(&query.file_id).bind(owner).execute(&self.pool).await?.rows_affected();
        Ok(changed == 1)
    }

    /// 每次最多清理四个文件；任一对象删除失败时保留墓碑，下次或重启后继续。
    pub async fn cleanup(&self) -> Result<usize> {
        sqlx::query("UPDATE file_objects SET state='deleting',cleanup_after=now()+interval '60 seconds' WHERE tenant_id=$1 AND state='uploading' AND expires_at<=now()")
            .bind(&self.tenant).execute(&self.pool).await?;
        let mut removed = 0;
        for _ in 0..4 {
            let mut transaction = self.pool.begin().await?;
            let row:Option<(String,i64)>=sqlx::query_as("SELECT id,size_bytes FROM file_objects WHERE tenant_id=$1 AND state='deleting' AND cleanup_after<=now() ORDER BY cleanup_after,id LIMIT 1 FOR UPDATE SKIP LOCKED")
                .bind(&self.tenant).fetch_optional(&mut *transaction).await?;
            let Some((id, size)) = row else {
                break;
            };
            // 即使写入成功但块回执没有提交，也会枚举到该键。
            for index in 0..(size as u64).div_ceil(FILE_CHUNK_BYTES) {
                self.storage
                    .remove(&super::util::chunk_key(&id, index as u32))
                    .await?;
            }
            sqlx::query("DELETE FROM file_objects WHERE tenant_id=$1 AND id=$2")
                .bind(&self.tenant)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            removed += 1;
        }
        Ok(removed)
    }
}
