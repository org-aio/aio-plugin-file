mod delete_dialog;
mod http;
mod page;

use az_dioxus_admin_shell::{
    ApplicationMenuGroup, ApplicationPage, ApplicationPlugin, ApplicationScene,
};
use dill::CatalogBuilder;

#[derive(Debug)]
pub struct FilePlugin;

impl ApplicationPlugin for FilePlugin {
    fn pages(&self) -> Vec<ApplicationPage> {
        vec![ApplicationPage {
            id: "file-management",
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
            required_permission: Some("rbac:manage"),
            render: page::FileManagementPage,
        }]
    }
}

pub fn register(builder: &mut CatalogBuilder) {
    builder
        .add_value(FilePlugin)
        .bind::<dyn ApplicationPlugin, FilePlugin>();
}
