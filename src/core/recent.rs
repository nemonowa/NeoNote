// 最近開いたファイルの一覧。neodir/config.txt の `recent=` に保存される（core/config.rs）。
use std::path::{Path, PathBuf};

pub const MAX: usize = 10;

/// 先頭に入れる。すでにあれば先頭へ移し、MAX を超えた分は捨てる。
pub fn push(list: &mut Vec<PathBuf>, p: PathBuf) {
    list.retain(|x| x != &p);
    list.insert(0, p);
    list.truncate(MAX);
}

/// FLTK のメニューで特別扱いされる文字（`/` 階層、`&` ショートカット、`@` 記号）を無害にする。
fn clean(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '/' | '\\' | '&' | '@') { '_' } else { c }).collect()
}

/// メニューの表示: `1  name.txt  (folder)`。同名ファイルを見分けられるよう、親フォルダ名を添える。
pub fn label(index: usize, p: &Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let parent = p.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    match parent {
        Some(f) => format!("{}  {}  ({})", index + 1, clean(&name), clean(&f)),
        None => format!("{}  {}", index + 1, clean(&name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_deduped_and_capped() {
        let mut v = vec![];
        for i in 0..12 {
            push(&mut v, PathBuf::from(format!("/a/{i}.txt")));
        }
        assert_eq!(v.len(), MAX);
        assert_eq!(v[0], PathBuf::from("/a/11.txt"));
        push(&mut v, PathBuf::from("/a/5.txt"));
        assert_eq!(v[0], PathBuf::from("/a/5.txt"));
        assert_eq!(v.iter().filter(|p| **p == PathBuf::from("/a/5.txt")).count(), 1);
    }

    #[test]
    fn labels_are_menu_safe_and_distinguish_folders() {
        assert_eq!(label(0, Path::new("/x/mods/notes.txt")), "1  notes.txt  (mods)");
        assert_eq!(label(1, Path::new("/x/a&b/@c.txt")), "2  _c.txt  (a_b)");
        assert!(!label(0, Path::new("/x/y/z.txt")).contains(['/', '&', '@']));
    }
}
