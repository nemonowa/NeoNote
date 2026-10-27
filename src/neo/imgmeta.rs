// 画像のメタデータ（文字）を、`key=value` の行として読み書きする（GUI なし・依存なし）。
// 対象は「文字として素直に編集できるもの」だけ:
//   PNG  … tEXt / 圧縮なしの iTXt（Title, Author, Comment など）
//   JPEG … COM（コメント。キーは常に `comment`）
// EXIF（撮影日時・GPS・カメラ情報）や zTXt・圧縮 iTXt は **編集せず、そのまま残す**。
// 書き戻しは画像の本体（ピクセル）に触れず、メタデータの部分だけ作り直す。
use std::path::Path;

pub fn supports(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpeg,
}

const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn format_of(b: &[u8]) -> Option<Format> {
    if b.starts_with(PNG_SIG) {
        Some(Format::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else {
        None
    }
}

// ---- 値の退避（1 行に収めるため、改行と `\` を書き換える） ----

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o
}

pub fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x);
                }
                None => o.push('\\'),
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// `key=value` の行（空行と `#` で始まる行は無視）。キーが空・`=` が無い行はエラー。
pub fn parse_lines(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, val)) = line.split_once('=') else {
            return Err(format!("{} 行目: `キー=値` の形ではありません", i + 1));
        };
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("{} 行目: キーが空です", i + 1));
        }
        v.push((k.to_string(), unescape(val)));
    }
    Ok(v)
}

fn format_lines(items: &[(String, String)]) -> String {
    items.iter().map(|(k, v)| format!("{k}={}\n", escape(v))).collect()
}

const HELP_PNG: &str = "# PNG のテキスト情報（tEXt / iTXt）。1 行 1 項目の `キー=値`。保存すると画像に書き戻す。\n# 行を足す・消す・直すのが自由。値の改行は \\n と書く。EXIF など他の情報は触らない。\n";
const HELP_JPEG: &str = "# JPEG のコメント（COM）。1 行 1 件の `comment=値`。保存すると画像に書き戻す。\n# EXIF（撮影日時・GPS など）は触らない。値の改行は \\n と書く。\n";

/// 画像から編集用の文字を作る。
pub fn read(bytes: &[u8]) -> Result<(Format, String), String> {
    match format_of(bytes) {
        Some(Format::Png) => {
            let items: Vec<(String, String)> = png_chunks(bytes)?
                .iter()
                .filter_map(|c| match c.kind {
                    [b't', b'E', b'X', b't'] => text_chunk(c.data),
                    [b'i', b'T', b'X', b't'] => itxt_chunk(c.data),
                    _ => None,
                })
                .collect();
            Ok((Format::Png, format!("{HELP_PNG}{}", format_lines(&items))))
        }
        Some(Format::Jpeg) => {
            let items: Vec<(String, String)> =
                jpeg_segments(bytes)?.iter().filter(|s| s.marker == 0xFE).map(|s| ("comment".to_string(), String::from_utf8_lossy(s.data).into_owned())).collect();
            Ok((Format::Jpeg, format!("{HELP_JPEG}{}", format_lines(&items))))
        }
        None => Err("PNG / JPEG ではありません".into()),
    }
}

/// 編集した文字を画像のバイト列へ書き戻す（メタデータ以外はそのまま）。
pub fn write(bytes: &[u8], text: &str) -> Result<Vec<u8>, String> {
    let items = parse_lines(text)?;
    match format_of(bytes) {
        Some(Format::Png) => write_png(bytes, &items),
        Some(Format::Jpeg) => write_jpeg(bytes, &items),
        None => Err("PNG / JPEG ではありません".into()),
    }
}

// ---- PNG ----

struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

fn png_chunks(b: &[u8]) -> Result<Vec<Chunk<'_>>, String> {
    let mut v = Vec::new();
    let mut i = PNG_SIG.len();
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let kind = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        let end = i + 8 + len;
        if end + 4 > b.len() {
            return Err("PNG が壊れています（チャンクが途中で切れています）".into());
        }
        v.push(Chunk { kind, data: &b[i + 8..end] });
        i = end + 4;
        if kind == *b"IEND" {
            break;
        }
    }
    if v.last().map(|c| c.kind) != Some(*b"IEND") {
        return Err("PNG が壊れています（IEND がありません）".into());
    }
    Ok(v)
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn text_chunk(d: &[u8]) -> Option<(String, String)> {
    let z = d.iter().position(|&c| c == 0)?;
    Some((latin1(&d[..z]), latin1(&d[z + 1..])))
}

/// 圧縮なしの iTXt（圧縮ありは None = 触らない）。
fn itxt_chunk(d: &[u8]) -> Option<(String, String)> {
    let z = d.iter().position(|&c| c == 0)?;
    let rest = d.get(z + 1..)?;
    if *rest.first()? != 0 {
        return None; // 圧縮あり
    }
    let rest = rest.get(2..)?; // 圧縮フラグ・方式
    let z2 = rest.iter().position(|&c| c == 0)?; // 言語タグ
    let rest = &rest[z2 + 1..];
    let z3 = rest.iter().position(|&c| c == 0)?; // 翻訳キーワード
    Some((latin1(&d[..z]), String::from_utf8_lossy(&rest[z3 + 1..]).into_owned()))
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn to_latin1(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

fn write_png(b: &[u8], items: &[(String, String)]) -> Result<Vec<u8>, String> {
    let chunks = png_chunks(b)?;
    let mut out = Vec::with_capacity(b.len() + 256);
    out.extend_from_slice(PNG_SIG);
    for c in &chunks {
        let editable = c.kind == *b"tEXt" || (c.kind == *b"iTXt" && itxt_chunk(c.data).is_some());
        if c.kind == *b"IEND" {
            for (k, v) in items {
                let kb = to_latin1(k).filter(|kb| !kb.is_empty() && kb.len() <= 79 && !kb.contains(&0));
                let Some(kb) = kb else { return Err(format!("キー「{k}」は PNG では使えません（1〜79 文字・Latin-1）")) };
                match to_latin1(v).filter(|vb| !vb.contains(&0)) {
                    Some(vb) => {
                        let mut d = kb;
                        d.push(0);
                        d.extend_from_slice(&vb);
                        push_chunk(&mut out, b"tEXt", &d);
                    }
                    None => {
                        // Latin-1 に収まらない値（日本語など）は UTF-8 の iTXt にする
                        let mut d = kb;
                        d.extend_from_slice(&[0, 0, 0, 0, 0]); // 区切り・圧縮なし・方式・言語タグ空・翻訳キー空
                        d.extend_from_slice(v.replace('\0', "").as_bytes());
                        push_chunk(&mut out, b"iTXt", &d);
                    }
                }
            }
        }
        if !editable {
            push_chunk(&mut out, &c.kind, c.data);
        }
    }
    Ok(out)
}

// ---- JPEG ----

struct Segment<'a> {
    marker: u8,
    /// マーカーと長さを除いた中身。
    data: &'a [u8],
}

/// SOS（画像データの始まり）の手前までのセグメント。戻り値の 2 つ目の要素は無いので、呼び出し側は位置を `jpeg_split` で得る。
fn jpeg_segments(b: &[u8]) -> Result<Vec<Segment<'_>>, String> {
    jpeg_split(b).map(|(segs, _)| segs)
}

/// (SOS より前のセグメント, SOS 以降の生のバイト列の開始位置)。
fn jpeg_split(b: &[u8]) -> Result<(Vec<Segment<'_>>, usize), String> {
    let mut v = Vec::new();
    let mut i = 2; // SOI の次
    loop {
        if i + 4 > b.len() || b[i] != 0xFF {
            return Err("JPEG が壊れています（マーカーが見つかりません）".into());
        }
        let marker = b[i + 1];
        if marker == 0xDA {
            return Ok((v, i)); // SOS
        }
        if marker == 0xFF {
            i += 1; // 詰め物
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if len < 2 || i + 2 + len > b.len() {
            return Err("JPEG が壊れています（セグメントが途中で切れています）".into());
        }
        v.push(Segment { marker, data: &b[i + 4..i + 2 + len] });
        i += 2 + len;
    }
}

fn write_jpeg(b: &[u8], items: &[(String, String)]) -> Result<Vec<u8>, String> {
    let (segs, sos) = jpeg_split(b)?;
    let mut out = Vec::with_capacity(b.len() + 256);
    out.extend_from_slice(&b[..2]); // SOI
    let put_comments = |out: &mut Vec<u8>| -> Result<(), String> {
        for (k, v) in items {
            if k != "comment" {
                return Err(format!("JPEG で使えるキーは `comment` だけです（「{k}」）"));
            }
            if v.len() > 65533 {
                return Err("コメントが長すぎます（65533 バイトまで）".into());
            }
            out.extend_from_slice(&[0xFF, 0xFE]);
            out.extend_from_slice(&((v.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        Ok(())
    };
    // コメントは、先頭に並ぶ APPn（JFIF・EXIF）の後ろへ置く（JFIF は SOI の直後でなければならないため）
    let mut placed = false;
    for s in &segs {
        if !placed && !(0xE0..=0xEF).contains(&s.marker) {
            put_comments(&mut out)?;
            placed = true;
        }
        if s.marker != 0xFE {
            out.extend_from_slice(&[0xFF, s.marker]);
            out.extend_from_slice(&((s.data.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(s.data);
        }
    }
    if !placed {
        put_comments(&mut out)?;
    }
    out.extend_from_slice(&b[sos..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_with(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut o = PNG_SIG.to_vec();
        push_chunk(&mut o, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
        for (k, d) in chunks {
            push_chunk(&mut o, k, d);
        }
        push_chunk(&mut o, b"IDAT", &[1, 2, 3]);
        push_chunk(&mut o, b"IEND", &[]);
        o
    }

    #[test]
    fn crc_matches_the_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082); // どの PNG の末尾にもある CRC
    }

    #[test]
    fn png_text_round_trips_and_keeps_other_chunks() {
        let src = png_with(&[(b"tEXt", b"Title\0Hello".to_vec()), (b"zTXt", vec![b'z', 0, 0, 9, 9])]);
        let (f, text) = read(&src).unwrap();
        assert_eq!(f, Format::Png);
        assert!(text.contains("Title=Hello\n"));
        let edited = text.replace("Title=Hello", "Title=World\nAuthor=me\nNote=a\\nb");
        let out = write(&src, &edited).unwrap();
        let (_, again) = read(&out).unwrap();
        assert!(again.contains("Title=World\n") && again.contains("Author=me\n") && again.contains("Note=a\\nb\n"));
        let kinds: Vec<[u8; 4]> = png_chunks(&out).unwrap().iter().map(|c| c.kind).collect();
        assert!(kinds.contains(b"zTXt") && kinds.contains(b"IDAT")); // 触らない
        assert_eq!(kinds.last(), Some(b"IEND"));
        assert_eq!(kinds.iter().filter(|k| **k == *b"tEXt").count(), 3);
    }

    #[test]
    fn png_non_latin1_goes_to_itxt_as_utf8() {
        let src = png_with(&[]);
        let out = write(&src, "Title=日本語のタイトル\n").unwrap();
        assert!(png_chunks(&out).unwrap().iter().any(|c| c.kind == *b"iTXt"));
        let (_, t) = read(&out).unwrap();
        assert!(t.contains("Title=日本語のタイトル\n"));
    }

    #[test]
    fn png_deleting_all_lines_removes_the_chunks() {
        let src = png_with(&[(b"tEXt", b"Title\0Hello".to_vec())]);
        let out = write(&src, "# nothing\n").unwrap();
        assert!(!png_chunks(&out).unwrap().iter().any(|c| c.kind == *b"tEXt"));
    }

    #[test]
    fn png_bad_input_is_an_error() {
        assert!(read(b"\x89PNG\r\n\x1a\nxx").is_err());
        assert!(write(&png_with(&[]), "no equals sign\n").is_err());
        assert!(write(&png_with(&[]), "=v\n").is_err());
        assert!(write(&png_with(&[]), &format!("{}=v\n", "k".repeat(80))).is_err());
        assert!(read(b"nope").is_err());
    }

    fn jpeg(extra: &[(u8, &[u8])]) -> Vec<u8> {
        let mut o = vec![0xFF, 0xD8];
        let seg = |m: u8, d: &[u8], o: &mut Vec<u8>| {
            o.extend_from_slice(&[0xFF, m]);
            o.extend_from_slice(&((d.len() + 2) as u16).to_be_bytes());
            o.extend_from_slice(d);
        };
        seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0", &mut o);
        for (m, d) in extra {
            seg(*m, d, &mut o);
        }
        seg(0xDB, &[0; 4], &mut o);
        o.extend_from_slice(&[0xFF, 0xDA, 0, 3, 0, 0xAA, 0xBB, 0xFF, 0xD9]);
        o
    }

    #[test]
    fn jpeg_comment_round_trips_after_jfif() {
        let src = jpeg(&[(0xFE, b"old"), (0xE1, b"Exif\0\0data")]);
        let (f, text) = read(&src).unwrap();
        assert_eq!(f, Format::Jpeg);
        assert!(text.contains("comment=old\n"));
        let out = write(&src, "comment=new one\ncomment=日本語\n").unwrap();
        // JFIF は SOI の直後のまま、EXIF も残り、画像データ（SOS 以降）は同一
        assert_eq!(&out[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
        assert!(out.windows(4).any(|w| w == b"Exif"));
        assert!(out.ends_with(&[0xFF, 0xDA, 0, 3, 0, 0xAA, 0xBB, 0xFF, 0xD9]));
        let (_, t2) = read(&out).unwrap();
        assert!(t2.contains("comment=new one\n") && t2.contains("comment=日本語\n") && !t2.contains("old"));
    }

    #[test]
    fn jpeg_rejects_other_keys_and_garbage() {
        assert!(write(&jpeg(&[]), "Title=x\n").is_err());
        assert!(read(&[0xFF, 0xD8, 0xFF]).is_err());
    }

    #[test]
    fn escape_round_trips() {
        for s in ["a\\b", "x\ny", "\\n", "end\\", ""] {
            assert_eq!(unescape(&escape(s)), s.replace('\r', ""));
        }
    }

    #[test]
    fn supports_follows_the_extension() {
        assert!(supports(Path::new("a.PNG")) && supports(Path::new("a.jpeg")));
        assert!(!supports(Path::new("a.gif")) && !supports(Path::new("a.txt")));
    }
}
