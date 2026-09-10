use std::sync::Arc;

use aio_plugin_file_model::{FileErrorResponse, FileItem, FileListing, FileResponse};
use aio_plugin_identity_server::{IdentityService, SessionContext};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use super::{
    model::{FileQuery, UploadCommand},
    service::FileService,
    util::{FileDomainError, content_disposition},
};

const NOSNIFF: axum::http::HeaderName =
    axum::http::HeaderName::from_static("x-content-type-options");

#[dill::component]
#[dill::scope(dill::Singleton)]
pub struct FileController {
    service: Arc<dyn FileService>,
    identity: Arc<IdentityService>,
}

impl FileController {
    pub fn router(self: Arc<Self>) -> Router {
        let max_file_bytes = self.service.max_file_bytes();
        Router::new()
            .route("/api/plugins/file/health", get(health))
            .route("/api/files", get(list).post(upload))
            .route("/api/files/{id}", get(download).delete(delete_file))
            .layer(DefaultBodyLimit::max(max_file_bytes))
            .with_state(self)
    }

    async fn authenticate_manager(
        &self,
        headers: &HeaderMap,
    ) -> Result<SessionContext, FileHttpError> {
        let session = self
            .identity
            .authenticate(headers)
            .await?
            .ok_or_else(|| FileHttpError::unauthorized("会话无效或已过期"))?;
        if !session
            .permissions
            .iter()
            .any(|permission| permission == "file:manage")
        {
            return Err(FileHttpError::forbidden("当前角色没有文件管理权限"));
        }
        Ok(session)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadQuery {
    filename: String,
}

async fn health(
    State(controller): State<Arc<FileController>>,
) -> Result<&'static str, FileHttpError> {
    controller.service.initialize().await?;
    Ok("ok")
}

async fn list(
    State(controller): State<Arc<FileController>>,
    headers: HeaderMap,
) -> Result<Json<FileResponse<FileListing>>, FileHttpError> {
    let session = controller.authenticate_manager(&headers).await?;
    Ok(Json(FileResponse {
        data: FileListing {
            files: controller.service.list(&session.tenant_id).await?,
            max_file_bytes: controller.service.max_file_bytes() as u64,
        },
    }))
}

async fn upload(
    State(controller): State<Arc<FileController>>,
    headers: HeaderMap,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<FileResponse<FileItem>>), FileHttpError> {
    let session = controller.authenticate_manager(&headers).await?;
    let content_type = headers
        .get(CONTENT_TYPE)
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| FileHttpError::bad_request("Content-Type 无效"))?
        .unwrap_or("application/octet-stream")
        .to_owned();
    let item = controller
        .service
        .upload(UploadCommand {
            tenant_id: session.tenant_id,
            user_id: session.user_id,
            filename: query.filename,
            content_type,
            body,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(FileResponse { data: item })))
}

async fn download(
    State(controller): State<Arc<FileController>>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> Result<Response, FileHttpError> {
    let session = controller.authenticate_manager(&headers).await?;
    let Some(file) = controller
        .service
        .download(FileQuery {
            tenant_id: session.tenant_id,
            file_id,
        })
        .await?
    else {
        return Err(FileHttpError::not_found("文件不存在"));
    };
    let mut response = Response::new(Body::from(file.body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&file.item.content_type).map_err(|_| FileHttpError::internal())?,
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&file.item.name))
            .map_err(|_| FileHttpError::internal())?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&file.item.size_bytes.to_string())
            .map_err(|_| FileHttpError::internal())?,
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(NOSNIFF, HeaderValue::from_static("nosniff"));
    Ok(response)
}

async fn delete_file(
    State(controller): State<Arc<FileController>>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> Result<StatusCode, FileHttpError> {
    let session = controller.authenticate_manager(&headers).await?;
    if controller
        .service
        .delete(FileQuery {
            tenant_id: session.tenant_id,
            file_id,
        })
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(FileHttpError::not_found("文件不存在"))
    }
}

struct FileHttpError {
    status: StatusCode,
    message: String,
}

impl FileHttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "文件服务暂时不可用".to_owned(),
        }
    }
}

impl From<anyhow::Error> for FileHttpError {
    fn from(error: anyhow::Error) -> Self {
        if let Some(error) = error.downcast_ref::<FileDomainError>() {
            Self::bad_request(error.to_string())
        } else {
            Self::internal()
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
