use super::model::{DownloadObject, FileQuery, ImageQuery};
use aio_plugin_file_model::{BeginUpload, FileItem, UploadSession};
use anyhow::Result;
use axum::body::Bytes;

/// Controller 仅使用领域契约；持久化与对象写入由实现协调。
#[async_trait::async_trait]
pub trait FileService: std::any::Any + Send + Sync {
    async fn list(&self, tenant: &str) -> Result<Vec<FileItem>>;
    async fn begin(&self, tenant: &str, user: &str, request: BeginUpload) -> Result<UploadSession>;
    async fn upload_status(&self, query: FileQuery, user: &str) -> Result<UploadSession>;
    async fn put_chunk(&self, query: FileQuery, user: &str, index: u32, body: Bytes) -> Result<()>;
    async fn complete(&self, query: FileQuery, user: &str) -> Result<FileItem>;
    async fn cancel_upload(&self, query: FileQuery, user: &str) -> Result<bool>;
    async fn download(&self, query: FileQuery) -> Result<Option<DownloadObject>>;
    async fn open_image(&self, query: ImageQuery) -> Result<Option<DownloadObject>>;
    async fn delete(&self, query: FileQuery) -> Result<bool>;
    fn max_file_bytes(&self) -> u64;
}
