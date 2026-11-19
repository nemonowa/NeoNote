// Windows のサムネイル・アイコンのキャッシュ（thumbcache_*.db・iconcache_*.db）を、一覧として読み取り専用で開く（GUI なし）。
// 形式: 先頭に `CMMM` と版の番号の見出し（24 バイト）、続いて `CMMM` で始まる記録が並ぶ。記録は 56 バイトの見出し・名前（UTF-16）・
// 埋め・画像のデータ（BMP・PNG・JPEG）。見出しの中の並びは版で違う（Windows 8 以降＝版 0x1A 以上と、Vista・7）。
// 一覧の名前は「キャッシュの番号（16 桁の 16 進）_幅x高さ.拡張子」。空の記録（データの大きさ 0）は出さない。
use super::archive::Entry;
use std::path::Path;

const HEADER: usize = 24;
const RECORD_HEAD: usize = 56;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    name: String,
    off: usize,
    len: usize,
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], p: usize) -> Option<u64> {
    b.get(p..p + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
}

/// キャッシュのファイルか（拡張子 .db と、先頭の `CMMM`）。
pub fn is_thumbcache_path(p: &Path) -> bool {
    use std::io::Read;
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("db")) || p.is_dir() {
        return false;
    }
    let mut head = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"CMMM"
}

/// 記録の一覧。壊れた記録に当たったら、そこまでを返す（読めた分は見せる）。
fn items(b: &[u8]) -> Result<Vec<Item>, String> {
    if !b.starts_with(b"CMMM") || b.len() < HEADER {
        return Err(crate::lang::tr("サムネイルのキャッシュではありません", "Not a thumbnail cache").into());
    }
    let version = u32_at(b, 4).unwrap_or(0);
    let new_layout = version >= 0x1A;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut p = HEADER;
    while p + RECORD_HEAD <= b.len() && &b[p..p + 4] == b"CMMM" {
        let Some(size) = u32_at(b, p + 4).map(|s| s as usize) else { break };
        if size < RECORD_HEAD || p + size > b.len() {
            break;
        }
        let hash = u64_at(b, p + 8).unwrap_or(0);
        let (id_len, pad, data_len, wh) = if new_layout {
            (u32_at(b, p + 16), u32_at(b, p + 20), u32_at(b, p + 24), Some((u32_at(b, p + 28).unwrap_or(0), u32_at(b, p + 32).unwrap_or(0))))
        } else {
            (u32_at(b, p + 24), u32_at(b, p + 28), u32_at(b, p + 32), None)
        };
        let (id_len, pad, data_len) = (id_len.unwrap_or(0) as usize, pad.unwrap_or(0) as usize, data_len.unwrap_or(0) as usize);
        let off = p + RECORD_HEAD + id_len + pad;
        if data_len > 0 && off + data_len <= p + size {
            let data = &b[off..off + data_len];
            let ext = super::imagefile::sniff(data).unwrap_or("bin");
            let mut name = match wh {
                Some((w, h)) if w > 0 && h > 0 => format!("{hash:016x}_{w}x{h}.{ext}"),
                _ => format!("{hash:016x}.{ext}"),
            };
            // 同じ番号が 2 度出たら、見分けられるように後ろへ番号を付ける
            let mut n = 2;
            while !seen.insert(name.clone()) {
                name = format!("{hash:016x}_{n}.{ext}");
                n += 1;
            }
            out.push(Item { name, off, len: data_len });
        }
        p += size;
    }
    Ok(out)
}

pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    let mut v: Vec<Entry> = items(b)?.into_iter().map(|i| Entry { path: i.name, is_dir: false, size: i.len as u64, packed: None }).collect();
    v.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(v)
}

fn read_all(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| e.to_string())
}

pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    list_bytes(&read_all(p)?)
}

pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let b = read_all(p)?;
    let it = items(&b)?.into_iter().find(|i| i.name == path).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
    if it.len as u64 > max {
        return Err(crate::lang::tr("大きすぎます", "Too large").into());
    }
    Ok(b[it.off..it.off + it.len].to_vec())
}

pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    let b = read_all(p)?;
    for it in items(&b)?.into_iter().filter(|i| i.len as u64 <= max_each) {
        if !f(&it.name, b[it.off..it.off + it.len].to_vec()) {
            break;
        }
    }
    Ok(())
}

pub fn extract_to_temp(image: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    super::extfs::write_temp(image, path, &read_path(image, path, super::extfs::EXTRACT_MAX)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows 10 の形（版 0x20）の記録を 1 つ作る。
    fn record(hash: u64, w: u32, h: u32, data: &[u8]) -> Vec<u8> {
        let id: Vec<u8> = "abc".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let size = RECORD_HEAD + id.len() + 2 + data.len();
        let mut r = b"CMMM".to_vec();
        r.extend((size as u32).to_le_bytes());
        r.extend(hash.to_le_bytes());
        r.extend((id.len() as u32).to_le_bytes());
        r.extend(2u32.to_le_bytes());
        r.extend((data.len() as u32).to_le_bytes());
        r.extend(w.to_le_bytes());
        r.extend(h.to_le_bytes());
        r.extend([0u8; 20]);
        r.extend(id);
        r.extend([0u8; 2]);
        r.extend(data);
        r
    }

    fn cache(records: &[Vec<u8>]) -> Vec<u8> {
        let mut b = b"CMMM".to_vec();
        b.extend(0x20u32.to_le_bytes());
        b.extend([0u8; 8]);
        b.extend(24u32.to_le_bytes());
        b.extend([0u8; 4]);
        for r in records {
            b.extend(r);
        }
        b
    }

    #[test]
    fn lists_and_reads_thumbnails() {
        let bmp = b"BM0123456789abcdef".to_vec();
        let png = b"\x89PNG\r\n\x1a\nxyz".to_vec();
        let b = cache(&[record(0x1122, 16, 16, &bmp), record(0x33, 0, 0, &[]), record(0x44, 96, 64, &png)]);
        let v = list_bytes(&b).unwrap();
        let names: Vec<&str> = v.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["0000000000000044_96x64.png", "0000000000001122_16x16.bmp"], "空の記録は出さず、名前の順");
        assert_eq!(v.iter().find(|e| e.path.ends_with(".png")).unwrap().size, png.len() as u64);
        let it = items(&b).unwrap();
        let p = it.iter().find(|i| i.name.ends_with(".bmp")).unwrap();
        assert_eq!(&b[p.off..p.off + p.len], bmp.as_slice());
    }

    #[test]
    fn broken_input_does_not_panic() {
        assert!(items(b"").is_err());
        assert!(items(b"XXXX").is_err());
        let mut b = cache(&[record(1, 16, 16, b"BM0123456789abcdef")]);
        b.truncate(b.len() - 5);
        assert_eq!(items(&b).unwrap().len(), 0, "途中で切れた記録は出さない");
        for cut in 0..b.len() {
            let _ = items(&b[..cut]);
        }
    }
}
