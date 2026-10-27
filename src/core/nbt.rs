// Minecraft（Java 版）の NBT。バイナリ（ビッグエンディアン・名前付きの複合タグ 1 つ）と、
// 文字で書いた SNBT（/data コマンドと同じ書き方）を行き来する。GUI なし・依存なし。
// level.dat・playerdata は gzip、.nbt（構造物）は gzip、リージョンのチャンクは zlib で包まれている（compression で扱う）。
use super::deflate;
use std::fmt::Write;

/// 解凍した NBT の大きさの上限（壊れた・悪意ある入力で、メモリを使い切らないように）。
pub const MAX_RAW: usize = 256 * 1024 * 1024;
/// 入れ子の深さの上限（Minecraft と同じ 512）。
const MAX_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    /// 要素の型の番号（空のリストでも残す）と要素。
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
}

/// ファイルの包み方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zlib,
}

impl Compression {
    pub fn label(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Gzip => "gzip",
            Compression::Zlib => "zlib",
        }
    }
}

/// 包みを見分けて解く。
pub fn unpack(bytes: &[u8]) -> Result<(Compression, Vec<u8>), String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return deflate::gunzip(bytes, MAX_RAW).map(|d| (Compression::Gzip, d));
    }
    if bytes.len() > 2 && bytes[0] == 0x78 && (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 31 == 0 {
        return deflate::unzlib(bytes, MAX_RAW).map(|d| (Compression::Zlib, d));
    }
    Ok((Compression::None, bytes.to_vec()))
}

pub fn pack(c: Compression, raw: &[u8]) -> Vec<u8> {
    match c {
        Compression::None => raw.to_vec(),
        Compression::Gzip => deflate::gzip(raw),
        Compression::Zlib => deflate::zlib(raw),
    }
}

/// NBT のファイルとして開いたときに覚えておくこと（保存で元の形に戻すため）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NbtFile {
    pub compression: Compression,
    /// 根の複合タグの名前（ほとんどは空）。
    pub root_name: String,
}

/// NBT として開く拡張子（level.dat・playerdata・構造物・スケマティック）。
pub fn is_nbt_path(p: &std::path::Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| matches!(e.as_str(), "dat" | "dat_old" | "nbt" | "schematic" | "schem" | "litematic"))
}

/// ファイルの中身（包まれたまま）を SNBT の文字にする。
pub fn open(bytes: &[u8]) -> Result<(NbtFile, String), String> {
    let (compression, raw) = unpack(bytes)?;
    open_raw(compression, &raw)
}

/// 解凍済みの NBT を SNBT の文字にする（リージョンのチャンク）。
pub fn open_raw(compression: Compression, raw: &[u8]) -> Result<(NbtFile, String), String> {
    let (root_name, root) = read(raw)?;
    Ok((NbtFile { compression, root_name }, to_snbt(&root)))
}

/// SNBT の文字を、解凍済みの NBT のバイト列にする（包むのは呼び出し側）。
pub fn save_raw(file: &NbtFile, text: &str) -> Result<Vec<u8>, SnbtError> {
    Ok(write(&file.root_name, &from_snbt(text)?))
}

// ---- バイナリ ----

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| format!("NBT が途中で切れています（{} バイト目）", self.p))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap_or_default()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap_or_default()))
    }
    fn len(&mut self) -> Result<usize, String> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| format!("長さが負です（{} バイト目）", self.p - 4))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.i16()? as u16 as usize;
        Ok(decode_mutf8(self.take(n)?))
    }
    fn payload(&mut self, id: u8, depth: usize) -> Result<Tag, String> {
        if depth > MAX_DEPTH {
            return Err("入れ子が深すぎます".into());
        }
        Ok(match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&x| x as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let n = self.len()?;
                if elem == 0 && n > 0 {
                    return Err("End のリストに要素があります".into());
                }
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.payload(elem, depth + 1)?);
                }
                Tag::List(elem, v)
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    v.push((name, self.payload(t, depth + 1)?));
                }
                Tag::Compound(v)
            }
            11 => {
                let n = self.len()?;
                Tag::IntArray(self.take(n.checked_mul(4).ok_or("長さが大きすぎます")?)?.chunks(4).map(|c| i32::from_be_bytes(c.try_into().unwrap_or_default())).collect())
            }
            12 => {
                let n = self.len()?;
                Tag::LongArray(self.take(n.checked_mul(8).ok_or("長さが大きすぎます")?)?.chunks(8).map(|c| i64::from_be_bytes(c.try_into().unwrap_or_default())).collect())
            }
            _ => return Err(format!("知らないタグの種類です: {id}（{} バイト目）", self.p - 1)),
        })
    }
}

/// 解凍済みのバイナリを読む。戻り値は (根の名前, 根の複合タグ)。
pub fn read(raw: &[u8]) -> Result<(String, Tag), String> {
    let mut r = Reader { b: raw, p: 0 };
    let id = r.u8()?;
    if id != 10 {
        return Err(format!("NBT ではありません（先頭のタグが複合タグではなく {id}）"));
    }
    let name = r.string()?;
    let tag = r.payload(10, 0)?;
    Ok((name, tag))
}

fn put_string(out: &mut Vec<u8>, s: &str) {
    let b = encode_mutf8(s);
    out.extend((b.len().min(u16::MAX as usize) as u16).to_be_bytes());
    out.extend(&b[..b.len().min(u16::MAX as usize)]);
}

fn put_payload(out: &mut Vec<u8>, t: &Tag) {
    match t {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend(v.to_be_bytes()),
        Tag::Int(v) => out.extend(v.to_be_bytes()),
        Tag::Long(v) => out.extend(v.to_be_bytes()),
        Tag::Float(v) => out.extend(v.to_bits().to_be_bytes()),
        Tag::Double(v) => out.extend(v.to_bits().to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            out.extend(v.iter().map(|&x| x as u8));
        }
        Tag::String(s) => put_string(out, s),
        Tag::List(elem, v) => {
            out.push(if v.is_empty() { *elem } else { v[0].id() });
            out.extend((v.len() as i32).to_be_bytes());
            for x in v {
                put_payload(out, x);
            }
        }
        Tag::Compound(v) => {
            for (k, x) in v {
                out.push(x.id());
                put_string(out, k);
                put_payload(out, x);
            }
            out.push(0);
        }
        Tag::IntArray(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            for x in v {
                out.extend(x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            for x in v {
                out.extend(x.to_be_bytes());
            }
        }
    }
}

/// 根の名前と複合タグを、解凍済みのバイナリにする。
pub fn write(name: &str, root: &Tag) -> Vec<u8> {
    let mut out = vec![10];
    put_string(&mut out, name);
    put_payload(&mut out, root);
    out
}

/// Java の「修正 UTF-8」（NUL は C0 80、BMP の外はサロゲートの組を 3 バイトずつ）を読む。壊れたバイトは置換文字。
fn decode_mutf8(b: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(b) {
        if !s.contains('\u{0}') {
            return s.to_string(); // ほとんどの文字列はふつうの UTF-8 と同じ
        }
    }
    let mut units: Vec<u16> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            units.push(c as u16);
            i += 1;
        } else if c & 0xe0 == 0xc0 && i + 1 < b.len() {
            units.push(((c as u16 & 0x1f) << 6) | (b[i + 1] as u16 & 0x3f));
            i += 2;
        } else if c & 0xf0 == 0xe0 && i + 2 < b.len() {
            units.push(((c as u16 & 0x0f) << 12) | ((b[i + 1] as u16 & 0x3f) << 6) | (b[i + 2] as u16 & 0x3f));
            i += 3;
        } else {
            units.push(0xfffd);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

fn encode_mutf8(s: &str) -> Vec<u8> {
    if !s.contains('\u{0}') && s.chars().all(|c| (c as u32) < 0x10000) {
        return s.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(s.len() + 8);
    for u in s.encode_utf16() {
        match u {
            0x01..=0x7f => out.push(u as u8),
            0 | 0x80..=0x7ff => {
                out.push(0xc0 | (u >> 6) as u8);
                out.push(0x80 | (u & 0x3f) as u8);
            }
            _ => {
                out.push(0xe0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3f) as u8);
                out.push(0x80 | (u & 0x3f) as u8);
            }
        }
    }
    out
}

// ---- SNBT（文字）----

/// 引用符なしで書ける名前・文字か（Minecraft の SNBT と同じ範囲）。
fn is_plain(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
}

fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

fn key(s: &str) -> String {
    if is_plain(s) { s.to_string() } else { quote(s) }
}

fn float_text(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E']) { s } else { format!("{s}.0") }
}

fn float32_text(v: f32) -> String {
    if v.is_finite() {
        let s = format!("{v}");
        if s.contains(['.', 'e', 'E']) { s } else { format!("{s}.0") }
    } else {
        float_text(v as f64)
    }
}

/// 1 行で書く値か（数・文字列・配列と、数だけのリスト）。
fn is_inline(t: &Tag) -> bool {
    match t {
        Tag::Compound(v) => v.is_empty(),
        Tag::List(_, v) => {
            v.is_empty()
                || (v.len() <= 16 && v.iter().all(|x| !matches!(x, Tag::Compound(_) | Tag::List(..) | Tag::String(_))))
                || (v.len() <= 4 && v.iter().all(|x| matches!(x, Tag::String(s) if s.chars().count() <= 24)))
        }
        _ => true,
    }
}

fn write_snbt(o: &mut String, t: &Tag, indent: usize) {
    let pad = |o: &mut String, n: usize| o.push_str(&"    ".repeat(n));
    match t {
        Tag::Byte(v) => {
            let _ = write!(o, "{v}b");
        }
        Tag::Short(v) => {
            let _ = write!(o, "{v}s");
        }
        Tag::Int(v) => {
            let _ = write!(o, "{v}");
        }
        Tag::Long(v) => {
            let _ = write!(o, "{v}L");
        }
        Tag::Float(v) => {
            let _ = write!(o, "{}f", float32_text(*v));
        }
        Tag::Double(v) => {
            let _ = write!(o, "{}d", float_text(*v));
        }
        Tag::String(s) => o.push_str(&quote(s)),
        Tag::ByteArray(v) => {
            o.push_str("[B;");
            for (i, x) in v.iter().enumerate() {
                let _ = write!(o, "{}{x}b", if i == 0 { " " } else { ", " });
            }
            o.push(']');
        }
        Tag::IntArray(v) => {
            o.push_str("[I;");
            for (i, x) in v.iter().enumerate() {
                let _ = write!(o, "{}{x}", if i == 0 { " " } else { ", " });
            }
            o.push(']');
        }
        Tag::LongArray(v) => {
            o.push_str("[L;");
            for (i, x) in v.iter().enumerate() {
                let _ = write!(o, "{}{x}L", if i == 0 { " " } else { ", " });
            }
            o.push(']');
        }
        Tag::List(_, v) if is_inline(t) => {
            o.push('[');
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    o.push_str(", ");
                }
                write_snbt(o, x, indent);
            }
            o.push(']');
        }
        Tag::List(_, v) => {
            o.push_str("[\n");
            for (i, x) in v.iter().enumerate() {
                pad(o, indent + 1);
                write_snbt(o, x, indent + 1);
                o.push_str(if i + 1 < v.len() { ",\n" } else { "\n" });
            }
            pad(o, indent);
            o.push(']');
        }
        Tag::Compound(v) if v.is_empty() => o.push_str("{}"),
        Tag::Compound(v) => {
            o.push_str("{\n");
            for (i, (k, x)) in v.iter().enumerate() {
                pad(o, indent + 1);
                o.push_str(&key(k));
                o.push_str(": ");
                write_snbt(o, x, indent + 1);
                o.push_str(if i + 1 < v.len() { ",\n" } else { "\n" });
            }
            pad(o, indent);
            o.push('}');
        }
    }
}

/// 複合タグを、字下げした SNBT にする（末尾に改行）。
pub fn to_snbt(t: &Tag) -> String {
    let mut o = String::new();
    write_snbt(&mut o, t, 0);
    o.push('\n');
    o
}

/// SNBT の書き間違い。行と桁は 1 始まり。
#[derive(Debug, PartialEq)]
pub struct SnbtError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for SnbtError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} 行 {} 列: {}", self.line, self.col, self.message)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    p: usize,
    text: &'a str,
}

impl Parser<'_> {
    fn err(&self, message: impl Into<String>) -> SnbtError {
        let before = &self.text[..self.p.min(self.text.len())];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        SnbtError { line, col, message: message.into() }
    }
    fn ws(&mut self) {
        while self.p < self.s.len() && self.s[self.p].is_ascii_whitespace() {
            self.p += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.p).copied()
    }
    fn expect(&mut self, c: u8) -> Result<(), SnbtError> {
        if self.peek() == Some(c) {
            self.p += 1;
            Ok(())
        } else {
            Err(self.err(format!("「{}」がありません", c as char)))
        }
    }
    fn quoted(&mut self) -> Result<String, SnbtError> {
        let q = self.s[self.p];
        self.p += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.p..];
            let Some(c) = rest.chars().next() else { return Err(self.err("引用符が閉じていません")) };
            self.p += c.len_utf8();
            match c {
                '\\' => {
                    let Some(e) = self.text[self.p..].chars().next() else { return Err(self.err("引用符が閉じていません")) };
                    self.p += e.len_utf8();
                    out.push(match e {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    });
                }
                c if c as u32 == q as u32 => return Ok(out),
                c => out.push(c),
            }
        }
    }
    fn bare(&mut self) -> String {
        let start = self.p;
        while self.p < self.s.len() && (self.s[self.p].is_ascii_alphanumeric() || matches!(self.s[self.p], b'_' | b'-' | b'.' | b'+')) {
            self.p += 1;
        }
        self.text[start..self.p].to_string()
    }
    fn name(&mut self) -> Result<String, SnbtError> {
        match self.peek() {
            Some(b'"' | b'\'') => self.quoted(),
            _ => {
                let n = self.bare();
                if n.is_empty() { Err(self.err("名前がありません")) } else { Ok(n) }
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Tag, SnbtError> {
        if depth > MAX_DEPTH {
            return Err(self.err("入れ子が深すぎます"));
        }
        match self.peek() {
            None => Err(self.err("値がありません")),
            Some(b'{') => {
                self.p += 1;
                let mut v: Vec<(String, Tag)> = Vec::new();
                if self.peek() == Some(b'}') {
                    self.p += 1;
                    return Ok(Tag::Compound(v));
                }
                loop {
                    let at = self.p;
                    let k = self.name()?;
                    if v.iter().any(|(x, _)| *x == k) {
                        self.p = at;
                        return Err(self.err(format!("名前「{k}」が 2 回あります")));
                    }
                    self.expect(b':')?;
                    let x = self.value(depth + 1)?;
                    v.push((k, x));
                    match self.peek() {
                        Some(b',') => {
                            self.p += 1;
                            if self.peek() == Some(b'}') {
                                self.p += 1;
                                return Ok(Tag::Compound(v));
                            }
                        }
                        Some(b'}') => {
                            self.p += 1;
                            return Ok(Tag::Compound(v));
                        }
                        _ => return Err(self.err("「,」か「}」がありません")),
                    }
                }
            }
            Some(b'[') => {
                self.p += 1;
                self.ws();
                let rest = &self.s[self.p..];
                if rest.len() >= 2 && matches!(rest[0], b'B' | b'I' | b'L') && rest[1] == b';' {
                    let kind = rest[0];
                    self.p += 2;
                    return self.array(kind);
                }
                let mut v: Vec<Tag> = Vec::new();
                if self.peek() == Some(b']') {
                    self.p += 1;
                    return Ok(Tag::List(0, v));
                }
                loop {
                    let at = self.p;
                    let x = self.value(depth + 1)?;
                    if let Some(first) = v.first() {
                        if first.id() != x.id() {
                            self.p = at;
                            return Err(self.err("リストの要素の型がそろっていません"));
                        }
                    }
                    v.push(x);
                    match self.peek() {
                        Some(b',') => {
                            self.p += 1;
                            if self.peek() == Some(b']') {
                                self.p += 1;
                                break;
                            }
                        }
                        Some(b']') => {
                            self.p += 1;
                            break;
                        }
                        _ => return Err(self.err("「,」か「]」がありません")),
                    }
                }
                let id = v[0].id();
                Ok(Tag::List(id, v))
            }
            Some(b'"' | b'\'') => Ok(Tag::String(self.quoted()?)),
            Some(_) => {
                let at = self.p;
                let w = self.bare();
                if w.is_empty() {
                    return Err(self.err("値として読めない文字です"));
                }
                Ok(number(&w).unwrap_or_else(|| {
                    let _ = at;
                    Tag::String(w)
                }))
            }
        }
    }
    fn array(&mut self, kind: u8) -> Result<Tag, SnbtError> {
        let mut nums: Vec<Tag> = Vec::new();
        if self.peek() != Some(b']') {
            loop {
                self.ws();
                let w = self.bare();
                let t = number(&w).ok_or_else(|| self.err(format!("配列の要素が数ではありません: {w}")))?;
                nums.push(t);
                match self.peek() {
                    Some(b',') => self.p += 1,
                    Some(b']') => break,
                    _ => return Err(self.err("「,」か「]」がありません")),
                }
            }
        }
        self.p += 1;
        let as_i64 = |t: &Tag| match *t {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            _ => None,
        };
        let mut vals = Vec::with_capacity(nums.len());
        for t in &nums {
            vals.push(as_i64(t).ok_or_else(|| self.err("配列の要素は整数にしてください"))?);
        }
        Ok(match kind {
            b'B' => Tag::ByteArray(vals.iter().map(|&v| v as i8).collect()),
            b'I' => Tag::IntArray(vals.iter().map(|&v| v as i32).collect()),
            _ => Tag::LongArray(vals),
        })
    }
}

/// 引用符なしの語を数として読む（読めなければ None で、文字列として扱う）。
fn number(w: &str) -> Option<Tag> {
    match w {
        "true" => return Some(Tag::Byte(1)),
        "false" => return Some(Tag::Byte(0)),
        _ => {}
    }
    let (body, suffix) = match w.chars().last()? {
        c @ ('b' | 'B' | 's' | 'S' | 'l' | 'L' | 'f' | 'F' | 'd' | 'D') if w.len() > 1 => (&w[..w.len() - 1], Some(c.to_ascii_lowercase())),
        _ => (w, None),
    };
    let special = |b: &str| match b {
        "NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    };
    let is_int = !body.is_empty() && body.trim_start_matches(['-', '+']).chars().all(|c| c.is_ascii_digit()) && body.trim_start_matches(['-', '+']).len() > 0;
    match suffix {
        Some('b') if is_int => body.parse::<i8>().ok().map(Tag::Byte),
        Some('s') if is_int => body.parse::<i16>().ok().map(Tag::Short),
        Some('l') if is_int => body.parse::<i64>().ok().map(Tag::Long),
        Some('f') => special(body).map(|v| v as f32).or_else(|| body.parse::<f32>().ok()).map(Tag::Float),
        Some('d') => special(body).or_else(|| body.parse::<f64>().ok()).map(Tag::Double),
        None if is_int => body.parse::<i32>().ok().map(Tag::Int),
        None if body.contains(['.', 'e', 'E']) && body.chars().any(|c| c.is_ascii_digit()) => body.parse::<f64>().ok().map(Tag::Double),
        _ => None,
    }
}

/// SNBT を読む。全体は 1 つの複合タグ。
pub fn from_snbt(text: &str) -> Result<Tag, SnbtError> {
    let mut p = Parser { s: text.as_bytes(), p: 0, text };
    if p.peek() != Some(b'{') {
        return Err(p.err("全体を { } で囲んでください（根は複合タグ）"));
    }
    let t = p.value(0)?;
    if p.peek().is_some() {
        return Err(p.err("最後の } の後ろに余分な文字があります"));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tag {
        Tag::Compound(vec![
            ("Data".into(), Tag::Compound(vec![
                ("LevelName".into(), Tag::String("ワールド \"1\"\\x".into())),
                ("hardcore".into(), Tag::Byte(0)),
                ("version".into(), Tag::Int(19133)),
                ("RandomSeed".into(), Tag::Long(-4_611_686_018_427_387_904)),
                ("BorderSize".into(), Tag::Double(59_999_968.0)),
                ("Rot".into(), Tag::List(5, vec![Tag::Float(90.5), Tag::Float(-12.0)])),
                ("Short".into(), Tag::Short(-3)),
                ("Bytes".into(), Tag::ByteArray(vec![1, -2, 3])),
                ("Ints".into(), Tag::IntArray(vec![1, -2])),
                ("Longs".into(), Tag::LongArray(vec![i64::MAX, i64::MIN])),
                ("Empty".into(), Tag::List(0, vec![])),
                ("Players".into(), Tag::List(10, vec![Tag::Compound(vec![("name".into(), Tag::String("a b".into()))])])),
                ("weird key".into(), Tag::Compound(vec![])),
                ("nul\u{0}😀".into(), Tag::String("\u{0}😀".into())),
            ])),
        ])
    }

    #[test]
    fn binary_round_trips_with_every_compression() {
        let raw = write("", &sample());
        assert_eq!(read(&raw).unwrap(), ("".to_string(), sample()));
        for c in [Compression::None, Compression::Gzip, Compression::Zlib] {
            let (got, back) = unpack(&pack(c, &raw)).unwrap();
            assert_eq!(got, c);
            assert_eq!(back, raw);
        }
    }

    #[test]
    fn snbt_round_trips() {
        let text = to_snbt(&sample());
        assert_eq!(from_snbt(&text).unwrap(), sample(), "{text}");
        assert!(text.contains("LevelName: \"ワールド \\\"1\\\"\\\\x\""));
        assert!(text.contains("RandomSeed: -4611686018427387904L"));
        assert!(text.contains("Rot: [90.5f, -12.0f]"));
        assert!(text.contains("Bytes: [B; 1b, -2b, 3b]"));
        assert!(text.contains("\"weird key\": {}"));
    }

    #[test]
    fn modified_utf8_matches_java() {
        assert_eq!(encode_mutf8("\u{0}"), vec![0xc0, 0x80]);
        assert_eq!(encode_mutf8("😀"), vec![0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]);
        assert_eq!(decode_mutf8(&[0xc0, 0x80, b'a']), "\u{0}a");
        assert_eq!(decode_mutf8(&[0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]), "😀");
    }

    #[test]
    fn snbt_accepts_minecraft_shorthands() {
        let t = from_snbt("{a: true, b: 'x y', c: 1.5, d: 2e3, e: stone, f: [], g: [1, 2,], }").unwrap();
        assert_eq!(
            t,
            Tag::Compound(vec![
                ("a".into(), Tag::Byte(1)),
                ("b".into(), Tag::String("x y".into())),
                ("c".into(), Tag::Double(1.5)),
                ("d".into(), Tag::Double(2000.0)),
                ("e".into(), Tag::String("stone".into())),
                ("f".into(), Tag::List(0, vec![])),
                ("g".into(), Tag::List(3, vec![Tag::Int(1), Tag::Int(2)])),
            ])
        );
        // 「:」を含む語は引用符が要る（Minecraft と同じ）
        assert!(from_snbt("{e: minecraft:stone}").is_err());
        assert!(from_snbt("{e: \"minecraft:stone\"}").is_ok());
    }

    #[test]
    fn snbt_errors_point_at_the_problem() {
        let e = from_snbt("{\n  a: 1,\n  b: [1, 2b]\n}").unwrap_err();
        assert_eq!((e.line, e.message.as_str()), (3, "リストの要素の型がそろっていません"));
        assert_eq!(from_snbt("{a: 1, a: 2}").unwrap_err().message, "名前「a」が 2 回あります");
        assert!(from_snbt("[1]").is_err());
        assert!(from_snbt("{a: \"x}").is_err());
        assert!(from_snbt("{a: 1} x").is_err());
        assert!(from_snbt("{a: [B; 1, x]}").is_err());
    }

    #[test]
    fn broken_binary_is_an_error_not_a_panic() {
        let raw = write("", &sample());
        for cut in [0, 1, 5, raw.len() / 2, raw.len() - 1] {
            assert!(read(&raw[..cut]).is_err());
        }
        assert!(read(&[8, 0, 0]).is_err());
        assert!(read(&[10, 0, 0, 9, 0, 0, 0, 0, 0, 0, 5]).is_err()); // End のリストに要素
    }
}
