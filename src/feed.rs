use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_SOURCE_URL: &str =
    "https://muhammad-favel.github.io/FlowDownload-Releases/source.json";

#[derive(Debug, Deserialize)]
pub struct AltSource {
    #[allow(dead_code)]
    pub name: String,
    pub apps: Vec<AltApp>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AltApp {
    pub name: String,
    pub bundle_identifier: String,
    pub versions: Vec<AltVersion>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AltVersion {
    pub version: String,
    pub date: String,
    #[serde(rename = "downloadURL")]
    pub download_url: String,
    pub size: u64,
    #[allow(dead_code)]
    pub min_os_version: Option<String>,
    pub sha256: Option<String>,
}

pub async fn fetch_latest_release(source_url: &str) -> Result<(AltApp, AltVersion)> {
    let client = reqwest::Client::builder()
        .user_agent("FlowDownload-Installer/0.1")
        .build()?;

    let source: AltSource = client
        .get(source_url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch {source_url}"))?
        .error_for_status()?
        .json()
        .await
        .context("Failed to parse source.json")?;

    let mut app = source
        .apps
        .into_iter()
        .next()
        .context("source.json contains no apps")?;

    if app.versions.is_empty() {
        bail!("No versions listed for {}", app.name);
    }

    let latest = app.versions.remove(0);
    Ok((app, latest))
}

pub async fn download_ipa(version: &AltVersion, cache_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)?;
    let filename = format!("FlowDownload-v{}.ipa", version.version);
    let dest = cache_dir.join(filename);

    if dest.exists() {
        if let Some(expected) = &version.sha256 {
            if verify_sha256(&dest, expected).is_ok() {
                println!("Using cached IPA: {}", dest.display());
                return Ok(dest);
            }
        } else if fs::metadata(&dest)?.len() == version.size {
            println!("Using cached IPA: {}", dest.display());
            return Ok(dest);
        }
    }

    println!("Downloading {} ({:.1} MB)...", version.download_url, version.size as f64 / 1_000_000.0);
    let client = reqwest::Client::builder()
        .user_agent("FlowDownload-Installer/0.1")
        .build()?;

    let mut resp = client
        .get(&version.download_url)
        .send()
        .await?
        .error_for_status()?;

    let tmp = dest.with_extension("ipa.part");
    let mut file = File::create(&tmp)?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk)?;
        hasher.update(&chunk);
    }
    file.flush()?;
    drop(file);

    let actual_sha = hex::encode(hasher.finalize());
    if let Some(expected) = &version.sha256 {
        if !actual_sha.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&tmp);
            bail!("SHA-256 mismatch! expected {expected}, got {actual_sha}");
        }
        println!("SHA-256 verified: {actual_sha}");
    }

    fs::rename(&tmp, &dest)?;
    Ok(dest)
}

pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<()> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_hex) {
        bail!("SHA-256 mismatch: expected {expected_hex}, got {actual}");
    }
    Ok(())
}
