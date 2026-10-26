// 文字コードの判定・変換。fltk に依存しない。
// 対応: UTF-8 / Windows-31J / EUC-JP / UTF-16LE / UTF-16BE / ISO-8859-1 / US-ASCII。
// 日本語は encoding_rs の SHIFT_JIS を使う。これは WHATWG の「Shift_JIS」で、実体は Windows-31J（CP932）
// （NEC・IBM 拡張文字を含む）。JIS 標準の Shift_JIS とは違うので、名前は実際の方式の Windows-31J にする。
use encoding_rs::{EUC_JP, SHIFT_JIS, UTF_16BE, UTF_16LE};

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf8,
    Windows31J,
    EucJp,
    Utf16Le,
    Utf16Be,
    Iso8859_1,
    UsAscii,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Windows31J => "Windows-31J",
            Encoding::EucJp => "EUC-JP",
            Encoding::Utf16Le => "UTF-16LE",
            Encoding::Utf16Be => "UTF-16BE",
            Encoding::Iso8859_1 => "ISO-8859-1",
            Encoding::UsAscii => "US-ASCII",
        }
    }

    /// この文字コードの BOM のバイト列（BOM を持たない文字コードは空）。
    pub fn bom_bytes(self) -> &'static [u8] {
        match self {
            Encoding::Utf8 => &UTF8_BOM[..],
            Encoding::Utf16Le => &UTF16LE_BOM[..],
            Encoding::Utf16Be => &UTF16BE_BOM[..],
            _ => &[],
        }
    }

    /// 新しく作る文書でこの文字コードを選んだとき、BOM を付けて保存するか。UTF-16 は BOM が無いと向きが読み取れないので付ける。
    pub fn wants_bom(self) -> bool {
        matches!(self, Encoding::Utf16Le | Encoding::Utf16Be)
    }

    /// 厳密に復号する。不正なバイトがあれば Err(最初の不正位置のバイトオフセット)。
    pub fn decode(self, bytes: &[u8]) -> Result<String, usize> {
        match self {
            Encoding::Utf8 => std::str::from_utf8(bytes).map(str::to_owned).map_err(|e| e.valid_up_to()),
            Encoding::Iso8859_1 => Ok(bytes.iter().map(|&b| b as char).collect()),
            Encoding::UsAscii => match bytes.iter().position(|&b| b >= 0x80) {
                Some(i) => Err(i),
                None => Ok(bytes.iter().map(|&b| b as char).collect()),
            },
            Encoding::Windows31J => decode_rs(SHIFT_JIS, bytes),
            Encoding::EucJp => decode_rs(EUC_JP, bytes),
            Encoding::Utf16Le => decode_rs(UTF_16LE, bytes),
            Encoding::Utf16Be => decode_rs(UTF_16BE, bytes),
        }
    }

    /// 変換する。表現できない文字は `?` にして続行し、その件数と最初の位置を返す。
    /// 呼び出し側は `lossy_count > 0` のとき保存前に必ず警告すること。
    pub fn encode(self, text: &str) -> Encoded {
        let mut lossy = Lossy::default();
        let bytes = match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Iso8859_1 => encode_single(text, 0xFF, &mut lossy),
            Encoding::UsAscii => encode_single(text, 0x7F, &mut lossy),
            Encoding::Windows31J => encode_rs(SHIFT_JIS, text, &mut lossy),
            Encoding::EucJp => encode_rs(EUC_JP, text, &mut lossy),
            // encoding_rs は UTF-16 へは書き出せない（UTF-8 になる）ので自前。UTF-16 は全ての文字を表せる
            Encoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            Encoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        };
        let first = lossy.first.map(|(off, ch)| locate(text, off, ch));
        Encoded { bytes, lossy_count: lossy.count, first }
    }
}

#[derive(Default)]
struct Lossy {
    count: usize,
    first: Option<(usize, char)>,
}

impl Lossy {
    fn note(&mut self, byte_offset: usize, ch: char) {
        self.count += 1;
        self.first.get_or_insert((byte_offset, ch));
    }
}

fn encode_single(text: &str, max: u32, lossy: &mut Lossy) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        if (c as u32) <= max {
            out.push(c as u32 as u8);
        } else {
            lossy.note(i, c);
            out.push(b'?');
        }
    }
    out
}

/// encoding_rs で厳密に復号する。不正なバイトがあれば、その位置（バイトオフセット）を Err で返す。
fn decode_rs(enc: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String, usize> {
    let mut dec = enc.new_decoder_without_bom_handling();
    let mut out = String::new();
    out.reserve(dec.max_utf8_buffer_length_without_replacement(bytes.len()).unwrap_or(bytes.len() * 3));
    let (res, read) = dec.decode_to_string_without_replacement(bytes, &mut out, true);
    match res {
        encoding_rs::DecoderResult::InputEmpty => Ok(out),
        encoding_rs::DecoderResult::Malformed(bad, ahead) => Err(read.saturating_sub(bad as usize + ahead as usize)),
        encoding_rs::DecoderResult::OutputFull => Err(read),
    }
}

/// encoding_rs で変換する。表せない文字は `?` にして `lossy` に記録する（UTF-16 以外の日本語の文字コード用）。
fn encode_rs(enc_rs: &'static encoding_rs::Encoding, text: &str, lossy: &mut Lossy) -> Vec<u8> {
    let mut enc = enc_rs.new_encoder();
    let mut out: Vec<u8> = Vec::with_capacity(text.len());
    let mut buf = vec![0u8; enc.max_buffer_length_from_utf8_without_replacement(text.len()).unwrap_or(text.len() * 2 + 16)];
    let mut pos = 0;
    loop {
        let (res, read, written) = enc.encode_from_utf8_without_replacement(&text[pos..], &mut buf, true);
        out.extend_from_slice(&buf[..written]);
        match res {
            encoding_rs::EncoderResult::InputEmpty => break,
            encoding_rs::EncoderResult::Unmappable(c) => {
                lossy.note(pos + read - c.len_utf8(), c);
                out.push(b'?');
                pos += read;
            }
            encoding_rs::EncoderResult::OutputFull => pos += read,
        }
    }
    out
}

/// バイトオフセットを 1 始まりの 行・列（文字単位）に直す。
fn locate(text: &str, byte_offset: usize, ch: char) -> Unmappable {
    let before = &text[..byte_offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    Unmappable { line, col, ch }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Unmappable {
    pub line: usize,
    pub col: usize,
    pub ch: char,
}

#[derive(Debug)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub lossy_count: usize,
    pub first: Option<Unmappable>,
}

/// 自動判定の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Detected {
    Text { encoding: Encoding, bom: bool },
    /// テキストではなさそう（HEX モードは第5段階）。
    Binary,
}

/// BOM なしの UTF-16 の推定。ASCII の多い UTF-16 は、2 バイトの片方が 0 になる対が目立つ（LE なら上位、BE なら下位）。
/// 対の 3 割以上がその形で、逆の形が 1 つも無く、全体が厳密に復号でき、制御文字（改行・タブ以外）が出ないときだけ。
/// 日本語だけの BOM なし UTF-16 は 0 が出ないので推定できない（手動で選ぶ）。
fn guess_utf16_without_bom(bytes: &[u8]) -> Option<Encoding> {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return None;
    }
    let head = &bytes[..bytes.len().min(8192) & !1];
    let pairs = head.len() / 2;
    let (mut le, mut be) = (0usize, 0usize);
    for p in head.chunks_exact(2) {
        match (p[0], p[1]) {
            (c, 0) if c != 0 => le += 1,
            (0, c) if c != 0 => be += 1,
            _ => {}
        }
    }
    let enc = if le * 10 >= pairs * 3 && be == 0 {
        Encoding::Utf16Le
    } else if be * 10 >= pairs * 3 && le == 0 {
        Encoding::Utf16Be
    } else {
        return None;
    };
    let text = enc.decode(bytes).ok()?;
    text.chars().all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r' | '\u{c}')).then_some(enc)
}

/// BOM（UTF-8 / UTF-16）→ BOM なしの UTF-16 の推定 → バイナリ判定 → UTF-8 → Windows-31J → EUC-JP → ISO-8859-1 の順で決める。
/// EUC-JP は Windows-31J として読めなかったときの次点（両方で読めるバイト列は Windows-31J を選ぶ）。
/// UTF-16 の BOM は、本文が厳密に復号できるときだけ採る（FF FE 00 00 で始まる UTF-32 は対象外）。
/// 誤判定は手動切替で直せる前提。
pub fn detect(bytes: &[u8]) -> Detected {
    if bytes.starts_with(&UTF8_BOM) {
        return Detected::Text { encoding: Encoding::Utf8, bom: true };
    }
    if bytes.starts_with(&UTF16LE_BOM) && !bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) && Encoding::Utf16Le.decode(&bytes[2..]).is_ok() {
        return Detected::Text { encoding: Encoding::Utf16Le, bom: true };
    }
    if bytes.starts_with(&UTF16BE_BOM) && Encoding::Utf16Be.decode(&bytes[2..]).is_ok() {
        return Detected::Text { encoding: Encoding::Utf16Be, bom: true };
    }
    if let Some(encoding) = guess_utf16_without_bom(bytes) {
        return Detected::Text { encoding, bom: false };
    }
    let head = &bytes[..bytes.len().min(8192)];
    let looks_binary = head.iter().any(|&b| b == 0 || matches!(b, 0x01..=0x08 | 0x0E..=0x1A | 0x1C..=0x1F));
    if looks_binary {
        return Detected::Binary;
    }
    for enc in [Encoding::Utf8, Encoding::Windows31J, Encoding::EucJp] {
        if enc.decode(bytes).is_ok() {
            return Detected::Text { encoding: enc, bom: false };
        }
    }
    Detected::Text { encoding: Encoding::Iso8859_1, bom: false }
}

/// 先頭にある、その文字コードの BOM の長さ（無ければ 0）。
pub fn bom_len(enc: Encoding, bytes: &[u8]) -> usize {
    let b = enc.bom_bytes();
    if !b.is_empty() && bytes.starts_with(b) {
        b.len()
    } else {
        0
    }
}

/// 変換後のバイト列の先頭に、その文字コードの BOM を付ける。
pub fn with_bom(enc: Encoding, mut bytes: Vec<u8>) -> Vec<u8> {
    let mut v = enc.bom_bytes().to_vec();
    v.append(&mut bytes);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    const SJIS_TEST: [u8; 6] = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67]; // テスト

    #[test]
    fn detect_order() {
        assert_eq!(detect("hello こんにちは".as_bytes()), Detected::Text { encoding: Encoding::Utf8, bom: false });
        assert_eq!(detect(b"\xEF\xBB\xBFabc"), Detected::Text { encoding: Encoding::Utf8, bom: true });
        assert_eq!(detect(&SJIS_TEST), Detected::Text { encoding: Encoding::Windows31J, bom: false });
        assert_eq!(detect(b"caf\xE9"), Detected::Text { encoding: Encoding::Iso8859_1, bom: false });
        assert_eq!(detect(b"MZ\x90\x00\x03"), Detected::Binary);
        assert_eq!(detect(b""), Detected::Text { encoding: Encoding::Utf8, bom: false });
    }

    #[test]
    fn windows31j_extensions_roundtrip() {
        // NEC 特殊文字（①）と IBM 拡張（髙）は、JIS 標準の Shift_JIS には無く Windows-31J にある
        let e = Encoding::Windows31J.encode("①髙");
        assert_eq!(e.bytes, [0x87, 0x40, 0xFB, 0xFC]);
        assert_eq!(Encoding::Windows31J.decode(&e.bytes).unwrap(), "①髙");
    }

    #[test]
    fn sjis_roundtrip() {
        assert_eq!(Encoding::Windows31J.decode(&SJIS_TEST).unwrap(), "テスト");
        let e = Encoding::Windows31J.encode("テスト");
        assert_eq!(e.bytes, SJIS_TEST);
        assert_eq!(e.lossy_count, 0);
    }

    #[test]
    fn invalid_offsets_are_reported() {
        assert_eq!(Encoding::Utf8.decode(b"ab\xFFcd"), Err(2));
        assert_eq!(Encoding::UsAscii.decode(b"abc\x80"), Err(3));
        assert_eq!(Encoding::Windows31J.decode(b"abc\x83"), Err(3)); // 途中で切れた2バイト文字
        assert_eq!(Encoding::Iso8859_1.decode(b"\xE9").unwrap(), "é");
    }

    #[test]
    fn unmappable_is_counted_and_located() {
        let e = Encoding::Windows31J.encode("ab\n日本😀x😀");
        assert_eq!(e.lossy_count, 2);
        assert_eq!(e.first, Some(Unmappable { line: 2, col: 3, ch: '😀' }));
        assert!(e.bytes.ends_with(b"?x?"));

        let e = Encoding::Iso8859_1.encode("é日");
        assert_eq!(e.bytes, vec![0xE9, b'?']);
        assert_eq!(e.first, Some(Unmappable { line: 1, col: 2, ch: '日' }));

        let e = Encoding::UsAscii.encode("aé");
        assert_eq!((e.lossy_count, e.bytes), (1, b"a?".to_vec()));
        assert_eq!(Encoding::Utf8.encode("😀").lossy_count, 0);
    }

    #[test]
    fn utf16_with_bom_is_detected_and_roundtrips() {
        assert_eq!(detect(b"\xFF\xFEa\x00b\x00"), Detected::Text { encoding: Encoding::Utf16Le, bom: true });
        assert_eq!(detect(b"\xFE\xFF\x00a\x00b"), Detected::Text { encoding: Encoding::Utf16Be, bom: true });
        assert_eq!(Encoding::Utf16Le.decode(b"a\x00\xe5\x65").unwrap(), "a日");
        assert_eq!(Encoding::Utf16Be.decode(b"\x00a\x65\xe5").unwrap(), "a日");
        assert_eq!(Encoding::Utf16Le.encode("a日").bytes, b"a\x00\xe5\x65");
        assert_eq!(Encoding::Utf16Be.encode("a日").bytes, b"\x00a\x65\xe5");
        // サロゲートペア（絵文字）も表せる（?に置き換わらない）
        let e = Encoding::Utf16Le.encode("😀");
        assert_eq!((e.bytes.as_slice(), e.lossy_count), (&b"\x3d\xd8\x00\xde"[..], 0));
        assert_eq!(Encoding::Utf16Le.decode(&e.bytes).unwrap(), "😀");
        // 奇数バイト・対にならないサロゲートは不正
        assert!(Encoding::Utf16Le.decode(b"a\x00b").is_err());
        assert!(Encoding::Utf16Le.decode(b"\x00\xd8a\x00").is_err());
        assert_eq!(bom_len(Encoding::Utf16Le, b"\xFF\xFEx"), 2);
        assert_eq!(bom_len(Encoding::Utf16Le, b"\xFE\xFFx"), 0);
        assert_eq!(bom_len(Encoding::Iso8859_1, b"\xFF\xFEx"), 0);
        assert_eq!(with_bom(Encoding::Utf16Be, vec![0, 0x61]), b"\xFE\xFF\x00a");
        assert!(Encoding::Utf16Le.wants_bom() && !Encoding::Utf8.wants_bom());
    }

    #[test]
    fn utf16_bom_is_not_trusted_when_the_body_is_not_utf16() {
        // FF FE で始まっても、本文が UTF-16 として読めなければバイナリ（HEX）に落ちる
        assert_eq!(detect(b"\xFF\xFE\x00\xd8\x41\x00"), Detected::Binary);
        // UTF-32LE の BOM は UTF-16LE と見なさない
        assert_eq!(detect(b"\xFF\xFE\x00\x00a\x00\x00\x00"), Detected::Binary);
    }

    #[test]
    fn utf16_without_bom_is_guessed_only_for_ascii_heavy_text() {
        let le: Vec<u8> = "Hello World\r\nabc".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = "Hello World\r\nabc".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(detect(&le), Detected::Text { encoding: Encoding::Utf16Le, bom: false });
        assert_eq!(detect(&be), Detected::Text { encoding: Encoding::Utf16Be, bom: false });
        // 0 が混ざる普通のバイナリ・短すぎるもの・奇数長は推定しない
        assert_eq!(detect(b"MZ\x90\x00\x03\x00\x00\x00"), Detected::Binary);
        assert_eq!(guess_utf16_without_bom(b"a\x00"), None);
        assert_eq!(guess_utf16_without_bom(b"a\x00b\x00c"), None);
        // 制御文字が出てしまう並びは採らない（0x01 0x00 …）
        assert_eq!(guess_utf16_without_bom(b"\x01\x00\x02\x00\x03\x00"), None);
    }

    #[test]
    fn euc_jp_roundtrip_and_unmappable() {
        let euc = [0xC6, 0xFC, 0xCB, 0xDC, 0xB8, 0xEC]; // 日本語
        assert_eq!(Encoding::EucJp.decode(&euc).unwrap(), "日本語");
        let e = Encoding::EucJp.encode("日本語");
        assert_eq!((e.bytes.as_slice(), e.lossy_count), (&euc[..], 0));
        let e = Encoding::EucJp.encode("a😀");
        assert_eq!((e.bytes.as_slice(), e.lossy_count), (&b"a?"[..], 1));
        assert!(Encoding::EucJp.decode(b"abc\xC6").is_err()); // 途中で切れた 2 バイト文字
    }

    #[test]
    fn large_sjis_encode_does_not_loop_forever() {
        let text = "あ".repeat(200_000) + "😀";
        let e = Encoding::Windows31J.encode(&text);
        assert_eq!(e.lossy_count, 1);
        assert_eq!(e.bytes.len(), 400_001);
    }
}
