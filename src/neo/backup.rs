// 上書き保存の前の控え（GUI なし）。保存で既存のファイルを置き換える直前に、元のファイルを 1 世代だけ
// `設定フォルダ/backup/<名前>~<パスの印>` へ写す（その起動で最初に上書きするときだけ）。元のフォルダ（データパックやワールド）には何も足さない。
// 設定ファイルの `backup=false` で切れる。大きすぎるファイル（`MAX_BYTES` 超）は控えない。
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
const DIR: &str = "backup";

static DEST: Mutex<Option<PathBuf>> = Mutex::new(None);
/// この起動で控えたファイル。控えるのは、そのファイルを最初に上書きするときだけ（自動保存で何度書いても、
/// NeoNote が触る前の中身が残る）。
static DONE: Mutex<Option<std::collections::HashSet<PathBuf>>> = Mutex::new(None);

/// 起動時に呼ぶ。`dir` は設定フォルダ。`enabled` が false か `dir` が無ければ控えない。
pub fn configure(dir: Option<&Path>, enabled: bool) {
    *DEST.lock().unwrap_or_else(|e| e.into_inner()) = dir.filter(|_| enabled).map(|d| d.join(DIR));
}

/// `target` の控えの置き場所（同じ名前の別のフォルダのファイルとぶつからないよう、フルパスの印を付ける）。
pub fn path_for(backup_dir: &Path, target: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    target.to_string_lossy().to_lowercase().hash(&mut h);
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    backup_dir.join(format!("{name}~{:08x}", h.finish() as u32))
}

/// 上書きの直前に呼ぶ。控えに失敗しても保存は止めない（控えは安心のためで、保存の条件ではない）。控えた場所を返す。
pub fn before_overwrite(target: &Path) -> Option<PathBuf> {
    let dir = DEST.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let key = target.to_path_buf();
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    if !done.get_or_insert_with(Default::default).insert(key) {
        return None;
    }
    drop(done);
    copy_to(&dir, target)
}

fn copy_to(dir: &Path, target: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(target).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    let dest = path_for(dir, target);
    std::fs::copy(target, &dest).ok()?;
    Some(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_one_generation_per_path() {
        let root = std::env::temp_dir().join(format!("neonote_bak_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (a, b) = (root.join("x/level.dat"), root.join("y/level.dat"));
        for p in [&a, &b] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        std::fs::write(&a, "a1").unwrap();
        std::fs::write(&b, "b1").unwrap();
        let bak = root.join("bak");
        let pa = copy_to(&bak, &a).unwrap();
        let pb = copy_to(&bak, &b).unwrap();
        assert_ne!(pa, pb); // 同じ名前でも別の控え
        std::fs::write(&a, "a2").unwrap();
        copy_to(&bak, &a).unwrap();
        assert_eq!(std::fs::read_to_string(&pa).unwrap(), "a2"); // 1 世代だけ（上書き）
        assert!(copy_to(&bak, &root.join("missing")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
