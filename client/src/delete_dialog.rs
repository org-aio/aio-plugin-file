use aio_plugin_file_model::FileItem;
use az_ui_components::{
    button::{Button, ButtonVariant},
    dialog::{Dialog, DialogDescription, DialogTitle},
};
use dioxus::prelude::*;
use dioxus_icons::lucide::Trash2;

use crate::http;

#[allow(non_snake_case)]
#[component]
pub fn DeleteFileDialog(
    file: FileItem,
    on_close: EventHandler<()>,
    on_deleted: EventHandler<()>,
) -> Element {
    let mut deleting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    rsx! {
        Dialog {
            open: true,
            on_open_change: move |open: bool| if !open && !deleting() { on_close.call(()) },
            DialogTitle { "删除文件" }
            DialogDescription {
                "将永久删除“{file.name}”，该操作无法撤销。"
            }
            if let Some(message) = error() {
                p { role: "alert", "{message}" }
            }
            footer { class: "flex justify-end gap-2",
                Button {
                    r#type: "button",
                    variant: ButtonVariant::Ghost,
                    disabled: deleting(),
                    onclick: move |_| on_close.call(()),
                    "取消"
                }
                Button {
                    r#type: "button",
                    variant: ButtonVariant::Destructive,
                    disabled: deleting(),
                    onclick: {
                        let id = file.id.clone();
                        move |_| {
                            let id = id.clone();
                            deleting.set(true);
                            error.set(None);
                            spawn(async move {
                                match http::delete(&id).await {
                                    Ok(()) => on_deleted.call(()),
                                    Err(message) => {
                                        deleting.set(false);
                                        error.set(Some(message));
                                    }
                                }
                            });
                        }
                    },
                    Trash2 { class: "size-4" }
                    if deleting() { "正在删除" } else { "确认删除" }
                }
            }
        }
    }
}
