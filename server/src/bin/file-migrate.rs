//! 仅部署者运行的迁移工具，使用正在安装实例的宿主配置和对象授权。
use aio_plugin_file_server::{FileServiceImpl, migration::copy_legacy};
use anyhow::{Context, Result};
use az_plugin_process::Host;
use sqlx::postgres::PgPoolOptions;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let host = Host::from_env()?;
    let source = std::env::var("AIO_FILE_LEGACY_DATABASE_URL").context("缺少旧宿主数据库配置")?;
    let bindings =
        std::env::var("AIO_FILE_BINDINGS_DATABASE_URL").context("缺少权威绑定数据库配置")?;
    let root =
        PathBuf::from(std::env::var_os("AIO_FILE_LEGACY_STORAGE_DIR").context("缺少旧文件目录")?);
    let service = Arc::new(FileServiceImpl::new(
        host.configuration
            .database_url
            .as_deref()
            .context("缺少文件数据库授权")?,
        host.storage()?,
        host.configuration.tenant_id.clone(),
        host.configuration
            .public_base
            .clone()
            .context("缺少公开入口")?,
    )?);
    let source = PgPoolOptions::new()
        .max_connections(2)
        .connect(&source)
        .await?;
    let bindings = PgPoolOptions::new()
        .max_connections(2)
        .connect(&bindings)
        .await?;
    let result = copy_legacy(&source, &bindings, &root, &service).await;
    service.shutdown().await;
    source.close().await;
    bindings.close().await;
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
