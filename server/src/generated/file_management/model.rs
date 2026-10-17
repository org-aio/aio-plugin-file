use aio_plugin_file_model::FileItem;
use axum::body::Body;

/// 管理请求中的租户来自可信 Caller，不能由浏览器参数覆盖。
pub struct FileQuery {
    pub tenant_id: String,
    pub file_id: String,
}

/// 匿名访问也固定到当前插件实例的租户。
pub struct ImageQuery {
    pub tenant_id: String,
    pub token: String,
}

/// 内容按块校验后交给 HTTP Body，不能一次读取整个文件。
pub struct DownloadObject {
    pub item: FileItem,
    pub body: Body,
}

pub(super) struct StoredFile {
    pub item: FileItem,
    pub state: String,
    pub expired: bool,
}
