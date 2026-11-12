// UDF の DVD・Blu-ray のイメージ（Windows のインストール用 ISO など）の中身の一覧と読み出し（GUI なし・読み取り専用・依存の追加なし）。
// ISO 9660 と UDF の両方を持つイメージは、UDF を使う（Windows の ISO は ISO 9660 の側に案内の文書しか無いため）。
//
// 読むもの: UDF 1.02〜2.60 の、普通の区画（タイプ 1）とメタデータ区画（タイプ 2。UDF 2.50 以降・Blu-ray）、
//   ファイル項目（FE）と拡張ファイル項目（EFE）、short_ad / long_ad / 中に埋め込まれたデータ、割り当ての続き（AED）、
//   名前の 8 ビット・16 ビットの形式。
// 読まないもの: 仮想区画（VAT。追記型の CD-R）・スペアリング区画（CD-RW）・ext_ad・ストリーム（名前付きの別データ）。
// イメージは丸ごとメモリに載せず、必要な所だけ `Read + Seek` で読む。
use crate::neo::archive::{Entry, MAX_ENTRIES};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

const SECTOR: u64 = 2048;
const MAX_DIR: u64 = 16 << 20;
const MAX_DEPTH: usize = 64;
/// 割り当ての続き（AED）をたどる回数の上限（壊れたイメージで輪にならないため）。
const MAX_AED: usize = 1024;

const TAG_PD: u16 = 5;
const TAG_LVD: u16 = 6;
const TAG_TD: u16 = 8;
const TAG_FSD: u16 = 256;
const TAG_FID: u16 = 257;
const TAG_AED: u16 = 258;
const TAG_FE: u16 = 261;
const TAG_EFE: u16 = 266;

fn err<T>(ja: &'static str, en: &'static str) -> Result<T, String> {
    Err(crate::lang::tr(ja, en).to_string())
}
fn broken<T>() -> Result<T, String> {
    err("UDF のイメージが壊れています", "Broken UDF image")
}

fn u16le(b: &[u8], i: usize) -> u16 {
    b.get(i..i + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32le(b: &[u8], i: usize) -> u32 {
    b.get(i..i + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u64le(b: &[u8], i: usize) -> u64 {
    b.get(i..i + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}

/// 記述子のタグ（先頭 16 バイト）の種類。タグの合計（チェックサム）が合わなければ None。
fn tag_id(b: &[u8]) -> Option<u16> {
    if b.len() < 16 {
        return None;
    }
    let sum = b[..16].iter().enumerate().filter(|&(i, _)| i != 4).fold(0u8, |s, (_, &v)| s.wrapping_add(v));
    (sum == b[4]).then(|| u16le(b, 0))
}

/// 区画の割り当て表の 1 つ。
#[derive(Clone, Debug)]
enum Part {
    /// 普通の区画（区画の先頭のセクター）。
    Physical { start: u64 },
    /// メタデータ区画: 下の普通の区画（番号）の上で、メタデータのファイルの中身として並ぶ。
    /// `extents` は (メタデータ区画の中の先頭のブロック, ブロック数, 下の区画の中のブロック)。
    Metadata { under: usize, extents: Vec<(u64, u64, u64)> },
}

/// 割り当ての 1 つ（区画, 区画の中のブロック, 長さ, 書かれているか）。
#[derive(Clone, Copy, Debug)]
struct Ad {
    part: u16,
    lbn: u32,
    len: u32,
    recorded: bool,
}

/// ファイル項目から読んだ中身の場所。
#[derive(Clone, Debug)]
struct Node {
    is_dir: bool,
    size: u64,
    ads: Vec<Ad>,
    /// 中に埋め込まれたデータ（小さなファイル・ディレクトリ）。
    embedded: Option<Vec<u8>>,
}

struct Udf<R> {
    r: R,
    bs: u64,
    parts: Vec<Part>,
    /// 区画の番号（記述子の PartitionNumber）→ 先頭のセクター。
    root: (u16, u32),
    len: u64,
}

fn read_at<R: Read + Seek>(r: &mut R, pos: u64, buf: &mut [u8]) -> Result<(), String> {
    r.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    r.read_exact(buf).map_err(|_| crate::lang::tr("UDF のイメージが途中で切れています", "UDF image is truncated").to_string())
}

/// セクター 16 からの見分けの並びに、UDF の印（NSR02 / NSR03）があるか。
pub fn has_udf_mark<R: Read + Seek>(r: &mut R) -> bool {
    let mut b = [0u8; 6];
    for i in 16..32u64 {
        if read_at(r, i * SECTOR, &mut b).is_err() {
            return false;
        }
        match &b[1..6] {
            b"NSR02" | b"NSR03" => return true,
            b"BEA01" | b"CD001" | b"CDW02" | b"BOOT2" => {}
            _ => return false,
        }
    }
    false
}

impl<R: Read + Seek> Udf<R> {
    fn open(mut r: R) -> Result<Self, String> {
        let len = r.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        if !has_udf_mark(&mut r) {
            return err("UDF のイメージではありません", "Not a UDF image");
        }
        // 錨（AVDP）はセクター 256、無ければ最後と、最後から 256 番目
        let mut avdp = [0u8; 512];
        let last = (len / SECTOR).saturating_sub(1);
        let found = [256, last, last.saturating_sub(256)].into_iter().any(|s| read_at(&mut r, s * SECTOR, &mut avdp).is_ok() && tag_id(&avdp) == Some(2));
        if !found {
            return err("UDF の錨（AVDP）が見つかりません", "UDF anchor (AVDP) not found");
        }
        let (vds_len, vds_loc) = (u32le(&avdp, 16) as u64, u32le(&avdp, 20) as u64);
        // ボリュームの記述子の並びから、区画（PD）と論理ボリューム（LVD）を読む
        let mut pds: Vec<(u16, u64)> = Vec::new();
        let mut lvd: Option<Vec<u8>> = None;
        let mut d = vec![0u8; SECTOR as usize];
        for k in 0..(vds_len / SECTOR).clamp(1, 64) {
            if read_at(&mut r, (vds_loc + k) * SECTOR, &mut d).is_err() {
                break;
            }
            match tag_id(&d) {
                Some(TAG_PD) => pds.push((u16le(&d, 22), u32le(&d, 188) as u64)),
                Some(TAG_LVD) if lvd.is_none() => lvd = Some(d.clone()),
                Some(TAG_TD) => break,
                _ => {}
            }
        }
        let Some(lvd) = lvd else { return broken() };
        let bs = u32le(&lvd, 212) as u64;
        if !(512..=65536).contains(&bs) || !bs.is_power_of_two() {
            return broken();
        }
        let pd_start = |num: u16| pds.iter().find(|p| p.0 == num).map(|p| p.1);
        // 区画の割り当て表
        let n_maps = u32le(&lvd, 268) as usize;
        let mut parts: Vec<Part> = Vec::new();
        let mut pending_meta: Vec<(usize, usize, u32)> = Vec::new(); // (表の番号, 下の区画, メタデータのファイルの場所)
        let mut at = 440usize;
        for _ in 0..n_maps.min(64) {
            let (ty, ml) = (*lvd.get(at).unwrap_or(&0), *lvd.get(at + 1).unwrap_or(&0) as usize);
            if ml < 2 || at + ml > lvd.len() {
                break;
            }
            let m = &lvd[at..at + ml];
            match ty {
                1 => {
                    let Some(start) = pd_start(u16le(m, 4)) else { return broken() };
                    parts.push(Part::Physical { start });
                }
                2 if m.get(5..28).is_some_and(|id| id.starts_with(b"*UDF Metadata Partition")) => {
                    let num = u16le(m, 38);
                    let Some(start) = pd_start(num) else { return broken() };
                    // 下の普通の区画は、同じ区画の番号の物（表に無ければ足す）
                    let under = match parts.iter().position(|p| matches!(p, Part::Physical { start: s } if *s == start)) {
                        Some(i) => i,
                        None => {
                            parts.push(Part::Physical { start });
                            parts.len() - 1
                        }
                    };
                    pending_meta.push((parts.len(), under, u32le(m, 40)));
                    parts.push(Part::Metadata { under, extents: Vec::new() });
                }
                _ => return err("この UDF の区画の形式（仮想区画・スペアリング区画など）には対応していません", "This UDF partition type (virtual, sparable, ...) is not supported"),
            }
            at += ml;
        }
        if parts.is_empty() {
            return broken();
        }
        let fsd_ad = (u16le(&lvd, 256), u32le(&lvd, 252));
        let mut u = Udf { r, bs, parts, root: (0, 0), len };
        // メタデータのファイル（下の区画の中の FE）の割り当てを、メタデータ区画のブロックの並びにする
        for (idx, under, loc) in pending_meta {
            let node = u.read_node(under as u16, loc)?;
            let mut pos = 0u64;
            let mut ext = Vec::new();
            for a in node.ads.iter().filter(|a| a.recorded) {
                let blocks = (a.len as u64).div_ceil(bs);
                ext.push((pos, blocks, a.lbn as u64));
                pos += blocks;
            }
            u.parts[idx] = Part::Metadata { under, extents: ext };
        }
        // ファイルの組の記述子（FSD）からルートのディレクトリ
        let fsd = u.read_block(fsd_ad.0, fsd_ad.1)?;
        if tag_id(&fsd) != Some(TAG_FSD) {
            return broken();
        }
        u.root = (u16le(&fsd, 408), u32le(&fsd, 404));
        Ok(u)
    }

    /// 区画 `part` のブロック `lbn` のイメージの中の位置（バイト）。
    fn pos_of(&self, part: u16, lbn: u64) -> Result<u64, String> {
        let pos = match self.parts.get(part as usize) {
            Some(Part::Physical { start }) => start * SECTOR + lbn * self.bs,
            Some(Part::Metadata { under, extents }) => {
                let Some(&(s, _, ul)) = extents.iter().find(|&&(s, n, _)| lbn >= s && lbn < s + n) else { return broken() };
                match self.parts.get(*under) {
                    Some(Part::Physical { start }) => start * SECTOR + (ul + lbn - s) * self.bs,
                    _ => return broken(),
                }
            }
            None => return broken(),
        };
        if pos >= self.len {
            return broken();
        }
        Ok(pos)
    }

    fn read_block(&mut self, part: u16, lbn: u32) -> Result<Vec<u8>, String> {
        let pos = self.pos_of(part, lbn as u64)?;
        let mut b = vec![0u8; self.bs as usize];
        read_at(&mut self.r, pos, &mut b)?;
        Ok(b)
    }

    /// ファイル項目（FE / EFE）を読む。
    fn read_node(&mut self, part: u16, lbn: u32) -> Result<Node, String> {
        let b = self.read_block(part, lbn)?;
        let (l_ea, l_ad, base) = match tag_id(&b) {
            Some(TAG_FE) => (u32le(&b, 168) as usize, u32le(&b, 172) as usize, 176usize),
            Some(TAG_EFE) => (u32le(&b, 208) as usize, u32le(&b, 212) as usize, 216usize),
            _ => return broken(),
        };
        let file_type = b[27];
        let ad_kind = u16le(&b, 34) & 7;
        let size = u64le(&b, 56);
        let start = base + l_ea;
        let Some(area) = b.get(start..start + l_ad) else { return broken() };
        let mut node = Node { is_dir: file_type == 4, size, ads: Vec::new(), embedded: None };
        match ad_kind {
            3 => node.embedded = Some(area[..area.len().min(size as usize)].to_vec()),
            0 | 1 => self.collect_ads(area, ad_kind, part, &mut node.ads)?,
            _ => return err("この UDF の割り当ての形式（ext_ad）には対応していません", "This UDF allocation type (ext_ad) is not supported"),
        }
        Ok(node)
    }

    /// 割り当ての並び（short_ad = 8 バイト / long_ad = 16 バイト）を読み、続き（AED）もたどる。
    fn collect_ads(&mut self, area: &[u8], kind: u16, part: u16, out: &mut Vec<Ad>) -> Result<(), String> {
        let step = if kind == 0 { 8 } else { 16 };
        let mut area = area.to_vec();
        for _ in 0..MAX_AED {
            let mut next: Option<(u16, u32, u32)> = None;
            for c in area.chunks_exact(step) {
                let raw = u32le(c, 0);
                let (len, ty) = (raw & 0x3FFF_FFFF, raw >> 30);
                if len == 0 {
                    break;
                }
                let (lbn, p) = if kind == 0 { (u32le(c, 4), part) } else { (u32le(c, 4), u16le(c, 8)) };
                if ty == 3 {
                    next = Some((p, lbn, len));
                    break;
                }
                out.push(Ad { part: p, lbn, len, recorded: ty == 0 });
                if out.len() > 1 << 20 {
                    return broken();
                }
            }
            let Some((p, lbn, len)) = next else { return Ok(()) };
            let b = self.read_block(p, lbn)?;
            if tag_id(&b) != Some(TAG_AED) {
                return broken();
            }
            let l = (u32le(&b, 20) as usize).min(len as usize).min(b.len().saturating_sub(24));
            area = b[24..24 + l].to_vec();
        }
        broken()
    }

    /// 中身を読む（`max` を超えるならエラー）。書かれていない部分は 0。
    fn read_data(&mut self, node: &Node, max: u64) -> Result<Vec<u8>, String> {
        if node.size > max {
            return err("ファイルが大きすぎます", "File too large");
        }
        if let Some(e) = &node.embedded {
            return Ok(e.clone());
        }
        let mut out: Vec<u8> = Vec::with_capacity(node.size as usize);
        for a in node.ads.clone() {
            let want = (a.len as u64).min(node.size - out.len() as u64) as usize;
            if want == 0 {
                break;
            }
            let start = out.len();
            out.resize(start + want, 0);
            if a.recorded {
                let pos = self.pos_of(a.part, a.lbn as u64)?;
                // メタデータ区画は、ブロックごとに場所が飛ぶことがあるので、ブロックずつ読む
                if matches!(self.parts.get(a.part as usize), Some(Part::Metadata { .. })) {
                    let bs = self.bs as usize;
                    for (k, chunk) in out[start..].chunks_mut(bs).enumerate() {
                        let p = self.pos_of(a.part, a.lbn as u64 + k as u64)?;
                        read_at(&mut self.r, p, chunk)?;
                    }
                } else {
                    read_at(&mut self.r, pos, &mut out[start..])?;
                }
            }
        }
        out.resize(node.size as usize, 0);
        Ok(out)
    }

    /// ディレクトリの項目（名前, フォルダか, 項目の場所）。親と消された物は除く。
    fn read_dir(&mut self, node: &Node) -> Result<Vec<(String, bool, (u16, u32))>, String> {
        if node.size > MAX_DIR {
            return err("ディレクトリが大きすぎます", "Directory too large");
        }
        let data = self.read_data(node, MAX_DIR)?;
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + 38 <= data.len() {
            let d = &data[i..];
            if tag_id(d) != Some(TAG_FID) {
                break;
            }
            let (ch, l_fi, l_iu) = (d[18], d[19] as usize, u16le(d, 36) as usize);
            let total = (38 + l_iu + l_fi + 3) & !3;
            if i + total > data.len() {
                break;
            }
            let icb = (u16le(d, 28), u32le(d, 24));
            if ch & 0x0C == 0 && l_fi > 0 {
                let name = dstring(&d[38 + l_iu..38 + l_iu + l_fi]);
                if !name.is_empty() && name != "." && name != ".." && !name.contains('/') {
                    out.push((name, ch & 0x02 != 0, icb));
                }
            }
            i += total;
        }
        Ok(out)
    }

    fn walk(&mut self, dir: (u16, u32), prefix: &str, depth: usize, seen: &mut HashSet<(u16, u32)>, out: &mut Vec<Entry>, visit: &mut dyn FnMut(&str, &Node)) -> Result<(), String> {
        if depth > MAX_DEPTH || !seen.insert(dir) {
            return Ok(());
        }
        let node = self.read_node(dir.0, dir.1)?;
        for (name, is_dir, icb) in self.read_dir(&node)? {
            if out.len() >= MAX_ENTRIES {
                return Ok(());
            }
            let path = format!("{prefix}{name}{}", if is_dir { "/" } else { "" });
            if is_dir {
                out.push(Entry { path: path.clone(), is_dir: true, size: 0, packed: None });
                let _ = self.walk(icb, &path, depth + 1, seen, out, visit);
            } else {
                // 大きさはファイル項目にあるので、1 つずつ読む（読めない物は 0 として一覧には出す）
                let child = self.read_node(icb.0, icb.1).ok();
                out.push(Entry { path: path.clone(), is_dir: false, size: child.as_ref().map_or(0, |n| n.size), packed: None });
                if let Some(n) = child {
                    visit(&path, &n);
                }
            }
        }
        Ok(())
    }

    fn list(&mut self, out: &mut Vec<Entry>) -> Result<(), String> {
        let root = self.root;
        self.walk(root, "", 0, &mut HashSet::new(), out, &mut |_, _| {})
    }

    fn lookup(&mut self, path: &str) -> Result<Option<Node>, String> {
        let mut node = self.read_node(self.root.0, self.root.1)?;
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for part in parts {
            if !node.is_dir {
                return Ok(None);
            }
            let Some((_, _, icb)) = self.read_dir(&node)?.into_iter().find(|(n, _, _)| n == part) else { return Ok(None) };
            node = self.read_node(icb.0, icb.1)?;
        }
        Ok(Some(node))
    }
}

/// UDF の名前（先頭の 1 バイトが形式: 8 = 1 バイトずつ、16 = UCS-2 の大きい方から）。
fn dstring(b: &[u8]) -> String {
    match b.split_first() {
        Some((8, rest)) => rest.iter().map(|&c| c as char).collect(),
        Some((16, rest)) => String::from_utf16_lossy(&rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()),
        _ => String::new(),
    }
}

// ------------------------------------------------------------------ isofs から呼ぶ入口

pub fn list<R: Read + Seek>(r: R) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    Udf::open(r)?.list(&mut out)?;
    out.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

pub fn read_path<R: Read + Seek>(r: R, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut u = Udf::open(r)?;
    match u.lookup(path)? {
        Some(n) if !n.is_dir => u.read_data(&n, max),
        _ => err("見つかりません", "Not found"),
    }
}

pub fn each_file<R: Read + Seek>(r: R, max_each: u64, f: &mut dyn FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    let mut u = Udf::open(r)?;
    let mut files: Vec<(String, Node)> = Vec::new();
    let root = u.root;
    u.walk(root, "", 0, &mut HashSet::new(), &mut Vec::new(), &mut |p, n| files.push((p.to_string(), n.clone())))?;
    for (p, n) in files.into_iter().filter(|(_, n)| n.size <= max_each) {
        if let Ok(data) = u.read_data(&n, max_each) {
            if !f(&p, data) {
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Windows の IMAPI で作った小さな UDF のイメージ（gzip）。中身は readme.txt・資料/data.bin（5000 バイト）・資料/深い/長い名前.txt。
    pub fn fixture(name: &str) -> Vec<u8> {
        let gz: &[u8] = match name {
            "102" => include_bytes!("testdata/udf_102.iso.gz"),
            "250" => include_bytes!("testdata/udf_250.iso.gz"),
            _ => include_bytes!("testdata/bridge.iso.gz"),
        };
        crate::neo::deflate::gunzip(gz, 8 << 20).unwrap()
    }

    fn check(img: &[u8]) {
        let es = list(std::io::Cursor::new(img)).unwrap();
        let v: Vec<(String, u64)> = es.iter().map(|e| (e.path.clone(), e.size)).collect();
        assert!(v.contains(&("readme.txt".into(), 10)), "{v:?}");
        assert!(v.contains(&("資料/data.bin".into(), 5000)), "{v:?}");
        assert!(v.contains(&("資料/深い/とても長い名前のファイルです_long_name_test.txt".into(), 37)), "{v:?}");
        assert_eq!(read_path(std::io::Cursor::new(img), "readme.txt", 1 << 20).unwrap(), b"hello udf\n");
        assert_eq!(read_path(std::io::Cursor::new(img), "資料/data.bin", 1 << 20).unwrap().len(), 5000);
        let t = read_path(std::io::Cursor::new(img), "資料/深い/とても長い名前のファイルです_long_name_test.txt", 1 << 20).unwrap();
        assert_eq!(String::from_utf8(t).unwrap(), "日本語の長い名前のテスト\n");
        assert!(read_path(std::io::Cursor::new(img), "無い.txt", 1 << 20).is_err());
    }

    #[test]
    fn reads_udf_102() {
        check(&fixture("102"));
    }

    #[test]
    fn reads_udf_250_with_metadata_partition() {
        check(&fixture("250"));
    }

    #[test]
    fn bridge_image_uses_udf() {
        check(&fixture("bridge"));
    }

    #[test]
    fn broken_images_give_errors() {
        let img = fixture("102");
        for cut in [0, 100, 17 * 2048, 257 * 2048, img.len() / 2] {
            let _ = list(std::io::Cursor::new(&img[..cut]));
        }
        assert!(list(std::io::Cursor::new(&[0u8; 4096][..])).is_err());
    }
}
