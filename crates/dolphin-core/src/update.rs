//! Update check against a raw JSON manifest in the GitHub repo.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{CoreError, Result};

/// Raw GitHub manifest shared by the desktop apps, the iOS app and the website.
pub const UPDATE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/montanari-luca/MintFlow-Releases/refs/heads/main/dolphinpod.json";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Manifest {
    pub latest: String,
    #[serde(default)]
    pub macos: String,
    #[serde(default)]
    pub windows: String,
    #[serde(default)]
    pub linux: String,
    #[serde(default)]
    pub ios: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub url: Option<String>,
}

pub fn platform_link(m: &Manifest) -> &str {
    if cfg!(target_os = "macos") {
        &m.macos
    } else if cfg!(target_os = "windows") {
        &m.windows
    } else if cfg!(target_os = "ios") {
        &m.ios
    } else {
        &m.linux
    }
}

pub fn evaluate(m: &Manifest, current: &str) -> Result<UpdateInfo> {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches('v')).map_err(|e| CoreError::invalid(format!("bad version '{s}': {e}")));
    let latest = parse(&m.latest)?;
    let cur = parse(current)?;
    let link = platform_link(m).trim();
    Ok(UpdateInfo {
        current: cur.to_string(),
        latest: latest.to_string(),
        available: latest > cur && !link.is_empty(),
        url: (!link.is_empty()).then(|| link.to_string()),
    })
}

pub async fn check() -> Result<UpdateInfo> {
    let http = crate::providers::http_client();
    let m: Manifest = crate::providers::send_with_retry(http.get(UPDATE_MANIFEST_URL)).await?.json().await?;
    evaluate(&m, CURRENT_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(latest: &str) -> Manifest {
        Manifest { latest: latest.into(), macos: "https://x/mac.dmg".into(), windows: "https://x/win.msi".into(), linux: "https://x/linux.AppImage".into(), ios: "https://x/ios".into() }
    }

    #[test]
    fn compares_semver() {
        assert!(evaluate(&manifest("0.2.0"), "0.1.0").unwrap().available);
        assert!(evaluate(&manifest("v0.1.1"), "0.1.0").unwrap().available);
        assert!(!evaluate(&manifest("0.1.0"), "0.1.0").unwrap().available);
        assert!(!evaluate(&manifest("0.0.9"), "0.1.0").unwrap().available);
        assert!(!evaluate(&manifest("0.10.0-beta.1"), "0.10.0").unwrap().available);
        assert!(evaluate(&manifest("nope"), "0.1.0").is_err());
        let mut m = manifest("9.0.0");
        m.macos.clear();
        m.windows.clear();
        m.linux.clear();
        assert!(!evaluate(&m, "0.1.0").unwrap().available, "no link for this platform");
    }

    /// Hits the network: `cargo test -p dolphin-core live_manifest -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_manifest_is_reachable_and_valid() {
        let info = check().await.expect("manifest reachable and parseable");
        assert!(semver::Version::parse(&info.latest).is_ok());
    }

    #[test]
    fn parses_manifest_shape() {
        let m: Manifest = serde_json::from_str(r#"{"latest": "0.1.0", "macos": "", "windows": "", "linux": "", "ios": ""}"#).unwrap();
        assert_eq!(m.latest, "0.1.0");
    }
}
