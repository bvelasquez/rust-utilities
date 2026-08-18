#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pi_commander::run().await
}