use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub uploaded_by: String,
    pub created_at: String,
    /// 图片的不可猜测业务令牌；公开请求仍由宿主固定租户和插件版本。
    pub image_token: Option<String>,
    /// 宿主提供的公开路径，前端以外层应用来源构造完整链接。
    pub public_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileListing {
    pub files: Vec<FileItem>,
    pub max_file_bytes: u64,
}

/// 图床公开地址的多种嵌入形式。
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

/// 每块小于宿主普通请求及对象存储的上限，客户端不能自行扩大。
pub const FILE_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// 开始一次上传，元数据在写入任何对象前持久化。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BeginUpload {
    pub name: String,
    pub content_type: String,
    pub size_bytes: u64,
}

/// 同一用户可续传尚未过期的上传；已保存块允许相同内容的幂等重试。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UploadSession {
    pub id: String,
    pub chunk_bytes: u64,
    pub chunk_count: u32,
    pub received: Vec<u32>,
}
