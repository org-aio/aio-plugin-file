use dioxus::prelude::*;
fn main() {
    dioxus::launch(App);
}
#[component]
fn App() -> Element {
    rsx! {
        az_ui_components::UiStylesheets { relative_paths: true }
        aio_plugin_file_client::FileManagementPage {}
    }
}
