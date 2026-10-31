// Tab / Shift+Tab のまとめインデントと、Enter のオートインデント。GUI 非依存。
// 位置はすべて UTF-8 のバイト位置（TextBuffer と同じ）。

/// 1 段のインデント。メモ帳と同じくタブ文字を入れる（ファイルの見た目を勝手に変えない）。
const UNIT: &str = "\t";
/// Shift+Tab で、行頭がタブでないときに外す空白の最大数。
const SPACES: usize = 4;

/// 本文の `start..end` を `text` に置き換え、そのあと `sel`（開始, 終了）を選ぶ。
/// 範囲は行全体なので、1 回の置き換え = 元に戻す 1 回。
#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub sel: (usize, usize),
}

impl Edit {
    /// 本文の一部（`by` バイト目から）を渡して計算したときに、本文全体の位置へ直す。
    pub fn shifted(mut self, by: usize) -> Edit {
        self.start += by;
        self.end += by;
        self.sel = (self.sel.0 + by, self.sel.1 + by);
        self
    }
}

/// 範囲外・文字の途中の位置を、手前の文字境界へ寄せる。
fn floor_boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `s..e` にかかる行のまとまり `(行頭, 最終行の行末)`。行末は改行（`\n`）の手前。
/// 選択が次の行の行頭ちょうどで終わっているときは、その行を含めない（多くのエディタと同じ）。
fn block_range(text: &str, s: usize, e: usize) -> (usize, usize) {
    let s = floor_boundary(text, s);
    let e = floor_boundary(text, e.max(s));
    let ls = text[..s].rfind('\n').map_or(0, |i| i + 1);
    let last = if e > s && text.as_bytes()[e - 1] == b'\n' { e - 1 } else { e };
    let le = text[last..].find('\n').map_or(text.len(), |i| last + i);
    (ls, le)
}

/// 空行（CRLF の `\r` だけの行も含む）。インデントを足さない・外さない。
fn is_blank(line: &str) -> bool {
    line.trim_end_matches('\r').is_empty()
}

/// 行頭から外す長さ（タブ 1 つ、または空白を最大 `SPACES` 個）。
fn leading_removal(line: &str) -> usize {
    if line.starts_with('\t') {
        1
    } else {
        line.bytes().take(SPACES).take_while(|&b| b == b' ').count()
    }
}

/// 選択にかかる行の先頭にインデントを足す。空行は飛ばす。足す行が無ければ None。
pub fn indent(text: &str, s: usize, e: usize) -> Option<Edit> {
    let (ls, le) = block_range(text, s, e);
    let mut changed = 0;
    let new: Vec<String> = text[ls..le]
        .split('\n')
        .map(|l| {
            if is_blank(l) {
                l.to_string()
            } else {
                changed += 1;
                format!("{UNIT}{l}")
            }
        })
        .collect();
    if changed == 0 {
        return None;
    }
    let new = new.join("\n");
    let sel = if s == e { (s + UNIT.len(), e + UNIT.len()) } else { (ls, ls + new.len()) };
    Some(Edit { start: ls, end: le, text: new, sel })
}

/// 選択にかかる行の先頭から、インデントを 1 段外す。外す行が無ければ None。
/// 選択が無いとき（カーソルだけ）は、その行を外してカーソルを同じ文字の上に保つ。
pub fn unindent(text: &str, s: usize, e: usize) -> Option<Edit> {
    let (ls, le) = block_range(text, s, e);
    let mut removed_total = 0;
    let mut first_removed = 0;
    let new: Vec<&str> = text[ls..le]
        .split('\n')
        .enumerate()
        .map(|(i, l)| {
            let n = leading_removal(l);
            if i == 0 {
                first_removed = n;
            }
            removed_total += n;
            &l[n..]
        })
        .collect();
    if removed_total == 0 {
        return None;
    }
    let new = new.join("\n");
    let sel = if s == e {
        let p = (s - first_removed.min(s - ls)).max(ls);
        (p, p)
    } else {
        (ls, ls + new.len())
    };
    Some(Edit { start: ls, end: le, text: new, sel })
}

/// Enter で `pos` に入れる文字列（改行 + 引き継ぐインデント）。戻り値は `(pos の手前で一緒に置き換える長さ, 入れる文字列)`。
/// インデントは、その行の先頭から `pos` までの空白とタブ。改行コードは、今の行（最終行なら前の行）に合わせ、
/// 手がかりが無いとき（1 行だけの文書）は `default_crlf`。
/// CRLF の行末では、End キーなどでカーソルが `\r` と `\n` の間に来る。そのまま `\r\n` を入れると `\r\r\n` に
/// なるので、挿入位置を `\r` の手前へ戻す（戻り値の最初が 1。選択が無ければ元の `\r\n` は残る）。
pub fn newline_with_indent(text: &str, pos: usize, default_crlf: bool) -> (usize, String) {
    let pos = floor_boundary(text, pos);
    let ls = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[ls..pos].chars().take_while(|&c| c == ' ' || c == '\t').collect();
    let crlf = match text[pos..].find('\n') {
        Some(i) => (pos + i) > 0 && text.as_bytes()[pos + i - 1] == b'\r',
        None if ls >= 1 => ls >= 2 && &text.as_bytes()[ls - 2..ls] == b"\r\n",
        None => default_crlf, // 改行がどこにも見えない（空の文書など）
    };
    let between = pos > 0 && text.as_bytes()[pos - 1] == b'\r' && text.as_bytes().get(pos) == Some(&b'\n');
    (usize::from(between), format!("{}{indent}", if crlf { "\r\n" } else { "\n" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, ed: &Edit) -> String {
        format!("{}{}{}", &text[..ed.start], ed.text, &text[ed.end..])
    }

    #[test]
    fn indent_multiple_lines_and_skip_blank() {
        let t = "a\n\nb\nc";
        let ed = indent(t, 0, t.len()).unwrap();
        assert_eq!(apply(t, &ed), "\ta\n\n\tb\n\tc");
        assert_eq!(ed.sel, (0, ed.text.len()));
    }

    #[test]
    fn selection_ending_at_next_line_start_excludes_that_line() {
        let t = "a\nb\nc";
        // "a\nb\n" を選択（c の行頭で終わる）
        let ed = indent(t, 0, 4).unwrap();
        assert_eq!(apply(t, &ed), "\ta\n\tb\nc");
    }

    #[test]
    fn indent_keeps_crlf_and_skips_cr_only_lines() {
        let t = "a\r\n\r\nb\r\n";
        let ed = indent(t, 0, t.len() - 1).unwrap();
        assert_eq!(apply(t, &ed), "\ta\r\n\r\n\tb\r\n");
    }

    #[test]
    fn indent_only_blank_lines_is_none() {
        assert_eq!(indent("\n\n", 0, 2), None);
    }

    #[test]
    fn indent_works_on_multibyte_lines() {
        let t = "あ\nい";
        let ed = indent(t, 0, t.len()).unwrap();
        assert_eq!(apply(t, &ed), "\tあ\n\tい");
    }

    #[test]
    fn unindent_tab_and_up_to_four_spaces() {
        let t = "\ta\n      b\n  c\nd";
        let ed = unindent(t, 0, t.len()).unwrap();
        assert_eq!(apply(t, &ed), "a\n  b\nc\nd");
    }

    #[test]
    fn unindent_nothing_to_remove_is_none() {
        assert_eq!(unindent("a\nb", 0, 3), None);
    }

    #[test]
    fn unindent_caret_only_moves_caret_with_text() {
        let t = "x\n\tabc";
        // カーソルは abc の b の前（行頭 2 + タブ 1 + a 1 = 4）
        let ed = unindent(t, 4, 4).unwrap();
        assert_eq!(apply(t, &ed), "x\nabc");
        assert_eq!(ed.sel, (3, 3));
    }

    #[test]
    fn unindent_caret_inside_indent_stops_at_line_start() {
        let t = "    a";
        let ed = unindent(t, 1, 1).unwrap();
        assert_eq!(apply(t, &ed), "a");
        assert_eq!(ed.sel, (0, 0));
    }

    fn nl(text: &str, pos: usize) -> String {
        newline_with_indent(text, pos, false).1
    }

    #[test]
    fn newline_copies_indent_up_to_caret() {
        let t = "\t  abc";
        assert_eq!(nl(t, t.len()), "\n\t  ");
        // 字下げの途中で Enter: カーソルより前の分だけ引き継ぐ
        assert_eq!(nl(t, 2), "\n\t ");
        // 行頭で Enter: 引き継ぐものなし
        assert_eq!(nl(t, 0), "\n");
    }

    #[test]
    fn newline_follows_line_ending_of_the_line() {
        assert_eq!(nl("  a\r\nb", 3), "\r\n  ");
        assert_eq!(nl("a\nb", 1), "\n");
        // 最終行（終端なし）は前の行に合わせる
        assert_eq!(nl("a\r\n  b", 6), "\r\n  ");
        assert_eq!(nl("a\n  b", 5), "\n  ");
        assert_eq!(nl("  b", 3), "\n  ");
        assert_eq!(nl("", 0), "\n");
        // 手がかりが無いときだけ既定に従う
        assert_eq!(newline_with_indent("ab", 2, true).1, "\r\n");
        assert_eq!(newline_with_indent("a\nb", 3, true).1, "\n");
    }

    #[test]
    fn newline_between_cr_and_lf_replaces_the_cr() {
        // "  ab\r|\n" にカーソル（End キーで来る位置）
        let t = "  ab\r\ncd";
        assert_eq!(newline_with_indent(t, 5, false), (1, "\r\n  ".to_string()));
        // \r の手前なら普通に挿入
        assert_eq!(newline_with_indent(t, 4, false), (0, "\r\n  ".to_string()));
        // LF のファイルでは起きない
        assert_eq!(newline_with_indent("ab\ncd", 2, false), (0, "\n".to_string()));
    }

    #[test]
    fn newline_in_multibyte_text() {
        let t = "\tあいう";
        assert_eq!(nl(t, t.len()), "\n\t");
    }
}
