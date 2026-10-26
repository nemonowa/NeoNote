// xlsx / ods（表計算ブック）→ シートごとの表（GUI なし）。読み取り専用の簡易対応: Preview で見るだけで、編集・計算・保存はしない。
// zip の展開は `archive::read_entry` + `inflate`（自前）、XML は下の最小の走査器で読む。依存は増やさない。
//
// 読むもの: シート名・セルの値（数値・文字列・共有文字列・真偽・エラー・日付）。数式は計算済みの値だけ。
// 読まないもの: 書式（桁数・通貨・色・罫線・列幅）、結合セル、画像・グラフ、非表示シート（xlsx）、ods の数式。
// 日付は、セルの書式が日付・時刻のものだけを `YYYY-MM-DD` / `YYYY-MM-DD HH:MM:SS` / `HH:MM:SS` にする（ods は表示用の文字をそのまま使う）。
use crate::core::{archive, sheet};
use std::collections::HashMap;
use std::path::Path;

/// 読む zip（ファイル全体）の大きさの上限。
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
/// 1 つの xml を展開した後の大きさの上限（小さな zip が巨大に膨らむのを避ける）。
const MAX_XML: usize = 128 * 1024 * 1024;
/// 出すシートの最大数。
const MAX_SHEETS: usize = 50;

/// 1 枚のシート。行・列は `sheet::MAX_ROWS` / `MAX_COLS` までで、超えた分は切って印を付ける。
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub cut_cols: bool,
}

pub fn is_ods(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("ods"))
}

/// zip 全体のバイト列から、全シートを読む。
pub fn read(bytes: &[u8], ods: bool) -> Result<Vec<Sheet>, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("ファイルが大きすぎます（上限 {} MiB）", MAX_BYTES / 1024 / 1024));
    }
    if ods {
        read_ods(bytes)
    } else {
        read_xlsx(bytes)
    }
}

pub(crate) fn entry_text(bytes: &[u8], name: &str) -> Result<String, String> {
    archive::read_entry(bytes, name, MAX_XML).map(|b| String::from_utf8_lossy(&b).into_owned())
}

// ------------------------------------------------------------------ 最小の XML 走査器

pub(crate) enum Ev {
    /// 開始タグ（名前空間の接頭辞を除いた名前, 属性）。`<a/>` は Start のすぐ後に End が来る。
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

/// `ns:name` → `name`。
fn local(n: &str) -> &str {
    n.rsplit(':').next().unwrap_or(n)
}

pub(crate) fn attr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// `&lt;` `&gt;` `&amp;` `&quot;` `&apos;` `&#65;` `&#x41;` を戻す。知らないものはそのまま。
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.char_indices().take(12).find(|&(_, c)| c == ';').map(|(j, _)| j);
        let decoded = end.and_then(|j| {
            let ent = &rest[1..j];
            match ent {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => {
                    let num = ent.strip_prefix('#')?;
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => num.parse::<u32>().ok()?,
                    };
                    char::from_u32(code)
                }
            }
        });
        match (decoded, end) {
            (Some(c), Some(j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) struct Xml<'a> {
    s: &'a str,
    i: usize,
    end_next: Option<String>,
}

impl<'a> Xml<'a> {
    pub(crate) fn new(s: &'a str) -> Self {
        Xml { s, i: 0, end_next: None }
    }
}

/// `<` から始まるタグの終わり（`>` の位置）。属性値の引用符の中の `>` は数えない。
fn tag_end(rest: &str) -> Option<usize> {
    let mut q: Option<u8> = None;
    for (i, &b) in rest.as_bytes().iter().enumerate().skip(1) {
        match q {
            Some(c) if b == c => q = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => q = Some(b),
            None if b == b'>' => return Some(i),
            None => {}
        }
    }
    None
}

fn parse_attrs(mut s: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    loop {
        s = s.trim_start();
        let Some(eq) = s.find('=') else { break };
        let key = s[..eq].trim();
        s = s[eq + 1..].trim_start();
        let Some(q) = s.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let Some(close) = s[1..].find(q) else { break };
        v.push((local(key).to_string(), unescape(&s[1..1 + close])));
        s = &s[close + 2..];
    }
    v
}

impl<'a> Iterator for Xml<'a> {
    type Item = Ev;

    fn next(&mut self) -> Option<Ev> {
        if let Some(n) = self.end_next.take() {
            return Some(Ev::End(n));
        }
        let s: &'a str = self.s;
        loop {
            let rest = &s[self.i..];
            if rest.is_empty() {
                return None;
            }
            if !rest.starts_with('<') {
                let n = rest.find('<').unwrap_or(rest.len());
                self.i += n;
                return Some(Ev::Text(unescape(&rest[..n])));
            }
            if let Some(r) = rest.strip_prefix("<!--") {
                match r.find("-->") {
                    Some(p) => self.i += 4 + p + 3,
                    None => self.i = self.s.len(),
                }
                continue;
            }
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                return match r.find("]]>") {
                    Some(p) => {
                        self.i += 9 + p + 3;
                        Some(Ev::Text(r[..p].to_string()))
                    }
                    None => {
                        self.i = self.s.len();
                        None
                    }
                };
            }
            let Some(end) = tag_end(rest) else {
                self.i = self.s.len();
                return None;
            };
            self.i += end + 1;
            let inner = &rest[1..end];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue; // <?xml ?> / <!DOCTYPE>
            }
            if let Some(name) = inner.strip_prefix('/') {
                return Some(Ev::End(local(name.trim()).to_string()));
            }
            let self_close = inner.ends_with('/');
            let inner = inner.strip_suffix('/').unwrap_or(inner);
            let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            let name = local(&inner[..name_end]).to_string();
            let attrs = parse_attrs(&inner[name_end..]);
            if self_close {
                self.end_next = Some(name.clone());
            }
            return Some(Ev::Start(name, attrs));
        }
    }
}

// ------------------------------------------------------------------ 日付（xlsx）

#[derive(Clone, Copy, Debug, PartialEq)]
enum DateKind {
    Date,
    DateTime,
    Time,
}

/// 組み込みの表示形式の番号から。14〜17 と 27〜31・34〜36・50〜58（日本語などの日付）は日付、22 は日時、18〜21・32・33・45〜47 は時刻。
fn builtin_kind(id: u32) -> Option<DateKind> {
    match id {
        14..=17 | 27..=31 | 34..=36 | 50..=58 => Some(DateKind::Date),
        22 => Some(DateKind::DateTime),
        18..=21 | 32 | 33 | 45..=47 => Some(DateKind::Time),
        _ => None,
    }
}

/// 独自の表示形式（`yyyy/m/d` `h:mm` など）が日付・時刻か。引用符・`[...]`・`\` と `_` `*` の次の 1 文字は読み飛ばす。
fn classify_code(code: &str) -> Option<DateKind> {
    if code.eq_ignore_ascii_case("general") {
        return None;
    }
    let (mut date, mut time) = (false, false);
    let mut it = code.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                for d in it.by_ref() {
                    if d == '"' {
                        break;
                    }
                }
            }
            '[' => {
                for d in it.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                it.next();
            }
            'y' | 'Y' | 'd' | 'D' => date = true,
            'h' | 'H' | 's' | 'S' => time = true,
            _ => {}
        }
    }
    match (date, time) {
        (true, true) => Some(DateKind::DateTime),
        (true, false) => Some(DateKind::Date),
        (false, true) => Some(DateKind::Time),
        _ => None,
    }
}

/// 1970-01-01 からの日数 → (年, 月, 日)。H. Hinnant の civil_from_days。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (mp * 153 + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Excel のシリアル値 → 日付・時刻の文字。範囲外なら None（呼び出し側が元の数のまま出す）。
/// 1900 年 3 月 1 日より前（シリアル 60 以下。Excel の 1900 年うるう年の誤りの範囲）は 1 日ずれる。
fn serial_text(v: f64, kind: DateKind, date1904: bool) -> Option<String> {
    if !v.is_finite() || !(0.0..=2_958_465.0).contains(&v) {
        return None;
    }
    let whole = v.floor();
    let mut days = whole as i64;
    let mut secs = ((v - whole) * 86_400.0).round() as i64;
    if secs >= 86_400 {
        secs -= 86_400;
        days += 1;
    }
    if date1904 {
        days += 1462;
    }
    let (y, m, d) = civil_from_days(days - 25_569); // 25569 = 1970-01-01 のシリアル値
    let ymd = format!("{y:04}-{m:02}-{d:02}");
    let time = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    Some(match kind {
        DateKind::Date => ymd,
        DateKind::Time => time,
        DateKind::DateTime if secs == 0 => ymd,
        DateKind::DateTime => format!("{ymd} {time}"),
    })
}

// ------------------------------------------------------------------ xlsx

/// `B3` → 列の番号（0 始まり）。`AA` 以降も読む。列より前の英字が無い・長すぎるときは None。
fn col_of(r: &str) -> Option<usize> {
    let letters: Vec<u8> = r.bytes().take_while(u8::is_ascii_alphabetic).collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let n = letters.iter().fold(0usize, |a, b| a * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize);
    Some(n - 1)
}

/// 共有文字列。`<si>` の中の `<t>` をつなぐ（リッチテキストは `<r><t>` の連なり。ふりがな `<rPh>` は除く）。
fn shared_strings(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_si, mut in_t, mut in_ph) = (false, false, false);
    for ev in Xml::new(xml) {
        match ev {
            Ev::Start(n, _) => match n.as_str() {
                "si" => {
                    in_si = true;
                    cur.clear();
                }
                "rPh" => in_ph = true,
                "t" => in_t = true,
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "si" => {
                    in_si = false;
                    out.push(std::mem::take(&mut cur));
                }
                "rPh" => in_ph = false,
                "t" => in_t = false,
                _ => {}
            },
            Ev::Text(t) => {
                if in_si && in_t && !in_ph {
                    cur.push_str(&t);
                }
            }
        }
    }
    out
}

/// セルの書式の番号（`s`）→ 日付・時刻か。`xl/styles.xml` の `cellXfs` の並びがそのまま `s` の番号。
fn read_styles(bytes: &[u8]) -> Vec<Option<DateKind>> {
    let Ok(x) = entry_text(bytes, "xl/styles.xml") else { return Vec::new() };
    let mut codes: HashMap<u32, String> = HashMap::new();
    let mut in_xfs = false;
    let mut out = Vec::new();
    for ev in Xml::new(&x) {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (attr(&a, "numFmtId").and_then(|s| s.parse::<u32>().ok()), attr(&a, "formatCode")) {
                        codes.insert(id, code.to_string());
                    }
                }
                "cellXfs" => in_xfs = true,
                "xf" if in_xfs => {
                    let id = attr(&a, "numFmtId").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                    out.push(match codes.get(&id) {
                        Some(c) => classify_code(c),
                        None => builtin_kind(id),
                    });
                }
                _ => {}
            },
            Ev::End(n) => {
                if n == "cellXfs" {
                    in_xfs = false;
                }
            }
            Ev::Text(_) => {}
        }
    }
    out
}

struct XlsxCtx<'a> {
    shared: &'a [String],
    styles: &'a [Option<DateKind>],
    date1904: bool,
}

/// セルの種類 `t`・書式 `s`・中身の文字から、見せる文字を決める。
fn cell_text(ctx: &XlsxCtx, ty: &str, style: Option<usize>, raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    match ty {
        "s" => raw.trim().parse::<usize>().ok().and_then(|i| ctx.shared.get(i)).cloned().unwrap_or_default(),
        "b" => (if raw.trim() == "1" { "TRUE" } else { "FALSE" }).to_string(),
        "n" => {
            let kind = style.and_then(|s| ctx.styles.get(s)).copied().flatten();
            match (kind, raw.trim().parse::<f64>()) {
                (Some(k), Ok(v)) => serial_text(v, k, ctx.date1904).unwrap_or_else(|| raw.to_string()),
                _ => raw.to_string(),
            }
        }
        _ => raw.to_string(), // str（数式の文字の結果）・inlineStr・e（#DIV/0! など）
    }
}

fn parse_xlsx_sheet(name: &str, xml: &str, ctx: &XlsxCtx) -> Sheet {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let (mut truncated, mut cut_cols) = (false, false);
    let (mut next_row, mut row_idx, mut skip_row) = (0usize, 0usize, false);
    let (mut next_col, mut col) = (0usize, 0usize);
    let mut ty = String::new();
    let mut style: Option<usize> = None;
    let mut buf = String::new();
    let (mut in_c, mut in_v, mut in_t) = (false, false, false);
    for ev in Xml::new(xml) {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "row" => {
                    row_idx = attr(&a, "r").and_then(|s| s.parse::<usize>().ok()).map(|r| r.saturating_sub(1)).unwrap_or(next_row);
                    next_row = row_idx + 1;
                    skip_row = row_idx >= sheet::MAX_ROWS;
                    row.clear();
                    next_col = 0;
                }
                "c" => {
                    col = attr(&a, "r").and_then(col_of).unwrap_or(next_col);
                    next_col = col + 1;
                    ty = attr(&a, "t").unwrap_or("n").to_string();
                    style = attr(&a, "s").and_then(|s| s.parse().ok());
                    buf.clear();
                    in_c = true;
                }
                "v" if in_c => in_v = true,
                "t" if in_c => in_t = true, // インライン文字列 <is><t>
                _ => {}
            },
            Ev::Text(t) => {
                if in_v || in_t {
                    buf.push_str(&t);
                }
            }
            Ev::End(n) => match n.as_str() {
                "v" => in_v = false,
                "t" => in_t = false,
                "c" => {
                    in_c = false;
                    let val = cell_text(ctx, &ty, style, &buf);
                    if val.is_empty() {
                    } else if skip_row {
                        truncated = true;
                    } else if col >= sheet::MAX_COLS {
                        cut_cols = true;
                    } else {
                        if row.len() <= col {
                            row.resize(col + 1, String::new());
                        }
                        row[col] = val;
                    }
                }
                "row" => {
                    if skip_row {
                        if truncated {
                            break;
                        }
                    } else {
                        while rows.len() < row_idx {
                            rows.push(Vec::new());
                        }
                        rows.push(std::mem::take(&mut row));
                    }
                }
                _ => {}
            },
        }
    }
    while rows.last().is_some_and(|r| r.is_empty()) {
        rows.pop();
    }
    Sheet { name: name.to_string(), rows, truncated, cut_cols }
}

fn read_xlsx(bytes: &[u8]) -> Result<Vec<Sheet>, String> {
    let wb = entry_text(bytes, "xl/workbook.xml").map_err(|e| format!("xlsx として読めません: {e}"))?;
    let mut date1904 = false;
    // (シート名, 関係の id, 非表示か)
    let mut defs: Vec<(String, Option<String>, bool)> = Vec::new();
    for ev in Xml::new(&wb) {
        if let Ev::Start(n, a) = ev {
            match n.as_str() {
                "workbookPr" => date1904 = matches!(attr(&a, "date1904"), Some("1" | "true")),
                "sheet" => defs.push((
                    attr(&a, "name").unwrap_or("").to_string(),
                    attr(&a, "id").map(String::from),
                    matches!(attr(&a, "state"), Some("hidden" | "veryHidden")),
                )),
                _ => {}
            }
        }
    }
    let mut targets: HashMap<String, String> = HashMap::new();
    if let Ok(rels) = entry_text(bytes, "xl/_rels/workbook.xml.rels") {
        for ev in Xml::new(&rels) {
            if let Ev::Start(n, a) = ev {
                if n == "Relationship" {
                    if let (Some(id), Some(t)) = (attr(&a, "Id"), attr(&a, "Target")) {
                        targets.insert(id.to_string(), t.to_string());
                    }
                }
            }
        }
    }
    let shared = entry_text(bytes, "xl/sharedStrings.xml").map(|x| shared_strings(&x)).unwrap_or_default();
    let styles = read_styles(bytes);
    let ctx = XlsxCtx { shared: &shared, styles: &styles, date1904 };
    let mut out = Vec::new();
    for (i, (name, rid, hidden)) in defs.iter().enumerate() {
        if *hidden {
            continue;
        }
        if out.len() >= MAX_SHEETS {
            break;
        }
        let path = match rid.as_ref().and_then(|r| targets.get(r)) {
            Some(t) => match t.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("xl/{t}"),
            },
            None => format!("xl/worksheets/sheet{}.xml", i + 1), // 関係が読めないときの慣例の名前
        };
        if let Ok(xml) = entry_text(bytes, &path) {
            out.push(parse_xlsx_sheet(name, &xml, &ctx));
        }
    }
    if out.is_empty() {
        Err("表示できるシートがありません".into())
    } else {
        Ok(out)
    }
}

// ------------------------------------------------------------------ ods

fn repeat_of(a: &[(String, String)], k: &str) -> usize {
    attr(a, k).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).max(1)
}

fn parse_ods(xml: &str) -> Vec<Sheet> {
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut cur: Option<Sheet> = None;
    let mut row: Vec<String> = Vec::new();
    // 空の行・セルの繰り返し（1048576 行などの末尾の空）を実際には作らず、後ろに中身が来たときだけ埋める
    let (mut pend_rows, mut row_rep, mut pend_cols) = (0usize, 1usize, 0usize);
    let mut col_rep = 1usize;
    let (mut in_cell, mut in_p) = (false, false);
    let mut p_seen = 0usize;
    let mut text = String::new();
    let mut fallback = String::new();
    for ev in Xml::new(xml) {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "table" => {
                    cur = Some(Sheet { name: attr(&a, "name").unwrap_or("").to_string(), rows: Vec::new(), truncated: false, cut_cols: false });
                    pend_rows = 0;
                }
                "table-row" if cur.is_some() => {
                    row.clear();
                    pend_cols = 0;
                    row_rep = repeat_of(&a, "number-rows-repeated");
                }
                "table-cell" | "covered-table-cell" if cur.is_some() => {
                    in_cell = true;
                    col_rep = repeat_of(&a, "number-columns-repeated");
                    text.clear();
                    p_seen = 0;
                    // 表示用の文字（text:p）が無いときの代わり
                    fallback = ["date-value", "time-value", "boolean-value", "value", "string-value"]
                        .iter()
                        .find_map(|k| attr(&a, k))
                        .unwrap_or("")
                        .to_string();
                }
                "p" if in_cell => {
                    if p_seen > 0 {
                        text.push('\n');
                    }
                    p_seen += 1;
                    in_p = true;
                }
                "s" if in_p => text.push_str(&" ".repeat(repeat_of(&a, "c").min(100))),
                "tab" if in_p => text.push(' '),
                "line-break" if in_p => text.push('\n'),
                _ => {}
            },
            Ev::Text(t) => {
                if in_p {
                    text.push_str(&t);
                }
            }
            Ev::End(n) => match n.as_str() {
                "p" => in_p = false,
                "table-cell" | "covered-table-cell" if in_cell => {
                    in_cell = false;
                    let value = if text.is_empty() { std::mem::take(&mut fallback) } else { std::mem::take(&mut text) };
                    if value.is_empty() {
                        pend_cols = pend_cols.saturating_add(col_rep);
                    } else if let Some(s) = cur.as_mut() {
                        let start = row.len().saturating_add(pend_cols);
                        pend_cols = 0;
                        if start >= sheet::MAX_COLS {
                            s.cut_cols = true;
                        } else {
                            row.resize(start, String::new());
                            let take = col_rep.min(sheet::MAX_COLS - start);
                            for _ in 0..take {
                                row.push(value.clone());
                            }
                            if take < col_rep {
                                s.cut_cols = true;
                            }
                        }
                    }
                }
                "table-row" => {
                    let Some(s) = cur.as_mut() else { continue };
                    if row.is_empty() {
                        pend_rows = pend_rows.saturating_add(row_rep);
                    } else {
                        for _ in 0..pend_rows {
                            if s.rows.len() >= sheet::MAX_ROWS {
                                break;
                            }
                            s.rows.push(Vec::new());
                        }
                        pend_rows = 0;
                        for _ in 0..row_rep {
                            if s.rows.len() >= sheet::MAX_ROWS {
                                s.truncated = true;
                                break;
                            }
                            s.rows.push(row.clone());
                        }
                    }
                    row.clear();
                }
                "table" => {
                    if let Some(s) = cur.take() {
                        if sheets.len() < MAX_SHEETS {
                            sheets.push(s);
                        }
                    }
                }
                _ => {}
            },
        }
    }
    sheets
}

fn read_ods(bytes: &[u8]) -> Result<Vec<Sheet>, String> {
    let xml = entry_text(bytes, "content.xml").map_err(|e| format!("ods として読めません: {e}"))?;
    let sheets = parse_ods(&xml);
    if sheets.is_empty() {
        Err("表示できるシートがありません".into())
    } else {
        Ok(sheets)
    }
}

// ------------------------------------------------------------------ 表示

/// 全シートを HelpView に渡す完全な HTML にする（シートごとに名前の見出し + 表）。色は `#rrggbb`。
pub fn to_html(sheets: &[Sheet], bg: &str, fg: &str, sub: &str) -> String {
    let mut body = String::new();
    for s in sheets {
        let mut name = String::new();
        for c in s.name.chars() {
            match c {
                '&' => name.push_str("&amp;"),
                '<' => name.push_str("&lt;"),
                '>' => name.push_str("&gt;"),
                c => name.push(c),
            }
        }
        body.push_str(&format!("<p><font color=\"{sub}\">── </font><b>{name}</b><font color=\"{sub}\"> ──</font></p>"));
        if s.rows.is_empty() {
            body.push_str("<p>（空のシートです）</p>");
        } else {
            body.push_str(&sheet::table_html(&s.rows, s.truncated, s.cut_cols, false, sub, false));
        }
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の zip（各項目は (名前, 圧縮方式, 圧縮後のバイト列, 展開後の大きさ)）。CRC は読まないので 0。
    fn make_zip(entries: &[(&str, u16, &[u8], usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, method, data, usize_) in entries {
            let off = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]); // version, flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]); // time, date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(*usize_ as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0]); // made by, needed, flags
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&[0; 4]);
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(*usize_ as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attr（外部属性 4 バイトは次の行）
            cd.extend_from_slice(&0u32.to_le_bytes()); // external attr
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn stored<'a>(name: &'a str, data: &'a str) -> (&'a str, u16, &'a [u8], usize) {
        (name, 0, data.as_bytes(), data.len())
    }

    fn v(r: &[&[&str]]) -> Vec<Vec<String>> {
        r.iter().map(|x| x.iter().map(|s| s.to_string()).collect()).collect()
    }

    // ---- XML

    #[test]
    fn xml_events_entities_cdata_comments_and_self_closing() {
        let x = "<?xml version=\"1.0\"?><!-- c --><a:r x=\"1&amp;2\" y='>'><t>a&lt;b&#65;&#x42;</t><e/><![CDATA[<raw>]]></a:r>";
        let evs: Vec<String> = Xml::new(x)
            .map(|e| match e {
                Ev::Start(n, a) => format!("S:{n}:{}", a.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")),
                Ev::End(n) => format!("E:{n}"),
                Ev::Text(t) => format!("T:{t}"),
            })
            .collect();
        assert_eq!(evs, ["S:r:x=1&2,y=>", "S:t:", "T:a<bAB", "E:t", "S:e:", "E:e", "T:<raw>", "E:r"]);
    }

    #[test]
    fn xml_unterminated_input_does_not_panic_or_loop() {
        for x in ["<a", "<a b=\"1", "<!-- x", "<![CDATA[x", "&", "&#;", "&#xZZ;", "<", "</", "x&amp"] {
            let _ = Xml::new(x).count();
        }
        assert_eq!(unescape("a &unknown; b &amp c"), "a &unknown; b &amp c");
    }

    // ---- 日付

    #[test]
    fn serial_dates_and_times() {
        assert_eq!(serial_text(45000.0, DateKind::Date, false).unwrap(), "2023-03-15");
        assert_eq!(serial_text(45000.5, DateKind::DateTime, false).unwrap(), "2023-03-15 12:00:00");
        assert_eq!(serial_text(45000.0, DateKind::DateTime, false).unwrap(), "2023-03-15");
        assert_eq!(serial_text(0.75, DateKind::Time, false).unwrap(), "18:00:00");
        assert_eq!(serial_text(25569.0, DateKind::Date, false).unwrap(), "1970-01-01");
        assert_eq!(serial_text(0.0, DateKind::Date, true).unwrap(), "1904-01-01"); // 1904 年起点
        assert_eq!(serial_text(1.0, DateKind::Date, true).unwrap(), "1904-01-02");
        assert_eq!(serial_text(0.999999999, DateKind::Time, false).unwrap(), "00:00:00"); // 丸めで繰り上がる
        assert!(serial_text(-1.0, DateKind::Date, false).is_none());
        assert!(serial_text(f64::NAN, DateKind::Date, false).is_none());
        assert!(serial_text(1e12, DateKind::Date, false).is_none());
    }

    #[test]
    fn format_codes_are_classified() {
        assert_eq!(classify_code("yyyy/m/d"), Some(DateKind::Date));
        assert_eq!(classify_code("yyyy-mm-dd hh:mm:ss"), Some(DateKind::DateTime));
        assert_eq!(classify_code("h:mm"), Some(DateKind::Time));
        assert_eq!(classify_code("[$-411]ggge\"年\"m\"月\"d\"日\""), Some(DateKind::Date));
        assert_eq!(classify_code("0.00"), None);
        assert_eq!(classify_code("#,##0\" pcs\""), None); // 引用符の中の s は見ない
        assert_eq!(classify_code("0.00E+00"), None);
        assert_eq!(classify_code("General"), None);
        assert_eq!(classify_code("\\d0"), None); // バックスラッシュの次は文字そのもの
        assert_eq!(builtin_kind(14), Some(DateKind::Date));
        assert_eq!(builtin_kind(0), None);
    }

    #[test]
    fn column_letters() {
        assert_eq!(col_of("A1"), Some(0));
        assert_eq!(col_of("Z9"), Some(25));
        assert_eq!(col_of("AA10"), Some(26));
        assert_eq!(col_of("b2"), Some(1));
        assert_eq!(col_of("12"), None);
        assert_eq!(col_of("ABCD1"), None);
    }

    // ---- xlsx

    const WB: &str = r#"<workbook xmlns:r="x"><workbookPr/><sheets><sheet name="売上" sheetId="1" r:id="rId1"/><sheet name="隠し" sheetId="2" state="hidden" r:id="rId2"/><sheet name="B&amp;C" sheetId="3" r:id="rId3"/></sheets></workbook>"#;
    const RELS: &str = r#"<Relationships><Relationship Id="rId1" Type="t" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="t" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="t" Target="/xl/worksheets/s3.xml"/></Relationships>"#;
    const SST: &str = r#"<sst><si><t>名前</t></si><si><r><t>赤</t></r><r><t xml:space="preserve"> 青</t></r><rPh><t>あか</t></rPh></si><si><t>日付</t></si></sst>"#;
    const STYLES: &str = r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy/m/d"/></numFmts><cellStyleXfs><xf numFmtId="14"/></cellStyleXfs><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="22"/></cellXfs></styleSheet>"#;
    const SHEET1: &str = r#"<worksheet><sheetData>
        <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>2</v></c><c r="C1" t="s"><v>1</v></c></row>
        <row r="2"><c r="A2" t="inlineStr"><is><t>x</t></is></c><c r="B2" s="1"><v>45000</v></c><c r="D2" t="b"><v>1</v></c></row>
        <row r="4"><c r="A4"><f>1+2</f><v>3</v></c><c r="B4" s="2"><v>45000.5</v></c><c r="C4" t="e"><v>#DIV/0!</v></c><c r="E4" t="str"><v>a&amp;b</v></c></row>
        <row r="5"><c r="A5"/></row></sheetData></worksheet>"#;

    fn xlsx() -> Vec<u8> {
        make_zip(&[
            stored("xl/workbook.xml", WB),
            stored("xl/_rels/workbook.xml.rels", RELS),
            stored("xl/sharedStrings.xml", SST),
            stored("xl/styles.xml", STYLES),
            stored("xl/worksheets/sheet1.xml", SHEET1),
            stored("xl/worksheets/sheet2.xml", "<worksheet><sheetData/></worksheet>"),
            stored("xl/worksheets/s3.xml", r#"<worksheet><sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#),
        ])
    }

    #[test]
    fn xlsx_reads_values_dates_shared_strings_and_skips_hidden_sheets() {
        let s = read(&xlsx(), false).unwrap();
        assert_eq!(s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["売上", "B&C"]);
        assert_eq!(
            s[0].rows,
            v(&[
                &["名前", "日付", "赤 青"],
                &["x", "2023-03-15", "", "TRUE"],
                &[],
                &["3", "2023-03-15 12:00:00", "#DIV/0!", "", "a&b"],
            ])
        );
        assert!(!s[0].truncated && !s[0].cut_cols);
        assert_eq!(s[1].rows, v(&[&["7"]]));
    }

    #[test]
    fn xlsx_limits_rows_and_columns() {
        let mut rows = String::new();
        for r in 1..=sheet::MAX_ROWS + 3 {
            rows.push_str(&format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c><c r="AE{r}"><v>1</v></c></row>"#));
        }
        let wb = r#"<workbook><sheets><sheet name="s" sheetId="1"/></sheets></workbook>"#;
        let sheet_xml = format!("<worksheet><sheetData>{rows}</sheetData></worksheet>");
        let z = make_zip(&[stored("xl/workbook.xml", wb), stored("xl/worksheets/sheet1.xml", &sheet_xml)]); // 関係なし → sheet1.xml
        let s = read(&z, false).unwrap();
        assert_eq!(s[0].rows.len(), sheet::MAX_ROWS);
        assert!(s[0].truncated && s[0].cut_cols);
        assert_eq!(s[0].rows[0], v(&[&["1"]])[0]);
    }

    #[test]
    fn broken_books_are_errors() {
        assert!(read(b"not a zip", false).is_err());
        assert!(read(&make_zip(&[stored("a.txt", "x")]), false).is_err());
        assert!(read(&make_zip(&[stored("xl/workbook.xml", "<workbook><sheets/></workbook>")]), false).is_err());
        assert!(read(&make_zip(&[stored("a.txt", "x")]), true).is_err());
    }

    #[test]
    fn deflated_entries_are_read_through_inflate() {
        // Python の zlib（wbits=-15, level 9）で作った deflate: "<sst><si><t>A</t></si><si><t>B</t></si></sst>" を 8 回繰り返した 360 バイト
        let packed: &[u8] = &[
            0xb3, 0x29, 0x2e, 0x2e, 0xb1, 0xb3, 0x29, 0xce, 0xb4, 0xb3, 0x29, 0xb1, 0x73, 0xb4, 0xd1, 0x07, 0xb2, 0xf5, 0x41, 0x1c, 0x88, 0x80, 0x13, 0x42, 0x40,
            0x1f, 0xa2, 0x6e, 0x54, 0x31, 0x99, 0x8a, 0x01,
        ];
        let z = make_zip(&[("xl/sharedStrings.xml", 8, packed, 360)]);
        let t = entry_text(&z, "xl/sharedStrings.xml").unwrap();
        assert_eq!(t, "<sst><si><t>A</t></si><si><t>B</t></si></sst>".repeat(8));
        assert_eq!(shared_strings(&t).len(), 16);
        assert!(archive::read_entry(&z, "xl/sharedStrings.xml", 100).is_err()); // 上限
        assert!(archive::read_entry(&z, "nothing", 1000).is_err());
        assert!(archive::read_entry(&make_zip(&[("a", 12, &b"x"[..], 1)]), "a", 10).is_err()); // 未対応の方式
    }

    // ---- ods

    const ODS: &str = r#"<office:document-content><office:body><office:spreadsheet>
        <table:table table:name="Sheet1">
          <table:table-column table:number-columns-repeated="3"/>
          <table:table-row>
            <table:table-cell office:value-type="string"><text:p>名前</text:p></table:table-cell>
            <table:table-cell table:number-columns-repeated="2"/>
            <table:table-cell office:value-type="float" office:value="1.5"><text:p>1,50</text:p></table:table-cell>
          </table:table-row>
          <table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>
          <table:table-row>
            <table:table-cell office:value-type="float" office:value="42"/>
            <table:table-cell office:value-type="string"><text:p>a</text:p><text:p>b<text:s text:c="2"/>c&amp;d</text:p></table:table-cell>
            <table:covered-table-cell/>
            <table:table-cell office:value-type="date" office:date-value="2023-03-15"/>
          </table:table-row>
          <table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>
        </table:table>
        <table:table table:name="Empty"><table:table-row><table:table-cell/></table:table-row></table:table>
        </office:spreadsheet></office:body></office:document-content>"#;

    #[test]
    fn ods_reads_text_repeats_and_drops_trailing_blanks() {
        let s = read(&make_zip(&[stored("content.xml", ODS)]), true).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "Sheet1");
        assert_eq!(s[0].rows, v(&[&["名前", "", "", "1,50"], &[], &[], &["42", "a\nb  c&d", "", "2023-03-15"]]));
        assert!(!s[0].truncated && !s[0].cut_cols);
        assert!(s[1].rows.is_empty());
    }

    #[test]
    fn ods_limits_rows_and_columns() {
        let big = r#"<table:table table:name="t"><table:table-row table:number-rows-repeated="100000"><table:table-cell table:number-columns-repeated="100000"><text:p>x</text:p></table:table-cell></table:table-row></table:table>"#;
        let s = parse_ods(big);
        assert_eq!(s[0].rows.len(), sheet::MAX_ROWS);
        assert_eq!(s[0].rows[0].len(), sheet::MAX_COLS);
        assert!(s[0].truncated && s[0].cut_cols);
    }

    #[test]
    fn is_ods_by_extension() {
        assert!(is_ods(Path::new("a/B.ODS")));
        assert!(!is_ods(Path::new("a.xlsx")));
    }

    // ---- 表示

    #[test]
    fn html_has_sheet_names_tables_and_escapes() {
        let s = read(&xlsx(), false).unwrap();
        let h = to_html(&s, "#000", "#fff", "#888");
        assert!(h.contains("<b>売上</b>") && h.contains("<b>B&amp;C</b>"));
        assert!(h.contains("<table border=\"1\""));
        assert!(h.contains("<td>a&amp;b</td>"));
        assert!(!h.contains("Text で見られます")); // xlsx / ods は Text では見られない
        let e = to_html(&[Sheet { name: "x".into(), rows: vec![], truncated: false, cut_cols: false }], "#000", "#fff", "#888");
        assert!(e.contains("空のシート"));
    }
}
