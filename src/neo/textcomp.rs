// Minecraft のテキスト部品（`{"text":"…","color":"red"}`・SNBT の `{text:'…',bold:true}`、`extra`・配列つき）を
// 色と装飾つきの文字の並びにする（GUI なし）。マウスを乗せたときの小窓が、見た目の見本を出すのに使う。
// 文字どおりの `text` だけを出す。`translate`・`score`・`selector` などは、そのキーを `[…]` で出す。
use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub color: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
    pub obfuscated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq)]
enum V {
    Str(String),
    Bool(bool),
    Num,
    Obj(Vec<(String, V)>),
    Arr(Vec<V>),
}

/// 長い行は前後だけを見る。
const WINDOW: usize = 2048;
/// 出す文字の上限（小窓に収める）。
const MAX_CHARS: usize = 200;

/// `line` の `at` を含むテキスト部品（`text` / `translate` を持つ一番外側の `{…}`）の範囲と、その見た目。
pub fn at(line: &str, at: usize) -> Option<(Range<usize>, Vec<Span>)> {
    if at >= line.len() || line.len() > WINDOW * 8 {
        return None;
    }
    let b = line.as_bytes();
    // `at` を含む `{` の候補を、外側から順に試す
    let mut starts = Vec::new();
    let mut depth_at = Vec::new(); // 各 `{` の位置（スタック）
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'{' {
            depth_at.push(i);
        } else if c == b'}' {
            if let Some(s) = depth_at.pop() {
                if s <= at && at <= i {
                    starts.push((s, i + 1));
                }
            }
        }
        i += 1;
    }
    // 外側（長い）から
    starts.sort_by_key(|&(s, e)| std::cmp::Reverse(e - s));
    for (s, e) in starts {
        let Some(v) = parse(&line[s..e]) else { continue };
        if let V::Obj(kv) = &v {
            if kv.iter().any(|(k, _)| k == "text" || k == "translate") {
                let mut out = Vec::new();
                flatten(&v, &Style::default(), &mut out);
                if out.iter().any(|s| !s.text.is_empty()) {
                    return Some((s..e, out));
                }
            }
        }
    }
    None
}

fn parse(s: &str) -> Option<V> {
    let mut p = P { b: s.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    (p.i == p.b.len()).then_some(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn value(&mut self) -> Option<V> {
        self.ws();
        match *self.b.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut kv = Vec::new();
                if self.eat(b'}') {
                    return Some(V::Obj(kv));
                }
                loop {
                    let k = self.key()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    kv.push((k, self.value()?));
                    if self.eat(b',') {
                        continue;
                    }
                    return self.eat(b'}').then_some(V::Obj(kv));
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                if self.eat(b']') {
                    return Some(V::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    if self.eat(b',') {
                        continue;
                    }
                    return self.eat(b']').then_some(V::Arr(a));
                }
            }
            b'"' | b'\'' => self.string().map(V::Str),
            _ => {
                let w = self.word()?;
                match w.as_str() {
                    "true" | "1b" => Some(V::Bool(true)),
                    "false" | "0b" => Some(V::Bool(false)),
                    _ if w.bytes().next().is_some_and(|c| c.is_ascii_digit() || c == b'-') => Some(V::Num),
                    _ => Some(V::Str(w)), // SNBT の引用符なしの文字列
                }
            }
        }
    }
    fn key(&mut self) -> Option<String> {
        self.ws();
        match self.b.get(self.i)? {
            b'"' | b'\'' => self.string(),
            _ => self.word(),
        }
    }
    fn word(&mut self) -> Option<String> {
        self.ws();
        let s = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || matches!(self.b[self.i], b'_' | b'-' | b'.' | b'+')) {
            self.i += 1;
        }
        (self.i > s).then(|| String::from_utf8_lossy(&self.b[s..self.i]).into_owned())
    }
    fn string(&mut self) -> Option<String> {
        let q = self.b[self.i];
        self.i += 1;
        let mut out = Vec::new();
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            if c == q {
                return String::from_utf8(out).ok();
            }
            if c == b'\\' {
                let e = *self.b.get(self.i)?;
                self.i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let h = std::str::from_utf8(self.b.get(self.i..self.i + 4)?).ok()?;
                        self.i += 4;
                        let ch = char::from_u32(u32::from_str_radix(h, 16).ok()?)?;
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        None
    }
}

/// Minecraft の色の名前。
pub fn named_color(name: &str) -> Option<(u8, u8, u8)> {
    Some(match name {
        "black" => (0x00, 0x00, 0x00),
        "dark_blue" => (0x00, 0x00, 0xAA),
        "dark_green" => (0x00, 0xAA, 0x00),
        "dark_aqua" => (0x00, 0xAA, 0xAA),
        "dark_red" => (0xAA, 0x00, 0x00),
        "dark_purple" => (0xAA, 0x00, 0xAA),
        "gold" => (0xFF, 0xAA, 0x00),
        "gray" => (0xAA, 0xAA, 0xAA),
        "dark_gray" => (0x55, 0x55, 0x55),
        "blue" => (0x55, 0x55, 0xFF),
        "green" => (0x55, 0xFF, 0x55),
        "aqua" => (0x55, 0xFF, 0xFF),
        "red" => (0xFF, 0x55, 0x55),
        "light_purple" => (0xFF, 0x55, 0xFF),
        "yellow" => (0xFF, 0xFF, 0x55),
        "white" => (0xFF, 0xFF, 0xFF),
        _ => {
            let h = name.strip_prefix('#')?;
            if h.len() != 6 {
                return None;
            }
            let v = u32::from_str_radix(h, 16).ok()?;
            ((v >> 16) as u8, (v >> 8) as u8, v as u8)
        }
    })
}

fn flatten(v: &V, parent: &Style, out: &mut Vec<Span>) {
    if out.iter().map(|s| s.text.chars().count()).sum::<usize>() >= MAX_CHARS {
        return;
    }
    match v {
        V::Str(s) => out.push(Span { text: s.clone(), style: parent.clone() }),
        V::Arr(a) => {
            // 配列は、最初の要素の書式を後ろへ引き継ぐ（Minecraft と同じ）
            let mut base = parent.clone();
            for (k, x) in a.iter().enumerate() {
                flatten(x, &base, out);
                if k == 0 {
                    if let V::Obj(kv) = x {
                        base = style_of(kv, parent);
                    }
                }
            }
        }
        V::Obj(kv) => {
            let st = style_of(kv, parent);
            let get = |k: &str| kv.iter().find(|(n, _)| n == k).map(|(_, v)| v);
            let text = match (get("text"), get("translate")) {
                (Some(V::Str(t)), _) => Some(t.clone()),
                (_, Some(V::Str(t))) => Some(format!("[{t}]")),
                _ => ["score", "selector", "keybind", "nbt"].iter().find(|k| get(k).is_some()).map(|k| format!("[{k}]")),
            };
            if let Some(t) = text {
                out.push(Span { text: t, style: st.clone() });
            }
            if let Some(V::Arr(extra)) = get("extra") {
                for x in extra {
                    flatten(x, &st, out);
                }
            }
        }
        _ => {}
    }
}

fn style_of(kv: &[(String, V)], parent: &Style) -> Style {
    let mut st = parent.clone();
    for (k, v) in kv {
        match (k.as_str(), v) {
            ("color", V::Str(c)) => st.color = named_color(c).or(st.color),
            ("bold", V::Bool(b)) => st.bold = *b,
            ("italic", V::Bool(b)) => st.italic = *b,
            ("underlined", V::Bool(b)) => st.underlined = *b,
            ("strikethrough", V::Bool(b)) => st.strikethrough = *b,
            ("obfuscated", V::Bool(b)) => st.obfuscated = *b,
            _ => {}
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_component_with_extra() {
        let line = r##"tellraw @a {"text":"Hi ","color":"gold","bold":true,"extra":[{"text":"you","color":"#00ff00","bold":false}]}"##;
        let (r, s) = at(line, line.find("Hi").unwrap()).unwrap();
        assert_eq!(&line[r], &line[11..]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].style.color, Some((0xFF, 0xAA, 0x00)));
        assert!(s[0].style.bold);
        assert_eq!(s[1].style.color, Some((0, 255, 0)));
        assert!(!s[1].style.bold);
    }

    #[test]
    fn snbt_component_and_inner_cursor_picks_outermost() {
        let line = "give @s stick[item_name={text:'剣',color:red,italic:false,extra:[{text:\"!\"}]}]";
        let (_, s) = at(line, line.find('!').unwrap()).unwrap();
        assert_eq!(s.iter().map(|x| x.text.as_str()).collect::<String>(), "剣!");
        assert_eq!(s[1].style.color, Some((0xFF, 0x55, 0x55))); // 親の色を引き継ぐ
    }

    #[test]
    fn non_components_and_broken_input_give_none() {
        assert!(at(r#"{"a":1}"#, 2).is_none());
        assert!(at(r#"{"text":"x""#, 3).is_none());
        assert!(at("plain", 1).is_none());
        let (_, s) = at(r#"{"translate":"item.minecraft.stick"}"#, 3).unwrap();
        assert_eq!(s[0].text, "[item.minecraft.stick]");
    }
}
