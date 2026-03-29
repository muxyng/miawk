use std::{
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    time::Duration,
};

use flate2::read::GzDecoder;
use reqwest::{
    Client,
    header::{ACCEPT, USER_AGENT},
};
use serde::{Deserialize, Serialize};

const RELEASE_TAG_API_PREFIX: &str = "https://api.github.com/repos/openai/codex/releases/tags/";
const INSTALL_DIR: &str = "codex-runtime";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone)]
pub struct ManagedCodexInstallation {
    pub codex_bin: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexManifest {
    version: String,
    asset_name: String,
    download_url: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn pinned_codex_version() -> &'static str {
    env!("RSC_CODEX_VERSION")
}

pub fn managed_codex_needs_install(data_dir: &Path) -> Result<bool, String> {
    let install_root = data_dir.join(INSTALL_DIR).join(pinned_codex_version());
    Ok(read_cached_codex(&install_root)?.is_none())
}

pub async fn ensure_managed_codex(data_dir: &Path) -> Result<ManagedCodexInstallation, String> {
    let version = pinned_codex_version();
    let install_root = data_dir.join(INSTALL_DIR).join(version);
    fs::create_dir_all(&install_root).map_err(|error| error.to_string())?;

    if let Some(cached) = read_cached_codex(&install_root)? {
        return Ok(cached);
    }

    download_pinned_codex_release(version, &install_root).await?;
    read_cached_codex(&install_root)?
        .ok_or_else(|| format!("Codex binary {version} was not cached after download"))
}

async fn download_pinned_codex_release(version: &str, install_root: &Path) -> Result<(), String> {
    let client = github_client()?;
    let asset_name = release_asset_name()?;
    let release = fetch_release_for_tag(&client, version).await?;
    if release.draft {
        return Err(format!("Codex release {version} is a draft"));
    }

    let asset = release
        .assets
        .iter()
        .find(|candidate| candidate.name == asset_name)
        .cloned()
        .ok_or_else(|| {
            format!(
                "release {} did not include asset {asset_name}",
                release.tag_name
            )
        })?;

    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|error| format!("failed to download Codex release asset: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to download Codex release asset: {error}"))?
        .bytes()
        .await
        .map_err(|error| format!("failed to read Codex release asset: {error}"))?;

    let destination_bin = install_root.join(binary_file_name());
    let temporary_bin = install_root.join(format!("{}.tmp", binary_file_name()));
    if temporary_bin.exists() {
        fs::remove_file(&temporary_bin).map_err(|error| error.to_string())?;
    }

    write_downloaded_binary(&temporary_bin, &asset.name, &bytes)?;
    ensure_executable(&temporary_bin)?;

    if destination_bin.exists() {
        fs::remove_file(&destination_bin).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary_bin, &destination_bin).map_err(|error| error.to_string())?;

    let manifest = CodexManifest {
        version: release.tag_name,
        asset_name: asset.name,
        download_url: asset.browser_download_url,
        source: "package-metadata".into(),
    };
    write_manifest(&install_root.join(MANIFEST_FILE), &manifest)
}

async fn fetch_release_for_tag(client: &Client, version: &str) -> Result<GithubRelease, String> {
    let candidate_tags = release_tag_candidates(version);
    let mut last_error = None;

    for tag in candidate_tags {
        let url = format!("{RELEASE_TAG_API_PREFIX}{tag}");
        match client.get(url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(success) => {
                    return success
                        .json::<GithubRelease>()
                        .await
                        .map_err(|error| format!("failed to parse Codex release {tag}: {error}"));
                }
                Err(error) => {
                    last_error = Some(format!("failed to query Codex release {tag}: {error}"));
                }
            },
            Err(error) => {
                last_error = Some(format!("failed to query Codex release {tag}: {error}"));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| format!("failed to resolve Codex release for {version}")))
}

fn release_tag_candidates(version: &str) -> Vec<String> {
    let mut candidates = vec![version.to_string()];
    if !version.starts_with("rust-v") {
        candidates.push(format!("rust-v{version}"));
    }
    candidates
}

fn github_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(USER_AGENT, "miawk/0.1".parse().unwrap());
            headers.insert(ACCEPT, "application/vnd.github+json".parse().unwrap());
            headers
        })
        .build()
        .map_err(|error| format!("failed to build GitHub client: {error}"))
}

fn release_asset_name() -> Result<String, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("codex-x86_64-unknown-linux-gnu.tar.gz".into()),
        ("linux", "aarch64") => Ok("codex-aarch64-unknown-linux-gnu.tar.gz".into()),
        ("macos", "x86_64") => Ok("codex-x86_64-apple-darwin.tar.gz".into()),
        ("macos", "aarch64") => Ok("codex-aarch64-apple-darwin.tar.gz".into()),
        ("windows", "x86_64") => Ok("codex-x86_64-pc-windows-msvc.exe".into()),
        ("windows", "aarch64") => Ok("codex-aarch64-pc-windows-msvc.exe".into()),
        (os, arch) => Err(format!("unsupported Codex platform target: {os}/{arch}")),
    }
}

fn binary_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "codex.exe"
    } else {
        "codex"
    }
}

fn read_cached_codex(install_root: &Path) -> Result<Option<ManagedCodexInstallation>, String> {
    let manifest_path = install_root.join(MANIFEST_FILE);
    let codex_bin = install_root.join(binary_file_name());
    if !codex_bin.exists() || !manifest_path.exists() {
        return Ok(None);
    }

    ensure_executable(&codex_bin)?;
    let manifest = read_manifest(&manifest_path)?;
    let _ = manifest;
    Ok(Some(ManagedCodexInstallation { codex_bin }))
}

fn write_downloaded_binary(
    destination: &Path,
    asset_name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    if asset_name.ends_with(".tar.gz") {
        let decoder = GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        let mut entries = archive.entries().map_err(|error| error.to_string())?;
        let mut entry = entries
            .next()
            .transpose()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Codex archive {asset_name} was empty"))?;
        let mut output = fs::File::create(destination).map_err(|error| error.to_string())?;
        io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        return Ok(());
    }

    fs::write(destination, bytes).map_err(|error| error.to_string())
}

fn read_manifest(path: &Path) -> Result<CodexManifest, String> {
    let payload = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&payload).map_err(|error| error.to_string())
}

fn write_manifest(path: &Path, manifest: &CodexManifest) -> Result<(), String> {
    let payload = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(path, payload).map_err(|error| error.to_string())
}

fn ensure_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }

    Ok(())
}
