use aio_plugin_file_model::{FileItem, ImageLink};

pub(crate) fn format_bytes(bytes: u64) -> String {
    for (unit, divisor) in [
        ("GiB", 1024_u64.pow(3)),
        ("MiB", 1024_u64.pow(2)),
        ("KiB", 1024_u64),
    ] {
        if bytes >= divisor {
            return format!("{:.1} {unit}", bytes as f64 / divisor as f64);
        }
    }
    format!("{bytes} B")
}

/// 判断文件是否已生成图床公开地址。
pub(crate) fn image_link(file: &FileItem) -> Option<ImageLink> {
    let token = file.image_token.as_deref()?;
    let origin = web_sys::window()
        .and_then(|window| window.location().origin().ok())
        .filter(|origin| origin != "null")
        .unwrap_or_default();
    Some(image_link_for(token, &file.name, &origin))
}

/// 依据令牌、文件名和站点来源拼装图床链接，便于单测覆盖。
fn image_link_for(token: &str, name: &str, origin: &str) -> ImageLink {
    let path = format!("/i/{token}");
    let url = format!("{origin}{path}");
    ImageLink {
        token: token.to_owned(),
        path,
        url: url.clone(),
        markdown: format!("![{name}]({url})"),
        html: format!("<img src=\"{url}\" alt=\"{name}\">"),
        bbcode: format!("[img]{url}[/img]"),
    }
}

pub(crate) fn category(content_type: &str) -> &'static str {
    if content_type.starts_with("image/") {
        "image"
    } else if content_type.starts_with("audio/") || content_type.starts_with("video/") {
        "media"
    } else if content_type.starts_with("text/")
        || content_type == "application/pdf"
        || content_type.contains("officedocument")
        || content_type.contains("msword")
    {
        "document"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_and_categories_are_readable() {
        assert_eq!(format_bytes(7), "7 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MiB");
        assert_eq!(category("image/png"), "image");
        assert_eq!(category("application/pdf"), "document");
        assert_eq!(category("video/mp4"), "media");
        assert_eq!(category("application/octet-stream"), "other");
    }

    #[test]
    fn image_link_builds_public_embed_forms() {
        let token = "a".repeat(64);
        let link = image_link_for(&token, "封面.png", "https://aio.addzero.site");
        assert_eq!(link.path, format!("/i/{token}"));
        assert_eq!(link.url, format!("https://aio.addzero.site/i/{token}"));
        assert_eq!(link.markdown, format!("![封面.png]({})", link.url));
        assert_eq!(
            link.html,
            format!("<img src=\"{}\" alt=\"封面.png\">", link.url)
        );
        assert_eq!(link.bbcode, format!("[img]{}[/img]", link.url));
    }
}
