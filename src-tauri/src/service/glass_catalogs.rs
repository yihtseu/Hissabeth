// SPDX-License-Identifier: GPL-3.0-or-later
//! Glass catalogs the user installs from the manufacturers' own sites.
//!
//! The data belongs to each manufacturer and this project does not
//! redistribute it: the desktop app fetches a catalog on the user's request,
//! or reads one the user downloaded, checks it with `parse_agf` and keeps the
//! extracted AGF under `<data dir>/glass/`. `catalogs.json` beside it records
//! where each catalog came from and when; every engine loads that directory
//! at start, and the desktop app reloads it after each install or removal.
use crate::optics::material::{self, CatalogWarning, ParsedCatalog};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One manufacturer's catalog and where its owner publishes it.
pub struct Vendor {
    /// The catalog name glasses carry (`N-BK7_SCHOTT`) and the AGF's file stem.
    pub catalog: &'static str,
    pub name: &'static str,
    /// The site terms that govern the downloaded data; `None` where the
    /// site states none, which grants nothing.
    pub terms: Option<&'static str>,
    /// The page the user can download the AGF from by hand.
    pub page: &'static str,
    /// A direct file address, an AGF or a zip holding one; `None` when the
    /// manufacturer offers none that a client can fetch.
    pub download: Option<&'static str>,
    /// File stems an archive may hold this catalog under, upper case,
    /// for an archive carrying several AGF files.
    pub entries: &'static [&'static str],
}

// Checked on 2026-11-28. OHARA's and CDGM's addresses carry a date or an
// opaque id and change with each edition; HOYA names its file by date and
// gives no stable address, so it is installed from its page by hand.
pub const VENDORS: &[Vendor] = &[
    Vendor {
        catalog: "SCHOTT",
        name: "SCHOTT",
        terms: Some("https://www.schott.com/en-us/disclaimer"),
        page: "https://www.schott.com/en-us/products/optical-glass-p1000267/downloads",
        download: Some(
            "https://media.schott.com/api/public/content/a79c07aa61da4c05a2c0bbab93d09a7f?download=true",
        ),
        entries: &["SCHOTT"],
    },
    Vendor {
        catalog: "HOYA",
        name: "HOYA",
        terms: Some("https://www.hoya-opticalworld.com/english/sitepolicy.html"),
        page: "https://www.hoya-opticalworld.com/english/datadownload/",
        download: None,
        entries: &["HOYA"],
    },
    Vendor {
        catalog: "OHARA",
        name: "OHARA",
        terms: None,
        page: "https://www.ohara-inc.co.jp/en/product/catalog/",
        // The edition with special-order and discontinued glasses, which
        // projects made before vendor data was unbundled may name.
        download: Some("https://www.ohara-inc.co.jp/wp-content/uploads/2022/02/OHARA_261118.zip"),
        entries: &["OHARA"],
    },
    Vendor {
        catalog: "CDGM",
        name: "CDGM",
        terms: None,
        page: "https://www.cdgmgd.com/go.htm?k=ge_lei_xia_zai&url=downList",
        download: Some("https://www.cdgmgd.com/downloadFile.htm?uid=vkYlfwDmuNtvITOp4NV"),
        entries: &["CDGM"],
    },
    Vendor {
        catalog: "NIKON-HIKARI",
        name: "Nikon (Hikari)",
        terms: Some("https://www.nikon.com/usage/group-info/"),
        page: "https://www.nikon.com/business/components/downloads/",
        download: Some("https://www.nikon.com/business/components/assets/pdf/nikon_zemax_data.zip"),
        entries: &["NIKON-HIKARI", "HIKARI", "NIKON"],
    },
];

pub fn vendor(catalog: &str) -> Result<&'static Vendor, String> {
    VENDORS
        .iter()
        .find(|v| v.catalog.eq_ignore_ascii_case(catalog))
        .ok_or_else(|| format!("Unknown glass catalog {catalog}"))
}

/// Where an installed catalog came from.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Fetched from this address (after redirects).
    Download { url: String },
    /// Read from a file the user chose; only its name is kept.
    File { name: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Record {
    pub origin: Origin,
    /// Seconds since the Unix epoch.
    pub installed_at: u64,
    pub glasses: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    /// The user skipped the first-run window; it no longer opens by itself.
    #[serde(default)]
    dismissed: bool,
    #[serde(default)]
    catalogs: BTreeMap<String, Record>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct VendorStatus {
    pub catalog: &'static str,
    pub name: &'static str,
    pub terms: Option<&'static str>,
    pub page: &'static str,
    pub download: Option<&'static str>,
    pub installed: Option<Record>,
    /// Parse warnings of the installed file.
    pub warnings: Vec<CatalogWarning>,
    /// Why the installed file could not be loaded.
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Status {
    /// Open the window by itself: nothing installed and never skipped.
    pub prompt: bool,
    pub vendors: Vec<VendorStatus>,
    /// A manifest that could not be read; the catalogs it listed are not loaded.
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Installed {
    pub catalog: &'static str,
    pub record: Record,
    pub warnings: Vec<CatalogWarning>,
}

/// Installs, removals and reloads run one at a time.
static LOCK: Mutex<()> = Mutex::new(());
/// Each recorded catalog at the last reload: its parse warnings, or why it
/// failed to load.
static LOADED: Mutex<BTreeMap<String, Result<Vec<CatalogWarning>, String>>> =
    Mutex::new(BTreeMap::new());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn glass_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("glass")
}
fn manifest_path(data_dir: &Path) -> PathBuf {
    glass_dir(data_dir).join("catalogs.json")
}
fn agf_path(data_dir: &Path, vendor: &Vendor) -> PathBuf {
    glass_dir(data_dir).join(format!("{}.agf", vendor.catalog))
}

fn read_manifest(data_dir: &Path) -> Result<Manifest, String> {
    let path = manifest_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Writes beside the target and renames over it, so a reader sees the old
/// file or the new one and never part of either.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("Path has no parent directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path)
        .map_err(|e| format!("{}: {}", path.display(), e.error))?;
    Ok(())
}

fn write_manifest(data_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    write_atomic(&manifest_path(data_dir), &bytes)
}

/// AGF text in UTF-8, UTF-16 (Zemax writes either, with or without a byte
/// order mark) or a single-byte code page, read as Latin-1.
pub fn decode_agf(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |bytes: &[u8], little: bool| {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if little {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| "AGF file is not valid UTF-16".to_string())
    };
    if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8(rest.to_vec()).map_err(|_| "AGF file is not valid UTF-8".into());
    }
    if let Some(rest) = bytes.strip_prefix(b"\xff\xfe") {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xfe\xff") {
        return utf16(rest, false);
    }
    // ASCII text in UTF-16 without a mark puts a zero in every other byte.
    let sample = &bytes[..bytes.len().min(512)];
    let zeros = |offset: usize| {
        sample
            .iter()
            .skip(offset)
            .step_by(2)
            .filter(|b| **b == 0)
            .count()
    };
    if sample.len() >= 4 && zeros(1) * 2 >= sample.len() / 2 && zeros(0) == 0 {
        return utf16(bytes, true);
    }
    if sample.len() >= 4 && zeros(0) * 2 >= sample.len() / 2 && zeros(1) == 0 {
        return utf16(bytes, false);
    }
    Ok(match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    })
}

/// Reads the catalogs `catalogs.json` lists and makes them the vendor part of
/// the material library; nothing else is bundled. A file that fails to load is left out and
/// its error kept for [`status`]; the others still load.
pub fn reload(data_dir: &Path) -> Result<(), String> {
    let _guard = guard();
    reload_locked(data_dir)
}

fn reload_locked(data_dir: &Path) -> Result<(), String> {
    let manifest = read_manifest(data_dir);
    let mut loaded = vec![];
    let mut outcomes = BTreeMap::new();
    for catalog in manifest
        .as_ref()
        .map(|m| m.catalogs.keys())
        .into_iter()
        .flatten()
    {
        let outcome = load_one(data_dir, catalog).map(|parsed| {
            let warnings = parsed.warnings.clone();
            loaded.push(parsed);
            warnings
        });
        outcomes.insert(catalog.clone(), outcome);
    }
    // Unit tests run side by side in one process and read the fixture
    // library; a test's own catalog directory must not replace it under them.
    if !cfg!(test) {
        material::set_installed(&loaded);
    }
    *LOADED.lock().unwrap_or_else(|e| e.into_inner()) = outcomes;
    manifest.map(|_| ())
}

fn load_one(data_dir: &Path, catalog: &str) -> Result<ParsedCatalog, String> {
    let vendor = vendor(catalog)?;
    let path = agf_path(data_dir, vendor);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    material::parse_agf(vendor.catalog, &decode_agf(&bytes)?)
}

pub fn status(data_dir: &Path) -> Status {
    let _guard = guard();
    let manifest = read_manifest(data_dir);
    let loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let empty = Manifest::default();
    let recorded = manifest.as_ref().unwrap_or(&empty);
    Status {
        prompt: manifest
            .as_ref()
            .is_ok_and(|m| !m.dismissed && m.catalogs.is_empty()),
        vendors: VENDORS
            .iter()
            .map(|v| {
                let installed = recorded.catalogs.get(v.catalog).cloned();
                let (warnings, error) = match loaded.get(v.catalog) {
                    _ if installed.is_none() => (vec![], None),
                    Some(Ok(warnings)) => (warnings.clone(), None),
                    Some(Err(error)) => (vec![], Some(error.clone())),
                    None => (vec![], Some("Not loaded since it was recorded".into())),
                };
                VendorStatus {
                    catalog: v.catalog,
                    name: v.name,
                    terms: v.terms,
                    page: v.page,
                    download: v.download,
                    installed,
                    warnings,
                    error,
                }
            })
            .collect(),
        error: manifest.err(),
    }
}

/// Stops the window from opening by itself at later starts.
pub fn dismiss(data_dir: &Path) -> Result<(), String> {
    let _guard = guard();
    let mut manifest = read_manifest(data_dir)?;
    manifest.dismissed = true;
    write_manifest(data_dir, &manifest)
}

pub fn remove(data_dir: &Path, catalog: &str) -> Result<(), String> {
    let vendor = vendor(catalog)?;
    let _guard = guard();
    let mut manifest = read_manifest(data_dir)?;
    if manifest.catalogs.remove(vendor.catalog).is_none() {
        return Err(format!("{} is not installed", vendor.catalog));
    }
    write_manifest(data_dir, &manifest)?;
    let path = agf_path(data_dir, vendor);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("Glass catalog {}: {e}", path.display()),
    }
    reload_locked(data_dir)
}

/// The largest download or archive member read, well above any vendor AGF.
#[cfg(feature = "desktop")]
pub const MAX_BYTES: usize = 32 << 20;

/// The AGF in `bytes`: the file itself, or the one member of a zip archive
/// that holds this vendor's catalog.
#[cfg(feature = "desktop")]
pub fn extract_agf(vendor: &Vendor, bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(bytes.to_vec());
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Unreadable zip archive: {e}"))?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|name| name.ok().map(std::borrow::Cow::into_owned))
        .filter(|name| {
            !name.ends_with('/')
                && !name.starts_with("__MACOSX/")
                && name.to_ascii_lowercase().ends_with(".agf")
        })
        .collect();
    let stem = |name: &str| {
        let file = name.rsplit('/').next().unwrap_or(name);
        file[..file.len() - 4].to_ascii_uppercase()
    };
    let chosen = match names.as_slice() {
        [] => return Err("The archive holds no AGF file".into()),
        [only] => only.clone(),
        _ => names
            .iter()
            .find(|n| vendor.entries.contains(&stem(n).as_str()))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "The archive holds several AGF files ({}); none is the {} catalog",
                    names.join(", "),
                    vendor.catalog
                )
            })?,
    };
    let entry = archive.by_name(&chosen).map_err(|e| e.to_string())?;
    let mut out = vec![];
    entry
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("{chosen}: {e}"))?;
    if out.len() > MAX_BYTES {
        return Err(format!("{chosen} is larger than {} MB", MAX_BYTES >> 20));
    }
    Ok(out)
}

/// Checks `bytes` (an AGF or a zip holding one) as this vendor's catalog and,
/// only if it parses, replaces the installed file, records its origin and
/// reloads the library. A file that fails leaves the installed one in place.
#[cfg(feature = "desktop")]
pub fn install(
    data_dir: &Path,
    catalog: &str,
    bytes: &[u8],
    origin: Origin,
) -> Result<Installed, String> {
    let vendor = vendor(catalog)?;
    let text = decode_agf(&extract_agf(vendor, bytes)?)?;
    let parsed = material::parse_agf(vendor.catalog, &text)
        .map_err(|e| format!("Not a valid {} AGF catalog: {e}", vendor.catalog))?;
    let record = Record {
        origin,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        glasses: parsed.glasses.len(),
    };
    let _guard = guard();
    let mut manifest = read_manifest(data_dir)?;
    write_atomic(&agf_path(data_dir, vendor), text.as_bytes())?;
    manifest
        .catalogs
        .insert(vendor.catalog.to_owned(), record.clone());
    write_manifest(data_dir, &manifest)?;
    reload_locked(data_dir)?;
    Ok(Installed {
        catalog: vendor.catalog,
        record,
        warnings: parsed.warnings,
    })
}

/// Fetches a vendor's direct download address from the user's machine.
#[cfg(feature = "desktop")]
pub async fn download(vendor: &Vendor) -> Result<(Vec<u8>, String), String> {
    let url = vendor.download.ok_or_else(|| {
        format!(
            "{} offers no direct download; open its download page instead",
            vendor.name
        )
    })?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("Hissabeth/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let failed = |e: reqwest::Error| format!("Download from {url} failed: {e}");
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(failed)?;
    let final_url = response.url().to_string();
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(format!("{url} is larger than {} MB", MAX_BYTES >> 20));
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_BYTES {
            return Err(format!("{url} is larger than {} MB", MAX_BYTES >> 20));
        }
    }
    Ok((bytes, final_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "desktop")]
    const AGF: &str = "CC test\nNM N-BK7 2 517642 1.5168 64.17 0 1\nCD 1.03961212 0.00600069867 0.231792344 0.0200179144 1.01046945 103.560653\nIT 0.5 0.9 10\nIT 0.5 2 10\n";

    #[test]
    fn decodes_utf16_and_latin1() {
        let le: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("NM A".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_agf(&le).unwrap(), "NM A");
        let bare: Vec<u8> = "NM A 2".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_agf(&bare).unwrap(), "NM A 2");
        let be: Vec<u8> = "NM A 2".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode_agf(&be).unwrap(), "NM A 2");
        assert_eq!(decode_agf(b"GC \xb5m").unwrap(), "GC µm");
    }

    #[test]
    fn unknown_catalogs_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(vendor("../SCHOTT").is_err());
        assert!(remove(dir.path(), "SUMITA").is_err());
    }

    #[test]
    fn first_run_prompts_until_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(status(dir.path()).prompt);
        dismiss(dir.path()).unwrap();
        assert!(!status(dir.path()).prompt);
    }

    #[cfg(feature = "desktop")]
    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        for (name, text) in files {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn archives_yield_the_vendor_member() {
        let nikon = vendor("NIKON-HIKARI").unwrap();
        let one = zip_of(&[("dir/Any.AGF", AGF), ("readme.txt", "x")]);
        assert_eq!(extract_agf(nikon, &one).unwrap(), AGF.as_bytes());
        let several = zip_of(&[("OTHER.agf", "x"), ("glass/HIKARI.agf", AGF)]);
        assert_eq!(extract_agf(nikon, &several).unwrap(), AGF.as_bytes());
        let neither = zip_of(&[("A.agf", AGF), ("B.agf", AGF)]);
        assert!(
            extract_agf(nikon, &neither)
                .unwrap_err()
                .contains("A.agf, B.agf")
        );
        assert!(extract_agf(nikon, &zip_of(&[("a.txt", "x")])).is_err());
    }

    /// Files fetched from the vendors, named `CATALOG=path` in
    /// `HISSABETH_GLASS_FILES` (comma separated); CI never downloads them.
    #[cfg(feature = "desktop")]
    #[test]
    #[ignore = "needs vendor files downloaded by hand"]
    fn vendor_files_install() {
        let list = std::env::var("HISSABETH_GLASS_FILES").expect("HISSABETH_GLASS_FILES");
        for item in list.split(',') {
            let (catalog, path) = item.split_once('=').unwrap();
            let vendor = vendor(catalog).unwrap();
            let bytes = std::fs::read(path).unwrap();
            let text = decode_agf(&extract_agf(vendor, &bytes).unwrap()).unwrap();
            let parsed = material::parse_agf(vendor.catalog, &text)
                .unwrap_or_else(|e| panic!("{catalog}: {e}"));
            println!(
                "{catalog}: {} glasses, {} warnings",
                parsed.glasses.len(),
                parsed.warnings.len()
            );
            for w in parsed.warnings.iter().take(5) {
                println!("  line {} {}: {}", w.line, w.glass, w.message);
            }
        }
    }

    /// Installing writes the file and record; a bad file keeps the old one.
    #[cfg(feature = "desktop")]
    #[test]
    fn install_validates_before_replacing() {
        let dir = tempfile::tempdir().unwrap();
        let origin = Origin::File {
            name: "SCHOTT.agf".into(),
        };
        let done = install(dir.path(), "schott", AGF.as_bytes(), origin.clone()).unwrap();
        assert_eq!(done.record.glasses, 1);
        assert_eq!(done.warnings.len(), 1, "the nonphysical IT record");
        let path = agf_path(dir.path(), vendor("SCHOTT").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), AGF);
        let bad = install(dir.path(), "SCHOTT", b"NM broken", origin.clone());
        assert!(bad.unwrap_err().starts_with("Not a valid SCHOTT AGF"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), AGF);
        let state = status(dir.path());
        assert!(!state.prompt);
        let schott = &state.vendors[0];
        assert_eq!(schott.installed.as_ref().unwrap().origin, origin);
        remove(dir.path(), "SCHOTT").unwrap();
        assert!(!path.exists());
        assert!(status(dir.path()).vendors[0].installed.is_none());
    }
}
