// 改行コード（CRLF / LF）の判定と変換。GUI 非依存。単独の CR（古い Mac 形式）は改行として扱わない。

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eol {
    Lf,
    Crlf,
    /// CRLF と LF が両方ある。
    Mixed,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
            Eol::Mixed => "Mixed",
        }
    }

    /// 改行が 1 つも無い文書の既定（Windows は CRLF、それ以外は LF）。
    pub fn platform_default() -> Eol {
        if cfg!(windows) { Eol::Crlf } else { Eol::Lf }
    }
}

/// 本文の改行コード。改行が無ければ `none`。
pub fn detect(text: &str, none: Eol) -> Eol {
    let b = text.as_bytes();
    let (mut crlf, mut lf) = (false, false);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' {
            if i > 0 && b[i - 1] == b'\r' { crlf = true } else { lf = true }
            if crlf && lf {
                return Eol::Mixed;
            }
        }
    }
    match (crlf, lf) {
        (true, _) => Eol::Crlf,
        (_, true) => Eol::Lf,
        _ => none,
    }
}

/// 全ての改行を `to`（Lf か Crlf）にそろえる。
pub fn convert(text: &str, to: Eol) -> String {
    let lf = text.replace("\r\n", "\n");
    if to == Eol::Crlf { lf.replace('\n', "\r\n") } else { lf }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_kind() {
        assert_eq!(detect("a\nb\n", Eol::Crlf), Eol::Lf);
        assert_eq!(detect("a\r\nb", Eol::Lf), Eol::Crlf);
        assert_eq!(detect("a\r\nb\n", Eol::Lf), Eol::Mixed);
        assert_eq!(detect("a\nb\r\n", Eol::Lf), Eol::Mixed);
        assert_eq!(detect("abc", Eol::Crlf), Eol::Crlf);
        assert_eq!(detect("", Eol::Lf), Eol::Lf);
        assert_eq!(detect("a\rb", Eol::Lf), Eol::Lf); // 単独の CR は改行ではない
    }

    #[test]
    fn converts_both_ways_without_doubling() {
        assert_eq!(convert("a\r\nb\nc", Eol::Crlf), "a\r\nb\r\nc");
        assert_eq!(convert("a\r\nb\nc", Eol::Lf), "a\nb\nc");
        assert_eq!(convert("あ\nい\n", Eol::Crlf), "あ\r\nい\r\n");
        assert_eq!(convert("a\rb", Eol::Crlf), "a\rb");
    }
}
