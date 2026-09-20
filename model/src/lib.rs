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
    /// 图片文件的公开访问令牌；非图片文件为 `None`。
    ///
    /// 该令牌只用于拼接 `/i/{token}` 公开图床地址，不参与租户鉴权，
    /// 因此不能反推出文件 ID、租户或原始文件名。
    pub image_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileListing {
    pub files: Vec<FileItem>,
    pub max_file_bytes: u64,
}

/// 图床公开地址的多种嵌入形式，全部指向 `/i/{token}`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageLink {
    pub token: String,
    pub path: String,
    pub url: String,
    pub markdown: String,
    pub html: String,
    pub bbcode: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileResponse<T> {
    pub data: T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileErrorResponse {
    pub error: String,
}
