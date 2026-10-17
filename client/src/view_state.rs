use aio_plugin_file_model::FileItem;
use dioxus::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};

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
    pub url: FileUrl,
    pub category: FileCategory,
    pub search: String,
    pub sort: SortMode,
}

pub(super) fn use_file_view_state() -> FileViewState {
    let url = use_file_url();
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

#[derive(Clone, Copy)]
pub(super) enum UrlUpdate {
    Push,
    Replace,
}
#[derive(Clone, Copy)]
pub(super) struct FileUrl {
    fragment: Signal<String>,
    pub error: Signal<Option<String>>,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace=["window","aioPlugin"],js_name=navigate,catch)]
    fn navigate(fragment: &str, options: &JsValue) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace=["window","aioPlugin"],js_name=onNavigationChange,catch)]
    fn subscribe(callback: &js_sys::Function) -> Result<js_sys::Function, JsValue>;
}
struct Subscription {
    unsubscribe: js_sys::Function,
    _listener: Closure<dyn FnMut(String)>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.unsubscribe.call0(&JsValue::UNDEFINED);
    }
}
impl FileUrl {
    pub fn value(self, key: &str) -> Option<String> {
        parameters(&self.fragment.read())
            .into_iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
    pub fn update(self, changes: &[(&str, Option<String>)], mode: UrlUpdate) {
        let mut values = parameters(&self.fragment.read());
        for (name, value) in changes {
            values.retain(|(key, _)| key != name);
            if let Some(value) = value {
                values.push((name.to_string(), value.clone()));
            }
        }
        let target = serialize(values);
        if target == *self.fragment.read() {
            return;
        }
        let options = js_sys::Object::new();
        let mut error = self.error;
        let configured = js_sys::Reflect::set(
            &options,
            &JsValue::from_str("replace"),
            &JsValue::from_bool(matches!(mode, UrlUpdate::Replace)),
        );
        if configured.is_err() {
            error.set(Some("导航参数无效".into()));
            return;
        }
        spawn(async move {
            let result = match navigate(&target, &options) {
                Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await,
                Err(error) => Err(error),
            };
            error.set(result.err().map(|_| "页面导航失败，请重试".into()));
        });
    }
}
fn parameters(fragment: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(fragment.strip_prefix("#?").unwrap_or("").as_bytes())
        .into_owned()
        .collect()
}
fn serialize(values: Vec<(String, String)>) -> String {
    let mut encoder = url::form_urlencoded::Serializer::new(String::new());
    // 只规范化当前页面拥有的参数，保留未知参数及其重复值。
    for (key, value) in &values {
        if !matches!(key.as_str(), "category" | "q" | "sort") {
            encoder.append_pair(key, value);
        }
    }
    for key in ["category", "q", "sort"] {
        let Some((_, value)) = values.iter().rev().find(|(name, _)| name == key) else {
            continue;
        };
        let keep = match key {
            "category" => FileCategory::from_value(value) != FileCategory::All,
            "sort" => matches!(value.as_str(), "name" | "largest"),
            _ => !value.is_empty(),
        };
        if keep {
            encoder.append_pair(key, value);
        }
    }
    let query = encoder.finish();
    if query.is_empty() {
        String::new()
    } else {
        format!("#?{query}")
    }
}
fn use_file_url() -> FileUrl {
    let mut fragment = use_signal(|| {
        web_sys::window()
            .and_then(|window| window.location().hash().ok())
            .unwrap_or_default()
    });
    let mut error = use_signal(|| None::<String>);
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<Subscription>)));
    use_effect(move || {
        if subscription.borrow().is_some() {
            return;
        }
        let listener = Closure::<dyn FnMut(String)>::new(move |value| fragment.set(value));
        match subscribe(listener.as_ref().unchecked_ref()) {
            Ok(unsubscribe) => {
                *subscription.borrow_mut() = Some(Subscription {
                    unsubscribe,
                    _listener: listener,
                })
            }
            Err(_) => error.set(Some("宿主导航桥不可用".into())),
        }
    });
    let state = FileUrl { fragment, error };
    use_effect(move || {
        let raw = fragment();
        if serialize(parameters(&raw)) != raw {
            state.update(&[], UrlUpdate::Replace);
        }
    });
    state
}

#[cfg(test)]
mod url_tests {
    use super::*;
    #[test]
    fn query_round_trip_keeps_other_parameters_and_normalizes_only_owned_values() {
        let value = serialize(parameters(
            "#?extra=a&category=wrong&extra=b&q=%E6%8A%A5%E5%91%8A&sort=name&category=image",
        ));
        assert_eq!(
            value,
            "#?extra=a&extra=b&category=image&q=%E6%8A%A5%E5%91%8A&sort=name"
        );
        assert_eq!(serialize(parameters(&value)), value);
        assert_eq!(serialize(parameters("#?category=all&sort=newest&q=")), "");
    }
}
