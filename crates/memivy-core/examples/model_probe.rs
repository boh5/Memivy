//! Probe a model connection using synthetic text without opening a memory store.
use std::time::Duration;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or("model_probe requires a private configuration file path")?;
    let report = memivy_core::model::probe(
        memivy_core::model::ModelConfig::read(std::path::Path::new(path))?,
        Duration::from_secs(90),
    )
    .await?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
