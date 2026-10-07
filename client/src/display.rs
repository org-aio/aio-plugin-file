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

/// 依据文件后缀优先归类，缺失或未知后缀时回落到 Content-Type。
///
/// 浏览器对 `.apk`、`.sql`、`.db` 等后缀常常上报通用 MIME
/// （如 `application/octet-stream`、`application/x-www-form-urlencoded`），
/// 只按 MIME 会全部落到「其他」，因此这里以文件名后缀为主判据。
pub(crate) fn file_category(file: &FileItem) -> &'static str {
    category_of(&file.name, &file.content_type)
}

/// 后缀与 MIME 的分类入口，便于单测覆盖；后缀大小写不敏感。
fn category_of(name: &str, content_type: &str) -> &'static str {
    if let Some(kind) = extension_kind(&extension(name)) {
        return kind;
    }
    content_type_kind(&content_type.to_ascii_lowercase())
}

/// 取最后一个 `.` 之后的小写后缀；无后缀或后缀超长时返回空串。
fn extension(name: &str) -> String {
    let Some((_, suffix)) = name.rsplit_once('.') else {
        return String::new();
    };
    if suffix.is_empty() || suffix.len() > 12 || !suffix.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return String::new();
    }
    suffix.to_ascii_lowercase()
}

fn extension_kind(extension: &str) -> Option<&'static str> {
    let kind = match extension {
        "" => return None,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "heic" | "heif"
        | "avif" | "tif" | "tiff" => "image",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "oga" | "m4a" | "wma" | "opus" | "mp4" | "mkv"
        | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" | "3gp" => "media",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "txt" | "rtf" | "md" | "csv" | "epub" | "mobi" => "document",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "tbz" | "txz"
        | "lz4" | "cab" | "iso" => "archive",
        "apk" | "aab" | "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "ipa"
        | "jar" | "war" => "application",
        "rs" | "kt" | "kts" | "java" | "scala" | "groovy" | "py" | "rb" | "php" | "go" | "js"
        | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "c" | "cc" | "cpp" | "h" | "hpp" | "cs"
        | "swift" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" | "html" | "htm"
        | "css" | "scss" | "sass" | "less" | "vue" | "svelte" | "json" | "json5" | "yaml"
        | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" | "env" | "properties" | "gradle"
        | "lock" => "code",
        "db" | "sqlite" | "sqlite3" | "sql" | "mdb" | "accdb" | "dump" | "bak" | "bkp" => {
            "database"
        }
        _ => return None,
    };
    Some(kind)
}

pub(crate) fn content_type_kind(content_type: &str) -> &'static str {
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
    } else if content_type.contains("zip")
        || content_type.contains("tar")
        || content_type.contains("compressed")
        || content_type.contains("x-7z")
        || content_type.contains("x-rar")
    {
        "archive"
    } else if content_type.contains("android.package-archive")
        || content_type.contains("x-msdownload")
        || content_type.contains("x-msi")
        || content_type.contains("x-deb")
        || content_type.contains("x-rpm")
        || content_type.contains("x-apple-diskimage")
    {
        "application"
    } else if content_type.contains("sql") || content_type.contains("sqlite") {
        "database"
    } else if content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("yaml")
        || content_type.contains("javascript")
        || content_type.contains("ecmascript")
    {
        "code"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_are_readable() {
        assert_eq!(format_bytes(7), "7 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MiB");
    }

    #[test]
    fn classifies_by_extension_before_content_type() {
        // 后缀优先：浏览器对 apk/sql 常上报通用 MIME，仍应按后缀归类。
        assert_eq!(
            category_of("app.apk", "application/x-www-form-urlencoded"),
            "application"
        );
        assert_eq!(
            category_of("dump.sql", "application/octet-stream"),
            "database"
        );
        assert_eq!(
            category_of("data.db", "application/octet-stream"),
            "database"
        );
        assert_eq!(
            category_of("bundle.zip", "application/octet-stream"),
            "archive"
        );
        assert_eq!(category_of("Main.kt", "application/octet-stream"), "code");
        assert_eq!(
            category_of("config.json", "application/octet-stream"),
            "code"
        );
        // 后缀大小写不敏感。
        assert_eq!(
            category_of("PHOTO.PNG", "application/octet-stream"),
            "image"
        );
    }

    #[test]
    fn falls_back_to_content_type_without_known_extension() {
        assert_eq!(category_of("report", "image/png"), "image");
        assert_eq!(category_of("report", "application/pdf"), "document");
        assert_eq!(category_of("clip", "video/mp4"), "media");
        assert_eq!(category_of("bundle", "application/zip"), "archive");
        assert_eq!(
            category_of("payload", "application/vnd.android.package-archive"),
            "application"
        );
        assert_eq!(category_of("model", "application/sql"), "database");
        assert_eq!(
            category_of("payload.bin", "application/octet-stream"),
            "other"
        );
    }

    #[test]
    fn content_type_fallback_is_case_insensitive() {
        assert_eq!(category_of("report", "IMAGE/PNG"), "image");
        assert_eq!(category_of("clip", "Video/MP4"), "media");
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
