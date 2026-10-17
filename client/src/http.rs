use aio_plugin_file_model::{
    BeginUpload, FILE_CHUNK_BYTES, FileErrorResponse, FileItem, FileListing, FileResponse,
    UploadSession,
};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

// 直接绑定宿主 SDK，文件字节作为 Uint8Array 传递，不拼接脚本或复制整个文件。
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = json, catch)]
    fn json_call(method: &str, path: &str, body: &JsValue) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = request, catch)]
    fn binary_call(input: &JsValue) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = downloadFrom, catch)]
    fn download_call(path: &str, name: &str) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = copy, catch)]
    fn copy_call(text: &str) -> Result<js_sys::Promise, JsValue>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadProgress {
    pub loaded: u64,
    pub total: u64,
    pub percent: u8,
}
impl UploadProgress {
    pub fn new(loaded: u64, total: u64) -> Self {
        Self {
            loaded,
            total,
            percent: (u128::from(loaded.min(total)) * 100)
                .checked_div(u128::from(total))
                .unwrap_or(0) as u8,
        }
    }
}
async fn json<T: DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<&impl Serialize>,
) -> Result<T, String> {
    let body = body
        .map(|value| value.serialize(&serde_wasm_bindgen::Serializer::json_compatible()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(JsValue::UNDEFINED);
    let response = JsFuture::from(json_call(method, path, &body).map_err(js_error)?)
        .await
        .map_err(js_error)?;
    serde_wasm_bindgen::from_value::<FileResponse<T>>(response)
        .map(|value| value.data)
        .map_err(|error| error.to_string())
}
async fn empty(method: &str, path: &str) -> Result<(), String> {
    JsFuture::from(json_call(method, path, &JsValue::UNDEFINED).map_err(js_error)?)
        .await
        .map_err(js_error)?;
    Ok(())
}
pub async fn list() -> Result<FileListing, String> {
    json::<FileListing>("GET", "/files", None::<&()>).await
}
pub async fn delete(id: &str) -> Result<(), String> {
    empty("DELETE", &format!("/files/{id}")).await
}

pub async fn upload(
    file: web_sys::File,
    mut on_progress: impl FnMut(UploadProgress) + 'static,
    cancelled: impl Fn() -> bool,
) -> Result<FileItem, String> {
    let size = file.size() as u64;
    let content_type = if file.type_().is_empty() {
        "application/octet-stream".into()
    } else {
        file.type_()
    };
    let upload: UploadSession = json(
        "POST",
        "/uploads",
        Some(&BeginUpload {
            name: file.name(),
            content_type,
            size_bytes: size,
        }),
    )
    .await?;
    let result = async {
        if upload.chunk_bytes != FILE_CHUNK_BYTES
            || u64::from(upload.chunk_count) != size.div_ceil(FILE_CHUNK_BYTES)
        {
            return Err("宿主上传契约无效".into());
        }
        for index in 0..upload.chunk_count {
            if cancelled() {
                return Err("上传已取消".into());
            }
            let start = u64::from(index) * FILE_CHUNK_BYTES;
            let end = (start + FILE_CHUNK_BYTES).min(size);
            let slice = file
                .slice_with_f64_and_f64(start as f64, end as f64)
                .map_err(js_error)?;
            let bytes = js_sys::Uint8Array::new(
                &JsFuture::from(slice.array_buffer())
                    .await
                    .map_err(js_error)?,
            );
            let path = format!("/uploads/{}/chunks/{index}", upload.id);
            put_chunk(&path, &bytes, &cancelled).await?;
            on_progress(UploadProgress::new(end, size));
        }
        if cancelled() {
            return Err("上传已取消".into());
        }
        json::<FileItem>(
            "POST",
            &format!("/uploads/{}/complete", upload.id),
            None::<&()>,
        )
        .await
    }
    .await;
    if result.is_err() {
        // 已完成文件不会被取消接口删除；回执丢失时可通过刷新核对是否已完成。
        if let Err(cleanup) = empty("DELETE", &format!("/uploads/{}", upload.id)).await {
            return result
                .map_err(|error| format!("{error}；上传状态未能确认，请刷新核对（{cleanup}）"));
        }
    }
    result
}
async fn put_chunk(
    path: &str,
    bytes: &js_sys::Uint8Array,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    let input = js_sys::Object::new();
    for (key, value) in [
        ("method", JsValue::from_str("PUT")),
        ("path", JsValue::from_str(path)),
        ("body", bytes.clone().into()),
    ] {
        js_sys::Reflect::set(&input, &JsValue::from_str(key), &value).map_err(js_error)?;
    }
    // 摘要在服务端持久化，网络断线后的有限重试只能重复同一块。
    for attempt in 0..3 {
        if cancelled() {
            return Err("上传已取消".into());
        }
        let response = match binary_call(&input) {
            Ok(promise) => JsFuture::from(promise).await,
            Err(error) => Err(error),
        };
        match response {
            Ok(response) => {
                let status = js_sys::Reflect::get(&response, &JsValue::from_str("status"))
                    .map_err(js_error)?
                    .as_f64()
                    .ok_or("上传响应缺少状态")? as u16;
                if (200..300).contains(&status) {
                    return Ok(());
                }
                let body = js_sys::Reflect::get(&response, &JsValue::from_str("body"))
                    .map_err(js_error)?;
                let message = decode_error(&String::from_utf8_lossy(
                    &js_sys::Uint8Array::new(&body).to_vec(),
                ));
                if status < 500 || attempt == 2 {
                    return Err(message);
                }
            }
            Err(error) => {
                if attempt == 2 {
                    return Err(js_error(error));
                }
            }
        }
        gloo_timers::future::TimeoutFuture::new(300 * (attempt + 1)).await;
    }
    Err("上传分块未完成".into())
}
/// 在点击事件内同步发送请求，再异步等待确认，保留宿主对真实手势的验证。
pub fn download(id: &str, name: &str) -> Result<JsFuture, String> {
    download_call(&format!("/files/{id}"), name)
        .map(JsFuture::from)
        .map_err(js_error)
}
pub fn copy(value: &str) -> Result<JsFuture, String> {
    copy_call(value).map(JsFuture::from).map_err(js_error)
}
pub async fn acknowledged(pending: JsFuture) -> Result<(), String> {
    pending.await.map_err(js_error)?;
    Ok(())
}
fn js_error(error: JsValue) -> String {
    let message = error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| "宿主调用失败".into());
    decode_error(&message)
}
fn decode_error(value: &str) -> String {
    serde_json::from_str::<FileErrorResponse>(value)
        .map(|body| body.error)
        .unwrap_or_else(|_| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::UploadProgress;
    #[test]
    fn upload_progress_reports_a_bounded_percentage() {
        assert_eq!(UploadProgress::new(512, 1024).percent, 50);
        assert_eq!(UploadProgress::new(2048, 1024).percent, 100);
        assert_eq!(UploadProgress::new(0, 0).percent, 0);
    }
}
