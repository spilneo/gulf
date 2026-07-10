#[tokio::main]
async fn main() -> anyhow::Result<()> {
    gulf::cli::run().await
}
