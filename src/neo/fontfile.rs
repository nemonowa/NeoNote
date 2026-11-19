// フォント（ttf・otf・ttc）の名前と、入っている文字の一覧（Unicode の順）。読むのは ttf-parser（FLTK が既に使っている部品）。

pub struct FontInfo {
    /// 正式な名前（無ければファミリー名）
    pub name: String,
    pub style: String,
    pub glyphs: u16,
    /// 入っている文字（Unicode の番号、小さい順、重なりなし）
    pub chars: Vec<u32>,
}

/// ttc は最初のフォントを読む。読めなければ None。
pub fn info(bytes: &[u8]) -> Option<FontInfo> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let name_of = |id: u16| face.names().into_iter().filter(|n| n.name_id == id).find_map(|n| if n.is_unicode() { n.to_string() } else { None });
    let family = name_of(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name_of(ttf_parser::name_id::FAMILY)).unwrap_or_default();
    let style = name_of(ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY).or_else(|| name_of(ttf_parser::name_id::SUBFAMILY)).unwrap_or_default();
    let name = name_of(ttf_parser::name_id::FULL_NAME).unwrap_or_else(|| family.clone());
    let mut chars = Vec::new();
    if let Some(cmap) = face.tables().cmap {
        for sub in cmap.subtables {
            if sub.is_unicode() {
                sub.codepoints(|c| {
                    // 制御文字は描けない（^A などになる）ので並べない
                    if !(c < 0x20 || (0x7F..0xA0).contains(&c)) && sub.glyph_index(c).is_some_and(|g| g.0 != 0) {
                        chars.push(c);
                    }
                });
            }
        }
    }
    chars.sort_unstable();
    chars.dedup();
    Some(FontInfo { name, style, glyphs: face.number_of_glyphs(), chars })
}

/// Unicode の区画の名前（大まかに。表の見出しに使う）。
pub fn block_name(c: u32) -> &'static str {
    match c {
        0x0000..=0x007F => "Basic Latin",
        0x0080..=0x00FF => "Latin-1",
        0x0100..=0x024F => "Latin Extended",
        0x0250..=0x02FF => "IPA / Spacing Modifiers",
        0x0300..=0x036F => "Combining Marks",
        0x0370..=0x03FF => "Greek",
        0x0400..=0x04FF => "Cyrillic",
        0x0590..=0x05FF => "Hebrew",
        0x0600..=0x06FF => "Arabic",
        0x0E00..=0x0E7F => "Thai",
        0x1E00..=0x1EFF => "Latin Extended Additional",
        0x2000..=0x206F => "General Punctuation",
        0x2070..=0x20CF => "Super/Subscripts, Currency",
        0x2100..=0x214F => "Letterlike Symbols",
        0x2150..=0x218F => "Number Forms",
        0x2190..=0x21FF => "Arrows",
        0x2200..=0x22FF => "Math Operators",
        0x2300..=0x23FF => "Misc Technical",
        0x2460..=0x24FF => "Enclosed Alphanumerics",
        0x2500..=0x257F => "Box Drawing",
        0x2580..=0x25FF => "Blocks / Shapes",
        0x2600..=0x27BF => "Symbols / Dingbats",
        0x2E80..=0x2FDF => "CJK Radicals",
        0x3000..=0x303F => "CJK Symbols",
        0x3040..=0x309F => "Hiragana",
        0x30A0..=0x30FF => "Katakana",
        0x3100..=0x33FF => "CJK Compatibility",
        0x3400..=0x4DBF => "CJK Ext A",
        0x4E00..=0x9FFF => "CJK Unified Ideographs",
        0xAC00..=0xD7AF => "Hangul",
        0xE000..=0xF8FF => "Private Use",
        0xF900..=0xFAFF => "CJK Compatibility Ideographs",
        0xFE30..=0xFE4F => "CJK Compatibility Forms",
        0xFF00..=0xFFEF => "Halfwidth / Fullwidth",
        0x1F000..=0x1FAFF => "Emoji / Symbols",
        0x20000..=0x2FFFF => "CJK Ext B-",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_and_names() {
        assert!(info(b"not a font").is_none());
        assert!(info(&[]).is_none());
        assert_eq!(block_name(0x3042), "Hiragana");
    }

    #[test]
    fn bundled_font_lists_chars_in_order() {
        let f = info(include_bytes!("../../assets/NeoFont.ttf")).unwrap();
        assert!(!f.name.is_empty());
        assert!(f.chars.windows(2).all(|w| w[0] < w[1]));
        assert!(f.chars.contains(&('A' as u32)));
    }
}
