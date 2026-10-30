// 表示モードの基盤（GUI なし）。ファイルの種類ごとに「どの表示を選べるか」を決める。
// 実装済みは Text（編集）・Hex・md / html・htm / csv・tsv / xlsx・ods / docx・odt・pptx・odp・epub / pdf / 画像 / 音声 / フォルダ・zip の Preview。
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Text,
    Hex,
    Preview,
}

impl View {
    /// 中身が実装済みで、選べるか。Preview は md・html・csv / tsv・pdf・画像・音声・フォルダ / zip。
    pub fn implemented(self, kind: Kind) -> bool {
        !matches!(self, View::Preview) || matches!(kind, Kind::Markdown | Kind::Html | Kind::Sheet | Kind::Book | Kind::Doc | Kind::Pdf | Kind::Image | Kind::Archive | Kind::Audio | Kind::Nbt)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Markdown,
    /// html / htm / xhtml。HelpView が描ける範囲の Preview（読み取り専用の簡易対応。CSS・JS は効かない。編集は Text で行う）。
    Html,
    /// csv / tsv。罫線つきの表の Preview（読み取り専用の簡易対応。編集は Text で行う）。
    Sheet,
    /// xlsx / xlsm / ods。シートごとの表の Preview（読み取り専用。zip + XML を自前で読む。バイナリなので HEX でも開ける）。
    Book,
    /// docx / odt / pptx / odp / epub。文字だけの Preview（読み取り専用。zip + XML を自前で読む。バイナリなので HEX でも開ける）。
    Doc,
    Pdf,
    Image,
    /// フォルダ・zip・tar・tar.gz・gz。階層ツリーの Preview だけ（中身を文字や HEX では開かない）。
    Archive,
    /// mp3 / flac / wav。情報とタグの Preview（再生・停止つき）。
    Audio,
    /// level.dat などの NBT。Text は SNBT、Preview は階層の木（編集できる）。
    Nbt,
    Other,
}

pub fn kind_of(path: Option<&Path>) -> Kind {
    if path.is_some_and(crate::neo::archive::is_archive_path) {
        return Kind::Archive;
    }
    let ext = path.and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("md" | "markdown" | "mdown") => Kind::Markdown,
        Some("html" | "htm" | "xhtml") => Kind::Html,
        Some("csv" | "tsv" | "tab") => Kind::Sheet,
        Some("xlsx" | "xlsm" | "ods") => Kind::Book,
        Some("docx" | "docm" | "odt" | "pptx" | "pptm" | "odp" | "epub") => Kind::Doc,
        Some("pdf") => Kind::Pdf,
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp") => Kind::Image,
        Some("mp3" | "flac" | "wav") => Kind::Audio,
        _ if path.is_some_and(crate::neo::nbt::is_nbt_path) => Kind::Nbt,
        _ => Kind::Other,
    }
}

/// 形式の自動判定で見る、本文の先頭の大きさ。
pub const SNIFF_BYTES: i32 = 8192;

/// 拡張子で決まらない文字の文書の形式を、先頭の中身から推す（html・Markdown・CSV / TSV。決まらなければ Other）。
pub fn sniff(text: &str) -> Kind {
    let head = text.trim_start_matches('\u{feff}').trim_start();
    let low: String = head.chars().take(64).collect::<String>().to_lowercase();
    if low.starts_with("<!doctype html") || low.starts_with("<html") {
        return Kind::Html;
    }
    let lines: Vec<&str> = head.lines().take(50).collect();
    let md = lines
        .iter()
        .filter(|l| {
            let t = l.trim_start();
            (t.starts_with('#') && t.trim_start_matches('#').starts_with(' ')) || t.starts_with("```") || t.starts_with("- ") || t.starts_with("* ") || t.starts_with("> ") || (t.contains("](") && t.contains('['))
        })
        .count();
    if md >= 2 || lines.first().is_some_and(|l| l.starts_with("# ")) {
        return Kind::Markdown;
    }
    for sep in [',', '\t'] {
        let rows: Vec<usize> = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.matches(sep).count()).collect();
        if rows.len() >= 2 && rows[0] >= 1 && rows.iter().all(|&n| n == rows[0]) {
            return Kind::Sheet;
        }
    }
    Kind::Other
}

/// Auto の行に添える、いまの形式の名前。
pub fn kind_name(kind: Kind, preview: bool, hex: bool) -> &'static str {
    if hex && !preview {
        return "HEX";
    }
    match kind {
        Kind::Markdown => "Markdown",
        Kind::Html => "HTML",
        Kind::Sheet => "CSV",
        Kind::Book => "Sheet",
        Kind::Doc => "Document",
        Kind::Pdf => "PDF",
        Kind::Image => "Image",
        Kind::Archive => "Folder",
        Kind::Audio => "Audio",
        Kind::Nbt => "NBT",
        Kind::Other => "Text",
    }
}

/// そのファイルで選べる表示（並べる順）。Preview があるものは、文字や HEX でも開き直せる（手動で何でも選べる）。
#[cfg(test)]
pub fn available(path: Option<&Path>) -> Vec<View> {
    available_for(kind_of(path))
}

pub fn available_for(kind: Kind) -> Vec<View> {
    match kind {
        Kind::Markdown | Kind::Html | Kind::Sheet | Kind::Nbt | Kind::Book | Kind::Doc | Kind::Pdf | Kind::Image | Kind::Audio | Kind::Archive => {
            vec![View::Text, View::Preview, View::Hex]
        }
        Kind::Other => vec![View::Text, View::Hex],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sniff_guesses_format() {
        assert_eq!(sniff("<!DOCTYPE html><p>a"), Kind::Html);
        assert_eq!(sniff("# Title\nbody"), Kind::Markdown);
        assert_eq!(sniff("a,b,c\n1,2,3\n"), Kind::Sheet);
        assert_eq!(sniff("just a memo, nothing else"), Kind::Other);
    }

    #[test]
    fn kind_follows_the_extension_case_insensitively() {
        assert_eq!(kind_of(Some(Path::new("a/README.MD"))), Kind::Markdown);
        assert_eq!(kind_of(Some(Path::new("x.pdf"))), Kind::Pdf);
        assert_eq!(kind_of(Some(Path::new("d/Data.CSV"))), Kind::Sheet);
        assert_eq!(kind_of(Some(Path::new("x.tsv"))), Kind::Sheet);
        assert_eq!(kind_of(Some(Path::new("x.txt"))), Kind::Other);
        assert_eq!(kind_of(Some(Path::new("a/B.JPG"))), Kind::Image);
        assert_eq!(kind_of(Some(Path::new("x.svg"))), Kind::Other);
        assert_eq!(kind_of(Some(Path::new("a/B.FLAC"))), Kind::Audio);
        assert_eq!(kind_of(Some(Path::new("x.m4a"))), Kind::Other);
        assert_eq!(kind_of(Some(Path::new("noext"))), Kind::Other);
        assert_eq!(kind_of(None), Kind::Other);
        assert_eq!(kind_of(Some(Path::new("a/B.ZIP"))), Kind::Archive);
        assert_eq!(kind_of(Some(&std::env::temp_dir())), Kind::Archive); // フォルダ
    }

    #[test]
    fn available_views_depend_on_the_kind() {
        assert_eq!(available(None), vec![View::Text, View::Hex]);
        assert_eq!(available(Some(Path::new("a.md"))), vec![View::Text, View::Preview, View::Hex]);
        assert_eq!(available(Some(Path::new("a.pdf"))), vec![View::Text, View::Preview, View::Hex]);
        assert_eq!(available(Some(Path::new("a.xlsx"))), vec![View::Text, View::Preview, View::Hex]);
        assert_eq!(kind_of(Some(Path::new("a.ODS"))), Kind::Book);
        for ext in ["docx", "DOCM", "odt", "pptx", "pptm", "odp", "EPUB"] {
            let p = format!("a.{ext}");
            assert_eq!(kind_of(Some(Path::new(&p))), Kind::Doc, "{ext}");
            assert!(crate::neo::doctext::format_of(Path::new(&p)).is_some(), "{ext}");
            assert_eq!(available(Some(Path::new(&p))), vec![View::Text, View::Preview, View::Hex]);
        }
        for ext in ["html", "HTM", "xhtml"] {
            let p = format!("a.{ext}");
            assert_eq!(kind_of(Some(Path::new(&p))), Kind::Html, "{ext}");
            assert_eq!(available(Some(Path::new(&p))), vec![View::Text, View::Preview, View::Hex]);
        }
        assert!(View::Preview.implemented(Kind::Html));
        assert_eq!(kind_of(Some(Path::new("a.doc"))), Kind::Other);
        assert!(View::Preview.implemented(Kind::Doc));
        assert!(View::Preview.implemented(Kind::Book));
        assert_eq!(available(Some(Path::new("a.png"))), vec![View::Text, View::Preview, View::Hex]);
        assert_eq!(available(Some(Path::new("a.mp3"))), vec![View::Text, View::Preview, View::Hex]);
        assert!(View::Preview.implemented(Kind::Audio));
        assert!(View::Text.implemented(Kind::Other) && View::Hex.implemented(Kind::Pdf));
        assert!(View::Preview.implemented(Kind::Markdown));
        assert!(View::Preview.implemented(Kind::Pdf));
        assert!(View::Preview.implemented(Kind::Archive));
        assert_eq!(available(Some(Path::new("a.zip"))), vec![View::Text, View::Preview, View::Hex]);
        for name in ["a.tar", "a.TAR.GZ", "a.tgz", "a.gz"] {
            assert_eq!(kind_of(Some(Path::new(name))), Kind::Archive, "{name}");
            assert_eq!(available(Some(Path::new(name))), vec![View::Text, View::Preview, View::Hex]);
        }
        assert_eq!(kind_of(Some(Path::new("a.bz2"))), Kind::Other);
        assert!(!View::Preview.implemented(Kind::Other));
    }
}
