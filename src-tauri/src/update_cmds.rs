//! G2 Boost's own updates from GitHub releases: thin HTTP over `selfupdate` (which validates the
//! release, picks the asset and checks the hash). Only GitHub hosts are ever contacted.

use std::time::Duration;

use g2boost_core::selfupdate;
use tauri::AppHandle;
use tauri_plugin_http::reqwest;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    current: &'static str,
    latest: String,
    newer: bool,
    name: String,
    notes: String,
    url: String,
    published_at: String,
    /// `installed` (the button installs) | `portable` | `dev` (the button opens the release page).
    kind: &'static str,
    installer_size: Option<u64>,
}

const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn allowed(u: &reqwest::Url) -> bool {
    u.scheme() == "https" && u.host_str().is_some_and(|h| selfupdate::HOSTS.contains(&h))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("G2-Boost/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(90))
        // Release downloads redirect to GitHub's storage hosts; anything else stops the request.
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() < 5 && allowed(a.url()) { a.follow() } else { a.stop() }
        }))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(client: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>, String> {
    let url = reqwest::Url::parse(url).map_err(|_| "bad_url".to_string())?;
    if !allowed(&url) {
        return Err("bad_url".into());
    }
    let resp = client.get(url).send().await.map_err(|_| "offline".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("http_{}", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err("too_big".into());
    }
    let bytes = resp.bytes().await.map_err(|_| "offline".to_string())?;
    if bytes.len() > max {
        return Err("too_big".into());
    }
    Ok(bytes.to_vec())
}

async fn latest(client: &reqwest::Client) -> Result<selfupdate::Release, String> {
    let json = get(client, selfupdate::LATEST_URL, selfupdate::MAX_JSON).await?;
    selfupdate::parse_release(&json).ok_or_else(|| "bad_release".into())
}

fn kind() -> &'static str {
    std::env::current_exe().map(|e| selfupdate::install_kind(&e)).unwrap_or("dev")
}

/// What the latest published release is, and whether it is newer than this copy.
#[tauri::command]
pub async fn update_check() -> Result<UpdateInfo, String> {
    let r = latest(&client()?).await?;
    Ok(UpdateInfo {
        current: CURRENT,
        newer: selfupdate::newer(CURRENT, &r.version),
        installer_size: selfupdate::installer(&r).map(|a| a.size),
        kind: kind(),
        latest: r.version,
        name: r.name,
        notes: r.notes,
        url: r.url,
        published_at: r.published_at,
    })
}

/// Installed copies only: download the newer installer and SHA256SUMS.txt from the same release,
/// refuse unless the hashes match, start the installer and quit so it can replace the files.
/// Nothing from the UI is used (the release is fetched again here).
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if kind() != "installed" {
        return Err("not_installed".into());
    }
    let c = client()?;
    let r = latest(&c).await?;
    if !selfupdate::newer(CURRENT, &r.version) {
        return Err("up_to_date".into());
    }
    let inst = selfupdate::installer(&r).ok_or("no_installer")?;
    let sums = selfupdate::sums(&r).ok_or("no_sums")?;
    let sums = String::from_utf8(get(&c, &sums.url, selfupdate::MAX_SUMS).await?).map_err(|_| "bad_sums".to_string())?;
    let want = selfupdate::expected_hash(&sums, &inst.name).ok_or("no_hash")?;
    let bytes = get(&c, &inst.url, selfupdate::MAX_INSTALLER).await?;
    if selfupdate::sha256_hex(&bytes) != want {
        return Err("hash_mismatch".into());
    }
    let dir = g2boost_core::registry::data_dir().join("updates");
    let name = inst.name.clone();
    tauri::async_runtime::spawn_blocking(move || selfupdate::run_installer(&dir, &name, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live, read-only: the latest real release parses, its installer and SHA256SUMS.txt download
    /// through the allowed hosts and the hash matches (the install path minus starting it).
    /// `cargo test -p g2-boost -- --ignored update_download_live --nocapture`.
    #[test]
    #[ignore]
    fn update_download_live() {
        tauri::async_runtime::block_on(async {
            let c = client().unwrap();
            let r = latest(&c).await.unwrap();
            println!("latest {} ({} assets), newer than {CURRENT}: {}", r.version, r.assets.len(), selfupdate::newer(CURRENT, &r.version));
            let inst = selfupdate::installer(&r).expect("installer asset");
            let sums = String::from_utf8(get(&c, &selfupdate::sums(&r).unwrap().url, selfupdate::MAX_SUMS).await.unwrap()).unwrap();
            let want = selfupdate::expected_hash(&sums, &inst.name).expect("hash listed");
            let bytes = get(&c, &inst.url, selfupdate::MAX_INSTALLER).await.unwrap();
            assert_eq!(selfupdate::sha256_hex(&bytes), want, "installer hash");
            println!("{}: {} bytes, sha256 ok", inst.name, bytes.len());
            assert!(get(&c, "https://example.com/x.exe", 10).await.is_err(), "foreign host refused");
        });
    }
}
