use super::{
    model::{DownloadObject, FileQuery, ImageQuery},
    service::FileService,
    util::{FileDomainError, content_disposition, inline_disposition},
};
use aio_plugin_file_model::{
    BeginUpload, FILE_CHUNK_BYTES, FileErrorResponse, FileItem, FileListing, FileResponse,
    UploadSession,
};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use az_plugin_process::{Caller, PublicCaller};
use std::sync::Arc;

/// 业务入口消费 SDK 注入的身份；权限由整包 required_permissions 实时约束。
#[dill::component]
#[dill::scope(dill::Singleton)]
pub struct FileController {
    service: Arc<dyn FileService>,
}
impl FileController {
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/files", get(list))
            .route("/files/{id}", get(download).delete(delete_file))
            .route("/uploads", post(begin))
            .route("/uploads/{id}", get(upload_status).delete(cancel_upload))
            .route("/uploads/{id}/chunks/{index}", put(put_chunk))
            .route("/uploads/{id}/complete", post(complete))
            .layer(DefaultBodyLimit::max(FILE_CHUNK_BYTES as usize))
            .with_state(self)
    }
    pub fn public_router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/i/{token}", get(public_image))
            .with_state(self)
    }
}
async fn list(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<FileResponse<FileListing>>, FileHttpError> {
    let files = controller.service.list(&caller.tenant_id).await?;
    Ok(Json(FileResponse {
        data: FileListing {
            files,
            max_file_bytes: controller.service.max_file_bytes(),
        },
    }))
}
async fn begin(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<BeginUpload>,
) -> Result<(StatusCode, Json<FileResponse<UploadSession>>), FileHttpError> {
    let data = controller
        .service
        .begin(&caller.tenant_id, &caller.user_id, request)
        .await?;
    Ok((StatusCode::CREATED, Json(FileResponse { data })))
}
async fn upload_status(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<Json<FileResponse<UploadSession>>, FileHttpError> {
    let data = controller
        .service
        .upload_status(
            FileQuery {
                tenant_id: caller.tenant_id,
                file_id: id,
            },
            &caller.user_id,
        )
        .await?;
    Ok(Json(FileResponse { data }))
}
async fn put_chunk(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path((id, index)): Path<(String, u32)>,
    body: Bytes,
) -> Result<StatusCode, FileHttpError> {
    controller
        .service
        .put_chunk(
            FileQuery {
                tenant_id: caller.tenant_id,
                file_id: id,
            },
            &caller.user_id,
            index,
            body,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn complete(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<Json<FileResponse<FileItem>>, FileHttpError> {
    let data = controller
        .service
        .complete(
            FileQuery {
                tenant_id: caller.tenant_id,
                file_id: id,
            },
            &caller.user_id,
        )
        .await?;
    Ok(Json(FileResponse { data }))
}
async fn cancel_upload(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<StatusCode, FileHttpError> {
    let removed = controller
        .service
        .cancel_upload(
            FileQuery {
                tenant_id: caller.tenant_id,
                file_id: id,
            },
            &caller.user_id,
        )
        .await?;
    removal(removed)
}
async fn delete_file(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<StatusCode, FileHttpError> {
    let removed = controller
        .service
        .delete(FileQuery {
            tenant_id: caller.tenant_id,
            file_id: id,
        })
        .await?;
    removal(removed)
}
fn removal(removed: bool) -> Result<StatusCode, FileHttpError> {
    if !removed {
        return Err(FileDomainError::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn download(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<Response, FileHttpError> {
    let file = controller
        .service
        .download(FileQuery {
            tenant_id: caller.tenant_id,
            file_id: id,
        })
        .await?
        .ok_or(FileDomainError::NotFound)?;
    file_response(file, false)
}
async fn public_image(
    State(controller): State<Arc<FileController>>,
    Extension(caller): Extension<PublicCaller>,
    Path(token): Path<String>,
) -> Result<Response, FileHttpError> {
    let file = controller
        .service
        .open_image(ImageQuery {
            tenant_id: caller.tenant_id,
            token,
        })
        .await?
        .ok_or(FileDomainError::NotFound)?;
    file_response(file, true)
}
fn file_response(file: DownloadObject, public: bool) -> Result<Response, FileHttpError> {
    let mut response = Response::new(file.body);
    let headers = response.headers_mut();
    let mime = if public {
        file.item.content_type.as_str()
    } else {
        "application/octet-stream"
    };
    let disposition = if public {
        inline_disposition(&file.item.name)
    } else {
        content_disposition(&file.item.name)
    };
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime).map_err(anyhow::Error::from)?,
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(anyhow::Error::from)?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&file.item.size_bytes.to_string()).map_err(anyhow::Error::from)?,
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
struct FileHttpError {
    status: StatusCode,
    message: String,
}
impl From<FileDomainError> for FileHttpError {
    fn from(error: FileDomainError) -> Self {
        let status = match &error {
            FileDomainError::Validation(_) => StatusCode::BAD_REQUEST,
            FileDomainError::NotFound => StatusCode::NOT_FOUND,
            FileDomainError::Conflict(_) => StatusCode::CONFLICT,
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
}
impl From<anyhow::Error> for FileHttpError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<FileDomainError>() {
            Ok(error) => error.into(),
            Err(error) => {
                eprintln!("文件服务操作失败: {error:#}");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "文件服务暂时不可用".into(),
                }
            }
        }
    }
}
impl IntoResponse for FileHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(FileErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
