// アイコンのファイル（.ico・.cur・.icns）を、大きさごとの画像の一覧として読み取り専用で開く（GUI なし）。
// .ico・.cur: 6 バイトの見出しと 16 バイトの項目の並び。中身は PNG か、BMP の見出しの無い DIB（高さは色と透明の 2 枚分）。
//   DIB は、BMP のファイルの見出しを付け、高さを半分（色の部分だけ）に直して .bmp として見せる。
// .icns（macOS）: `icns` と全体の長さの後に「種類 4 文字・長さ」の塊が並ぶ。PNG・JPEG の塊だけを出す（古い独自形式は出さない）。
use super::archive::Entry;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    name: String,
    data: Vec<u8>,
}

fn u16_le(b: &[u8], p: usize) -> Option<u16> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_le(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u32_be(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn is_ico(b: &[u8]) -> bool {
    b.len() >= 6 && b[0] == 0 && b[1] == 0 && matches!(b[2], 1 | 2) && b[3] == 0 && u16_le(b, 4).is_some_and(|n| n > 0)
}

/// アイコンのファイルか（拡張子と、先頭の中身）。
pub fn is_icon_path(p: &Path) -> bool {
    use std::io::Read;
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "ico" | "cur" | "icns") || p.is_dir() {
        return false;
    }
    let mut head = [0u8; 6];
    if std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_err() {
        return false;
    }
    if ext == "icns" { &head[..4] == b"icns" } else { is_ico(&head) }
}

/// DIB（BITMAPINFOHEADER から始まる）に、BMP のファイルの見出しを付ける。高さは色の部分だけにする。
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let hsize = u32_le(dib, 0)? as usize;
    if hsize < 40 || dib.len() < hsize {
        return None;
    }
    let h = i32::from_le_bytes(dib.get(8..12)?.try_into().ok()?);
    let bpp = u16_le(dib, 14)? as usize;
    let used = u32_le(dib, 32)? as usize;
    let colors = if bpp <= 8 { if used == 0 { 1 << bpp } else { used } } else { 0 };
    let mut body = dib.to_vec();
    body[8..12].copy_from_slice(&(h / 2).to_le_bytes());
    let off = 14 + hsize + colors * 4;
    let mut out = Vec::with_capacity(14 + body.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(off as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Some(out)
}

fn ico_items(b: &[u8]) -> Result<Vec<Item>, String> {
    if !is_ico(b) {
        return Err(crate::lang::tr("アイコンのファイルではありません", "Not an icon file").into());
    }
    let n = u16_le(b, 4).unwrap_or(0) as usize;
    let mut out = Vec::new();
    for i in 0..n {
        let e = 6 + i * 16;
        let (Some(len), Some(off)) = (u32_le(b, e + 8), u32_le(b, e + 12)) else { break };
        let (len, off) = (len as usize, off as usize);
        let Some(data) = off.checked_add(len).and_then(|end| b.get(off..end)) else { continue };
        // 幅・高さの 0 は 256
        let side = |v: u8| if v == 0 { 256 } else { v as u32 };
        let (w, h) = (side(b[e]), side(b[e + 1]));
        let (ext, img) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            ("png", data.to_vec())
        } else if let Some(bmp) = dib_to_bmp(data) {
            ("bmp", bmp)
        } else {
            continue;
        };
        let bpp = u16_le(b, e + 6).unwrap_or(0);
        let name = if bpp > 0 && b[2] == 1 { format!("{:02}_{w}x{h}_{bpp}bit.{ext}", i + 1) } else { format!("{:02}_{w}x{h}.{ext}", i + 1) };
        out.push(Item { name, data: img });
    }
    Ok(out)
}

fn icns_items(b: &[u8]) -> Result<Vec<Item>, String> {
    if !b.starts_with(b"icns") {
        return Err(crate::lang::tr("アイコンのファイルではありません", "Not an icon file").into());
    }
    let total = (u32_be(b, 4).unwrap_or(0) as usize).min(b.len());
    let mut out = Vec::new();
    let mut p = 8;
    let mut i = 0;
    while p + 8 <= total {
        let kind = String::from_utf8_lossy(&b[p..p + 4]).into_owned();
        let len = u32_be(b, p + 4).unwrap_or(0) as usize;
        if len < 8 || p + len > total {
            break;
        }
        let data = &b[p + 8..p + len];
        if let Some(ext) = super::imagefile::sniff(data).filter(|e| matches!(*e, "png" | "jpg")) {
            i += 1;
            let kind: String = kind.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            out.push(Item { name: format!("{i:02}_{kind}.{ext}"), data: data.to_vec() });
        }
        p += len;
    }
    Ok(out)
}

fn items(b: &[u8]) -> Result<Vec<Item>, String> {
    if b.starts_with(b"icns") { icns_items(b) } else { ico_items(b) }
}

pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    Ok(items(b)?.into_iter().map(|i| Entry { path: i.name, is_dir: false, size: i.data.len() as u64, packed: None }).collect())
}

fn read_all(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| e.to_string())
}

pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    list_bytes(&read_all(p)?)
}

pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let it = items(&read_all(p)?)?.into_iter().find(|i| i.name == path).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
    if it.data.len() as u64 > max {
        return Err(crate::lang::tr("大きすぎます", "Too large").into());
    }
    Ok(it.data)
}

pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    for it in items(&read_all(p)?)?.into_iter().filter(|i| i.data.len() as u64 <= max_each) {
        if !f(&it.name, it.data) {
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

    /// 2×2・32 ビットの DIB（色 2 行＋透明の 2 行分の高さ）。
    fn dib() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(40u32.to_le_bytes());
        d.extend(2i32.to_le_bytes());
        d.extend(4i32.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(32u16.to_le_bytes());
        d.extend([0u8; 24]);
        d.extend([0xAAu8; 16]); // 色
        d.extend([0u8; 8]); // 透明（1 ビット、行は 4 バイト）
        d
    }

    fn ico(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut b = vec![0, 0, 1, 0];
        b.extend((entries.len() as u16).to_le_bytes());
        let mut off = 6 + 16 * entries.len();
        let mut body: Vec<u8> = Vec::new();
        for (side, data) in entries {
            b.extend([*side, *side, 0, 0, 1, 0, 32, 0]);
            b.extend((data.len() as u32).to_le_bytes());
            b.extend((off as u32).to_le_bytes());
            off += data.len();
            body.extend(data);
        }
        b.extend(body);
        b
    }

    #[test]
    fn ico_lists_png_and_dib_entries() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let b = ico(&[(0, png.clone()), (2, dib())]);
        let v = list_bytes(&b).unwrap();
        assert_eq!(v.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["01_256x256_32bit.png", "02_2x2_32bit.bmp"]);
        let it = items(&b).unwrap();
        assert_eq!(it[0].data, png);
        let bmp = &it[1].data;
        assert!(bmp.starts_with(b"BM"));
        assert_eq!(u32_le(bmp, 2), Some(bmp.len() as u32));
        assert_eq!(u32_le(bmp, 10), Some(14 + 40), "32 ビットは色の表なし");
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), 2, "高さは色の部分だけ");
    }

    #[test]
    fn icns_lists_png_chunks_only() {
        let png = b"\x89PNG\r\n\x1a\nxx".to_vec();
        let mut chunks = Vec::new();
        for (k, d) in [(b"ic08", png.clone()), (b"is32", vec![1, 2, 3])] {
            chunks.extend(k);
            chunks.extend(((d.len() + 8) as u32).to_be_bytes());
            chunks.extend(d);
        }
        let mut b = b"icns".to_vec();
        b.extend(((chunks.len() + 8) as u32).to_be_bytes());
        b.extend(chunks);
        let v = list_bytes(&b).unwrap();
        assert_eq!(v.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["01_ic08.png"]);
    }

    #[test]
    fn broken_icons_do_not_panic() {
        let b = ico(&[(16, dib())]);
        for cut in 0..b.len() {
            let _ = items(&b[..cut]);
        }
        assert!(items(b"").is_err());
        let mut bad = b.clone();
        bad[18..22].copy_from_slice(&u32::MAX.to_le_bytes()); // 中身の位置が外
        assert!(items(&bad).unwrap().is_empty());
    }
}
