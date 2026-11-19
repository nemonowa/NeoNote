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
    /// mp4・mov・mkv など。再生の画面（Windows は Media Foundation。ほかの OS は再生できない旨だけ）。
    Video,
    /// ttf・otf・ttc。名前と、入っている文字を Unicode の順に並べた見本。
    Font,
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
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "tga" | "dds" | "psd") => Kind::Image,
        Some("mp3" | "flac" | "wav" | "ogg" | "oga" | "m4a" | "aac" | "wma" | "opus") => Kind::Audio,
        Some("ttf" | "otf" | "ttc" | "otc") => Kind::Font,
        Some("mp4" | "m4v" | "mov" | "wmv" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "m2ts" | "3gp" | "flv") => Kind::Video,
        _ if path.is_some_and(crate::neo::nbt::is_nbt_path) => Kind::Nbt,
        _ => Kind::Other,
    }
}

/// 前後へ切り替えられる種類（Preview で見る物。文字の文書・フォルダ・NBT は除く）。
pub fn steppable(kind: Kind) -> bool {
    matches!(kind, Kind::Image | Kind::Pdf | Kind::Book | Kind::Doc | Kind::Audio | Kind::Video | Kind::Font)
}

/// `path` と同じフォルダにある、同じ種類のファイルのうち、名前の順で `step`（-1 前・+1 次）の物。端なら None。
pub fn sibling(path: &Path, step: i32) -> Option<std::path::PathBuf> {
    let kind = kind_of(Some(path));
    let dir = path.parent()?;
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| kind_of(Some(p)) == kind)
        .collect();
    files.sort_by_key(|p| natural_key(&p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()));
    let name = path.file_name()?;
    let i = files.iter().position(|p| p.file_name() == Some(name))? as i64 + step as i64;
    (i >= 0).then(|| files.get(i as usize).cloned()).flatten()
}

/// 名前の中の数字を数として比べる鍵（img2 が img10 より前に来る）。
fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut digits = false;
    for ch in s.chars().chain(std::iter::once('\0')) {
        let d = ch.is_ascii_digit();
        if (d != digits || ch == '\0') && !cur.is_empty() {
            out.push(if digits { (0, cur.parse().unwrap_or(u64::MAX), cur.clone()) } else { (1, 0, cur.clone()) });
            cur.clear();
        }
        digits = d;
        if ch != '\0' {
            cur.push(ch);
        }
    }
    out
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
        Kind::Video => "Video",
        Kind::Font => "Font",
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
        Kind::Markdown | Kind::Html | Kind::Sheet | Kind::Nbt | Kind::Book | Kind::Doc | Kind::Pdf | Kind::Image | Kind::Audio | Kind::Video | Kind::Font | Kind::Archive => {
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
    fn siblings_follow_the_name_order_and_kind() {
        let dir = std::env::temp_dir().join(format!("neonote_sib_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["img10.png", "img2.png", "img1.jpg", "note.txt", "b.mp3"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let p = dir.join("img2.png");
        assert_eq!(sibling(&p, -1).unwrap().file_name().unwrap(), "img1.jpg");
        assert_eq!(sibling(&p, 1).unwrap().file_name().unwrap(), "img10.png");
        assert!(sibling(&dir.join("img10.png"), 1).is_none(), "端");
        assert!(sibling(&dir.join("b.mp3"), 1).is_none(), "ほかに音声は無い");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        assert_eq!(kind_of(Some(Path::new("x.m4a"))), Kind::Audio);
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
