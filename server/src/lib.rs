mod generated;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use axum::Router;
use dill::CatalogBuilder;

pub use generated::file_management::FileService;

pub fn register(builder: &mut CatalogBuilder) -> Result<()> {
    generated::file_management::register(builder)
}

pub fn service(catalog: &dill::Catalog) -> Result<Arc<dyn FileService>> {
    catalog
        .get_one::<dyn FileService>()
        .context("文件服务未注册")
}

pub fn router(catalog: &dill::Catalog) -> Result<Router> {
    let controller = catalog
        .get_one::<generated::file_management::FileController>()
        .context("文件 Controller 未注册")?;
    Ok(controller.router())
}
