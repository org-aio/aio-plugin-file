use aio_plugin_file_model::FileItem;
use az_dioxus_admin_shell::{PageUrlState, UrlUpdate, use_page_url_state};
use dioxus::prelude::*;

use crate::display::file_category;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileCategory {
    All,
    Image,
    Document,
    Media,
    Archive,
    Application,
    Code,
    Database,
    Other,
}

impl FileCategory {
    pub const ALL: [Self; 9] = [
        Self::All,
        Self::Image,
        Self::Document,
        Self::Media,
        Self::Archive,
        Self::Application,
        Self::Code,
        Self::Database,
        Self::Other,
    ];

    fn from_value(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|category| category.key() == value)
            .unwrap_or(Self::All)
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Image => "image",
            Self::Document => "document",
            Self::Media => "media",
            Self::Archive => "archive",
            Self::Application => "application",
            Self::Code => "code",
            Self::Database => "database",
            Self::Other => "other",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "所有文件",
            Self::Image => "图片",
            Self::Document => "文档",
            Self::Media => "音视频",
            Self::Archive => "压缩包",
            Self::Application => "安装包",
            Self::Code => "代码/文本",
            Self::Database => "数据库/备份",
            Self::Other => "其他",
        }
    }

    pub fn matches(self, file: &FileItem) -> bool {
        self == Self::All || file_category(file) == self.key()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SortMode {
    Newest,
    Name,
    Largest,
}

impl SortMode {
    fn from_value(value: &str) -> Self {
        match value {
            "name" => Self::Name,
            "largest" => Self::Largest,
            _ => Self::Newest,
        }
    }
}

pub(super) struct FileViewState {
    pub url: PageUrlState,
    pub category: FileCategory,
    pub search: String,
    pub sort: SortMode,
}

pub(super) fn use_file_view_state() -> FileViewState {
    let url = use_page_url_state("file-list");
    use_effect(move || {
        let category = url.value("category");
        let sort = url.value("sort");
        let mut corrections = Vec::new();
        if category.as_deref().is_some_and(|value| {
            value == "all"
                || !FileCategory::ALL
                    .iter()
                    .any(|category| category.key() == value)
        }) {
            corrections.push(("category", None));
        }
        if sort
            .as_deref()
            .is_some_and(|value| !matches!(value, "name" | "largest"))
        {
            corrections.push(("sort", None));
        }
        if !corrections.is_empty() {
            url.update(&corrections, UrlUpdate::Replace);
        }
    });
    FileViewState {
        url,
        category: FileCategory::from_value(&url.value("category").unwrap_or_default()),
        search: url.value("q").unwrap_or_default(),
        sort: SortMode::from_value(&url.value("sort").unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_urls_preserve_known_values_and_reject_invalid_values() {
        for category in FileCategory::ALL {
            assert_eq!(FileCategory::from_value(category.key()), category);
        }
        assert_eq!(FileCategory::from_value(""), FileCategory::All);
        assert_eq!(
            FileCategory::from_value("not-a-category"),
            FileCategory::All
        );
        assert_eq!(SortMode::from_value("largest"), SortMode::Largest);
        assert_eq!(SortMode::from_value("name"), SortMode::Name);
        assert_eq!(SortMode::from_value("invalid"), SortMode::Newest);
    }
}
