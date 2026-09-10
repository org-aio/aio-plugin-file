use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub uploaded_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileListing {
    pub files: Vec<FileItem>,
    pub max_file_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileResponse<T> {
    pub data: T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileErrorResponse {
    pub error: String,
}
