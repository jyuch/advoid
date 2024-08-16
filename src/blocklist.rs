use rustc_hash::FxHashSet;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

pub async fn get(url: String) -> anyhow::Result<FxHashSet<String>> {
    let payload = if url.starts_with("http") {
        reqwest::get(url).await?.text().await?
    } else {
        let mut f = File::open(url).await?;
        let mut buf = String::new();
        let _ = f.read_to_string(&mut buf).await;
        buf
    };

    let blocklist: FxHashSet<String> = payload
        .lines()
        .map(|it| it.trim().to_string())
        .filter(|it| !it.is_empty())
        .filter(|it| !it.starts_with('#'))
        .map(|it| format!("{}.", it))
        .collect();

    Ok(blocklist)
}
