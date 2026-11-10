// 表示状態の記憶（GUI なし）。ファイルを閉じたときのカーソル位置を覚え、次に開いたときにそこへ戻す。
//
// 形式: 設定フォルダの `viewstate.txt`。1 行目は `NeoNote viewstate v1`、以降は 1 行 1 件で
//       `大きさ<TAB>更新時刻（UNIX 秒）<TAB>カーソル（バッファのバイト位置）<TAB>元のファイルのパス`。
//       新しく閉じた順に並べ、最大 `MAX_ENTRIES` 件で古いものから捨てる。
// 戻すのは、大きさと更新時刻が記録と一致するときだけ。外部で書き換わったファイルでは位置が意味を持たない。
// 読めない行・版の違うファイルは飛ばす（壊れていても、起動は止めない）。
use std::{
    io,
    path::{Path, PathBuf},
};

pub const FILE_NAME: &str = "viewstate.txt";
const HEADER: &str = "NeoNote viewstate v1";
/// 覚えるファイルの数の上限。
pub const MAX_ENTRIES: usize = 60;

/// ファイルの大きさと更新時刻（秒）。記録したときと今を比べる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub len: u64,
    pub mtime: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub fp: Fingerprint,
    pub cursor: i32,
}

/// `path` の今の大きさと更新時刻。読めなければ None。
pub fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(Fingerprint { len: m.len(), mtime })
}

/// `e` を先頭に入れる（同じパスの古い記録は取り除く）。上限を超えた分は古い方から捨てる。
pub fn remember(list: &mut Vec<Entry>, e: Entry) {
    list.retain(|x| x.path != e.path);
    list.insert(0, e);
    list.truncate(MAX_ENTRIES);
}

/// `path` の記録のうち、ファイルが今も同じ（`now` と一致）ときのカーソル位置。
pub fn lookup(list: &[Entry], path: &Path, now: Fingerprint) -> Option<i32> {
    list.iter().find(|e| e.path == path && e.fp == now).map(|e| e.cursor)
}

pub fn load(config_dir: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(config_dir.join(FILE_NAME)) else { return Vec::new() };
    parse(&text)
}

pub fn save(config_dir: &Path, list: &[Entry]) -> io::Result<()> {
    let file = config_dir.join(FILE_NAME);
    if list.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    crate::neo::document::write_atomic(&file, format(list).as_bytes())
}

/// ディスクにある記録（ほかのウィンドウが書いた分）に、このウィンドウの記録を重ねる。同じパスはこちらを優先し、
/// 並びは「このウィンドウの記録、続いてディスクだけにある記録」。上限を超えた分は古い方から捨てる。
/// 複数のウィンドウが、保存のたびに互いの記録を消さないようにする。
pub fn merged(disk: Vec<Entry>, mine: &[Entry]) -> Vec<Entry> {
    let mut out: Vec<Entry> = mine.to_vec();
    for e in disk {
        if !out.iter().any(|x| x.path == e.path) {
            out.push(e);
        }
    }
    out.truncate(MAX_ENTRIES);
    out
}

fn parse(text: &str) -> Vec<Entry> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in lines.take(MAX_ENTRIES) {
        let mut f = line.splitn(4, '\t');
        let (Some(len), Some(mtime), Some(cursor), Some(path)) = (f.next(), f.next(), f.next(), f.next()) else { continue };
        let (Ok(len), Ok(mtime), Ok(cursor)) = (len.parse::<u64>(), mtime.parse::<u64>(), cursor.parse::<i32>()) else { continue };
        if path.is_empty() || cursor < 0 {
            continue;
        }
        out.push(Entry { path: PathBuf::from(path), fp: Fingerprint { len, mtime }, cursor });
    }
    out
}

fn format(list: &[Entry]) -> String {
    let mut s = format!("{HEADER}\n");
    for e in list.iter().take(MAX_ENTRIES) {
        // 1 行に書けないパス（タブ・改行を含む）は記録しない
        let Some(p) = e.path.to_str().filter(|p| !p.contains(['\t', '\n', '\r'])) else { continue };
        s.push_str(&format!("{}\t{}\t{}\t{}\n", e.fp.len, e.fp.mtime, e.cursor, p));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(len: u64, mtime: u64) -> Fingerprint {
        Fingerprint { len, mtime }
    }

    fn entry(p: &str, cursor: i32) -> Entry {
        Entry { path: PathBuf::from(p), fp: fp(10, 20), cursor }
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neonote-viewstate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_keeps_order_and_japanese_paths() {
        let d = temp("rt");
        let list = vec![entry("/a/ノート.txt", 5), entry("C:\\my docs\\b.md", 0)];
        save(&d, &list).unwrap();
        assert_eq!(load(&d), list);
        save(&d, &[]).unwrap();
        assert!(!d.join(FILE_NAME).exists());
        assert!(load(&d).is_empty());
        save(&d, &[]).unwrap(); // 無いファイルを消そうとしても失敗しない
    }

    #[test]
    fn remember_moves_to_front_dedupes_and_caps() {
        let mut l = Vec::new();
        remember(&mut l, entry("a", 1));
        remember(&mut l, entry("b", 2));
        remember(&mut l, entry("a", 9));
        assert_eq!(l.iter().map(|e| (e.path.to_str().unwrap(), e.cursor)).collect::<Vec<_>>(), vec![("a", 9), ("b", 2)]);
        for i in 0..100 {
            remember(&mut l, entry(&format!("f{i}"), i));
        }
        assert_eq!(l.len(), MAX_ENTRIES);
        assert_eq!(l[0].path, PathBuf::from("f99")); // 新しい方が残る
        assert!(!l.iter().any(|e| e.path == *"a"));
    }

    #[test]
    fn lookup_needs_the_same_size_and_mtime() {
        let l = vec![entry("a", 7)];
        assert_eq!(lookup(&l, Path::new("a"), fp(10, 20)), Some(7));
        assert_eq!(lookup(&l, Path::new("a"), fp(11, 20)), None); // 大きさが違う
        assert_eq!(lookup(&l, Path::new("a"), fp(10, 21)), None); // 更新時刻が違う
        assert_eq!(lookup(&l, Path::new("b"), fp(10, 20)), None);
    }

    #[test]
    fn broken_lines_and_other_versions_are_skipped() {
        let text = format!("{HEADER}\n1\t2\t3\t/ok\nx\t2\t3\t/bad-len\n1\t2\t-5\t/negative\n1\t2\t3\n1\t2\t3\t\n\n9\t8\t7\t/with\ttab-in-path\n");
        let got = parse(&text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], Entry { path: PathBuf::from("/ok"), fp: fp(1, 2), cursor: 3 });
        assert_eq!(got[1].path, PathBuf::from("/with\ttab-in-path")); // 4 つ目以降は丸ごとパス
        assert!(parse("NeoNote viewstate v2\n1\t2\t3\t/ok\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn paths_that_cannot_sit_on_one_line_are_not_written() {
        let s = format(&[entry("a\nb", 1), entry("ok", 2)]);
        assert_eq!(s, format!("{HEADER}\n10\t20\t2\tok\n"));
    }

    #[test]
    fn fingerprint_follows_the_file() {
        let d = temp("fp");
        let f = d.join("x.txt");
        std::fs::write(&f, "abc").unwrap();
        let a = fingerprint(&f).unwrap();
        assert_eq!(a.len, 3);
        std::fs::write(&f, "abcd").unwrap();
        assert_ne!(fingerprint(&f).unwrap(), a);
        assert!(fingerprint(&d.join("none")).is_none());
    }

    #[test]
    fn merged_keeps_other_windows_records_and_prefers_mine() {
        let e = |p: &str, c: i32| Entry { path: PathBuf::from(p), fp: Fingerprint { len: 1, mtime: 2 }, cursor: c };
        let disk = vec![e("/b", 5), e("/a", 1), e("/c", 3)];
        let mine = vec![e("/a", 9), e("/d", 4)];
        let m = merged(disk, &mine);
        assert_eq!(m.iter().map(|x| (x.path.to_str().unwrap(), x.cursor)).collect::<Vec<_>>(), vec![("/a", 9), ("/d", 4), ("/b", 5), ("/c", 3)]);
        let many: Vec<Entry> = (0..MAX_ENTRIES + 10).map(|i| e(&format!("/f{i}"), 0)).collect();
        assert_eq!(merged(many, &[]).len(), MAX_ENTRIES);
    }
}
