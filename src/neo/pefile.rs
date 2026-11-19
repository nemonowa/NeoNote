// Windows の実行ファイル（.exe・.dll など。先頭の MZ と PE の見出し）の中の資源（リソース）を、一覧として読み取り専用で開く（GUI なし）。
// 一覧: `icons/<名前>.ico`（アイコンの組ごとに .ico のファイルに組み立てる）、`version.txt`（版の情報）、`manifest.xml`。
// 資源の表（.rsrc）は「種類 → 名前 → 言語 → データ」の 3 段の木。たどる深さと数に上限を付け、壊れた入力で回り続けない。
use super::archive::Entry;
use std::path::Path;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_VERSION: u32 = 16;
const RT_MANIFEST: u32 = 24;
/// 1 つの段の項目の数の上限。
const MAX_ENTRIES: usize = 4096;

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// 実行ファイルか（拡張子と、MZ・PE の見出し）。
pub fn is_pe_path(p: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "exe" | "dll" | "ocx" | "cpl" | "scr" | "mui") || p.is_dir() {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    let mut head = [0u8; 64];
    if f.read_exact(&mut head).is_err() || &head[..2] != b"MZ" {
        return false;
    }
    let off = u32::from_le_bytes([head[60], head[61], head[62], head[63]]) as u64;
    let mut sig = [0u8; 4];
    f.seek(SeekFrom::Start(off)).is_ok() && f.read_exact(&mut sig).is_ok() && &sig == b"PE\0\0"
}

struct Pe<'a> {
    b: &'a [u8],
    /// .rsrc の、ファイルの中の位置（無ければ 0）。
    rsrc: usize,
    sections: Vec<(u32, u32, u32)>, // (仮想アドレス, 大きさ, ファイルの位置)
}

/// 資源 1 つ（種類・名前・データ）。
struct Res {
    kind: u32,
    name: String,
    id: Option<u32>,
    data: Vec<u8>,
}

impl<'a> Pe<'a> {
    fn open(b: &'a [u8]) -> Result<Pe<'a>, String> {
        let bad = || crate::lang::tr("実行ファイルの形が読めません", "Cannot read the executable").to_string();
        if b.get(..2) != Some(b"MZ") {
            return Err(bad());
        }
        let pe = u32_at(b, 60).ok_or_else(bad)? as usize;
        if b.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(bad());
        }
        let coff = pe + 4;
        let nsec = u16_at(b, coff + 2).ok_or_else(bad)? as usize;
        let opt_size = u16_at(b, coff + 16).ok_or_else(bad)? as usize;
        let opt = coff + 20;
        let dirs = match u16_at(b, opt).ok_or_else(bad)? {
            0x10b => opt + 96,
            0x20b => opt + 112,
            _ => return Err(bad()),
        };
        let rsrc_rva = u32_at(b, dirs + 2 * 8).ok_or_else(bad)?;
        let mut sections = Vec::new();
        let table = opt + opt_size;
        for i in 0..nsec.min(96) {
            let s = table + i * 40;
            let (Some(vsize), Some(va), Some(raw_size), Some(raw)) = (u32_at(b, s + 8), u32_at(b, s + 12), u32_at(b, s + 16), u32_at(b, s + 20)) else { break };
            sections.push((va, vsize.max(raw_size), raw));
        }
        let mut pe = Pe { b, rsrc: 0, sections };
        if rsrc_rva == 0 {
            return Ok(pe);
        }
        pe.rsrc = pe.offset(rsrc_rva).ok_or_else(bad)?;
        Ok(pe)
    }

    fn offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find(|(va, size, _)| rva >= *va && rva < va.saturating_add(*size)).map(|(va, _, raw)| (rva - va + raw) as usize)
    }

    /// 資源の木の 1 段の項目（名前か番号, 子の位置, 子が段か）。
    fn dir(&self, at: usize) -> Vec<(Result<u32, String>, usize, bool)> {
        let b = self.b;
        let (Some(named), Some(ids)) = (u16_at(b, at + 12), u16_at(b, at + 14)) else { return Vec::new() };
        let n = (named as usize + ids as usize).min(MAX_ENTRIES);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let e = at + 16 + i * 8;
            let (Some(name), Some(off)) = (u32_at(b, e), u32_at(b, e + 4)) else { break };
            let key = if name & 0x8000_0000 != 0 {
                let s = self.rsrc + (name & 0x7FFF_FFFF) as usize;
                let len = u16_at(b, s).unwrap_or(0) as usize;
                let units: Vec<u16> = (0..len).filter_map(|k| u16_at(b, s + 2 + k * 2)).collect();
                Err(String::from_utf16_lossy(&units))
            } else {
                Ok(name)
            };
            out.push((key, self.rsrc + (off & 0x7FFF_FFFF) as usize, off & 0x8000_0000 != 0));
        }
        out
    }

    fn resources(&self) -> Vec<Res> {
        let mut out = Vec::new();
        if self.rsrc == 0 {
            return out;
        }
        for (kind, kd, ksub) in self.dir(self.rsrc) {
            let Ok(kind) = kind else { continue };
            if !ksub || !matches!(kind, RT_ICON | RT_GROUP_ICON | RT_VERSION | RT_MANIFEST) {
                continue;
            }
            for (name, nd, nsub) in self.dir(kd) {
                if !nsub {
                    continue;
                }
                // 言語の段は、最初の 1 つだけ使う
                let Some((_, ld, lsub)) = self.dir(nd).into_iter().next() else { continue };
                if lsub {
                    continue;
                }
                let (Some(rva), Some(size)) = (u32_at(self.b, ld), u32_at(self.b, ld + 4)) else { continue };
                let Some(data) = self.offset(rva).and_then(|o| self.b.get(o..o + size as usize)) else { continue };
                let (label, id) = match name {
                    Ok(n) => (n.to_string(), Some(n)),
                    Err(s) => (s, None),
                };
                out.push(Res { kind, name: label, id, data: data.to_vec() });
            }
        }
        out
    }
}

/// アイコンの組（GRPICONDIR）と、番号ごとのアイコンの画像から、.ico のファイルを組み立てる。
fn build_ico(group: &[u8], icon: impl Fn(u32) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let n = u16_at(group, 4)? as usize;
    let mut imgs = Vec::new();
    for i in 0..n {
        let e = 6 + i * 14;
        let fields = group.get(e..e + 12)?;
        let id = u16_at(group, e + 12)? as u32;
        if let Some(img) = icon(id) {
            imgs.push((fields.to_vec(), img));
        }
    }
    if imgs.is_empty() {
        return None;
    }
    let mut out = vec![0, 0, 1, 0];
    out.extend((imgs.len() as u16).to_le_bytes());
    let mut off = 6 + 16 * imgs.len();
    for (f, img) in &imgs {
        out.extend(&f[..8]); // 幅・高さ・色数・予約・面・ビット数
        out.extend((img.len() as u32).to_le_bytes());
        out.extend((off as u32).to_le_bytes());
        off += img.len();
    }
    for (_, img) in imgs {
        out.extend(img);
    }
    Some(out)
}

/// 版の情報（VS_VERSIONINFO）の「名前: 値」の行。
fn version_text(v: &[u8]) -> String {
    let mut lines = Vec::new();
    // 固定の部分（VS_FIXEDFILEINFO、印 0xFEEF04BD）からファイルの版
    if let Some(p) = v.windows(4).position(|w| w == [0xBD, 0x04, 0xEF, 0xFE]) {
        if let (Some(ms), Some(ls)) = (u32_at(v, p + 8), u32_at(v, p + 12)) {
            lines.push(format!("FileVersion (fixed): {}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF));
        }
    }
    fn walk(v: &[u8], at: usize, end: usize, depth: usize, lines: &mut Vec<String>) {
        let mut p = at;
        while p + 6 <= end && depth < 6 && lines.len() < 200 {
            let (Some(len), Some(vlen), Some(kind)) = (u16_at(v, p), u16_at(v, p + 2), u16_at(v, p + 4)) else { return };
            let len = len as usize;
            if len < 6 || p + len > end {
                return;
            }
            // 名前（UTF-16、0 で終わる）
            let mut q = p + 6;
            let mut key = Vec::new();
            while let Some(c) = u16_at(v, q) {
                q += 2;
                if c == 0 {
                    break;
                }
                key.push(c);
            }
            let key = String::from_utf16_lossy(&key);
            let value_at = (q + 3) & !3;
            let value_len = if kind == 1 { vlen as usize * 2 } else { vlen as usize };
            if kind == 1 && vlen > 0 && depth >= 3 {
                let units: Vec<u16> = (0..vlen as usize).filter_map(|k| u16_at(v, value_at + k * 2)).take_while(|c| *c != 0).collect();
                lines.push(format!("{key}: {}", String::from_utf16_lossy(&units)));
            }
            let kids = (value_at + value_len + 3) & !3;
            if kids < p + len {
                walk(v, kids, p + len, depth + 1, lines);
            }
            p = (p + len + 3) & !3;
        }
    }
    walk(v, 0, v.len(), 0, &mut lines);
    lines.join("\r\n")
}

fn files(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let pe = Pe::open(b)?;
    let res = pe.resources();
    let icon = |id: u32| res.iter().find(|r| r.kind == RT_ICON && r.id == Some(id)).map(|r| r.data.clone());
    let mut out = Vec::new();
    for r in res.iter().filter(|r| r.kind == RT_GROUP_ICON) {
        if let Some(ico) = build_ico(&r.data, icon) {
            let name: String = r.name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
            out.push((format!("icons/{name}.ico"), ico));
        }
    }
    if let Some(r) = res.iter().find(|r| r.kind == RT_VERSION) {
        let t = version_text(&r.data);
        if !t.is_empty() {
            out.push(("version.txt".into(), t.into_bytes()));
        }
    }
    if let Some(r) = res.iter().find(|r| r.kind == RT_MANIFEST) {
        out.push(("manifest.xml".into(), r.data.clone()));
    }
    Ok(out)
}

fn read_all(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| e.to_string())
}

pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    let mut v: Vec<Entry> = files(b)?.into_iter().map(|(n, d)| Entry { path: n, is_dir: false, size: d.len() as u64, packed: None }).collect();
    if v.iter().any(|e| e.path.starts_with("icons/")) {
        v.push(Entry { path: "icons/".into(), is_dir: true, size: 0, packed: None });
    }
    Ok(v)
}

pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    list_bytes(&read_all(p)?)
}

pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let (_, d) = files(&read_all(p)?)?.into_iter().find(|(n, _)| n == path).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
    if d.len() as u64 > max {
        return Err(crate::lang::tr("大きすぎます", "Too large").into());
    }
    Ok(d)
}

pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    for (n, d) in files(&read_all(p)?)?.into_iter().filter(|(_, d)| d.len() as u64 <= max_each) {
        if !f(&n, d) {
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

    #[test]
    fn builds_an_ico_from_a_group() {
        let mut group = vec![0, 0, 1, 0, 2, 0];
        for (side, id) in [(16u8, 1u16), (32, 2)] {
            group.extend([side, side, 0, 0, 1, 0, 32, 0]);
            group.extend(4u32.to_le_bytes());
            group.extend(id.to_le_bytes());
        }
        let ico = build_ico(&group, |id| (id == 2).then(|| b"\x89PNG".to_vec())).unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0], "見つかった画像だけ");
        assert_eq!(ico[6], 32);
        assert_eq!(u32_at(&ico, 18), Some(22));
        assert_eq!(&ico[22..], b"\x89PNG");
    }

    #[test]
    fn broken_input_does_not_panic() {
        assert!(files(b"").is_err());
        let mut b = b"MZ".to_vec();
        b.resize(64, 0);
        assert!(files(&b).is_err());
    }
}
