use aio_plugin_file_model::{FileErrorResponse, FileItem, FileListing, FileResponse};
use serde::de::DeserializeOwned;

pub async fn list() -> Result<FileListing, String> {
    let response = gloo_net::http::Request::get("/api/files")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    decode::<FileListing>(response).await
}

pub async fn upload(
    filename: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<FileItem, String> {
    let bytes = js_sys::Uint8Array::from(body);
    let response = gloo_net::http::Request::post("/api/files")
        .query([("filename", filename)])
        .header(
            "Content-Type",
            content_type.unwrap_or("application/octet-stream"),
        )
        .body(bytes)
        .map_err(|error| error.to_string())?
        .send()
        .await
        .map_err(|error| error.to_string())?;
    decode::<FileItem>(response).await
}

pub async fn delete(id: &str) -> Result<(), String> {
    let response = gloo_net::http::Request::delete(&format!("/api/files/{id}"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.ok() {
        Ok(())
    } else {
        decode_error(response).await
    }
}

pub fn download(id: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "当前环境没有浏览器窗口".to_owned())?;
    window
        .location()
        .set_href(&format!("/api/files/{id}"))
        .map_err(|_| "无法打开文件下载".to_owned())
}

async fn decode<T: DeserializeOwned>(response: gloo_net::http::Response) -> Result<T, String> {
    if !response.ok() {
        return decode_error(response).await;
    }
    response
        .json::<FileResponse<T>>()
        .await
        .map(|response| response.data)
        .map_err(|error| error.to_string())
}

async fn decode_error<T>(response: gloo_net::http::Response) -> Result<T, String> {
    let body = response.text().await.unwrap_or_default();
    Err(serde_json::from_str::<FileErrorResponse>(&body)
        .map(|response| response.error)
        .unwrap_or(body))
}
