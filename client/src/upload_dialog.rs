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
                busy.set(true); error.set(None);
                spawn(async move {
                    let result = match file.read_bytes().await {
                        Ok(bytes) => http::upload(&file.name(), file.content_type().as_deref(), bytes.as_ref()).await,
                        Err(cause) => Err(format!("读取文件失败：{cause}")),
                    };
                    busy.set(false);
                    match result { Ok(item) => on_uploaded.call(item), Err(message) => error.set(Some(message)) }
                });
            },
                label { class: "admin-file-choice",
                    span { "选择文件" }
                    Input { r#type: "file", aria_label: "选择文件", disabled: busy(), onchange: move |event: FormEvent| { selected.set(event.files().into_iter().next()); error.set(None); } }
                    if let Some(file) = selected() { span { class: "admin-meta", "{file.name()} · {format_bytes(file.size())}" } }
                }
                if let Some(message) = error() { StatusMessage { error: true, message } }
                footer { class: "admin-form-footer",
                    Button { r#type: "button", variant: ButtonVariant::Outline, disabled: busy(), onclick: move |_| on_close.call(()), "取消" }
                    Button { r#type: "submit", disabled: busy() || selected().is_none(), Upload {} if busy() { "正在上传" } else { "上传" } }
                }
            }
        }
    }
}
