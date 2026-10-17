//! 独立文件进程复用一个 Dill 容器，不编译宿主身份或持有宿主磁盘目录。
mod generated;
pub mod hosting;
pub use generated::file_management::{
    DownloadObject, FileController, FileQuery, FileService, FileServiceImpl, ImageQuery,
    PUBLIC_IMAGE_TYPES,
};

pub use generated::file_management::migration;
