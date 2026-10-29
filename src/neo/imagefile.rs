// 画像の Preview 用の部品（GUI なし）。画像の読み込みと描画は FLTK（HelpView）に任せ、
// ここでは「どのファイルが画像か」「md の画像パスをどう探すか」「表示サイズをどう縮めるか」だけを決める。
use std::path::{Path, PathBuf};

/// FLTK が標準で読める形式のうち、文字として編集しない（バイナリの）ものだけ。svg はテキストなので入れない。
const EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp"];
/// これより大きい画像ファイルは、md の中でも読み込まない（メモ帳が固まらないように）。
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

pub fn is_image_path(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| EXTS.contains(&e.as_str()))
}

/// 先頭のバイト列から画像の種類（拡張子）を判定する。URL から落としたファイルが本当に画像かの確認に使う。
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("gif")
    } else if b.starts_with(b"BM") && b.len() > 14 {
        Some("bmp")
    } else {
        None
    }
}

/// 幅が `max_w` を超えるときだけ、縦横比を保って縮める。
pub fn fit(w: i32, h: i32, max_w: i32) -> (i32, i32) {
    if w <= 0 || h <= 0 || max_w <= 0 || w <= max_w {
        return (w, h);
    }
    (max_w, ((h as i64 * max_w as i64) / w as i64).max(1) as i32)
}

/// `%20` などを戻す（md の画像パスは空白を `%20` と書くことがある）。不正な並びはそのまま残す。
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// md の画像の参照先を、実在するローカルの画像ファイルに解決する。URL（`://`・`//` で始まる・`data:`）は None。
/// 相対パスは `base`（md のあるフォルダ）を基準にする。
pub fn resolve_local(base: Option<&Path>, dest: &str) -> Option<PathBuf> {
    let d = dest.trim();
    if d.is_empty() || d.contains("://") || d.starts_with("//") || d.to_ascii_lowercase().starts_with("data:") {
        return None;
    }
    let d = percent_decode(d);
    let p = Path::new(&d);
    let full = if p.is_absolute() { p.to_path_buf() } else { base?.join(p) };
    let meta = std::fs::metadata(&full).ok()?;
    (meta.is_file() && meta.len() <= MAX_BYTES && is_image_path(&full)).then_some(full)
}

/// HelpView に渡す `<img>`。`size` は画像の (幅, 高さ)、`max_w` は表示できる最大幅。
pub fn img_tag(path: &Path, size: Option<(i32, i32)>, max_w: i32) -> String {
    let src = path.to_string_lossy().replace('"', "%22");
    match size {
        Some((w, h)) => {
            let (w, h) = fit(w, h, max_w);
            format!("<img src=\"{src}\" width=\"{w}\" height=\"{h}\">")
        }
        None => format!("<img src=\"{src}\">"),
    }
}

/// 画像 1 枚だけのページ（画像ファイルの Preview と、URL から開いた画像のウィンドウ）。
/// 画像を読めなかった（`size` が None）ときは、その旨の 1 行にする。
pub fn page_html(path: &Path, size: Option<(i32, i32)>, max_w: i32, bg: &str, fg: &str) -> String {
    let body = match size {
        Some(_) => format!("<p>{}</p>", img_tag(path, size, max_w)),
        None => crate::lang::tr("<p>画像を読めませんでした（壊れている、または対応していない形式です）。</p>", "<p>Could not read the image (it is broken, or the format is not supported).</p>").to_string(),
    };
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths_follow_the_extension() {
        assert!(is_image_path(Path::new("a/B.PNG")));
        assert!(is_image_path(Path::new("x.jpeg")));
        assert!(!is_image_path(Path::new("x.svg")));
        assert!(!is_image_path(Path::new("x.txt")));
        assert!(!is_image_path(Path::new("noext")));
    }

    #[test]
    fn sniff_recognizes_magic_numbers() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a...."), Some("gif"));
        assert_eq!(sniff(b"BM0123456789012345"), Some("bmp"));
        assert_eq!(sniff(b"<html>not an image</html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn fit_only_shrinks_wide_images() {
        assert_eq!(fit(200, 100, 400), (200, 100));
        assert_eq!(fit(800, 400, 400), (400, 200));
        assert_eq!(fit(1000, 1, 10), (10, 1));
        assert_eq!(fit(0, 0, 10), (0, 0));
    }

    #[test]
    fn percent_decoding_keeps_bad_sequences() {
        assert_eq!(percent_decode("my%20pic.png"), "my pic.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("%E3%81%82.png"), "あ.png");
        assert_eq!(percent_decode("%2"), "%2");
    }

    #[test]
    fn resolve_local_skips_urls_and_missing_files() {
        let dir = std::env::temp_dir().join(format!("neonote_img_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("my pic.png"), b"x").unwrap();
        std::fs::write(dir.join("note.txt"), b"x").unwrap();
        assert_eq!(resolve_local(Some(&dir), "my%20pic.png"), Some(dir.join("my pic.png")));
        assert_eq!(resolve_local(Some(&dir), "./my pic.png"), Some(dir.join("./my pic.png")));
        assert_eq!(resolve_local(Some(&dir), "note.txt"), None); // 画像ではない
        assert_eq!(resolve_local(Some(&dir), "missing.png"), None);
        assert_eq!(resolve_local(Some(&dir), "http://example.com/a.png"), None);
        assert_eq!(resolve_local(Some(&dir), "//example.com/a.png"), None);
        assert_eq!(resolve_local(Some(&dir), "data:image/png;base64,AAAA"), None);
        assert_eq!(resolve_local(None, "my pic.png"), None); // 基準が無い相対パス
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_html_shows_the_image_or_a_message() {
        let ok = page_html(Path::new("/a.png"), Some((10, 10)), 400, "#000", "#fff");
        assert!(ok.contains("<img src=\"/a.png\"") && ok.contains("bgcolor=\"#000\""));
        assert!(page_html(Path::new("/a.png"), None, 400, "#000", "#fff").contains("読めませんでした"));
    }

    #[test]
    fn img_tag_scales_and_quotes() {
        let t = img_tag(Path::new("/a/b\"c.png"), Some((800, 400)), 400);
        assert_eq!(t, "<img src=\"/a/b%22c.png\" width=\"400\" height=\"200\">");
        assert_eq!(img_tag(Path::new("/a.png"), None, 400), "<img src=\"/a.png\">");
    }
}
