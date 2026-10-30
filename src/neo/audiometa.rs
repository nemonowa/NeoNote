// 音声ファイル（MP3 / FLAC / WAV）の情報表示とメタデータの読み書き（GUI なし・依存なし）。
//   MP3  … ID3v2.3 / 2.4（題名・歌手・アルバム・年・トラック・ジャンル・コメント）と ID3v1
//   FLAC … Vorbis コメントのブロックだけ
//   WAV  … LIST/INFO チャンクだけ
// 編集しないもの（画像 APIC・歌詞・独自のフレーム・他のブロック・音声そのもの）は、そのまま残す。
// 書き戻しは `Patch`（先頭と末尾の差し替え）で返す。タグが小さくなる / パディングに収まるときは、
// 先頭だけを書き換えれば済む（`in_place`）。
use crate::neo::imgmeta::{escape, parse_lines};
use std::path::Path;

pub fn supports(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| matches!(e.as_str(), "mp3" | "flac" | "wav"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Mp3,
    Flac,
    Wav,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Mp3 => "MP3",
            Format::Flac => "FLAC",
            Format::Wav => "WAV",
        }
    }
}

pub fn format_of(b: &[u8]) -> Option<Format> {
    if b.starts_with(b"fLaC") {
        Some(Format::Flac)
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        Some(Format::Wav)
    } else if b.starts_with(b"ID3") || (b.len() >= 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0) {
        Some(Format::Mp3)
    } else {
        None
    }
}

/// 書き戻しの差分。結果 = `head` + 元の `[head_skip..tail_from]` + `tail`。
#[derive(Debug, Clone)]
pub struct Patch {
    pub head_skip: usize,
    pub head: Vec<u8>,
    pub tail_from: usize,
    pub tail: Vec<u8>,
    /// 先頭と末尾の大きさが変わらない（ファイルの全体を書き直さず、その場で上書きできる）。
    pub in_place: bool,
}

impl Patch {
    pub fn apply(&self, b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.head.len() + (self.tail_from - self.head_skip) + self.tail.len());
        out.extend_from_slice(&self.head);
        out.extend_from_slice(&b[self.head_skip..self.tail_from]);
        out.extend_from_slice(&self.tail);
        out
    }

    fn whole(old_len: usize, new: Vec<u8>) -> Patch {
        Patch { head_skip: old_len, head: new, tail_from: old_len, tail: Vec::new(), in_place: false }
    }
}

/// ヘッダーから読める音声の情報。
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub format: Format,
    pub seconds: Option<f64>,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bits: Option<u8>,
    /// 長さ・ビットレートが概算（Xing / VBRI ヘッダーのない MP3）。
    pub approx: bool,
}

impl Info {
    fn empty(format: Format) -> Info {
        Info { format, seconds: None, bitrate_kbps: None, sample_rate: None, channels: None, bits: None, approx: false }
    }
}

fn u32be(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn trim_nul(b: &[u8]) -> &[u8] {
    let mut n = b.len();
    while n > 0 && b[n - 1] == 0 {
        n -= 1;
    }
    &b[..n]
}

// =====================================================================
// 公開 API
// =====================================================================

/// 情報（長さ・ビットレートなど）。読めない部分は None。
pub fn info(b: &[u8]) -> Result<Info, String> {
    match format_of(b) {
        Some(Format::Mp3) => Ok(mp3_info(b)),
        Some(Format::Flac) => flac_info(b),
        Some(Format::Wav) => wav_info(b),
        None => Err(crate::lang::tr("MP3 / FLAC / WAV ではありません", "Not an MP3 / FLAC / WAV file").into()),
    }
}

/// メタデータを `(キー, 値)` の一覧で返す。
pub fn items(b: &[u8]) -> Result<(Format, Vec<(String, String)>), String> {
    match format_of(b) {
        Some(Format::Mp3) => Ok((Format::Mp3, mp3_items(b)?)),
        Some(Format::Flac) => Ok((Format::Flac, flac_items(b)?)),
        Some(Format::Wav) => Ok((Format::Wav, wav_items(b)?)),
        None => Err(crate::lang::tr("MP3 / FLAC / WAV ではありません", "Not an MP3 / FLAC / WAV file").into()),
    }
}

const HELP_MP3: &str = "# MP3 のタグ（ID3）。1 行 1 項目の `キー=値`。保存すると音声に書き戻す。\n# 使えるキー: title, artist, album, year, track, genre, comment（行を消すとその項目を消す）。\n# 値の改行は \\n と書く。画像・歌詞・その他のフレーム、音声の部分は触らない。\n";
const HELP_FLAC: &str = "# FLAC の Vorbis コメント。1 行 1 項目の `キー=値`（TITLE, ARTIST, ALBUM, DATE, TRACKNUMBER, GENRE など。同じキーを複数書いてよい）。\n# 保存すると音声に書き戻す。値の改行は \\n と書く。画像・他のブロック、音声の部分は触らない。\n";
const HELP_WAV: &str = "# WAV の LIST/INFO。1 行 1 項目の `キー=値`。保存すると音声に書き戻す。\n# 使えるキー: title, artist, album, date, genre, comment, track, software, copyright（ほかは 4 文字の ID をそのまま書く）。\n# 値の改行は \\n と書く。音声の部分は触らない。\n";

fn format_lines(items: &[(String, String)]) -> String {
    items.iter().map(|(k, v)| format!("{k}={}\n", escape(v))).collect()
}

/// 編集用の文字を作る。
pub fn read(b: &[u8]) -> Result<(Format, String), String> {
    let (f, it) = items(b)?;
    let help = match f {
        Format::Mp3 => HELP_MP3,
        Format::Flac => HELP_FLAC,
        Format::Wav => HELP_WAV,
    };
    Ok((f, format!("{help}{}", format_lines(&it))))
}

/// 編集した文字を、書き戻す差分にする（メタデータ以外はそのまま）。
pub fn write(b: &[u8], text: &str) -> Result<Patch, String> {
    let items = parse_lines(text)?;
    match format_of(b) {
        Some(Format::Mp3) => write_mp3(b, &items),
        Some(Format::Flac) => write_flac(b, &items),
        Some(Format::Wav) => write_wav(b, &items),
        None => Err(crate::lang::tr("MP3 / FLAC / WAV ではありません", "Not an MP3 / FLAC / WAV file").into()),
    }
}

// =====================================================================
// MP3（ID3v2 / ID3v1）
// =====================================================================

const MP3_KEYS: [&str; 7] = ["title", "artist", "album", "year", "track", "genre", "comment"];

#[derive(Clone)]
struct Frame {
    id: [u8; 4],
    flags: [u8; 2],
    data: Vec<u8>,
}

struct V2 {
    ver: u8,
    /// タグ全体の大きさ（ヘッダー 10 + 本体 + あればフッター 10）。音声はこの直後から。
    region: usize,
    frames: Vec<Frame>,
    /// フレームを最後まで（パディングまで）読めた。読めないものがあるときは書き換えない。
    clean: bool,
}

fn synchsafe(b: &[u8]) -> Option<usize> {
    if b.len() < 4 || b[..4].iter().any(|&c| c & 0x80 != 0) {
        return None;
    }
    Some(((b[0] as usize) << 21) | ((b[1] as usize) << 14) | ((b[2] as usize) << 7) | b[3] as usize)
}

fn synchsafe_enc(n: usize) -> Result<[u8; 4], String> {
    if n >= 1 << 28 {
        return Err(crate::lang::tr("タグが大きすぎます", "Tag is too large").into());
    }
    Ok([(n >> 21) as u8 & 0x7F, (n >> 14) as u8 & 0x7F, (n >> 7) as u8 & 0x7F, n as u8 & 0x7F])
}

fn parse_v2(b: &[u8]) -> Result<Option<V2>, String> {
    if b.len() < 10 || &b[..3] != b"ID3" {
        return Ok(None);
    }
    let ver = b[3];
    let flags = b[5];
    let size = synchsafe(&b[6..10]).ok_or(crate::lang::tr("ID3 のヘッダーが壊れています", "Broken ID3 header"))?;
    let region = 10 + size + if ver == 4 && flags & 0x10 != 0 { 10 } else { 0 };
    if 10 + size > b.len() {
        return Err(crate::lang::tr("ID3 タグが途中で切れています", "ID3 tag is truncated").into());
    }
    if ver != 3 && ver != 4 {
        return Ok(Some(V2 { ver, region: region.min(b.len()), frames: Vec::new(), clean: false }));
    }
    let raw = &b[10..10 + size];
    let body: Vec<u8> = if flags & 0x80 != 0 {
        // タグ全体の非同期化（FF 00 → FF）を戻す
        let mut o = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            o.push(raw[i]);
            if raw[i] == 0xFF && i + 1 < raw.len() && raw[i + 1] == 0 {
                i += 1;
            }
            i += 1;
        }
        o
    } else {
        raw.to_vec()
    };
    let mut i = 0;
    if flags & 0x40 != 0 {
        // 拡張ヘッダーは読み飛ばす（書き直すときは付けない）
        if body.len() < 4 {
            return Err(crate::lang::tr("ID3 の拡張ヘッダーが壊れています", "Broken ID3 extended header").into());
        }
        i = if ver == 4 { synchsafe(&body[..4]).ok_or(crate::lang::tr("ID3 の拡張ヘッダーが壊れています", "Broken ID3 extended header"))? } else { 4 + u32be(&body) as usize };
        if i > body.len() {
            return Err(crate::lang::tr("ID3 の拡張ヘッダーが壊れています", "Broken ID3 extended header").into());
        }
    }
    let mut frames = Vec::new();
    let mut clean = true;
    while i + 10 <= body.len() {
        if body[i] == 0 {
            break;
        }
        let id = [body[i], body[i + 1], body[i + 2], body[i + 3]];
        if !id.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            clean = false;
            break;
        }
        let sz = if ver == 4 { synchsafe(&body[i + 4..i + 8]) } else { Some(u32be(&body[i + 4..i + 8]) as usize) };
        let Some(sz) = sz else {
            clean = false;
            break;
        };
        if i + 10 + sz > body.len() {
            clean = false;
            break;
        }
        frames.push(Frame { id, flags: [body[i + 8], body[i + 9]], data: body[i + 10..i + 10 + sz].to_vec() });
        i += 10 + sz;
    }
    if clean && body[i.min(body.len())..].iter().any(|&c| c != 0) {
        clean = false;
    }
    Ok(Some(V2 { ver, region, frames, clean }))
}

/// 文字コード付きの文字列をデコードする（NUL 区切りの複数値は `/` でつなぐ）。
fn decode_text(enc: u8, b: &[u8]) -> Option<String> {
    let s = match enc {
        0 => latin1(b),
        3 => String::from_utf8_lossy(b).into_owned(),
        1 | 2 => {
            let (big, body) = if b.starts_with(&[0xFF, 0xFE]) {
                (false, &b[2..])
            } else if b.starts_with(&[0xFE, 0xFF]) {
                (true, &b[2..])
            } else {
                (enc == 2, b)
            };
            let units: Vec<u16> =
                body.chunks_exact(2).map(|c| if big { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
            String::from_utf16_lossy(&units)
        }
        _ => return None,
    };
    let parts: Vec<&str> = s.split('\0').filter(|p| !p.is_empty()).collect();
    Some(parts.join("/"))
}

fn text_frame_value(data: &[u8]) -> Option<String> {
    let (&enc, rest) = data.split_first()?;
    decode_text(enc, rest)
}

/// COMM: 文字コード・言語 3 文字・説明（終端つき）・本文。
fn comm_parse(data: &[u8]) -> Option<([u8; 3], String, String)> {
    if data.len() < 4 {
        return None;
    }
    let enc = data[0];
    let lang = [data[1], data[2], data[3]];
    let rest = &data[4..];
    let wide = enc == 1 || enc == 2;
    let (t, tl) = if wide {
        let mut i = 0;
        loop {
            if i + 2 > rest.len() {
                return None;
            }
            if rest[i] == 0 && rest[i + 1] == 0 {
                break (i, 2);
            }
            i += 2;
        }
    } else {
        (rest.iter().position(|&c| c == 0)?, 1)
    };
    let desc = decode_text(enc, &rest[..t])?;
    let text = decode_text(enc, &rest[t + tl..])?;
    Some((lang, desc, text))
}

/// 編集の対象になるフレームか。対象なら、キーと値。
/// 圧縮・暗号化・グループなどのフラグがあるもの、説明つきの COMM、読めないものは対象外（そのまま残す）。
fn managed(f: &Frame) -> Option<(&'static str, String)> {
    if f.flags != [0, 0] {
        return None;
    }
    match &f.id {
        b"TIT2" => text_frame_value(&f.data).map(|v| ("title", v)),
        b"TPE1" => text_frame_value(&f.data).map(|v| ("artist", v)),
        b"TALB" => text_frame_value(&f.data).map(|v| ("album", v)),
        b"TYER" | b"TDRC" => text_frame_value(&f.data).map(|v| ("year", v)),
        b"TRCK" => text_frame_value(&f.data).map(|v| ("track", v)),
        b"TCON" => text_frame_value(&f.data).map(|v| ("genre", v)),
        b"COMM" => comm_parse(&f.data).filter(|(_, d, _)| d.is_empty()).map(|(_, _, t)| ("comment", t)),
        _ => None,
    }
}

const GENRES: [&str; 80] = [
    "Blues", "Classic Rock", "Country", "Dance", "Disco", "Funk", "Grunge", "Hip-Hop", "Jazz", "Metal", "New Age", "Oldies", "Other", "Pop", "R&B", "Rap",
    "Reggae", "Rock", "Techno", "Industrial", "Alternative", "Ska", "Death Metal", "Pranks", "Soundtrack", "Euro-Techno", "Ambient", "Trip-Hop", "Vocal",
    "Jazz+Funk", "Fusion", "Trance", "Classical", "Instrumental", "Acid", "House", "Game", "Sound Clip", "Gospel", "Noise", "AlternRock", "Bass", "Soul",
    "Punk", "Space", "Meditative", "Instrumental Pop", "Instrumental Rock", "Ethnic", "Gothic", "Darkwave", "Techno-Industrial", "Electronic", "Pop-Folk",
    "Eurodance", "Dream", "Southern Rock", "Comedy", "Cult", "Gangsta", "Top 40", "Christian Rap", "Pop/Funk", "Jungle", "Native American", "Cabaret",
    "New Wave", "Psychadelic", "Rave", "Showtunes", "Trailer", "Lo-Fi", "Tribal", "Acid Punk", "Acid Jazz", "Polka", "Retro", "Musical", "Rock & Roll",
    "Hard Rock",
];

fn v1_tag(b: &[u8]) -> Option<&[u8]> {
    (b.len() >= 128 && &b[b.len() - 128..b.len() - 125] == b"TAG").then(|| &b[b.len() - 128..])
}

fn v1_field(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    latin1(&b[..end]).trim_end().to_string()
}

fn v1_items(t: &[u8]) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut push = |k: &str, s: String| {
        if !s.is_empty() {
            v.push((k.to_string(), s));
        }
    };
    push("title", v1_field(&t[3..33]));
    push("artist", v1_field(&t[33..63]));
    push("album", v1_field(&t[63..93]));
    push("year", v1_field(&t[93..97]));
    let has_track = t[125] == 0 && t[126] != 0;
    push("comment", v1_field(&t[97..if has_track { 125 } else { 127 }]));
    if has_track {
        push("track", t[126].to_string());
    }
    if let Some(g) = GENRES.get(t[127] as usize) {
        push("genre", g.to_string());
    }
    v
}

fn mp3_items(b: &[u8]) -> Result<Vec<(String, String)>, String> {
    let from_v2: Vec<(String, String)> = match parse_v2(b)? {
        Some(v) if v.ver == 3 || v.ver == 4 => v.frames.iter().filter_map(managed).map(|(k, s)| (k.to_string(), s)).collect(),
        _ => Vec::new(),
    };
    if !from_v2.is_empty() {
        return Ok(from_v2);
    }
    Ok(v1_tag(b).map(v1_items).unwrap_or_default())
}

fn emit_frame(ver: u8, f: &Frame, out: &mut Vec<u8>) -> Result<(), String> {
    out.extend_from_slice(&f.id);
    if ver == 4 {
        out.extend_from_slice(&synchsafe_enc(f.data.len())?);
    } else {
        out.extend_from_slice(&(f.data.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(&f.flags);
    out.extend_from_slice(&f.data);
    Ok(())
}

/// 文字コードの番号と、その文字コードで書いた値。ASCII は ISO-8859-1、それ以外は v2.4 なら UTF-8、v2.3 なら UTF-16（BOM つき）。
fn enc_text(ver: u8, s: &str) -> (u8, Vec<u8>) {
    if s.is_ascii() {
        (0, s.as_bytes().to_vec())
    } else if ver == 4 {
        (3, s.as_bytes().to_vec())
    } else {
        let mut o = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            o.extend_from_slice(&u.to_le_bytes());
        }
        (1, o)
    }
}

fn text_frame(ver: u8, id: &[u8; 4], s: &str) -> Frame {
    let (enc, bytes) = enc_text(ver, s);
    let mut data = vec![enc];
    data.extend_from_slice(&bytes);
    Frame { id: *id, flags: [0, 0], data }
}

fn comm_frame(ver: u8, lang: [u8; 3], s: &str) -> Frame {
    let (enc, bytes) = enc_text(ver, s);
    let mut data = vec![enc];
    data.extend_from_slice(&lang);
    data.extend_from_slice(if enc == 1 { &[0xFF, 0xFE, 0, 0] } else { &[0] }); // 空の説明
    data.extend_from_slice(&bytes);
    Frame { id: *b"COMM", flags: [0, 0], data }
}

fn latin1_fixed(s: &str, n: usize, out: &mut [u8]) {
    for (i, c) in s.chars().take(n).enumerate() {
        out[i] = if (c as u32) < 256 { c as u8 } else { b'?' };
    }
}

fn build_v1(m: &[(String, String)]) -> [u8; 128] {
    let get = |k: &str| m.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str()).unwrap_or("");
    let mut t = [0u8; 128];
    t[..3].copy_from_slice(b"TAG");
    latin1_fixed(get("title"), 30, &mut t[3..33]);
    latin1_fixed(get("artist"), 30, &mut t[33..63]);
    latin1_fixed(get("album"), 30, &mut t[63..93]);
    latin1_fixed(get("year"), 4, &mut t[93..97]);
    let track: u8 = get("track").split('/').next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    if track != 0 {
        latin1_fixed(get("comment"), 28, &mut t[97..125]);
        t[126] = track;
    } else {
        latin1_fixed(get("comment"), 30, &mut t[97..127]);
    }
    t[127] = GENRES.iter().position(|g| g.eq_ignore_ascii_case(get("genre"))).map(|i| i as u8).unwrap_or(255);
    t
}

fn write_mp3(b: &[u8], items: &[(String, String)]) -> Result<Patch, String> {
    let mut values: Vec<(String, String)> = Vec::new();
    for (k, v) in items {
        let key = k.to_lowercase();
        if !MP3_KEYS.contains(&key.as_str()) {
            return Err(if crate::lang::is_en() { format!("`{k}` cannot be used in MP3 (usable keys: {})", MP3_KEYS.join(", ")) } else { format!("`{k}` は MP3 では使えないキーです（{}）", MP3_KEYS.join(", ")) });
        }
        if values.iter().any(|(x, _)| *x == key) {
            return Err(if crate::lang::is_en() { format!("`{key}` appears more than once") } else { format!("`{key}` が 2 回以上あります") });
        }
        values.push((key, v.clone()));
    }
    let old = parse_v2(b)?;
    let (ver, region, mut frames, lang) = match &old {
        Some(v) => {
            if v.ver != 3 && v.ver != 4 {
                return Err(if crate::lang::is_en() { format!("ID3v2.{} tags cannot be edited (v2.3 / v2.4 only)", v.ver) } else { format!("ID3v2.{} のタグは編集できません（v2.3 / v2.4 のみ）", v.ver) });
            }
            if !v.clean {
                return Err(crate::lang::tr("ID3 タグに読めない部分があるため、壊さないよう編集しません", "Not editing, because the ID3 tag has unreadable parts and editing could damage it").into());
            }
            let lang = v.frames.iter().find(|f| managed(f).is_some_and(|(k, _)| k == "comment")).and_then(|f| comm_parse(&f.data)).map(|(l, _, _)| l);
            (v.ver, v.region, v.frames.iter().filter(|f| managed(f).is_none()).cloned().collect::<Vec<_>>(), lang.unwrap_or(*b"eng"))
        }
        None => (3, 0, Vec::new(), *b"eng"),
    };
    // TYER / TDRC は、読めればキー year として編集するので、残す側には入っていない（書くときは版に合う方を 1 つだけ作る）
    for (k, v) in values.iter().filter(|(_, v)| !v.is_empty()) {
        match k.as_str() {
            "title" => frames.push(text_frame(ver, b"TIT2", v)),
            "artist" => frames.push(text_frame(ver, b"TPE1", v)),
            "album" => frames.push(text_frame(ver, b"TALB", v)),
            "year" => {
                if ver == 4 {
                    frames.push(text_frame(ver, b"TDRC", v));
                } else {
                    if v.len() != 4 || !v.bytes().all(|c| c.is_ascii_digit()) {
                        return Err(crate::lang::tr("year は 4 桁の数字で書いてください（ID3v2.3）", "Write the year as 4 digits (ID3v2.3)").into());
                    }
                    frames.push(text_frame(ver, b"TYER", v));
                }
            }
            "track" => frames.push(text_frame(ver, b"TRCK", v)),
            "genre" => frames.push(text_frame(ver, b"TCON", v)),
            _ => frames.push(comm_frame(ver, lang, v)),
        }
    }
    let mut body = Vec::new();
    for f in &frames {
        emit_frame(ver, f, &mut body)?;
    }
    let had_tag = old.is_some();
    let (head, in_place) = if body.is_empty() && !had_tag {
        (Vec::new(), false)
    } else {
        let (pad, in_place) = if had_tag && 10 + body.len() <= region { (region - 10 - body.len(), true) } else { (1024, false) };
        let mut h = Vec::with_capacity(10 + body.len() + pad);
        h.extend_from_slice(b"ID3");
        h.push(ver);
        h.push(0);
        h.push(0);
        h.extend_from_slice(&synchsafe_enc(body.len() + pad)?);
        h.extend_from_slice(&body);
        h.resize(h.len() + pad, 0);
        (h, in_place)
    };
    let (tail_from, tail) = match v1_tag(b) {
        Some(_) => (b.len() - 128, build_v1(&values).to_vec()),
        None => (b.len(), Vec::new()),
    };
    if region > tail_from {
        return Err(crate::lang::tr("ID3 タグと ID3v1 が重なっています", "ID3 tag overlaps ID3v1").into());
    }
    Ok(Patch { head_skip: region, head, tail_from, tail, in_place })
}

fn mp3_info(b: &[u8]) -> Info {
    let mut info = Info::empty(Format::Mp3);
    let start = match parse_v2(b) {
        Ok(Some(v)) => v.region.min(b.len()),
        _ => 0,
    };
    let end = b.len() - if v1_tag(b).is_some() { 128 } else { 0 };
    let audio_bytes = end.saturating_sub(start) as f64;
    let Some((pos, h)) = mp3_find_frame(b, start) else { return info };
    info.sample_rate = Some(h.rate);
    info.channels = Some(h.channels);
    let spf = if h.mpeg1 { 1152.0 } else { 576.0 };
    let side = match (h.mpeg1, h.channels) {
        (true, 1) => 17,
        (true, _) => 32,
        (false, 1) => 9,
        (false, _) => 17,
    };
    let x = pos + 4 + if h.crc { 2 } else { 0 } + side;
    let mut frames: Option<u32> = None;
    if b.len() >= x + 12 && (&b[x..x + 4] == b"Xing" || &b[x..x + 4] == b"Info") {
        let flags = u32be(&b[x + 4..]);
        if flags & 1 != 0 {
            frames = Some(u32be(&b[x + 8..]));
        }
    } else if b.len() >= pos + 36 + 18 && &b[pos + 36..pos + 40] == b"VBRI" {
        frames = Some(u32be(&b[pos + 36 + 14..]));
    }
    match frames {
        Some(n) if n > 0 => {
            let secs = n as f64 * spf / h.rate as f64;
            info.seconds = Some(secs);
            info.bitrate_kbps = Some((audio_bytes * 8.0 / secs / 1000.0).round() as u32);
        }
        _ => {
            info.bitrate_kbps = Some(h.bitrate);
            info.seconds = Some(audio_bytes * 8.0 / (h.bitrate as f64 * 1000.0));
            info.approx = true;
        }
    }
    info
}

struct Hdr {
    mpeg1: bool,
    bitrate: u32,
    rate: u32,
    channels: u8,
    crc: bool,
    len: usize,
}

/// MPEG Layer III のフレームヘッダー（4 バイト）。それ以外は None。
fn mp3_header(h: &[u8]) -> Option<Hdr> {
    if h.len() < 4 || h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let ver = (h[1] >> 3) & 3;
    if ver == 1 || (h[1] >> 1) & 3 != 1 {
        return None;
    }
    let bi = (h[2] >> 4) as usize;
    let si = ((h[2] >> 2) & 3) as usize;
    if bi == 0 || bi == 15 || si == 3 {
        return None;
    }
    const BR1: [u32; 14] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const BR2: [u32; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let mpeg1 = ver == 3;
    let bitrate = if mpeg1 { BR1[bi - 1] } else { BR2[bi - 1] };
    let rate = match ver {
        3 => [44100, 48000, 32000][si],
        2 => [22050, 24000, 16000][si],
        _ => [11025, 12000, 8000][si],
    };
    let pad = ((h[2] >> 1) & 1) as usize;
    let len = (if mpeg1 { 144 } else { 72 }) * bitrate as usize * 1000 / rate as usize + pad;
    Some(Hdr { mpeg1, bitrate, rate, channels: if h[3] >> 6 == 3 { 1 } else { 2 }, crc: h[1] & 1 == 0, len })
}

/// `start` 以降で最初のフレームを探す（偶然の一致を避けるため、次のフレームの同期も確かめる）。
fn mp3_find_frame(b: &[u8], start: usize) -> Option<(usize, Hdr)> {
    let stop = b.len().min(start + 64 * 1024);
    let mut i = start;
    while i + 4 <= stop {
        if b[i] == 0xFF {
            if let Some(h) = mp3_header(&b[i..]) {
                let next = i + h.len;
                if next + 4 > b.len() || mp3_header(&b[next..]).is_some() {
                    return Some((i, h));
                }
            }
        }
        i += 1;
    }
    None
}

// =====================================================================
// FLAC（Vorbis コメント）
// =====================================================================

fn flac_blocks(b: &[u8]) -> Result<(Vec<(u8, &[u8])>, usize), String> {
    if !b.starts_with(b"fLaC") {
        return Err(crate::lang::tr("FLAC ではありません", "Not a FLAC file").into());
    }
    let mut pos = 4;
    let mut v = Vec::new();
    loop {
        if pos + 4 > b.len() {
            return Err(crate::lang::tr("FLAC のメタデータが途中で切れています", "FLAC metadata is truncated").into());
        }
        let h = b[pos];
        let len = ((b[pos + 1] as usize) << 16) | ((b[pos + 2] as usize) << 8) | b[pos + 3] as usize;
        if pos + 4 + len > b.len() {
            return Err(crate::lang::tr("FLAC のメタデータが途中で切れています", "FLAC metadata is truncated").into());
        }
        v.push((h & 0x7F, &b[pos + 4..pos + 4 + len]));
        pos += 4 + len;
        if h & 0x80 != 0 {
            break;
        }
    }
    Ok((v, pos))
}

/// Vorbis コメントのブロック本体 → (ベンダー, 各コメント)。
fn vorbis_parse(d: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
    let vl = u32le(d.get(..4)?) as usize;
    let vendor = d.get(4..4 + vl)?.to_vec();
    let mut p = 4 + vl;
    let n = u32le(d.get(p..p + 4)?) as usize;
    p += 4;
    let mut v = Vec::new();
    for _ in 0..n {
        let l = u32le(d.get(p..p + 4)?) as usize;
        p += 4;
        v.push(d.get(p..p + l)?.to_vec());
        p += l;
    }
    Some((vendor, v))
}

fn flac_items(b: &[u8]) -> Result<Vec<(String, String)>, String> {
    let (blocks, _) = flac_blocks(b)?;
    let mut out = Vec::new();
    for (_, d) in blocks.iter().filter(|(t, _)| *t == 4) {
        let (_, cs) = vorbis_parse(d).ok_or(crate::lang::tr("FLAC の Vorbis コメントが壊れています", "Broken FLAC Vorbis comment"))?;
        for c in cs {
            let s = String::from_utf8_lossy(&c).into_owned();
            if let Some((k, v)) = s.split_once('=') {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    Ok(out)
}

fn flac_info(b: &[u8]) -> Result<Info, String> {
    let (blocks, meta_end) = flac_blocks(b)?;
    let mut info = Info::empty(Format::Flac);
    if let Some((_, d)) = blocks.iter().find(|(t, _)| *t == 0).filter(|(_, d)| d.len() >= 18) {
        let rate = ((d[10] as u32) << 12) | ((d[11] as u32) << 4) | (d[12] as u32 >> 4);
        let total = (((d[13] & 0x0F) as u64) << 32) | u32be(&d[14..]) as u64;
        info.sample_rate = Some(rate);
        info.channels = Some(((d[12] >> 1) & 7) + 1);
        info.bits = Some((((d[12] & 1) << 4) | (d[13] >> 4)) + 1);
        if rate > 0 && total > 0 {
            let secs = total as f64 / rate as f64;
            info.seconds = Some(secs);
            info.bitrate_kbps = Some(((b.len() - meta_end) as f64 * 8.0 / secs / 1000.0).round() as u32);
        }
    }
    Ok(info)
}

fn write_flac(b: &[u8], items: &[(String, String)]) -> Result<Patch, String> {
    let (blocks, meta_end) = flac_blocks(b)?;
    let mut comments: Vec<Vec<u8>> = Vec::new();
    for (k, v) in items {
        if k.is_empty() || !k.bytes().all(|c| (0x20..=0x7D).contains(&c)) {
            return Err(if crate::lang::is_en() { format!("`{k}` cannot be used as a Vorbis comment key (ASCII letters, digits and symbols, except `=`)") } else { format!("`{k}` は Vorbis コメントのキーに使えません（半角の英数字・記号、`=` 以外）") });
        }
        comments.push(format!("{k}={v}").into_bytes());
    }
    let vendor = blocks.iter().find(|(t, _)| *t == 4).and_then(|(_, d)| vorbis_parse(d)).map(|(v, _)| v).unwrap_or_else(|| b"NeoNote".to_vec());
    let mut vc = Vec::new();
    vc.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    vc.extend_from_slice(&vendor);
    vc.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in &comments {
        vc.extend_from_slice(&(c.len() as u32).to_le_bytes());
        vc.extend_from_slice(c);
    }
    // 残すブロック（パディングは作り直す）。Vorbis コメントは最初にあった位置（無ければ STREAMINFO の次）へ
    let mut list: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut placed = false;
    for (t, d) in &blocks {
        match *t {
            1 => {}
            4 => {
                if !placed {
                    list.push((4, vc.clone()));
                    placed = true;
                }
            }
            t => list.push((t, d.to_vec())),
        }
    }
    if !placed {
        let at = list.len().min(1);
        list.insert(at, (4, vc));
    }
    if list.iter().any(|(_, d)| d.len() >= 1 << 24) {
        return Err(crate::lang::tr("FLAC のブロックが大きすぎます", "FLAC block is too large").into());
    }
    let need: usize = list.iter().map(|(_, d)| 4 + d.len()).sum();
    let old_total = meta_end - 4;
    let (pad_total, in_place) = if need == old_total {
        (0, true)
    } else if need + 4 <= old_total {
        (old_total - need, true) // 余りをパディングのブロックにする（4 バイト以上ある）
    } else {
        (4 + 2048, false)
    };
    if pad_total > 0 {
        list.push((1, vec![0; pad_total - 4]));
    }
    let mut head = b"fLaC".to_vec();
    let n = list.len();
    for (i, (t, d)) in list.iter().enumerate() {
        head.push(*t | if i + 1 == n { 0x80 } else { 0 });
        head.extend_from_slice(&(d.len() as u32).to_be_bytes()[1..]);
        head.extend_from_slice(d);
    }
    Ok(Patch { head_skip: meta_end, in_place: in_place && head.len() == meta_end, head, tail_from: b.len(), tail: Vec::new() })
}

// =====================================================================
// WAV（LIST/INFO）
// =====================================================================

struct Chunk<'a> {
    id: [u8; 4],
    data: &'a [u8],
    /// 宣言した大きさが、ファイルの残りより大きかった（途中で切れている）。
    cut: bool,
}

/// RIFF のチャンクを順に読む。戻り値は (チャンク, RIFF の終わり)。
fn wav_chunks(b: &[u8]) -> Result<(Vec<Chunk<'_>>, usize), String> {
    if format_of(b) != Some(Format::Wav) {
        return Err(crate::lang::tr("WAV ではありません", "Not a WAV file").into());
    }
    let declared = 8 + u32le(&b[4..]) as usize;
    let end = declared.min(b.len());
    let mut pos = 12;
    let mut v = Vec::new();
    while pos + 8 <= end {
        let id = [b[pos], b[pos + 1], b[pos + 2], b[pos + 3]];
        let size = u32le(&b[pos + 4..]) as usize;
        let data_end = pos + 8 + size;
        if data_end > end {
            v.push(Chunk { id, data: &b[pos + 8..end], cut: true });
            break;
        }
        v.push(Chunk { id, data: &b[pos + 8..data_end], cut: false });
        pos = data_end + (size & 1);
    }
    Ok((v, end))
}

const WAV_KEYS: [(&str, &[u8; 4]); 9] = [
    ("title", b"INAM"),
    ("artist", b"IART"),
    ("album", b"IPRD"),
    ("date", b"ICRD"),
    ("genre", b"IGNR"),
    ("comment", b"ICMT"),
    ("track", b"ITRK"),
    ("software", b"ISFT"),
    ("copyright", b"ICOP"),
];

fn wav_key_name(id: &[u8; 4]) -> String {
    WAV_KEYS.iter().find(|(_, i)| *i == id).map(|(k, _)| k.to_string()).unwrap_or_else(|| latin1(id))
}

fn wav_key_id(k: &str) -> Result<[u8; 4], String> {
    if let Some((_, id)) = WAV_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)) {
        return Ok(**id);
    }
    if k.len() == 4 && k.bytes().all(|c| (0x20..0x7F).contains(&c)) {
        let b = k.as_bytes();
        return Ok([b[0], b[1], b[2], b[3]]);
    }
    Err(if crate::lang::is_en() { format!("`{k}` cannot be used in WAV (usable keys: {}, or a 4-character ID)", WAV_KEYS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")) } else { format!("`{k}` は WAV では使えないキーです（{}、または 4 文字の ID）", WAV_KEYS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")) })
}

fn is_info_list(c: &Chunk<'_>) -> bool {
    &c.id == b"LIST" && c.data.starts_with(b"INFO")
}

fn wav_items(b: &[u8]) -> Result<Vec<(String, String)>, String> {
    let (chunks, _) = wav_chunks(b)?;
    let mut out = Vec::new();
    for c in chunks.iter().filter(|c| is_info_list(c)) {
        let d = c.data;
        let mut p = 4;
        while p + 8 <= d.len() {
            let id = [d[p], d[p + 1], d[p + 2], d[p + 3]];
            let n = u32le(&d[p + 4..]) as usize;
            let e = (p + 8 + n).min(d.len());
            let raw = trim_nul(&d[p + 8..e]);
            let val = match std::str::from_utf8(raw) {
                Ok(s) => s.to_string(),
                Err(_) => latin1(raw),
            };
            out.push((wav_key_name(&id), val));
            p = p + 8 + n + (n & 1);
        }
    }
    Ok(out)
}

fn wav_info(b: &[u8]) -> Result<Info, String> {
    let (chunks, _) = wav_chunks(b)?;
    let mut info = Info::empty(Format::Wav);
    let mut byte_rate = 0u32;
    if let Some(c) = chunks.iter().find(|c| &c.id == b"fmt ").filter(|c| c.data.len() >= 16) {
        let d = c.data;
        info.channels = Some(u16::from_le_bytes([d[2], d[3]]).min(255) as u8);
        info.sample_rate = Some(u32le(&d[4..]));
        byte_rate = u32le(&d[8..]);
        info.bits = Some(u16::from_le_bytes([d[14], d[15]]).min(255) as u8);
        info.bitrate_kbps = Some((byte_rate as u64 * 8 / 1000) as u32);
    }
    if let Some(c) = chunks.iter().find(|c| &c.id == b"data") {
        if byte_rate > 0 {
            info.seconds = Some(c.data.len() as f64 / byte_rate as f64);
        }
    }
    Ok(info)
}

fn write_wav(b: &[u8], items: &[(String, String)]) -> Result<Patch, String> {
    let (chunks, end) = wav_chunks(b)?;
    if chunks.iter().any(|c| c.cut) || 8 + u32le(&b[4..]) as usize > b.len() {
        return Err(crate::lang::tr("WAV が途中で切れている（または大きさの記録が正しくない）ため、壊さないよう編集しません", "Not editing, because the WAV is truncated (or its size record is wrong) and editing could damage it").into());
    }
    let mut info = b"INFO".to_vec();
    for (k, v) in items.iter().filter(|(_, v)| !v.is_empty()) {
        let id = wav_key_id(k)?;
        let mut val = v.as_bytes().to_vec();
        val.push(0);
        info.extend_from_slice(&id);
        info.extend_from_slice(&(val.len() as u32).to_le_bytes());
        info.extend_from_slice(&val);
        if val.len() & 1 == 1 {
            info.push(0);
        }
    }
    let has_items = info.len() > 4;
    let mut body = b"WAVE".to_vec();
    let put = |id: &[u8; 4], data: &[u8], body: &mut Vec<u8>| {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() & 1 == 1 {
            body.push(0);
        }
    };
    let mut placed = false;
    for c in &chunks {
        if is_info_list(c) {
            if has_items && !placed {
                put(b"LIST", &info, &mut body);
                placed = true;
            }
        } else {
            put(&c.id, c.data, &mut body);
        }
    }
    if has_items && !placed {
        put(b"LIST", &info, &mut body);
    }
    if body.len() > u32::MAX as usize - 8 {
        return Err(crate::lang::tr("WAV が大きすぎます", "WAV is too large").into());
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&b[end..]); // RIFF の後ろにあったものはそのまま
    Ok(Patch::whole(b.len(), out))
}

// =====================================================================
// Preview の HTML
// =====================================================================

fn escaped(s: &str) -> String {
    let mut out = String::new();
    esc(s, &mut out);
    out
}

fn esc(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str(" "),
            c => out.push(c),
        }
    }
}

pub fn format_time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 情報の行（ラベル, 値）。
/// 右パネルに出す行: タグ（無ければ「タグ: なし」）。情報（形式・長さなど）は呼び出し側がファイルから別に出す。
pub fn side_rows(items: &[(String, String)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    if items.is_empty() {
        v.push((crate::lang::tr("タグ", "Tags").into(), crate::lang::tr("なし", "none").into()));
    }
    v.extend(items.iter().cloned());
    v
}

pub fn info_rows(i: &Info) -> Vec<(&'static str, String)> {
    let mut v = vec![("Format", i.format.label().to_string())];
    let a = if i.approx { crate::lang::tr("約 ", "about ") } else { "" };
    if let Some(s) = i.seconds {
        v.push(("Length", format!("{a}{}", format_time(s))));
    }
    if let Some(k) = i.bitrate_kbps {
        v.push(("Bitrate", format!("{a}{k} kbps")));
    }
    if let Some(r) = i.sample_rate {
        v.push(("Sample rate", format!("{r} Hz")));
    }
    if let Some(c) = i.channels {
        v.push(("Channels", match c {
            1 => "1 (mono)".to_string(),
            2 => "2 (stereo)".to_string(),
            n => n.to_string(),
        }));
    }
    if let Some(b) = i.bits {
        v.push(("Bits", b.to_string()));
    }
    v
}

/// 音声の Preview。`status` は再生の状態や操作の案内（例: `Stopped — Space: Play`）。
/// 本文の Preview の見た目（色は `#rrggbb`）。
pub struct PlayerLook<'a> {
    pub bg: &'a str,
    pub fg: &'a str,
    pub dim: &'a str,
    /// 再生ボタンと、進み具合の帯の済んだ所。
    pub accent: &'a str,
    /// 進み具合の帯の残り。
    pub track: &'a str,
}

/// 本文の Preview: 簡単な音楽プレイヤー（曲名・アーティストとアルバム・再生ボタン・進み具合・時間・案内）。
/// 情報とタグの一覧は右パネル（`side_rows`）に出す。読めなかった時は理由も出す。
/// `pos` は今の位置（秒。鳴らしていなければ None）、`note` はボタンの下の案内。
pub fn page_html(name: &str, info: &Result<Info, String>, items: &[(String, String)], playing: bool, pos: Option<f64>, note: &str, look: &PlayerLook) -> String {
    let PlayerLook { bg, fg, dim, accent, track } = *look;
    let tag = |k: &str| items.iter().find(|(key, v)| key.eq_ignore_ascii_case(k) && !v.trim().is_empty()).map(|(_, v)| v.trim().to_string());
    let title = tag("title").unwrap_or_else(|| name.to_string());
    let sub: Vec<String> = [tag("artist"), tag("album")].into_iter().flatten().collect();
    let total = info.as_ref().ok().and_then(|i| i.seconds);
    // HelpView は表の入れ子を扱えない（行が崩れ、2 つ以上あると固まる）。表は進み具合の帯の 1 つだけにする
    let center = |inner: &str| format!("<p align=\"center\">{inner}</p>");
    let mut head = format!("<br><font size=\"7\">{}</font>", escaped(&title));
    if !sub.is_empty() {
        head.push_str(&format!("<br><font color=\"{dim}\">{}</font>", escaped(&sub.join(" — "))));
    }
    if title != name {
        head.push_str(&format!("<br><font color=\"{dim}\" size=\"2\">{}</font>", escaped(name)));
    }
    let mut body = center(&head);
    // 再生ボタン（大きな記号）
    let mark = if playing { "\u{25A0}" } else { "\u{25B6}" };
    body.push_str(&center(&format!("<font color=\"{accent}\" size=\"7\">{mark}</font>")));
    // 進み具合の帯: 左右の余白（各 15%）の間に、済んだ所と残り（合わせて 70%）
    let done = match (pos, total) {
        (Some(p), Some(t)) if t > 0.0 => ((p / t).clamp(0.0, 1.0) * 70.0).round() as u32,
        _ => 0,
    };
    let cell = |w: u32, color: Option<&str>| {
        let bg = color.map(|c| format!(" bgcolor=\"{c}\"")).unwrap_or_default();
        format!("<td width=\"{w}%\"{bg}><font size=\"1\">&nbsp;</font></td>")
    };
    body.push_str("<table width=\"100%\" border=\"0\" cellspacing=\"0\" cellpadding=\"0\"><tr>");
    body.push_str(&cell(15, None));
    if done > 0 {
        body.push_str(&cell(done, Some(accent)));
    }
    if done < 70 {
        body.push_str(&cell(70 - done, Some(track)));
    }
    body.push_str(&cell(15, None));
    body.push_str("</tr></table>");
    let time = match total {
        Some(t) => format!("{} / {}", format_time(pos.unwrap_or(0.0)), format_time(t)),
        None => format_time(pos.unwrap_or(0.0)),
    };
    body.push_str(&center(&format!("<font color=\"{dim}\">{time}</font>")));
    if !note.is_empty() {
        body.push_str(&center(&format!("<font color=\"{dim}\" size=\"2\">{}</font>", escaped(note))));
    }
    if let Err(e) = info {
        body.push_str(&center(&escaped(e)));
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

// =====================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn frame_bytes(ver: u8, id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut o = Vec::new();
        emit_frame(ver, &Frame { id: *id, flags: [0, 0], data: data.to_vec() }, &mut o).unwrap();
        o
    }

    /// ID3v2 + 偽の MP3 フレーム（CBR 128 kbps, 44.1 kHz。2 フレーム + 末尾）。
    fn mp3_with(ver: u8, frames: &[Vec<u8>], pad: usize, v1: bool) -> Vec<u8> {
        let mut body: Vec<u8> = frames.concat();
        body.resize(body.len() + pad, 0);
        let mut b = b"ID3".to_vec();
        b.extend_from_slice(&[ver, 0, 0]);
        b.extend_from_slice(&synchsafe_enc(body.len()).unwrap());
        b.extend_from_slice(&body);
        b.extend_from_slice(&audio_frames(8));
        if v1 {
            b.extend_from_slice(&build_v1(&[("title".into(), "Old".into())]));
        }
        b
    }

    fn audio_frames(n: usize) -> Vec<u8> {
        let mut a = Vec::new();
        for _ in 0..n {
            let mut f = vec![0u8; 417];
            f[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
            a.extend_from_slice(&f);
        }
        a
    }

    fn audio_part(b: &[u8], start: usize) -> &[u8] {
        &b[start..start + 417 * 8]
    }

    fn get<'a>(v: &'a [(String, String)], k: &str) -> Option<&'a str> {
        v.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn mp3_read_text_frames_and_comment() {
        let mut comm = vec![0u8];
        comm.extend_from_slice(b"eng\0hello");
        let b = mp3_with(3, &[frame_bytes(3, b"TIT2", b"\0Song"), frame_bytes(3, b"TPE1", b"\0Me"), frame_bytes(3, b"COMM", &comm)], 0, false);
        let (f, it) = items(&b).unwrap();
        assert_eq!(f, Format::Mp3);
        assert_eq!(get(&it, "title"), Some("Song"));
        assert_eq!(get(&it, "artist"), Some("Me"));
        assert_eq!(get(&it, "comment"), Some("hello"));
    }

    #[test]
    fn mp3_utf16_and_utf8_values_are_read() {
        let mut t16 = vec![1u8, 0xFF, 0xFE];
        for u in "日本語".encode_utf16() {
            t16.extend_from_slice(&u.to_le_bytes());
        }
        let b = mp3_with(3, &[frame_bytes(3, b"TIT2", &t16)], 0, false);
        assert_eq!(get(&items(&b).unwrap().1, "title"), Some("日本語"));
        let mut t8 = vec![3u8];
        t8.extend_from_slice("歌".as_bytes());
        let b = mp3_with(4, &[frame_bytes(4, b"TPE1", &t8)], 0, false);
        assert_eq!(get(&items(&b).unwrap().1, "artist"), Some("歌"));
    }

    #[test]
    fn mp3_write_keeps_other_frames_and_audio() {
        let apic = frame_bytes(3, b"APIC", &[0, 1, 2, 3, 4, 5]);
        let b = mp3_with(3, &[frame_bytes(3, b"TIT2", b"\0Old"), apic.clone()], 0, false);
        let p = write(&b, "title=新しい題名\nartist=A\nyear=1999\n").unwrap();
        let out = p.apply(&b);
        let (_, it) = items(&out).unwrap();
        assert_eq!(get(&it, "title"), Some("新しい題名"));
        assert_eq!(get(&it, "artist"), Some("A"));
        assert_eq!(get(&it, "year"), Some("1999"));
        // APIC は残る・音声は 1 バイトも変わらない
        let v = parse_v2(&out).unwrap().unwrap();
        assert!(v.clean && v.frames.iter().any(|f| &f.id == b"APIC" && f.data == [0, 1, 2, 3, 4, 5]));
        assert_eq!(audio_part(&out, v.region), &audio_frames(8)[..]);
    }

    #[test]
    fn mp3_smaller_tag_is_written_in_place_within_padding() {
        let b = mp3_with(3, &[frame_bytes(3, b"TIT2", b"\0A fairly long old title"), frame_bytes(3, b"TPE1", b"\0Artist")], 200, false);
        let p = write(&b, "title=x\n").unwrap();
        assert!(p.in_place);
        assert_eq!(p.head.len(), p.head_skip);
        let out = p.apply(&b);
        assert_eq!(out.len(), b.len());
        assert_eq!(&out[p.head_skip..], &b[p.head_skip..]);
        assert_eq!(get(&items(&out).unwrap().1, "title"), Some("x"));
        assert_eq!(get(&items(&out).unwrap().1, "artist"), None); // 行を消したので消える
    }

    #[test]
    fn mp3_bigger_tag_grows_and_is_not_in_place() {
        let b = mp3_with(4, &[frame_bytes(4, b"TIT2", b"\0a")], 0, false);
        let p = write(&b, &format!("title={}\n", "x".repeat(500))).unwrap();
        assert!(!p.in_place);
        assert_eq!(get(&items(&p.apply(&b)).unwrap().1, "title").map(|s| s.len()), Some(500));
    }

    #[test]
    fn mp3_without_tag_gets_a_new_v23_tag() {
        let b = audio_frames(8);
        let p = write(&b, "title=T\ncomment=c\\nd\n").unwrap();
        let out = p.apply(&b);
        assert_eq!(&out[..4], b"ID3\x03");
        let it = items(&out).unwrap().1;
        assert_eq!(get(&it, "title"), Some("T"));
        assert_eq!(get(&it, "comment"), Some("c\nd"));
        let v = parse_v2(&out).unwrap().unwrap();
        assert_eq!(audio_part(&out, v.region), &b[..]);
    }

    #[test]
    fn mp3_v1_only_is_read_and_updated() {
        let mut b = audio_frames(8);
        let mut t = build_v1(&[("title".into(), "Old".into()), ("artist".into(), "Who".into()), ("track".into(), "7".into())]);
        t[127] = 17;
        b.extend_from_slice(&t);
        let it = items(&b).unwrap().1;
        assert_eq!((get(&it, "title"), get(&it, "artist"), get(&it, "track"), get(&it, "genre")), (Some("Old"), Some("Who"), Some("7"), Some("Rock")));
        let out = write(&b, "title=New\ngenre=Jazz\n").unwrap().apply(&b);
        let t2 = &out[out.len() - 128..];
        assert_eq!(&t2[..3], b"TAG");
        assert_eq!(v1_field(&t2[3..33]), "New");
        assert_eq!(t2[127], 8);
        assert_eq!(get(&items(&out).unwrap().1, "title"), Some("New"));
    }

    #[test]
    fn mp3_rejects_bad_input_without_panicking() {
        assert!(write(&[], "title=x\n").is_err());
        assert!(write(b"ID3\x03\0\0\0\0\x01\0 short", "title=x\n").is_err());
        let b = mp3_with(3, &[frame_bytes(3, b"TIT2", b"\0A")], 0, false);
        assert!(write(&b, "bogus=1\n").is_err());
        assert!(write(&b, "title=a\ntitle=b\n").is_err());
        assert!(write(&b, "year=abc\n").is_err());
        assert!(write(&b, "no equals sign\n").is_err());
        let mut broken = b.clone();
        broken[10] = b'!'; // フレーム ID が壊れている
        assert!(write(&broken, "title=x\n").is_err());
        let v22 = {
            let mut v = b.clone();
            v[3] = 2;
            v
        };
        assert!(write(&v22, "title=x\n").is_err());
        for n in 0..b.len().min(80) {
            let _ = items(&b[..n]);
            let _ = info(&b[..n]);
            let _ = write(&b[..n], "title=x\n");
        }
    }

    #[test]
    fn mp3_info_cbr_and_xing() {
        let b = audio_frames(100);
        let i = info(&b).unwrap();
        assert_eq!((i.sample_rate, i.channels, i.bitrate_kbps, i.approx), (Some(44100), Some(2), Some(128), true));
        let secs = i.seconds.unwrap();
        assert!((secs - 417.0 * 100.0 * 8.0 / 128000.0).abs() < 0.01);
        // Xing ヘッダー（フレーム数 1000 → 約 26.1 秒）
        let mut x = audio_frames(100);
        let off = 4 + 32;
        x[off..off + 4].copy_from_slice(b"Xing");
        x[off + 4..off + 8].copy_from_slice(&1u32.to_be_bytes());
        x[off + 8..off + 12].copy_from_slice(&1000u32.to_be_bytes());
        let i = info(&x).unwrap();
        assert!(!i.approx);
        assert!((i.seconds.unwrap() - 1000.0 * 1152.0 / 44100.0).abs() < 0.01);
    }

    #[test]
    fn id3_unsync_tag_is_read() {
        let mut b = mp3_with(3, &[frame_bytes(3, b"TIT2", b"\0AB")], 0, false);
        // 本体を非同期化した体にする（FF を含めて FF 00 に）。ここでは FF 00 を 1 つ足した本文
        let body = {
            let mut body = frame_bytes(3, b"TIT2", &[0, 0xFF, b'x']);
            // FF の後ろに 00 を挿入した形に差し替える（長さは 1 増える）
            let pos = body.iter().rposition(|&c| c == 0xFF).unwrap();
            body.insert(pos + 1, 0);
            body
        };
        let mut t = b"ID3\x03\0\x80".to_vec();
        t.extend_from_slice(&synchsafe_enc(body.len()).unwrap());
        t.extend_from_slice(&body);
        t.extend_from_slice(&b[10 + synchsafe(&b[6..10]).unwrap()..]);
        b = t;
        assert_eq!(get(&items(&b).unwrap().1, "title"), Some("\u{FF}x"));
    }

    // ---- FLAC ----

    fn flac_with(comments: &[&str], padding: usize) -> Vec<u8> {
        let mut si = vec![0u8; 34];
        // 44100 Hz, 2 ch, 16 bit, 441000 samples（10 秒）
        let rate = 44100u32;
        si[10] = (rate >> 12) as u8;
        si[11] = (rate >> 4) as u8;
        si[12] = ((rate & 0xF) << 4) as u8 | (1 << 1);
        si[13] = (15 << 4) as u8;
        si[14..18].copy_from_slice(&441000u32.to_be_bytes());
        let mut vc = Vec::new();
        vc.extend_from_slice(&7u32.to_le_bytes());
        vc.extend_from_slice(b"vendor!");
        vc.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for c in comments {
            vc.extend_from_slice(&(c.len() as u32).to_le_bytes());
            vc.extend_from_slice(c.as_bytes());
        }
        let mut blocks = vec![(0u8, si), (4, vc), (6, vec![9; 20])];
        if padding > 0 {
            blocks.push((1, vec![0; padding]));
        }
        let mut b = b"fLaC".to_vec();
        let n = blocks.len();
        for (i, (t, d)) in blocks.iter().enumerate() {
            b.push(*t | if i + 1 == n { 0x80 } else { 0 });
            b.extend_from_slice(&(d.len() as u32).to_be_bytes()[1..]);
            b.extend_from_slice(d);
        }
        b.extend_from_slice(&[0xAB; 5000]); // 音声のフレームの代わり
        b
    }

    #[test]
    fn flac_read_info_and_comments() {
        let b = flac_with(&["TITLE=Song", "ARTIST=日本"], 0);
        let it = items(&b).unwrap().1;
        assert_eq!((get(&it, "TITLE"), get(&it, "ARTIST")), (Some("Song"), Some("日本")));
        let i = info(&b).unwrap();
        assert_eq!((i.sample_rate, i.channels, i.bits), (Some(44100), Some(2), Some(16)));
        assert!((i.seconds.unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn flac_write_replaces_only_the_comment_block() {
        let b = flac_with(&["TITLE=Song"], 0);
        let p = write(&b, "TITLE=Other\nARTIST=X\nARTIST=Y\nGENRE=a\\nb\n").unwrap();
        let out = p.apply(&b);
        let it = items(&out).unwrap().1;
        assert_eq!(it.len(), 4);
        assert_eq!(get(&it, "TITLE"), Some("Other"));
        assert_eq!(it.iter().filter(|(k, _)| k == "ARTIST").count(), 2);
        assert_eq!(get(&it, "GENRE"), Some("a\nb"));
        // 音声部分・PICTURE ブロックはそのまま
        assert!(out.ends_with(&[0xAB; 5000]));
        let (blocks, _) = flac_blocks(&out).unwrap();
        assert!(blocks.iter().any(|(t, d)| *t == 6 && d == &[9u8; 20]));
        assert_eq!(blocks[0].0, 0);
        assert_eq!(info(&out).unwrap().seconds, info(&b).unwrap().seconds);
    }

    #[test]
    fn flac_padding_absorbs_a_size_change_in_place() {
        let b = flac_with(&["TITLE=Song with a long value here"], 500);
        let p = write(&b, "TITLE=s\n").unwrap();
        assert!(p.in_place);
        let out = p.apply(&b);
        assert_eq!(out.len(), b.len());
        assert_eq!(get(&items(&out).unwrap().1, "TITLE"), Some("s"));
        // 増えるとき（パディングが足りない）は全体を書き直す
        let big = write(&b, &format!("TITLE={}\n", "y".repeat(2000))).unwrap();
        assert!(!big.in_place);
        assert_eq!(get(&items(&big.apply(&b)).unwrap().1, "TITLE").map(|s| s.len()), Some(2000));
    }

    #[test]
    fn flac_rejects_bad_keys_and_truncated_files() {
        let b = flac_with(&["A=b"], 0);
        assert!(write(&b, "a b=c\n").is_ok()); // 空白は使える
        assert!(write(&b, "日本=c\n").is_err());
        assert!(write(&b[..30], "A=b\n").is_err());
        for n in 0..120 {
            let _ = items(&b[..n]);
            let _ = info(&b[..n]);
        }
    }

    // ---- WAV ----

    fn wav_with(info_items: &[(&[u8; 4], &str)], info_first: bool) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&32000u32.to_le_bytes());
        fmt.extend_from_slice(&4u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        let data = vec![7u8; 64001]; // 奇数 → パディングの 1 バイトが付く
        let mut list = b"INFO".to_vec();
        for (id, v) in info_items {
            let mut val = v.as_bytes().to_vec();
            val.push(0);
            list.extend_from_slice(*id);
            list.extend_from_slice(&(val.len() as u32).to_le_bytes());
            list.extend_from_slice(&val);
            if val.len() & 1 == 1 {
                list.push(0);
            }
        }
        let mut body = b"WAVE".to_vec();
        let put = |id: &[u8; 4], d: &[u8], body: &mut Vec<u8>| {
            body.extend_from_slice(id);
            body.extend_from_slice(&(d.len() as u32).to_le_bytes());
            body.extend_from_slice(d);
            if d.len() & 1 == 1 {
                body.push(0);
            }
        };
        put(b"fmt ", &fmt, &mut body);
        if info_first && !info_items.is_empty() {
            put(b"LIST", &list, &mut body);
        }
        put(b"data", &data, &mut body);
        if !info_first && !info_items.is_empty() {
            put(b"LIST", &list, &mut body);
        }
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&body);
        b
    }

    #[test]
    fn wav_read_info_and_tags() {
        let b = wav_with(&[(b"INAM", "Title"), (b"IART", "歌手"), (b"ISBJ", "x")], false);
        let it = items(&b).unwrap().1;
        assert_eq!((get(&it, "title"), get(&it, "artist"), get(&it, "ISBJ")), (Some("Title"), Some("歌手"), Some("x")));
        let i = info(&b).unwrap();
        assert_eq!((i.sample_rate, i.channels, i.bits, i.bitrate_kbps), (Some(8000), Some(2), Some(16), Some(256)));
        assert!((i.seconds.unwrap() - 64001.0 / 32000.0).abs() < 1e-9);
    }

    #[test]
    fn wav_write_replaces_info_and_keeps_data() {
        for first in [true, false] {
            let b = wav_with(&[(b"INAM", "Old")], first);
            let out = write(&b, "title=新\ncomment=c\nISBJ=s\n").unwrap().apply(&b);
            let it = items(&out).unwrap().1;
            assert_eq!((get(&it, "title"), get(&it, "comment"), get(&it, "ISBJ")), (Some("新"), Some("c"), Some("s")));
            let (ch, end) = wav_chunks(&out).unwrap();
            assert_eq!(end, out.len());
            assert_eq!(ch.iter().find(|c| &c.id == b"data").unwrap().data, &[7u8; 64001][..]);
            assert_eq!(ch.iter().filter(|c| is_info_list(c)).count(), 1);
            assert_eq!(8 + u32le(&out[4..]) as usize, out.len());
        }
        // タグ無しの WAV に足す・全部消す
        let b = wav_with(&[], false);
        let out = write(&b, "title=T\n").unwrap().apply(&b);
        assert_eq!(get(&items(&out).unwrap().1, "title"), Some("T"));
        let none = write(&out, "# empty\n").unwrap().apply(&out);
        assert_eq!(none, b);
    }

    #[test]
    fn wav_rejects_bad_input_without_panicking() {
        let b = wav_with(&[(b"INAM", "x")], false);
        assert!(write(&b, "bad key=1\n").is_err());
        assert!(write(&b[..b.len() - 10], "title=x\n").is_err()); // 途中で切れている
        for n in 0..80 {
            let _ = items(&b[..n]);
            let _ = info(&b[..n]);
            let _ = write(&b[..n], "title=x\n");
        }
    }

    #[test]
    fn html_escapes_and_shows_rows() {
        let i = info(&audio_frames(10)).unwrap();
        assert!(side_rows(&[("title".into(), "<&>".into())]).iter().any(|(k, v)| k == "title" && v == "<&>"));
        assert!(side_rows(&[]).iter().any(|(k, _)| k == "タグ" || k == "Tags"));
        let look = PlayerLook { bg: "#000", fg: "#fff", dim: "#888", accent: "#0f0", track: "#333" };
        let h = page_html("a<b>.mp3", &Ok(i), &[("title".into(), "<&>".into())], false, None, "Stopped", &look);
        assert!(h.contains("&lt;&amp;&gt;") && h.contains("a&lt;b&gt;.mp3") && h.contains("Stopped") && !h.contains("128 kbps")); // 情報は右パネル
        assert!(h.contains("\u{25B6}") && h.contains("0:00"));
        let h = page_html("x", &Err("e".into()), &[], true, Some(1.0), "", &look);
        assert!(h.contains("\u{25A0}") && h.contains("<p align=\"center\">e</p>"));
        assert_eq!(format_time(65.4), "1:05");
        assert_eq!(format_time(3725.0), "1:02:05");
    }

    #[test]
    fn format_detection() {
        assert_eq!(format_of(b"fLaC...."), Some(Format::Flac));
        assert_eq!(format_of(b"ID3\x03"), Some(Format::Mp3));
        assert_eq!(format_of(&[0xFF, 0xFB, 0, 0]), Some(Format::Mp3));
        assert_eq!(format_of(b"hello world!"), None);
        assert!(supports(Path::new("a/B.MP3")) && supports(Path::new("x.flac")) && !supports(Path::new("x.m4a")));
    }
}
