/// 通过浏览器剪贴板写入文本，供图床链接一键复制。
pub(crate) async fn write_text(value: String) -> Result<(), String> {
    let script = format!(
        "return await navigator.clipboard.writeText({});",
        serde_json::to_string(&value).map_err(|error| error.to_string())?
    );
    dioxus::document::eval(&script)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}
