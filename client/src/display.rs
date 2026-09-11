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
}
