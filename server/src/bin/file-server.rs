#[tokio::main]
async fn main() -> anyhow::Result<()> {
    aio_plugin_file_server::hosting::run().await
}
