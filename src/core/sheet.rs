// csv / tsv → 罫線つきの表（GUI なし）。簡易対応: 読み取り専用の Preview だけで、編集は文字のまま行う。
// 区切りは tsv が タブ、csv は `,` を基本に先頭の数行から `;` / タブ / `|` も自動判定する。引用符（`"`）・`""` のエスケープ・引用符の中の改行・CRLF を読む。
// 1 行目は見出し行として出す。HelpView は大きな表が苦手なので、行数と列数に上限を置く。
use std::path::Path;

/// 表に出す最大の行数（見出しを含む）。超えた分は出さず、その旨を末尾に書く。
pub const MAX_ROWS: usize = 500;
/// 表に出す最大の列数。超えた分は切る。
pub const MAX_COLS: usize = 30;
/// 1 つのセルに出す最大の文字数。超えたら「…」で切る。
const MAX_CELL_CHARS: usize = 200;

/// 拡張子から区切りを決める。csv / tsv 以外は None。
pub fn delimiter_of(path: Option<&Path>) -> Option<char> {
    let ext = path?.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "csv" => Some(','),
        "tsv" | "tab" => Some('\t'),
        _ => None,
    }
}

/// 本文を見て区切りを決める。tsv / tab はタブ固定、csv は `,` を基本に、先頭の数行を読んで `;` / タブ / `|` の方が
/// 表として揃うときだけそちらにする（日本語環境の Excel が出す `;` 区切りの csv など）。csv 以外の拡張子は `,`。
pub fn delimiter_for(path: Option<&Path>, text: &str) -> char {
    match delimiter_of(path) {
        Some(',') => detect(text, ','),
        Some(d) => d,
        None => ',',
    }
}

/// 先頭 20 行を各候補で読み、「1 行目と同じ列数（2 列以上）の行が半数以上」で、点数（揃った行数 × 列数）が最も高い区切り。
/// 同点は `default` を優先する。
fn detect(text: &str, default: char) -> char {
    let score = |d: char| -> usize {
        let t = parse(text, d, 20);
        let n = t.rows.first().map_or(0, Vec::len);
        if n < 2 {
            return 0;
        }
        let same = t.rows.iter().filter(|r| r.len() == n).count();
        if same * 2 < t.rows.len() {
            return 0;
        }
        same * n
    };
    let mut best = (score(default), default);
    for d in [';', '\t', '|'] {
        let sc = score(d);
        if sc > best.0 {
            best = (sc, d);
        }
    }
    best.1
}

/// 数として読めるか（右寄せの判定用）。符号・末尾の `%`・小数点・千位区切り（`,` が区切りのときだけ `1,234`）を許す。
/// `;` 区切りのときは小数点の `,`（`1,5`）も数として扱う。
fn is_number(s: &str, semicolon: bool) -> bool {
    let s = s.trim();
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let s = s.strip_suffix('%').unwrap_or(s);
    let split = s.split_once('.').or_else(|| if semicolon { s.split_once(',') } else { None });
    let (int, frac) = match split {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    if let Some(f) = frac {
        if !digits(f) {
            return false;
        }
    }
    if int.is_empty() {
        return frac.is_some(); // ".5"
    }
    if digits(int) {
        return true;
    }
    if semicolon || !int.contains(',') {
        return false;
    }
    let mut parts = int.split(',');
    let first = parts.next().unwrap_or("");
    digits(first) && first.len() <= 3 && parts.all(|p| digits(p) && p.len() == 3)
}

/// 列ごとに「数の列」かを決める。見出し行を除く、空でないセルが 1 つ以上あり、その全てが数のとき。
fn numeric_columns(rows: &[Vec<String>], cols: usize, semicolon: bool) -> Vec<bool> {
    (0..cols)
        .map(|j| {
            let mut any = false;
            for r in rows.iter().skip(1) {
                match r.get(j).map(|c| c.trim()) {
                    Some(c) if !c.is_empty() => {
                        if !is_number(c, semicolon) {
                            return false;
                        }
                        any = true;
                    }
                    _ => {}
                }
            }
            any
        })
        .collect()
}

/// 読んだ結果。`rows` は最大 `limit` 行まで。`truncated` は、まだ先に行があるとき true。
#[derive(Debug, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

/// `text` を行と列に分ける。先頭の BOM は読み飛ばす。空行（全体が空の行）は飛ばす。
pub fn parse(text: &str, delim: char, limit: usize) -> Table {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false; // 引用符の中
    let mut was_quoted = false; // このセルは引用符で始まった（空でも「空のセル」として数える）
    let mut chars = text.chars().peekable();
    let mut truncated = false;

    // 行を閉じる。全体が空の行は捨てる
    fn end_row(rows: &mut Vec<Vec<String>>, row: &mut Vec<String>, cell: &mut String, was_quoted: &mut bool) {
        if !row.is_empty() || !cell.is_empty() || *was_quoted {
            row.push(std::mem::take(cell));
            rows.push(std::mem::take(row));
        }
        *was_quoted = false;
    }

    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut cell));
                was_quoted = false;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                end_row(&mut rows, &mut row, &mut cell, &mut was_quoted);
            }
            '\n' => end_row(&mut rows, &mut row, &mut cell, &mut was_quoted),
            c => cell.push(c),
        }
        if rows.len() >= limit {
            // 次の行が実際にあるかを確かめる（最後の改行だけなら切れていない）
            truncated = chars.any(|c| !matches!(c, '\r' | '\n'));
            break;
        }
    }
    if rows.len() < limit {
        end_row(&mut rows, &mut row, &mut cell, &mut was_quoted);
    }
    Table { rows, truncated }
}

fn esc_cell(s: &str, out: &mut String) {
    let mut n = 0;
    for c in s.chars() {
        if n >= MAX_CELL_CHARS {
            out.push('…');
            return;
        }
        n += 1;
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' | '\u{c}' => {}
            '\n' => out.push(' '), // セルの中の改行は 1 行に収める
            c => out.push(c),
        }
    }
}

/// 行の並びを罫線つきの表の HTML（`<table>` と、切った旨の注記）にする。1 行目は見出し。
/// `truncated` は行が上限で切れたとき、`cut_cols` は列が上限で切れたとき（`rows` の中の列が上限を超えていても切った扱いにする）。
/// `semicolon` は小数点の `,` を数として扱う（`;` 区切りの csv）。`full_hint` は注記に「全体は Text で見られます」を添える（csv / tsv だけ）。
pub fn table_html(rows: &[Vec<String>], truncated: bool, cut_cols: bool, semicolon: bool, sub: &str, full_hint: bool) -> String {
    if rows.is_empty() {
        return "<p>（空のファイルです）</p>".into();
    }
    let mut body = String::new();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).min(MAX_COLS);
    let cut_cols = cut_cols || rows.iter().any(|r| r.len() > MAX_COLS);
    let num = numeric_columns(rows, cols, semicolon);
    body.push_str("<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\">");
    for (i, r) in rows.iter().enumerate() {
        body.push_str("<tr>");
        let tag = if i == 0 { "th" } else { "td" };
        for j in 0..cols {
            // 数の列は右寄せ（HelpView の `align` 属性。見出しも揃える）
            let align = if num[j] { " align=\"right\"" } else { "" };
            body.push_str(&format!("<{tag}{align}>"));
            match r.get(j) {
                Some(c) if !c.is_empty() => esc_cell(c, &mut body),
                _ => body.push_str("&nbsp;"), // 空でも罫線が描かれるように
            }
            body.push_str(&format!("</{tag}>"));
        }
        body.push_str("</tr>");
    }
    body.push_str("</table>");
    let mut note = Vec::new();
    if truncated {
        note.push(format!("先頭の {} 行だけを表示しています", rows.len()));
    }
    if cut_cols {
        note.push(format!("{MAX_COLS} 列より右は表示していません"));
    }
    if !note.is_empty() {
        let tail = if full_hint { "全体は Text で見られます。" } else { "" };
        body.push_str(&format!("<p><font color=\"{sub}\">{}。{tail}</font></p>", note.join("。")));
    }
    body
}

/// 表を HelpView に渡す完全な HTML にする。色は `#rrggbb`。
pub fn to_html(text: &str, delim: char, bg: &str, fg: &str, sub: &str) -> String {
    let t = parse(text, delim, MAX_ROWS);
    let body = table_html(&t.rows, t.truncated, false, delim == ';', sub, true);
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, d: char) -> Vec<Vec<String>> {
        parse(text, d, 1000).rows
    }

    fn v(r: &[&[&str]]) -> Vec<Vec<String>> {
        r.iter().map(|x| x.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn delimiter_follows_the_extension() {
        assert_eq!(delimiter_of(Some(Path::new("a/x.CSV"))), Some(','));
        assert_eq!(delimiter_of(Some(Path::new("x.tsv"))), Some('\t'));
        assert_eq!(delimiter_of(Some(Path::new("x.txt"))), None);
        assert_eq!(delimiter_of(None), None);
    }

    #[test]
    fn plain_rows_with_lf_crlf_and_cr() {
        let want = v(&[&["a", "b"], &["1", "2"]]);
        assert_eq!(rows("a,b\n1,2\n", ','), want);
        assert_eq!(rows("a,b\r\n1,2\r\n", ','), want);
        assert_eq!(rows("a,b\r1,2", ','), want);
    }

    #[test]
    fn quotes_escapes_and_embedded_newlines() {
        assert_eq!(rows("\"a,b\",\"say \"\"hi\"\"\"\n\"x\ny\",z", ','), v(&[&["a,b", "say \"hi\""], &["x\ny", "z"]]));
    }

    #[test]
    fn empty_cells_blank_lines_and_bom() {
        assert_eq!(rows("a,,c\n\n,,\n\"\",x", ','), v(&[&["a", "", "c"], &["", "", ""], &["", "x"]]));
        assert_eq!(rows("\u{feff}a,b", ','), v(&[&["a", "b"]]));
        assert!(rows("", ',').is_empty());
        assert!(rows("\n\n", ',').is_empty());
    }

    #[test]
    fn tab_separated_and_unterminated_quote_do_not_panic() {
        assert_eq!(rows("a\tb,c\n1\t2", '\t'), v(&[&["a", "b,c"], &["1", "2"]]));
        assert_eq!(rows("a,\"b\nc", ','), v(&[&["a", "b\nc"]])); // 閉じていない引用符は、最後までを 1 つのセルにする
    }

    #[test]
    fn row_limit_marks_truncation_only_when_more_rows_follow() {
        let t = parse("1\n2\n3\n", ',', 2);
        assert_eq!((t.rows.len(), t.truncated), (2, true));
        let t = parse("1\n2\n", ',', 2);
        assert_eq!((t.rows.len(), t.truncated), (2, false));
        let t = parse("1\n2", ',', 2);
        assert_eq!((t.rows.len(), t.truncated), (2, false));
    }

    #[test]
    fn html_has_borders_header_and_escapes() {
        let h = to_html("名前,<b>\n&,", ',', "#000", "#fff", "#888");
        assert!(h.contains("border=\"1\""));
        assert!(h.contains("<th>名前</th><th>&lt;b&gt;</th>"));
        assert!(h.contains("<td>&amp;</td><td>&nbsp;</td>"));
        assert!(h.contains("bgcolor=\"#000\""));
        assert!(to_html("", ',', "#000", "#fff", "#888").contains("空のファイル"));
    }

    #[test]
    fn html_notes_when_rows_or_columns_are_cut() {
        let big: String = (0..MAX_ROWS + 5).map(|i| format!("{i}\n")).collect();
        assert!(to_html(&big, ',', "#000", "#fff", "#888").contains(&format!("先頭の {MAX_ROWS} 行")));
        let wide = (0..MAX_COLS + 3).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let h = to_html(&wide, ',', "#000", "#fff", "#888");
        assert!(h.contains("列より右は表示していません"));
        assert_eq!(h.matches("<th>").count(), MAX_COLS);
    }

    #[test]
    fn long_cells_are_cut() {
        let h = to_html(&"x".repeat(MAX_CELL_CHARS + 50), ',', "#000", "#fff", "#888");
        assert!(h.contains('…'));
    }

    #[test]
    fn delimiter_is_detected_for_csv_only() {
        let semi = "名前;点数\nA;1,5\nB;2,5\n";
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), semi), ';');
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), "a,b\n1,2\n"), ',');
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), "a\tb\n1\t2\n"), '\t');
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), "a|b|c\n1|2|3\n"), '|');
        // csv で `,` が使えているときは、本文の `;` に引きずられない
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), "a,b;c\n1,2;3\n"), ',');
        // 1 列だけ・空は既定のまま
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), "hello\nworld\n"), ',');
        assert_eq!(delimiter_for(Some(Path::new("x.csv")), ""), ',');
        // tsv はタブ固定、csv 以外は `,`
        assert_eq!(delimiter_for(Some(Path::new("x.tsv")), semi), '\t');
        assert_eq!(delimiter_for(Some(Path::new("x.txt")), semi), ',');
        assert_eq!(delimiter_for(None, semi), ',');
    }

    #[test]
    fn number_detection() {
        for ok in ["1", "-2", "+3.5", ".5", "12%", "1,234", "1,234,567.89", " 42 "] {
            assert!(is_number(ok, false), "{ok}");
        }
        for ng in ["", "-", "%", "a", "1a", "1.", "1,23", "12,345,67", "1,2", "1.2.3", "＋1"] {
            assert!(!is_number(ng, false), "{ng}");
        }
        assert!(is_number("1,5", true));
        assert!(is_number("-0,25%", true));
        assert!(!is_number("1,234,5", true));
    }

    #[test]
    fn numeric_columns_are_right_aligned_including_the_header() {
        let h = to_html("name,qty,note\nA,10,x\nB,2.5,\nC,,3", ',', "#000", "#fff", "#888");
        assert!(h.contains("<th>name</th><th align=\"right\">qty</th><th>note</th>"), "{h}");
        assert!(h.contains("<td align=\"right\">10</td>"));
        assert!(h.contains("<td align=\"right\">&nbsp;</td>")); // 空のセルも列に合わせる
        assert!(h.contains("<td>x</td>")); // 文字の列は左のまま
        // 1 つでも数でないセルがある列、数が 1 つも無い列は右寄せにしない
        let m = to_html("k,v\na,1\nb,x\nc,", ',', "#000", "#fff", "#888");
        assert!(!m.contains("align"));
        let hdr_only = to_html("a,b\n", ',', "#000", "#fff", "#888");
        assert!(!hdr_only.contains("align"));
    }

    #[test]
    fn semicolon_decimal_comma_is_numeric() {
        let h = to_html("n;v\nA;1,5\nB;22,25", ';', "#000", "#fff", "#888");
        assert!(h.contains("<td align=\"right\">1,5</td>"), "{h}");
    }
}
