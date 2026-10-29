// HEX エディタ用: バイト列 <-> HEX テキスト。GUI 非依存。
// 1 行 = `オフセット(8桁)  HEX×16  |ASCII|`。編集対象は HEX 部分だけで、
// オフセットと ASCII 欄は表示専用（保存時は読み飛ばし、保存後に作り直す）。

/// 読み込む上限。超えたら先頭だけ読み、保存は無効にする（破損防止）。
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
const DIGITS: &[u8; 16] = b"0123456789ABCDEF";

pub fn format(bytes: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 16 * 80 + 80);
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let off = i * 16;
        for s in (0..8).rev() {
            out.push(DIGITS[(off >> (s * 4)) & 0xF]);
        }
        out.extend_from_slice(b"  ");
        for j in 0..16 {
            match chunk.get(j) {
                Some(&b) => {
                    out.push(DIGITS[(b >> 4) as usize]);
                    out.push(DIGITS[(b & 0xF) as usize]);
                }
                None => out.extend_from_slice(b"  "),
            }
            if j < 15 {
                out.push(b' ');
            }
        }
        out.extend_from_slice(b"  |");
        out.extend(chunk.iter().map(|&b| if (0x20..0x7F).contains(&b) { b } else { b'.' }));
        out.extend_from_slice(b"|\n");
    }
    String::from_utf8(out).unwrap_or_default() // 全て ASCII
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1 始まり
    pub line: usize,
    pub col: usize,
    /// テキスト先頭からのバイト位置（カーソル移動用）
    pub offset: usize,
    pub token: String,
    pub reason: &'static str,
}

fn is_hex2(t: &str) -> bool {
    t.len() == 2 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

/// HEX テキストに含まれるバイト数（タイトルの `Chars:` 用）。`parse` と同じ読み方で、
/// オフセットと ASCII 欄を数えず、不正なトークンがあっても止まらずに 2 桁の 16 進だけを数える。
pub fn byte_count(text: &str) -> usize {
    let mut n = 0usize;
    for line in text.split('\n') {
        for tok in line.split_whitespace().skip(1) {
            if tok.starts_with('|') {
                break;
            }
            if is_hex2(tok) {
                n += 1;
            }
        }
    }
    n
}

/// HEX テキストをバイト列に戻す。空白・改行は無視。
/// 各行は「オフセット(8桁の16進)、2桁の16進のトークン…、`|` で始まる ASCII 欄」。
/// 不正があれば最初の位置を返す（呼び出し側はファイルを書き換えない）。
pub fn parse(text: &str) -> Result<Vec<u8>, ParseError> {
    let mut bytes = Vec::with_capacity(text.len() / 3);
    let mut line_start = 0usize;
    for (ln, line) in text.split('\n').enumerate() {
        let mut first = true;
        for tok in line.split_whitespace() {
            let start = tok.as_ptr() as usize - line.as_ptr() as usize;
            let err = |reason| ParseError {
                line: ln + 1,
                col: line[..start].chars().count() + 1,
                offset: line_start + start,
                token: tok.chars().take(16).collect(),
                reason,
            };
            if first {
                first = false;
                if tok.len() != 8 || !tok.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(err(crate::lang::tr("行頭はオフセット（8桁の16進数）です", "The start of a line is an offset (8-digit hex)")));
                }
                continue;
            }
            if tok.starts_with('|') {
                break;
            }
            if !is_hex2(tok) {
                return Err(err(crate::lang::tr("2桁の16進数（00-FF）だけ書けます", "Only 2-digit hex values (00-FF) can be written")));
            }
            bytes.push(u8::from_str_radix(tok, 16).unwrap_or(0));
        }
        line_start += line.len() + 1;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_byte_values_and_partial_last_line() {
        let all: Vec<u8> = (0..=255u8).chain(0..7).collect();
        let t = format(&all);
        assert_eq!(parse(&t).unwrap(), all);
        assert!(t.starts_with("00000000  00 01 02"));
        assert_eq!(format(&[]), "");
        assert_eq!(parse("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn byte_count_matches_parse_and_skips_offset_and_ascii_column() {
        let all: Vec<u8> = (0..=255u8).chain(0..7).collect();
        assert_eq!(byte_count(&format(&all)), all.len());
        assert_eq!(byte_count(&format(b"|AB CD|ef\x00\xFF")), 11);
        assert_eq!(byte_count(""), 0);
        // 不正なトークンは数えない（止まらない）
        assert_eq!(byte_count("00000000  41 zz 42"), 2);
    }

    #[test]
    fn ascii_column_is_ignored_even_with_pipes_and_hex_lookalikes() {
        let t = format(b"|AB CD|ef\x00\xFF");
        assert_eq!(parse(&t).unwrap(), b"|AB CD|ef\x00\xFF".to_vec());
    }

    #[test]
    fn edited_hex_is_read_back_and_whitespace_is_free() {
        let t = format(b"Hello");
        let t = t.replacen("48 65", "4865  7A", 1); // 4865 は 1 トークンなのでエラー
        assert!(parse(&t).is_err());
        let t = format(b"Hello").replacen("48 65", "7A\t65", 1);
        assert_eq!(&parse(&t).unwrap(), b"zello");
    }

    #[test]
    fn errors_report_line_col_and_offset() {
        let mut t = format(&[0u8; 20]);
        t = t.replacen("00 00", "00 0G", 3); // 1 行目と 2 行目の最初の不正から
        let e = parse(&t).unwrap_err();
        assert_eq!((e.line, e.col), (1, 14));
        assert_eq!(&t[e.offset..e.offset + 2], "0G");
        // 奇数桁
        let e = parse("00000000  4  |x|\n").unwrap_err();
        assert_eq!((e.line, e.col, e.token.as_str()), (1, 11, "4"));
        // オフセットを消した行は黙って捨てずにエラー
        let e = parse("00000000  41  |A|\n41 42\n").unwrap_err();
        assert_eq!((e.line, e.col), (2, 1));
    }

    #[test]
    fn crlf_lines_are_fine() {
        assert_eq!(parse("00000000  41 42  |AB|\r\n").unwrap(), b"AB".to_vec());
    }
}
