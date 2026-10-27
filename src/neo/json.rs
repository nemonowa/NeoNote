// JSON 整形（依存を足さない自前の検証つき整形器）。Minecraft のデータ・言語ファイルを直す用途。
// 数値と文字列は入力の文字列のまま出す（桁や `\u` 表記を変えない）。キーの順も変えない。
// 厳密な JSON だけを受ける（末尾カンマ・コメント・単引用符はエラー。位置を返す）。

#[derive(Debug, PartialEq, Eq)]
pub struct JsonError {
    /// 入力先頭からのバイト位置。
    pub pos: usize,
    pub msg: &'static str,
}

const MAX_DEPTH: usize = 256;
const INDENT: &str = "  ";

struct P<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    out: String,
}

type R = Result<(), JsonError>;

impl<'a> P<'a> {
    fn err<T>(&self, msg: &'static str) -> Result<T, JsonError> {
        Err(JsonError { pos: self.i.min(self.b.len()), msg })
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn nl(&mut self, depth: usize) {
        self.out.push('\n');
        for _ in 0..depth {
            self.out.push_str(INDENT);
        }
    }
    fn lit(&mut self, word: &str) -> R {
        if self.s[self.i..].starts_with(word) {
            self.out.push_str(word);
            self.i += word.len();
            Ok(())
        } else {
            self.err("invalid literal")
        }
    }
    fn string(&mut self) -> R {
        let start = self.i;
        self.i += 1; // "
        loop {
            match self.peek() {
                None => return self.err("unterminated string"),
                Some(b'"') => {
                    self.i += 1;
                    self.out.push_str(&self.s[start..self.i]);
                    return Ok(());
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.i += 1,
                        Some(b'u') => {
                            self.i += 1;
                            for _ in 0..4 {
                                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                                    return self.err("invalid \\u escape");
                                }
                                self.i += 1;
                            }
                        }
                        _ => return self.err("invalid escape"),
                    }
                }
                Some(c) if c < 0x20 => return self.err("control character in string"),
                Some(_) => self.i += 1,
            }
        }
    }
    fn digits(&mut self) -> bool {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.i > st
    }
    fn number(&mut self) -> R {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return self.err("invalid number"),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !self.digits() {
                return self.err("digits expected after '.'");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !self.digits() {
                return self.err("digits expected in exponent");
            }
        }
        self.out.push_str(&self.s[start..self.i]);
        Ok(())
    }
    fn value(&mut self, depth: usize) -> R {
        if depth > MAX_DEPTH {
            return self.err("nesting too deep");
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string(),
            Some(b't') => self.lit("true"),
            Some(b'f') => self.lit("false"),
            Some(b'n') => self.lit("null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            None => self.err("value expected"),
            Some(_) => self.err("unexpected character"),
        }
    }
    fn object(&mut self, depth: usize) -> R {
        self.i += 1;
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            self.out.push_str("{}");
            return Ok(());
        }
        self.out.push('{');
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return self.err("string key expected");
            }
            self.nl(depth + 1);
            self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return self.err("':' expected");
            }
            self.i += 1;
            self.out.push_str(": ");
            self.ws();
            self.value(depth + 1)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.out.push(',');
                }
                Some(b'}') => {
                    self.i += 1;
                    self.nl(depth);
                    self.out.push('}');
                    return Ok(());
                }
                _ => return self.err("',' or '}' expected"),
            }
        }
    }
    fn array(&mut self, depth: usize) -> R {
        self.i += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            self.out.push_str("[]");
            return Ok(());
        }
        self.out.push('[');
        loop {
            self.ws();
            self.nl(depth + 1);
            self.value(depth + 1)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    self.out.push(',');
                }
                Some(b']') => {
                    self.i += 1;
                    self.nl(depth);
                    self.out.push(']');
                    return Ok(());
                }
                _ => return self.err("',' or ']' expected"),
            }
        }
    }
}

/// 検証しながら整形する（インデント 2 スペース、改行は `\n`）。前後の空白は落とす。
pub fn format(text: &str) -> Result<String, JsonError> {
    let mut p = P { s: text, b: text.as_bytes(), i: 0, out: String::with_capacity(text.len() * 2) };
    p.ws();
    p.value(0)?;
    p.ws();
    if p.i < p.b.len() {
        return p.err("unexpected data after the value");
    }
    Ok(p.out)
}

/// バイト位置から 1 始まりの（行, 桁[文字数]）。
pub fn line_col(text: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(text.len());
    let mut end = pos;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_nested_values_and_keeps_text_as_is() {
        let src = r#"{"a":1,"b":[true,null,{"c":"x\u00e9\n"}],"d":{},"e":[],"f":-1.50e+3}"#;
        let want = "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null,\n    {\n      \"c\": \"x\\u00e9\\n\"\n    }\n  ],\n  \"d\": {},\n  \"e\": [],\n  \"f\": -1.50e+3\n}";
        assert_eq!(format(src).unwrap(), want);
        assert_eq!(format(want).unwrap(), want); // 整形済みは変わらない
    }

    #[test]
    fn scalars_and_surrounding_whitespace() {
        assert_eq!(format("  \n 42 \n").unwrap(), "42");
        assert_eq!(format("\"日本語\"").unwrap(), "\"日本語\"");
    }

    #[test]
    fn errors_point_at_the_problem() {
        let e = format("{\"a\": 1,}").unwrap_err();
        assert_eq!((e.pos, e.msg), (8, "string key expected"));
        assert_eq!(format("[1 2]").unwrap_err().pos, 3);
        assert_eq!(format("").unwrap_err().msg, "value expected");
        assert_eq!(format("{} x").unwrap_err().msg, "unexpected data after the value");
        assert_eq!(format("'a'").unwrap_err().msg, "unexpected character");
        assert_eq!(format("[01]").unwrap_err().pos, 2);
        assert_eq!(format("\"a\nb\"").unwrap_err().msg, "control character in string");
        assert_eq!(format("\"\\x\"").unwrap_err().msg, "invalid escape");
        assert_eq!(format("\"abc").unwrap_err().msg, "unterminated string");
        assert_eq!(format("tru").unwrap_err().msg, "invalid literal");
        assert_eq!(format("1.").unwrap_err().msg, "digits expected after '.'");
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let s = "[".repeat(10_000);
        assert_eq!(format(&s).unwrap_err().msg, "nesting too deep");
    }

    #[test]
    fn line_col_counts_chars() {
        assert_eq!(line_col("ab\nいc", 0), (1, 1));
        assert_eq!(line_col("ab\nいc", 6), (2, 2));
        assert_eq!(line_col("ab\nいc", 4), (2, 1)); // 文字の途中は手前に寄せる
    }
}
