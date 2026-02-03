#[cfg(feature = "server")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    claw_spawn::server::run().await
}
