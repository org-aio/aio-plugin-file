/// 通过浏览器剪贴板写入文本，供图床链接一键复制。
pub(crate) async fn write_text(value: String) -> Result<(), String> {
    let script = copy_script(&value);
    let result = dioxus::document::eval(&script)
        .await
        .map_err(|error| error.to_string())?;
    match result.as_bool() {
        Some(true) => Ok(()),
        _ => Err("浏览器未允许写入剪贴板".to_owned()),
    }
}

/// 生成剪贴板写入脚本；返回布尔结果，便于 Rust 侧区分成功与拒绝。
fn copy_script(value: &str) -> String {
    let encoded = serde_json::to_string(value).expect("字符串应可序列化");
    format!(
        "try {{ await navigator.clipboard.writeText({encoded}); return true; }} catch (_) {{ return false; }}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_escapes_untrusted_link_content() {
        let script = copy_script("a\"b\nc");
        assert!(script.contains("\\\""));
        assert!(!script.contains('\n'));
        assert!(script.contains("return true;"));
        assert!(script.contains("return false;"));
    }
}
