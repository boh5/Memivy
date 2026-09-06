//! Test harness for Phase 1 acceptance. This is not an additional MCP tool.
use memivy_core::{CaptureInput, DataPaths, Store};
use std::{io::Write, time::Duration};
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("model") {
        let path = args
            .get(2)
            .ok_or("model requires a private configuration file path")?;
        let report = memivy_core::model::probe(
            memivy_core::model::ModelConfig::read(std::path::Path::new(path))?,
            Duration::from_secs(90),
        )
        .await?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    let store = Store::open(DataPaths::resolve()?)?;
    match args.get(1).map(String::as_str){
        Some("capture")|Some("hold")=>{
            let text=args.get(2).ok_or("capture requires synthetic test text")?;
            let capture=store.capture(CaptureInput{request_id:args.get(3).cloned().unwrap_or_else(||uuid::Uuid::new_v4().to_string()),text:text.clone(),source_app:"Phase 1 probe".into(),project:None,session_uri:None})?;
            println!("{}",serde_json::to_string(&capture)?);std::io::stdout().flush()?;
            if args[1]=="hold"{std::thread::sleep(Duration::from_secs(60));}
        },
        Some("search")=>println!("{}",serde_json::to_string(&store.search(args.get(2).map(String::as_str).unwrap_or(""),50)?)?),
        Some("mcp-on")=>store.paths.set_mcp_enabled(true)?,
        Some("mcp-off")=>store.paths.set_mcp_enabled(false)?,
        Some("seed")=>{let n:usize=args.get(2).ok_or("seed requires count")?.parse()?;for i in 0..n{store.capture(CaptureInput{request_id:uuid::Uuid::new_v4().to_string(),text:format!("合成样本 {i} 先给结果，再解释配置。example.com/{i} 长中文内容。"),source_app:"benchmark".into(),project:None,session_uri:None})?;}},
        Some("diagnostics")=>println!("{}",serde_json::to_string(&store.diagnostics()?)?),
        _=>return Err("usage: probe diagnostics | capture TEXT [UUID] | hold TEXT | search QUERY | mcp-on | mcp-off | seed COUNT | model PRIVATE_CONFIG".into())
    }
    Ok(())
}
