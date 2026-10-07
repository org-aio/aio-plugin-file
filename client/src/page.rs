use std::collections::BTreeSet;

use crate::{
    clipboard,
    details::FileDetails,
    display::{file_category, format_bytes, image_link},
    http,
    upload_dialog::UploadFileDialog,
    view_state::{FileCategory, SortMode, use_file_view_state},
};
use aio_plugin_file_model::FileItem;
use az_dioxus_admin_shell::UrlUpdate;
use az_ui_components::{
    admin::{
        AsyncResult, DeleteRecordsDialog, PageHeader, PageSurface, RequestState, StatusMessage,
    },
    button::{Button, ButtonSize, ButtonVariant},
    checkbox::{Checkbox, CheckboxState},
    collection_tree::{CollectionTree, CollectionTreeData, CollectionTreeItemContext},
    input::Input,
    select::{Select, SelectItem},
};
use dioxus::html::HasFileData as _;
use dioxus::prelude::{dioxus_elements::FileData, *};
use dioxus_icons::lucide::{
    Archive, Copy, Database, Download, File, FileCode, FileImage, FileText, Files, Film, Folder,
    HardDrive, Image, Info, Package, RefreshCw, Search, Trash2, Upload,
};

const PAGE_STYLES: &str = include_str!("page.css");

#[derive(Clone, Debug, PartialEq)]
struct CategoryEntry {
    category: FileCategory,
    count: usize,
}

#[allow(non_snake_case)]
pub fn FileManagementPage() -> Element {
    let mut revision = use_signal(|| 0_u64);
    let files = use_resource(move || {
        let _ = revision();
        http::list()
    });
    let mut uploading = use_signal(|| false);
    let mut drag_active = use_signal(|| false);
    let drag_uploading = use_signal(|| None::<String>);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut selected_ids = use_signal(BTreeSet::<String>::new);
    let mut active_file = use_signal(|| None::<FileItem>);
    let mut delete_target = use_signal(|| None::<Vec<FileItem>>);
    let mut detail_target = use_signal(|| None::<FileItem>);
    let view = use_file_view_state();
    let url = view.url;
    let selected_category = view.category;
    let search = view.search;
    let sort_mode = view.sort;

    let result = files.read().as_ref().cloned();
    let listing = result.as_ref().and_then(|result| result.as_ref().ok());
    let count = listing.map_or(0, |listing| listing.files.len());
    let bytes = listing.map_or(0, |listing| {
        listing.files.iter().map(|file| file.size_bytes).sum()
    });
    let maximum = listing.map_or(0, |listing| listing.max_file_bytes);
    let all_files = listing.map_or_else(Vec::new, |listing| listing.files.clone());
    let visible_files = visible_files(&all_files, selected_category, &search, sort_mode);
    let categories = category_entries(&all_files);
    let selected_files = selected_files(&all_files, &selected_ids());
    let delete_selection = if selected_files.is_empty() {
        active_file().into_iter().collect::<Vec<_>>()
    } else {
        selected_files
    };
    let visible_count = visible_files.len();
    let selected_count = selected_ids().len();
    let active_id = active_file().map(|file| file.id);
    let keyboard_files = visible_files.clone();

    rsx! {
        document::Style { {PAGE_STYLES} }
        PageSurface {
            PageHeader {
                title: "文件管理",
                detail: format!("{count} 个文件 · {}", format_bytes(bytes)),
                Button {
                    size: ButtonSize::Icon,
                    variant: ButtonVariant::Outline,
                    aria_label: "刷新文件",
                    title: "刷新文件",
                    onclick: move |_| revision += 1,
                    RefreshCw {}
                }
                Button {
                    disabled: listing.is_none() || drag_uploading().is_some(),
                    onclick: move |_| uploading.set(true),
                    Upload {}
                    "上传文件"
                }
            }
            if let Some(message) = status() {
                StatusMessage { error: message.is_err(), message: message.unwrap_or_else(|message| message) }
            }
            match result {
                Some(Ok(_)) => rsx! {
                    section {
                        class: "file-browser",
                        "data-drag-active": drag_active().to_string(),
                        ondragenter: move |event: DragEvent| {
                            event.prevent_default();
                            drag_active.set(true);
                        },
                        ondragover: move |event: DragEvent| {
                            event.prevent_default();
                            event.data_transfer().set_drop_effect("copy");
                        },
                        ondragleave: move |event: DragEvent| {
                            event.prevent_default();
                            drag_active.set(false);
                        },
                        ondrop: move |event: DragEvent| {
                            event.prevent_default();
                            drag_active.set(false);
                            if drag_uploading().is_some() {
                                return;
                            }
                            let dropped = event.files();
                            if dropped.is_empty() {
                                status.set(Some(Err("没有检测到可上传的文件".to_owned())));
                                return;
                            }
                            upload_dropped_files(dropped, maximum, drag_uploading, status, revision);
                        },
                        aside { class: "file-browser__sidebar",
                            div { class: "file-browser__panel-heading",
                                div {
                                    span { class: "file-browser__eyebrow", "分类" }
                                    strong { "资料库" }
                                }
                                HardDrive { class: "file-browser__heading-icon" }
                            }
                            CollectionTree {
                                aria_label: "文件分类".to_owned(),
                                class: "file-browser__categories".to_owned(),
                                data: CollectionTreeData::Collection(categories),
                                item_key: |entry: CategoryEntry| entry.category.key().to_owned(),
                                selected_key: Some(selected_category.key().to_owned()),
                                on_select: move |entry: CategoryEntry| {
                                    url.update(&[("category", (entry.category != FileCategory::All).then(|| entry.category.key().to_owned()))], UrlUpdate::Push);
                                    selected_ids.set(BTreeSet::new());
                                    active_file.set(None);
                                },
                                render_item: move |context: CollectionTreeItemContext<CategoryEntry>| rsx! {
                                    CategoryRow { entry: context.item }
                                },
                            }
                            div { class: "file-browser__capacity",
                                span { "已存储" }
                                strong { "{format_bytes(bytes)}" }
                                small { "单个文件上限 {format_bytes(maximum)}" }
                            }
                        }
                        main {
                            class: "file-browser__files",
                            tabindex: "0",
                            aria_label: "文件列表，使用上下方向键移动",
                            onkeydown: move |event: KeyboardEvent| {
                                handle_file_keyboard(
                                    event,
                                    &keyboard_files,
                                    active_file,
                                    selected_ids,
                                    detail_target,
                                    delete_target,
                                );
                            },
                            header { class: "file-browser__toolbar",
                                label { class: "file-browser__search",
                                    Search {}
                                    Input {
                                        r#type: "search",
                                        aria_label: "搜索文件",
                                        placeholder: "搜索名称或类型",
                                        value: search.clone(),
                                        oninput: move |event: FormEvent| {
                                            let value = event.value();
                                            url.update(&[("q", (!value.is_empty()).then_some(value))], UrlUpdate::Continuous);
                                        },
                                    }
                                }
                                Select {
                                    aria_label: "文件排序",
                                    value: match sort_mode {
                                        SortMode::Newest => "newest",
                                        SortMode::Name => "name",
                                        SortMode::Largest => "largest",
                                    },
                                    options: [
                                        SelectItem::new("newest", "最新上传"),
                                        SelectItem::new("name", "按名称"),
                                        SelectItem::new("largest", "按大小"),
                                    ].to_vec(),
                                    on_value_change: move |value: String| url.update(&[("sort", (value != "newest").then_some(value))], UrlUpdate::Push),
                                }
                            }
                            div { class: "file-browser__list-heading",
                                div {
                                    strong { "{selected_category.label()}" }
                                    span { "{visible_count} 项" }
                                }
                                if selected_count > 0 {
                                    div { class: "admin-actions",
                                        span { class: "file-browser__selection", "已选择 {selected_count} 项" }
                                        Button {
                                            size: ButtonSize::Sm,
                                            variant: ButtonVariant::Ghost,
                                            onclick: move |_| selected_ids.set(BTreeSet::new()),
                                            "取消选择"
                                        }
                                        Button {
                                            size: ButtonSize::Sm,
                                            variant: ButtonVariant::Outline,
                                            onclick: move |_| delete_target.set(Some(delete_selection.clone())),
                                            Trash2 {}
                                            "删除"
                                        }
                                    }
                                }
                            }
                            if visible_files.is_empty() {
                                FileEmptyState {
                                    searching: !search.trim().is_empty(),
                                    on_upload: move |_| uploading.set(true),
                                }
                            } else {
                                div { class: "file-browser__list", role: "listbox", aria_label: "文件", "data-url-scroll": "files-list",
                                    for file in visible_files {
                                        FileRow {
                                            key: "{file.id}",
                                            active: active_id.as_deref() == Some(file.id.as_str()),
                                            checked: selected_ids().contains(&file.id),
                                            file,
                                            on_activate: move |file: FileItem| active_file.set(Some(file)),
                                            on_check: move |(file_id, checked): (String, bool)| {
                                                let mut ids = selected_ids();
                                                if checked {
                                                    ids.insert(file_id);
                                                } else {
                                                    ids.remove(&file_id);
                                                }
                                                selected_ids.set(ids);
                                            },
                                        }
                                    }
                                }
                            }
                        }
                        aside { class: "file-browser__preview",
                            FilePreview {
                                file: active_file(),
                                on_copy: move |file: FileItem| copy_image_link(file, status),
                                on_download: move |file: FileItem| {
                                    if let Err(message) = http::download(&file.id) {
                                        status.set(Some(Err(message)));
                                    }
                                },
                                on_details: move |file: FileItem| detail_target.set(Some(file)),
                                on_delete: move |file: FileItem| delete_target.set(Some(vec![file])),
                            }
                        }
                        if drag_active() || drag_uploading().is_some() {
                            div { class: "file-browser__drop-overlay", role: "status",
                                Upload {}
                                if let Some(name) = drag_uploading() {
                                    strong { "正在上传 {name}" }
                                    span { "请保持当前页面打开" }
                                } else {
                                    strong { "松开以上传文件" }
                                    span { "支持拖入一个或多个文件" }
                                }
                            }
                        }
                    }
                },
                Some(Err(error)) => rsx! { RequestState { error, on_retry: move |_| revision += 1 } },
                None => rsx! { RequestState {} },
            }
        }
        if uploading() {
            UploadFileDialog {
                max_file_bytes: maximum,
                on_close: move |_| uploading.set(false),
                on_uploaded: move |file: FileItem| {
                    uploading.set(false);
                    active_file.set(Some(file.clone()));
                    status.set(Some(Ok(format!("已上传 {}", file.name))));
                    revision += 1;
                },
            }
        }
        if let Some(files) = delete_target() {
            DeleteRecordsDialog {
                title: "删除文件",
                items: files,
                item_label: |file: FileItem| file.name,
                delete: |file: FileItem| -> AsyncResult<()> {
                    Box::pin(async move { http::delete(&file.id).await })
                },
                on_close: move |_| delete_target.set(None),
                on_deleted: move |count| {
                    selected_ids.set(BTreeSet::new());
                    active_file.set(None);
                    status.set(Some(Ok(format!("已删除 {count} 个文件"))));
                    revision += 1;
                },
            }
        }
        if let Some(file) = detail_target() {
            FileDetails { file, on_close: move |_| detail_target.set(None) }
        }
    }
}

#[component]
fn CategoryRow(entry: CategoryEntry) -> Element {
    rsx! {
        div { class: "file-browser__category-row",
            span { class: "file-browser__category-label",
                match entry.category {
                    FileCategory::All => rsx! { Files {} },
                    FileCategory::Image => rsx! { Image {} },
                    FileCategory::Document => rsx! { FileText {} },
                    FileCategory::Media => rsx! { Film {} },
                    FileCategory::Archive => rsx! { Archive {} },
                    FileCategory::Application => rsx! { Package {} },
                    FileCategory::Code => rsx! { FileCode {} },
                    FileCategory::Database => rsx! { Database {} },
                    FileCategory::Other => rsx! { File {} },
                }
                "{entry.category.label()}"
            }
            span { class: "file-browser__count", "{entry.count}" }
        }
    }
}

#[component]
fn FileRow(
    file: FileItem,
    active: bool,
    checked: bool,
    on_activate: Callback<FileItem>,
    on_check: Callback<(String, bool)>,
) -> Element {
    let file_for_open = file.clone();
    let file_id = file.id.clone();
    rsx! {
        article {
            class: "file-browser__row",
            "data-active": active.to_string(),
            role: "option",
            aria_selected: active.to_string(),
            Checkbox {
                aria_label: "选择 {file.name}",
                checked: Some(if checked { CheckboxState::Checked } else { CheckboxState::Unchecked }),
                on_checked_change: move |state| on_check.call((file_id.clone(), bool::from(state))),
            }
            button {
                r#type: "button",
                class: "file-browser__row-main",
                onclick: move |_| on_activate.call(file_for_open.clone()),
                div { class: "file-browser__file-icon", "data-kind": file_category(&file),
                    FileKindIcon { file: file.clone() }
                }
                div { class: "file-browser__file-copy",
                    strong { title: "{file.name}", "{file.name}" }
                    span { "{file.content_type}" }
                }
                div { class: "file-browser__row-meta",
                    span { "{format_bytes(file.size_bytes)}" }
                    time { datetime: file.created_at.clone(), "{file.created_at}" }
                }
            }
        }
    }
}

#[component]
fn FileKindIcon(file: FileItem) -> Element {
    match file_category(&file) {
        "image" => rsx! { FileImage {} },
        "document" => rsx! { FileText {} },
        "media" => rsx! { Film {} },
        "archive" => rsx! { Archive {} },
        "application" => rsx! { Package {} },
        "code" => rsx! { FileCode {} },
        "database" => rsx! { Database {} },
        _ => rsx! { File {} },
    }
}

#[component]
fn FilePreview(
    file: Option<FileItem>,
    on_copy: Callback<FileItem>,
    on_download: Callback<FileItem>,
    on_details: Callback<FileItem>,
    on_delete: Callback<FileItem>,
) -> Element {
    let Some(file) = file else {
        return rsx! {
            div { class: "file-browser__preview-empty",
                div { class: "file-browser__empty-icon", Info {} }
                strong { "选择文件以预览" }
                span { "可查看文件信息、下载或复制图床链接" }
            }
        };
    };
    let link = image_link(&file);
    rsx! {
        header { class: "file-browser__preview-heading",
            span { class: "file-browser__eyebrow", "预览" }
            strong { title: "{file.name}", "{file.name}" }
        }
        div { class: "file-browser__preview-stage",
            if let Some(link) = link.clone() {
                img { src: link.url, alt: "{file.name}" }
            } else {
                div { class: "file-browser__preview-file",
                    FileKindIcon { file: file.clone() }
                    span { "{file.content_type}" }
                }
            }
        }
        div { class: "file-browser__preview-actions",
            Button {
                variant: ButtonVariant::Outline,
                onclick: { let file = file.clone(); move |_| on_download.call(file.clone()) },
                Download {}
                "下载"
            }
            if link.is_some() {
                Button {
                    variant: ButtonVariant::Outline,
                    onclick: { let file = file.clone(); move |_| on_copy.call(file.clone()) },
                    Copy {}
                    "复制图床链接"
                }
            }
        }
        dl { class: "file-browser__metadata",
            dt { "类型" }
            dd { "{file.content_type}" }
            dt { "大小" }
            dd { "{format_bytes(file.size_bytes)}" }
            dt { "上传时间" }
            dd { "{file.created_at}" }
            dt { "上传者" }
            dd { "{file.uploaded_by}" }
        }
        footer { class: "file-browser__preview-footer",
            Button {
                size: ButtonSize::Sm,
                variant: ButtonVariant::Ghost,
                onclick: { let file = file.clone(); move |_| on_details.call(file.clone()) },
                Info {}
                "完整信息"
            }
            Button {
                size: ButtonSize::IconSm,
                variant: ButtonVariant::Ghost,
                title: "删除 {file.name}",
                aria_label: "删除 {file.name}",
                onclick: { let file = file.clone(); move |_| on_delete.call(file.clone()) },
                Trash2 {}
            }
        }
    }
}

#[component]
fn FileEmptyState(searching: bool, on_upload: Callback<MouseEvent>) -> Element {
    rsx! {
        div { class: "file-browser__empty",
            div { class: "file-browser__empty-icon",
                if searching { Search {} } else { Folder {} }
            }
            strong { if searching { "没有匹配的文件" } else { "这里还没有文件" } }
            span {
                if searching { "换个关键词，或切换左侧分类" } else { "上传文件后即可在这里统一管理" }
            }
            if !searching {
                Button { onclick: move |event| on_upload.call(event), Upload {} "上传文件" }
            }
        }
    }
}

fn category_entries(files: &[FileItem]) -> Vec<CategoryEntry> {
    FileCategory::ALL
        .into_iter()
        .map(|category| CategoryEntry {
            category,
            count: files.iter().filter(|file| category.matches(file)).count(),
        })
        .collect()
}

fn visible_files(
    files: &[FileItem],
    selected_category: FileCategory,
    search: &str,
    sort_mode: SortMode,
) -> Vec<FileItem> {
    let query = search.trim().to_lowercase();
    let mut visible = files
        .iter()
        .filter(|file| {
            selected_category.matches(file)
                && (query.is_empty()
                    || file.name.to_lowercase().contains(&query)
                    || file.content_type.to_lowercase().contains(&query))
        })
        .cloned()
        .collect::<Vec<_>>();
    visible.sort_by(|left, right| match sort_mode {
        SortMode::Newest => right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
        SortMode::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        SortMode::Largest => right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
    });
    visible
}

fn selected_files(files: &[FileItem], selected_ids: &BTreeSet<String>) -> Vec<FileItem> {
    files
        .iter()
        .filter(|file| selected_ids.contains(&file.id))
        .cloned()
        .collect()
}

fn handle_file_keyboard(
    event: KeyboardEvent,
    visible_files: &[FileItem],
    mut active_file: Signal<Option<FileItem>>,
    mut selected_ids: Signal<BTreeSet<String>>,
    mut detail_target: Signal<Option<FileItem>>,
    mut delete_target: Signal<Option<Vec<FileItem>>>,
) {
    if visible_files.is_empty() {
        return;
    }
    let current_index =
        active_file().and_then(|active| visible_files.iter().position(|file| file.id == active.id));
    let next_index = match event.key() {
        Key::ArrowDown => {
            Some(current_index.map_or(0, |index| (index + 1).min(visible_files.len() - 1)))
        }
        Key::ArrowUp => {
            Some(current_index.map_or(visible_files.len() - 1, |index| index.saturating_sub(1)))
        }
        Key::Home => Some(0),
        Key::End => Some(visible_files.len() - 1),
        Key::Enter => {
            if let Some(file) = active_file() {
                event.prevent_default();
                detail_target.set(Some(file));
            }
            None
        }
        Key::Delete | Key::Backspace => {
            if let Some(file) = active_file() {
                event.prevent_default();
                delete_target.set(Some(vec![file]));
            }
            None
        }
        Key::Character(ref value) if value == " " => {
            if let Some(file) = active_file() {
                event.prevent_default();
                let mut ids = selected_ids();
                if !ids.insert(file.id.clone()) {
                    ids.remove(&file.id);
                }
                selected_ids.set(ids);
            }
            None
        }
        _ => None,
    };
    if let Some(index) = next_index {
        event.prevent_default();
        active_file.set(Some(visible_files[index].clone()));
    }
}

fn copy_image_link(file: FileItem, mut status: Signal<Option<Result<String, String>>>) {
    let Some(link) = image_link(&file) else {
        status.set(Some(Err("该文件没有图床链接".to_owned())));
        return;
    };
    spawn(async move {
        match clipboard::write_text(link.markdown).await {
            Ok(()) => status.set(Some(Ok(format!("已复制图床链接：{}", file.name)))),
            Err(message) => status.set(Some(Err(message))),
        }
    });
}

fn upload_dropped_files(
    files: Vec<FileData>,
    max_file_bytes: u64,
    mut drag_uploading: Signal<Option<String>>,
    mut status: Signal<Option<Result<String, String>>>,
    mut revision: Signal<u64>,
) {
    spawn(async move {
        let total = files.len();
        let mut uploaded = 0_usize;
        for file in files {
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
            match http::upload(web_file, |_| {}).await {
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
        status.set(Some(Ok(format!("已上传 {uploaded} 个文件"))));
        revision += 1;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        id: &str,
        name: &str,
        content_type: &str,
        size_bytes: u64,
        created_at: &str,
    ) -> FileItem {
        FileItem {
            id: id.to_owned(),
            name: name.to_owned(),
            content_type: content_type.to_owned(),
            size_bytes,
            sha256: String::new(),
            uploaded_by: String::new(),
            created_at: created_at.to_owned(),
            image_token: None,
        }
    }

    fn entry(entries: &[CategoryEntry], category: FileCategory) -> usize {
        entries
            .iter()
            .find(|entry| entry.category == category)
            .map(|entry| entry.count)
            .unwrap()
    }

    #[test]
    fn category_entries_cover_all_files() {
        let files = vec![
            file("1", "cover.png", "image/png", 20, "2026-10-02"),
            file("2", "guide.pdf", "application/pdf", 10, "2026-10-01"),
        ];
        let entries = category_entries(&files);
        assert_eq!(entry(&entries, FileCategory::All), 2);
        assert_eq!(entry(&entries, FileCategory::Image), 1);
        assert_eq!(entry(&entries, FileCategory::Document), 1);
        assert_eq!(entry(&entries, FileCategory::Archive), 0);
    }

    #[test]
    fn extensions_drive_the_visible_categories() {
        // 复现线上截图：apk 上报 x-www-form-urlencoded、sql 上报 octet-stream，
        // 仍应分别归入「安装包」与「数据库/备份」而不是「其他」。
        let files = vec![
            file(
                "1",
                "千寻.apk",
                "application/x-www-form-urlencoded",
                8,
                "2026-10-03",
            ),
            file(
                "2",
                "野草助手.apk",
                "application/x-www-form-urlencoded",
                9,
                "2026-10-03",
            ),
            file(
                "3",
                "blinko_pg.sql",
                "application/octet-stream",
                5,
                "2026-10-02",
            ),
            file(
                "4",
                "boxun_sit.sql",
                "application/octet-stream",
                6,
                "2026-09-23",
            ),
        ];
        let entries = category_entries(&files);
        assert_eq!(entry(&entries, FileCategory::Other), 0);
        assert_eq!(entry(&entries, FileCategory::Application), 2);
        assert_eq!(entry(&entries, FileCategory::Database), 2);
        let applications = visible_files(&files, FileCategory::Application, "", SortMode::Newest);
        assert_eq!(applications.len(), 2);
        assert!(
            applications
                .iter()
                .all(|file| file_category(file) == "application")
        );
    }

    #[test]
    fn visible_files_filter_and_sort_without_mutating_source() {
        let files = vec![
            file("1", "small.png", "image/png", 20, "2026-10-01"),
            file("2", "Large.PNG", "image/png", 40, "2026-10-02"),
            file("3", "notes.txt", "text/plain", 80, "2026-10-03"),
        ];
        let visible = visible_files(&files, FileCategory::Image, "png", SortMode::Largest);
        assert_eq!(
            visible
                .iter()
                .map(|file| file.id.as_str())
                .collect::<Vec<_>>(),
            ["2", "1"]
        );
        assert_eq!(files[0].id, "1");
    }
}
