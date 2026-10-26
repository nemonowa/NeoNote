// 指定行へ移動（Ctrl+G）。GUI 非依存。
// 入力は `12`（行）か `12:5`（行:桁）。`12,5` と `12 5` も受ける。行・桁とも 1 始まり。
// 桁は文字単位（バイトではない）。JSON error の `12:5` と同じ数え方なので、そのまま貼れる。

/// 入力を (行, 桁) にする。桁が無ければ None。0 や数字以外は None。
pub fn parse(input: &str) -> Option<(usize, Option<usize>)> {
    let mut it = input.split(|c: char| c == ':' || c == ',' || c.is_whitespace()).filter(|s| !s.is_empty());
    let line: usize = it.next()?.parse().ok().filter(|&n| n >= 1)?;
    let col = match it.next() {
        None => None,
        Some(s) => Some(s.parse::<usize>().ok().filter(|&n| n >= 1)?),
    };
    if it.next().is_some() {
        return None;
    }
    Some((line, col))
}

/// 行・桁に当たる、全文の中のバイト位置を返す（TextBuffer の位置と同じ単位）。
/// 行が多すぎれば最終行、桁が多すぎれば行末（CRLF の `\r` の手前）に丸める。
pub fn offset(text: &str, line: usize, col: Option<usize>) -> usize {
    let mut start = 0;
    let mut cur = 1;
    while cur < line {
        match text[start..].find('\n') {
            Some(i) => {
                start += i + 1;
                cur += 1;
            }
            None => break, // 最終行に丸める
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let body = text[start..end].strip_suffix('\r').unwrap_or(&text[start..end]);
    let want = col.unwrap_or(1).saturating_sub(1);
    let at = body.char_indices().nth(want).map_or(body.len(), |(i, _)| i);
    start + at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_line_and_line_col() {
        assert_eq!(parse("12"), Some((12, None)));
        assert_eq!(parse(" 12:5 "), Some((12, Some(5))));
        assert_eq!(parse("12,5"), Some((12, Some(5))));
        assert_eq!(parse("12 5"), Some((12, Some(5))));
    }

    #[test]
    fn parse_rejects_junk_zero_and_extra_parts() {
        for s in ["", "  ", "0", "0:1", "3:0", "a", "1:b", "1:2:3", "-1", "1.5"] {
            assert_eq!(parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn offset_finds_line_and_column() {
        let t = "ab\ncde\nf";
        assert_eq!(offset(t, 1, None), 0);
        assert_eq!(offset(t, 2, None), 3);
        assert_eq!(offset(t, 2, Some(3)), 5);
        assert_eq!(offset(t, 3, Some(1)), 7);
    }

    #[test]
    fn offset_clamps_line_and_column() {
        let t = "ab\ncde\nf";
        assert_eq!(offset(t, 99, None), 7); // 最終行の先頭
        assert_eq!(offset(t, 2, Some(99)), 6); // 行末
        assert_eq!(offset(t, 3, Some(99)), 8); // 最終行の末尾（改行なし）
        assert_eq!(offset("", 5, Some(5)), 0);
    }

    #[test]
    fn column_counts_chars_not_bytes() {
        let t = "あい😀う\nx";
        assert_eq!(offset(t, 1, Some(3)), 6); // あ(3)+い(3)
        assert_eq!(offset(t, 1, Some(4)), 10); // +😀(4)
        assert_eq!(offset(t, 2, Some(1)), t.len() - 1);
    }

    #[test]
    fn crlf_end_of_line_is_before_the_cr() {
        let t = "ab\r\ncd\r\n";
        assert_eq!(offset(t, 1, Some(99)), 2); // \r の手前
        assert_eq!(offset(t, 2, None), 4);
        assert_eq!(offset(t, 3, None), 8); // 末尾の空行
    }

    #[test]
    fn matches_json_line_col() {
        use crate::core::json::line_col;
        let t = "{\n  \"a\": あ,\n}";
        for pos in [0, 4, 9, 12] {
            if t.is_char_boundary(pos) {
                let (l, c) = line_col(t, pos);
                assert_eq!(offset(t, l, Some(c)), pos);
            }
        }
    }
}
