// 本文の中の「見るとわかる値」を見分ける（GUI なし）。マウスを乗せたときの小窓（ui/peekwin.rs）が使う。
// 見分けるもの: 色（#rgb・#rrggbb・rgb()・色の名前）、文字の参照（&amp; や &#x3042; と \u3042 などのエスケープ）、
// UNIX 時刻（10 桁の秒・13 桁のミリ秒）、日時（UNIX 時刻へ）、数（0x・0b・0o の付いた数）、Minecraft の色コード（§a・&a）、画像ファイルのパス。
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
    /// URL エンコード（`%XX` の並び）を戻した文字。
    Url(String),
    /// UNIX 時刻（秒）。ミリ秒で書かれていたら true。
    Time(i64, bool),
    /// `YYYY-MM-DD HH:MM[:SS]` の日時。値は、書かれた日時を UTC とみなしたときの UNIX 時刻（時間帯は呼び出し側で直す）。
    Date(i64),
    /// 数（`0xFF`・`0b1010`・`0o17`）。
    Number(u64),
    /// Minecraft の書式コード（`§a`・`&a` の `a`。小文字にしたもの）。
    McCode(char),
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

    // §a / &a（Minecraft の書式コード）。& は、後ろに小文字が続くとき（&and など）は除く
    for s in at.saturating_sub(2)..=at {
        if !t.is_char_boundary(s) {
            continue;
        }
        let rest = &t[s..];
        let (lead, n) = if rest.starts_with('§') { ('§', '§'.len_utf8()) } else if rest.starts_with('&') { ('&', 1) } else { continue };
        let Some(code) = rest[n..].chars().next().map(|c| c.to_ascii_lowercase()) else { continue };
        if !(code.is_ascii_digit() || ('a'..='f').contains(&code) || ('k'..='o').contains(&code) || code == 'r') {
            continue;
        }
        let end = s + n + 1;
        if at >= end || (lead == '&' && t[end..].starts_with(|c: char| c.is_ascii_lowercase())) {
            continue;
        }
        return Some((s..end, Peek::McCode(code)));
    }

    // YYYY-MM-DD HH:MM[:SS]
    if let Some((r, secs)) = date_around(t, at) {
        return Some((r, Peek::Date(secs)));
    }

    // rgb( ... ) / rgba( ... )
    if let Some(r) = func_around(t, at, &["rgb(", "rgba("]) {
        if let Some(c) = htmlstyle::parse_color(&t[r.clone()]) {
            return Some((r, Peek::Color(c, htmlstyle::hex(c))));
        }
    }

    // hsv( ... )
    if let Some(r) = func_around(t, at, &["hsv("]) {
        if let Some(c) = super::textconv::parse_hsv(&t[r.clone()]) {
            return Some((r, Peek::Color(c, htmlstyle::hex(c))));
        }
    }

    // %E3%81%82 のような URL エンコードの並び（戻すと ASCII 以外の文字になるときだけ。%20 だけなどは出さない）
    if let Some(r) = percent_around(t, at) {
        let s = super::textconv::url_decode(&t[r.clone()]);
        if !s.is_ascii() && !s.contains('%') {
            return Some((r, Peek::Url(s)));
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
        // 数（0x・0b・0o の付いた数。ただの 10 進は、どこにでもあって小窓がうるさいので出さない）
        if (ws == 0 || b[ws - 1] != b'.') && (we >= b.len() || b[we] != b'.') {
            let low = w.to_ascii_lowercase();
            let n = if let Some(h) = low.strip_prefix("0x") {
                u64::from_str_radix(h, 16).ok()
            } else if let Some(h) = low.strip_prefix("0b") {
                u64::from_str_radix(h, 2).ok()
            } else if let Some(h) = low.strip_prefix("0o") {
                u64::from_str_radix(h, 8).ok()
            } else {
                None
            };
            if let Some(n) = n {
                return Some((ws..we, Peek::Number(n)));
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

/// `at` を含む `YYYY-MM-DD HH:MM[:SS]`（`/` 区切り・`T` も読む）と、それを UTC とみなした UNIX 時刻。
fn date_around(t: &str, at: usize) -> Option<(Range<usize>, i64)> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})[ T](\d{1,2}):(\d{2})(?::(\d{2}))?\b").expect("日時の正規表現"));
    let lo = at.saturating_sub(24);
    let mut lo = lo;
    while !t.is_char_boundary(lo) {
        lo -= 1;
    }
    for c in re.captures_iter(&t[lo..]) {
        let m = c.get(0)?;
        let (s, e) = (lo + m.start(), lo + m.end());
        if !(s <= at && at < e) {
            continue;
        }
        let n = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<i64>().ok());
        let (y, mo, d, h, mi, sec) = (n(1)?, n(2)?, n(3)?, n(4)?, n(5)?, n(6)?);
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
            return None;
        }
        let days = super::textconv::days_from_civil(y, mo as u32, d as u32);
        return Some((s..e, days * 86_400 + h * 3600 + mi * 60 + sec));
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

/// `at` が乗っている、続いた `%XX` の並び。
fn percent_around(t: &str, at: usize) -> Option<Range<usize>> {
    let b = t.as_bytes();
    let is_pct = |p: usize| p + 2 < b.len() && b[p] == b'%' && b[p + 1].is_ascii_hexdigit() && b[p + 2].is_ascii_hexdigit();
    let mut s = (at.saturating_sub(2)..=at).find(|&p| is_pct(p))?;
    while s >= 3 && is_pct(s - 3) {
        s -= 3;
    }
    let mut e = s;
    while is_pct(e) {
        e += 3;
    }
    Some(s..e)
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
    fn numbers_dates_and_minecraft_codes() {
        assert_eq!(at("x = 0xFF;", 5, false).map(|x| x.1), Some(Peek::Number(255)));
        assert_eq!(at("0b1010", 2, false).map(|x| x.1), Some(Peek::Number(10)));
        assert_eq!(at("count 255 items", 7, false), None, "ただの 10 進は出さない");
        assert_eq!(at("at 2026-11-19 09:30 done", 8, false), Some((3..19, Peek::Date(1_795_046_400 + 9 * 3600 + 30 * 60))));
        assert_eq!(at("2026/1/2T03:04:05", 3, false).map(|x| x.1), Some(Peek::Date(1_767_312_000 + 3 * 3600 + 4 * 60 + 5)));
        assert_eq!(at("§aHello", 0, false), Some((0..3, Peek::McCode('a'))));
        assert_eq!(at("&lBold", 1, false), Some((0..2, Peek::McCode('l'))));
        assert_eq!(at("&and", 1, false), None, "& の後に小文字が続くものは除く");
    }

    #[test]
    fn hsv_and_percent_encoding_are_recognized() {
        assert_eq!(at("c: hsv(0, 100%, 100%);", 5, false).map(|x| x.1), Some(Peek::Color((255, 0, 0), "#ff0000".into())));
        let t = "q=%E3%81%82%E3%81%84&x";
        assert_eq!(at(t, 6, false), Some((2..20, Peek::Url("あい".into()))));
        assert_eq!(at("a%20b", 2, false), None, "ASCII だけになる並びは出さない");
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
