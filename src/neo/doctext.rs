// docx / odt / pptx / odp / epub → 文字だけの文書（GUI なし）。読み取り専用の簡易対応: Preview で本文を読むだけ。
// zip の展開は `archive::read_entry` + `inflate`（自前）、XML は `book` の最小の走査器を共用する。依存は増やさない。
//
// 読むもの: 見出し・段落・リスト（記号だけ）・表（罫線つき）。docx / odt は本文を 1 区切り、pptx / odp はスライドごと、epub は章ごとに区切る。
// 読まないもの: 書式（太字・色・フォント）、画像・図形・グラフ、ヘッダー・フッター、脚注・コメント、変更履歴の削除分、
//   ふりがな（本文だけを出す）、スライドのノート、リストの番号、旧形式（doc / ppt）、パスワード付き。
use crate::neo::book::{attr, entry_text, Ev, Xml};
use crate::neo::sheet;
use std::collections::HashMap;
use std::path::Path;

/// 読む zip（ファイル全体）の大きさの上限。
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
/// 出す文字数の上限（超えた分は切って印を付ける。HelpView が重くならないように）。
const MAX_CHARS: usize = 4_000_000;
/// スライド・章の最大数。
const MAX_SECTIONS: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Docx,
    Odt,
    Pptx,
    Odp,
    Epub,
}

/// 拡張子から形式を決める（大文字小文字を区別しない）。`viewmode::kind_of` の `Kind::Doc` と同じ集合。
pub fn format_of(p: &Path) -> Option<Format> {
    let ext = p.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "docx" | "docm" => Some(Format::Docx),
        "odt" => Some(Format::Odt),
        "pptx" | "pptm" => Some(Format::Pptx),
        "odp" => Some(Format::Odp),
        "epub" => Some(Format::Epub),
        _ => None,
    }
}

impl Format {
    fn label(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Odt => "odt",
            Format::Pptx => "pptx",
            Format::Odp => "odp",
            Format::Epub => "epub",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// 見出し（1〜6）。
    Heading(u8, String),
    Para(String),
    /// リストの項目（入れ子の深さ 0〜）。番号は出さず「・」にする。
    Item(usize, String),
    /// 表（行 → 列）。1 行目を見出しとして出す（csv の Preview と同じ）。
    Table(Vec<Vec<String>>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Text {
    /// 区切りごとのブロック（docx / odt は 1 つ、pptx / odp はスライドごと、epub は章ごと）。
    pub sections: Vec<Vec<Block>>,
    /// 文字数・区切りの上限で、後ろを切ったか。
    pub truncated: bool,
}

/// zip 全体のバイト列から、文字だけを読む。
pub fn read(bytes: &[u8], fmt: Format) -> Result<Text, String> {
    if bytes.len() > MAX_BYTES {
        return Err(if crate::lang::is_en() { format!("File is too large (limit {} MiB)", MAX_BYTES / 1024 / 1024) } else { format!("ファイルが大きすぎます（上限 {} MiB）", MAX_BYTES / 1024 / 1024) });
    }
    match fmt {
        Format::Docx => read_docx(bytes),
        Format::Odt | Format::Odp => read_odf(bytes, fmt),
        Format::Pptx => read_pptx(bytes),
        Format::Epub => read_epub(bytes),
    }
}

// ------------------------------------------------------------------ 共通の部品

fn block_chars(b: &Block) -> usize {
    match b {
        Block::Heading(_, s) | Block::Para(s) | Block::Item(_, s) => s.chars().count(),
        Block::Table(rows) => rows.iter().flatten().map(|c| c.chars().count()).sum(),
    }
}

fn section_chars(s: &[Block]) -> usize {
    s.iter().map(block_chars).sum()
}

/// 全体の文字数が `MAX_CHARS` を超えるなら、ブロックの切れ目で後ろを捨てる。
fn cap(sections: &mut Vec<Vec<Block>>) -> bool {
    let mut total = 0usize;
    for si in 0..sections.len() {
        for bi in 0..sections[si].len() {
            total += block_chars(&sections[si][bi]);
            if total > MAX_CHARS {
                sections[si].truncate(bi + 1);
                sections.truncate(si + 1);
                return true;
            }
        }
    }
    false
}

/// 段落・見出し・表を組み立てる状態。docx / pptx / odt / odp / epub で共用する。
#[derive(Default)]
struct Builder {
    out: Vec<Block>,
    /// 入れ子の段落（テキストボックスは段落の中に段落が入る）。
    paras: Vec<P>,
    /// 表の入れ子の深さ。内側の表の文字は、外側のセルに連結する。
    tbl: usize,
    table: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    /// pptx / odp: 今いる図形がタイトル（見出しにする）か、番号・日付・フッター（出さない）か。
    title: bool,
    suppress: bool,
}

struct P {
    text: String,
    level: u8,
    item: Option<usize>,
}

impl Builder {
    fn start_p(&mut self, level: u8, item: Option<usize>) {
        self.paras.push(P { text: String::new(), level, item });
    }

    fn push_text(&mut self, s: &str) {
        if let Some(p) = self.paras.last_mut() {
            p.text.push_str(s);
        }
    }

    fn end_p(&mut self) {
        let Some(p) = self.paras.pop() else { return };
        self.emit(p.level, p.item, p.text.trim().to_string());
    }

    /// 1 つのブロックを出す（空は捨てる）。表の中なら、セルに連結する。
    fn emit(&mut self, level: u8, item: Option<usize>, text: String) {
        if text.is_empty() {
            return;
        }
        if self.tbl > 0 {
            if !self.cell.is_empty() {
                self.cell.push(' ');
            }
            self.cell.push_str(&text);
            return;
        }
        if self.suppress {
            return;
        }
        self.out.push(if level > 0 {
            Block::Heading(level, text)
        } else if self.title {
            Block::Heading(2, text)
        } else if let Some(l) = item {
            Block::Item(l, text)
        } else {
            Block::Para(text)
        });
    }

    fn start_table(&mut self) {
        self.tbl += 1;
        if self.tbl == 1 {
            self.table.clear();
        }
    }

    fn start_row(&mut self) {
        if self.tbl == 1 {
            self.row.clear();
        }
    }

    fn start_cell(&mut self) {
        if self.tbl == 1 {
            self.cell.clear();
        }
    }

    fn end_cell(&mut self) {
        if self.tbl == 1 {
            let c = std::mem::take(&mut self.cell);
            self.row.push(c);
        }
    }

    fn end_row(&mut self) {
        if self.tbl == 1 {
            let r = std::mem::take(&mut self.row);
            self.table.push(r);
        }
    }

    fn end_table(&mut self) {
        if self.tbl == 1 {
            let t = std::mem::take(&mut self.table);
            if !t.is_empty() && !self.suppress {
                self.out.push(Block::Table(t));
            }
        }
        self.tbl = self.tbl.saturating_sub(1);
    }
}

/// zip の中の相対参照（`rels` の Target・epub の href）を、zip の中のパスにする。`base` は参照元のフォルダ（`ppt` など。空でもよい）。
fn resolve(base: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or("");
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// `%E3%81%82` → `あ`。壊れた並びはそのまま残す。
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `Id → Target` の表（`*.rels`）。
fn read_rels(bytes: &[u8], name: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(x) = entry_text(bytes, name) {
        for ev in Xml::new(&x) {
            if let Ev::Start(n, a) = ev {
                if n == "Relationship" {
                    if let (Some(id), Some(t)) = (attr(&a, "Id"), attr(&a, "Target")) {
                        m.insert(id.to_string(), t.to_string());
                    }
                }
            }
        }
    }
    m
}

/// 見出しのスタイル名 → 段階。`Title` は 1、`heading 3` / `Heading3` は 3（6 まで）。
fn heading_level(name: &str) -> Option<u8> {
    let n = name.to_lowercase().replace(' ', "");
    if n == "title" {
        return Some(1);
    }
    let d = n.strip_prefix("heading")?;
    d.parse::<u8>().ok().filter(|v| *v >= 1).map(|v| v.min(6))
}

// ------------------------------------------------------------------ docx / pptx

/// docx の `word/styles.xml`: スタイル id → 見出しの段階（日本語版 Word は id が `1` のこともあるので、名前を見る）。
fn docx_styles(bytes: &[u8]) -> HashMap<String, u8> {
    let mut m = HashMap::new();
    let Ok(x) = entry_text(bytes, "word/styles.xml") else { return m };
    let mut cur: Option<String> = None;
    for ev in Xml::new(&x) {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "style" => cur = attr(&a, "styleId").map(String::from),
                "name" => {
                    if let (Some(id), Some(v)) = (cur.as_ref(), attr(&a, "val")) {
                        if let Some(l) = heading_level(v) {
                            m.insert(id.clone(), l);
                        }
                    }
                }
                _ => {}
            },
            Ev::End(n) => {
                if n == "style" {
                    cur = None;
                }
            }
            Ev::Text(_) => {}
        }
    }
    m
}

/// docx の本文と pptx のスライドは、接頭辞を除いたタグ名（`p` `t` `br` `tab` `tbl` `tr` `tc`）が同じなので、1 つの走査で読める。
fn parse_ooxml(xml: &str, styles: &HashMap<String, u8>) -> Vec<Block> {
    let mut b = Builder::default();
    let (mut in_t, mut in_ppr) = (false, false);
    let mut skip = 0usize; // 読み飛ばす部分木の深さ（0 = 読む）
    for ev in Xml::new(xml) {
        if skip > 0 {
            match ev {
                Ev::Start(..) => skip += 1,
                Ev::End(_) => skip -= 1,
                Ev::Text(_) => {}
            }
            continue;
        }
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                // mc:Fallback は mc:Choice と同じ中身の複製。rt はふりがな
                "Fallback" | "rt" => skip = 1,
                "p" => b.start_p(0, None),
                "pPr" => in_ppr = true,
                "pStyle" => {
                    if let (Some(p), Some(v)) = (b.paras.last_mut(), attr(&a, "val")) {
                        if let Some(l) = styles.get(v).copied().or_else(|| heading_level(v)) {
                            p.level = l;
                        }
                    }
                }
                "numPr" => {
                    if let Some(p) = b.paras.last_mut() {
                        p.item = Some(0);
                    }
                }
                "ilvl" => {
                    if let (Some(p), Some(v)) = (b.paras.last_mut(), attr(&a, "val")) {
                        if p.item.is_some() {
                            p.item = Some(v.parse().unwrap_or(0));
                        }
                    }
                }
                "numId" => {
                    if attr(&a, "val") == Some("0") {
                        if let Some(p) = b.paras.last_mut() {
                            p.item = None; // 番号なし
                        }
                    }
                }
                "sp" => {
                    b.title = false;
                    b.suppress = false;
                }
                "ph" => match attr(&a, "type") {
                    Some("title" | "ctrTitle") => b.title = true,
                    Some("sldNum" | "dt" | "ftr" | "hdr") => b.suppress = true,
                    _ => {}
                },
                "t" => in_t = true,
                "br" | "cr" => {
                    if attr(&a, "type") != Some("page") {
                        b.push_text("\n");
                    }
                }
                "tab" => {
                    if !in_ppr {
                        b.push_text("\t"); // pPr の中の tab は tab 位置の定義
                    }
                }
                "tbl" => b.start_table(),
                "tr" => b.start_row(),
                "tc" => b.start_cell(),
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "t" => in_t = false,
                "pPr" => in_ppr = false,
                "p" => b.end_p(),
                "sp" => {
                    b.title = false;
                    b.suppress = false;
                }
                "tc" => b.end_cell(),
                "tr" => b.end_row(),
                "tbl" => b.end_table(),
                _ => {}
            },
            Ev::Text(t) => {
                if in_t {
                    b.push_text(&t);
                }
            }
        }
    }
    b.out
}

fn read_docx(bytes: &[u8]) -> Result<Text, String> {
    let xml = entry_text(bytes, "word/document.xml").map_err(|e| if crate::lang::is_en() { format!("Cannot read as docx: {e}") } else { format!("docx として読めません: {e}") })?;
    let styles = docx_styles(bytes);
    let mut sections = vec![parse_ooxml(&xml, &styles)];
    let truncated = cap(&mut sections);
    Ok(Text { sections, truncated })
}

fn read_pptx(bytes: &[u8]) -> Result<Text, String> {
    let pres = entry_text(bytes, "ppt/presentation.xml").map_err(|e| if crate::lang::is_en() { format!("Cannot read as pptx: {e}") } else { format!("pptx として読めません: {e}") })?;
    // `<p:sldId id="256" r:id="rId2"/>`: 接頭辞を除くと属性が両方 `id` になるので、rels に載っている方を関係の id とみなす
    let mut ids: Vec<Vec<String>> = Vec::new();
    for ev in Xml::new(&pres) {
        if let Ev::Start(n, a) = ev {
            if n == "sldId" {
                ids.push(a.iter().filter(|(k, _)| k == "id").map(|(_, v)| v.clone()).collect());
            }
        }
    }
    let rels = read_rels(bytes, "ppt/_rels/presentation.xml.rels");
    let mut paths: Vec<String> = ids.iter().filter_map(|c| c.iter().find_map(|i| rels.get(i))).map(|t| resolve("ppt", t)).collect();
    let listed = !paths.is_empty();
    if !listed {
        // 関係が読めないときは、慣例の名前を順に試す（無い番号で止める）
        paths = (1..=MAX_SECTIONS).map(|i| format!("ppt/slides/slide{i}.xml")).collect();
    }
    let no_styles = HashMap::new();
    let mut sections = Vec::new();
    let (mut total, mut truncated) = (0usize, false);
    for p in &paths {
        if sections.len() >= MAX_SECTIONS || total > MAX_CHARS {
            truncated = true;
            break;
        }
        match entry_text(bytes, p) {
            Ok(x) => {
                let blocks = parse_ooxml(&x, &no_styles);
                total += section_chars(&blocks);
                sections.push(blocks);
            }
            Err(_) if !listed => break,
            Err(_) => {}
        }
    }
    if sections.is_empty() {
        return Err(crate::lang::tr("pptx にスライドが見つかりません", "No slides found in pptx").into());
    }
    truncated |= cap(&mut sections);
    Ok(Text { sections, truncated })
}

// ------------------------------------------------------------------ odt / odp

/// odt / odp の `content.xml`。`draw:page`（odp のスライド）があれば、ページごとに区切る。
fn parse_odf(xml: &str) -> Vec<Vec<Block>> {
    let mut b = Builder::default();
    let mut sections: Vec<Vec<Block>> = Vec::new();
    let mut in_page = false;
    let mut lists = 0usize;
    // draw:frame の入れ子ごとの (タイトルか, 出さないか)
    let mut frames: Vec<(bool, bool)> = Vec::new();
    let mut skip = 0usize;
    for ev in Xml::new(xml) {
        if skip > 0 {
            match ev {
                Ev::Start(..) => skip += 1,
                Ev::End(_) => skip -= 1,
                Ev::Text(_) => {}
            }
            continue;
        }
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                // コメント・脚注・変更履歴（削除された文字を含む）・ふりがな・スライドのノート
                "annotation" | "note" | "tracked-changes" | "ruby-text" | "notes" => skip = 1,
                "page" => {
                    in_page = true;
                    b.out = Vec::new();
                }
                "frame" => {
                    let class = attr(&a, "class");
                    frames.push((class == Some("title"), matches!(class, Some("footer" | "date-time" | "page-number" | "header"))));
                    b.title = frames.iter().any(|f| f.0);
                    b.suppress = frames.iter().any(|f| f.1);
                }
                "list" => lists += 1,
                "p" => b.start_p(0, if lists > 0 { Some(lists - 1) } else { None }),
                "h" => {
                    let l = attr(&a, "outline-level").and_then(|v| v.parse::<u8>().ok()).unwrap_or(1).clamp(1, 6);
                    b.start_p(l, None);
                }
                "s" => {
                    let c = attr(&a, "c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(100);
                    b.push_text(&" ".repeat(c));
                }
                "tab" => b.push_text("\t"),
                "line-break" => b.push_text("\n"),
                "table" => b.start_table(),
                "table-row" => b.start_row(),
                "table-cell" => b.start_cell(),
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "page" => {
                    if in_page {
                        sections.push(std::mem::take(&mut b.out));
                        in_page = false;
                    }
                }
                "frame" => {
                    frames.pop();
                    b.title = frames.iter().any(|f| f.0);
                    b.suppress = frames.iter().any(|f| f.1);
                }
                "list" => lists = lists.saturating_sub(1),
                "p" | "h" => b.end_p(),
                "table-cell" => b.end_cell(),
                "table-row" => b.end_row(),
                "table" => b.end_table(),
                _ => {}
            },
            Ev::Text(t) => b.push_text(&t),
        }
    }
    // odt はページを持たない: 全体を 1 つの区切りにする。odp は、ページの外に残ったもの（あれば）を無視する
    if sections.is_empty() {
        sections.push(b.out);
    }
    sections
}

fn read_odf(bytes: &[u8], fmt: Format) -> Result<Text, String> {
    let xml = entry_text(bytes, "content.xml").map_err(|e| if crate::lang::is_en() { format!("Cannot read as {}: {e}", fmt.label()) } else { format!("{} として読めません: {e}", fmt.label()) })?;
    let mut sections = parse_odf(&xml);
    let mut truncated = false;
    if sections.len() > MAX_SECTIONS {
        sections.truncate(MAX_SECTIONS);
        truncated = true;
    }
    truncated |= cap(&mut sections);
    Ok(Text { sections, truncated })
}

// ------------------------------------------------------------------ epub

/// `h1`〜`h6` → 1〜6。
fn h_level(n: &str) -> Option<u8> {
    let b = n.as_bytes();
    (b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1])).then(|| b[1] - b'0')
}

/// 前後に空行を入れる要素（段落の切れ目になる）。
const BLOCKS: &[&str] = &[
    "p", "div", "blockquote", "section", "article", "aside", "header", "footer", "figure", "figcaption", "dl", "dt", "dd", "address", "hr", "body", "main",
    "center", "caption",
];

/// XML が戻さなかった、HTML の名前つき文字（`&nbsp;` など）を戻す。
fn fix_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = s.to_string();
    for (k, v) in [
        ("&nbsp;", " "),
        ("&ensp;", " "),
        ("&emsp;", "\u{3000}"),
        ("&mdash;", "—"),
        ("&ndash;", "–"),
        ("&hellip;", "…"),
        ("&lsquo;", "‘"),
        ("&rsquo;", "’"),
        ("&ldquo;", "“"),
        ("&rdquo;", "”"),
        ("&laquo;", "«"),
        ("&raquo;", "»"),
        ("&copy;", "©"),
        ("&middot;", "·"),
    ] {
        o = o.replace(k, v);
    }
    o
}

fn wide(c: char) -> bool {
    c as u32 >= 0x2E80
}

/// HTML の空白の畳み込み。連続する空白は 1 つにし、`\u{1}`（`<br>`）は改行にする。
/// 改行を含む空白の前後が両方とも日本語などの幅広い文字なら、空白を入れない（ブラウザと同じ）。
fn collapse(raw: &str) -> String {
    let mut out = String::new();
    let mut pending: Option<bool> = None; // Some(改行を含むか)
    for c in raw.chars() {
        if c == '\u{1}' {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
            pending = None;
            continue;
        }
        if c.is_whitespace() {
            pending = Some(pending.unwrap_or(false) || c == '\n');
            continue;
        }
        if let Some(nl) = pending.take() {
            let join = match out.chars().last() {
                None | Some('\n') => false,
                Some(l) => !(nl && wide(l) && wide(c)),
            };
            if join {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out.trim_matches(|c| c == '\n' || c == ' ').to_string()
}

struct Xh {
    b: Builder,
    buf: String,
    heading: u8,
    lists: usize,
    li: usize,
    pre: usize,
}

impl Xh {
    fn flush(&mut self) {
        let raw = std::mem::take(&mut self.buf);
        let text = if self.pre > 0 { raw.replace('\u{1}', "\n").trim_matches('\n').to_string() } else { collapse(&raw) };
        if text.trim().is_empty() {
            return;
        }
        let item = if self.li > 0 { Some(self.lists.saturating_sub(1)) } else { None };
        self.b.emit(self.heading, item, text);
    }
}

/// epub の章（XHTML）から、見出し・段落・リスト・表を取り出す。
fn parse_xhtml(xml: &str) -> Vec<Block> {
    let mut x = Xh { b: Builder::default(), buf: String::new(), heading: 0, lists: 0, li: 0, pre: 0 };
    let mut skip = 0usize;
    for ev in Xml::new(xml) {
        if skip > 0 {
            match ev {
                Ev::Start(..) => skip += 1,
                Ev::End(_) => skip -= 1,
                Ev::Text(_) => {}
            }
            continue;
        }
        match ev {
            Ev::Start(n, _) => {
                let name = n.as_str();
                if matches!(name, "head" | "script" | "style" | "svg" | "rt" | "rp") {
                    skip = 1;
                } else if name == "br" {
                    x.buf.push('\u{1}');
                } else if let Some(l) = h_level(name) {
                    x.flush();
                    x.heading = l;
                } else {
                    match name {
                        "ul" | "ol" => {
                            x.flush();
                            x.lists += 1;
                        }
                        "li" => {
                            x.flush();
                            x.li += 1;
                        }
                        "pre" => {
                            x.flush();
                            x.pre += 1;
                        }
                        "table" => {
                            x.flush();
                            x.b.start_table();
                        }
                        "tr" => {
                            x.flush();
                            x.b.start_row();
                        }
                        "td" | "th" => {
                            x.flush();
                            x.b.start_cell();
                        }
                        n if BLOCKS.contains(&n) => x.flush(),
                        _ => {}
                    }
                }
            }
            Ev::End(n) => {
                let name = n.as_str();
                if h_level(name).is_some() {
                    x.flush();
                    x.heading = 0;
                } else {
                    match name {
                        "ul" | "ol" => {
                            x.flush();
                            x.lists = x.lists.saturating_sub(1);
                        }
                        "li" => {
                            x.flush();
                            x.li = x.li.saturating_sub(1);
                        }
                        "pre" => {
                            x.flush();
                            x.pre = x.pre.saturating_sub(1);
                        }
                        "table" => {
                            x.flush();
                            x.b.end_table();
                        }
                        "tr" => {
                            x.flush();
                            x.b.end_row();
                        }
                        "td" | "th" => {
                            x.flush();
                            x.b.end_cell();
                        }
                        n if BLOCKS.contains(&n) => x.flush(),
                        _ => {}
                    }
                }
            }
            Ev::Text(t) => x.buf.push_str(&fix_entities(&t)),
        }
    }
    x.flush();
    x.b.out
}

fn read_epub(bytes: &[u8]) -> Result<Text, String> {
    let container = entry_text(bytes, "META-INF/container.xml").map_err(|e| if crate::lang::is_en() { format!("Cannot read as epub: {e}") } else { format!("epub として読めません: {e}") })?;
    let opf_path = Xml::new(&container)
        .find_map(|ev| match ev {
            Ev::Start(n, a) if n == "rootfile" => attr(&a, "full-path").map(String::from),
            _ => None,
        })
        .ok_or_else(|| crate::lang::tr("epub として読めません: 本文の一覧（OPF）が見つかりません", "Cannot read as epub: content list (OPF) not found").to_string())?;
    let opf = entry_text(bytes, &opf_path).map_err(|e| if crate::lang::is_en() { format!("Cannot read as epub: {e}") } else { format!("epub として読めません: {e}") })?;
    let base = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    for ev in Xml::new(&opf) {
        if let Ev::Start(n, a) = ev {
            match n.as_str() {
                "item" => {
                    if let (Some(id), Some(h)) = (attr(&a, "id"), attr(&a, "href")) {
                        manifest.insert(id.to_string(), h.to_string());
                    }
                }
                "itemref" => {
                    if let Some(r) = attr(&a, "idref") {
                        spine.push(r.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let mut sections = Vec::new();
    let (mut total, mut truncated) = (0usize, false);
    for idref in &spine {
        let Some(href) = manifest.get(idref) else { continue };
        if sections.len() >= MAX_SECTIONS || total > MAX_CHARS {
            truncated = true;
            break;
        }
        let path = resolve(base, &percent_decode(href));
        let Ok(x) = entry_text(bytes, &path) else { continue };
        let blocks = parse_xhtml(&x);
        if blocks.is_empty() {
            continue; // 表紙など、画像だけの章
        }
        total += section_chars(&blocks);
        sections.push(blocks);
    }
    if sections.is_empty() {
        return Err(crate::lang::tr("epub に読める章がありません", "No readable chapter in epub").into());
    }
    truncated |= cap(&mut sections);
    Ok(Text { sections, truncated })
}

// ------------------------------------------------------------------ 表示

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            '\t' => out.push_str("&nbsp;&nbsp;&nbsp;&nbsp;"),
            '\r' | '\u{c}' => {}
            c => out.push(c),
        }
    }
}

/// 文字を HelpView に渡す完全な HTML にする。区切りが複数あるとき（スライド・章）は `── 1 / N ──` を挟む。色は `#rrggbb`。
pub fn to_html(t: &Text, bg: &str, fg: &str, sub: &str) -> String {
    let mut body = String::new();
    let total = t.sections.len();
    if t.sections.iter().all(|s| s.is_empty()) {
        body.push_str(crate::lang::tr("<p>文字を取り出せませんでした（画像だけの文書、または対応していない内容です）。</p>", "<p>Could not extract text (the document is images only, or its content is not supported).</p>"));
    }
    for (i, s) in t.sections.iter().enumerate() {
        if total > 1 {
            body.push_str(&format!("<p><font color=\"{sub}\">── {} / {total} ──</font></p>", i + 1));
        }
        for b in s {
            match b {
                Block::Heading(l, s) => {
                    let l = (*l).clamp(1, 6);
                    body.push_str(&format!("<h{l}>"));
                    esc_text(s, &mut body);
                    body.push_str(&format!("</h{l}>"));
                }
                Block::Para(s) => {
                    body.push_str("<p>");
                    esc_text(s, &mut body);
                    body.push_str("</p>");
                }
                Block::Item(l, s) => {
                    body.push_str("<p>");
                    for _ in 0..*l {
                        body.push_str("&nbsp;&nbsp;&nbsp;&nbsp;");
                    }
                    body.push_str(crate::lang::tr("・", "- "));
                    esc_text(s, &mut body);
                    body.push_str("</p>");
                }
                Block::Table(rows) => {
                    let cut = rows.len() > sheet::MAX_ROWS;
                    body.push_str(&sheet::table_html(&rows[..rows.len().min(sheet::MAX_ROWS)], cut, false, false, sub, false));
                }
            }
        }
    }
    if t.truncated {
        body.push_str(&if crate::lang::is_en() { format!("<p><font color=\"{sub}\">Showing only part of it because it is long.</font></p>") } else { format!("<p><font color=\"{sub}\">長いため、途中までを表示しています。</font></p>") });
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

/// 新しいタブに出す文字（Text in New Tab）。ブロックの間は空行、リストの項目と表の行は詰める。表のセルはタブで区切る。
pub fn to_plain(t: &Text) -> String {
    let mut out = String::new();
    let mut prev_item = false;
    for s in &t.sections {
        for b in s {
            let is_item = matches!(b, Block::Item(..));
            if !out.is_empty() {
                out.push_str(if is_item && prev_item { "\n" } else { "\n\n" });
            }
            prev_item = is_item;
            match b {
                Block::Heading(_, s) | Block::Para(s) => out.push_str(s),
                Block::Item(l, s) => {
                    out.push_str(&"  ".repeat(*l));
                    out.push_str(crate::lang::tr("・", "- "));
                    out.push_str(s);
                }
                Block::Table(rows) => {
                    let lines: Vec<String> = rows.iter().map(|r| r.join("\t")).collect();
                    out.push_str(&lines.join("\n"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の zip（無圧縮）。CRC は読まないので 0。
    fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in entries {
            let off = out.len() as u32;
            let d = data.as_bytes();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0]); // version, flags, method(0)
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(d);

            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0]); // made by, needed, flags, method(0)
            cd.extend_from_slice(&[0; 4]);
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&(d.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(d.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attr
            cd.extend_from_slice(&0u32.to_le_bytes()); // external attr
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn para(s: &str) -> Block {
        Block::Para(s.to_string())
    }

    #[test]
    fn extensions_pick_the_format() {
        assert_eq!(format_of(Path::new("a/B.DOCX")), Some(Format::Docx));
        assert_eq!(format_of(Path::new("x.docm")), Some(Format::Docx));
        assert_eq!(format_of(Path::new("x.odt")), Some(Format::Odt));
        assert_eq!(format_of(Path::new("x.pptx")), Some(Format::Pptx));
        assert_eq!(format_of(Path::new("x.odp")), Some(Format::Odp));
        assert_eq!(format_of(Path::new("x.EPUB")), Some(Format::Epub));
        assert_eq!(format_of(Path::new("x.doc")), None);
        assert_eq!(format_of(Path::new("noext")), None);
    }

    #[test]
    fn resolve_and_percent_decode() {
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt", "/ppt/slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("OEBPS/text", "../img/a.xhtml#top"), "OEBPS/img/a.xhtml");
        assert_eq!(resolve("", "c.xhtml"), "c.xhtml");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%E3%81%82.xhtml"), "あ.xhtml");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn heading_names() {
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("heading 3"), Some(3));
        assert_eq!(heading_level("Heading2"), Some(2));
        assert_eq!(heading_level("heading 9"), Some(6));
        assert_eq!(heading_level("Normal"), None);
        assert_eq!(heading_level("heading 0"), None);
    }

    #[test]
    fn collapse_whitespace_like_a_browser() {
        assert_eq!(collapse("  a \n  b  "), "a b");
        assert_eq!(collapse("あ\nい"), "あい"); // 日本語どうしの改行は詰める
        assert_eq!(collapse("あ い"), "あ い"); // 改行でない空白は残す
        assert_eq!(collapse("x\u{1}  y"), "x\ny");
        assert_eq!(collapse("x \u{1}y\u{1}"), "x\ny");
        assert_eq!(collapse("   "), "");
    }

    const DOCX: &str = r#"<?xml version="1.0"?><w:document xmlns:w="w" xmlns:mc="mc"><w:body>
<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>第一章</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Hello </w:t></w:r><w:r><w:t>&amp; world</w:t></w:r><w:r><w:br/></w:r><w:r><w:t>next</w:t></w:r></w:p>
<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>a</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>b</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="0"/></w:numPr></w:pPr><w:r><w:t>not item</w:t></w:r></w:p>
<w:p><w:r><w:t>漢字</w:t></w:r><w:ruby><w:rt><w:r><w:t>かんじ</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>X</w:t></w:r></w:rubyBase></w:ruby></w:p>
<w:p/>
<w:p><w:r><w:del><w:delText>gone</w:delText></w:del></w:r></w:p>
<mc:AlternateContent><mc:Choice><w:p><w:r><w:t>box</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>box</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>h1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>h2</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>in</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;

    const STYLES: &str = r#"<w:styles xmlns:w="w"><w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;

    #[test]
    fn docx_reads_headings_runs_lists_tables_and_skips_noise() {
        let z = make_zip(&[("word/document.xml", DOCX), ("word/styles.xml", STYLES)]);
        let t = read(&z, Format::Docx).unwrap();
        assert_eq!(t.sections.len(), 1);
        let b = &t.sections[0];
        assert_eq!(b[0], Block::Heading(1, "第一章".into())); // 日本語版 Word の styleId "1"
        assert_eq!(b[1], para("Hello & world\nnext"));
        assert_eq!(b[2], para("a\tb")); // pPr の中の tab は文字にしない
        assert_eq!(b[3], Block::Item(1, "item".into()));
        assert_eq!(b[4], para("not item")); // numId 0 は番号なし
        assert_eq!(b[5], para("漢字X")); // ふりがな（rt）は出さない
        assert_eq!(b[6], para("box")); // Fallback の複製は出さない。空の段落・削除分も出ない
        assert_eq!(b[7], Block::Table(vec![vec!["h1".into(), "h2".into()], vec!["a b".into(), "in".into()]]));
        assert_eq!(b.len(), 8);
        assert!(!t.truncated);
    }

    #[test]
    fn docx_without_styles_still_finds_heading_ids() {
        let x = r#"<w:document xmlns:w="w"><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>H</w:t></w:r></w:p></w:document>"#;
        let z = make_zip(&[("word/document.xml", x)]);
        assert_eq!(read(&z, Format::Docx).unwrap().sections[0], vec![Block::Heading(2, "H".into())]);
    }

    #[test]
    fn broken_files_are_errors_not_panics() {
        assert!(read(b"not a zip", Format::Docx).is_err());
        assert!(read(b"", Format::Epub).is_err());
        let z = make_zip(&[("x.txt", "hi")]);
        for f in [Format::Docx, Format::Odt, Format::Pptx, Format::Odp, Format::Epub] {
            assert!(read(&z, f).is_err(), "{f:?}");
        }
        let z = make_zip(&[("word/document.xml", "<w:p><w:t>unterminated")]);
        assert!(read(&z, Format::Docx).is_ok()); // 閉じていない XML でも、エラーにも無限ループにもしない
    }

    fn slide(title: &str, body: &str) -> String {
        format!(
            r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{body}</a:t></a:r><a:br/><a:r><a:t>line2</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld><a:t>‹#›</a:t></a:fld></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"#
        )
    }

    #[test]
    fn pptx_follows_the_slide_order_in_presentation_xml() {
        let pres = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rId2" Type="t" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="t" Target="slides/slide2.xml"/></Relationships>"#;
        let (s1, s2) = (slide("One", "first"), slide("Two", "second"));
        let z = make_zip(&[
            ("ppt/presentation.xml", pres),
            ("ppt/_rels/presentation.xml.rels", rels),
            ("ppt/slides/slide1.xml", s1.as_str()),
            ("ppt/slides/slide2.xml", s2.as_str()),
        ]);
        let t = read(&z, Format::Pptx).unwrap();
        assert_eq!(t.sections.len(), 2);
        // 並びは rId3 → slide2、rId2 → slide1
        assert_eq!(t.sections[0], vec![Block::Heading(2, "Two".into()), para("second\nline2")]);
        assert_eq!(t.sections[1][0], Block::Heading(2, "One".into()));
        assert_eq!(t.sections[1].len(), 2); // 番号のプレースホルダーは出さない
    }

    #[test]
    fn pptx_falls_back_to_numbered_slide_names() {
        let s1 = slide("A", "a");
        let z = make_zip(&[("ppt/presentation.xml", "<p:presentation/>"), ("ppt/slides/slide1.xml", s1.as_str())]);
        let t = read(&z, Format::Pptx).unwrap();
        assert_eq!(t.sections.len(), 1);
    }

    const ODT: &str = r#"<office:document-content xmlns:office="o" xmlns:text="t" xmlns:table="tb"><office:body><office:text>
<text:tracked-changes><text:changed-region><text:deletion><text:p>deleted</text:p></text:deletion></text:changed-region></text:tracked-changes>
<text:h text:outline-level="2">Heading</text:h>
<text:p>A<text:s text:c="3"/>B<text:tab/>C<text:line-break/>D<text:span>E</text:span><text:note><text:note-citation>1</text:note-citation><text:note-body><text:p>foot</text:p></text:note-body></text:note></text:p>
<text:list><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item></text:list>
<text:p>漢字<text:ruby><text:ruby-base>X</text:ruby-base><text:ruby-text>かんじ</text:ruby-text></text:ruby></text:p>
<table:table><table:table-row><table:table-cell><text:p>c1</text:p></table:table-cell><table:table-cell><text:p>c2</text:p></table:table-cell></table:table-row></table:table>
</office:text></office:body></office:document-content>"#;

    #[test]
    fn odt_reads_headings_spaces_lists_tables_and_skips_notes() {
        let z = make_zip(&[("content.xml", ODT)]);
        let t = read(&z, Format::Odt).unwrap();
        assert_eq!(t.sections.len(), 1);
        let b = &t.sections[0];
        assert_eq!(b[0], Block::Heading(2, "Heading".into()));
        assert_eq!(b[1], para("A   B\tC\nDE")); // 脚注・変更履歴の削除分は出ない
        assert_eq!(b[2], Block::Item(0, "one".into()));
        assert_eq!(b[3], Block::Item(1, "nested".into()));
        assert_eq!(b[4], para("漢字X"));
        assert_eq!(b[5], Block::Table(vec![vec!["c1".into(), "c2".into()]]));
        assert_eq!(b.len(), 6);
    }

    #[test]
    fn odp_splits_pages_and_marks_titles() {
        let x = r#"<office:document-content xmlns:office="o" xmlns:text="t" xmlns:draw="d" xmlns:presentation="p"><office:body><office:presentation>
<draw:page draw:name="a"><draw:frame presentation:class="title"><draw:text-box><text:p>T1</text:p></draw:text-box></draw:frame>
<draw:frame presentation:class="outline"><draw:text-box><text:list><text:list-item><text:p>pt</text:p></text:list-item></text:list></draw:text-box></draw:frame>
<draw:frame presentation:class="page-number"><draw:text-box><text:p>1</text:p></draw:text-box></draw:frame>
<presentation:notes><draw:frame><draw:text-box><text:p>note</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:name="b"><draw:frame><draw:text-box><text:p>plain</text:p></draw:text-box></draw:frame></draw:page>
</office:presentation></office:body></office:document-content>"#;
        let z = make_zip(&[("content.xml", x)]);
        let t = read(&z, Format::Odp).unwrap();
        assert_eq!(t.sections.len(), 2);
        assert_eq!(t.sections[0], vec![Block::Heading(2, "T1".into()), Block::Item(0, "pt".into())]);
        assert_eq!(t.sections[1], vec![para("plain")]);
    }

    fn epub() -> Vec<u8> {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let opf = r#"<package><manifest><item id="c1" href="text/ch%201.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/><item id="cv" href="cover.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="cv"/><itemref idref="c2"/><itemref idref="c1"/><itemref idref="missing"/></spine></package>"#;
        let ch1 = r#"<html xmlns="x"><head><title>skip me</title><style>p{}</style></head><body>
<h1>Chapter 1</h1>
<p>He said&nbsp;hi &amp; left.<br/>Next line.</p>
<p>日本語の
文章<ruby>漢<rt>かん</rt></ruby>字</p>
<ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>
<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td><p>2</p></td></tr></table>
<pre>x  y
z</pre>
<script>alert(1)</script>
</body></html>"#;
        let ch2 = r#"<html><body><div>bare text in div</div><svg><title>no</title></svg></body></html>"#;
        let cover = r#"<html><body><img src="c.jpg"/></body></html>"#;
        make_zip(&[
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", opf),
            ("OEBPS/text/ch 1.xhtml", ch1),
            ("OEBPS/text/ch2.xhtml", ch2),
            ("OEBPS/cover.xhtml", cover),
        ])
    }

    #[test]
    fn epub_follows_the_spine_and_reads_xhtml() {
        let t = read(&epub(), Format::Epub).unwrap();
        assert_eq!(t.sections.len(), 2); // 表紙（画像だけ）と、無い章は数えない
        assert_eq!(t.sections[0], vec![para("bare text in div")]);
        let c = &t.sections[1];
        assert_eq!(c[0], Block::Heading(1, "Chapter 1".into()));
        assert_eq!(c[1], para("He said hi & left.\nNext line."));
        assert_eq!(c[2], para("日本語の文章漢字")); // 日本語どうしの改行は詰める。ふりがなは出さない
        assert_eq!(c[3], Block::Item(0, "one".into()));
        assert_eq!(c[4], Block::Item(0, "two".into()));
        assert_eq!(c[5], Block::Item(1, "deep".into()));
        assert_eq!(c[6], Block::Table(vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]]));
        assert_eq!(c[7], para("x  y\nz")); // pre は空白と改行をそのまま
        assert_eq!(c.len(), 8); // head / script は出ない
    }

    #[test]
    fn epub_without_chapters_is_an_error() {
        let container = r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#;
        let z = make_zip(&[("META-INF/container.xml", container), ("c.opf", "<package><spine/></package>")]);
        assert!(read(&z, Format::Epub).is_err());
    }

    #[test]
    fn cap_cuts_at_a_block_boundary() {
        let big = "x".repeat(MAX_CHARS / 2 + 1);
        let mut s = vec![vec![para(&big), para(&big), para("tail")], vec![para("next")]];
        assert!(cap(&mut s));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].len(), 2); // 2 つ目で上限を超えるので、そこまで残して後ろを捨てる
        let mut small = vec![vec![para("a")]];
        assert!(!cap(&mut small));
    }

    #[test]
    fn html_has_headings_dividers_tables_and_escapes() {
        let t = Text {
            sections: vec![
                vec![Block::Heading(1, "T <1>".into()), para("a & b\nc\td"), Block::Item(1, "i".into()), Block::Table(vec![vec!["h".into()], vec!["v".into()]])],
                vec![para("second")],
            ],
            truncated: true,
        };
        let h = to_html(&t, "#000", "#fff", "#888");
        assert!(h.contains("<h1>T &lt;1&gt;</h1>"));
        assert!(h.contains("1 / 2") && h.contains("2 / 2"));
        assert!(h.contains("a &amp; b<br>c&nbsp;&nbsp;&nbsp;&nbsp;d"));
        assert!(h.contains("&nbsp;&nbsp;&nbsp;&nbsp;・i"));
        assert!(h.contains("<table") && h.contains("<th>h</th>"));
        assert!(h.contains("途中までを表示"));
        assert!(!h.contains("<1>"));
        let one = to_html(&Text { sections: vec![vec![para("x")]], truncated: false }, "#000", "#fff", "#888");
        assert!(!one.contains(" / 1 ")); // 区切りが 1 つなら出さない
        assert!(to_html(&Text { sections: vec![vec![]], truncated: false }, "#000", "#fff", "#888").contains("取り出せませんでした"));
    }

    #[test]
    fn plain_text_joins_blocks() {
        let t = Text {
            sections: vec![vec![
                Block::Heading(1, "H".into()),
                para("p"),
                Block::Item(0, "a".into()),
                Block::Item(1, "b".into()),
                Block::Table(vec![vec!["x".into(), "y".into()], vec!["1".into(), "2".into()]]),
            ]],
            truncated: false,
        };
        assert_eq!(to_plain(&t), "H\n\np\n\n・a\n  ・b\n\nx\ty\n1\t2");
    }
}
