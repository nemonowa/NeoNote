// クリップボード履歴（Copy to Clipboard History / Paste From History。全文書で共通・この起動中だけ。最大 MAX 件、新しい順）。
// View History / Restore From History は、これではなくパス履歴（core/pathlog.rs）。

pub const MAX: usize = 10;
/// これを超える文書はスナップショットを取らない（メモリを食うため）。
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

pub struct Snapshot {
    pub text: String,
}

#[derive(Default)]
pub struct History {
    items: Vec<Snapshot>,
}

impl History {
    /// 先頭に入れる。直前と同じ内容・大きすぎる内容は入れない。入れたら true。
    pub fn push(&mut self, text: &str) -> bool {
        if text.len() > MAX_BYTES || self.items.first().is_some_and(|s| s.text == text) {
            return false;
        }
        self.items.insert(0, Snapshot { text: text.to_string() });
        self.items.truncate(MAX);
        true
    }

    pub fn items(&self) -> &[Snapshot] {
        &self.items
    }
}

/// クリップボード履歴の表示: `1  先頭行の頭 30 文字…  (120 chars)`。FLTK の特殊文字・制御文字は `_` にする。
pub fn clip_label(index: usize, snap: &Snapshot) -> String {
    let first = snap.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut head: String = first.chars().take(30).map(|c| if c.is_control() || matches!(c, '/' | '\\' | '&' | '@') { '_' } else { c }).collect();
    if first.chars().count() > 30 {
        head.push('…');
    }
    format!("{}  {}  ({} chars)", index + 1, head, snap.text.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_capped_and_no_consecutive_duplicates() {
        let mut h = History::default();
        assert!(h.push("a"));
        assert!(!h.push("a"));
        for i in 0..12 {
            h.push(&format!("v{i}"));
        }
        assert_eq!(h.items().len(), MAX);
        assert_eq!(h.items()[0].text, "v11");
        assert!(h.push("a")); // 直前と違えば同じ内容でも入る
    }

    #[test]
    fn oversized_text_is_skipped() {
        let mut h = History::default();
        assert!(!h.push(&"x".repeat(MAX_BYTES + 1)));
        assert!(h.items().is_empty());
    }

    #[test]
    fn clip_label_uses_first_line_and_is_menu_safe() {
        let s = |t: &str| Snapshot { text: t.into() };
        assert_eq!(clip_label(0, &s("
  hello world
second")), "1  hello world  (21 chars)");
        assert!(!clip_label(1, &s("a/b&c@d	e")).contains(['/', '&', '@', '\t']));
        assert!(clip_label(0, &s(&"x".repeat(40))).contains("x…"));
        assert_eq!(clip_label(2, &s("")), "3    (0 chars)");
    }
}
