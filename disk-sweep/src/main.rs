#[tokio::main]
async fn main() -> anyhow::Result<()> {
    disk_sweep::run().await
}
