// html / htm → FLTK HelpView 用の HTML（GUI なし）。読み取り専用の簡易対応: Preview で見るだけで、編集は Text で行う。
// HelpView は HTML の小さな部分集合しか描けず、CSS・JS は効かない。そこで、ファイルの HTML を
// 「HelpView が描けるタグだけ」に作り直す（タグの対応を取り直し、描けないものは中身の文字だけ残す）。
//
// 出すもの: 見出し・段落・リスト・表（罫線つき）・太字 / 斜体 / 下線・コード・pre・引用・中央寄せ・ローカルの画像。
// 色と揃え: htmlstyle.rs が <style>・style 属性・古い属性から読んだ分だけ、HelpView の属性（body の bgcolor、font color、td の bgcolor、align）にして出す。
//   読めない値は捨ててテーマの色のまま描く。文字が背景に溶けて読めなくなる色は使わず、読める色（黒か白）にする。
// 出さないもの: JS・フォント・レイアウト（div は改行になるだけ）、フォーム、動画・音声・iframe・svg、head の中身（title を含む）。
// リンクは辿らせない（HelpView は辿るとファイルとして読みに行って失敗する）。文字として残し、リンク先は「 (URL)」で後ろに添える。
// 文字コードは NeoNote が開くときに決めたものに従う（`<meta charset>` は見ない）。
use super::htmlstyle::{self, Css, Rgb, Style};
use super::imagefile;
use super::markdown::Images;

/// 読む HTML の上限（バイト）。超えた分は切って注記する。
const MAX_IN: usize = 4_000_000;
/// 出す HTML の上限（バイト）。HelpView が重くならないように。
const MAX_OUT: usize = 3_000_000;
/// タグの入れ子の上限（超えた分のタグは捨てる）。
const MAX_DEPTH: usize = 100;
/// 画像を読む枚数の上限。
const MAX_IMAGES: usize = 100;

const TABLE_ATTRS: &str = " border=\"1\" cellspacing=\"0\" cellpadding=\"4\"";

/// 中身ごと捨てる要素（開きタグから対応する閉じタグまで）。
const SKIP: &[&str] = &[
    "script", "style", "head", "title", "template", "noscript", "svg", "iframe", "object", "embed", "math", "select", "datalist", "canvas",
    "audio", "video", "caption",
];

/// HelpView では描けないが、前後で改行になるべき要素（中の文字は残す）。
const BREAKS: &[&str] = &[
    "div", "section", "article", "header", "footer", "nav", "main", "aside", "figure", "figcaption", "form", "fieldset", "details", "summary",
    "address", "legend", "hgroup",
];

/// 段落（p）が開いていても、その外側までは閉じに行かない境界。
const P_BARRIER: &[&str] = &["table", "tr", "td", "th", "li", "dd", "blockquote", "center", "ul", "ol", "dl"];

/// タグ名 → HelpView に渡すタグ名。描けるものだけ（なければ None = タグは捨てて中の文字だけ残す）。
fn keep_name(n: &str) -> Option<&'static str> {
    Some(match n {
        "b" | "strong" => "b",
        "i" | "em" | "cite" | "dfn" => "i",
        "u" | "ins" => "u",
        "code" | "tt" | "samp" => "code",
        "kbd" => "kbd",
        "var" => "var",
        "pre" => "pre",
        "h1" => "h1",
        "h2" => "h2",
        "h3" => "h3",
        "h4" => "h4",
        "h5" => "h5",
        "h6" => "h6",
        "p" => "p",
        "ul" | "menu" | "dir" => "ul",
        "ol" => "ol",
        "li" => "li",
        "dl" => "dl",
        "dt" => "dt",
        "dd" => "dd",
        "table" => "table",
        "tr" => "tr",
        "td" => "td",
        "th" => "th",
        "center" => "center",
        "blockquote" => "blockquote",
        _ => return None,
    })
}

/// 改行のあとに閉じる（段落の切れ目になる）タグか。
fn is_block(k: &str) -> bool {
    !matches!(k, "b" | "i" | "u" | "code" | "kbd" | "var" | "font")
}

fn wide(c: char) -> bool {
    c as u32 >= 0x2E80
}

/// `&name;` / `&#NN;` / `&#xHH;` の名前の部分から文字を決める（知らない名前は None）。
fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let v = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        return char::from_u32(v).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" | "thinsp" => ' ',
        "emsp" => '\u{3000}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "middot" => '·',
        "bull" => '•',
        "yen" => '¥',
        "euro" => '€',
        "pound" => '£',
        "cent" => '¢',
        "sect" => '§',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "frac12" => '½',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "hearts" => '♥',
        _ => return None,
    })
}

/// 文字参照を文字に戻す。知らない名前・`;` の無いものは `&` のまま残す。
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        o.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        let ab = after.as_bytes();
        let mut n = 0;
        while n < ab.len() && n < 12 && (ab[n].is_ascii_alphanumeric() || ab[n] == b'#') {
            n += 1;
        }
        if n > 0 && n < ab.len() && ab[n] == b';' {
            if let Some(ch) = entity(&after[..n]) {
                o.push(ch);
                rest = &after[n + 1..];
                continue;
            }
        }
        o.push('&');
        rest = after;
    }
    o.push_str(rest);
    o
}

/// HelpView に渡す文字として 1 文字を足す（`&` `<` `>` と no-break space だけ置き換える）。
fn put(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '\u{a0}' => out.push_str("&nbsp;"),
        c => out.push(c),
    }
}

fn escape_into(s: &str, out: &mut String) {
    for c in s.chars() {
        put(out, c);
    }
}

/// 属性の値を取る（名前は小文字で渡す）。`a="x"` / `a='x'` / `a=x` / 値なし（空文字）を読む。値の文字参照は戻す。
fn attr(attrs: &str, name: &str) -> Option<String> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let key = attrs[s..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = attrs[vs..i].to_string();
                if i < b.len() {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                val = attrs[vs..i].to_string();
            }
        }
        if !key.is_empty() && key == name {
            return Some(decode_entities(&val));
        }
        if s == i {
            i += 1; // 進まないときの保険
        }
    }
    None
}

/// 開きタグの `>` の位置（`from` はタグ名の先頭）。`=` の直後の引用符の中の `>` は閉じとみなさない。
fn tag_end(s: &str, from: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut q = 0u8;
    let mut prev = 0u8;
    for (i, &c) in b.iter().enumerate().skip(from) {
        if q != 0 {
            if c == q {
                q = 0;
            }
        } else if (c == b'"' || c == b'\'') && prev == b'=' {
            q = c;
        } else if c == b'>' {
            return Some(i);
        }
        if !c.is_ascii_whitespace() {
            prev = c;
        }
    }
    None
}

struct Out {
    s: String,
    /// いま開いている（出力に開きタグを書いた）要素。
    stack: Vec<&'static str>,
    /// stack と同じ並びで、その要素を開かせた元のタグ名（span の色のための font なら "span"）。
    owners: Vec<String>,
    /// 色を付けた要素の (stack の位置, 背景色) と (stack の位置, 文字色)。閉じたら外す。
    bgs: Vec<(usize, Rgb)>,
    fgs: Vec<(usize, Rgb)>,
    css: Css,
    /// html・body に書かれた見た目（`<style>` の html・body の規則は page_css。body のタグが無いページにも効かせる）。
    page: Style,
    page_css: Style,
    /// テーマの背景色・文字色・リンクの色。
    theme: (Rgb, Rgb, Rgb),
    /// 開いている `<a>` の (href, 出力のその時点の長さ)。
    links: Vec<(String, usize)>,
    /// 直前が改行（または先頭）か。改行を重ねないために持つ。
    brk: bool,
    /// 見える文字・画像を 1 つでも出したか。
    seen: bool,
    /// 開いたばかりのセルがまだ空か（空なら閉じるときに `&nbsp;` を入れて罫線を描かせる）。
    cell_empty: bool,
    /// 直前に出した文字（空白の畳み込みと、日本語の改行の詰めに使う）。
    last: Option<char>,
    images: usize,
    truncated: bool,
}

impl Out {
    fn new(theme: (Rgb, Rgb, Rgb)) -> Out {
        Out { s: String::new(), stack: Vec::new(), owners: Vec::new(), bgs: Vec::new(), fgs: Vec::new(), css: Css::default(), page: Style::default(), page_css: Style::default(), theme, links: Vec::new(), brk: true, seen: false, cell_empty: false, last: None, images: 0, truncated: false }
    }

    fn in_table_gap(&self) -> bool {
        matches!(self.stack.last(), Some(&"table") | Some(&"tr"))
    }

    fn mark_visible(&mut self) {
        self.seen = true;
        self.brk = false;
        self.cell_empty = false;
    }

    fn text(&mut self, raw: &str) {
        if self.in_table_gap() {
            return; // セルの外の文字（ブラウザなら表の前へ追い出される）。出さない
        }
        let t = decode_entities(raw);
        if self.stack.contains(&"pre") {
            for c in t.chars() {
                if c == '\r' || (c.is_control() && c != '\n' && c != '\t') {
                    continue;
                }
                put(&mut self.s, c);
                if !c.is_whitespace() {
                    self.mark_visible();
                }
            }
            return;
        }
        let mut pending: Option<bool> = None; // Some(改行を含むか)
        for c in t.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                pending = Some(pending.unwrap_or(false) || c == '\n');
                continue;
            }
            if c.is_control() {
                continue;
            }
            if let Some(nl) = pending.take() {
                // 改行を含む空白の前後が日本語などの幅広い文字なら詰める（ブラウザと同じ）
                let join = match self.last {
                    None => false,
                    Some(l) => !(nl && wide(l) && wide(c)),
                };
                if join {
                    self.s.push(' ');
                }
            }
            put(&mut self.s, c);
            self.mark_visible();
            self.last = Some(c);
        }
        if pending.is_some() && self.last.is_some() {
            self.s.push(' ');
        }
    }

    fn br(&mut self) {
        if !self.in_table_gap() {
            self.s.push_str("<br>");
        }
        self.brk = true;
        self.last = None;
    }

    fn soft_break(&mut self) {
        if !self.brk && !self.s.is_empty() && !self.in_table_gap() {
            self.s.push_str("<br>");
        }
        self.brk = true;
        self.last = None;
    }

    fn hr(&mut self) {
        self.close_until(&["p"], P_BARRIER);
        if !self.in_table_gap() {
            self.s.push_str("<hr>");
        }
        self.brk = true;
        self.last = None;
    }

    /// 一番上の要素を閉じる。
    fn close_top(&mut self) {
        let Some(name) = self.stack.pop() else { return };
        self.owners.pop();
        let depth = self.stack.len();
        self.bgs.retain(|(i, _)| *i < depth);
        self.fgs.retain(|(i, _)| *i < depth);
        if matches!(name, "td" | "th") && self.cell_empty {
            self.s.push_str("&nbsp;");
        }
        self.cell_empty = false;
        self.s.push_str("</");
        self.s.push_str(name);
        self.s.push('>');
        if is_block(name) {
            self.brk = true;
            self.last = None;
        }
    }

    /// 上から探して、`targets` のどれかが見つかったら、それ（と上に載っているもの）を閉じる。
    /// 先に `barriers` に当たったら何もしない（外側の要素までは閉じに行かない）。
    fn close_until(&mut self, targets: &[&str], barriers: &[&str]) {
        for idx in (0..self.stack.len()).rev() {
            let t = self.stack[idx];
            if targets.contains(&t) {
                while self.stack.len() > idx {
                    self.close_top();
                }
                return;
            }
            if barriers.contains(&t) {
                return;
            }
        }
    }

    /// `owner` は元のタグ名（閉じタグが来たときに、どの要素の分かを見分ける）。
    fn open_for(&mut self, name: &'static str, extra: &str, owner: &str) {
        // 閉じタグの省略（`<li>a<li>b`・`<p>a<p>b`・`<td>` など）を補う
        match name {
            "li" => self.close_until(&["li"], &["ul", "ol"]),
            "dt" | "dd" => self.close_until(&["dt", "dd"], &["dl"]),
            "tr" => self.close_until(&["tr"], &["table"]),
            "td" | "th" => self.close_until(&["td", "th"], &["tr", "table"]),
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "dl" | "table" | "pre" | "blockquote" | "center" => {
                self.close_until(&["p"], P_BARRIER)
            }
            _ => {}
        }
        if self.stack.len() >= MAX_DEPTH {
            return;
        }
        self.stack.push(name);
        self.owners.push(owner.to_string());
        self.s.push('<');
        self.s.push_str(name);
        self.s.push_str(extra);
        self.s.push('>');
        if is_block(name) {
            self.brk = true;
            self.last = None;
        }
        if matches!(name, "td" | "th") {
            self.cell_empty = true;
        }
    }

    fn end(&mut self, name: &str) {
        if name == "a" {
            self.link_close();
            return;
        }
        let Some(k) = keep_name(name) else {
            self.close_owned(name);
            if BREAKS.contains(&name) {
                self.soft_break();
            }
            return;
        };
        match k {
            "td" | "th" => self.close_until(&["td", "th"], &["tr", "table"]),
            "tr" => self.close_until(&["tr"], &["table"]),
            "table" => self.close_until(&["table"], &[]),
            "p" | "li" | "dt" | "dd" => self.close_until(&[k], &["table", "td", "th", "ul", "ol", "dl", "blockquote"]),
            _ => self.close_until(&[k], &["td", "th", "table"]),
        }
    }

    /// ページの (背景, 文字, リンク) の色。読めない・読めなくなる色はテーマか、背景に合う黒・白にする。
    fn page_colors(&self) -> (Rgb, Rgb, Rgb) {
        let (tbg, tfg, tlink) = self.theme;
        let readable = |c: Rgb, bg: Rgb| htmlstyle::contrast(c, bg) >= htmlstyle::MIN_CONTRAST;
        let mut page = self.page_css.clone();
        page.merge(&self.page);
        let (bg, fg) = match (page.background, page.color) {
            (Some(bg), Some(fg)) if readable(fg, bg) => (bg, fg),
            (Some(bg), _) => (bg, if readable(tfg, bg) { tfg } else { htmlstyle::readable_on(bg) }),
            (None, Some(fg)) if readable(fg, tbg) => (tbg, fg),
            (None, Some(fg)) => (htmlstyle::readable_on(fg), fg),
            (None, None) => (tbg, tfg),
        };
        let link = [page.link, Some(tlink)].into_iter().flatten().find(|&c| readable(c, bg)).unwrap_or(fg);
        (bg, fg, link)
    }

    /// `<style>` を読んだあと: html・body の規則を page_css に取り直す。
    fn refresh_page_css(&mut self) {
        let none = |_: &str| None;
        let mut st = self.css.resolve("html", &none);
        st.merge(&self.css.resolve("body", &none));
        self.page_css = st;
    }

    fn current_bg(&self) -> Rgb {
        self.bgs.last().map_or_else(|| self.page_colors().0, |b| b.1)
    }

    fn current_fg(&self) -> Rgb {
        self.fgs.last().map_or_else(|| self.page_colors().1, |f| f.1)
    }

    /// 開いたばかりの要素（stack の一番上）の中を、`st` の文字色にする。
    /// 背景を変えた要素では、文字色が無くても、今の文字色が読めなくなるなら読める色にする。
    fn apply_color(&mut self, st: &Style, owner: &str, bg_changed: bool) {
        let bg = self.current_bg();
        let color = match st.color {
            Some(c) if htmlstyle::contrast(c, bg) >= htmlstyle::MIN_CONTRAST => Some(c),
            Some(_) => Some(htmlstyle::readable_on(bg)),
            None if bg_changed && htmlstyle::contrast(self.current_fg(), bg) < htmlstyle::MIN_CONTRAST => Some(htmlstyle::readable_on(bg)),
            None => None,
        };
        if let Some(c) = color.filter(|&c| c != self.current_fg()) {
            let depth = self.stack.len();
            self.open_for("font", &format!(" color=\"{}\"", htmlstyle::hex(c)), owner);
            if self.stack.len() > depth {
                self.fgs.push((self.stack.len() - 1, c));
            }
        }
    }

    /// 元のタグ `owner` のために開いた要素（色の font・揃えの center）を閉じる。表や項目の外までは閉じに行かない。
    fn close_owned(&mut self, owner: &str) {
        for idx in (0..self.stack.len()).rev() {
            if self.owners[idx] == owner && matches!(self.stack[idx], "font" | "center") {
                while self.stack.len() > idx {
                    self.close_top();
                }
                return;
            }
            if matches!(self.stack[idx], "td" | "th" | "tr" | "table" | "li" | "dd" | "blockquote") {
                return;
            }
        }
    }

    fn link_open(&mut self, attrs: &str) {
        let href = attr(attrs, "href").unwrap_or_default();
        self.links.push((href, self.s.len()));
    }

    fn link_close(&mut self) {
        let Some((href, at)) = self.links.pop() else { return };
        let h = href.trim();
        let low = h.to_ascii_lowercase();
        if h.is_empty() || h.starts_with('#') || low.starts_with("javascript:") || low.starts_with("data:") || h.chars().count() > 200 {
            return;
        }
        let mut e = String::new();
        escape_into(h, &mut e);
        let same = self.s.get(at..).is_some_and(|shown| shown.trim() == e); // 文字がそのままリンク先なら、重ねて出さない
        if !same && !self.in_table_gap() {
            self.s.push_str(" (");
            self.s.push_str(&e);
            self.s.push(')');
        }
    }

    fn image(&mut self, attrs: &str, imgs: &Images) {
        let src = attr(attrs, "src").unwrap_or_default();
        if self.images < MAX_IMAGES && !self.in_table_gap() {
            if let Some(p) = imagefile::resolve_local(imgs.base, &src) {
                self.images += 1;
                let tag = imagefile::img_tag(&p, (imgs.size_of)(&p), imgs.max_w);
                self.s.push_str(&tag);
                self.mark_visible();
                self.last = Some('\u{1}');
                return;
            }
        }
        // 読み込まない（URL・見つからない・画像ではない）。alt だけを角括弧で出す
        let alt = attr(attrs, "alt").unwrap_or_default();
        let alt = alt.trim();
        if !alt.is_empty() {
            self.text(&format!("[{alt}]"));
        }
    }

    /// 開きタグに添える属性。表の罫線、セルの colspan / rowspan / align だけを通す。
    fn extra_attrs(&self, name: &str, attrs: &str) -> String {
        match name {
            "table" => TABLE_ATTRS.to_string(),
            "td" | "th" => {
                let mut e = String::new();
                for key in ["colspan", "rowspan"] {
                    if let Some(n) = attr(attrs, key).and_then(|v| v.trim().parse::<u32>().ok()) {
                        if (1..=100).contains(&n) {
                            e.push_str(&format!(" {key}=\"{n}\""));
                        }
                    }
                }
                if let Some(a) = attr(attrs, "align") {
                    let a = a.trim().to_ascii_lowercase();
                    if matches!(a.as_str(), "left" | "right" | "center") {
                        e.push_str(&format!(" align=\"{a}\""));
                    }
                }
                e
            }
            _ => String::new(),
        }
    }
}

/// 読む長さを上限で切る（文字の途中では切らない）。切ったら true。
fn limit(src: &str) -> (&str, bool) {
    if src.len() <= MAX_IN {
        return (src, false);
    }
    let mut n = MAX_IN;
    while !src.is_char_boundary(n) {
        n -= 1;
    }
    (&src[..n], true)
}

/// 捨てる範囲（head の中）にある `<style>` だけを読む。`low` は `src` の ASCII を小文字にしたもの。
fn add_styles_in(css: &mut Css, src: &str, low: &str) {
    let mut at = 0;
    while let Some(p) = low[at..].find("<style") {
        let Some(gt) = low[at + p..].find('>') else { return };
        let body = at + p + gt + 1;
        let end = low[body..].find("</style").map_or(low.len(), |q| body + q);
        css.add(&src[body..end]);
        at = end;
        if at >= low.len() {
            return;
        }
    }
}

/// 本文の HTML を HelpView 向けに作り直す。戻り値は (本体, 見える内容があるか, 途中で切ったか)。
fn convert(src: &str, imgs: &Images, theme: (Rgb, Rgb, Rgb)) -> (String, bool, bool, (Rgb, Rgb, Rgb)) {
    let (src, cut_in) = limit(src);
    let low = src.to_ascii_lowercase(); // ASCII だけ小文字にするので、バイト位置は src と同じ
    let b = src.as_bytes();
    let mut o = Out::new(theme);
    let mut i = 0;
    while i < b.len() {
        if o.s.len() > MAX_OUT {
            o.truncated = true;
            break;
        }
        let Some(rel) = src[i..].find('<') else {
            o.text(&src[i..]);
            break;
        };
        let lt = i + rel;
        if lt > i {
            o.text(&src[i..lt]);
        }
        if low[lt..].starts_with("<!--") {
            i = match low[lt + 4..].find("-->") {
                Some(p) => lt + 4 + p + 3,
                None => b.len(),
            };
            continue;
        }
        let c1 = b.get(lt + 1).copied().unwrap_or(0);
        let closing = c1 == b'/';
        let name_at = lt + if closing { 2 } else { 1 };
        let c2 = b.get(name_at).copied().unwrap_or(0);
        if !c2.is_ascii_alphabetic() {
            if c1 == b'!' || c1 == b'?' {
                // <!DOCTYPE ...> / <?xml ...?> / <![CDATA[ ... ]]> の頭。`>` まで読み飛ばす
                i = match src[lt..].find('>') {
                    Some(p) => lt + p + 1,
                    None => b.len(),
                };
            } else {
                o.text("<"); // タグではない `<`（`a < b`）
                i = lt + 1;
            }
            continue;
        }
        let Some(end) = tag_end(src, name_at) else {
            o.text(&src[lt..]);
            break;
        };
        let mut ne = name_at;
        while ne < end && b[ne].is_ascii_alphanumeric() {
            ne += 1;
        }
        let name = &low[name_at..ne];
        let attrs = &src[ne..end];
        i = end + 1;
        if closing {
            o.end(name);
            continue;
        }
        if SKIP.contains(&name) {
            if !attrs.trim_end().ends_with('/') {
                let pat = format!("</{name}");
                match low[i..].find(&pat) {
                    Some(p) => {
                        let from = i + p;
                        if name == "style" {
                            o.css.add(&src[i..from]);
                        } else if name == "head" {
                            add_styles_in(&mut o.css, &src[i..from], &low[i..from]);
                        }
                        o.refresh_page_css();
                        i = match src[from..].find('>') {
                            Some(q) => from + q + 1,
                            None => b.len(),
                        };
                    }
                    // 閉じが無い: script / style は終わりまで捨てる。head は閉じを省ける要素なのでタグだけ捨てる
                    None => {
                        if name != "head" {
                            i = b.len();
                        }
                    }
                }
            }
            continue;
        }
        match name {
            "br" => o.br(),
            "hr" => o.hr(),
            "img" => o.image(attrs, imgs),
            "a" => o.link_open(attrs),
            "html" | "body" => {
                let st = o.css.resolve(name, &|k| attr(attrs, k));
                o.page.merge(&st);
            }
            _ => {
                let st = o.css.resolve(name, &|k| attr(attrs, k));
                if let Some(k) = keep_name(name) {
                    let mut extra = o.extra_attrs(k, attrs);
                    // 背景は表・行・セルだけ（HelpView はほかの要素の背景を描けない）。揃えは段落・見出し・セル
                    let bg = st.background.filter(|_| matches!(k, "table" | "tr" | "td" | "th"));
                    if let Some(c) = bg {
                        extra.push_str(&format!(" bgcolor=\"{}\"", htmlstyle::hex(c)));
                    }
                    if let Some(a) = st.align.filter(|_| matches!(k, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th") && !extra.contains(" align=")) {
                        extra.push_str(&format!(" align=\"{a}\""));
                    }
                    let depth = o.stack.len();
                    o.open_for(k, &extra, name);
                    if o.stack.len() > depth {
                        if let Some(c) = bg {
                            o.bgs.push((o.stack.len() - 1, c));
                        }
                        if !matches!(k, "table" | "tr") {
                            o.apply_color(&st, name, bg.is_some());
                        }
                    }
                } else {
                    if BREAKS.contains(&name) {
                        o.soft_break();
                    }
                    if !o.in_table_gap() {
                        // div などの中央寄せは center で、span・font などの色は font で包む
                        if st.align == Some("center") && BREAKS.contains(&name) {
                            o.open_for("center", "", name);
                        }
                        o.apply_color(&st, name, false);
                    }
                }
            }
        }
    }
    while !o.stack.is_empty() {
        o.close_top();
    }
    let page = o.page_colors();
    (o.s, o.seen, o.truncated || cut_in, page)
}

/// html を、HelpView に渡す完全な HTML にする。色は `#rrggbb`。
/// ページの色（body・html の指定）が読めればそれを、読めなければテーマの色を使う。
pub fn to_html(src: &str, bg: &str, fg: &str, link: &str, sub: &str, imgs: &Images) -> String {
    let c = |s: &str, d: Rgb| htmlstyle::parse_color(s).unwrap_or(d);
    let theme = (c(bg, (255, 255, 255)), c(fg, (0, 0, 0)), c(link, (0, 0, 238)));
    let (mut body, seen, cut, (pbg, pfg, plink)) = convert(src, imgs, theme);
    let (bg, fg, link) = (htmlstyle::hex(pbg), htmlstyle::hex(pfg), htmlstyle::hex(plink));
    if !seen {
        body = "<p>表示できる内容がありません（本文が空、または script・style などだけです）。</p>".to_string();
    }
    if cut {
        body.push_str(&format!("<p><font color=\"{sub}\">── 長いため途中までを表示しています。全体は Text で見られます ──</font></p>"));
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\" link=\"{link}\">{body}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn conv(h: &str) -> String {
        let f = |_: &Path| -> Option<(i32, i32)> { None };
        let imgs = Images { base: None, max_w: 400, size_of: &f };
        convert(h, &imgs, ((17, 17, 17), (238, 238, 238), (51, 153, 255))).0
    }

    fn page(h: &str) -> String {
        let f = |_: &Path| -> Option<(i32, i32)> { None };
        let imgs = Images { base: None, max_w: 400, size_of: &f };
        to_html(h, "#111111", "#eeeeee", "#3399ff", "#888888", &imgs)
    }

    #[test]
    fn page_colors_come_from_body_style_and_attributes() {
        assert!(page("<body bgcolor=white text=navy><p>a</p></body>").starts_with("<html><body bgcolor=\"#ffffff\" text=\"#000080\""));
        let r = page("<style>body { background: #fffff0 }</style><body><p>a</p>");
        // 背景だけ: テーマの明るい文字色は読めないので、黒にする
        assert!(r.starts_with("<html><body bgcolor=\"#fffff0\" text=\"#000000\""), "{r}");
        // 文字色だけで、テーマの背景では読めない: 背景を文字に合う色にする
        let r = page("<body style=\"color:#222\"><p>a</p>");
        assert!(r.starts_with("<html><body bgcolor=\"#ffffff\" text=\"#222222\""), "{r}");
        // 読めない値はテーマのまま
        assert!(page("<body bgcolor=\"var(--bg)\" style=\"color: nope\"><p>a</p>").starts_with("<html><body bgcolor=\"#111111\" text=\"#eeeeee\""));
    }

    #[test]
    fn text_colors_wrap_in_font_and_close_with_their_element() {
        assert_eq!(conv("<p style=\"color:red\">a</p>b"), "<p><font color=\"#ff0000\">a</font></p>b");
        assert_eq!(conv("<p>x<span style=\"color:#0f0\">y</span>z</p>"), "<p>x<font color=\"#00ff00\">y</font>z</p>");
        assert_eq!(conv("<style>.w{color:orange}</style><p class=w>a"), "<p><font color=\"#ffa500\">a</font></p>");
        // 閉じ忘れた span はセルの外まで漏らさない
        let r = conv("<table><tr><td><span style=color:red>a</td><td>b</td></tr></table>");
        assert!(r.contains("<td><font color=\"#ff0000\">a</font></td><td>b</td>"), "{r}");
        // 背景に溶ける文字色（暗いテーマに黒）は読める色にする
        assert_eq!(conv("<p style=\"color:#000\">a</p>"), "<p><font color=\"#ffffff\">a</font></p>");
        // 読めない色は何もしない
        assert_eq!(conv("<p style=\"color:inherit\">a</p>"), "<p>a</p>");
    }

    #[test]
    fn space_after_a_colored_span_is_kept() {
        assert_eq!(conv("<p>a <span style=color:red>b</span> c</p>"), "<p>a <font color=\"#ff0000\">b</font> c</p>");
    }

    #[test]
    fn styles_inside_head_are_read() {
        assert_eq!(conv("<head><title>t</title><style>p{color:red}</style></head><p>a"), "<p><font color=\"#ff0000\">a</font></p>");
        assert!(page("<head><style>body{background:white}</style></head><p>a").starts_with("<html><body bgcolor=\"#ffffff\""));
    }

    #[test]
    fn cell_backgrounds_and_alignment() {
        let r = conv("<table><tr><td style=\"background:#ffffcc;text-align:right\">a</td><td bgcolor=navy>b</td></tr></table>");
        // 明るいセルでは、テーマの明るい文字が読めないので黒にする
        assert!(r.contains("<td align=\"right\" bgcolor=\"#ffffcc\"><font color=\"#000000\">a</font></td>") || r.contains("<td bgcolor=\"#ffffcc\" align=\"right\"><font color=\"#000000\">a</font></td>"), "{r}");
        assert!(r.contains("<td bgcolor=\"#000080\">b</td>"), "{r}");
        assert_eq!(conv("<div style=\"text-align:center\">a</div>b"), "<center>a</center>b");
        assert_eq!(conv("<p align=center>a</p>"), "<p align=\"center\">a</p>");
    }

    #[test]
    fn head_script_and_style_are_dropped() {
        let h = "<html><head><title>T</title><style>p{}</style></head><body><p>Hi</p><script>var a='<b>';</script></body></html>";
        assert_eq!(conv(h), "<p>Hi</p>");
    }


    #[test]
    fn comments_and_doctype_are_dropped() {
        assert_eq!(conv("<!DOCTYPE html><!-- c <b> --><p>a</p>"), "<p>a</p>");
    }

    #[test]
    fn entities_and_stray_lt_are_escaped() {
        assert_eq!(conv("a &amp; b &lt; c < d &unknown; e"), "a &amp; b &lt; c &lt; d &amp;unknown; e");
        assert_eq!(decode_entities("&#65;&#x42;&hearts;"), "AB♥");
        assert_eq!(conv("x&nbsp;y"), "x&nbsp;y");
    }

    #[test]
    fn links_are_text_with_the_target_appended() {
        let h = "<p><a href=\"https://x.test/a?b=1&amp;c=2\">site</a> <a href=\"#top\">top</a></p>";
        assert_eq!(conv(h), "<p>site (https://x.test/a?b=1&amp;c=2) top</p>");
        // 文字がそのままリンク先なら重ねない
        assert_eq!(conv("<p><a href=\"https://x.test\">https://x.test</a></p>"), "<p>https://x.test</p>");
        assert_eq!(conv("<p><a href=\"javascript:void(0)\">go</a></p>"), "<p>go</p>");
    }

    #[test]
    fn omitted_end_tags_are_completed() {
        assert_eq!(conv("<ul><li>a<li>b</ul>"), "<ul><li>a</li><li>b</li></ul>");
        assert_eq!(conv("<p>a<p>b"), "<p>a</p><p>b</p>");
        assert_eq!(conv("<b>x<i>y"), "<b>x<i>y</i></b>");
        assert_eq!(conv("x</b>y"), "xy"); // 対応する開きが無い閉じタグは捨てる
    }

    #[test]
    fn tables_get_borders_spans_and_empty_cells() {
        let h = "<table><tr><td colspan=2>a</td><td></td></tr></table>";
        assert_eq!(
            conv(h),
            "<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\"><tr><td colspan=\"2\">a</td><td>&nbsp;</td></tr></table>"
        );
        // 省略された閉じタグ
        let h = "<table><tr><td>a<td>b<tr><td>c</table>";
        assert_eq!(
            conv(h),
            "<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\"><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>"
        );
        // セルの外の文字は出さない
        assert!(!conv("<table>stray<tr><td>a</td></tr></table>").contains("stray"));
    }

    #[test]
    fn divs_become_single_breaks() {
        let r = conv("<div>a</div><div>b</div>");
        assert!(r.starts_with("a<br>b"), "{r}");
        assert!(!r.contains("<br><br>"), "{r}");
        assert!(!r.contains("div"), "{r}");
    }

    #[test]
    fn images_fall_back_to_alt_text() {
        assert_eq!(conv("<p><img src=\"https://x/y.png\" alt=\"logo\"> text</p>"), "<p>[logo] text</p>");
        assert_eq!(conv("<p>a<img src=\"https://x/y.png\">b</p>"), "<p>ab</p>");
    }

    #[test]
    fn japanese_lines_join_without_a_space() {
        assert_eq!(conv("<p>日本\n語です</p>"), "<p>日本語です</p>");
        assert_eq!(conv("<p>a\nb</p>"), "<p>a b</p>");
    }

    #[test]
    fn pre_keeps_its_whitespace() {
        assert_eq!(conv("<pre>a\n  b</pre>"), "<pre>a\n  b</pre>");
    }

    #[test]
    fn attributes_are_read_in_any_quoting() {
        assert_eq!(attr(" href='a b' alt=x class", "alt").as_deref(), Some("x"));
        assert_eq!(attr(" href='a b' alt=x class", "href").as_deref(), Some("a b"));
        assert_eq!(attr(" HREF=\"a&amp;b\"", "href").as_deref(), Some("a&b"));
        assert_eq!(attr(" class", "class").as_deref(), Some(""));
        assert_eq!(attr(" a=1", "b"), None);
        // 引用符の中の `>` はタグの終わりではない
        assert_eq!(conv("<p title=\"a>b\">x</p>"), "<p>x</p>");
    }

    #[test]
    fn unclosed_script_drops_the_rest_but_unclosed_head_does_not() {
        assert_eq!(conv("<p>a</p><script>alert(1)"), "<p>a</p>");
        assert_eq!(conv("<head><p>a</p>"), "<p>a</p>");
    }

    #[test]
    fn page_is_wrapped_with_the_theme_colors() {
        let f = |_: &Path| -> Option<(i32, i32)> { None };
        let imgs = Images { base: None, max_w: 400, size_of: &f };
        let r = to_html("<p>a</p>", "#111111", "#eeeeee", "#3399ff", "#888888", &imgs);
        assert!(r.starts_with("<html><body bgcolor=\"#111111\" text=\"#eeeeee\" link=\"#3399ff\">"), "{r}");
        assert!(r.contains("<p>a</p>") && r.ends_with("</body></html>"));
        let e = to_html("<script>1</script>", "#111111", "#eeeeee", "#3399ff", "#888888", &imgs);
        assert!(e.contains("表示できる内容がありません"), "{e}");
    }

    #[test]
    fn long_input_is_cut_on_a_char_boundary() {
        let s = "あ".repeat(MAX_IN / 3 + 10); // 3 バイトずつ。上限をわずかに超える
        let (cut, flag) = limit(&s);
        assert!(flag && cut.len() <= MAX_IN && s.is_char_boundary(cut.len()));
    }
}
