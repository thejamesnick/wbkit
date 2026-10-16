//! Runs the real `wbkit` program against throwaway game folders.

use std::{fs, path::PathBuf, process::Command};

fn wbkit(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_wbkit")).args(args).output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn game(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wbkit-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("js")).unwrap();
    fs::write(dir.join("index.html"), "<title>Space Rocks</title><meta name=\"description\" content=\"Blast rocks.\"><script src=\"js/game.js\"></script>").unwrap();
    fs::write(dir.join("js/game.js"), "// http://example.com keeps working\nconsole.log('hi')").unwrap();
    dir
}

fn s(path: &PathBuf) -> &str { path.to_str().unwrap() }

#[test]
fn init_then_bundle_then_validate_and_info() {
    let dir = game("flow");
    let (ok, out) = wbkit(&["init", s(&dir)]);
    assert!(ok, "{out}");
    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["name"], "Space Rocks");
    assert_eq!(manifest["id"], "space-rocks");
    assert_eq!(manifest["description"], "Blast rocks.");
    assert!(!wbkit(&["init", s(&dir)]).0, "init must not overwrite");

    let package = dir.with_extension("wbx");
    let (ok, out) = wbkit(&["bundle", s(&dir), "-o", s(&package), "--reproducible"]);
    assert!(ok, "{out}");
    let (ok, out) = wbkit(&["validate", s(&package)]);
    assert!(ok && out.contains("Valid: Space Rocks 1.0.0"), "{out}");
    let (ok, out) = wbkit(&["info", s(&package)]);
    assert!(ok && out.contains("js/game.js") && out.contains("manifest.json"), "{out}");
}

#[test]
fn reproducible_builds_are_identical() {
    let dir = game("repro");
    wbkit(&["init", s(&dir)]);
    let (a, b) = (dir.join("..").join("repro-a.wbx"), dir.join("..").join("repro-b.wbx"));
    assert!(wbkit(&["bundle", s(&dir), "-o", s(&a), "--reproducible"]).0);
    assert!(wbkit(&["bundle", s(&dir), "-o", s(&b), "--reproducible"]).0);
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
}

#[test]
fn rejects_unknown_fields_and_missing_author_or_rating() {
    let dir = game("strict");
    fs::write(dir.join("manifest.json"), r#"{"name":"X","author":"A","rating":"E","catgory":"puzzle"}"#).unwrap();
    let (ok, out) = wbkit(&["bundle", s(&dir), "-o", s(&dir.with_extension("wbx"))]);
    assert!(!ok && out.contains("unknown field \"catgory\""), "{out}");
    fs::write(dir.join("manifest.json"), r#"{"name":"X","rating":"E"}"#).unwrap();
    let (ok, out) = wbkit(&["bundle", s(&dir), "-o", s(&dir.with_extension("wbx"))]);
    assert!(!ok && out.contains("needs \"author\""), "{out}");
}

#[test]
fn leaves_out_system_files_and_refuses_programs() {
    let dir = game("files");
    wbkit(&["init", s(&dir)]);
    fs::write(dir.join(".DS_Store"), "x").unwrap();
    fs::write(dir.join("._index.html"), "x").unwrap();
    fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
    fs::write(dir.join("node_modules/pkg/a.js"), "x").unwrap();
    let package = dir.with_extension("wbx");
    assert!(wbkit(&["bundle", s(&dir), "-o", s(&package)]).0);
    let (_, out) = wbkit(&["info", s(&package)]);
    assert!(!out.contains(".DS_Store") && !out.contains("._index") && !out.contains("node_modules"), "{out}");
    fs::write(dir.join("setup.exe"), "MZ").unwrap();
    let (ok, out) = wbkit(&["bundle", s(&dir), "-o", s(&package)]);
    assert!(!ok && out.contains("forbidden_file"), "{out}");
}

#[test]
fn enforces_the_upload_file_limit() {
    let dir = game("limit");
    wbkit(&["init", s(&dir)]);
    for i in 0..520 { fs::write(dir.join(format!("f{i}.txt")), "x").unwrap(); }
    let (ok, out) = wbkit(&["bundle", s(&dir), "-o", s(&dir.with_extension("wbx"))]);
    assert!(!ok && out.contains("512 files"), "{out}");
}

#[test]
fn says_where_to_start_without_a_manifest() {
    let dir = game("nomanifest");
    let (ok, out) = wbkit(&["bundle", s(&dir)]);
    assert!(!ok && out.contains("wbkit init"), "{out}");
}
