// Markdown → FLTK HelpView 用の HTML（GUI なし）。
// HelpView は HTML の小さな部分集合しか描けず CSS も効かないので、GitHub の見た目
// （コードの背景・引用の帯・表の見出し・見出しの下線・注記）は、表と色の属性で近づける。
// 描けないもの（生の HTML・リンク先の読み込み）は文字として出す。
use super::imagefile;
use pulldown_cmark::{html, BlockQuoteKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::Path;

/// md の中の画像をどう出すかの手がかり。
pub struct Images<'a> {
    /// 相対パスの基準（md のあるフォルダ）。保存前の文書は None（相対パスの画像は出さない）。
    pub base: Option<&'a Path>,
    /// 表示できる最大幅（これを超える画像は縮める）。
    pub max_w: i32,
    /// 画像ファイルの (幅, 高さ)。GUI 側が FLTK で読んで返す。
    pub size_of: &'a dyn Fn(&Path) -> Option<(i32, i32)>,
}

/// 罫線つきの表の開始タグ。
const TABLE_OPEN: &str = "<table border=\"1\" cellspacing=\"0\" cellpadding=\"6\">";

/// プレビューの色（どれも `#rrggbb`）。
pub struct Colors<'a> {
    pub bg: &'a str,
    pub fg: &'a str,
    pub link: &'a str,
    /// コードの背景・表の見出しの背景。
    pub sub: &'a str,
    /// 引用の文字。
    pub subtext: &'a str,
    /// 引用の帯。
    pub border: &'a str,
}

/// GitHub の注記（`> [!NOTE]` など）の色と見出し。
fn alert(kind: BlockQuoteKind) -> (&'static str, &'static str) {
    match kind {
        BlockQuoteKind::Note => ("#4493f8", "\u{24D8} Note"),
        BlockQuoteKind::Tip => ("#3fb950", "\u{2606} Tip"),
        BlockQuoteKind::Important => ("#ab7df8", "\u{2709} Important"),
        BlockQuoteKind::Warning => ("#d29922", "\u{26A0} Warning"),
        BlockQuoteKind::Caution => ("#f85149", "\u{2716} Caution"),
    }
}

fn html_ev(s: String) -> Event<'static> {
    Event::Html(CowStr::from(s))
}

/// 本文（md）を、HelpView に渡す完全な HTML にする。
pub fn to_html(md: &str, c: &Colors, imgs: &Images) -> String {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM;
    // リンクは HelpView に辿らせない（辿ると、ファイルとして読みに行って失敗する）。
    // 文字はリンクの色で出し、リンク先は出さない（GitHub と同じ見た目）。
    let mut events: Vec<Event> = Vec::new();
    // 開いている引用ごとに、閉じる時に font も閉じるか（普通の引用は文字を薄くする）
    let mut quotes: Vec<bool> = Vec::new();
    // 画像: 中の文字（alt）を集めておき、終わりで「ローカルの画像なら <img>、そうでなければ alt（+ URL）」にする
    let mut img: Option<(String, String)> = None; // (参照先, alt)
    for ev in Parser::new_ext(md, opts) {
        if let Some((_, alt)) = img.as_mut() {
            match ev {
                Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                Event::End(TagEnd::Image) => {
                    let (d, alt) = img.take().unwrap();
                    match imagefile::resolve_local(imgs.base, &d) {
                        Some(p) => {
                            let tag = imagefile::img_tag(&p, (imgs.size_of)(&p), imgs.max_w);
                            events.push(Event::Html(format!("<p>{tag}</p>").into()));
                        }
                        None => {
                            // 読み込まない（URL・見つからない・画像ではない）。alt と参照先を文字で出す
                            let shown = if alt.is_empty() { format!("({d})") } else { format!("{alt} ({d})") };
                            events.push(Event::Text(shown.into()));
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Image { dest_url, .. }) => img = Some((dest_url.into_string(), String::new())),
            // 生の HTML はタグとして解釈させない（そのまま文字で見せる）
            Event::Html(s) | Event::InlineHtml(s) => events.push(Event::Text(s)),
            Event::Start(Tag::Link { .. }) => events.push(html_ev(format!("<font color=\"{}\">", c.link))),
            Event::End(TagEnd::Link) => events.push(html_ev("</font>".into())),
            // コードは背景つきの枠に入れる（HelpView が背景を描けるのは表だけ）
            Event::Start(Tag::CodeBlock(_)) => events.push(html_ev(format!(
                "<table width=\"100%\" border=\"0\" cellspacing=\"0\" cellpadding=\"10\"><tr><td bgcolor=\"{}\"><pre>",
                c.sub
            ))),
            Event::End(TagEnd::CodeBlock) => events.push(html_ev("</pre></td></tr></table>".into())),
            // 引用: 左の帯を細い列で描く。注記（[!NOTE] など）は帯と見出しに色を付ける
            Event::Start(Tag::BlockQuote(kind)) => {
                let (bar, head) = match kind {
                    Some(k) => {
                        let (col, title) = alert(k);
                        (col, format!("<p><font color=\"{col}\"><b>{title}</b></font></p>"))
                    }
                    None => (c.border, format!("<font color=\"{}\">", c.subtext)),
                };
                quotes.push(kind.is_none());
                events.push(html_ev(format!(
                    "<table width=\"100%\" border=\"0\" cellspacing=\"0\" cellpadding=\"0\"><tr>\
                     <td bgcolor=\"{bar}\" width=\"4\"><font size=\"1\">&nbsp;</font></td><td width=\"14\"></td><td width=\"99%\">{head}"
                )));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                let font = if quotes.pop().unwrap_or(false) { "</font>" } else { "" };
                events.push(html_ev(format!("{font}</td></tr></table>")));
            }
            Event::TaskListMarker(done) => events.push(Event::Text(if done { "\u{2611} " } else { "\u{2610} " }.into())),
            // 大きな見出しの下には線を引く
            Event::End(TagEnd::Heading(level)) => {
                events.push(Event::End(TagEnd::Heading(level)));
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    events.push(html_ev("<hr>".into()));
                }
            }
            e => events.push(e),
        }
    }
    let mut body = String::with_capacity(md.len() * 2);
    html::push_html(&mut body, events.into_iter());
    // 表は罫線つきで出す（簡易対応。HelpView が描ける `border` / `cellpadding` だけを使う）。
    // 本文中の「<table>」という文字は `&lt;` に直されているので、ここで置き換わるのは表のタグだけ。
    let body = body
        .replace("<table>", TABLE_OPEN)
        .replace("<thead><tr>", &format!("<thead><tr bgcolor=\"{}\">", c.sub));
    format!("<html><body bgcolor=\"{}\" text=\"{}\" link=\"{}\">{body}</body></html>", c.bg, c.fg, c.link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(md: &str) -> String {
        conv_in(md, None)
    }

    fn conv_in(md: &str, base: Option<&Path>) -> String {
        let c = Colors { bg: "#000000", fg: "#ffffff", link: "#00ff00", sub: "#111111", subtext: "#888888", border: "#444444" };
        to_html(md, &c, &Images { base, max_w: 400, size_of: &|_| Some((800, 400)) })
    }

    #[test]
    fn headings_lists_and_emphasis() {
        let h = conv("# T\n\n- a\n- **b**\n\n*i*");
        assert!(h.contains("<h1>T</h1>"));
        assert!(h.contains("<li>a</li>"));
        assert!(h.contains("<strong>b</strong>"));
        assert!(h.contains("<em>i</em>"));
    }

    #[test]
    fn colors_go_on_the_body_tag() {
        let h = conv("x");
        assert!(h.starts_with("<html><body bgcolor=\"#000000\" text=\"#ffffff\" link=\"#00ff00\">"));
        assert!(h.ends_with("</body></html>"));
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        let h = conv("<script>alert(1)</script>\n\na <b>x</b> b");
        assert!(!h.contains("<script>"));
        assert!(!h.contains("<b>"));
        assert!(h.contains("&lt;script&gt;"));
    }

    #[test]
    fn remote_images_become_alt_text_with_the_url() {
        let h = conv("![logo](http://example.com/a.png)");
        assert!(!h.contains("<img"));
        assert!(h.contains("logo (http://example.com/a.png)"));
        assert!(conv("![](http://example.com/a.png)").contains("(http://example.com/a.png)"));
    }

    #[test]
    fn local_images_are_embedded_and_scaled() {
        let dir = std::env::temp_dir().join(format!("neonote_md_img_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pic.png"), b"x").unwrap();
        let h = conv_in("![alt text](pic.png)\n\n![gone](none.png)", Some(&dir));
        assert!(h.contains(&format!("<img src=\"{}\" width=\"400\" height=\"200\">", dir.join("pic.png").display())));
        assert!(!h.contains("alt text")); // 画像にできたら alt は出さない
        assert!(h.contains("gone (none.png)")); // 見つからなければ文字で
        // 保存前の文書（基準が無い）は相対パスの画像を出さない
        assert!(!conv_in("![a](pic.png)", None).contains("<img"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_become_colored_text() {
        let h = conv("[site](http://example.com/a) and [top](#top)");
        assert!(!h.contains("<a "));
        assert!(h.contains("<font color=\"#00ff00\">site</font>"));
        assert!(!h.contains("example.com"));
    }

    #[test]
    fn github_extras() {
        let h = conv("# T\n\n- [x] done\n- [ ] todo\n\n> q\n\n> [!WARNING]\n> w\n\n|a|\n|-|\n|1|");
        assert!(h.contains("</h1>\n<hr>"));
        assert!(h.contains("\u{2611} done") && h.contains("\u{2610} todo"));
        assert!(h.contains("bgcolor=\"#444444\"") && h.contains("<font color=\"#888888\">"));
        assert!(h.contains("#d29922") && h.contains("Warning"));
        assert!(h.contains("<thead><tr bgcolor=\"#111111\">"));
    }

    #[test]
    fn tables_and_code_blocks_are_kept() {
        let h = conv("|a|b|\n|-|-|\n|1|2|\n\n```\nlet x = 1;\n```");
        assert!(h.contains(TABLE_OPEN));
        assert!(h.contains("<td bgcolor=\"#111111\"><pre>let x = 1;"));
    }

    #[test]
    fn tables_get_borders_but_the_word_table_in_text_does_not_change() {
        let h = conv("|a|b|\n|-|-|\n|1|2|\n\n`<table>` と <table> の文字");
        assert_eq!(h.matches("border=\"1\"").count(), 1);
        assert!(h.contains("&lt;table&gt;"));
    }

    #[test]
    fn empty_text_still_gives_a_page() {
        assert!(conv("").contains("<body"));
    }
}
