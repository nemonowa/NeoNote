// 選択範囲の計算。位置は UTF-8 のバイトオフセット（fltk の TextBuffer と同じ単位）。
// GUI に依存しないので単体でテストできる。

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn clamp_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn extend_left(text: &str, mut i: usize) -> usize {
    while let Some(c) = text[..i].chars().next_back() {
        if !is_word(c) {
            break;
        }
        i -= c.len_utf8();
    }
    i
}

fn extend_right(text: &str, mut i: usize) -> usize {
    while let Some(c) = text[i..].chars().next() {
        if !is_word(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// `pos` にある単語（英数字・`_`・かな・漢字の連続）。右側が単語でなければ左側を見る。
pub fn word_at(text: &str, pos: usize) -> Option<(usize, usize)> {
    let pos = clamp_boundary(text, pos);
    let right = text[pos..].chars().next().is_some_and(is_word);
    let left = text[..pos].chars().next_back().is_some_and(is_word);
    (right || left).then(|| (extend_left(text, pos), extend_right(text, pos)))
}

/// 範囲 [start, end] を含む行（改行は含めない）。
fn line_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let ls = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let mut le = text[end..].find('\n').map_or(text.len(), |i| end + i);
    if le > ls && text.as_bytes()[le - 1] == b'\r' {
        le -= 1;
    }
    (ls, le)
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// 行 [ls, le] を含む段落（空行で区切られた範囲）。
fn paragraph_range(text: &str, mut ls: usize, mut le: usize) -> (usize, usize) {
    while ls > 0 {
        let prev_end = ls - 1; // 直前の '\n'
        let prev_start = text[..prev_end].rfind('\n').map_or(0, |i| i + 1);
        if is_blank(&text[prev_start..prev_end]) {
            break;
        }
        ls = prev_start;
    }
    loop {
        let Some(nl) = text[le..].find('\n').map(|i| le + i) else { break };
        let next_start = nl + 1;
        if next_start >= text.len() {
            break;
        }
        let next_end = text[next_start..].find('\n').map_or(text.len(), |i| next_start + i);
        if is_blank(&text[next_start..next_end]) {
            break;
        }
        le = if next_end > next_start && text.as_bytes()[next_end - 1] == b'\r' { next_end - 1 } else { next_end };
    }
    (ls, le)
}

/// 今の選択を、単語 → 行 → 段落 → 全体の順に 1 段だけ広げる。
/// 選択が無ければ（start == end）カーソル位置の単語から始める。これ以上広げられなければそのまま返す。
pub fn expand(text: &str, start: usize, end: usize) -> (usize, usize) {
    let start = clamp_boundary(text, start.min(end));
    let end = clamp_boundary(text, end.max(start));
    let word = if start == end {
        word_at(text, start).unwrap_or((start, end))
    } else {
        let ws = if text[start..].chars().next().is_some_and(is_word) { extend_left(text, start) } else { start };
        let we = if text[..end].chars().next_back().is_some_and(is_word) { extend_right(text, end) } else { end };
        (ws, we)
    };
    let line = line_range(text, start, end);
    let para = paragraph_range(text, line.0, line.1);
    let all = (0, text.len());
    [word, line, para, all]
        .into_iter()
        .find(|&(s, e)| s <= start && end <= e && (s, e) != (start, end))
        .unwrap_or((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "foo bar_1 baz\nqux 日本語です\n\nnext para\nsecond line\n";

    fn grow(mut r: (usize, usize)) -> Vec<&'static str> {
        // 各段階で選ばれた文字列を集める
        let mut out = vec![];
        for _ in 0..6 {
            let n = expand(T, r.0, r.1);
            if n == r {
                break;
            }
            r = n;
            out.push(Box::leak(T[r.0..r.1].to_string().into_boxed_str()) as &str);
        }
        out
    }

    #[test]
    fn grows_word_line_paragraph_all() {
        let steps = grow((5, 5)); // "bar_1" の中
        assert_eq!(steps, vec!["bar_1", "foo bar_1 baz", "foo bar_1 baz\nqux 日本語です", T]);
    }

    #[test]
    fn japanese_run_is_one_word_and_cursor_after_word_counts() {
        let p = T.find("日本語").unwrap();
        assert_eq!(&T[expand(T, p, p).0..expand(T, p, p).1], "日本語です");
        let after_foo = 3; // "foo" の直後
        assert_eq!(word_at(T, after_foo), Some((0, 3)));
        assert_eq!(word_at(T, 3 + 0).map(|(s, e)| &T[s..e]), Some("foo"));
    }

    #[test]
    fn partial_selection_snaps_to_word_but_whitespace_edges_do_not() {
        // "oo b" を選ぶ → 先頭は foo に、末尾は bar_1 に広がる
        let r = expand(T, 1, 5);
        assert_eq!(&T[r.0..r.1], "foo bar_1");
        // " bar" の前の空白から選んでも、左隣の foo を巻き込まない
        let r = expand(T, 3, 7);
        assert_eq!(&T[r.0..r.1], " bar_1");
    }

    #[test]
    fn blank_line_and_edges_do_not_panic() {
        assert_eq!(expand("", 0, 0), (0, 0));
        let t = "a\n\nb";
        assert_eq!(expand(t, 2, 2), (0, 4)); // 空行: 単語・行・段落が広がらず全体へ
        assert_eq!(expand("あい", 1, 1), (0, 6)); // 文字境界の途中でも壊れない
        assert_eq!(expand(T, T.len(), T.len()).1, T.len());
    }

    #[test]
    fn crlf_lines_do_not_include_the_carriage_return() {
        let t = "ab cd\r\nef\r\n";
        let r = expand(t, 0, 2); // "ab" → 行
        assert_eq!(&t[r.0..r.1], "ab cd");
    }
}
