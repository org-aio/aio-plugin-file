use std::{env, path::PathBuf};

use aio_plugin_file_model::DEFAULT_MAX_FILE_BYTES;
use anyhow::{Context as _, Result, bail};
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use uuid::Uuid;

const MAX_CONFIGURED_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FileDomainError {
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug)]
pub(super) struct FileStorageConfig {
    pub root: PathBuf,
    pub max_bytes: usize,
}

impl FileStorageConfig {
    pub fn from_env() -> Result<Self> {
        let root = env::var_os("AIO_FILE_STORAGE_DIR")
            .map(PathBuf::from)
            .context("文件插件缺少 AIO_FILE_STORAGE_DIR")?;
        if !root.is_absolute() {
            bail!("AIO_FILE_STORAGE_DIR 必须是绝对路径");
        }
        let max_bytes = env::var("AIO_FILE_MAX_BYTES")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .context("AIO_FILE_MAX_BYTES 必须是正整数")
            })
            .transpose()?
            .unwrap_or(DEFAULT_MAX_FILE_BYTES);
        if max_bytes == 0 || max_bytes > MAX_CONFIGURED_FILE_BYTES {
            bail!("AIO_FILE_MAX_BYTES 必须介于 1 与 {MAX_CONFIGURED_FILE_BYTES} 之间");
        }
        Ok(Self {
            root,
            max_bytes: usize::try_from(max_bytes).context("文件大小限制超出当前平台范围")?,
        })
    }

    #[cfg(test)]
    pub fn for_test(root: PathBuf, max_bytes: usize) -> Self {
        Self { root, max_bytes }
    }
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
    content_type.starts_with("image/")
}

pub(super) fn storage_name(file_id: &str) -> Result<String> {
    let id = Uuid::parse_str(file_id)
        .map_err(|_| FileDomainError::Validation("文件 ID 无效".to_owned()))?;
    Ok(format!("{id}.blob"))
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
        assert_eq!(validate_filename(" 报告.pdf ").unwrap(), "报告.pdf");
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
            validate_content_type("").unwrap(),
            "application/octet-stream"
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
        assert_eq!(validate_image_token(&token).unwrap(), token);
        assert_eq!(
            validate_image_token(&token.to_ascii_uppercase()).unwrap(),
            token
        );
        assert!(validate_image_token("short").is_err());
        assert!(validate_image_token(&"z".repeat(64)).is_err());
        assert!(validate_image_token(&"0".repeat(63)).is_err());
    }
}
