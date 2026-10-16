//! The .wbx format engine, in plain Rust (no Node types), so it can be tested with `cargo test` and later shared
//! with the desktop and wbOS runtimes. The Node bindings in `lib.rs` are a thin layer over this file.
//!
//! Layout (little-endian):
//!   "WBX\x01" | u32 header length | header JSON | u32 file count |
//!   per file: u32 path length | path | u32 original size | u32 compressed size | 64 hex chars SHA-256 | gzip bytes
//!
//! Format 1.1 (this code) keeps that layout, so old readers still open new packages. What changed:
//! files are written in sorted order and the creation time is an input, so the same game packs to the same bytes;
//! the reader verifies every checksum, refuses unsafe paths and decompression bombs; the manifest can describe a
//! `hosted` game (lives at its own web address) and carries `icon` and `screenshots`.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"WBX\x01";
pub const FORMAT_VERSION: &str = "1.1.0";
/// Used when the caller gives no creation time, so the output is identical every time.
pub const NO_TIMESTAMP: &str = "1970-01-01T00:00:00+00:00";
const HASH_LEN: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_package_bytes: usize,
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_package_bytes: 50 * 1024 * 1024, // spec: 50 MB for the MVP
            max_files: 5000,
            max_file_bytes: 50 * 1024 * 1024,
            max_total_bytes: 200 * 1024 * 1024, // what the files may add up to once unpacked
        }
    }
}

/// Every way a package can be refused. `code()` is stable and matches the TypeScript reader in the API.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WbxError {
    #[error("This package is over the size limit.")]
    TooBig,
    #[error("This is not a .wbx file.")]
    BadMagic,
    #[error("The package is cut short.")]
    Truncated,
    #[error("The package header is not valid: {0}")]
    BadMetadata(String),
    #[error("The manifest is invalid: {0}")]
    BadManifest(String),
    #[error("Unsafe file path: {0}")]
    UnsafePath(String),
    #[error("The file {0} appears twice.")]
    DuplicatePath(String),
    #[error("Programs are not allowed in a game: {0}")]
    ForbiddenFile(String),
    #[error("{0} was changed after packaging.")]
    BadChecksum(String),
    #[error("A file does not match its declared size: {0}")]
    BadSize(String),
    #[error("Too many files in this package.")]
    TooManyFiles,
    #[error("The entry point {0} is not in the package.")]
    MissingEntry(String),
    #[error("A file could not be decompressed: {0}")]
    BadCompression(String),
    #[error("Compression level must be between 0 and 9.")]
    BadLevel,
}

impl WbxError {
    pub fn code(&self) -> &'static str {
        match self {
            WbxError::TooBig => "too_big",
            WbxError::BadMagic => "bad_magic",
            WbxError::Truncated => "truncated",
            WbxError::BadMetadata(_) => "bad_metadata",
            WbxError::BadManifest(_) => "bad_manifest",
            WbxError::UnsafePath(_) => "unsafe_path",
            WbxError::DuplicatePath(_) => "duplicate_path",
            WbxError::ForbiddenFile(_) => "forbidden_file",
            WbxError::BadChecksum(_) => "bad_checksum",
            WbxError::BadSize(_) => "bad_size",
            WbxError::TooManyFiles => "too_many_files",
            WbxError::MissingEntry(_) => "missing_entry",
            WbxError::BadCompression(_) => "bad_compression",
            WbxError::BadLevel => "bad_level",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GameType {
    /// The game's files are inside the package.
    #[default]
    Bundled,
    /// The game lives at `url`; the package only describes it (name, icon, screenshots, rules).
    Hosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Features {
    pub multiplayer: bool,
    pub achievements: bool,
    pub leaderboards: bool,
    pub cloud_save: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebboxConfig {
    pub min_version: String,
    pub permissions: Vec<String>,
    pub features: Features,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub scalable: bool,
    pub aspect_ratio: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Technical {
    pub renderer: String,
    pub orientation: String,
    pub fullscreen: bool,
    pub resolution: Resolution,
}

/// Field order here is the order written to disk, and the package checksum covers it, so do not reorder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub id: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub entry_point: String,
    pub category: String,
    pub tags: Vec<String>,
    pub rating: String,
    pub webbox: WebboxConfig,
    pub technical: Technical,
    // ---- added in format 1.1; packages from the first packager simply do not have them ----
    #[serde(default, rename = "type")]
    pub game_type: GameType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<Vec<String>>,
    // ---- engagement (Building/Engagement.md); absent in older packages ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub achievements: Option<Vec<Achievement>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaderboards: Option<Vec<Leaderboard>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub points: u32,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leaderboard {
    pub id: String,
    pub name: String,
    /// "desc": higher is better; "asc": lower is better (times).
    pub order: String,
    /// "number", "time_ms" or "distance_m".
    pub format: String,
}

fn valid_short_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty() && b.len() <= 40 && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-') && b[0] != b'-'
}

const PERMISSIONS: [&str; 5] = ["audio", "gamepad", "storage", "microphone", "fullscreen"];
const RENDERERS: [&str; 3] = ["canvas", "webgl", "webgpu"];
const ORIENTATIONS: [&str; 3] = ["landscape", "portrait", "any"];
const RATINGS: [&str; 4] = ["E", "T", "M", "A"];

fn bad<T>(message: &str) -> Result<T, WbxError> {
    Err(WbxError::BadManifest(message.to_string()))
}

/// Same shape as the store's rule for game ids: lowercase letters, numbers and dashes.
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=63).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn valid_version(version: &str) -> bool {
    let core = version.split(|c| c == '-' || c == '+').next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A hosted game's address: https, a real host, no embedded passwords.
fn valid_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    url.len() <= 500
        && !authority.is_empty()
        && !authority.contains('@')
        && authority.contains('.')
        && url.bytes().all(|b| b.is_ascii_graphic())
}

impl Manifest {
    pub fn validate(&self) -> Result<(), WbxError> {
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return bad("name must be 1 to 80 characters");
        }
        if !valid_id(&self.id) {
            return bad("id must be lowercase letters, numbers and dashes");
        }
        if !valid_version(&self.version) {
            return bad("version must look like 1.0.0");
        }
        if self.author.trim().is_empty() || self.author.chars().count() > 80 {
            return bad("author must be 1 to 80 characters");
        }
        if self.description.chars().count() > 2000 {
            return bad("description is too long");
        }
        if self.tags.len() > 20 || self.tags.iter().any(|t| t.chars().count() > 40) {
            return bad("too many or too long tags");
        }
        if !RATINGS.contains(&self.rating.as_str()) {
            return bad("rating must be E, T, M or A");
        }
        if self.webbox.permissions.len() > 10 || self.webbox.permissions.iter().any(|p| !PERMISSIONS.contains(&p.as_str())) {
            return bad("unknown permission");
        }
        if !RENDERERS.contains(&self.technical.renderer.as_str()) {
            return bad("renderer must be canvas, webgl or webgpu");
        }
        if !ORIENTATIONS.contains(&self.technical.orientation.as_str()) {
            return bad("orientation must be landscape, portrait or any");
        }
        let r = &self.technical.resolution;
        if r.width == 0 || r.width > 7680 || r.height == 0 || r.height > 4320 {
            return bad("resolution is out of range");
        }
        for path in self.icon.iter().chain(self.screenshots.iter().flatten()) {
            if !is_safe_path(path) {
                return Err(WbxError::UnsafePath(path.clone()));
            }
        }
        if self.screenshots.as_ref().is_some_and(|s| s.len() > 10) {
            return bad("at most 10 screenshots");
        }
        if let Some(list) = &self.achievements {
            if !self.webbox.features.achievements {
                return bad("achievements are listed but webbox.features.achievements is false");
            }
            if list.len() > 100 {
                return bad("at most 100 achievements");
            }
            let mut seen = std::collections::HashSet::new();
            let mut total = 0u32;
            for a in list {
                if !valid_short_id(&a.id) || !seen.insert(a.id.as_str()) {
                    return bad("achievement ids must be unique, 1 to 40 lowercase letters, numbers and dashes");
                }
                if a.name.trim().is_empty() || a.name.chars().count() > 40 || a.description.chars().count() > 120 || a.points > 100 {
                    return bad("achievement name 1-40 characters, description up to 120, points 0-100");
                }
                total += a.points;
            }
            if total > 1000 {
                return bad("achievements add up to more than 1000 points");
            }
        }
        if let Some(list) = &self.leaderboards {
            if !self.webbox.features.leaderboards {
                return bad("leaderboards are listed but webbox.features.leaderboards is false");
            }
            if list.len() > 20 {
                return bad("at most 20 leaderboards");
            }
            let mut seen = std::collections::HashSet::new();
            for l in list {
                if !valid_short_id(&l.id) || !seen.insert(l.id.as_str()) {
                    return bad("leaderboard ids must be unique, 1 to 40 lowercase letters, numbers and dashes");
                }
                if l.name.trim().is_empty() || l.name.chars().count() > 40 || !["asc", "desc"].contains(&l.order.as_str()) || !["number", "time_ms", "distance_m"].contains(&l.format.as_str()) {
                    return bad("leaderboard name 1-40 characters, order asc or desc, format number, time_ms or distance_m");
                }
            }
        }
        match self.game_type {
            GameType::Bundled => {
                if self.url.is_some() {
                    return bad("a bundled game must not have a url");
                }
                if !is_safe_path(&self.entry_point) {
                    return Err(WbxError::UnsafePath(self.entry_point.clone()));
                }
                let lower = self.entry_point.to_ascii_lowercase();
                if !(lower.ends_with(".html") || lower.ends_with(".htm")) {
                    return Err(WbxError::MissingEntry(self.entry_point.clone()));
                }
            }
            GameType::Hosted => match &self.url {
                Some(url) if valid_https_url(url) => {}
                _ => return bad("a hosted game needs an https url"),
            },
        }
        Ok(())
    }
}

/// A path that stays inside the package: no `..`, no absolute or drive paths, no backslashes, no control characters.
pub fn is_safe_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 255 || path.starts_with('/') || path.contains('\\') || path.contains("//") {
        return false;
    }
    if path.chars().any(|c| c.is_control()) {
        return false;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Nothing that can run natively belongs in a web game. WebAssembly is fine.
pub fn is_forbidden_file(path: &str) -> bool {
    const BAD: [&str; 12] = ["exe", "dll", "so", "dylib", "bat", "cmd", "com", "msi", "scr", "sh", "ps1", "app"];
    const BAD_MORE: [&str; 3] = ["apk", "jar", "vbs"];
    match path.rsplit_once('.') {
        Some((_, ext)) => {
            let ext = ext.to_ascii_lowercase();
            BAD.contains(&ext.as_str()) || BAD_MORE.contains(&ext.as_str())
        }
        None => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub format: String,
    pub version: String,
    pub created: String,
    pub compression: String,
    pub manifest: Manifest,
    pub files: Vec<String>,
    /// A decimal on purpose: the first packager wrote it that way (206.0) and old packages must keep opening.
    pub total_size: f64,
    pub checksum: String,
}

#[derive(Debug)]
pub struct Package {
    pub header: Header,
    /// Every file, decompressed and verified, in path order.
    pub files: BTreeMap<String, Vec<u8>>,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Covers the manifest and every file's path and hash, in path order, so it cannot depend on how the packer iterated.
fn package_checksum(manifest: &Manifest, hashes: &[(String, String)]) -> Result<String, WbxError> {
    let manifest_json = serde_json::to_string(manifest).map_err(|e| WbxError::BadMetadata(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(manifest_json.as_bytes());
    for (path, hash) in hashes {
        hasher.update(b"\n");
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
    }
    Ok(hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

/// Builds a package. The same inputs always give the same bytes (files are sorted; `created` is yours to set).
pub fn build_package(
    manifest: &Manifest,
    files: &BTreeMap<String, Vec<u8>>,
    compression_level: u32,
    created: Option<&str>,
    limits: &Limits,
) -> Result<Vec<u8>, WbxError> {
    if compression_level > 9 {
        return Err(WbxError::BadLevel);
    }
    manifest.validate()?;
    if files.len() > limits.max_files {
        return Err(WbxError::TooManyFiles);
    }

    let mut total: usize = 0;
    for (path, content) in files {
        if !is_safe_path(path) {
            return Err(WbxError::UnsafePath(path.clone()));
        }
        if is_forbidden_file(path) {
            return Err(WbxError::ForbiddenFile(path.clone()));
        }
        if content.len() > limits.max_file_bytes {
            return Err(WbxError::TooBig);
        }
        total += content.len();
    }
    if total > limits.max_total_bytes {
        return Err(WbxError::TooBig);
    }
    if manifest.game_type == GameType::Bundled && !files.contains_key(&manifest.entry_point) {
        return Err(WbxError::MissingEntry(manifest.entry_point.clone()));
    }
    for path in manifest.icon.iter().chain(manifest.screenshots.iter().flatten()) {
        if !files.contains_key(path) {
            return Err(WbxError::BadManifest(format!("{} is listed in the manifest but is not in the package", path)));
        }
    }

    let mut entries: Vec<(String, String, Vec<u8>, usize)> = Vec::with_capacity(files.len());
    for (path, content) in files {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::new(compression_level));
        encoder.write_all(content).map_err(|e| WbxError::BadCompression(e.to_string()))?;
        let compressed = encoder.finish().map_err(|e| WbxError::BadCompression(e.to_string()))?;
        entries.push((path.clone(), sha256_hex(content), compressed, content.len()));
    }

    let hashes: Vec<(String, String)> = entries.iter().map(|(p, h, _, _)| (p.clone(), h.clone())).collect();
    let header = Header {
        format: "WBX".to_string(),
        version: FORMAT_VERSION.to_string(),
        created: created.unwrap_or(NO_TIMESTAMP).to_string(),
        compression: compression_level.to_string(),
        manifest: manifest.clone(),
        files: files.keys().cloned().collect(),
        total_size: total as f64,
        checksum: package_checksum(manifest, &hashes)?,
    };
    let header_json = serde_json::to_vec(&header).map_err(|e| WbxError::BadMetadata(e.to_string()))?;

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&header_json);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (path, hash, compressed, original) in &entries {
        out.extend_from_slice(&(path.len() as u32).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&(*original as u32).to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(hash.as_bytes());
        out.extend_from_slice(compressed);
    }
    if out.len() > limits.max_package_bytes {
        return Err(WbxError::TooBig);
    }
    Ok(out)
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], WbxError> {
        let end = self.at.checked_add(n).ok_or(WbxError::Truncated)?;
        if end > self.data.len() {
            return Err(WbxError::Truncated);
        }
        let slice = &self.data[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u32(&mut self) -> Result<u32, WbxError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Gunzip, but never produce more than the declared size, so a tiny file cannot expand into gigabytes.
fn gunzip_exactly(compressed: &[u8], expected: usize) -> Result<Vec<u8>, WbxError> {
    let mut out = Vec::with_capacity(expected.min(1 << 20));
    let mut decoder = GzDecoder::new(compressed).take(expected as u64 + 1);
    decoder.read_to_end(&mut out).map_err(|e| WbxError::BadCompression(e.to_string()))?;
    if out.len() != expected {
        return Err(WbxError::BadSize(format!("expected {} bytes, got {}", expected, out.len())));
    }
    Ok(out)
}

/// Reads a package and checks everything: header, manifest, every path, every size, every checksum.
pub fn read_package(bytes: &[u8], limits: &Limits) -> Result<Package, WbxError> {
    if bytes.len() > limits.max_package_bytes {
        return Err(WbxError::TooBig);
    }
    if bytes.is_empty() || MAGIC.iter().zip(bytes.iter()).any(|(a, b)| a != b) {
        return Err(WbxError::BadMagic);
    }
    if bytes.len() < 12 {
        return Err(WbxError::Truncated);
    }
    let mut cur = Cursor { data: bytes, at: 4 };
    let header_len = cur.u32()? as usize;
    let header_bytes = cur.take(header_len)?;
    let header: Header = serde_json::from_slice(header_bytes).map_err(|e| WbxError::BadMetadata(e.to_string()))?;
    if header.format != "WBX" {
        return Err(WbxError::BadMetadata("not a WBX header".to_string()));
    }
    header.manifest.validate()?;

    let count = cur.u32()? as usize;
    if count > limits.max_files {
        return Err(WbxError::TooManyFiles);
    }

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut hashes: Vec<(String, String)> = Vec::with_capacity(count);
    let mut total: usize = 0;
    for _ in 0..count {
        let path_len = cur.u32()? as usize;
        if path_len > 255 {
            return Err(WbxError::UnsafePath("path is too long".to_string()));
        }
        let path = std::str::from_utf8(cur.take(path_len)?).map_err(|_| WbxError::UnsafePath("path is not valid text".to_string()))?.to_string();
        if !is_safe_path(&path) {
            return Err(WbxError::UnsafePath(path));
        }
        if files.contains_key(&path) {
            return Err(WbxError::DuplicatePath(path));
        }
        if is_forbidden_file(&path) {
            return Err(WbxError::ForbiddenFile(path));
        }
        let original = cur.u32()? as usize;
        let compressed_len = cur.u32()? as usize;
        if original > limits.max_file_bytes {
            return Err(WbxError::TooBig);
        }
        total += original;
        if total > limits.max_total_bytes {
            return Err(WbxError::TooBig);
        }
        let expected = std::str::from_utf8(cur.take(HASH_LEN)?).map_err(|_| WbxError::BadChecksum(path.clone()))?.to_ascii_lowercase();
        if !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(WbxError::BadChecksum(path));
        }
        let data = gunzip_exactly(cur.take(compressed_len)?, original)?;
        if sha256_hex(&data) != expected {
            return Err(WbxError::BadChecksum(path));
        }
        hashes.push((path.clone(), expected));
        files.insert(path, data);
    }
    if cur.at != bytes.len() {
        return Err(WbxError::Truncated);
    }

    if header.manifest.game_type == GameType::Bundled && !files.contains_key(&header.manifest.entry_point) {
        return Err(WbxError::MissingEntry(header.manifest.entry_point.clone()));
    }
    // Packages from the first packager (1.0.x) used a different checksum recipe; their files are still verified one by one.
    if header.version.starts_with("1.1") {
        hashes.sort_by(|a, b| a.0.cmp(&b.0));
        if package_checksum(&header.manifest, &hashes)? != header.checksum {
            return Err(WbxError::BadChecksum("the package as a whole".to_string()));
        }
    }
    Ok(Package { header, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            name: "Test Game".into(),
            id: "test-game".into(),
            version: "1.0.0".into(),
            description: "A test".into(),
            author: "Tester".into(),
            entry_point: "index.html".into(),
            category: "arcade".into(),
            tags: vec!["test".into()],
            rating: "E".into(),
            webbox: WebboxConfig {
                min_version: "1.0.0".into(),
                permissions: vec!["audio".into()],
                features: Features { multiplayer: false, achievements: false, leaderboards: false, cloud_save: false },
            },
            technical: Technical {
                renderer: "canvas".into(),
                orientation: "landscape".into(),
                fullscreen: true,
                resolution: Resolution { width: 1920, height: 1080, scalable: true, aspect_ratio: "16:9".into() },
            },
            game_type: GameType::Bundled,
            url: None,
            icon: None,
            screenshots: None,
            achievements: None,
            leaderboards: None,
        }
    }

    fn files() -> BTreeMap<String, Vec<u8>> {
        let mut f = BTreeMap::new();
        f.insert("index.html".to_string(), b"<html><script src=\"main.js\"></script></html>".to_vec());
        f.insert("main.js".to_string(), b"console.log('http://example.com'); // keep me".to_vec());
        f
    }

    fn build() -> Vec<u8> {
        build_package(&manifest(), &files(), 6, Some("2026-01-01T00:00:00+00:00"), &Limits::default()).unwrap()
    }

    #[test]
    fn round_trips_every_file_exactly() {
        let pkg = read_package(&build(), &Limits::default()).unwrap();
        assert_eq!(pkg.files, files());
        assert_eq!(pkg.header.manifest, manifest());
        assert_eq!(pkg.header.version, FORMAT_VERSION);
    }

    #[test]
    fn the_same_game_always_packs_to_the_same_bytes() {
        assert_eq!(build(), build());
        // and the input order must not matter: BTreeMap sorts, so insert in a different order and compare
        let mut shuffled = BTreeMap::new();
        for (k, v) in files().into_iter().rev() {
            shuffled.insert(k, v);
        }
        let again = build_package(&manifest(), &shuffled, 6, Some("2026-01-01T00:00:00+00:00"), &Limits::default()).unwrap();
        assert_eq!(build(), again);
    }

    #[test]
    fn without_a_timestamp_it_is_still_reproducible() {
        let a = build_package(&manifest(), &files(), 6, None, &Limits::default()).unwrap();
        let b = build_package(&manifest(), &files(), 6, None, &Limits::default()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn code_inside_files_is_never_altered() {
        let pkg = read_package(&build(), &Limits::default()).unwrap();
        assert_eq!(pkg.files["main.js"], b"console.log('http://example.com'); // keep me".to_vec());
    }

    #[test]
    fn detects_a_changed_file() {
        let mut bytes = build();
        let n = bytes.len();
        bytes[n - 12] ^= 0xff;
        let err = read_package(&bytes, &Limits::default()).unwrap_err();
        assert!(matches!(err.code(), "bad_checksum" | "bad_compression" | "bad_size"), "got {:?}", err);
    }

    #[test]
    fn detects_a_changed_manifest_through_the_package_checksum() {
        let bytes = build();
        // flip the rating letter inside the header JSON ("rating":"E" -> "T")
        let needle = b"\"rating\":\"E\"";
        let pos = bytes.windows(needle.len()).position(|w| w == needle).unwrap();
        let mut tampered = bytes.clone();
        tampered[pos + 10] = b'T';
        assert_eq!(read_package(&tampered, &Limits::default()).unwrap_err().code(), "bad_checksum");
    }

    #[test]
    fn rejects_non_wbx_and_cut_short_packages() {
        assert_eq!(read_package(b"", &Limits::default()).unwrap_err().code(), "bad_magic");
        assert_eq!(read_package(b"PK\x03\x04 zip", &Limits::default()).unwrap_err().code(), "bad_magic");
        let good = build();
        for cut in [5, 9, 40, good.len() - 5] {
            assert_eq!(read_package(&good[..cut], &Limits::default()).unwrap_err().code(), "truncated", "cut at {}", cut);
        }
    }

    #[test]
    fn refuses_unsafe_paths_and_programs_when_building() {
        for bad in ["../evil.js", "/etc/passwd", "a/../../x", "C:/x.js", "a\\b.js", "a//b.js", "./x.js"] {
            let mut f = files();
            f.insert(bad.to_string(), b"x".to_vec());
            assert_eq!(build_package(&manifest(), &f, 6, None, &Limits::default()).unwrap_err().code(), "unsafe_path", "{}", bad);
        }
        let mut f = files();
        f.insert("tools/run.exe".to_string(), b"MZ".to_vec());
        assert_eq!(build_package(&manifest(), &f, 6, None, &Limits::default()).unwrap_err().code(), "forbidden_file");
        let mut ok = files();
        ok.insert("game.wasm".to_string(), vec![0, 97, 115, 109]);
        assert!(build_package(&manifest(), &ok, 6, None, &Limits::default()).is_ok());
    }

    #[test]
    fn refuses_a_decompression_bomb_that_lies_about_its_size() {
        // Build a valid package, then lower the first file's declared original size so the data "grows" past it.
        let mut f = BTreeMap::new();
        f.insert("index.html".to_string(), vec![b'a'; 200_000]);
        let mut bytes = build_package(&manifest(), &f, 9, None, &Limits::default()).unwrap();
        let header_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let mut at = 8 + header_len + 4; // after the file count
        let path_len = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        at += 4 + path_len; // now at "original size"
        bytes[at..at + 4].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(read_package(&bytes, &Limits::default()).unwrap_err().code(), "bad_size");
    }

    #[test]
    fn enforces_limits() {
        let tiny = Limits { max_package_bytes: 10, ..Limits::default() };
        assert_eq!(read_package(&build(), &tiny).unwrap_err().code(), "too_big");
        let few = Limits { max_files: 1, ..Limits::default() };
        assert_eq!(build_package(&manifest(), &files(), 6, None, &few).unwrap_err().code(), "too_many_files");
        assert_eq!(build_package(&manifest(), &files(), 10, None, &Limits::default()).unwrap_err().code(), "bad_level");
    }

    #[test]
    fn checks_the_manifest_rules() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Manifest)>)> = vec![
            ("id", Box::new(|m| m.id = "Bad Id!".into())),
            ("id dots", Box::new(|m| m.id = "com.test.game".into())),
            ("version", Box::new(|m| m.version = "one".into())),
            ("rating", Box::new(|m| m.rating = "X".into())),
            ("permission", Box::new(|m| m.webbox.permissions = vec!["camera".into()])),
            ("renderer", Box::new(|m| m.technical.renderer = "vulkan".into())),
            ("entry not html", Box::new(|m| m.entry_point = "main.js".into())),
            ("bundled with url", Box::new(|m| m.url = Some("https://x.example.com".into()))),
            ("name", Box::new(|m| m.name = "   ".into())),
        ];
        for (what, change) in cases {
            let mut m = manifest();
            change(&mut m);
            assert!(m.validate().is_err(), "{} should be rejected", what);
        }
    }

    #[test]
    fn a_hosted_game_is_just_a_listing_with_an_https_address() {
        let mut m = manifest();
        m.game_type = GameType::Hosted;
        m.entry_point = String::new();
        m.url = Some("https://lagoslife.eliysites.com/".into());
        m.icon = Some("icon.png".into());
        let mut f = BTreeMap::new();
        f.insert("icon.png".to_string(), vec![137, 80, 78, 71]);
        let bytes = build_package(&m, &f, 6, None, &Limits::default()).unwrap();
        let pkg = read_package(&bytes, &Limits::default()).unwrap();
        assert_eq!(pkg.header.manifest.game_type, GameType::Hosted);
        assert_eq!(pkg.header.manifest.url.as_deref(), Some("https://lagoslife.eliysites.com/"));
        for bad in ["http://x.example.com", "https://", "https://user:pw@x.example.com", "ftp://x.example.com", "https://nodot", "https://x.example.com/a b"] {
            let mut h = m.clone();
            h.url = Some(bad.to_string());
            assert!(h.validate().is_err(), "{}", bad);
        }
        let mut no_url = m.clone();
        no_url.url = None;
        assert!(no_url.validate().is_err());
    }

    #[test]
    fn manifest_files_must_exist() {
        let mut m = manifest();
        m.icon = Some("icon.png".into());
        assert_eq!(build_package(&m, &files(), 6, None, &Limits::default()).unwrap_err().code(), "bad_manifest");
    }

    #[test]
    fn still_reads_a_package_made_by_the_first_packager() {
        // Made by the original wbkit-core (format 1.0.0, random file order, old checksum recipe).
        let old = include_bytes!("../tests/fixtures/first-packager.wbx");
        let pkg = read_package(old, &Limits::default()).unwrap();
        assert_eq!(pkg.header.version, "1.0.0");
        assert_eq!(pkg.header.manifest.game_type, GameType::Bundled);
        assert!(pkg.files.contains_key("index.html") && pkg.files.contains_key("main.js"));
    }

    #[test]
    fn engagement_definitions_are_checked_and_round_trip() {
        let mut m = manifest();
        m.achievements = Some(vec![Achievement { id: "first-win".into(), name: "First win".into(), description: "Win once.".into(), points: 10, hidden: false }]);
        assert!(m.validate().is_err(), "needs the achievements feature");
        m.webbox.features.achievements = true;
        m.validate().unwrap();
        m.leaderboards = Some(vec![Leaderboard { id: "high-score".into(), name: "High score".into(), order: "desc".into(), format: "number".into() }]);
        m.webbox.features.leaderboards = true;
        m.validate().unwrap();
        let package = build_package(&m, &files(), 6, None, &Limits::default()).unwrap();
        assert_eq!(read_package(&package, &Limits::default()).unwrap().header.manifest, m);
        m.achievements.as_mut().unwrap()[0].points = 101;
        assert!(m.validate().is_err());
        m.achievements.as_mut().unwrap()[0].points = 10;
        m.leaderboards.as_mut().unwrap()[0].order = "up".into();
        assert!(m.validate().is_err());
    }
}
