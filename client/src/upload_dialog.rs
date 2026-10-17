use crate::{display::format_bytes, http};
use aio_plugin_file_model::FileItem;
use az_ui_components::{
    admin::StatusMessage,
    button::{Button, ButtonVariant},
    dialog::{Dialog, DialogDescription, DialogTitle},
    input::Input,
};
use dioxus::prelude::{dioxus_elements::FileData, *};
use dioxus_icons::lucide::Upload;

#[component]
pub(crate) fn UploadFileDialog(
    max_file_bytes: u64,
    on_close: Callback<()>,
    on_uploaded: Callback<FileItem>,
) -> Element {
    let mut selected = use_signal(|| None::<FileData>);
    let mut busy = use_signal(|| false);
    let mut cancelled = use_signal(|| false);
    let mut upload_progress = use_signal(|| None::<http::UploadProgress>);
    let mut error = use_signal(|| None::<String>);
    rsx! {
        Dialog { open: true, on_open_change: move |open: bool| { if !open && !busy() { on_close.call(()); } },
            DialogTitle { "上传文件" }
            DialogDescription { "单个文件最大 {format_bytes(max_file_bytes)}" }
            form { class: "admin-form", onsubmit: move |event: FormEvent| {
                event.prevent_default();
                if busy() { return; }
                let Some(file) = selected() else { return; };
                if file.size() > max_file_bytes { error.set(Some(format!("文件过大，请选择不超过 {} 的文件", format_bytes(max_file_bytes)))); return; }
                let Some(web_file) = file.inner().downcast_ref::<web_sys::File>().cloned() else {
                    error.set(Some("当前浏览器无法读取所选文件".to_owned()));
                    return;
                };
                busy.set(true); cancelled.set(false); error.set(None);
                upload_progress.set(Some(http::UploadProgress::new(0, file.size())));
                spawn(async move {
                    let result = http::upload(web_file, move |progress| upload_progress.set(Some(progress)), &*cancelled).await;
                    busy.set(false);
                    match result { Ok(item) => on_uploaded.call(item), Err(message) => error.set(Some(message)) }
                });
            },
                label { class: "admin-file-choice",
                    span { "选择文件" }
                    Input { r#type: "file", aria_label: "选择文件", disabled: busy(), onchange: move |event: FormEvent| { selected.set(event.files().into_iter().next()); error.set(None); } }
                    if let Some(file) = selected() { span { class: "admin-meta", "{file.name()} · {format_bytes(file.size())}" } }
                }
                if let Some(update) = upload_progress() {
                    label { class: "admin-field",
                        span { "上传进度" }
                        progress { max: "100", value: "{update.percent}" }
                        span { class: "admin-meta", "{update.percent}% · {format_bytes(update.loaded)} / {format_bytes(update.total)}" }
                    }
                }
                if let Some(message) = error() { StatusMessage { error: true, message } }
                if busy() && cancelled() { StatusMessage { message: "正在取消，等待当前分块结束并清理上传" } }
                footer { class: "admin-form-footer",
                    Button { r#type: "button", variant: ButtonVariant::Outline, disabled: busy() && cancelled(), onclick: move |_| { if busy() { cancelled.set(true); } else { on_close.call(()); } }, "取消" }
                    Button { r#type: "submit", disabled: busy() || selected().is_none(), Upload {} if busy() { "正在上传" } else { "上传" } }
                }
            }
        }
    }
}

pub(crate) fn upload_dropped_files(
    files: Vec<FileData>,
    max_file_bytes: u64,
    mut drag_uploading: Signal<Option<String>>,
    mut status: Signal<Option<Result<String, String>>>,
    mut revision: Signal<u64>,
    cancelled: Signal<bool>,
) {
    spawn(async move {
        let total = files.len();
        let mut uploaded = 0_usize;
        for file in files {
            if cancelled() {
                break;
            }
            if file.size() > max_file_bytes {
                status.set(Some(Err(format!(
                    "{} 超过单文件上限 {}",
                    file.name(),
                    format_bytes(max_file_bytes)
                ))));
                drag_uploading.set(None);
                return;
            }
            let Some(web_file) = file.inner().downcast_ref::<web_sys::File>().cloned() else {
                status.set(Some(Err(format!("浏览器无法读取 {}", file.name()))));
                drag_uploading.set(None);
                return;
            };
            drag_uploading.set(Some(file.name()));
            match http::upload(web_file, |_| {}, &*cancelled).await {
                Ok(_) => uploaded += 1,
                Err(message) => {
                    status.set(Some(Err(format!(
                        "已上传 {uploaded}/{total} 个文件；{message}"
                    ))));
                    drag_uploading.set(None);
                    if uploaded > 0 {
                        revision += 1;
                    }
                    return;
                }
            }
        }
        drag_uploading.set(None);
        status.set(Some(Ok(format!(
            "已上传 {uploaded} 个文件{}",
            if cancelled() {
                "，其余已取消"
            } else {
                ""
            }
        ))));
        revision += 1;
    });
}
