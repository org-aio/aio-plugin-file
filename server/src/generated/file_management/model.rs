use aio_plugin_file_model::FileItem;
use axum::body::Bytes;

pub struct UploadCommand {
    pub tenant_id: String,
    pub user_id: String,
    pub filename: String,
    pub content_type: String,
    pub body: Bytes,
}

pub struct FileQuery {
    pub tenant_id: String,
    pub file_id: String,
}

pub struct DownloadObject {
    pub item: FileItem,
    pub body: Bytes,
}

pub(super) struct StoredFile {
    pub item: FileItem,
    pub storage_name: String,
}
