// データパックの雛形を作る（GUI なし）。
// 作る物: pack.mcmeta、data/<名前空間>/function/load.mcfunction・tick.mcfunction、
// data/minecraft/tags/function/load.json・tick.json（読み込み時と毎 tick に呼ぶ）。
// すでにあるファイルは上書きしない（作った物と、残した物を返す）。
use std::path::{Path, PathBuf};

/// pack.mcmeta に書く版（Minecraft Java 26.3 のデータパック）。
pub const MIN_FORMAT: (u32, u32) = (121, 0);
pub const MAX_FORMAT: u32 = 121;

/// 名前空間として使えるか（小文字の英数字と `_` `-` `.`）。
pub fn is_valid_namespace(ns: &str) -> bool {
    !ns.is_empty() && ns.len() <= 64 && ns.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.'))
}

/// 作るファイルの一覧（`root` からの相対パスと中身）。
pub fn files(ns: &str, description: &str) -> Vec<(PathBuf, String)> {
    let desc = description.replace('\\', "\\\\").replace('"', "\\\"");
    let (a, b) = MIN_FORMAT;
    let header = |name: &str, what: &str, caller: &str| {
        format!("# 命名：{name}\n# 説明：{what}\n# >/{caller}\n# =/function {ns}:{name}\n\n")
    };
    vec![
        (PathBuf::from("pack.mcmeta"), format!("{{\n  \"pack\": {{\n    \"description\": \"{desc}\",\n    \"min_format\": [{a}, {b}],\n    \"max_format\": {MAX_FORMAT}\n  }}\n}}\n")),
        (PathBuf::from(format!("data/{ns}/function/load.mcfunction")), header("load", "読み込み時に 1 回", "#minecraft:load") + "tellraw @a {text:\"loaded\",color:\"green\"}\n"),
        (PathBuf::from(format!("data/{ns}/function/tick.mcfunction")), header("tick", "毎 tick", "#minecraft:tick")),
        (PathBuf::from("data/minecraft/tags/function/load.json"), format!("{{\n  \"values\": [\"{ns}:load\"]\n}}\n")),
        (PathBuf::from("data/minecraft/tags/function/tick.json"), format!("{{\n  \"values\": [\"{ns}:tick\"]\n}}\n")),
    ]
}

pub struct Created {
    pub written: Vec<PathBuf>,
    pub kept: Vec<PathBuf>,
}

/// `root` に雛形を書く。すでにあるファイルは残す。
pub fn create(root: &Path, ns: &str, description: &str) -> std::io::Result<Created> {
    if !is_valid_namespace(ns) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "namespace"));
    }
    let mut c = Created { written: Vec::new(), kept: Vec::new() };
    for (rel, body) in files(ns, description) {
        let p = root.join(rel);
        if p.exists() {
            c.kept.push(p);
            continue;
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, body)?;
        c.written.push(p);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_rules() {
        assert!(is_valid_namespace("my_pack.v2-x"));
        assert!(!is_valid_namespace("My"));
        assert!(!is_valid_namespace(""));
        assert!(!is_valid_namespace("a/b"));
    }

    #[test]
    fn creates_files_and_keeps_existing_ones() {
        let d = std::env::temp_dir().join(format!("neonote_dp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pack.mcmeta"), "mine").unwrap();
        let c = create(&d, "demo", "say \"hi\"").unwrap();
        assert_eq!(c.kept.len(), 1);
        assert_eq!(c.written.len(), 4);
        assert_eq!(std::fs::read_to_string(d.join("pack.mcmeta")).unwrap(), "mine");
        let tag = std::fs::read_to_string(d.join("data/minecraft/tags/function/load.json")).unwrap();
        assert!(tag.contains("\"demo:load\""));
        // pack.mcmeta は JSON として正しい（説明の引用符はエスケープされる）
        let body = &files("demo", "say \"hi\"")[0].1;
        assert!(crate::neo::json::format(body).is_ok());
        assert!(create(&d, "Bad", "").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
