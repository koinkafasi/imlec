//! Self-update against GitHub releases.
//!
//! Installing a new binary is never done silently: the background check only
//! reports that one is available, and replacement happens when the user runs
//! `imlec update`. Installations managed by a package manager are left alone.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPO: &str = "koinkafasi/imlec";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const USER_AGENT: &str = concat!("imlec/", env!("CARGO_PKG_VERSION"));

#[cfg(target_os = "windows")]
const ASSET: &str = "imlec-x86_64-windows.zip";
#[cfg(target_os = "linux")]
const ASSET: &str = "imlec-x86_64-linux.tar.gz";

pub struct Release {
    pub version: String,
    pub url: String,
    pub notes_url: String,
}

fn api_get(url: &str) -> Result<String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("requesting {url}"))?;
    Ok(response.into_body().read_to_string()?)
}

/// Extracts one string field from a flat JSON object without pulling in a parser.
fn json_str<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":");
    let start = body.find(&needle)? + needle.len();
    let rest = body[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

pub fn latest_release() -> Result<Release> {
    let body = api_get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))?;
    let tag = json_str(&body, "tag_name")
        .ok_or_else(|| anyhow!("no tag_name in the release response"))?
        .to_string();

    // Find the download URL that belongs to this platform's asset.
    let url = body
        .match_indices("\"browser_download_url\":")
        .filter_map(|(index, _)| {
            let rest = &body[index..];
            json_str(rest, "browser_download_url")
        })
        .find(|url| url.ends_with(ASSET))
        .ok_or_else(|| anyhow!("release {tag} has no {ASSET} asset"))?
        .to_string();

    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        url,
        notes_url: format!("https://github.com/{REPO}/releases/tag/{tag}"),
    })
}

/// Naive semantic comparison; a non-numeric component sorts as zero.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split(['.', '-', '+'])
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn stamp_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "particle-cursor")?;
    Some(dirs.cache_dir().join("last-update-check"))
}

fn due_for_check() -> bool {
    let Some(path) = stamp_path() else {
        return false;
    };
    match std::fs::metadata(&path).and_then(|m| m.modified()) {
        Ok(modified) => SystemTime::now()
            .duration_since(modified)
            .map(|age| age >= CHECK_INTERVAL)
            .unwrap_or(true),
        Err(_) => true,
    }
}

fn touch_stamp() {
    if let Some(path) = stamp_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, current_version());
    }
}

/// Fires once a day on a background thread and only logs. Never installs.
pub fn spawn_background_check() {
    if !due_for_check() {
        return;
    }
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(|| {
            touch_stamp();
            match latest_release() {
                Ok(release) if is_newer(&release.version, current_version()) => {
                    log::warn!(
                        "imlec {} is available (running {}). Run `imlec update` to install it: {}",
                        release.version,
                        current_version(),
                        release.notes_url
                    );
                }
                Ok(_) => log::debug!("imlec is up to date"),
                Err(err) => log::debug!("update check failed: {err:#}"),
            }
        })
        .ok();
}

/// True when the binary sits somewhere a package manager owns, in which case
/// replacing it in place would fight with the package database.
fn is_managed_install(exe: &Path) -> bool {
    let path = exe.to_string_lossy();
    ["/usr/bin/", "/usr/local/bin/", "/opt/", "/nix/store/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub fn run(check_only: bool) -> Result<()> {
    let release = latest_release()?;
    println!("installed {}", current_version());
    println!("latest    {}", release.version);

    if !is_newer(&release.version, current_version()) {
        println!("already up to date");
        touch_stamp();
        return Ok(());
    }
    println!("release notes: {}", release.notes_url);
    if check_only {
        return Ok(());
    }

    let exe = std::env::current_exe().context("locating the running binary")?;
    if is_managed_install(&exe) {
        bail!(
            "{} is managed by your package manager; update it with pacman/AUR instead",
            exe.display()
        );
    }

    println!("downloading {}", release.url);
    let bytes = download(&release.url)?;
    let staged = extract_binary(&bytes)?;
    self_replace::self_replace(&staged).context("replacing the running binary")?;
    let _ = std::fs::remove_file(&staged);
    touch_stamp();

    println!(
        "updated to {}. Restart imlec for it to take effect.",
        release.version
    );
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let mut buffer = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut buffer)
        .context("reading the download")?;
    Ok(buffer)
}

/// Unpacks the release archive into a temporary file next to the current binary
/// and returns its path.
fn extract_binary(bytes: &[u8]) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("imlec-update");
    std::fs::create_dir_all(&dir)?;

    #[cfg(target_os = "windows")]
    {
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).context("opening the zip")?;
        let mut file = archive
            .by_name("imlec.exe")
            .context("imlec.exe missing from the archive")?;
        let target = dir.join("imlec.exe");
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut file, &mut out)?;
        Ok(target)
    }

    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries().context("reading the tarball")? {
            let mut entry = entry?;
            let path = entry.path()?.to_path_buf();
            if path.file_name().and_then(|n| n.to_str()) != Some("imlec") {
                continue;
            }
            let target = dir.join("imlec");
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            std::fs::write(&target, buffer)?;

            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
            return Ok(target);
        }
        Err(anyhow!("imlec missing from the archive"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn reads_flat_json_fields() {
        let body = r#"{"tag_name": "v1.2.3", "name":"release"}"#;
        assert_eq!(json_str(body, "tag_name"), Some("v1.2.3"));
        assert_eq!(json_str(body, "missing"), None);
    }
}
