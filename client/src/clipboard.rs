/// 复用宿主剪贴板能力；真正写入成功后才显示反馈。
pub(crate) async fn write_text(value: String) -> Result<(), String> {
    crate::http::acknowledged(crate::http::copy(&value)?).await
}
