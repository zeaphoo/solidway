//! 隔离守卫：方向之间零引用，`common` 不依赖协议语义。

use std::fs;
use std::path::{Path, PathBuf};

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rs_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn direction_modules_do_not_reference_each_other() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directions = root.join("src/directions");
    let names: Vec<String> = fs::read_dir(&directions)
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();

    for name in &names {
        for file in rs_files(&directions.join(name)) {
            let source = read(&file);
            for other in names.iter().filter(|other| *other != name) {
                for needle in [
                    format!("crate::directions::{other}"),
                    format!("super::{other}"),
                ] {
                    assert!(
                        !source.contains(&needle),
                        "{} references another direction via {needle}",
                        file.display()
                    );
                }
            }
        }
    }
}

#[test]
fn common_does_not_depend_on_protocol_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in rs_files(&root.join("src/common")) {
        let source = read(&file);
        for needle in ["crate::wire", "crate::directions", "crate::dialect"] {
            assert!(
                !source.contains(needle),
                "{} must not depend on {needle}",
                file.display()
            );
        }
    }
}

#[test]
fn wire_does_not_depend_on_directions_or_dialects() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in rs_files(&root.join("src/wire")) {
        let source = read(&file);
        for needle in ["crate::directions", "crate::dialect"] {
            assert!(
                !source.contains(needle),
                "{} must not depend on {needle}",
                file.display()
            );
        }
    }
}

#[test]
fn dialect_does_not_depend_on_directions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dialect = root.join("src/dialect");
    if !dialect.exists() {
        return;
    }
    for file in rs_files(&dialect) {
        let source = read(&file);
        assert!(
            !source.contains("crate::directions"),
            "{} must not depend on crate::directions",
            file.display()
        );
    }
}
