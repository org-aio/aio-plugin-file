mod clipboard;
mod details;
mod display;
mod http;
mod page;
mod upload_dialog;

use az_dioxus_admin_shell::{
    ApplicationMenuGroup, ApplicationPage, ApplicationPlugin, ApplicationScene,
};
use dill::CatalogBuilder;

#[derive(Debug)]
pub struct FilePlugin;

impl ApplicationPlugin for FilePlugin {
    fn pages(&self) -> Vec<ApplicationPage> {
        vec![ApplicationPage {
            id: "file-list",
            label: "文件列表",
            icon: Some("file_text"),
            scene: ApplicationScene {
                id: "system",
                label: "系统",
            },
            menu_path: vec![
                ApplicationMenuGroup {
                    id: "infrastructure".to_owned(),
                    label: "基础设施".to_owned(),
                    icon: Some("hard_drive".to_owned()),
                },
                ApplicationMenuGroup {
                    id: "file-management".to_owned(),
                    label: "文件管理".to_owned(),
                    icon: Some("folder".to_owned()),
                },
            ],
            required_permission: Some("file:manage"),
            render: page::FileManagementPage,
        }]
    }
}

pub fn register(builder: &mut CatalogBuilder) {
    builder
        .add_value(FilePlugin)
        .bind::<dyn ApplicationPlugin, FilePlugin>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contributes_a_distinct_leaf_under_file_management() {
        let pages = FilePlugin.pages();
        assert_eq!(pages[0].id, "file-list");
        assert_eq!(
            pages[0]
                .menu_path
                .iter()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            ["infrastructure", "file-management"]
        );
        assert_eq!(pages[0].required_permission, Some("file:manage"));
    }
}
