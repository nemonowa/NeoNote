// html の Preview（htmlview.rs）に、色と揃えを持ち込むための基盤（GUI なし）。
// HelpView は CSS を読めないので、ここで「色・背景色・左右の揃え」だけを読み取り、
// htmlview が HelpView の描ける属性（body の bgcolor / text / link、font color、td の bgcolor、align）に置き換える。
//
// 読むもの: 色（#rgb・#rrggbb・#rrggbbaa・rgb()/rgba()・色の名前）、
//           <style> の単純な規則（タグ名・.class・#id・tag.class と、それをカンマで並べたもの）、style 属性、古い属性（bgcolor・color・text・link・align）。
// 読まないもの: 子孫・兄弟の指定、:hover などの擬似クラス、@media などの @ 規則、!important の順位、継承の細かい決まり。
// 読めない値は捨てる（呼び出し側はテーマの色のまま描く）。どの関数も失敗で panic しない。
pub type Rgb = (u8, u8, u8);

/// 読む CSS の上限（バイト）と規則の数の上限。重いページで Preview が止まらないように。
const MAX_CSS: usize = 200_000;
const MAX_RULES: usize = 2_000;

/// 1 つの要素に効く見た目。どれも読めなければ None。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub color: Option<Rgb>,
    pub background: Option<Rgb>,
    pub align: Option<&'static str>,
    /// a の色（body に書かれたときだけ使う）。
    pub link: Option<Rgb>,
}

impl Style {
    /// `other` に書かれている項目で上書きする（後に書かれたもの・詳しい指定が勝つ）。
    pub fn merge(&mut self, other: &Style) {
        self.color = other.color.or(self.color);
        self.background = other.background.or(self.background);
        self.align = other.align.or(self.align);
        self.link = other.link.or(self.link);
    }

    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }
}

#[derive(Debug)]
struct Rule {
    tag: Option<String>,
    class: Option<String>,
    id: Option<String>,
    style: Style,
}

impl Rule {
    /// 詳しさ（id > class > タグ）。同じなら後に書かれた規則が勝つ。
    fn rank(&self) -> u32 {
        (self.id.is_some() as u32) * 100 + (self.class.is_some() as u32) * 10 + self.tag.is_some() as u32
    }

    fn matches(&self, tag: &str, classes: &[&str], id: Option<&str>) -> bool {
        self.tag.as_deref().is_none_or(|t| t == tag)
            && self.class.as_deref().is_none_or(|c| classes.contains(&c))
            && self.id.as_deref().is_none_or(|i| Some(i) == id)
    }
}

/// `<style>` から読んだ規則。
#[derive(Debug, Default)]
pub struct Css {
    rules: Vec<Rule>,
}

impl Css {
    /// `<style>` の中身を足す（複数の `<style>` は書かれた順に足す）。読めない規則は飛ばす。
    pub fn add(&mut self, src: &str) {
        let src = &src[..floor_boundary(src, MAX_CSS)];
        let src = strip_comments(src);
        let mut rest = src.as_str();
        while let Some(open) = rest.find('{') {
            let head = rest[..open].trim();
            let Some(close) = matching_brace(rest, open) else { break };
            let body = &rest[open + 1..close];
            rest = &rest[close + 1..];
            if head.starts_with('@') {
                continue; // @media・@font-face など。中身は読まない
            }
            let style = parse_declarations(body);
            if style.is_empty() {
                continue;
            }
            for sel in head.split(',') {
                if self.rules.len() >= MAX_RULES {
                    return;
                }
                if let Some((tag, class, id)) = parse_selector(sel) {
                    self.rules.push(Rule { tag, class, id, style: style.clone() });
                }
            }
        }
    }

    /// 要素に効く見た目: 規則（詳しさの順、同じなら書かれた順）→ 古い属性 → style 属性 の順に上書きする。
    /// `tag` は小文字のタグ名、`attrs` は開きタグの属性の文字列（`attr` で引く）。
    pub fn resolve(&self, tag: &str, attrs: &dyn Fn(&str) -> Option<String>) -> Style {
        let class = attrs("class").unwrap_or_default();
        let classes: Vec<&str> = class.split_ascii_whitespace().collect();
        let id = attrs("id");
        let mut hits: Vec<(u32, usize)> =
            self.rules.iter().enumerate().filter(|(_, r)| r.matches(tag, &classes, id.as_deref())).map(|(i, r)| (r.rank(), i)).collect();
        hits.sort();
        let mut st = Style::default();
        for (_, i) in hits {
            st.merge(&self.rules[i].style);
        }
        let legacy = Style {
            color: attrs(if tag == "body" { "text" } else { "color" }).as_deref().and_then(parse_color),
            background: attrs("bgcolor").as_deref().and_then(parse_color),
            align: attrs("align").as_deref().and_then(parse_align),
            link: if tag == "body" { attrs("link").as_deref().and_then(parse_color) } else { None },
        };
        st.merge(&legacy);
        if let Some(inline) = attrs("style") {
            st.merge(&parse_declarations(&inline));
        }
        st
    }
}

fn floor_boundary(s: &str, max: usize) -> usize {
    let mut n = max.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        match rest[p + 2..].find("*/") {
            Some(q) => rest = &rest[p + 2 + q + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `{` に対応する `}`（入れ子の @media などを飛ばすため、深さを数える）。
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 単純なセレクタだけを読む: `p`・`.note`・`#top`・`td.warn`・`*`。それ以外（空白・>・+・~・[・: を含む）は None。
fn parse_selector(sel: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let sel = sel.trim();
    if sel.is_empty() || sel.contains(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~' | '[' | ':' | '(')) {
        return None;
    }
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    let (tag, rest) = match sel.find(['.', '#']) {
        Some(p) => (&sel[..p], &sel[p..]),
        None => (sel, ""),
    };
    let tag = match tag {
        "" | "*" => None,
        t if ident(t) => Some(t.to_ascii_lowercase()),
        _ => return None,
    };
    let (mut class, mut id) = (None, None);
    if !rest.is_empty() {
        let (kind, name) = rest.split_at(1);
        if !ident(name) {
            return None; // `.a.b` や `.a#b` は読まない
        }
        if kind == "." {
            class = Some(name.to_string());
        } else {
            id = Some(name.to_string());
        }
    }
    if tag.is_none() && class.is_none() && id.is_none() && sel != "*" {
        return None;
    }
    Some((tag, class, id))
}

/// `color: red; background: #fff url(x)` のような宣言の並びから、読める項目だけを取る。
pub fn parse_declarations(s: &str) -> Style {
    let mut st = Style::default();
    for decl in s.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let v = v.trim().trim_end_matches("!important").trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "color" => st.color = parse_color(v).or(st.color),
            "background-color" => st.background = parse_color(v).or(st.background),
            // background は色以外（画像・位置）も並ぶので、色として読める語を探す
            "background" => st.background = background_color(v).or(st.background),
            "text-align" => st.align = parse_align(v).or(st.align),
            _ => {}
        }
    }
    st
}

fn background_color(v: &str) -> Option<Rgb> {
    if let Some(c) = parse_color(v) {
        return Some(c);
    }
    // rgb( ... ) は中に空白があるので、先に丸ごと探す
    let low = v.to_ascii_lowercase();
    if let Some(p) = low.find("rgb") {
        if let Some(q) = low[p..].find(')') {
            return parse_color(&v[p..p + q + 1]);
        }
    }
    v.split_ascii_whitespace().find_map(parse_color)
}

fn parse_align(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Some("left"),
        "right" | "end" => Some("right"),
        "center" | "middle" => Some("center"),
        _ => None,
    }
}

/// 色を読む。読めない・透明なら None。
pub fn parse_color(v: &str) -> Option<Rgb> {
    let v = v.trim();
    let low = v.to_ascii_lowercase();
    if let Some(h) = low.strip_prefix('#') {
        return parse_hex(h);
    }
    if let Some(inner) = low.strip_prefix("rgba(").or_else(|| low.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')')) {
        return parse_rgb_fn(inner);
    }
    // 古い属性の `bgcolor="ffffff"`（# なし）
    if low.len() == 6 && low.bytes().all(|b| b.is_ascii_hexdigit()) {
        return parse_hex(&low);
    }
    named(&low)
}

fn parse_hex(h: &str) -> Option<Rgb> {
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3 | 4 => {
            if h.len() == 4 && &h[3..4] == "0" {
                return None; // 透明
            }
            let d = |i: usize| n(&h[i..i + 1]).map(|x| x * 17);
            Some((d(0)?, d(1)?, d(2)?))
        }
        6 | 8 => {
            if h.len() == 8 && &h[6..8] == "00" {
                return None;
            }
            Some((n(&h[0..2])?, n(&h[2..4])?, n(&h[4..6])?))
        }
        _ => None,
    }
}

/// `255, 0, 0` / `255 0 0 / 50%` / `100%, 0%, 0%`。
fn parse_rgb_fn(inner: &str) -> Option<Rgb> {
    let (rgb, alpha) = match inner.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (inner, None),
    };
    let parts: Vec<&str> = rgb.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).collect();
    let alpha = alpha.or_else(|| parts.get(3).copied());
    if parts.len() < 3 {
        return None;
    }
    if let Some(a) = alpha {
        let a = a.trim();
        let zero = a.trim_end_matches('%').parse::<f64>().ok().is_some_and(|x| x <= 0.0);
        if zero {
            return None;
        }
    }
    let ch = |s: &str| -> Option<u8> {
        let x = match s.strip_suffix('%') {
            Some(p) => p.parse::<f64>().ok()? * 2.55,
            None => s.parse::<f64>().ok()?,
        };
        x.is_finite().then(|| x.round().clamp(0.0, 255.0) as u8)
    };
    Some((ch(parts[0])?, ch(parts[1])?, ch(parts[2])?))
}

fn named(n: &str) -> Option<Rgb> {
    Some(match n {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "dimgray" | "dimgrey" => (105, 105, 105),
        "gainsboro" => (220, 220, 220),
        "whitesmoke" => (245, 245, 245),
        "maroon" => (128, 0, 0),
        "darkred" => (139, 0, 0),
        "olive" => (128, 128, 0),
        "navy" => (0, 0, 128),
        "darkblue" => (0, 0, 139),
        "purple" => (128, 0, 128),
        "teal" => (0, 128, 128),
        "orange" => (255, 165, 0),
        "darkorange" => (255, 140, 0),
        "gold" => (255, 215, 0),
        "pink" => (255, 192, 203),
        "hotpink" => (255, 105, 180),
        "brown" => (165, 42, 42),
        "tan" => (210, 180, 140),
        "beige" => (245, 245, 220),
        "ivory" => (255, 255, 240),
        "lavender" => (230, 230, 250),
        "violet" => (238, 130, 238),
        "indigo" => (75, 0, 130),
        "coral" => (255, 127, 80),
        "tomato" => (255, 99, 71),
        "salmon" => (250, 128, 114),
        "crimson" => (220, 20, 60),
        "khaki" => (240, 230, 140),
        "skyblue" => (135, 206, 235),
        "lightblue" => (173, 216, 230),
        "steelblue" => (70, 130, 180),
        "royalblue" => (65, 105, 225),
        "dodgerblue" => (30, 144, 255),
        "lightgreen" => (144, 238, 144),
        "darkgreen" => (0, 100, 0),
        "forestgreen" => (34, 139, 34),
        "seagreen" => (46, 139, 87),
        "lightyellow" => (255, 255, 224),
        "lightcyan" => (224, 255, 255),
        "lightpink" => (255, 182, 193),
        "aliceblue" => (240, 248, 255),
        "honeydew" => (240, 255, 240),
        "mintcream" => (245, 255, 250),
        "snow" => (255, 250, 250),
        "linen" => (250, 240, 230),
        "wheat" => (245, 222, 179),
        "chocolate" => (210, 105, 30),
        "sienna" => (160, 82, 45),
        "slategray" | "slategrey" => (112, 128, 144),
        "darkslategray" | "darkslategrey" => (47, 79, 79),
        _ => return None, // transparent・inherit・currentcolor・var(...) なども読まない
    })
}

/// 相対輝度（WCAG）。
fn luminance((r, g, b): Rgb) -> f64 {
    let f = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

/// 2 色のコントラスト比（1〜21）。
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// 背景 `bg` の上で読める文字色（黒か白）。
pub fn readable_on(bg: Rgb) -> Rgb {
    if contrast(bg, (0, 0, 0)) >= contrast(bg, (255, 255, 255)) { (0, 0, 0) } else { (255, 255, 255) }
}

/// 文字が読めるとみなすコントラスト比の下限（これ未満の文字色は捨てて、背景に合う色にする）。
pub const MIN_CONTRAST: f64 = 2.0;

pub fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn colors_in_every_notation() {
        assert_eq!(parse_color("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_color("#1E28D2"), Some((30, 40, 210)));
        assert_eq!(parse_color("#11223344"), Some((17, 34, 51)));
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some((10, 20, 30)));
        assert_eq!(parse_color("rgba(10 20 30 / 0.5)"), Some((10, 20, 30)));
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some((255, 0, 0)));
        assert_eq!(parse_color("Navy"), Some((0, 0, 128)));
        assert_eq!(parse_color("ffffff"), Some((255, 255, 255)));
    }

    #[test]
    fn unreadable_colors_are_none() {
        for v in ["", "transparent", "inherit", "var(--x)", "#12", "#ggg", "rgb(1,2)", "rgba(0,0,0,0)", "#0000", "hsl(0,0%,0%)", "rgb(a,b,c)"] {
            assert_eq!(parse_color(v), None, "{v}");
        }
    }

    #[test]
    fn declarations_take_only_what_we_can_draw() {
        let s = parse_declarations("color: red; background: url(x.png) no-repeat #eee; text-align:center; margin:0; bogus");
        assert_eq!(s.color, Some((255, 0, 0)));
        assert_eq!(s.background, Some((238, 238, 238)));
        assert_eq!(s.align, Some("center"));
        assert_eq!(parse_declarations("background: rgb(1, 2, 3) url(a)").background, Some((1, 2, 3)));
        assert!(parse_declarations("color: nope; background: url(a)").is_empty());
    }

    #[test]
    fn rules_apply_by_rank_then_order_then_inline() {
        let mut css = Css::default();
        css.add("/* c */ p { color: blue } .warn { color: red } #x { color: green } p.warn, td { background: yellow }
                 @media print { p { color: black } } div p { color: pink } a:hover { color: pink }");
        assert_eq!(css.resolve("p", &attrs(&[])).color, Some((0, 0, 255)));
        let st = css.resolve("p", &attrs(&[("class", "a warn")]));
        assert_eq!((st.color, st.background), (Some((255, 0, 0)), Some((255, 255, 0))));
        assert_eq!(css.resolve("p", &attrs(&[("class", "warn"), ("id", "x")])).color, Some((0, 128, 0)));
        assert_eq!(css.resolve("p", &attrs(&[("id", "x"), ("style", "color:#000")])).color, Some((0, 0, 0)));
        assert_eq!(css.resolve("td", &attrs(&[])).background, Some((255, 255, 0)));
        assert_eq!(css.resolve("span", &attrs(&[])), Style::default());
    }

    #[test]
    fn legacy_attributes_are_read() {
        let css = Css::default();
        let b = css.resolve("body", &attrs(&[("bgcolor", "#000000"), ("text", "white"), ("link", "#0f0")]));
        assert_eq!((b.background, b.color, b.link), (Some((0, 0, 0)), Some((255, 255, 255)), Some((0, 255, 0))));
        assert_eq!(css.resolve("font", &attrs(&[("color", "red")])).color, Some((255, 0, 0)));
        assert_eq!(css.resolve("td", &attrs(&[("align", "RIGHT")])).align, Some("right"));
    }

    #[test]
    fn broken_css_does_not_panic_or_leak() {
        let mut css = Css::default();
        css.add("p { color: red");
        css.add("}}} {{{ ::: ;;; .a.b { color: red } #{ } p { color: }");
        css.add("é { color: red } .日本 { color: blue }");
        assert_eq!(css.resolve("p", &attrs(&[])).color, None);
        assert_eq!(css.resolve("x", &attrs(&[("class", "日本")])).color, Some((0, 0, 255)));
    }

    #[test]
    fn contrast_and_readable_text() {
        assert!(contrast((0, 0, 0), (255, 255, 255)) > 20.0);
        assert!((contrast((10, 10, 10), (10, 10, 10)) - 1.0).abs() < 1e-9);
        assert_eq!(readable_on((255, 255, 200)), (0, 0, 0));
        assert_eq!(readable_on((20, 20, 60)), (255, 255, 255));
    }
}
