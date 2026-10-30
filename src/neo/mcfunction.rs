// .mcfunction（Minecraft Java 26.3 のデータパックの関数）の色分け（GUI なし）。
// 本文の 1 バイトごとに、スタイルバッファの 1 文字（'A' = 通常 など）を返す。
// 行末の `\` は次の行へ続く 1 つのコマンドとして扱い、`$` で始まる行はマクロの行として扱う。
// 構文は版ごとに大きく変わるので、知らないコマンドや引数は咎めない。印を付けるのは、どの版でも確実に
// 誤りと言える「括弧（[ ] { }）の閉じ忘れ・対応しない閉じ括弧・閉じていない引用符」だけ。
// 次のものは確かめない（正しくても印が付きうるため）: マクロの行（`$(…)` が何に置き換わるか分からない）、
// 自由な文を取るコマンド（say など。`don't` の ' や顔文字の括弧を書ける）。

/// 通常の文字。
pub const NORMAL: u8 = b'A';
/// `#` で始まるコメントの行。
pub const COMMENT: u8 = b'C';
/// コマンド名（行の先頭と、execute の `run` の後ろ）。
pub const COMMAND: u8 = b'D';
/// `@a` などのターゲットセレクター。
pub const SELECTOR: u8 = b'E';
/// 数値と座標（`~` `^`）。
pub const NUMBER: u8 = b'F';
/// `名前空間:パス` の名前と `#名前空間:タグ`。
pub const RESOURCE: u8 = b'G';
/// 引用符で囲んだ文字列。
pub const STRING: u8 = b'H';
/// マクロ（行頭の `$` と `$(名前)`）。
pub const MACRO: u8 = b'I';
/// 確実な誤り（閉じていない括弧・引用符、対応しない閉じ括弧）。下線で控えめに示す。
pub const PROBLEM: u8 = b'J';

/// 残りを自由な文として取るコマンド（括弧や引用符を確かめない）。
const FREE_TEXT: &[&[u8]] = &[b"say", b"me", b"tell", b"msg", b"w", b"teammsg", b"tm", b"kick", b"ban", b"ban-ip"];

/// 拡張子が .mcfunction か。
pub fn is_mcfunction(path: Option<&std::path::Path>) -> bool {
    path.and_then(|p| p.extension()).is_some_and(|e| e.eq_ignore_ascii_case("mcfunction"))
}

/// 本文の各バイトのスタイル（長さは `text.len()`）。
pub fn styles(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = vec![NORMAL; b.len()];
    let mut i = 0;
    while i < b.len() {
        // 1 つのコマンド（行末の `\` で続く行をまとめた範囲）
        let start = i;
        let mut end = i;
        loop {
            let le = b[end..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| end + p);
            let content = trim_end(b, end, le);
            if content > end && b[content - 1] == b'\\' && le < b.len() {
                end = le + 1;
                continue;
            }
            end = le;
            break;
        }
        command(b, start, end, &mut out);
        i = end + 1;
    }
    out
}

/// 行末の空白（`\r` を含む）を除いた終わりの位置。
fn trim_end(b: &[u8], s: usize, mut e: usize) -> usize {
    while e > s && matches!(b[e - 1], b' ' | b'\t' | b'\r') {
        e -= 1;
    }
    e
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'/' | b':' | b'+')
}

fn command(b: &[u8], s: usize, e: usize, out: &mut [u8]) {
    let mut i = s;
    while i < e && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= e {
        return;
    }
    if b[i] == b'#' {
        out[i..e].fill(COMMENT);
        return;
    }
    let is_macro = b[i] == b'$';
    if is_macro {
        out[i] = MACRO;
        i += 1;
    }
    let mut expect_command = true;
    // 確かめるか（マクロの行と、自由な文の後ろは確かめない）
    let mut check = !is_macro;
    // 開いている括弧の (文字, 位置)
    let mut open: Vec<(u8, usize)> = Vec::new();
    while i < e {
        let c = b[i];
        if check && matches!(c, b'[' | b'{') {
            open.push((c, i));
            i += 1;
            expect_command = false;
            continue;
        }
        if check && matches!(c, b']' | b'}') {
            let want = if c == b']' { b'[' } else { b'{' };
            match open.pop() {
                Some((o, _)) if o == want => {}
                // 対応しない閉じ括弧（中身の取り違えまで追うと誤りやすいので、この 1 文字だけ示して確かめるのをやめる）
                _ => {
                    out[i] = PROBLEM;
                    check = false;
                    open.clear();
                }
            }
            i += 1;
            continue;
        }
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'\\') {
            i += 1;
            continue;
        }
        if is_macro && c == b'$' && b.get(i + 1) == Some(&b'(') {
            let close = b[i..e].iter().position(|&x| x == b')').map_or(e, |p| i + p + 1);
            out[i..close].fill(MACRO);
            i = close;
            expect_command = false;
            continue;
        }
        if (c == b'"' || c == b'\'') && (check || is_macro) {
            let mut j = i + 1;
            while j < e && b[j] != c {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            if j >= e {
                // 閉じていない引用符: 文字列の色にはせず、開いた所だけ示す
                if check {
                    out[i] = PROBLEM;
                    check = false;
                    open.clear();
                }
                i += 1;
                continue;
            }
            out[i..=j].fill(STRING);
            i = j + 1;
            expect_command = false;
            continue;
        }
        if c == b'@' && b.get(i + 1).is_some_and(|x| matches!(x, b'a' | b'p' | b'r' | b's' | b'e' | b'n')) {
            out[i..i + 2].fill(SELECTOR);
            i += 2;
            expect_command = false;
            continue;
        }
        if c == b'~' || c == b'^' {
            let mut j = i + 1;
            while j < e && (b[j].is_ascii_digit() || matches!(b[j], b'.' | b'-')) {
                j += 1;
            }
            out[i..j].fill(NUMBER);
            i = j;
            expect_command = false;
            continue;
        }
        let tag = c == b'#' && b.get(i + 1).is_some_and(|&x| is_word(x));
        if is_word(c) || tag {
            let mut j = i + 1;
            while j < e && is_word(b[j]) {
                j += 1;
            }
            let word = &b[i..j];
            let style = if expect_command && !tag {
                COMMAND
            } else if is_number(word) {
                NUMBER
            } else if word.contains(&b':') && word.iter().any(|x| x.is_ascii_alphabetic()) {
                RESOURCE
            } else {
                NORMAL
            };
            out[i..j].fill(style);
            if style == COMMAND && FREE_TEXT.contains(&word) {
                check = false;
                open.clear();
            }
            // execute … run の後ろは、またコマンド
            expect_command = !tag && word == b"run";
            i = j;
            continue;
        }
        expect_command = false;
        i += 1;
    }
    for (_, at) in open {
        out[at] = PROBLEM;
    }
}

/// `12` `-3` `0.5` `1..5` `10b` `2.5f` などの数値か。
fn is_number(w: &[u8]) -> bool {
    let w = match w.last() {
        Some(b'b' | b's' | b'l' | b'f' | b'd' | b'B' | b'S' | b'L' | b'F' | b'D' | b't') if w.len() > 1 => &w[..w.len() - 1],
        _ => w,
    };
    !w.is_empty() && w.iter().any(|c| c.is_ascii_digit()) && w.iter().all(|&c| c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 各バイトのスタイルを文字列で返す（読みやすさのため）。
    fn st(text: &str) -> String {
        String::from_utf8(styles(text)).unwrap()
    }

    #[test]
    fn comments_commands_and_selectors() {
        assert_eq!(st("# hi\nsay @a"), "CCCCADDDAEE");
        assert_eq!(st("  #x"), "AACC");
    }

    #[test]
    fn numbers_coordinates_and_resources() {
        assert_eq!(st("tp ~1 ^ 64"), "DDAFFAFAFF");
        assert_eq!(st("give @s minecraft:stone 3"), format!("DDDDAEEA{}AF", "G".repeat(15)));
        assert_eq!(st("function #ns:t"), "DDDDDDDDAGGGGG");
    }

    #[test]
    fn strings_and_run_starts_a_command() {
        assert_eq!(st("execute run tag \"a b\""), "DDDDDDDAAAAADDDAHHHHH");
    }

    #[test]
    fn backslash_continues_the_command() {
        // 2 行目の先頭はコマンドではない（前の行の続き）
        assert_eq!(st("say \\\n  @a"), "DDDAAAAAEE");
        assert_eq!(st("say \\\r\n@a"), "DDDAAAAEE");
    }

    #[test]
    fn macro_lines() {
        assert_eq!(st("$say $(name) hi"), "IDDDAIIIIIIIAAA");
        // マクロの行でなければ $( は普通の文字
        assert_eq!(st("say $(x)"), "DDDAAAAA");
    }

    /// 印の付いた位置。
    fn problems(text: &str) -> Vec<usize> {
        styles(text).iter().enumerate().filter(|(_, &c)| c == PROBLEM).map(|(i, _)| i).collect()
    }

    #[test]
    fn unclosed_and_stray_brackets() {
        assert_eq!(problems("kill @e[type=pig"), vec![7]);
        assert_eq!(problems("give @s stone{a:1"), vec![13]);
        assert_eq!(problems("kill @e]"), vec![7]);
        assert_eq!(problems("kill @e[type=pig}"), vec![16]);
        // 閉じ忘れはコマンドごと（次の行には持ち越さない）
        assert_eq!(problems("kill @e[\nsay hi"), vec![7]);
    }

    #[test]
    fn unclosed_quote() {
        assert_eq!(problems("tellraw @a {\"text\":\"hi}"), vec![19]);
    }

    #[test]
    fn correct_commands_have_no_marks() {
        for ok in [
            "tellraw @a {\"text\":\"a]b}\\\"c\",\"extra\":[{\"text\":\"x\"}]}",
            "give @s diamond_sword[enchantments={sharpness:5},custom_name='Don\\'t']",
            "execute if block ~ ~ ~ chest[facing=north]{Items:[]} run say ok",
            "data merge entity @s {Tags:[\"a\",'b']}",
            "execute as @a \\\n  run tp @s[tag=x] ~ ~ ~",
            "# comment with [ and \"",
            "",
        ] {
            assert_eq!(problems(ok), Vec::<usize>::new(), "{ok}");
        }
    }

    #[test]
    fn free_text_and_macros_are_not_checked() {
        assert!(problems("say don't (^_^ [").is_empty());
        assert!(problems("execute as @a run say { oops").is_empty());
        assert!(problems("tell @p it's").is_empty());
        assert!(problems("$kill @e[$(sel)").is_empty());
        assert!(problems("$tellraw @a {\"text\":\"$(t)").is_empty());
    }

    #[test]
    fn length_matches_text_with_multibyte() {
        let t = "say あ\n# い";
        assert_eq!(styles(t).len(), t.len());
    }

    #[test]
    fn extension() {
        assert!(is_mcfunction(Some(std::path::Path::new("a/b.MCFUNCTION"))));
        assert!(!is_mcfunction(Some(std::path::Path::new("a.txt"))));
        assert!(!is_mcfunction(None));
    }
}
