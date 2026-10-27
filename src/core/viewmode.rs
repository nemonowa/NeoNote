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
    if path.is_some_and(crate::core::archive::is_archive_path) {
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
        _ if path.is_some_and(crate::core::nbt::is_nbt_path) => Kind::Nbt,
        _ => Kind::Other,
    }
}

/// そのファイルで選べる表示（並べる順）。pdf は文字として編集できないので Text を出さない。
#[cfg(test)]
pub fn available(path: Option<&Path>) -> Vec<View> {
    available_for(kind_of(path))
}

pub fn available_for(kind: Kind) -> Vec<View> {
    match kind {
        Kind::Markdown | Kind::Html | Kind::Sheet | Kind::Nbt => vec![View::Text, View::Preview, View::Hex],
        Kind::Book | Kind::Doc | Kind::Pdf | Kind::Image | Kind::Audio => vec![View::Preview, View::Hex],
        Kind::Archive => vec![View::Preview],
        Kind::Other => vec![View::Text, View::Hex],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        assert_eq!(available(Some(Path::new("a.pdf"))), vec![View::Preview, View::Hex]);
        assert_eq!(available(Some(Path::new("a.xlsx"))), vec![View::Preview, View::Hex]);
        assert_eq!(kind_of(Some(Path::new("a.ODS"))), Kind::Book);
        for ext in ["docx", "DOCM", "odt", "pptx", "pptm", "odp", "EPUB"] {
            let p = format!("a.{ext}");
            assert_eq!(kind_of(Some(Path::new(&p))), Kind::Doc, "{ext}");
            assert!(crate::core::doctext::format_of(Path::new(&p)).is_some(), "{ext}");
            assert_eq!(available(Some(Path::new(&p))), vec![View::Preview, View::Hex]);
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
        assert_eq!(available(Some(Path::new("a.png"))), vec![View::Preview, View::Hex]);
        assert_eq!(available(Some(Path::new("a.mp3"))), vec![View::Preview, View::Hex]);
        assert!(View::Preview.implemented(Kind::Audio));
        assert!(View::Text.implemented(Kind::Other) && View::Hex.implemented(Kind::Pdf));
        assert!(View::Preview.implemented(Kind::Markdown));
        assert!(View::Preview.implemented(Kind::Pdf));
        assert!(View::Preview.implemented(Kind::Archive));
        assert_eq!(available(Some(Path::new("a.zip"))), vec![View::Preview]);
        for name in ["a.tar", "a.TAR.GZ", "a.tgz", "a.gz"] {
            assert_eq!(kind_of(Some(Path::new(name))), Kind::Archive, "{name}");
            assert_eq!(available(Some(Path::new(name))), vec![View::Preview]);
        }
        assert_eq!(kind_of(Some(Path::new("a.bz2"))), Kind::Other);
        assert!(!View::Preview.implemented(Kind::Other));
    }
}
