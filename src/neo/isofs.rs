// CD・DVD のイメージ（.iso）の中身の一覧と、ファイルの取り出し。UDF を持つイメージは udffs.rs で読み、ここは ISO 9660（GUI なし・読み取り専用・依存の追加なし）。
//
// 読むもの:
//   - ISO 9660 の基本の名前（`NAME.EXT;1`）、Joliet（日本語・長い名前。あればこちらを使う）、
//     Rock Ridge の NM（Linux の ISO の長い名前。Joliet が無いとき）。
//   - 4 GB を超えるファイル（複数のエクステントに分かれた物）・isohybrid（先頭に MBR のある ISO）。
// 読まないもの:
//   - インターリーブ（ファイルを飛び飛びに置く古い形式）・拡張属性・セッションが複数ある CD の 2 つ目以降。
// イメージは丸ごとメモリに載せず、必要な所だけ `Read + Seek` で読む。
use crate::neo::archive::{Entry, MAX_ENTRIES};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const SECTOR: u64 = 2048;
/// ディレクトリ 1 つを読む上限。
const MAX_DIR: u64 = 16 << 20;
/// ディレクトリの入れ子の上限（壊れたイメージで深く潜り続けないため）。
const MAX_DEPTH: usize = 64;

fn err<T>(ja: &'static str, en: &'static str) -> Result<T, String> {
    Err(crate::lang::tr(ja, en).to_string())
}

fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// ディレクトリの 1 項目。
#[derive(Clone, Debug)]
struct Rec {
    name: String,
    is_dir: bool,
    /// (先頭のセクター, 長さ) の並び。4 GB を超えるファイルは複数。
    extents: Vec<(u32, u32)>,
}

impl Rec {
    fn size(&self) -> u64 {
        self.extents.iter().map(|&(_, l)| l as u64).sum()
    }
}

struct Iso<R> {
    r: R,
    /// 使うボリューム記述子のルートのディレクトリ（セクター, 長さ）。
    root: (u32, u32),
    joliet: bool,
    /// 基本のボリューム記述子で Rock Ridge が使われているか（ルートの「.」に SP がある）。
    rock_ridge: bool,
    /// イメージの大きさ（セクターがこれを超える項目は壊れているとみなす）。
    len: u64,
}

impl<R: Read + Seek> Iso<R> {
    fn open(mut r: R) -> Result<Self, String> {
        let len = r.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        let mut pvd: Option<(u32, u32)> = None;
        let mut jol: Option<(u32, u32)> = None;
        let mut buf = [0u8; SECTOR as usize];
        // ボリューム記述子はセクター 16 から。終わりの印（255）まで、多くても 64 個
        for i in 16..80u64 {
            if read_at(&mut r, i * SECTOR, &mut buf).is_err() || &buf[1..6] != b"CD001" {
                break;
            }
            let root = (u32le(&buf, 156 + 2), u32le(&buf, 156 + 10));
            match buf[0] {
                1 if pvd.is_none() => pvd = Some(root),
                // Joliet は補助の記述子で、エスケープ列が %/@ %/C %/E
                2 if jol.is_none() && buf[88] == b'%' && buf[89] == b'/' && matches!(buf[90], b'@' | b'C' | b'E') => jol = Some(root),
                255 => break,
                _ => {}
            }
        }
        let Some(pvd) = pvd else { return err("ISO 9660 のイメージではありません", "Not an ISO 9660 image") };
        let mut iso = Iso { r, root: jol.unwrap_or(pvd), joliet: jol.is_some(), rock_ridge: false, len };
        if !iso.joliet {
            iso.rock_ridge = iso.detect_rock_ridge();
        }
        Ok(iso)
    }

    /// ルートの「.」の項目の後ろ（システム使用領域）に SUSP の SP があるか。
    fn detect_rock_ridge(&mut self) -> bool {
        let mut b = [0u8; SECTOR as usize];
        if read_at(&mut self.r, self.root.0 as u64 * SECTOR, &mut b).is_err() {
            return false;
        }
        let (len, nlen) = (b[0] as usize, b[32] as usize);
        let su = 33 + nlen + (nlen + 1) % 2;
        len > su + 4 && &b[su..su + 2] == b"SP"
    }

    fn check(&self, sector: u32, len: u32) -> Result<(), String> {
        let end = sector as u64 * SECTOR + len as u64;
        if end > self.len {
            return err("ISO の中の位置がイメージの外を指しています（壊れたイメージ）", "An entry points outside the image (broken image)");
        }
        Ok(())
    }

    /// ディレクトリ（セクター, 長さ）の項目。「.」「..」は除く。
    fn read_dir(&mut self, (sector, len): (u32, u32)) -> Result<Vec<Rec>, String> {
        if len as u64 > MAX_DIR {
            return err("ディレクトリが大きすぎます", "Directory too large");
        }
        self.check(sector, len)?;
        let mut data = vec![0u8; len as usize];
        read_at(&mut self.r, sector as u64 * SECTOR, &mut data).map_err(|e| e.to_string())?;
        let mut out: Vec<Rec> = Vec::new();
        let mut i = 0usize;
        // 前の項目が「続きがある」（複数のエクステント）なら、次の同じ名前の項目をつなげる
        let mut continuing = false;
        while i < data.len() {
            let rl = data[i] as usize;
            if rl == 0 {
                // 項目はセクターをまたがない。残りは 0 で埋まっているので、次のセクターへ
                i = (i / SECTOR as usize + 1) * SECTOR as usize;
                continue;
            }
            if rl < 34 || i + rl > data.len() {
                break;
            }
            let r = &data[i..i + rl];
            i += rl;
            let nlen = r[32] as usize;
            if 33 + nlen > rl {
                continue;
            }
            let raw = &r[33..33 + nlen];
            if nlen == 1 && (raw[0] == 0 || raw[0] == 1) {
                continue; // 「.」と「..」
            }
            let flags = r[25];
            let extent = (u32le(r, 2), u32le(r, 10));
            if continuing {
                if let Some(last) = out.last_mut() {
                    last.extents.push(extent);
                }
                continuing = flags & 0x80 != 0;
                continue;
            }
            continuing = flags & 0x80 != 0;
            let name = if self.joliet {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                strip_version(&String::from_utf16_lossy(&units))
            } else {
                let su = 33 + nlen + (nlen + 1) % 2;
                match (self.rock_ridge && su < rl).then(|| rr_name(&r[su..])).flatten() {
                    Some(n) => n,
                    None => strip_version(&String::from_utf8_lossy(raw)),
                }
            };
            if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                continue;
            }
            out.push(Rec { name, is_dir: flags & 0x02 != 0, extents: vec![extent] });
        }
        Ok(out)
    }

    /// 一覧を作る。
    fn list(&mut self, out: &mut Vec<Entry>) -> Result<(), String> {
        let mut seen = HashSet::new();
        let root = self.root;
        self.walk(root, "", 0, &mut seen, out, &mut |_, _| {})
    }

    fn walk(
        &mut self,
        dir: (u32, u32),
        prefix: &str,
        depth: usize,
        seen: &mut HashSet<u32>,
        out: &mut Vec<Entry>,
        visit: &mut dyn FnMut(&str, &Rec),
    ) -> Result<(), String> {
        if depth > MAX_DEPTH || !seen.insert(dir.0) {
            return Ok(()); // 深すぎる・同じディレクトリへ戻る（壊れたイメージ）
        }
        for rec in self.read_dir(dir)? {
            if out.len() >= MAX_ENTRIES {
                return Ok(());
            }
            let path = format!("{prefix}{}{}", rec.name, if rec.is_dir { "/" } else { "" });
            out.push(Entry { path: path.clone(), is_dir: rec.is_dir, size: if rec.is_dir { 0 } else { rec.size() }, packed: None });
            if rec.is_dir {
                // 読めないディレクトリは飛ばす（ほかの部分は見せる）
                let _ = self.walk(rec.extents[0], &path, depth + 1, seen, out, visit);
            } else {
                visit(&path, &rec);
            }
        }
        Ok(())
    }

    /// `path`（`a/b.txt`）の項目を探す。
    fn lookup(&mut self, path: &str) -> Result<Option<Rec>, String> {
        let mut dir = self.root;
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for (k, part) in parts.iter().enumerate() {
            let Some(rec) = self.read_dir(dir)?.into_iter().find(|r| r.name == *part) else { return Ok(None) };
            if k + 1 == parts.len() {
                return Ok(Some(rec));
            }
            if !rec.is_dir {
                return Ok(None);
            }
            dir = rec.extents[0];
        }
        Ok(None)
    }

    fn read_file(&mut self, rec: &Rec, max: u64) -> Result<Vec<u8>, String> {
        if rec.size() > max {
            return err("ファイルが大きすぎます", "File too large");
        }
        let mut out = Vec::with_capacity(rec.size() as usize);
        for &(sector, len) in &rec.extents {
            self.check(sector, len)?;
            let start = out.len();
            out.resize(start + len as usize, 0);
            read_at(&mut self.r, sector as u64 * SECTOR, &mut out[start..]).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

fn read_at<R: Read + Seek>(r: &mut R, pos: u64, buf: &mut [u8]) -> std::io::Result<()> {
    r.seek(SeekFrom::Start(pos))?;
    r.read_exact(buf)
}

/// `NAME.EXT;1` → `NAME.EXT`、拡張子の無い `NAME.;1` → `NAME`。
fn strip_version(s: &str) -> String {
    let s = s.split(';').next().unwrap_or(s);
    s.strip_suffix('.').unwrap_or(s).to_string()
}

/// Rock Ridge の NM（名前。続きの印で複数に分かれることがある）。無ければ None。
fn rr_name(su: &[u8]) -> Option<String> {
    let mut i = 0;
    let mut name = Vec::new();
    let mut found = false;
    while i + 4 <= su.len() {
        let (sig, len) = (&su[i..i + 2], su[i + 2] as usize);
        if len < 4 || i + len > su.len() {
            break;
        }
        if sig == b"NM" && len >= 5 {
            let flags = su[i + 4];
            if flags & 0x06 == 0 {
                // 0x02（.）と 0x04（..）は名前を持たない
                name.extend_from_slice(&su[i + 5..i + len]);
                found = true;
            }
        } else if sig == b"ST" {
            break;
        }
        i += len;
    }
    found.then(|| String::from_utf8_lossy(&name).into_owned()).filter(|n| !n.is_empty())
}

// ------------------------------------------------------------------ ファイルとして開く

fn has_iso_name(p: &Path) -> bool {
    p.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "iso" | "img"))
}

/// CD・DVD のイメージとして一覧で開くか（拡張子が iso か img で、セクター 16 に CD001 か、UDF の印がある）。
pub fn is_iso_path(p: &Path) -> bool {
    if !has_iso_name(p) || p.is_dir() {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    let mut b = [0u8; 6];
    (read_at(&mut f, 16 * SECTOR, &mut b).is_ok() && &b[1..6] == b"CD001") || super::udffs::has_udf_mark(&mut f)
}

/// UDF を持つか（あれば UDF で読む。ISO 9660 の側は案内だけのことが多いため）。
fn is_udf(p: &Path) -> bool {
    std::fs::File::open(p).is_ok_and(|mut f| super::udffs::has_udf_mark(&mut f))
}

fn file(p: &Path) -> Result<std::fs::File, String> {
    std::fs::File::open(p).map_err(|e| format!("{}
{e}", p.display()))
}

fn has_9660(p: &Path) -> bool {
    let mut b = [0u8; 6];
    std::fs::File::open(p).is_ok_and(|mut f| read_at(&mut f, 16 * SECTOR, &mut b).is_ok() && &b[1..6] == b"CD001")
}

fn open_file(p: &Path) -> Result<Iso<std::fs::File>, String> {
    let f = std::fs::File::open(p).map_err(|e| format!("{}\n{e}", p.display()))?;
    Iso::open(f)
}

/// イメージの全てのファイルを一覧にする。
pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    if is_udf(p) {
        // UDF が読めなければ ISO 9660 の側で見せる（どちらも無理ならその理由）
        match super::udffs::list(file(p)?) {
            Ok(v) => return Ok(v),
            Err(e) if !has_9660(p) => return Err(e),
            Err(_) => {}
        }
    }
    let mut out = Vec::new();
    open_file(p)?.list(&mut out)?;
    out.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// イメージの中の、大きさが `max_each` 以下のファイルを 1 つずつ読んで `f(パス, 中身)` に渡す（`f` が false を返したら止める）。
pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    if is_udf(p) && super::udffs::each_file(file(p)?, max_each, &mut f).is_ok() {
        return Ok(());
    }
    let mut iso = open_file(p)?;
    let mut files: Vec<(String, Rec)> = Vec::new();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let root = iso.root;
    iso.walk(root, "", 0, &mut seen, &mut out, &mut |path, rec| files.push((path.to_string(), rec.clone())))?;
    for (path, rec) in files.into_iter().filter(|(_, r)| r.size() <= max_each) {
        if let Ok(data) = iso.read_file(&rec, max_each) {
            if !f(&path, data) {
                break;
            }
        }
    }
    Ok(())
}

/// イメージの中のファイル 1 つの中身（`path` は `list_file` が返したパス）。
pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    if is_udf(p) {
        match super::udffs::read_path(file(p)?, path, max) {
            Ok(v) => return Ok(v),
            Err(e) if !has_9660(p) => return Err(e),
            Err(_) => {}
        }
    }
    let mut iso = open_file(p)?;
    match iso.lookup(path)? {
        Some(rec) if !rec.is_dir => iso.read_file(&rec, max),
        _ => err("見つかりません", "Not found"),
    }
}

/// イメージの中のファイルを一時フォルダへ取り出して、そのパスを返す（開くため。ext と同じ一時フォルダ）。
pub fn extract_to_temp(image: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    super::extfs::write_temp(image, path, &read_path(image, path, super::extfs::EXTRACT_MAX)?)
}

/// テスト用: バイト列の ISO の一覧（壊れた入力で落ちないことの確かめにも使う）。
#[cfg(test)]
pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    Iso::open(std::io::Cursor::new(b))?.list(&mut out)?;
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// ディレクトリの項目を 1 つ作る。
    fn rec(name: &[u8], sector: u32, len: u32, flags: u8, su: &[u8]) -> Vec<u8> {
        let pad = (name.len() + 1) % 2;
        let rl = 33 + name.len() + pad + su.len();
        let mut r = vec![0u8; rl];
        r[0] = rl as u8;
        r[2..6].copy_from_slice(&sector.to_le_bytes());
        r[6..10].copy_from_slice(&sector.to_be_bytes());
        r[10..14].copy_from_slice(&len.to_le_bytes());
        r[14..18].copy_from_slice(&len.to_be_bytes());
        r[25] = flags;
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r[33 + name.len() + pad..].copy_from_slice(su);
        r
    }

    fn put(img: &mut Vec<u8>, sector: u64, data: &[u8]) {
        let at = (sector * SECTOR) as usize;
        if img.len() < at + data.len() {
            img.resize(at + data.len(), 0);
        }
        img[at..at + data.len()].copy_from_slice(data);
    }

    fn vd(kind: u8, root: (u32, u32), joliet: bool) -> Vec<u8> {
        let mut b = vec![0u8; SECTOR as usize];
        b[0] = kind;
        b[1..6].copy_from_slice(b"CD001");
        b[6] = 1;
        if joliet {
            b[88..91].copy_from_slice(b"%/E");
        }
        let r = rec(&[0], root.0, root.1, 2, &[]);
        b[156..156 + r.len()].copy_from_slice(&r);
        b
    }

    fn ucs2(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    /// 基本の名前の木（セクター 20〜）と Joliet の木（セクター 30〜）を持つ小さな ISO。
    pub fn sample(with_joliet: bool) -> Vec<u8> {
        let mut img = vec![0u8; 16 * SECTOR as usize];
        put(&mut img, 16, &vd(1, (20, SECTOR as u32), false));
        let mut next = 17;
        if with_joliet {
            put(&mut img, 17, &vd(2, (30, SECTOR as u32), true));
            next = 18;
        }
        let mut term = vec![0u8; SECTOR as usize];
        term[0] = 255;
        term[1..6].copy_from_slice(b"CD001");
        put(&mut img, next, &term);
        // 基本の木: README.TXT;1、DOCS/（中に A.TXT;1）
        let mut root = Vec::new();
        root.extend(rec(&[0], 20, SECTOR as u32, 2, &[]));
        root.extend(rec(&[1], 20, SECTOR as u32, 2, &[]));
        root.extend(rec(b"README.TXT;1", 40, 5, 0, &[]));
        root.extend(rec(b"DOCS", 21, SECTOR as u32, 2, &[]));
        put(&mut img, 20, &root);
        let mut docs = Vec::new();
        docs.extend(rec(&[0], 21, SECTOR as u32, 2, &[]));
        docs.extend(rec(&[1], 20, SECTOR as u32, 2, &[]));
        docs.extend(rec(b"A.TXT;1", 41, 3, 0, &[]));
        put(&mut img, 21, &docs);
        if with_joliet {
            let mut jr = Vec::new();
            jr.extend(rec(&[0], 30, SECTOR as u32, 2, &[]));
            jr.extend(rec(&[1], 30, SECTOR as u32, 2, &[]));
            jr.extend(rec(&ucs2("はじめに.txt;1"), 40, 5, 0, &[]));
            jr.extend(rec(&ucs2("資料"), 31, SECTOR as u32, 2, &[]));
            put(&mut img, 30, &jr);
            let mut jd = Vec::new();
            jd.extend(rec(&[0], 31, SECTOR as u32, 2, &[]));
            jd.extend(rec(&[1], 30, SECTOR as u32, 2, &[]));
            jd.extend(rec(&ucs2("長い名前のファイル.txt;1"), 41, 3, 0, &[]));
            put(&mut img, 31, &jd);
        }
        put(&mut img, 40, b"hello");
        put(&mut img, 41, b"abc");
        img.resize(42 * SECTOR as usize, 0);
        img
    }

    fn names(es: &[Entry]) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = es.iter().map(|e| (e.path.clone(), e.size)).collect();
        v.sort();
        v
    }

    #[test]
    fn lists_plain_iso9660_names() {
        let es = list_bytes(&sample(false)).unwrap();
        assert_eq!(names(&es), vec![("DOCS/".into(), 0), ("DOCS/A.TXT".into(), 3), ("README.TXT".into(), 5)]);
    }

    #[test]
    fn prefers_joliet_and_reads_files() {
        let img = sample(true);
        let es = list_bytes(&img).unwrap();
        assert_eq!(names(&es), vec![("はじめに.txt".into(), 5), ("資料/".into(), 0), ("資料/長い名前のファイル.txt".into(), 3)]);
        let mut iso = Iso::open(std::io::Cursor::new(&img[..])).unwrap();
        let r = iso.lookup("資料/長い名前のファイル.txt").unwrap().unwrap();
        assert_eq!(iso.read_file(&r, 1 << 20).unwrap(), b"abc");
        assert!(iso.lookup("資料/無い.txt").unwrap().is_none());
    }

    #[test]
    fn rock_ridge_names_and_multi_extent_files() {
        let mut img = vec![0u8; 16 * SECTOR as usize];
        put(&mut img, 16, &vd(1, (20, SECTOR as u32), false));
        let mut term = vec![0u8; SECTOR as usize];
        term[0] = 255;
        term[1..6].copy_from_slice(b"CD001");
        put(&mut img, 17, &term);
        let sp = [b'S', b'P', 7, 1, 0xBE, 0xEF, 0];
        let nm = |n: &str| {
            let mut v = vec![b'N', b'M', (5 + n.len()) as u8, 1, 0];
            v.extend_from_slice(n.as_bytes());
            v
        };
        let mut root = Vec::new();
        root.extend(rec(&[0], 20, SECTOR as u32, 2, &sp));
        root.extend(rec(&[1], 20, SECTOR as u32, 2, &[]));
        root.extend(rec(b"LONGFI~1.TXT;1", 40, 2, 0, &nm("long file name.txt")));
        // 2 つのエクステントに分かれたファイル（1 つ目に続きの印 0x80）
        root.extend(rec(b"BIG.BIN;1", 41, 3, 0x80, &nm("big.bin")));
        root.extend(rec(b"BIG.BIN;1", 42, 2, 0, &nm("big.bin")));
        put(&mut img, 20, &root);
        put(&mut img, 40, b"hi");
        put(&mut img, 41, b"abc");
        put(&mut img, 42, b"de");
        img.resize(43 * SECTOR as usize, 0);
        let es = list_bytes(&img).unwrap();
        assert_eq!(names(&es), vec![("big.bin".into(), 5), ("long file name.txt".into(), 2)]);
        let mut iso = Iso::open(std::io::Cursor::new(&img[..])).unwrap();
        let r = iso.lookup("big.bin").unwrap().unwrap();
        assert_eq!(iso.read_file(&r, 1 << 20).unwrap(), b"abcde");
    }

    #[test]
    fn broken_images_give_errors_not_panics() {
        assert!(list_bytes(b"").is_err());
        assert!(list_bytes(&[0u8; 40000]).is_err());
        let img = sample(true);
        // 途中で切れたイメージ・外を指す位置
        for cut in [16 * 2048 + 10, 18 * 2048, 31 * 2048, 40 * 2048 + 1] {
            let _ = list_bytes(&img[..cut]);
        }
        let mut bad = img.clone();
        bad[30 * 2048 + 34 * 2 + 2..30 * 2048 + 34 * 2 + 6].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let _ = list_bytes(&bad);
        // 自分自身を指すディレクトリ（無限に潜らない）
        let mut lp = sample(false);
        let docs_at = 21 * 2048 + 34 * 2;
        let a = rec(b"LOOP", 21, SECTOR as u32, 2, &[]);
        lp[docs_at..docs_at + a.len()].copy_from_slice(&a);
        assert!(list_bytes(&lp).is_ok());
    }
}
