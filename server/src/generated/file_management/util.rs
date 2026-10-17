use anyhow::Result;
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use uuid::Uuid;

/// 公开类型与整包声明一致；SVG 由宿主无脚本 CSP 限制。
pub const PUBLIC_IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/svg+xml",
    "image/avif",
    "image/bmp",
    "image/tiff",
    "image/x-icon",
    "image/vnd.microsoft.icon",
    "image/heic",
    "image/heif",
    "image/jxl",
];

#[derive(Debug, Error)]
pub enum FileDomainError {
    #[error("{0}")]
    Validation(String),
    #[error("文件或上传不存在")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
}

/// 对象键不含用户文件名，且一个上传的每个索引只允许一种内容摘要。
pub(super) fn chunk_key(id: &str, index: u32) -> String {
    format!("files/{id}/{index}")
}

pub(super) fn validate_filename(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return validation("文件名不能为空");
    }
    if value.len() > 255 {
        return validation("文件名不能超过 255 字节");
    }
    if matches!(value, "." | "..") {
        return validation("文件名无效");
    }
    if value
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return validation("文件名不能包含路径分隔符或控制字符");
    }
    Ok(value.to_owned())
}

pub(super) fn validate_content_type(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok("application/octet-stream".to_owned());
    }
    if value.len() > 127
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || !value.contains('/')
    {
        return validation("Content-Type 无效");
    }
    Ok(value.to_ascii_lowercase())
}

pub(super) fn tenant_bucket(tenant_id: &str) -> String {
    format!("{:x}", Sha256::digest(tenant_id.as_bytes()))
}

/// 图床令牌使用独立随机值，避免公开地址暴露文件 ID 或租户信息。
pub(super) fn new_image_token() -> String {
    format!("{:x}", Sha256::digest(Uuid::new_v4().as_bytes()))
}

pub(super) fn validate_image_token(value: &str) -> Result<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return validation("图床令牌无效");
    }
    Ok(value.to_ascii_lowercase())
}

pub(super) fn is_image_content_type(content_type: &str) -> bool {
    PUBLIC_IMAGE_TYPES.contains(&content_type)
}

pub(super) fn content_disposition(filename: &str) -> String {
    format!("attachment; {}", encoded_filename(filename))
}

/// 图床公开地址使用 inline，浏览器可直接渲染图片而不是触发下载。
pub(super) fn inline_disposition(filename: &str) -> String {
    format!("inline; {}", encoded_filename(filename))
}

fn encoded_filename(filename: &str) -> String {
    let encoded = filename
        .as_bytes()
        .iter()
        .map(|byte| {
            if byte.is_ascii_alphanumeric()
                || matches!(
                    *byte,
                    b'!' | b'#'
                        | b'$'
                        | b'&'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
            {
                char::from(*byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect::<String>();
    format!("filename=\"download\"; filename*=UTF-8''{encoded}")
}

pub(super) fn validation<T>(message: impl Into<String>) -> Result<T> {
    Err(FileDomainError::Validation(message.into()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_like_and_control_filenames() {
        assert!(validate_filename("../secret").is_err());
        assert!(validate_filename("folder/file.txt").is_err());
        assert!(validate_filename("bad\0name").is_err());
        assert_eq!(
            validate_filename(" 报告.pdf ").ok().as_deref(),
            Some("报告.pdf")
        );
    }

    #[test]
    fn tenant_bucket_does_not_expose_tenant_identity() {
        let bucket = tenant_bucket("tenant/acme");
        assert_eq!(bucket.len(), 64);
        assert!(!bucket.contains("tenant"));
        assert_ne!(bucket, tenant_bucket("tenant/other"));
    }

    #[test]
    fn disposition_encodes_untrusted_filename() {
        let value = content_disposition("季度 报告.pdf");
        assert!(value.starts_with("attachment;"));
        assert!(value.contains("%E5%AD%A3%E5%BA%A6%20"));
        assert!(!value.contains('\n'));
    }

    #[test]
    fn rejects_unsafe_content_types() {
        assert!(validate_content_type("text/plain\r\nx-test: yes").is_err());
        assert!(validate_content_type("not-a-media-type").is_err());
        assert_eq!(
            validate_content_type("").ok().as_deref(),
            Some("application/octet-stream")
        );
    }

    #[test]
    fn recognizes_only_images_for_public_tokens() {
        assert!(is_image_content_type("image/png"));
        assert!(is_image_content_type("image/svg+xml"));
        assert!(!is_image_content_type("application/pdf"));
        assert!(!is_image_content_type("text/plain"));
    }

    #[test]
    fn validates_public_image_tokens() {
        let token = new_image_token();
        assert_eq!(token.len(), 64);
        assert_eq!(
            validate_image_token(&token).ok().as_deref(),
            Some(token.as_str())
        );
        assert_eq!(
            validate_image_token(&token.to_ascii_uppercase())
                .ok()
                .as_deref(),
            Some(token.as_str())
        );
        assert!(validate_image_token("short").is_err());
        assert!(validate_image_token(&"z".repeat(64)).is_err());
        assert!(validate_image_token(&"0".repeat(63)).is_err());
    }
}
