// 本文の中の「見るとわかる値」を見分ける（GUI なし）。マウスを乗せたときの小窓（ui/peekwin.rs）が使う。
// 見分けるもの: 色（#rgb・#rrggbb・rgb()・色の名前）、文字の参照（&amp; や &#x3042; と \u3042 などのエスケープ）、
// UNIX 時刻（10 桁の秒・13 桁のミリ秒）、画像ファイルのパス。
// 1 行の中で、指定の位置にかかっている最初の 1 つだけを返す。何も見つからなければ None。
use super::htmlstyle::{self, Rgb};
use std::ops::Range;

/// 長い行では、マウスの位置の前後この幅（バイト）だけを見る。
const WINDOW: usize = 256;
/// UNIX 時刻とみなす範囲（2001-09-09〜2100-01-01 の秒）。ただの大きな数を時刻と取り違えないため。
const TIME_MIN: i64 = 1_000_000_000;
const TIME_MAX: i64 = 4_102_444_800;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Peek {
    /// 色と、`#rrggbb` の表記。
    Color(Rgb, String),
    /// 文字参照・エスケープが表す文字と、その U+XXXX。
    Char(String, String),
    /// UNIX 時刻（秒）。ミリ秒で書かれていたら true。
    Time(i64, bool),
    /// 画像のパス（書かれたまま。実在するかは呼び出し側が確かめる）。
    Image(String),
}

/// `line` の `at`（バイト位置）にかかっている値。`names` が true なら色の名前（red など）も見る（CSS・HTML などのファイルだけ）。
pub fn at(line: &str, at: usize, names: bool) -> Option<(Range<usize>, Peek)> {
    if at >= line.len() {
        return None;
    }
    // 呼び出し元が文字の途中を指しても、その文字の先頭として扱う
    let mut at = at;
    while !line.is_char_boundary(at) {
        at -= 1;
    }
    // 長い行は前後だけを見る（文字の途中で切らない）
    let mut s = at.saturating_sub(WINDOW);
    while !line.is_char_boundary(s) {
        s -= 1;
    }
    let mut e = (at + WINDOW).min(line.len());
    while !line.is_char_boundary(e) {
        e += 1;
    }
    let part = &line[s..e];
    let rel = at - s;
    find(part, rel, names).map(|(r, p)| (r.start + s..r.end + s, p))
}

fn find(t: &str, at: usize, names: bool) -> Option<(Range<usize>, Peek)> {
    let b = t.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    // マウスのある語（英数字・_・-）の範囲
    let mut ws = at;
    while ws > 0 && word(b[ws - 1]) {
        ws -= 1;
    }
    let mut we = at;
    while we < b.len() && word(b[we]) {
        we += 1;
    }

    // #rgb / #rrggbb / #rrggbbaa
    let hs = if ws > 0 && b[ws - 1] == b'#' { Some(ws - 1) } else if b[at] == b'#' { Some(at) } else { None };
    if let Some(hs) = hs {
        let mut he = hs + 1;
        while he < b.len() && b[he].is_ascii_hexdigit() {
            he += 1;
        }
        let boundary = he >= b.len() || !word(b[he]);
        if boundary && matches!(he - hs - 1, 3 | 4 | 6 | 8) && (hs == 0 || !word(b[hs - 1])) {
            if let Some(c) = htmlstyle::parse_color(&t[hs..he]) {
                return Some((hs..he, Peek::Color(c, htmlstyle::hex(c))));
            }
        }
    }

    // rgb( ... ) / rgba( ... )
    if let Some(r) = func_around(t, at, &["rgb(", "rgba("]) {
        if let Some(c) = htmlstyle::parse_color(&t[r.clone()]) {
            return Some((r, Peek::Color(c, htmlstyle::hex(c))));
        }
    }

    // &name; / &#123; / &#x3042;
    if let Some(r) = entity_around(t, at) {
        let s = crate::neo::htmlview::decode_entities(&t[r.clone()]);
        if s != t[r.clone()] {
            if let Some(c) = s.chars().next() {
                return Some((r, Peek::Char(s.clone(), codepoint(c))));
            }
        }
    }

    // \uXXXX / \u{XXXX} / \UXXXXXXXX / \xXX
    if let Some((r, c)) = escape_around(t, at) {
        return Some((r, Peek::Char(c.to_string(), codepoint(c))));
    }

    if ws < we {
        let w = &t[ws..we];
        // UNIX 時刻（前後が数字・小数点でない 10 桁か 13 桁）
        if w.bytes().all(|c| c.is_ascii_digit()) && (ws == 0 || b[ws - 1] != b'.') && (we >= b.len() || b[we] != b'.') {
            if let Ok(n) = w.parse::<i64>() {
                let (secs, ms) = match w.len() {
                    10 => (n, false),
                    13 => (n / 1000, true),
                    _ => (0, false),
                };
                if (TIME_MIN..TIME_MAX).contains(&secs) {
                    return Some((ws..we, Peek::Time(secs, ms)));
                }
            }
        }
        // 色の名前（前が # や英数字でないときだけ。`--red` のような変数名は名前として扱わない）
        if names && w.bytes().all(|c| c.is_ascii_alphabetic()) && (ws == 0 || !matches!(b[ws - 1], b'#' | b'-' | b'.' | b'$' | b'@')) {
            if let Some(c) = htmlstyle::parse_color(w) {
                return Some((ws..we, Peek::Color(c, htmlstyle::hex(c))));
            }
        }
    }

    // 画像のパス（空白・引用符・括弧で区切られた語が、画像の拡張子で終わる）
    let stop = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '<' | '>' | '[' | ']' | '`' | ',' | ';' | '=');
    let ps = t[..at].rfind(stop).map_or(0, |p| p + t[p..].chars().next().map_or(1, char::len_utf8));
    let pe = t[at..].find(stop).map_or(t.len(), |p| at + p);
    if ps < pe {
        let p = &t[ps..pe];
        let low = p.to_ascii_lowercase();
        let is_img = [".png", ".jpg", ".jpeg", ".gif", ".bmp"].iter().any(|x| low.ends_with(x));
        if is_img && p.len() > 4 && !low.contains("://") && !low.starts_with("data:") {
            return Some((ps..pe, Peek::Image(p.to_string())));
        }
    }
    None
}

fn codepoint(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

/// `at` を含む `name(...)`（閉じ括弧まで）。
fn func_around(t: &str, at: usize, names: &[&str]) -> Option<Range<usize>> {
    let low = t.to_ascii_lowercase();
    for n in names {
        let mut from = 0;
        while let Some(p) = low[from..].find(n) {
            let s = from + p;
            let e = low[s..].find(')').map(|q| s + q + 1)?;
            if s <= at && at < e && e - s < 64 && (s == 0 || !low.as_bytes()[s - 1].is_ascii_alphanumeric()) {
                return Some(s..e);
            }
            from = s + n.len();
        }
    }
    None
}

/// `at` を含む `&...;`（中は英数字と #・x だけ、長さ 32 まで）。
fn entity_around(t: &str, at: usize) -> Option<Range<usize>> {
    let b = t.as_bytes();
    let s = b[..=at].iter().rposition(|&c| c == b'&')?;
    if at - s > 32 {
        return None;
    }
    let e = s + 1 + t[s + 1..].find(';')?;
    let inner = &b[s + 1..e];
    (at <= e && !inner.is_empty() && inner.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'#')).then_some(s..e + 1)
}

/// `at` を含む `\u3042`・`\u{1F600}`・`\U0001F600`・`\x41` と、その文字。
fn escape_around(t: &str, at: usize) -> Option<(Range<usize>, char)> {
    let b = t.as_bytes();
    let lo = at.saturating_sub(11);
    for s in (lo..=at).rev() {
        if b[s] != b'\\' || s + 1 >= b.len() {
            continue;
        }
        let (digits_at, len, braced) = match b[s + 1] {
            b'u' if b.get(s + 2) == Some(&b'{') => (s + 3, 0, true),
            b'u' => (s + 2, 4, false),
            b'U' => (s + 2, 8, false),
            b'x' => (s + 2, 2, false),
            _ => continue,
        };
        let (hex_end, end) = if braced {
            let close = s + 3 + t.get(s + 3..)?.find('}')?;
            (close, close + 1)
        } else {
            (digits_at + len, digits_at + len)
        };
        let hex = t.get(digits_at..hex_end)?;
        if hex.is_empty() || hex.len() > 8 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) || at >= end {
            return None;
        }
        let c = char::from_u32(u32::from_str_radix(hex, 16).ok()?)?;
        return (!c.is_control()).then_some((s..end, c));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peek(line: &str, needle: &str, names: bool) -> Option<Peek> {
        let at = line.find(needle).unwrap();
        at_(line, at, names)
    }
    fn at_(line: &str, i: usize, names: bool) -> Option<Peek> {
        at(line, i, names).map(|(_, p)| p)
    }

    #[test]
    fn colors() {
        assert_eq!(peek("color: #ff8800;", "ff8", false), Some(Peek::Color((255, 136, 0), "#ff8800".into())));
        assert_eq!(peek("a #FFF b", "#", false), Some(Peek::Color((255, 255, 255), "#ffffff".into())));
        assert_eq!(peek("bg: rgb(1, 2, 3) x", "2", false), Some(Peek::Color((1, 2, 3), "#010203".into())));
        assert_eq!(peek("p { color: navy }", "av", true), Some(Peek::Color((0, 0, 128), "#000080".into())));
        assert_eq!(peek("p { color: navy }", "av", false), None); // 名前は CSS などのときだけ
        // 色ではない #
        assert_eq!(peek("issue #12345", "123", false), None);
        assert_eq!(peek("#define X", "def", false), None);
        assert_eq!(peek("--red: 1", "red", true), None);
    }

    #[test]
    fn character_references_and_escapes() {
        assert_eq!(peek("a &hearts; b", "hea", false), Some(Peek::Char("♥".into(), "U+2665".into())));
        assert_eq!(peek("&#x3042;", "30", false), Some(Peek::Char("あ".into(), "U+3042".into())));
        assert_eq!(peek(r#""\u3042""#, "30", false), Some(Peek::Char("あ".into(), "U+3042".into())));
        assert_eq!(peek(r"\u{1F600}", "1F", false), Some(Peek::Char("😀".into(), "U+1F600".into())));
        assert_eq!(peek("a & b; c", "b", false), None);
        assert_eq!(peek(r"\u00", "00", false), None);
    }

    #[test]
    fn unix_times() {
        assert_eq!(peek("\"ts\": 1700000000,", "17", false), Some(Peek::Time(1_700_000_000, false)));
        assert_eq!(peek("t=1700000000123", "17", false), Some(Peek::Time(1_700_000_000, true)));
        assert_eq!(peek("n = 123456", "12", false), None);
        assert_eq!(peek("v 1.1700000000", "17", false), None);
        assert_eq!(peek("id 9999999999", "99", false), None); // 2286 年は範囲の外
    }

    #[test]
    fn image_paths() {
        assert_eq!(peek("![x](img/a b.png)", "img", false), None); // 空白で区切れるので「b.png」だけ
        assert_eq!(peek("![x](img/a.png)", "img", false), Some(Peek::Image("img/a.png".into())));
        assert_eq!(peek("src=\"C:\\pics\\日本.jpg\"", "pics", false), Some(Peek::Image("C:\\pics\\日本.jpg".into())));
        assert_eq!(peek("see https://x.test/a.png", "x.test", false), None);
        assert_eq!(peek("file.txt", "file", false), None);
    }

    #[test]
    fn positions_outside_and_long_lines() {
        assert_eq!(at_("#fff", 4, false), None);
        assert_eq!(at_("", 0, false), None);
        let long = format!("{}#00ff00 {}", "あ".repeat(400), "x".repeat(1000));
        let i = long.find("#00").unwrap() + 2;
        assert_eq!(at_(&long, i, false), Some(Peek::Color((0, 255, 0), "#00ff00".into())));
        // 文字の途中の位置でも落ちない
        let jp = "# 日本語の説明 &amp; 本文";
        for i in 0..jp.len() {
            at_(jp, i, true);
        }
    }
}
