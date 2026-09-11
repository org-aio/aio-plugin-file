use crate::{
    details::FileDetails,
    display::{category, format_bytes},
    http,
    upload_dialog::UploadFileDialog,
};
use aio_plugin_file_model::FileItem;
use az_ui_components::{
    admin::{
        AsyncResult, CollectionTable, DeleteRecordsDialog, PageHeader, PageSurface, RequestState,
        SortValue, StatusMessage,
    },
    button::{Button, ButtonSize, ButtonVariant},
    data_table::{DataTableAlign, DataTableCellContext, DataTableColumn},
    select::{Select, SelectItem},
};
use dioxus::prelude::*;
use dioxus_icons::lucide::{Download, File, RefreshCw, Trash2, Upload};

#[allow(non_snake_case)]
pub fn FileManagementPage() -> Element {
    let mut revision = use_signal(|| 0_u64);
    let files = use_resource(move || {
        let _ = revision();
        http::list()
    });
    let mut uploading = use_signal(|| false);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut selected = use_signal(Vec::<FileItem>::new);
    let mut delete_target = use_signal(|| None::<Vec<FileItem>>);
    let mut detail_target = use_signal(|| None::<FileItem>);
    let mut file_type = use_signal(|| "all".to_owned());
    let result = files.read().as_ref().cloned();
    let listing = result.as_ref().and_then(|result| result.as_ref().ok());
    let count = listing.map_or(0, |listing| listing.files.len());
    let bytes = listing.map_or(0, |listing| {
        listing.files.iter().map(|file| file.size_bytes).sum()
    });
    let maximum = listing.map_or(0, |listing| listing.max_file_bytes);
    rsx! {
        PageSurface {
            PageHeader { title: "文件管理", detail: format!("{count} 个文件 · {}", format_bytes(bytes)),
                Button { size: ButtonSize::Icon, variant: ButtonVariant::Outline, aria_label: "刷新文件", title: "刷新文件", onclick: move |_| revision += 1, RefreshCw {} }
                Button { disabled: listing.is_none(), onclick: move |_| uploading.set(true), Upload {} "上传文件" }
            }
            if let Some(message) = status() { StatusMessage { error: message.is_err(), message: message.unwrap_or_else(|message| message) } }
            match result {
                Some(Ok(listing)) => rsx! {
                    CollectionTable {
                        key: "{file_type}", label: "文件",
                        rows: listing.files.into_iter().filter(|file| file_type() == "all" || category(&file.content_type) == file_type()).collect::<Vec<_>>(),
                        columns: columns(), row_key: |file: FileItem| file.id,
                        search_text: |file: FileItem| format!("{} {}", file.name, file.content_type),
                        sort_value: |(file, key): (FileItem, String)| match key.as_str() {
                            "size" => SortValue::Number(file.size_bytes.into()),
                            "created" => SortValue::Text(file.created_at),
                            _ => SortValue::Text(file.name.to_lowercase()),
                        },
                        sortable: vec!["name".into(), "size".into(), "created".into()],
                        selected_keys: selected().iter().map(|file| file.id.clone()).collect::<std::collections::BTreeSet<_>>(),
                        on_selection_change: move |value| selected.set(value), empty_text: "暂无文件".to_owned(),
                        tools: rsx! {
                            label { class: "admin-filter", "文件类型"
                                Select { aria_label: "文件类型", value: file_type(),
                                    options: [("all", "全部类型"), ("image", "图片"), ("document", "文档"), ("media", "音视频"), ("other", "其他")].into_iter().map(|(id, label)| SelectItem::new(id, label)).collect(),
                                    on_value_change: move |value| { file_type.set(value); selected.set(Vec::new()); },
                                }
                            }
                            if !selected().is_empty() {
                                Button { variant: ButtonVariant::Outline, onclick: move |_| delete_target.set(Some(selected())), Trash2 {} "删除选中 ({selected().len()})" }
                            }
                        },
                        render_cell: move |cell: DataTableCellContext<FileItem>| match cell.column.key.as_str() {
                            "name" => { let item = cell.row.clone(); rsx! { div { class: "admin-row-title", File {} button { r#type: "button", onclick: move |_| detail_target.set(Some(item.clone())), "{cell.row.name}" } } } },
                            "type" => rsx! { span { class: "admin-meta", "{cell.row.content_type}" } },
                            "size" => rsx! { "{format_bytes(cell.row.size_bytes)}" },
                            "created" => rsx! { time { datetime: cell.row.created_at.clone(), "{cell.row.created_at}" } },
                            "actions" => {
                                let download_id = cell.row.id.clone();
                                let item = cell.row.clone();
                                rsx! { div { class: "admin-actions",
                                    Button { size: ButtonSize::IconSm, variant: ButtonVariant::Ghost, title: "下载 {cell.row.name}", aria_label: "下载 {cell.row.name}",
                                        onclick: move |_| { if let Err(message) = http::download(&download_id) { status.set(Some(Err(message))); } }, Download {} }
                                    Button { size: ButtonSize::IconSm, variant: ButtonVariant::Ghost, title: "删除 {cell.row.name}", aria_label: "删除 {cell.row.name}",
                                        onclick: move |_| delete_target.set(Some(vec![item.clone()])), Trash2 {} }
                                } }
                            },
                            _ => rsx! {},
                        },
                    }
                },
                Some(Err(error)) => rsx! { RequestState { error, on_retry: move |_| revision += 1 } },
                None => rsx! { RequestState {} },
            }
        }
        if uploading() {
            UploadFileDialog { max_file_bytes: maximum, on_close: move |_| uploading.set(false),
                on_uploaded: move |file: FileItem| { uploading.set(false); status.set(Some(Ok(format!("已上传 {}", file.name)))); revision += 1; },
            }
        }
        if let Some(files) = delete_target() {
            DeleteRecordsDialog { title: "删除文件", items: files, item_label: |file: FileItem| file.name,
                delete: |file: FileItem| -> AsyncResult<()> { Box::pin(async move { http::delete(&file.id).await }) },
                on_close: move |_| delete_target.set(None),
                on_deleted: move |count| { selected.set(Vec::new()); status.set(Some(Ok(format!("已删除 {count} 个文件")))); revision += 1; },
            }
        }
        if let Some(file) = detail_target() { FileDetails { file, on_close: move |_| detail_target.set(None) } }
    }
}

fn columns() -> Vec<DataTableColumn> {
    vec![
        DataTableColumn::leaf("name", "文件名").width(280),
        DataTableColumn::leaf("type", "类型").width(180),
        DataTableColumn::leaf("size", "大小")
            .width(110)
            .align(DataTableAlign::End),
        DataTableColumn::leaf("created", "上传时间").width(210),
        DataTableColumn::leaf("actions", "操作").width(100),
    ]
}
