use crate::{display::format_bytes, http};
use aio_plugin_file_model::FileItem;
use az_ui_components::{
    admin::StatusMessage,
    button::{Button, ButtonVariant},
    dialog::{Dialog, DialogDescription, DialogTitle},
};
use dioxus::prelude::*;
use dioxus_icons::lucide::Download;

#[component]
pub(crate) fn FileDetails(file: FileItem, on_close: Callback<()>) -> Element {
    let mut error = use_signal(|| None::<String>);
    rsx! {
        Dialog { open: true, on_open_change: move |open: bool| { if !open { on_close.call(()); } },
            DialogTitle { "文件详情" }
            DialogDescription { "{file.name}" }
            dl { class: "admin-details",
                dt { "类型" } dd { "{file.content_type}" }
                dt { "大小" } dd { "{format_bytes(file.size_bytes)}" }
                dt { "上传时间" } dd { "{file.created_at}" }
                dt { "上传者" } dd { class: "admin-code", "{file.uploaded_by}" }
                dt { "SHA-256" } dd { class: "admin-code", "{file.sha256}" }
                dt { "文件 ID" } dd { class: "admin-code", "{file.id}" }
            }
            if let Some(message) = error() { StatusMessage { error: true, message } }
            footer { class: "admin-form-footer",
                Button { variant: ButtonVariant::Outline, onclick: move |_| on_close.call(()), "关闭" }
                Button { onclick: move |_| { if let Err(message) = http::download(&file.id) { error.set(Some(message)); } }, Download {} "下载文件" }
            }
        }
    }
}
