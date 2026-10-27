// Minecraft（Java 版）のリージョンファイル（アンビル形式 .mca）。32 x 32 チャンクの NBT を 4 KiB の区画に並べたもの。
// 先頭 4 KiB が各チャンクの場所（区画の番号 3 バイト + 区画の数 1 バイト）、次の 4 KiB が更新時刻。
// チャンクは「長さ 4 バイト + 圧縮の種類 1 バイト（1 gzip・2 zlib・3 なし・4 LZ4）+ 本体」。GUI なし・依存なし。
use super::nbt::{self, Compression};

const SECTOR: usize = 4096;
pub const CHUNKS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// リージョンの中の位置（0〜31）。
    pub x: u32,
    pub z: u32,
    pub sector: usize,
    pub count: usize,
    pub timestamp: u32,
}

impl Entry {
    pub fn index(&self) -> usize {
        (self.z * 32 + self.x) as usize
    }
}

pub fn is_region_path(p: &std::path::Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mca") || e.eq_ignore_ascii_case("mcc") || e.eq_ignore_ascii_case("mcr"))
}

fn be_u32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// 中身のあるチャンクの一覧（z, x の順）。
pub fn list(b: &[u8]) -> Result<Vec<Entry>, String> {
    if b.is_empty() {
        return Ok(Vec::new()); // Minecraft が作ったばかりの空のファイル
    }
    if b.len() < 2 * SECTOR {
        return Err("リージョンファイルとして短すぎます".into());
    }
    let mut out = Vec::new();
    for i in 0..CHUNKS {
        let loc = be_u32(b, i * 4);
        let (sector, count) = ((loc >> 8) as usize, (loc & 0xff) as usize);
        if sector == 0 && count == 0 {
            continue;
        }
        out.push(Entry { x: (i % 32) as u32, z: (i / 32) as u32, sector, count, timestamp: be_u32(b, SECTOR + i * 4) });
    }
    Ok(out)
}

/// チャンクの NBT（解凍済み）と、その圧縮の種類。
pub fn read_chunk(b: &[u8], e: &Entry) -> Result<(Compression, Vec<u8>), String> {
    let start = e.sector * SECTOR;
    if start + 5 > b.len() {
        return Err(format!("チャンク {},{} の場所がファイルの外です", e.x, e.z));
    }
    let len = be_u32(b, start) as usize;
    let kind = b[start + 4];
    if kind & 0x80 != 0 {
        return Err(format!("チャンク {},{} は別のファイル（.mcc）に入っています（対応していません）", e.x, e.z));
    }
    if len < 1 || start + 4 + len > b.len() {
        return Err(format!("チャンク {},{} の長さが壊れています", e.x, e.z));
    }
    let body = &b[start + 5..start + 4 + len];
    let raw = match kind {
        1 => super::deflate::gunzip(body, nbt::MAX_RAW)?,
        2 => super::deflate::unzlib(body, nbt::MAX_RAW)?,
        3 => body.to_vec(),
        4 => return Err(format!("チャンク {},{} は LZ4 で圧縮されています（対応していません）", e.x, e.z)),
        k => return Err(format!("チャンク {},{} の圧縮の種類 {k} を知りません", e.x, e.z)),
    };
    let c = match kind {
        1 => Compression::Gzip,
        3 => Compression::None,
        _ => Compression::Zlib,
    };
    Ok((c, raw))
}

/// チャンク 1 つを差し替えたファイル全体を作る。ほかのチャンクの中身はそのまま、詰め直して並べる。
/// 更新時刻は `now`（秒）にする。
pub fn replace_chunk(b: &[u8], index: usize, c: Compression, raw: &[u8], now: u32) -> Result<Vec<u8>, String> {
    let entries = list(b)?;
    let mut chunks: Vec<(usize, Vec<u8>, u32)> = Vec::new(); // (番号, 区画に書く「長さ + 種類 + 本体」, 時刻)
    for e in &entries {
        if e.index() == index {
            continue;
        }
        let start = e.sector * SECTOR;
        let len = b.get(start..start + 4).map(|_| be_u32(b, start) as usize).unwrap_or(0);
        let Some(bytes) = b.get(start..start + 4 + len) else { continue };
        chunks.push((e.index(), bytes.to_vec(), e.timestamp));
    }
    let packed = nbt::pack(c, raw);
    let kind = match c {
        Compression::Gzip => 1,
        Compression::Zlib => 2,
        Compression::None => 3,
    };
    let mut rec = ((packed.len() + 1) as u32).to_be_bytes().to_vec();
    rec.push(kind);
    rec.extend(packed);
    if rec.len().div_ceil(SECTOR) > 255 {
        return Err("チャンクが大きすぎます（1 MiB を超えます）".into());
    }
    chunks.push((index, rec, now));
    chunks.sort_by_key(|c| c.0);

    let mut out = vec![0u8; 2 * SECTOR];
    for (i, bytes, ts) in chunks {
        let sector = out.len() / SECTOR;
        let count = bytes.len().div_ceil(SECTOR);
        out[i * 4..i * 4 + 4].copy_from_slice(&((sector as u32) << 8 | count as u32).to_be_bytes());
        out[SECTOR + i * 4..SECTOR + i * 4 + 4].copy_from_slice(&ts.to_be_bytes());
        out.extend(&bytes);
        out.resize((sector + count) * SECTOR, 0);
    }
    Ok(out)
}

/// チャンクの一覧の文字（1 行に 1 チャンク。先頭の 2 つの数が x z）。`time` は更新時刻（秒）を日時にする。
pub fn listing(path: &std::path::Path, entries: &[Entry], time: impl Fn(u32) -> String) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut out = format!("# {name}: チャンク {} 個（行で Enter かダブルクリックで、そのチャンクを開く）\n#   x   z   大きさ  更新日時\n", entries.len());
    for e in entries {
        let t = if e.timestamp == 0 { String::new() } else { time(e.timestamp) };
        out.push_str(&format!("{:>5}{:>4} {:>6} KB  {t}\n", e.x, e.z, e.count * SECTOR / 1024));
    }
    out
}

/// 一覧の 1 行から、チャンクの位置（x, z）を読む。
pub fn parse_listing_line(line: &str) -> Option<(u32, u32)> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let mut it = line.split_whitespace();
    let x: u32 = it.next()?.parse().ok()?;
    let z: u32 = it.next()?.parse().ok()?;
    (x < 32 && z < 32).then_some((x, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neo::nbt::Tag;

    fn chunk(n: i32) -> Vec<u8> {
        nbt::write("", &Tag::Compound(vec![("xPos".into(), Tag::Int(n)), ("pad".into(), Tag::ByteArray(vec![7; 6000]))]))
    }

    #[test]
    fn build_list_read_and_replace() {
        let mut file = Vec::new();
        file = replace_chunk(&file, 0, Compression::Zlib, &chunk(0), 10).unwrap();
        file = replace_chunk(&file, 33, Compression::Gzip, &chunk(33), 20).unwrap();
        file = replace_chunk(&file, 1023, Compression::None, &chunk(1023), 30).unwrap();
        let l = list(&file).unwrap();
        assert_eq!(l.iter().map(|e| (e.x, e.z, e.timestamp)).collect::<Vec<_>>(), vec![(0, 0, 10), (1, 1, 20), (31, 31, 30)]);
        for e in &l {
            let (_, raw) = read_chunk(&file, e).unwrap();
            assert_eq!(raw, chunk(e.index() as i32));
        }
        // 大きくなる差し替えでも、ほかのチャンクは壊れない
        let big = nbt::write("", &Tag::Compound(vec![("pad".into(), Tag::LongArray((0..20_000).collect()))]));
        let file2 = replace_chunk(&file, 33, Compression::Zlib, &big, 40).unwrap();
        let l2 = list(&file2).unwrap();
        assert_eq!(read_chunk(&file2, &l2[1]).unwrap().1, big);
        assert_eq!(read_chunk(&file2, &l2[0]).unwrap().1, chunk(0));
        assert_eq!(read_chunk(&file2, &l2[2]).unwrap().1, chunk(1023));
        assert_eq!(file2.len() % SECTOR, 0);
    }

    #[test]
    fn listing_lines_point_back_to_chunks() {
        let e = Entry { x: 3, z: 31, sector: 2, count: 2, timestamp: 0 };
        let text = listing(std::path::Path::new("/w/r.0.0.mca"), &[e], |_| "t".into());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(parse_listing_line(lines[0]), None);
        assert_eq!(parse_listing_line(lines[2]), Some((3, 31)));
        assert_eq!(parse_listing_line("40 1"), None);
    }

    #[test]
    fn broken_files_are_errors() {
        assert!(list(&[0; 100]).is_err());
        assert_eq!(list(&[]).unwrap(), vec![]);
        let mut f = vec![0u8; 2 * SECTOR];
        f[3] = 1; // 区画 0・数 1 だが、中身が無い
        f[2] = 9;
        let l = list(&f).unwrap();
        assert!(read_chunk(&f, &l[0]).is_err());
    }
}
