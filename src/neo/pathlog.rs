// 過去に読み込んだ（開いた・保存した）ファイルのパス履歴。View History / Restore From History で使う。
// config.txt の `history=` に保存され、終了後も残る（core/config.rs）。最大 MAX 件、新しい順。
// 「最近開いたファイル」（core/recent.rs、最大 10 件）とは別の、長い履歴。
use std::path::{Path, PathBuf};

pub const MAX: usize = 100;

/// 先頭に入れる。すでにあれば先頭へ移し、MAX を超えた分は捨てる。
pub fn push(list: &mut Vec<PathBuf>, p: PathBuf) {
    list.retain(|x| x != &p);
    list.insert(0, p);
    list.truncate(MAX);
}

/// ファイル名の表示上限（文字数）。超えたら先頭 HEAD 文字 + `…` + 末尾 TAIL 文字（拡張子を残す）。
const NAME_MAX: usize = 60;
const HEAD: usize = 40;
const TAIL: usize = 19;

/// 一覧の表示: `1  name.txt`（ファイル名のみ。長ければ `先頭…末尾` に省略）。
/// FLTK の Browser の特殊文字（`@` 書式、タブの桁区切り）は `_` にする。
pub fn label(index: usize, p: &Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let chars: Vec<char> = name.chars().map(|c| if c.is_control() || c == '@' { '_' } else { c }).collect();
    let shown: String = if chars.len() > NAME_MAX {
        chars[..HEAD].iter().collect::<String>() + "…" + &chars[chars.len() - TAIL..].iter().collect::<String>()
    } else {
        chars.iter().collect()
    };
    format!("{}  {}", index + 1, shown)
}

/// 存在しないパスを履歴から消す。消した件数を返す。
pub fn prune_missing(list: &mut Vec<PathBuf>) -> usize {
    let before = list.len();
    list.retain(|p| p.exists());
    before - list.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_deduped_and_capped_at_100() {
        let mut v = vec![];
        for i in 0..120 {
            push(&mut v, PathBuf::from(format!("/a/{i}.txt")));
        }
        assert_eq!(v.len(), 100);
        assert_eq!(v[0], PathBuf::from("/a/119.txt"));
        assert_eq!(v[99], PathBuf::from("/a/20.txt"));
        push(&mut v, PathBuf::from("/a/50.txt"));
        assert_eq!(v[0], PathBuf::from("/a/50.txt"));
        assert_eq!(v.iter().filter(|p| **p == PathBuf::from("/a/50.txt")).count(), 1);
        assert_eq!(v.len(), 100);
    }

    #[test]
    fn label_is_file_name_only_browser_safe_and_long_names_are_shortened() {
        assert_eq!(label(0, Path::new("/x/mods/notes.txt")), "1  notes.txt");
        assert!(!label(1, Path::new("/x/@b\tc.txt")).contains(['@', '\t']));
        let long = format!("/x/{}.rs", "あ".repeat(80));
        let l = label(2, Path::new(&long));
        assert!(l.starts_with("3  あ") && l.ends_with(".rs") && l.contains('…'));
        assert_eq!(l.chars().count(), "3  ".len() + HEAD + 1 + TAIL);
    }

    #[test]
    fn prune_missing_drops_only_absent_paths() {
        let dir = std::env::temp_dir().join(format!("neonote-pathlog-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.txt");
        std::fs::write(&ok, "x").unwrap();
        let mut v = vec![dir.join("gone.txt"), ok.clone()];
        assert_eq!(prune_missing(&mut v), 1);
        assert_eq!(v, vec![ok]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
