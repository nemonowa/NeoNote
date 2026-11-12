// 面積図の四角にマウスを乗せた・押したときの、簡単なプレビューの中身（GUI なし）。
// ファイルは先頭の数行（文字のファイルだけ）、フォルダは中の大きい順の数件。
use super::archive::Entry;

/// プレビューに出す行の上限と、1 行の文字数の上限。
pub const MAX_LINES: usize = 14;
pub const MAX_COLS: usize = 80;
/// 先頭を読む量（バイト）。
pub const HEAD_BYTES: usize = 8 * 1024;

/// 文字のファイルなら先頭の数行（タブは空白に、長い行は切って `…`）。NUL を含む・UTF-8 として崩れが多いものは None（バイナリ）。
pub fn text_head(bytes: &[u8]) -> Option<Vec<String>> {
    let b = &bytes[..bytes.len().min(HEAD_BYTES)];
    if b.contains(&0) {
        return None;
    }
    // 終わりで文字が切れているのは許す（先頭だけを読んだため）
    let s = String::from_utf8_lossy(b);
    let bad = s.chars().filter(|&c| c == '\u{FFFD}').count();
    if bad > 4 + s.chars().count() / 50 {
        return None;
    }
    let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
    let mut out = Vec::new();
    for line in s.lines().take(MAX_LINES) {
        let line = line.replace('\t', "    ");
        let line = line.trim_end();
        if line.chars().count() > MAX_COLS {
            out.push(line.chars().take(MAX_COLS).collect::<String>() + "…");
        } else {
            out.push(line.to_string());
        }
    }
    Some(out)
}

/// フォルダ `dir`（末尾の `/` は有っても無くても）の直下の項目を、大きい順に (名前, 大きさ, フォルダか)。あわせて中のファイルの総数。
pub fn dir_children(entries: &[Entry], dir: &str) -> (Vec<(String, u64, bool)>, usize) {
    let prefix = format!("{}/", dir.trim_end_matches('/'));
    let mut kids: std::collections::HashMap<&str, (u64, bool)> = std::collections::HashMap::new();
    let mut files = 0;
    for e in entries {
        let Some(rest) = e.path.strip_prefix(&prefix) else { continue };
        let rest = rest.trim_end_matches('/');
        if rest.is_empty() {
            continue;
        }
        if !e.is_dir {
            files += 1;
        }
        let (name, deeper) = match rest.split_once('/') {
            Some((n, _)) => (n, true),
            None => (rest, false),
        };
        let k = kids.entry(name).or_insert((0, false));
        if !e.is_dir {
            k.0 += e.size;
        }
        k.1 |= deeper || e.is_dir;
    }
    let mut v: Vec<(String, u64, bool)> = kids.into_iter().map(|(n, (s, d))| (n.to_string(), s, d)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    (v, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(p: &str, s: u64) -> Entry {
        Entry { path: p.into(), is_dir: p.ends_with('/'), size: s, packed: None }
    }

    #[test]
    fn text_head_takes_first_lines_and_rejects_binary() {
        let t = text_head(b"\xEF\xBB\xBFa\tb\nline2\r\n").unwrap();
        assert_eq!(t, vec!["a    b", "line2"]);
        let long = "x".repeat(200);
        assert!(text_head(long.as_bytes()).unwrap()[0].ends_with('…'));
        assert_eq!(text_head(&(0..40).map(|i| format!("{i}\n")).collect::<String>().into_bytes()).unwrap().len(), MAX_LINES);
        assert!(text_head(b"PK\x03\x04\x00\x00").is_none());
        assert!(text_head(&[0xFF; 300]).is_none());
        // 先頭だけ読んで、最後の文字が途中で切れていても文字のファイル
        let mut j = "あいう".repeat(10).into_bytes();
        j.pop();
        assert!(text_head(&j).is_some());
    }

    #[test]
    fn dir_children_groups_and_sorts_by_size() {
        let es = [e("d/", 0), e("d/a.txt", 5), e("d/sub/", 0), e("d/sub/x", 10), e("d/sub/y", 1), e("other/z", 99)];
        let (kids, files) = dir_children(&es, "d/");
        assert_eq!(kids, vec![("sub".into(), 11, true), ("a.txt".into(), 5, false)]);
        assert_eq!(files, 3);
        assert_eq!(dir_children(&es, "d").1, 3);
    }
}
