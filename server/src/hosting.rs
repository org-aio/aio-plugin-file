//! 宿主配置、生命周期与业务装配；健康和描述不承载业务数据。
use crate::{FileController, FileService, FileServiceImpl, PUBLIC_IMAGE_TYPES};
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, routing::get};
use az_plugin_process::Host;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

/// 生产及真实 HTTP 验收使用同一身份入口和 Dill 图。
pub fn router(host: &Host, service: Arc<FileServiceImpl>) -> Result<Router> {
    let mut builder = dill::CatalogBuilder::new();
    builder
        .add_builder(service)
        .bind::<dyn FileService, FileServiceImpl>()
        .add::<FileController>();
    let controller = builder
        .build()
        .get_one::<FileController>()
        .context("文件 Controller 未注册")?;
    Ok(host
        .protect(controller.clone().router())
        .merge(host.protect_public(controller.public_router()))
        .route("/health", get(|| async { "ok" }))
        .route("/aio/describe", get(describe)))
}
/// 只使用宿主配置；没有全局数据库、任意磁盘路径或未认证的备用入口。
pub async fn run() -> Result<()> {
    let host = Host::from_env()?;
    let database = host
        .configuration
        .database_url
        .as_deref()
        .context("宿主未授权文件数据库")?;
    let public_base = host
        .configuration
        .public_base
        .clone()
        .context("宿主未配置图片公开入口")?;
    ensure!(
        host.configuration
            .public_routes
            .iter()
            .any(|route| route.path == "/i/{token}"
                && PUBLIC_IMAGE_TYPES
                    .iter()
                    .all(|mime| route.content_types.iter().any(|value| value == mime))),
        "文件公开类型与整包声明不一致"
    );
    let service = Arc::new(FileServiceImpl::new(
        database,
        host.storage()?,
        host.configuration.tenant_id.clone(),
        public_base,
    )?);
    let result = async {
        service.verify_host_database().await?;
        let router = router(&host, service.clone())?;
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(maintain(service.clone(), cancellation.clone()));
        let served = host.serve(router).await;
        cancellation.cancel();
        task.await.context("文件清理任务异常结束")?;
        served
    }
    .await;
    service.shutdown().await;
    result
}
async fn maintain(service: Arc<FileServiceImpl>, cancellation: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {_=cancellation.cancelled()=>break,_=interval.tick()=>{}}
        let result =
            tokio::select! {_=cancellation.cancelled()=>break,result=service.cleanup()=>result};
        if let Err(error) = result {
            eprintln!("文件清理暂未完成，将重试: {error:#}");
        }
    }
}
/// 文件页面由安装版本动态贡献，停用后随整包移除。
async fn describe() -> Json<Value> {
    Json(json!({"label":"文件管理","pages":[{
        "id":"file-list","label":"文件列表","entry":"index.html","scene":["system","系统"],
        "menu_path":["基础设施","文件管理"],
        "permission":null,"surface":"workspace"
    }]}))
}
