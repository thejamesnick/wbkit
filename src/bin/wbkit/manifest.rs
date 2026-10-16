//! Reads the developer's manifest.json, refuses unknown fields (the upload server does too), and fills only the
//! gaps that have a safe default. Name and description can come from index.html; author and rating never default.

use serde_json::{json, Map, Value};
use std::{fs, path::Path};
use wbkit_core::wbx::Manifest;

const TOP: &[&str] = &["name", "id", "version", "description", "author", "entry_point", "type", "url", "category", "tags", "rating", "icon", "screenshots", "webbox", "technical"];
const WEBBOX: &[&str] = &["min_version", "permissions", "features"];
const FEATURES: &[&str] = &["multiplayer", "achievements", "leaderboards", "cloud_save"];
const TECHNICAL: &[&str] = &["renderer", "orientation", "fullscreen", "resolution"];
const RESOLUTION: &[&str] = &["width", "height", "scalable", "aspect_ratio"];

fn check_keys(value: &Value, known: &[&str], at: &str) -> Result<(), String> {
    let Some(object) = value.as_object() else { return Err(format!("{} must be an object", if at.is_empty() { "the manifest" } else { at })) };
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            return Err(format!("unknown field \"{}{}\" (check the spelling; see https://publish.webbox.fun/docs/manifest)", at, key));
        }
    }
    Ok(())
}

fn check_all(manifest: &Value) -> Result<(), String> {
    check_keys(manifest, TOP, "")?;
    if let Some(webbox) = manifest.get("webbox") {
        check_keys(webbox, WEBBOX, "webbox.")?;
        if let Some(features) = webbox.get("features") { check_keys(features, FEATURES, "webbox.features.")?; }
    }
    if let Some(technical) = manifest.get("technical") {
        check_keys(technical, TECHNICAL, "technical.")?;
        if let Some(resolution) = technical.get("resolution") { check_keys(resolution, RESOLUTION, "technical.resolution.")?; }
    }
    Ok(())
}

/// Copies every field of `ours` that `theirs` lacks, recursing into objects. The developer's values always win.
fn fill(theirs: &mut Map<String, Value>, ours: Value) {
    let Value::Object(ours) = ours else { return };
    for (key, default) in ours {
        match theirs.get_mut(&key) {
            Some(Value::Object(inner)) if default.is_object() => fill(inner, default),
            Some(_) => {}
            None => { theirs.insert(key, default); }
        }
    }
}

/// `<title>` and `<meta name="description">` from the entry page, if there is one.
fn page_details(dir: &Path, entry: &str) -> (Option<String>, Option<String>) {
    let Ok(html) = fs::read_to_string(dir.join(entry)) else { return (None, None) };
    let lower = html.to_ascii_lowercase();
    let title = lower.find("<title>").and_then(|start| {
        let rest = &html[start + 7..];
        rest.to_ascii_lowercase().find("</title>").map(|end| rest[..end].trim().to_string())
    });
    let description = lower.find("name=\"description\"").and_then(|at| {
        let rest = &html[at..];
        let start = rest.find("content=\"")? + 9;
        let end = rest[start..].find('"')?;
        Some(rest[start..start + end].trim().to_string())
    });
    (title.filter(|t| !t.is_empty()), description.filter(|d| !d.is_empty()))
}

pub fn slug(name: &str) -> String {
    let raw: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    raw.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
}

fn defaults(dir: &Path, given: &Map<String, Value>) -> Value {
    let hosted = given.get("type").and_then(Value::as_str) == Some("hosted");
    let entry = given.get("entry_point").and_then(Value::as_str).unwrap_or(if hosted { "" } else { "index.html" }).to_string();
    let (title, description) = if hosted { (None, None) } else { page_details(dir, &entry) };
    let name = given.get("name").and_then(Value::as_str).map(str::to_string).or(title).unwrap_or_default();
    let orientation = given.get("technical").and_then(|t| t.get("orientation")).and_then(Value::as_str).unwrap_or("landscape").to_string();
    json!({
        "name": name,
        "id": slug(&name),
        "version": "1.0.0",
        "description": description.unwrap_or_default(),
        "entry_point": entry,
        "type": "bundled",
        "category": "arcade",
        "tags": [],
        "webbox": { "min_version": "1.0.0", "permissions": ["audio"], "features": { "multiplayer": false, "achievements": false, "leaderboards": false, "cloud_save": false } },
        "technical": { "renderer": "canvas", "orientation": orientation, "fullscreen": true,
            "resolution": { "width": 1920, "height": 1080, "scalable": true, "aspect_ratio": if orientation == "portrait" { "9:16" } else { "16:9" } } },
    })
}

pub fn load(dir: &Path, manifest_path: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(manifest_path).map_err(|e| format!("can't read {}: {}", manifest_path.display(), e))?;
    let mut value: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {}", manifest_path.display(), e))?;
    check_all(&value)?;
    let object = value.as_object_mut().ok_or("the manifest must be an object")?;
    for required in ["author", "rating"] {
        if !object.contains_key(required) { return Err(format!("the manifest needs \"{}\"", required)); }
    }
    let ours = defaults(dir, object);
    fill(object, ours);
    let manifest: Manifest = serde_json::from_value(value).map_err(|e| format!("the manifest is invalid: {}", e))?;
    manifest.validate().map_err(|e| e.to_string())?;
    Ok(manifest)
}

/// The starter manifest `wbkit init` writes: the page title becomes the name, everything else is a sensible start.
pub fn starter(dir: &Path) -> Value {
    let (title, description) = page_details(dir, "index.html");
    let name = title.unwrap_or_else(|| dir.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "My Game".into()));
    let mut value = json!({ "name": name, "id": slug(&name), "version": "1.0.0", "description": description.unwrap_or_default(), "author": "Your Studio", "rating": "E" });
    let ours = defaults(dir, value.as_object().unwrap());
    fill(value.as_object_mut().unwrap(), ours);
    value
}
