use aio_plugin_file_model::FileItem;
use anyhow::Result;

use super::model::{DownloadObject, FileQuery, UploadCommand};

#[async_trait::async_trait]
pub trait FileService: Send + Sync {
    async fn initialize(&self) -> Result<()>;
    async fn list(&self, tenant_id: &str) -> Result<Vec<FileItem>>;
    async fn upload(&self, command: UploadCommand) -> Result<FileItem>;
    async fn download(&self, query: FileQuery) -> Result<Option<DownloadObject>>;
    async fn delete(&self, query: FileQuery) -> Result<bool>;
    fn max_file_bytes(&self) -> usize;
}
