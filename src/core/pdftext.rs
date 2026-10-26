// PDF から文字だけを取り出す簡易表示用（GUI なし）。レイアウト・画像・フォントは再現しない。
// 文字の取り出しは lopdf。ToUnicode を持たないフォントや画像だけのページ（スキャン）は文字が出ない。
use lopdf::Document;

/// これを超える PDF は読まない（壊れた PDF による暴走・メモリ消費を避ける）。
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// ページごとの文字（ページ番号は 1 から）。
pub fn extract(bytes: &[u8]) -> Result<Vec<String>, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("PDF が大きすぎます（上限 {} MiB）", MAX_BYTES / 1024 / 1024));
    }
    let doc = Document::load_mem(bytes).map_err(|e| format!("PDF を読めません: {e}"))?;
    if doc.is_encrypted() {
        return Err("暗号化された PDF は表示できません".into());
    }
    let nums: Vec<u32> = doc.get_pages().keys().copied().collect();
    // 1 ページずつ取る（取れなかったページだけ空にして、残りは出す）
    Ok(nums.iter().map(|n| doc.extract_text(&[*n]).unwrap_or_default()).collect())
}

fn esc(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' | '\u{c}' => {}
            c => out.push(c),
        }
    }
}

/// ページごとの文字を、HelpView に渡す HTML にする。色は `#rrggbb`。
pub fn to_html(pages: &[String], bg: &str, fg: &str, dim: &str) -> String {
    let mut body = String::new();
    let total = pages.len();
    let empty = pages.iter().all(|p| p.trim().is_empty());
    if empty {
        body.push_str("<p>文字を取り出せませんでした（画像だけのページ、または対応していないフォントです）。</p>");
    }
    for (i, p) in pages.iter().enumerate() {
        body.push_str(&format!("<p><font color=\"{dim}\">── {} / {total} ──</font></p>", i + 1));
        for line in p.lines().filter(|l| !l.trim().is_empty()) {
            body.push_str("<p>");
            esc(line, &mut body);
            body.push_str("</p>");
        }
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

/// 読み込み中・失敗のときの 1 行表示。
pub fn message_html(msg: &str, bg: &str, fg: &str) -> String {
    let mut m = String::new();
    esc(msg, &mut m);
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\"><p>{m}</p></body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{content::{Content, Operation}, dictionary, Object, Stream};

    fn sample(lines: &[&str]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let mut ops = vec![Operation::new("BT", vec![]), Operation::new("Tf", vec!["F1".into(), 12.into()])];
        for (i, l) in lines.iter().enumerate() {
            ops.push(Operation::new("Td", vec![0.into(), (if i == 0 { 700 } else { -20 }).into()]));
            ops.push(Operation::new("Tj", vec![Object::string_literal(*l)]));
        }
        ops.push(Operation::new("ET", vec![]));
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }));
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn extracts_text_per_page() {
        let pages = extract(&sample(&["Hello PDF", "second line"])).unwrap();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].contains("Hello PDF"));
        assert!(pages[0].contains("second line"));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(extract(b"not a pdf at all").is_err());
        assert!(extract(b"").is_err());
    }

    #[test]
    fn html_escapes_and_numbers_pages() {
        let h = to_html(&["a <b> & c\n\nnext".into(), "x".into()], "#000", "#fff", "#888");
        assert!(h.contains("1 / 2") && h.contains("2 / 2"));
        assert!(h.contains("a &lt;b&gt; &amp; c"));
        assert!(!h.contains("<b>"));
    }

    #[test]
    fn empty_pages_say_so() {
        assert!(to_html(&["  ".into()], "#000", "#fff", "#888").contains("取り出せませんでした"));
    }
}
