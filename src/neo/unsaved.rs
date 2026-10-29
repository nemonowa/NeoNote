// 未保存の変更の控え（GUI なし）。終了したときと、ときどき（app.rs の UNSAVED_SECS ごと）に、
// 未保存のタブの本文を設定フォルダの `unsaved/` へ書き、次に引数なしで起動したときに戻す（メモ帳の「続きから」）。
//
// 形式: `unsaved/index.txt` に 1 行 1 タブで `番号<TAB>元のファイルのパス`（名前の無いタブは空）、
//       本文は `unsaved/<番号>.txt`（UTF-8）。書くたびに全部を書き直し、使わなくなった本文のファイルは消す。
// 読めない行・本文の無い行は飛ばす（控えが壊れていても、起動は止めない）。
use std::{
    io,
    path::{Path, PathBuf},
};

pub const DIR_NAME: &str = "unsaved";
const INDEX: &str = "index.txt";
/// 控える本文の上限（バイト）。これより大きいタブは控えない。
pub const MAX_TEXT: usize = 20 * 1024 * 1024;
/// 控えるタブの数の上限。
pub const MAX_ENTRIES: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 元のファイル（名前の無いタブは None）。
    pub path: Option<PathBuf>,
    pub text: String,
}

/// 控えを書き直す。`entries` が空なら、控えを消す。
pub fn save(config_dir: &Path, entries: &[Entry]) -> io::Result<()> {
    let dir = config_dir.join(DIR_NAME);
    if entries.is_empty() {
        return clear(config_dir);
    }
    std::fs::create_dir_all(&dir)?;
    let mut index = String::new();
    let mut keep = vec![INDEX.to_string()];
    for (i, e) in entries.iter().take(MAX_ENTRIES).enumerate() {
        let path = match &e.path {
            Some(p) => match p.to_str() {
                Some(s) if !s.contains(['\t', '\n', '\r']) => s.to_string(),
                _ => continue, // 1 行に書けないパス
            },
            None => String::new(),
        };
        let name = format!("{i}.txt");
        write_atomic(&dir.join(&name), e.text.as_bytes())?;
        keep.push(name);
        index.push_str(&format!("{i}\t{path}\n"));
    }
    write_atomic(&dir.join(INDEX), index.as_bytes())?;
    // 前に書いた分の、使わなくなった本文を消す
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if !keep.contains(&n) && (n.ends_with(".txt") || n.ends_with(".tmp")) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    Ok(())
}

/// 控えを読む。無い・読めないときは空。
pub fn load(config_dir: &Path) -> Vec<Entry> {
    let dir = config_dir.join(DIR_NAME);
    let Ok(index) = std::fs::read_to_string(dir.join(INDEX)) else { return Vec::new() };
    let mut out = Vec::new();
    for line in index.lines().take(MAX_ENTRIES) {
        let Some((n, path)) = line.split_once('\t') else { continue };
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(dir.join(format!("{n}.txt"))) else { continue };
        if bytes.len() > MAX_TEXT {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        out.push(Entry { path: (!path.is_empty()).then(|| PathBuf::from(path)), text });
    }
    out
}

/// 控えを消す（無ければ何もしない）。
pub fn clear(config_dir: &Path) -> io::Result<()> {
    let dir = config_dir.join(DIR_NAME);
    match std::fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neonote-unsaved-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_and_shrink() {
        let d = temp("rt");
        let a = Entry { path: Some(PathBuf::from("/a/b.txt")), text: "あいう\n".into() };
        let b = Entry { path: None, text: "memo".into() };
        save(&d, &[a.clone(), b.clone()]).unwrap();
        assert_eq!(load(&d), vec![a.clone(), b]);
        // 減ったら、使わなくなった本文は消える
        save(&d, std::slice::from_ref(&a)).unwrap();
        assert_eq!(load(&d), vec![a]);
        assert!(!d.join(DIR_NAME).join("1.txt").exists());
        // 空なら控えごと消える
        save(&d, &[]).unwrap();
        assert!(!d.join(DIR_NAME).exists());
        assert!(load(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_index_is_skipped() {
        let d = temp("broken");
        let u = d.join(DIR_NAME);
        std::fs::create_dir_all(&u).unwrap();
        std::fs::write(u.join(INDEX), "x\t/a\nnope\n0\t\n1\t/missing\n").unwrap();
        std::fs::write(u.join("0.txt"), "ok").unwrap();
        assert_eq!(load(&d), vec![Entry { path: None, text: "ok".into() }]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
