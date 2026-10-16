//! Collects the files to pack: everything under the folder except system leftovers, version control, dependencies
//! and other packages. Package paths always use forward slashes.

use std::{collections::BTreeMap, fs, path::Path};
use walkdir::WalkDir;

fn ignored(name: &str, is_dir: bool) -> bool {
    if is_dir { return name == ".git" || name == "node_modules"; }
    name == ".DS_Store" || name == "Thumbs.db" || name.starts_with("._") || name.to_ascii_lowercase().ends_with(".wbx")
}

pub fn collect(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let walker = WalkDir::new(dir).follow_links(false).into_iter().filter_entry(|entry| {
        entry.depth() == 0 || !ignored(&entry.file_name().to_string_lossy(), entry.file_type().is_dir())
    });
    for entry in walker {
        let entry = entry.map_err(|e| format!("can't read the folder: {}", e))?;
        if !entry.file_type().is_file() { continue; }
        let relative = entry.path().strip_prefix(dir).map_err(|e| e.to_string())?;
        let parts: Vec<String> = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        let data = fs::read(entry.path()).map_err(|e| format!("can't read {}: {}", relative.display(), e))?;
        files.insert(parts.join("/"), data);
    }
    Ok(files)
}
