use aio_plugin_file_model::FileItem;
use az_ui_components::{
    button::{Button, ButtonSize, ButtonVariant},
    data_table::{DataTable, DataTableCellContext, DataTableColumn},
    input::Input,
};
use dioxus::prelude::{dioxus_elements::FileData, *};
use dioxus_icons::lucide::{Download, Trash2, Upload};

use crate::{delete_dialog::DeleteFileDialog, http};

#[allow(non_snake_case)]
pub fn FileManagementPage() -> Element {
    let mut revision = use_signal(|| 0_u64);
    let files = use_resource(move || {
        let _ = revision();
        http::list()
    });
    let mut selected = use_signal(|| None::<FileData>);
    let mut uploading = use_signal(|| false);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut delete_target = use_signal(|| None::<FileItem>);

    let listing = match files.read().as_ref().cloned() {
        Some(Ok(listing)) => listing,
        Some(Err(error)) => {
            return rsx! { p { role: "alert", "加载文件列表失败：{error}" } };
        }
        None => return rsx! { p { "正在读取文件列表" } },
    };
    let selected_label = selected()
        .map(|file| format!("{} · {}", file.name(), format_bytes(file.size())))
        .unwrap_or_else(|| "尚未选择文件".to_owned());
    let maximum_label = format_bytes(listing.max_file_bytes);
    let max_file_bytes = listing.max_file_bytes;
    let input_key = format!("upload-{}", revision());

    rsx! {
        section {
            div { class: "flex items-center justify-between gap-3",
                div {
                    h2 { "文件管理" }
                    p { "上传、下载和删除当前租户的文件。" }
                }
            }
            fieldset { class: "grid gap-3",
                legend { "上传文件" }
                Input {
                    key: "{input_key}",
                    r#type: "file",
                    aria_label: "选择上传文件",
                    disabled: uploading(),
                    onchange: move |event: FormEvent| {
                        selected.set(event.files().into_iter().next());
                        status.set(None);
                    },
                }
                p { "{selected_label}" }
                p { "单个文件最大 {maximum_label}。" }
                Button {
                    r#type: "button",
                    disabled: uploading() || selected().is_none(),
                    onclick: move |_| {
                        let Some(file) = selected() else {
                            return;
                        };
                        if file.size() > max_file_bytes {
                            status.set(Some(Err(format!(
                                "文件超过 {} 限制",
                                format_bytes(max_file_bytes),
                            ))));
                            return;
                        }
                        uploading.set(true);
                        status.set(None);
                        spawn(async move {
                            let result = match file.read_bytes().await {
                                Ok(bytes) => http::upload(
                                    &file.name(),
                                    file.content_type().as_deref(),
                                    bytes.as_ref(),
                                )
                                .await
                                .map(|item| format!("已上传 {}", item.name)),
                                Err(error) => Err(format!("读取本地文件失败：{error}")),
                            };
                            uploading.set(false);
                            if result.is_ok() {
                                selected.set(None);
                                revision.set(revision().wrapping_add(1));
                            }
                            status.set(Some(result));
                        });
                    },
                    Upload { class: "size-4" }
                    if uploading() { "正在上传" } else { "上传" }
                }
                if let Some(result) = status() {
                    match result {
                        Ok(message) => rsx! { p { role: "status", "{message}" } },
                        Err(message) => rsx! { p { role: "alert", "{message}" } },
                    }
                }
            }
            h3 { "当前租户文件" }
            DataTable {
                aria_label: "当前租户文件",
                rows: listing.files,
                columns: columns(),
                row_key: Callback::new(|file: FileItem| file.id),
                empty_text: "当前租户还没有文件".to_owned(),
                render_cell: Callback::new(move |cell: DataTableCellContext<FileItem>| {
                    match cell.column.key.as_str() {
                        "name" => rsx! { span { "{cell.row.name}" } },
                        "type" => rsx! { span { "{cell.row.content_type}" } },
                        "size" => rsx! { span { "{format_bytes(cell.row.size_bytes)}" } },
                        "created" => rsx! { span { "{cell.row.created_at}" } },
                        "actions" => {
                            let download_id = cell.row.id.clone();
                            let delete_file = cell.row.clone();
                            rsx! {
                                div { class: "flex items-center gap-1",
                                    Button {
                                        r#type: "button",
                                        size: ButtonSize::IconSm,
                                        variant: ButtonVariant::Ghost,
                                        title: "下载 {cell.row.name}",
                                        aria_label: "下载 {cell.row.name}",
                                        onclick: move |_| {
                                            if let Err(message) = http::download(&download_id) {
                                                status.set(Some(Err(message)));
                                            }
                                        },
                                        Download { class: "size-4" }
                                    }
                                    Button {
                                        r#type: "button",
                                        size: ButtonSize::IconSm,
                                        variant: ButtonVariant::Ghost,
                                        title: "删除 {cell.row.name}",
                                        aria_label: "删除 {cell.row.name}",
                                        onclick: move |_| delete_target.set(Some(delete_file.clone())),
                                        Trash2 { class: "size-4" }
                                    }
                                }
                            }
                        },
                        _ => rsx! {},
                    }
                }),
            }
        }
        if let Some(file) = delete_target() {
            DeleteFileDialog {
                file,
                on_close: move |_| delete_target.set(None),
                on_deleted: move |_| {
                    delete_target.set(None);
                    status.set(Some(Ok("文件已删除".to_owned())));
                    revision.set(revision().wrapping_add(1));
                },
            }
        }
    }
}

fn columns() -> Vec<DataTableColumn> {
    vec![
        DataTableColumn::leaf("name", "文件名").width(260),
        DataTableColumn::leaf("type", "类型").width(180),
        DataTableColumn::leaf("size", "大小").width(110),
        DataTableColumn::leaf("created", "上传时间").width(190),
        DataTableColumn::leaf("actions", "操作").width(100),
    ]
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_file_sizes_for_the_table() {
        assert_eq!(format_bytes(7), "7 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MiB");
    }
}
