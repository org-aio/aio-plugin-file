use crate::{
    clipboard,
    display::{format_bytes, image_link},
    http,
};
use aio_plugin_file_model::{FileItem, ImageLink};
use az_ui_components::{
    admin::StatusMessage,
    button::{Button, ButtonSize, ButtonVariant},
    dialog::{Dialog, DialogDescription, DialogTitle},
    input::Input,
};
use dioxus::prelude::*;
use dioxus_icons::lucide::{Copy, Download};

#[component]
pub(crate) fn FileDetails(file: FileItem, on_close: Callback<()>) -> Element {
    let mut error = use_signal(|| None::<String>);
    let copied = use_signal(|| None::<String>);
    let link = image_link(&file);
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
            if let Some(link) = link.clone() { ImageLinkSection { link, copied, error } }
            if let Some(message) = error() { StatusMessage { error: true, message } }
            footer { class: "admin-form-footer",
                Button { variant: ButtonVariant::Outline, onclick: move |_| on_close.call(()), "关闭" }
                Button { onclick: move |_| { if let Err(message) = http::download(&file.id) { error.set(Some(message)); } }, Download {} "下载文件" }
            }
        }
    }
}

#[component]
fn ImageLinkSection(
    link: ImageLink,
    copied: Signal<Option<String>>,
    error: Signal<Option<String>>,
) -> Element {
    rsx! {
        section { class: "admin-section", h2 { "图床链接" }
            p { class: "admin-meta", "公开图片地址，可粘贴到站外 Markdown、HTML 或论坛。" }
            ImageLinkField { label: "图片地址", value: link.url.clone(), copied, error }
            ImageLinkField { label: "Markdown", value: link.markdown.clone(), copied, error }
            ImageLinkField { label: "HTML", value: link.html.clone(), copied, error }
            ImageLinkField { label: "BBCode", value: link.bbcode.clone(), copied, error }
        }
    }
}

#[component]
fn ImageLinkField(
    label: &'static str,
    value: String,
    copied: Signal<Option<String>>,
    error: Signal<Option<String>>,
) -> Element {
    let field_label = format!("{label}链接");
    let clipboard_value = value.clone();
    rsx! {
        label { class: "admin-field",
            span { "{label}" }
            div { class: "admin-actions",
                Input {
                    aria_label: field_label,
                    readonly: true,
                    value: value.clone(),
                }
                Button {
                    size: ButtonSize::IconSm,
                    variant: ButtonVariant::Outline,
                    title: "复制{label}",
                    aria_label: "复制{label}",
                    onclick: move |_| {
                        let value = clipboard_value.clone();
                        spawn(async move {
                            match clipboard::write_text(value).await {
                                Ok(()) => copied.set(Some(label.to_owned())),
                                Err(message) => error.set(Some(message)),
                            }
                        });
                    },
                    Copy {}
                }
            }
            if copied() == Some(label.to_owned()) { span { class: "admin-meta", "已复制" } }
        }
    }
}
