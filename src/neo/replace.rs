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
///
/// 全文の小文字化コピーは作らない（大きな文書でメモリと時間を食うため）。`needle` の先頭 1 バイトが
/// 出てくる位置（大文字・小文字の両方）だけを拾い、残りをその場で比べる。
/// 位置が文字境界になる理由: `needle` は UTF-8 として正しく、先頭は文字の先頭バイトなので、
/// 継続バイトの途中から一致が始まることはなく、全バイトが一致すれば終わりも文字の境目になる。
pub fn find_all_smart(text: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.bytes().any(|b| b.is_ascii_uppercase()) {
        return find_all(text, needle);
    }
    let (t, n) = (text.as_bytes(), needle.as_bytes());
    let mut hits = Vec::new();
    if n.is_empty() || n.len() > t.len() {
        return hits;
    }
    // `needle` に ASCII の大文字は無いので、`n` の各バイトはすでに畳んである。text 側だけ畳んで比べる
    let (first, first_upper) = (n[0], n[0].to_ascii_uppercase());
    let last_start = t.len() - n.len();
    let mut i = 0;
    while i <= last_start {
        match t[i..=last_start].iter().position(|&b| b == first || b == first_upper) {
            Some(offset) => i += offset,
            None => break,
        }
        if t[i + 1..i + n.len()].iter().zip(&n[1..]).all(|(a, b)| a.to_ascii_lowercase() == *b) {
            hits.push((i, i + n.len()));
            i += n.len(); // 重ならない出現だけ数える（find_all と同じ）
        } else {
            i += 1;
        }
    }
    hits
}

/// 正規表現・単語単位の検索。`pattern` が不正なら、利用者に見せる説明を Err で返す。
/// `regex` が false なら `pattern` は文字どおりの文字列として扱う（単語単位だけ使いたいとき）。
/// 空の一致（`a*` が何も無い所に一致するなど）は、置換で無限に増えるため数えない。
/// 返す位置は UTF-8 の文字境界で、`find_all` と同じく、そのまま TextBuffer の位置に使える。
/// `regex` crate は入力の長さに対して線形時間で動くので、危険なパターンを弾く処理は要らない。
fn build_regex(pattern: &str, is_regex: bool, ignore_case: bool, whole_word: bool) -> Result<regex::Regex, String> {
    let body = if is_regex { pattern.to_string() } else { regex::escape(pattern) };
    let src = if whole_word { format!(r"\b(?:{body})\b") } else { body };
    regex::RegexBuilder::new(&src).case_insensitive(ignore_case).size_limit(10 * 1024 * 1024).build().map_err(|e| e.to_string())
}

pub fn find_all_regex(text: &str, pattern: &str, is_regex: bool, ignore_case: bool, whole_word: bool) -> Result<Vec<(usize, usize)>, String> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let re = build_regex(pattern, is_regex, ignore_case, whole_word)?;
    Ok(re.find_iter(text).filter(|m| !m.is_empty()).map(|m| (m.start(), m.end())).collect())
}

/// 一致ごとの置換後の文字列つき。`is_regex` のとき `with` の `$1`・`${name}`・`$$` を展開する
/// （文字どおりの `$` は `$$`）。そうでなければ `with` をそのまま使う。
pub fn regex_replacements(text: &str, pattern: &str, is_regex: bool, ignore_case: bool, whole_word: bool, with: &str) -> Result<Vec<(usize, usize, String)>, String> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let re = build_regex(pattern, is_regex, ignore_case, whole_word)?;
    let mut out = Vec::new();
    for caps in re.captures_iter(text) {
        let m = caps.get(0).expect("group 0");
        if m.is_empty() {
            continue;
        }
        let mut s = String::new();
        if is_regex {
            caps.expand(with, &mut s);
        } else {
            s.push_str(with);
        }
        out.push((m.start(), m.end(), s));
    }
    Ok(out)
}

/// `regex_replacements` の結果を本文に適用する。
pub fn apply_replacements(text: &str, reps: &[(usize, usize, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (s, e, with) in reps {
        out.push_str(&text[last..*s]);
        out.push_str(with);
        last = *e;
    }
    out.push_str(&text[last..]);
    out
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

#[cfg(test)]
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

    /// コピーを作る前の実装。新しい実装が、これと同じ結果を返すことの基準にする。
    fn reference_smart(text: &str, needle: &str) -> Vec<(usize, usize)> {
        if needle.bytes().any(|b| b.is_ascii_uppercase()) {
            return find_all(text, needle);
        }
        find_all(&text.to_ascii_lowercase(), needle)
    }

    #[test]
    fn smart_case_matches_the_copying_version_on_tricky_inputs() {
        let texts = ["", "a", "aAaAaA", "AAAA", "aaa", "あaAあAaい", "É é É", "xXyY", "ﬁ FI fi", "日本語 TEXT text Text", "abcABCabc"];
        let needles = ["a", "aa", "aaa", "ab", "abc", "あa", "é", "xy", "text", "日本", "fi", "z", "aaaa", "ab c"];
        for t in texts {
            for n in needles {
                assert_eq!(find_all_smart(t, n), reference_smart(t, n), "text={t:?} needle={n:?}");
            }
        }
        assert!(find_all_smart("abc", "").is_empty());
        assert!(find_all_smart("ab", "abc").is_empty());
    }

    #[test]
    fn smart_case_matches_the_copying_version_on_pseudo_random_text() {
        // 外部の乱数部品は使わず、決まった種の線形合同法で作る（毎回同じ入力になる）
        let alphabet: Vec<char> = "aAbB cC\nあいÉé".chars().collect();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..300 {
            let len = next() % 60;
            let text: String = (0..len).map(|_| alphabet[next() % alphabet.len()]).collect();
            let n_len = 1 + next() % 3;
            let needle: String = (0..n_len).map(|_| alphabet[next() % alphabet.len()]).collect::<String>().to_ascii_lowercase();
            assert_eq!(find_all_smart(&text, &needle), reference_smart(&text, &needle), "text={text:?} needle={needle:?}");
        }
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
        #[test]
    fn regex_finds_non_empty_matches_on_char_boundaries() {
        let t = "abc123 あい45 xyz6";
        assert_eq!(find_all_regex(t, r"[0-9]+", true, false, false).unwrap(), vec![(3, 6), (13, 15), (19, 20)]);
        for (s, e) in find_all_regex(t, "あ.", true, false, false).unwrap() {
            assert!(t.is_char_boundary(s) && t.is_char_boundary(e));
        }
        assert!(find_all_regex("bbb", "a*", true, false, false).unwrap().is_empty()); // 空の一致は数えない
        assert!(find_all_regex("abc", "", true, false, false).unwrap().is_empty());
    }

        #[test]
    fn regex_case_and_whole_word_options() {
        assert_eq!(find_all_regex("Cat cat CAT concat", "cat", true, true, true).unwrap().len(), 3);
        assert_eq!(find_all_regex("Cat cat CAT concat", "cat", true, true, false).unwrap().len(), 4);
        assert_eq!(find_all_regex("Cat cat CAT", "cat", true, false, false).unwrap(), vec![(4, 7)]);
        assert_eq!(find_all_regex("a.b axb", r"a\.b", true, false, true).unwrap(), vec![(0, 3)]);
    }

        #[test]
    fn regex_errors_are_returned_not_panicked() {
        assert!(find_all_regex("x", "(", true, false, false).is_err());
        assert!(find_all_regex("x", "[a-", true, false, false).is_err());
        // 入力に対して線形時間: 古典的な危険パターンでも固まらない
        let evil = "a".repeat(5000) + "!";
        let t = std::time::Instant::now();
        assert!(find_all_regex(&evil, "(a+)+$", true, false, false).unwrap().is_empty());
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn word_only_treats_pattern_literally() {
        // regex でなければ `.` は文字どおりの点
        assert_eq!(find_all_regex("a.b axb a.bc", "a.b", false, false, true).unwrap(), vec![(0, 3)]);
        assert_eq!(find_all_regex("cat concat", "cat", false, true, true).unwrap(), vec![(0, 3)]);
        // 日本語は単語の区切りが無いので、前後が文字だと一致しない（Unicode の \b）
        assert_eq!(find_all_regex("猫 猫です", "猫", false, false, true).unwrap(), vec![(0, 3)]);
    }

    #[test]
    fn replacements_expand_capture_groups_only_in_regex_mode() {
        let t = "2026-11-09 and 1999-01-02";
        let r = regex_replacements(t, r"(\d+)-(\d+)-(\d+)", true, false, false, "$3/$2/$1").unwrap();
        assert_eq!(apply_replacements(t, &r), "09/11/2026 and 02/01/1999");
        let r = regex_replacements("a.b", "a.b", false, false, false, "$1").unwrap();
        assert_eq!(apply_replacements("a.b", &r), "$1"); // 文字どおり
        let r = regex_replacements("x", "x", true, false, false, "$$").unwrap();
        assert_eq!(apply_replacements("x", &r), "$");
        assert!(regex_replacements("x", "(", true, false, false, "").is_err());
        assert_eq!(apply_replacements("abc", &[]), "abc");
    }
}
