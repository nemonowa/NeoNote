// 文字の変換（GUI なし）: Unicode エスケープ・URL エンコード・色の表記（#16 進・rgb()・hsv()）・Base64・HTML の文字参照・
// 全角/半角・ひらがな/カタカナ・名前の書き方（snake_case など）。
// 行の一括操作（lineops）から、選んだ範囲（無ければ全文）に当てる。小窓（peek）の色の表記もここで作る。
use super::htmlstyle::Rgb;
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// ASCII 以外の文字を `\uXXXX` にする（U+10000 以上はサロゲートペアの 2 つ。Java・JSON・JavaScript と同じ形）。
pub fn unicode_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}

/// `\uXXXX`（サロゲートペアは 1 文字に）・`\u{XXXX}`・`\UXXXXXXXX` を文字に戻す。読めない並びはそのまま残す。
pub fn unicode_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let hex_at = |from: usize, len: usize| -> Option<u32> {
        let h = s.get(from..from + len)?;
        (h.bytes().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(h, 16).ok()).flatten()
    };
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'u' if b.get(i + 2) == Some(&b'{') => {
                    if let Some(close) = s[i + 3..].find('}').map(|p| i + 3 + p) {
                        let h = &s[i + 3..close];
                        if (1..=6).contains(&h.len()) {
                            if let Some(c) = u32::from_str_radix(h, 16).ok().and_then(char::from_u32) {
                                out.push(c);
                                i = close + 1;
                                continue;
                            }
                        }
                    }
                }
                b'u' => {
                    if let Some(u) = hex_at(i + 2, 4) {
                        // 上位サロゲートの後に下位サロゲートが続けば、1 文字にする
                        if (0xD800..0xDC00).contains(&u) && b.get(i + 6) == Some(&b'\\') && b.get(i + 7) == Some(&b'u') {
                            if let Some(lo) = hex_at(i + 8, 4).filter(|lo| (0xDC00..0xE000).contains(lo)) {
                                if let Some(c) = char::from_u32(0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00)) {
                                    out.push(c);
                                    i += 12;
                                    continue;
                                }
                            }
                        }
                        if let Some(c) = char::from_u32(u) {
                            out.push(c);
                            i += 6;
                            continue;
                        }
                    }
                }
                b'U' => {
                    if let Some(c) = hex_at(i + 2, 8).and_then(char::from_u32) {
                        out.push(c);
                        i += 10;
                        continue;
                    }
                }
                _ => {}
            }
        }
        let c = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(c);
        i += c.len_utf8().max(1);
    }
    out
}

/// URL として使える文字（英数字・`-_.~` と、URL の区切りの記号）を残し、ほかを UTF-8 の `%XX` にする（JavaScript の encodeURI と同じ範囲）。
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"-_.~!*'();:@&=+$,/?#[]".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// `%XX` の並びを UTF-8 として戻す。UTF-8 として読めない並び・`%` の後が 16 進でないものは、そのまま残す。`+` は空白にしない。
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let byte_at = |p: usize| -> Option<u8> { s.get(p + 1..p + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit())).and_then(|h| u8::from_str_radix(h, 16).ok()) };
    while i < b.len() {
        if b[i] == b'%' {
            // 続いている %XX を集めて、読める所まで UTF-8 として戻す
            let mut bytes = Vec::new();
            let mut j = i;
            while j < b.len() && b[j] == b'%' {
                match byte_at(j) {
                    Some(v) => {
                        bytes.push(v);
                        j += 3;
                    }
                    None => break,
                }
            }
            if bytes.is_empty() {
                out.push('%');
                i += 1;
                continue;
            }
            let mut k = 0;
            while k < bytes.len() {
                match std::str::from_utf8(&bytes[k..]) {
                    Ok(t) => {
                        out.push_str(t);
                        k = bytes.len();
                    }
                    Err(e) => {
                        let good = e.valid_up_to();
                        out.push_str(std::str::from_utf8(&bytes[k..k + good]).unwrap_or_default());
                        k += good;
                        // 読めない 1 バイトは、元の %XX のまま残す
                        let bad = e.error_len().unwrap_or(bytes.len() - k).max(1);
                        for v in &bytes[k..k + bad] {
                            out.push_str(&format!("%{v:02X}"));
                        }
                        k += bad;
                    }
                }
            }
            i = j;
            continue;
        }
        let c = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(c);
        i += c.len_utf8().max(1);
    }
    out
}

/// RGB → HSV（色相 0〜360 度、彩度・明度 0〜100%）。
pub fn rgb_to_hsv((r, g, b): Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s * 100.0, max * 100.0)
}

/// HSV（色相 度、彩度・明度 %）→ RGB。範囲の外は丸める。
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> Rgb {
    let h = h.rem_euclid(360.0);
    let (s, v) = ((s / 100.0).clamp(0.0, 1.0), (v / 100.0).clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

pub fn rgb_text((r, g, b): Rgb) -> String {
    format!("rgb({r}, {g}, {b})")
}

pub fn hsv_text(c: Rgb) -> String {
    let (h, s, v) = rgb_to_hsv(c);
    format!("hsv({}, {}%, {}%)", h.round() as i64 % 360, s.round() as i64, v.round() as i64)
}

/// 小窓に出す、色の 3 つの表記。
pub fn color_formats(c: Rgb) -> String {
    format!("{}  {}  {}", super::htmlstyle::hex(c), rgb_text(c), hsv_text(c))
}

fn color_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)#([0-9a-f]{6}|[0-9a-f]{3})\b|\brgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)|\bhsv\(\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)%?\s*,\s*(\d+(?:\.\d+)?)%?\s*\)")
            .expect("色の正規表現")
    })
}

/// `hsv(...)` を色として読む（小窓用）。
pub fn parse_hsv(s: &str) -> Option<Rgb> {
    let c = color_re().captures(s)?;
    if c.get(0)?.as_str().len() != s.len() || c.get(5).is_none() {
        return None;
    }
    let f = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<f64>().ok());
    Some(hsv_to_rgb(f(5)?, f(6)?, f(7)?))
}

/// 色の表記を 1 つ先へ進める: `#16 進` → `rgb()` → `hsv()` → `#16 進`。透明度つきの表記は変えない。
pub fn color_cycle(s: &str) -> String {
    color_re()
        .replace_all(s, |c: &Captures| {
            let all = c.get(0).map_or("", |m| m.as_str());
            if let Some(h) = c.get(1) {
                let h = h.as_str();
                let full: String = if h.len() == 3 { h.chars().flat_map(|x| [x, x]).collect() } else { h.to_string() };
                let v = u32::from_str_radix(&full, 16).unwrap_or(0);
                return rgb_text(((v >> 16) as u8, (v >> 8) as u8, v as u8));
            }
            if c.get(2).is_some() {
                let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u16>().ok()).filter(|v| *v <= 255);
                return match (n(2), n(3), n(4)) {
                    (Some(r), Some(g), Some(b)) => hsv_text((r as u8, g as u8, b as u8)),
                    _ => all.to_string(),
                };
            }
            let f = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<f64>().ok());
            match (f(5), f(6), f(7)) {
                (Some(h), Some(s), Some(v)) => super::htmlstyle::hex(hsv_to_rgb(h, s, v)),
                _ => all.to_string(),
            }
        })
        .into_owned()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// UTF-8 のバイト列を Base64（`=` の埋めあり）に。
pub fn base64_encode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 のバイト列（空白・改行は無視。`-_` の URL 用の形も読む。`=` は無くてもよい）。読めなければ None。
pub fn base64_bytes(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut ended = false;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                ended = true;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if ended {
            return None;
        }
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // 余りが 6 ビット（1 文字）だけなのは、どの長さの元にもならない
    (bits < 6).then_some(out)
}

/// Base64 を文字に戻す。UTF-8 の文字として読めなければ None（画像などのバイナリは戻さない）。
pub fn base64_decode(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    String::from_utf8(base64_bytes(t)?).ok()
}

/// `& < > " '` を名前の参照に、ASCII 以外の文字を `&#xHEX;` にする。
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if !c.is_ascii() => out.push_str(&format!("&#x{:x};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn html_unescape(s: &str) -> String {
    super::htmlview::decode_entities(s)
}

/// 半角カタカナ（U+FF65〜U+FF9F）に対応する全角。
const HALF_KANA: &str = "・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜";

fn half_kana_to_full(c: char) -> Option<char> {
    let i = (c as u32).checked_sub(0xFF65)? as usize;
    HALF_KANA.chars().nth(i)
}

fn full_kana_to_half(c: char) -> Option<char> {
    let i = HALF_KANA.chars().position(|k| k == c)?;
    char::from_u32(0xFF65 + i as u32)
}

/// 濁点・半濁点の付いた全角カタカナを、元の字と、付く点（0 = ゛、1 = ゜）に分ける。
fn split_voiced(c: char) -> Option<(char, usize)> {
    if c == 'ヴ' {
        return Some(('ウ', 0));
    }
    let base = |x: u32| char::from_u32(x);
    let u = c as u32;
    // カ〜ト（ッ を除く）は 1 つ後ろが濁音、ハ〜ホは 1 つ後ろが濁音・2 つ後ろが半濁音
    if (0x30AC..=0x30C9).contains(&u) && u != 0x30C3 {
        let b = u - 1;
        if matches!(base(b), Some(k) if full_kana_to_half(k).is_some() && "カキクケコサシスセソタチツテト".contains(k)) {
            return base(b).map(|k| (k, 0));
        }
    }
    if (0x30CF..=0x30DD).contains(&u) {
        let off = (u - 0x30CF) % 3;
        let b = u - off;
        if off > 0 {
            return base(b).map(|k| (k, off as usize - 1));
        }
    }
    None
}

fn is_full_width_ascii(c: char) -> bool {
    ('\u{FF01}'..='\u{FF5E}').contains(&c) || c == '\u{3000}'
}

/// 全角と半角を切り替える。全角の英数字・記号・空白を含むか、英数字が無く全角のカタカナだけを含むなら「半角へ」、
/// そうでなければ「全角へ」。カタカナも同じ向きに変える（濁点・半濁点は分ける・合わせる）。
pub fn width_toggle(s: &str) -> String {
    let to_half = s.chars().any(is_full_width_ascii) || (!s.chars().any(|c| ('\u{FF65}'..='\u{FF9F}').contains(&c)) && s.chars().any(|c| full_kana_to_half(c).is_some() || split_voiced(c).is_some()) && !s.chars().any(|c| c.is_ascii_graphic()));
    let mut out = String::with_capacity(s.len());
    if to_half {
        for c in s.chars() {
            if c == '\u{3000}' {
                out.push(' ');
            } else if is_full_width_ascii(c) {
                out.push(char::from_u32(c as u32 - 0xFEE0).unwrap_or(c));
            } else if let Some((b, v)) = split_voiced(c) {
                out.push(full_kana_to_half(b).unwrap_or(b));
                out.push(if v == 0 { 'ﾞ' } else { 'ﾟ' });
            } else if let Some(h) = full_kana_to_half(c).filter(|_| c != '゛' && c != '゜') {
                out.push(h);
            } else {
                out.push(c);
            }
        }
    } else {
        let cs: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == ' ' {
                out.push('\u{3000}');
            } else if c.is_ascii_graphic() {
                out.push(char::from_u32(c as u32 + 0xFEE0).unwrap_or(c));
            } else if let Some(f) = half_kana_to_full(c) {
                // 後ろの濁点・半濁点を合わせる
                let next = cs.get(i + 1).copied();
                let combined = match next {
                    Some('ﾞ') if f == 'ウ' => Some('ヴ'),
                    Some('ﾞ') if "カキクケコサシスセソタチツテトハヒフヘホ".contains(f) => char::from_u32(f as u32 + 1),
                    Some('ﾟ') if "ハヒフヘホ".contains(f) => char::from_u32(f as u32 + 2),
                    _ => None,
                };
                match combined {
                    Some(v) => {
                        out.push(v);
                        i += 1;
                    }
                    None => out.push(f),
                }
            } else {
                out.push(c);
            }
            i += 1;
        }
    }
    out
}

/// ひらがなとカタカナを切り替える（ひらがなを含むならカタカナへ、そうでなければひらがなへ）。
pub fn kana_toggle(s: &str) -> String {
    let hira = |c: char| ('\u{3041}'..='\u{3096}').contains(&c) || matches!(c, 'ゝ' | 'ゞ');
    let kata = |c: char| ('\u{30A1}'..='\u{30F6}').contains(&c) || matches!(c, 'ヽ' | 'ヾ');
    let to_kata = s.chars().any(hira);
    s.chars()
        .map(|c| match (to_kata, hira(c), kata(c)) {
            (true, true, _) => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            (false, _, true) => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Snake,
    Camel,
    Pascal,
    Kebab,
}

fn words_of(id: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let cs: Vec<char> = id.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if c == '_' || c == '-' {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // 小文字・数字の後の大文字、または「大文字の並び」の最後の大文字（HTTPServer → HTTP Server）で区切る
        let prev = i.checked_sub(1).map(|j| cs[j]);
        let next = cs.get(i + 1).copied();
        let boundary = c.is_ascii_uppercase() && !cur.is_empty() && (prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) || (prev.is_some_and(|p| p.is_ascii_uppercase()) && next.is_some_and(|n| n.is_ascii_lowercase())));
        if boundary {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn case_of(id: &str) -> Option<Case> {
    if id.contains('_') {
        Some(Case::Snake)
    } else if id.contains('-') {
        Some(Case::Kebab)
    } else if id.chars().any(|c| c.is_ascii_uppercase()) && id.chars().any(|c| c.is_ascii_lowercase()) {
        Some(if id.starts_with(|c: char| c.is_ascii_uppercase()) { Case::Pascal } else { Case::Camel })
    } else {
        None
    }
}

fn join_case(words: &[String], case: Case) -> String {
    let cap = |w: &str| {
        let mut c = w.chars();
        c.next().map(|f| f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()).unwrap_or_default()
    };
    match case {
        Case::Snake => words.iter().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().join("_"),
        Case::Kebab => words.iter().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().join("-"),
        Case::Pascal => words.iter().map(|w| cap(w)).collect(),
        Case::Camel => words.iter().enumerate().map(|(i, w)| if i == 0 { w.to_ascii_lowercase() } else { cap(w) }).collect(),
    }
}

fn ident_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z][A-Za-z0-9]*(?:[_-][A-Za-z0-9]+)*").expect("名前の正規表現"))
}

/// 名前の書き方を `snake_case` → `camelCase` → `PascalCase` → `kebab-case` → `snake_case` と進める。
/// 向きは、最初に見つかった 2 語以上の名前の書き方で決め、2 語以上の名前をすべてその次の書き方にそろえる。
pub fn case_cycle(s: &str) -> String {
    let first = ident_re().find_iter(s).find_map(|m| case_of(m.as_str()).filter(|_| words_of(m.as_str()).len() >= 2));
    let Some(from) = first else { return s.to_string() };
    let to = match from {
        Case::Snake => Case::Camel,
        Case::Camel => Case::Pascal,
        Case::Pascal => Case::Kebab,
        Case::Kebab => Case::Snake,
    };
    ident_re()
        .replace_all(s, |c: &Captures| {
            let id = &c[0];
            let w = words_of(id);
            if case_of(id).is_some() && w.len() >= 2 { join_case(&w, to) } else { id.to_string() }
        })
        .into_owned()
}

/// 西暦の日付（グレゴリオ暦）を、1970-01-01 からの日数に（負もよい）。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        assert_eq!(base64_encode("あ"), "44GC");
        assert_eq!(base64_encode("ab"), "YWI=");
        assert_eq!(base64_decode("44GC"), Some("あ".into()));
        assert_eq!(base64_decode("YWI"), Some("ab".into()), "= が無くても読む");
        assert_eq!(base64_decode("not base64!"), None);
        assert_eq!(base64_decode("/w=="), None, "UTF-8 でないものは戻さない");
        let s = "日本語 🎉 text";
        assert_eq!(base64_decode(&base64_encode(s)), Some(s.into()));
    }

    #[test]
    fn html_round_trips() {
        assert_eq!(html_escape("<a href=\"x\">あ & 'b'</a>"), "&lt;a href=&quot;x&quot;&gt;&#x3042; &amp; &#39;b&#39;&lt;/a&gt;");
        assert_eq!(html_unescape("&#x3042;&amp;&lt;"), "あ&<");
        let s = "<p>日本 & \"🎉\"</p>";
        assert_eq!(html_unescape(&html_escape(s)), s);
    }

    #[test]
    fn width_and_kana_toggle() {
        assert_eq!(HALF_KANA.chars().count(), 0xFF9F - 0xFF65 + 1);
        assert_eq!(width_toggle("ＡＢＣ１２３　！"), "ABC123 !");
        assert_eq!(width_toggle("ABC 123!"), "ＡＢＣ　１２３！");
        assert_eq!(width_toggle("ｶﾞｷﾞﾊﾟｳﾞｱ"), "ガギパヴア");
        assert_eq!(width_toggle("ガギパヴア"), "ｶﾞｷﾞﾊﾟｳﾞｱ");
        assert_eq!(width_toggle("ｶﾞ"), "ガ");
        assert_eq!(kana_toggle("ひらがな"), "ヒラガナ");
        assert_eq!(kana_toggle("カタカナとABC"), "カタカナとABC".replace("と", "ト"), "ひらがなを含むのでカタカナへ");
        assert_eq!(kana_toggle("カタカナ"), "かたかな");
    }

    #[test]
    fn case_cycles_through_four_styles() {
        assert_eq!(case_cycle("user_name = max_id"), "userName = maxId");
        assert_eq!(case_cycle("userName"), "UserName");
        assert_eq!(case_cycle("UserName"), "user-name");
        assert_eq!(case_cycle("user-name"), "user_name");
        assert_eq!(case_cycle("parseHTTPServer"), "ParseHttpServer");
        assert_eq!(case_cycle("one two"), "one two", "1 語の名前は変えない");
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 19) * 86_400, 1_795_046_400);
    }

    #[test]
    fn unicode_escape_round_trips() {
        assert_eq!(unicode_escape("aあ"), "a\\u3042");
        assert_eq!(unicode_escape("😀"), "\\ud83d\\ude00");
        assert_eq!(unicode_unescape("a\\u3042"), "aあ");
        assert_eq!(unicode_unescape("\\ud83d\\ude00"), "😀");
        assert_eq!(unicode_unescape("\\u{1F600}\\U0001F600"), "😀😀");
        assert_eq!(unicode_unescape("\\u30 \\x41 \\\\n"), "\\u30 \\x41 \\\\n", "読めない並びは残す");
        let s = "日本語 and 🎉 text";
        assert_eq!(unicode_unescape(&unicode_escape(s)), s);
    }

    #[test]
    fn url_encode_round_trips() {
        assert_eq!(url_encode("あ"), "%E3%81%82");
        assert_eq!(url_encode("https://a.b/c d?q=あ"), "https://a.b/c%20d?q=%E3%81%82");
        assert_eq!(url_decode("%E3%81%82"), "あ");
        assert_eq!(url_decode("100% %zz %E3%81"), "100% %zz %E3%81", "読めない並びは残す");
        assert_eq!(url_decode("a+b%20c"), "a+b c");
        let s = "検索 = テスト & 😀";
        assert_eq!(url_decode(&url_encode(s)), s);
    }

    #[test]
    fn colors_convert_between_hex_rgb_and_hsv() {
        assert_eq!(rgb_to_hsv((255, 0, 0)), (0.0, 100.0, 100.0));
        assert_eq!(hsv_to_rgb(120.0, 100.0, 100.0), (0, 255, 0));
        assert_eq!(color_cycle("#3366cc"), "rgb(51, 102, 204)");
        assert_eq!(color_cycle("#fff"), "rgb(255, 255, 255)");
        assert_eq!(color_cycle("rgb(51, 102, 204)"), "hsv(220, 75%, 80%)");
        assert_eq!(color_cycle("hsv(220, 75%, 80%)"), "#3366cc");
        assert_eq!(color_cycle("color: #ff0000; x: rgb(300, 0, 0)"), "color: rgb(255, 0, 0); x: rgb(300, 0, 0)");
        assert_eq!(color_cycle("#define #12345678"), "#define #12345678", "色でない・透明度つきは変えない");
        for c in [(0, 0, 0), (12, 200, 99), (255, 255, 255), (128, 64, 255)] {
            let (h, s, v) = rgb_to_hsv(c);
            assert_eq!(hsv_to_rgb(h, s, v), c);
        }
        assert_eq!(parse_hsv("hsv(0, 100%, 100%)"), Some((255, 0, 0)));
        assert_eq!(color_formats((255, 0, 0)), "#ff0000  rgb(255, 0, 0)  hsv(0, 100%, 100%)");
    }
}
