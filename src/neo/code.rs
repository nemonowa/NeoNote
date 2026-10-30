// プログラムのコードの最小限の色分け（GUI なし）。コメント・文字列・数値・予約語だけに色を付ける。
// 言語ごとの違いは表（`LANGS`）にまとめ、構文の解析はしない（字句だけを見る）。
// スタイルの文字は mcfunction.rs と同じ（'A' 通常、'C' コメント、'D' 予約語、'F' 数値、'H' 文字列）。
use super::mcfunction::{COMMAND as KEYWORD, COMMENT, NORMAL, NUMBER, STRING};
use std::path::Path;

pub struct Lang {
    exts: &'static [&'static str],
    /// 行コメントの始まり。
    line: &'static [&'static str],
    /// 複数行コメントの (始まり, 終わり)。
    block: Option<(&'static str, &'static str)>,
    /// 文字列の引用符。`"""` のような 3 つ重ねは、ここに書いたものが先に試される。
    quotes: &'static [&'static str],
    /// 引用符の中で改行してよいもの（JS のテンプレート文字列・3 つ重ね）。
    multiline: &'static [&'static str],
    keywords: &'static [&'static str],
}

const C_LIKE_QUOTES: &[&str] = &["\"", "'"];

const LANGS: &[Lang] = &[
    Lang {
        exts: &["py", "pyw"],
        line: &["#"],
        block: None,
        quotes: &["\"\"\"", "'''", "\"", "'"],
        multiline: &["\"\"\"", "'''"],
        keywords: &[
            "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
            "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "match", "case", "nonlocal", "not", "or", "pass", "raise",
            "return", "self", "try", "while", "with", "yield",
        ],
    },
    Lang {
        exts: &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"],
        line: &["//"],
        block: Some(("/*", "*/")),
        quotes: &["\"", "'", "`"],
        multiline: &["`"],
        keywords: &[
            "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "enum", "export", "extends",
            "false", "finally", "for", "function", "if", "import", "in", "instanceof", "interface", "let", "new", "null", "of", "return", "static",
            "super", "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield",
        ],
    },
    Lang {
        exts: &["java", "kt", "kts", "cs", "scala", "groovy", "gradle"],
        line: &["//"],
        block: Some(("/*", "*/")),
        quotes: &["\"\"\"", "\"", "'"],
        multiline: &["\"\"\""],
        keywords: &[
            "abstract", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue", "default", "do", "double", "else",
            "enum", "extends", "false", "final", "finally", "float", "for", "fun", "if", "implements", "import", "instanceof", "int", "interface",
            "long", "namespace", "new", "null", "object", "override", "package", "private", "protected", "public", "record", "return", "sealed",
            "short", "static", "string", "super", "switch", "this", "throw", "throws", "true", "try", "using", "val", "var", "void", "when", "while",
        ],
    },
    Lang {
        exts: &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "go", "swift"],
        line: &["//"],
        block: Some(("/*", "*/")),
        quotes: C_LIKE_QUOTES,
        multiline: &[],
        keywords: &[
            "auto", "bool", "break", "case", "char", "class", "const", "continue", "default", "defer", "do", "double", "else", "enum", "extern",
            "false", "float", "for", "func", "go", "if", "import", "int", "let", "long", "namespace", "nullptr", "package", "private", "public",
            "return", "short", "signed", "sizeof", "static", "struct", "switch", "template", "true", "typedef", "union", "unsigned", "var", "void",
            "while",
        ],
    },
    Lang {
        exts: &["rs"],
        line: &["//"],
        block: Some(("/*", "*/")),
        quotes: &["\""],
        multiline: &["\""],
        keywords: &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false", "fn", "for", "if", "impl", "in", "let",
            "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while",
        ],
    },
    Lang {
        exts: &["sh", "bash", "zsh", "ps1", "psm1", "rb", "pl", "r", "toml", "yaml", "yml", "ini", "cfg", "conf", "properties"],
        line: &["#"],
        block: None,
        quotes: C_LIKE_QUOTES,
        multiline: &[],
        keywords: &[
            "case", "do", "done", "elif", "else", "end", "esac", "export", "fi", "for", "function", "if", "in", "local", "return", "then",
            "while", "def", "true", "false",
        ],
    },
    Lang {
        exts: &["lua"],
        line: &["--"],
        block: Some(("--[[", "]]")),
        quotes: C_LIKE_QUOTES,
        multiline: &[],
        keywords: &[
            "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return",
            "then", "true", "until", "while",
        ],
    },
    Lang {
        exts: &["sql"],
        line: &["--"],
        block: Some(("/*", "*/")),
        quotes: C_LIKE_QUOTES,
        multiline: &[],
        keywords: &[
            "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "JOIN", "LEFT",
            "RIGHT", "INNER", "ON", "AND", "OR", "NOT", "NULL", "AS", "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "PRIMARY", "KEY",
        ],
    },
    Lang {
        exts: &["css", "scss", "less"],
        line: &[],
        block: Some(("/*", "*/")),
        quotes: C_LIKE_QUOTES,
        multiline: &[],
        keywords: &[],
    },
    Lang { exts: &["json", "jsonc", "json5", "mcmeta"], line: &["//"], block: Some(("/*", "*/")), quotes: &["\""], multiline: &[], keywords: &["true", "false", "null"] },
];

/// 色分けする種類のファイルか（.mcfunction か、表にある言語）。
pub fn has_syntax(path: Option<&Path>) -> bool {
    super::mcfunction::is_mcfunction(path) || lang_of(path).is_some()
}

/// 色分けのスタイル。色分けしない種類なら None。
pub fn syntax_styles(path: Option<&Path>, text: &str) -> Option<Vec<u8>> {
    if super::mcfunction::is_mcfunction(path) {
        return Some(super::mcfunction::styles(text));
    }
    lang_of(path).map(|l| styles(l, text))
}

/// 拡張子から言語を選ぶ。表に無ければ None（色分けしない）。
pub fn lang_of(path: Option<&Path>) -> Option<&'static Lang> {
    let ext = path?.extension()?.to_str()?.to_ascii_lowercase();
    LANGS.iter().find(|l| l.exts.contains(&ext.as_str()))
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 本文の各バイトのスタイル（長さは `text.len()`）。
pub fn styles(lang: &Lang, text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = vec![NORMAL; b.len()];
    let sql = lang.exts.contains(&"sql");
    let mut i = 0;
    while i < b.len() {
        let rest = &b[i..];
        if let Some((open, close)) = lang.block.filter(|(o, _)| rest.starts_with(o.as_bytes())) {
            let end = find(b, i + open.len(), close.as_bytes()).map_or(b.len(), |p| p + close.len());
            out[i..end].fill(COMMENT);
            i = end;
            continue;
        }
        if lang.line.iter().any(|l| rest.starts_with(l.as_bytes())) {
            let end = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            out[i..end].fill(COMMENT);
            i = end;
            continue;
        }
        if let Some(q) = lang.quotes.iter().find(|q| rest.starts_with(q.as_bytes())) {
            let multi = lang.multiline.contains(q);
            let q = q.as_bytes();
            let mut j = i + q.len();
            while j < b.len() && !b[j..].starts_with(q) && (multi || b[j] != b'\n') {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = if j < b.len() && b[j..].starts_with(q) { j + q.len() } else { j.min(b.len()) };
            out[i..end].fill(STRING);
            i = end;
            continue;
        }
        let c = b[i];
        if is_ident(c) {
            let mut j = i + 1;
            while j < b.len() && is_ident(b[j]) {
                j += 1;
            }
            let word = &text[i..j];
            if c.is_ascii_digit() {
                // 0x1F・1_000・1.5e-3 など（数字で始まる語の続きの小数点と指数を含める）
                let mut k = j;
                while k < b.len() && (is_ident(b[k]) || (b[k] == b'.' && b.get(k + 1).is_some_and(u8::is_ascii_digit))) {
                    k += 1;
                }
                out[i..k].fill(NUMBER);
                i = k;
                continue;
            }
            let hit = if sql { lang.keywords.iter().any(|k| k.eq_ignore_ascii_case(word)) } else { lang.keywords.contains(&word) };
            // 前が `.` なら、ほかの物の名前（obj.class など）なので予約語にしない
            if hit && (i == 0 || b[i - 1] != b'.') {
                out[i..j].fill(KEYWORD);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    (from..=b.len().saturating_sub(pat.len())).find(|&k| b[k..].starts_with(pat))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(ext: &str, text: &str) -> String {
        let l = lang_of(Some(Path::new(&format!("a.{ext}")))).unwrap();
        String::from_utf8(styles(l, text)).unwrap()
    }

    #[test]
    fn python() {
        assert_eq!(st("py", "if x: # c"), "DDAAAACCC");
        assert_eq!(st("py", "s = 'a#b'"), "AAAAHHHHH");
        assert_eq!(st("py", "\"\"\"a\nb\"\"\" 1"), "HHHHHHHHHAF");
    }

    #[test]
    fn c_like_comments_strings_numbers() {
        assert_eq!(st("js", "let a = 0x1F; // x"), "DDDAAAAAFFFFAACCCC");
        assert_eq!(st("java", "/* a\nb */int"), "CCCCCCCCCDDD");
        assert_eq!(st("js", "\"a\\\"b\" 1.5"), "HHHHHHAFFF");
        assert_eq!(st("js", "`a\nb`"), "HHHHH");
    }

    #[test]
    fn keywords_need_whole_words_and_not_after_a_dot() {
        assert_eq!(st("java", "classy"), "AAAAAA");
        assert_eq!(st("java", "x.class"), "AAAAAAA");
        assert_eq!(st("sql", "select 1"), "DDDDDDAF");
    }

    #[test]
    fn unterminated_string_stops_at_the_line_end() {
        assert_eq!(st("js", "'a\nif"), "HHADD");
    }

    #[test]
    fn unknown_extension_has_no_language() {
        assert!(lang_of(Some(Path::new("a.txt"))).is_none());
        assert!(lang_of(Some(Path::new("Makefile"))).is_none());
        assert!(lang_of(None).is_none());
    }

    #[test]
    fn multibyte_keeps_length() {
        let t = "# あ\nx = 'い'";
        assert_eq!(st("py", t).len(), t.len());
    }
}
