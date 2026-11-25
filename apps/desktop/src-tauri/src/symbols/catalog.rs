//! Verified catalog pages for the isolated Firmware tab.

use std::path::Path;
use std::sync::Mutex;

use dkaddrmap::{Cpu, Key};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tauri::http::{Request, Response, StatusCode, header};
use tauri_plugin_opener::OpenerExt;

use super::{unique_port, verify_hash};

const TEMPLATE: &str = include_str!("../../../../../tools/firmware/catalog_page.html");
pub(crate) const SCHEME: &str = "digi-catalog";
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                   connect-src 'none'; form-action 'none'; base-uri 'none'";

#[derive(Deserialize)]
struct ReviewFile {
    key: Key,
    review: ReviewLocation,
}

#[derive(Deserialize)]
struct ReviewLocation {
    cpu: Cpu,
    file: String,
    html_sha256: String,
    entries: Vec<u64>,
}

#[derive(Serialize)]
pub(super) struct ReviewIndex {
    pub(super) cpu: Cpu,
    pub(super) path: String,
    pub(super) url: String,
    pub(super) entries: Vec<u64>,
}

struct VerifiedReview {
    index: ReviewIndex,
    html: Vec<u8>,
}

fn verified_review(map: &Path, stock: &str) -> Option<VerifiedReview> {
    let text = std::fs::read_to_string(map).ok()?;
    let parsed: ReviewFile = serde_json::from_str(&text).ok()?;
    verify_hash(parsed.key.image_sha256.as_deref(), stock, "review").ok()?;
    let file = Path::new(&parsed.review.file);
    if file.file_name()?.to_str()? != parsed.review.file
        || !matches!(file.extension()?.to_str()?, "htm" | "html")
    {
        return None;
    }
    let parent = map.parent()?.canonicalize().ok()?;
    let path = parent.join(file);
    if path.with_extension("addrmap.json") != parent.join(map.file_name()?) {
        return None;
    }
    let html = std::fs::read(&path).ok()?;
    if format!("{:x}", Sha256::digest(&html)) != parsed.review.html_sha256 {
        return None;
    }
    let index = ReviewIndex {
        cpu: parsed.review.cpu,
        path: path.to_str()?.into(),
        url: tauri::Url::from_file_path(&path).ok()?.into(),
        entries: parsed.review.entries,
    };
    Some(VerifiedReview { index, html })
}

pub(super) fn read_review(map: &Path, stock: &str) -> Option<ReviewIndex> {
    Some(verified_review(map, stock)?.index)
}

fn with_entry(path: &str, sha256: &str, entry: Option<u64>) -> Result<VerifiedReview, String> {
    let path = Path::new(path);
    verified_review(&path.with_extension("addrmap.json"), sha256)
        .filter(|page| {
            Path::new(&page.index.path) == path
                && entry.is_none_or(|address| page.index.entries.contains(&address))
        })
        .ok_or_else(|| {
            "Firmware catalog or function is unavailable. Regenerate the catalog and refresh Firmware settings.".into()
        })
}

fn document(html: &[u8], image: &str, entry: Option<u64>) -> Option<Vec<u8>> {
    // Use the selected document only as data. Run the repository's renderer in the frame.
    let source = std::str::from_utf8(html).ok()?;
    let (_, data) = source.split_once("\n  const DATA = ")?;
    let (data, _) = data.split_once(";\n")?;
    let mut data: Value = serde_json::from_str(data).ok()?;
    if data["image"] != image || data["cpu"] != "sharc" {
        return None;
    }
    if let Some(entry) = entry {
        let address = format!("{entry:#x}");
        let contains = ["rows", "linked_rows"]
            .into_iter()
            .filter_map(|key| data[key].as_array())
            .flatten()
            .any(|row| row["a"] == address);
        if !contains {
            return None;
        }
    }
    data["embedded"] = json!(true);
    let data = serde_json::to_string(&data).ok()?.replace('<', "\\u003c");
    Some(
        TEMPLATE
            .replace("__SUBTITLE__", "SHARC · ")
            .replace("__DATA__", &data)
            .into_bytes(),
    )
}

#[derive(Default)]
pub(crate) struct CatalogFrame(Mutex<(u64, Vec<u8>)>);

impl CatalogFrame {
    fn publish(&self, html: Vec<u8>, entry: Option<u64>) -> Result<String, String> {
        let mut page = self.0.lock().map_err(|_| "Catalog frame is unavailable")?;
        page.0 += 1;
        page.1 = html;
        let origin = if cfg!(windows) {
            "http://digi-catalog.localhost"
        } else {
            "digi-catalog://localhost"
        };
        let fragment = entry.map_or_else(String::new, |addr| format!("#r-{addr:#x}"));
        Ok(format!("{origin}/{}{fragment}", page.0))
    }

    pub(crate) fn response(&self, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let html = self.0.lock().ok().and_then(|page| {
            (page.0 > 0 && request.uri().path() == format!("/{}", page.0)).then(|| page.1.clone())
        });
        let mut response = Response::builder()
            .header(header::CONTENT_SECURITY_POLICY, CSP)
            .header(header::CACHE_CONTROL, "no-store");
        let body = if let Some(html) = html {
            response = response.header(header::CONTENT_TYPE, "text/html; charset=utf-8");
            html
        } else {
            response = response.status(StatusCode::NOT_FOUND);
            Vec::new()
        };
        response.body(body).unwrap()
    }
}

#[tauri::command]
pub(crate) async fn symbols_catalog_frame(
    app: tauri::AppHandle,
    path: String,
    sha256: String,
    entry: Option<u64>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let port = unique_port(|port| port.stock == sha256)?.ok_or("Unknown firmware release")?;
        let page = with_entry(&path, port.stock, entry)?;
        let html = document(&page.html, port.id, entry).ok_or("Invalid SHARC catalog data")?;
        app.state::<CatalogFrame>().publish(html, entry)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn symbols_open_review(
    app: tauri::AppHandle,
    path: String,
    sha256: String,
    entry: Option<u64>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let page = with_entry(&path, &sha256, entry)?;
        let mut url =
            tauri::Url::parse(&page.index.url).map_err(|_| "Invalid firmware catalog path")?;
        if let Some(entry) = entry {
            url.set_fragment(Some(&format!("r-{entry:#x}")));
        }
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_catalog_from_data_without_running_the_selected_html() {
        let html = br#"<script>alert("selected script")</script>
  const DATA = {"image":"example","cpu":"sharc","rows":["</script><script>alert(1)</script>"]};
<script>alert("selected footer")</script>"#;
        let rendered = String::from_utf8(document(html, "example", None).unwrap()).unwrap();
        assert!(rendered.contains(r#""embedded":true"#));
        assert!(rendered.contains(r"\u003c/script>"));
        assert!(!rendered.contains("selected script"));
        assert!(!rendered.contains("selected footer"));
        assert!(document(html, "different", None).is_none());
    }

    #[test]
    fn serves_only_the_current_frame_and_keeps_a_fragment_for_the_function() {
        let pages = CatalogFrame::default();
        assert!(
            pages
                .publish(b"first".to_vec(), Some(16))
                .unwrap()
                .ends_with("/1#r-0x10")
        );
        pages.publish(b"second".to_vec(), None).unwrap();
        let request = |path| Request::builder().uri(path).body(Vec::new()).unwrap();
        assert_eq!(
            pages.response(request("/1")).status(),
            StatusCode::NOT_FOUND
        );
        let page = pages.response(request("/2"));
        assert_eq!(page.body(), b"second");
        assert_eq!(page.headers()[header::CONTENT_SECURITY_POLICY], CSP);
    }
}
