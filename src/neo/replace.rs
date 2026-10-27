// 検索・全置換。GUI 非依存。どちらも全文を 1 パスで走査する。

/// `needle` の重ならない出現位置を、バイト範囲 (start, end) で返す。
/// 返す位置は UTF-8 の文字境界なので、そのまま TextBuffer の位置に使える。
pub fn find_all(text: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    text.match_indices(needle).map(|(i, m)| (i, i + m.len())).collect()
}

/// Ctrl+F 用の検索（スマートケース）。`needle` に ASCII の大文字が 1 つでもあれば大文字小文字を区別し、
/// 無ければ ASCII の大文字小文字を無視する。操作は増やさない（大文字で打てば区別、小文字で打てば無視）。
/// ASCII だけを畳むので、文字のバイト長が変わらず、返す位置は `text` にそのまま使える。
/// ASCII 以外（É と é、全角英字など）は畳まない。
pub fn find_all_smart(text: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.bytes().any(|b| b.is_ascii_uppercase()) {
        return find_all(text, needle);
    }
    find_all(&text.to_ascii_lowercase(), needle)
}

/// `target` を全て `with` に置き換える。1 つも無ければ None。
/// 戻り値は (新しい文字列, 置換した個数)。
pub fn replace_all(text: &str, target: &str, with: &str) -> Option<(String, usize)> {
    if target.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut n = 0;
    for (i, m) in text.match_indices(target) {
        out.push_str(&text[last..i]);
        out.push_str(with);
        last = i + m.len();
        n += 1;
    }
    if n == 0 {
        return None;
    }
    out.push_str(&text[last..]);
    Some((out, n))
}

/// `hits`（`find_all` / `find_all_smart` の結果。昇順で重ならない）の箇所を全て `with` に置き換えた文字列。
pub fn apply_replace(text: &str, hits: &[(usize, usize)], with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for &(s, e) in hits {
        out.push_str(&text[last..s]);
        out.push_str(with);
        last = e;
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_replace_swaps_each_hit_and_keeps_the_rest() {
        let t = "abc ABC aBc x";
        let hits = find_all_smart(t, "abc");
        assert_eq!(apply_replace(t, &hits, "-"), "- - - x");
        assert_eq!(apply_replace(t, &[], "-"), t);
        let t = "あいうあい";
        assert_eq!(apply_replace(t, &find_all(t, "あい"), ""), "う");
    }

    #[test]
    fn smart_case_lowercase_needle_ignores_ascii_case() {
        assert_eq!(find_all_smart("abc ABC aBc", "abc"), vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn smart_case_uppercase_in_needle_is_exact() {
        assert_eq!(find_all_smart("abc ABC aBc", "ABC"), vec![(4, 7)]);
        assert_eq!(find_all_smart("abc ABC aBc", "aBc"), vec![(8, 11)]);
    }

    #[test]
    fn smart_case_offsets_are_valid_after_multibyte() {
        let t = "あいう Hello えお hello";
        let hits = find_all_smart(t, "hello");
        assert_eq!(hits.len(), 2);
        for (s, e) in hits {
            assert!(t.is_char_boundary(s) && t.is_char_boundary(e));
            assert!(t[s..e].eq_ignore_ascii_case("hello"));
        }
    }

    #[test]
    fn smart_case_does_not_fold_non_ascii() {
        assert_eq!(find_all_smart("É é", "é"), vec![(3, 5)]);
    }

    #[test]
    fn find_is_non_overlapping_and_on_char_boundaries() {
        assert_eq!(find_all("aaaa", "aa"), vec![(0, 2), (2, 4)]);
        let t = "あいあ";
        let hits = find_all(t, "あ");
        assert_eq!(hits, vec![(0, 3), (6, 9)]);
        assert!(hits.iter().all(|&(s, e)| t.is_char_boundary(s) && t.is_char_boundary(e)));
        assert!(find_all("abc", "").is_empty());
    }

    #[test]
    fn replace_all_in_one_pass_does_not_rescan_replacement() {
        // Java 版は置換後を再走査していたが、ここでは置換結果の中は見ない
        assert_eq!(replace_all("ab ab", "ab", "abab"), Some(("abab abab".into(), 2)));
        assert_eq!(replace_all("あ-あ", "あ", ""), Some(("-".into(), 2)));
        assert_eq!(replace_all("abc", "x", "y"), None);
        assert_eq!(replace_all("abc", "", "y"), None);
    }
}
