#[tokio::main]
async fn main() -> anyhow::Result<()> {
    soki_ci::run().await
}
