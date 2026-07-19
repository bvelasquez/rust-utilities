#[tokio::main]
async fn main() -> anyhow::Result<()> {
    mail_sweep::run().await
}
