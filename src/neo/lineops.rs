// 行の一括操作（GUI なし）: 並べ替え・重複の削除・大文字/小文字。
// 対象は「選択範囲がかかっている行の全体」（選択が無ければ全文）。改行の形（CRLF / LF）と、最後の改行の有無は保つ。
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// 自然順（`f2` の次が `f10`。大文字小文字は区別しない）で並べる。
    Sort,
    /// 逆順に並べる（自然順の逆）。
    SortDesc,
    /// 同じ行の 2 つ目以降を消す（最初の位置を残す）。
    Unique,
    Upper,
    Lower,
}

/// `text` の `sel`（バイト範囲）がかかっている行の範囲。選択が空なら全文。末尾の改行は範囲に含めない。
pub fn line_span(text: &str, sel: Range<usize>) -> Range<usize> {
    if sel.start == sel.end {
        let end = text.trim_end_matches(['\n', '\r']).len();
        return 0..end;
    }
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    // 選択が行頭で終わる（次の行の先頭まで選んだ）ときは、その行を含めない
    let mut last = sel.end;
    if last > sel.start && text[..last].ends_with('\n') {
        last -= 1;
    }
    let end = text[last..].find('\n').map_or(text.len(), |i| last + i);
    let end = if text[..end].ends_with('\r') { end - 1 } else { end };
    start..end.max(start)
}

/// `part`（行の並び。末尾の改行なし）に操作を当てた結果。変わらなければ None。
pub fn apply(part: &str, op: Op) -> Option<String> {
    let eol = if part.contains("\r\n") { "\r\n" } else { "\n" };
    let out = match op {
        Op::Upper => part.to_uppercase(),
        Op::Lower => part.to_lowercase(),
        _ => {
            let mut lines: Vec<&str> = part.split(eol).collect();
            match op {
                Op::Sort => lines.sort_by(|a, b| natural_cmp(a, b)),
                Op::SortDesc => lines.sort_by(|a, b| natural_cmp(b, a)),
                Op::Unique => {
                    let mut seen = std::collections::HashSet::new();
                    lines.retain(|l| seen.insert(*l));
                }
                _ => {}
            }
            lines.join(eol)
        }
    };
    (out != part).then_some(out)
}

/// 自然順の比較（数字の並びは数として比べる。それ以外は大文字小文字を無視し、同じなら元の文字で比べる）。
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(&c) = it.peek().filter(|c| c.is_ascii_digit()) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (nt, mt) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = nt.len().cmp(&mt.len()).then_with(|| nt.cmp(mt));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_naturally_and_keeps_crlf() {
        assert_eq!(apply("f10\r\nb\r\nf2\r\nA", Op::Sort).as_deref(), Some("A\r\nb\r\nf2\r\nf10"));
        assert_eq!(apply("a\nc\nb", Op::SortDesc).as_deref(), Some("c\nb\na"));
        assert_eq!(apply("a\nb", Op::Sort), None);
    }

    #[test]
    fn unique_keeps_first_and_case_ops_work() {
        assert_eq!(apply("x\ny\nx\n\ny\n", Op::Unique).as_deref(), Some("x\ny\n"));
        assert_eq!(apply("Ab ä", Op::Upper).as_deref(), Some("AB Ä"));
        assert_eq!(apply("Ab", Op::Lower).as_deref(), Some("ab"));
    }

    #[test]
    fn span_covers_whole_lines_of_the_selection() {
        let t = "one\ntwo\nthree\nfour\n";
        assert_eq!(&t[line_span(t, 5..9)], "two\nthree"); // "wo\nth"
        assert_eq!(&t[line_span(t, 4..8)], "two"); // 次の行の先頭まで選んでも、その行は含めない
        assert_eq!(&t[line_span(t, 0..0)], "one\ntwo\nthree\nfour"); // 選択なしは全文（最後の改行を除く）
        let c = "a\r\nb\r\nc";
        assert_eq!(&c[line_span(c, 0..4)], "a\r\nb");
    }
}
