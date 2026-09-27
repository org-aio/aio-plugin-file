use std::{cell::RefCell, rc::Rc};

use aio_plugin_file_model::{FileErrorResponse, FileItem, FileListing, FileResponse};
use futures_channel::oneshot;
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};

type UploadCompletion = Result<(u16, String), String>;
type UploadSender = Rc<RefCell<Option<oneshot::Sender<UploadCompletion>>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadProgress {
    pub loaded: u64,
    pub total: u64,
    pub percent: u8,
}

impl UploadProgress {
    pub fn new(loaded: u64, total: u64) -> Self {
        let percent = if total == 0 {
            0
        } else {
            ((loaded.min(total) as f64 / total as f64) * 100.0).round() as u8
        };
        Self {
            loaded,
            total,
            percent,
        }
    }
}

pub async fn list() -> Result<FileListing, String> {
    let response = gloo_net::http::Request::get("/api/files")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    decode::<FileListing>(response).await
}

pub async fn upload(
    file: web_sys::File,
    mut on_progress: impl FnMut(UploadProgress) + 'static,
) -> Result<FileItem, String> {
    let request = web_sys::XmlHttpRequest::new().map_err(js_error)?;
    let filename = js_sys::encode_uri_component(&file.name());
    let url = format!(
        "/api/files?filename={}",
        filename.as_string().unwrap_or_default()
    );
    request
        .open_with_async("POST", &url, true)
        .map_err(js_error)?;
    let content_type = if file.type_().is_empty() {
        "application/octet-stream".to_owned()
    } else {
        file.type_()
    };
    request
        .set_request_header("Content-Type", &content_type)
        .map_err(js_error)?;

    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));

    let known_total = file.size() as u64;
    let upload = request.upload().map_err(js_error)?;
    let on_progress =
        Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(move |event: web_sys::ProgressEvent| {
            let event_total = event.total().max(0.0) as u64;
            on_progress(UploadProgress::new(
                event.loaded().max(0.0) as u64,
                if event_total == 0 {
                    known_total
                } else {
                    event_total
                },
            ));
        });
    upload.set_onprogress(Some(on_progress.as_ref().unchecked_ref()));

    let loaded_request = request.clone();
    let loaded_sender = sender.clone();
    let on_load = Closure::<dyn FnMut()>::new(move || {
        let status = loaded_request.status().unwrap_or_default();
        let body = loaded_request
            .response_text()
            .ok()
            .flatten()
            .unwrap_or_default();
        finish_upload(&loaded_sender, Ok((status, body)));
    });
    request.set_onload(Some(on_load.as_ref().unchecked_ref()));

    let failed_sender = sender.clone();
    let on_error = Closure::<dyn FnMut()>::new(move || {
        finish_upload(
            &failed_sender,
            Err("上传连接失败，请检查网络后重试".to_owned()),
        );
    });
    request.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    let aborted_sender = sender.clone();
    let on_abort = Closure::<dyn FnMut()>::new(move || {
        finish_upload(&aborted_sender, Err("上传已取消".to_owned()));
    });
    request.set_onabort(Some(on_abort.as_ref().unchecked_ref()));

    request
        .send_with_opt_blob(Some(file.as_ref()))
        .map_err(js_error)?;
    let (status, body) = receiver
        .await
        .map_err(|_| "上传连接意外中断".to_owned())??;
    decode_upload_response(status, body)
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
    Err(decode_error_body(body))
}

fn decode_upload_response(status: u16, body: String) -> Result<FileItem, String> {
    if !(200..300).contains(&status) {
        if body.is_empty() {
            return Err(format!("上传失败（HTTP {status}）"));
        }
        return Err(decode_error_body(body));
    }
    serde_json::from_str::<FileResponse<FileItem>>(&body)
        .map(|response| response.data)
        .map_err(|error| format!("解析上传结果失败：{error}"))
}

fn decode_error_body(body: String) -> String {
    serde_json::from_str::<FileErrorResponse>(&body)
        .map(|response| response.error)
        .unwrap_or(body)
}

fn finish_upload(sender: &UploadSender, result: UploadCompletion) {
    if let Some(sender) = sender.borrow_mut().take() {
        let _ = sender.send(result);
    }
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| "浏览器上传接口调用失败".to_owned())
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
