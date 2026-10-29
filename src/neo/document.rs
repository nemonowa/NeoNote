// ファイルの読み書き。第2段階は UTF-8 のみ（エンコーディング対応は第3段階）。
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use super::{encoding::{self, Detected, Encoding}, hex, nbt, region};

#[derive(Debug)]
pub struct Loaded {
    pub text: String,
    pub encoding: Encoding,
    /// 先頭に BOM（UTF-8 / UTF-16）があった（保存時に付け直す）。
    pub bom: bool,
    /// 文字コードを自動判定した（指定されたのではない）。
    pub auto: bool,
    /// HEX モードで読んだ（text は HEX 表示）。
    pub hex: bool,
    /// HEX モードで、上限を超えたため先頭だけ読んだ（保存は無効）。
    pub truncated: bool,
    /// 読み込む直前のディスク上の状態（保存時に、外部で書き換わっていないか確かめる）。
    pub stamp: Option<Stamp>,
    /// NBT（level.dat など）を SNBT の文字にして読んだ。保存するときは NBT に戻す。
    pub nbt: Option<nbt::NbtFile>,
    /// リージョンファイル（.mca）のチャンクの一覧を読んだ（text は一覧。保存はしない）。
    pub region: bool,
}

/// ファイルの更新時刻と大きさ。開いた（保存した）時点と今を比べて、外部での書き換えを見つける。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    mtime: Option<std::time::SystemTime>,
    len: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp { mtime: m.modified().ok(), len: m.len() })
}

/// 記録した状態と今が違うか。記録が無い・ファイルが無い（消された）ときは false（そのまま保存してよい）。
pub fn changed_on_disk(path: &Path, known: Option<Stamp>) -> bool {
    matches!((known, stamp(path)), (Some(k), Some(now)) if k != now)
}

/// 開き方の指定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Text(Encoding),
    Hex,
}

fn hex_loaded(bytes: &[u8]) -> Loaded {
    let truncated = bytes.len() > hex::MAX_BYTES;
    let shown = &bytes[..bytes.len().min(hex::MAX_BYTES)];
    Loaded { text: hex::format(shown), encoding: Encoding::Utf8, bom: false, auto: false, hex: true, truncated, stamp: None, nbt: None, region: false }
}

fn load_hex(path: &Path) -> Result<Loaded, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(hex::MAX_BYTES as u64 + 1).read_to_end(&mut buf))
        .map_err(|e| format!("{}\n{e}", path.display()))?;
    Ok(hex_loaded(&buf))
}

/// 読み込む。`forced` が None なら自動判定（バイナリなら HEX で開く）。
/// テキストで不正なバイト列は置換文字にせずエラーにする。
pub fn load(path: &Path, forced: Option<Mode>) -> Result<Loaded, String> {
    let before = stamp(path); // 読む前に取る（読んでいる間に変わっても見つけられる）
    load_inner(path, forced).map(|l| Loaded { stamp: before, ..l })
}

fn load_inner(path: &Path, forced: Option<Mode>) -> Result<Loaded, String> {
    if forced == Some(Mode::Hex) {
        return load_hex(path);
    }
    let bytes = fs::read(path).map_err(|e| format!("{}\n{e}", path.display()))?;
    if forced.is_none() {
        // NBT・リージョンとして読めなければ、ふつうのファイルとして開く（文字か HEX）
        if region::is_region_path(path) {
            if let Ok(entries) = region::list(&bytes) {
                let text = region::listing(path, &entries, |t| crate::platform::local_datetime(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t as u64)));
                return Ok(Loaded { text, encoding: Encoding::Utf8, bom: false, auto: true, hex: false, truncated: false, stamp: None, nbt: None, region: true });
            }
        } else if nbt::is_nbt_path(path) {
            if let Ok((file, text)) = nbt::open(&bytes) {
                return Ok(Loaded { text, encoding: Encoding::Utf8, bom: false, auto: true, hex: false, truncated: false, stamp: None, nbt: Some(file), region: false });
            }
        }
    }
    let (encoding, bom, auto) = match forced {
        Some(Mode::Text(e)) => (e, encoding::bom_len(e, &bytes) > 0, false),
        _ => match encoding::detect(&bytes) {
            Detected::Text { encoding, bom } => (encoding, bom, true),
            Detected::Binary => return Ok(Loaded { auto: true, ..hex_loaded(&bytes) }),
        },
    };
    let skip = if bom { encoding::bom_len(encoding, &bytes) } else { 0 };
    let body = &bytes[skip..];
    match encoding.decode(body) {
        Ok(text) => Ok(Loaded { text, encoding, bom, auto, hex: false, truncated: false, stamp: None, nbt: None, region: false }),
        Err(off) => Err(if crate::lang::is_en() { format!(
            "{}\nCannot read as {} (invalid at byte {}).\nPlease choose a different character encoding.",
            path.display(),
            encoding.label(),
            off + skip
        ) } else { format!(
            "{}\n{} として読めません（{} バイト目で不正）。\n別の文字コードを選んでください。",
            path.display(),
            encoding.label(),
            off + skip
        ) }),
    }
}

/// 保存用に変換する（BOM 付きで読んだ・選んだ文字コードなら BOM も付ける）。
pub fn encode_for_save(text: &str, enc: Encoding, bom: bool) -> encoding::Encoded {
    let mut e = enc.encode(text);
    if bom && !enc.bom_bytes().is_empty() {
        e.bytes = encoding::with_bom(enc, e.bytes);
    }
    e
}

/// 一時ファイルに書いてからリネームする。書込中に失敗しても元ファイルを壊さない。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".neonote.tmp");
    let tmp = path.with_file_name(tmp_name);

    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// 相対パスなら現在のフォルダを基準に絶対化する（正規化はしない）。
pub fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// 「開く」ダイアログの初期フォルダ: ~/Documents があればそこ、無ければホーム。
pub fn default_open_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = PathBuf::from(home);
    let docs = home.join("Documents");
    Some(if docs.is_dir() { docs } else { home })
}

/// 同じファイルを指しているか（開いているタブの重複検出用）。実体が引けなければ、パスの一致だけを見る。
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// タイトルの「Chars」に出す文字数（Unicode スカラー値の数）を返す。
/// 空白で区切った語の数（旧 Java 版の Word/Character Count と同じ数え方）。
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_splits_on_any_whitespace() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("  a b\t\nc  "), 3);
        assert_eq!(word_count("日本語 です\r\nok"), 3);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neonote-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn same_file_sees_through_dot_segments_and_rejects_others() {
        let d = temp_dir("same");
        let a = d.join("a.txt");
        let b = d.join("b.txt");
        fs::write(&a, "x").unwrap();
        fs::write(&b, "x").unwrap();
        assert!(same_file(&a, &a));
        assert!(same_file(&a, &d.join(".").join("a.txt")));
        assert!(!same_file(&a, &b));
        assert!(!same_file(&d.join("none1"), &d.join("none2")));
    }

    #[test]
    fn atomic_write_roundtrip_and_overwrite() {
        let d = temp_dir("write");
        let p = d.join("a.txt");
        write_atomic(&p, "こんにちは".as_bytes()).unwrap();
        assert_eq!(load(&p, None).unwrap().text, "こんにちは");
        write_atomic(&p, b"x").unwrap();
        assert_eq!(load(&p, None).unwrap().text, "x");
        assert!(!d.join("a.txt.neonote.tmp").exists());
    }

    #[test]
    fn auto_detects_sjis_and_forced_utf8_is_an_error() {
        let d = temp_dir("sjis");
        let p = d.join("sjis.txt");
        fs::write(&p, [0x83, 0x65, 0x83, 0x58, 0x83, 0x67]).unwrap();
        let l = load(&p, None).unwrap();
        assert_eq!((l.text.as_str(), l.encoding, l.auto), ("テスト", Encoding::Windows31J, true));
        let err = load(&p, Some(Mode::Text(Encoding::Utf8))).unwrap_err();
        assert!(err.contains("UTF-8") && err.contains("0 バイト目"));
        assert_eq!(load(&p, Some(Mode::Text(Encoding::Iso8859_1))).unwrap().text.chars().count(), 6);
    }

    #[test]
    fn bom_is_stripped_and_restored() {
        let d = temp_dir("bom");
        let p = d.join("bom.txt");
        fs::write(&p, b"\xEF\xBB\xBFabc").unwrap();
        let l = load(&p, None).unwrap();
        assert_eq!((l.text.as_str(), l.bom), ("abc", true));
        assert_eq!(encode_for_save(&l.text, l.encoding, l.bom).bytes, b"\xEF\xBB\xBFabc");
        assert_eq!(encode_for_save("abc", Encoding::Utf8, false).bytes, b"abc");
    }

    #[test]
    fn utf16_bom_is_stripped_restored_and_forced_modes_work() {
        let d = temp_dir("utf16");
        let p = d.join("u16.txt");
        let raw: Vec<u8> = [&[0xFF, 0xFE][..], &"日本a\r\nb".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>()[..]].concat();
        fs::write(&p, &raw).unwrap();
        let l = load(&p, None).unwrap();
        assert_eq!((l.text.as_str(), l.encoding, l.bom, l.auto), ("日本a\r\nb", Encoding::Utf16Le, true, true));
        assert_eq!(encode_for_save(&l.text, l.encoding, l.bom).bytes, raw);
        // 文字コードを指定して開いても BOM は外れ、付け直される。BE で読めば、本文は別の文字になる
        let forced = load(&p, Some(Mode::Text(Encoding::Utf16Le))).unwrap();
        assert_eq!((forced.text.as_str(), forced.bom, forced.auto), ("日本a\r\nb", true, false));
        // 本文が UTF-16 として読めないとき（奇数バイト）は、BOM の長さを足した位置で報告する
        fs::write(&p, b"\xFF\xFEa\x00b").unwrap();
        let err = load(&p, Some(Mode::Text(Encoding::Utf16Le))).unwrap_err();
        assert!(err.contains("UTF-16LE"), "{err}");
        // EUC-JP は指定して開ける（自動判定は Windows-31J を優先する）
        fs::write(&p, [0xC6, 0xFC, 0xCB, 0xDC]).unwrap();
        assert_eq!(load(&p, Some(Mode::Text(Encoding::EucJp))).unwrap().text, "日本");
    }

    #[test]
    fn binary_opens_as_hex_and_hex_can_be_forced() {
        let d = temp_dir("bin");
        let p = d.join("x.bin");
        fs::write(&p, [0x4D, 0x5A, 0x00, 0x01]).unwrap();
        let l = load(&p, None).unwrap();
        assert!(l.hex && !l.truncated);
        assert_eq!(hex::parse(&l.text).unwrap(), vec![0x4D, 0x5A, 0x00, 0x01]);
        let t = d.join("t.txt");
        fs::write(&t, "hi").unwrap();
        assert!(load(&t, Some(Mode::Hex)).unwrap().hex);
        assert!(!load(&t, None).unwrap().hex);
    }

    #[test]
    fn external_change_is_detected_but_own_state_is_not() {
        let d = temp_dir("stamp");
        let p = d.join("a.txt");
        fs::write(&p, "abc").unwrap();
        let l = load(&p, None).unwrap();
        assert!(l.stamp.is_some());
        assert!(!changed_on_disk(&p, l.stamp)); // 触っていない
        fs::write(&p, "abcd").unwrap(); // 大きさが変わる
        assert!(changed_on_disk(&p, l.stamp));
        assert!(!changed_on_disk(&p, stamp(&p))); // 保存直後に取り直せば一致
    }

    #[test]
    fn same_length_rewrite_is_detected_by_mtime() {
        let d = temp_dir("stamp2");
        let p = d.join("a.txt");
        fs::write(&p, "abc").unwrap();
        let before = stamp(&p);
        let f = fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        assert!(changed_on_disk(&p, before));
    }

    #[test]
    fn deleted_or_unknown_is_not_a_conflict() {
        let d = temp_dir("stamp3");
        let p = d.join("a.txt");
        fs::write(&p, "x").unwrap();
        let s = stamp(&p);
        assert!(!changed_on_disk(&p, None));
        fs::remove_file(&p).unwrap();
        assert!(!changed_on_disk(&p, s));
    }

    #[test]
    fn hex_and_failed_loads_carry_stamp_only_when_ok() {
        let d = temp_dir("stamp4");
        let p = d.join("x.bin");
        fs::write(&p, [0u8, 1, 2]).unwrap();
        assert!(load(&p, None).unwrap().stamp.is_some());
        assert!(load(Path::new("/no/such"), None).is_err());
    }

    #[test]
    fn missing_file_reports_path() {
        let e = load(Path::new("/no/such/file.txt"), None).unwrap_err();
        assert!(e.contains("/no/such/file.txt"));
    }

    #[test]
    fn counts_chars_not_bytes() {
        assert_eq!(char_count("aあ😀"), 3);
    }
}
