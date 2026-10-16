//! Friendly warnings that don't stop a build: things that will work locally but not on WebBox.

use std::collections::BTreeMap;

/// Every `<script src="https://...">` (or `http://`, `//`) in the package's HTML files. Bundled titles can only
/// load scripts from their own package on WebBox, so these would fail for players.
pub fn external_scripts(files: &BTreeMap<String, Vec<u8>>) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (path, bytes) in files {
        let lower_path = path.to_ascii_lowercase();
        if !(lower_path.ends_with(".html") || lower_path.ends_with(".htm")) { continue; }
        let html = String::from_utf8_lossy(bytes);
        let lower = html.to_ascii_lowercase();
        let mut at = 0;
        while let Some(start) = lower[at..].find("<script") {
            let tag_start = at + start;
            let tag_end = lower[tag_start..].find('>').map(|e| tag_start + e).unwrap_or(lower.len());
            let tag = &lower[tag_start..tag_end];
            if let Some(src_at) = tag.find("src=") {
                let rest = &html[tag_start + src_at + 4..tag_end];
                let value: String = rest.trim_start_matches(['"', '\'']).chars().take_while(|c| *c != '"' && *c != '\'' && !c.is_whitespace()).collect();
                let v = value.to_ascii_lowercase();
                if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("//") { found.push((path.clone(), value)); }
            }
            at = tag_end.max(tag_start + 1);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_scripts_from_other_websites_only() {
        let mut files = BTreeMap::new();
        files.insert("index.html".to_string(), br#"<script src="webbox.js"></script><SCRIPT SRC='https://webbox.fun/sdk/v1/webbox.js'></SCRIPT><script>inline()</script><script type="module" src="//cdn.example.com/a.js"></script>"#.to_vec());
        files.insert("game.js".to_string(), b"<script src=\"https://not-html.example\">".to_vec());
        assert_eq!(external_scripts(&files), vec![
            ("index.html".to_string(), "https://webbox.fun/sdk/v1/webbox.js".to_string()),
            ("index.html".to_string(), "//cdn.example.com/a.js".to_string()),
        ]);
    }
}
