mod controller;
mod model;
mod service;
mod service_impl;
mod util;

pub use controller::FileController;
pub use service::FileService;

use anyhow::Result;
use dill::CatalogBuilder;

pub fn register(builder: &mut CatalogBuilder) -> Result<()> {
    builder
        .add_value(service_impl::FileServiceImpl::from_env()?)
        .bind::<dyn FileService, service_impl::FileServiceImpl>()
        .add::<FileController>();
    Ok(())
}
