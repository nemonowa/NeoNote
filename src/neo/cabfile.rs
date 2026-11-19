// Windows の cab（キャビネット。先頭が `MSCF`）を、一覧として読み取り専用で開く（GUI なし）。
// 中身は「フォルダ」（圧縮の単位）ごとに、データのブロックが続く。ファイルは、展開したフォルダの中の位置と長さで決まる。
// 圧縮は「なし」と MSZIP（ブロックごとに `CK`＋deflate。前のブロックの続きとして解く）だけを読む。
// LZX・Quantum のフォルダは、一覧には出すが中身は読めない（理由を返す）。複数の cab にまたがるものも読めない。
use super::archive::Entry;
use std::path::Path;

/// 展開したフォルダの大きさの上限。
const MAX_FOLDER: usize = 512 << 20;

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn is_cab_path(p: &Path) -> bool {
    use std::io::Read;
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cab")) || p.is_dir() {
        return false;
    }
    let mut head = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"MSCF"
}

struct Folder {
    first_data: usize,
    blocks: usize,
    compress: u16,
}

struct File {
    name: String,
    size: usize,
    offset: usize,
    folder: usize,
}

struct Cab {
    folders: Vec<Folder>,
    files: Vec<File>,
    data_reserve: usize,
}

fn bad<T>() -> Result<T, String> {
    Err(crate::lang::tr("cab の形が読めません", "Cannot read the cab").into())
}

fn parse(b: &[u8]) -> Result<Cab, String> {
    if b.get(..4) != Some(b"MSCF") {
        return bad();
    }
    let (Some(files_at), Some(nfolders), Some(nfiles), Some(flags)) = (u32_at(b, 16), u16_at(b, 26), u16_at(b, 28), u16_at(b, 30)) else { return bad() };
    let mut p = 36;
    let (mut folder_reserve, mut data_reserve) = (0, 0);
    if flags & 0x0001 != 0 || flags & 0x0002 != 0 {
        return Err(crate::lang::tr("複数の cab にまたがるものは読めません", "Multi-part cabs are not supported").into());
    }
    if flags & 0x0004 != 0 {
        let (Some(hr), Some(fr), Some(dr)) = (u16_at(b, 36), b.get(38), b.get(39)) else { return bad() };
        folder_reserve = *fr as usize;
        data_reserve = *dr as usize;
        p = 40 + hr as usize;
    }
    let mut folders = Vec::new();
    for _ in 0..nfolders {
        let (Some(first), Some(blocks), Some(compress)) = (u32_at(b, p), u16_at(b, p + 4), u16_at(b, p + 6)) else { return bad() };
        folders.push(Folder { first_data: first as usize, blocks: blocks as usize, compress });
        p += 8 + folder_reserve;
    }
    let mut files = Vec::new();
    let mut q = files_at as usize;
    for _ in 0..nfiles {
        let (Some(size), Some(offset), Some(folder), Some(attr)) = (u32_at(b, q), u32_at(b, q + 4), u16_at(b, q + 8), u16_at(b, q + 14)) else { return bad() };
        let name_at = q + 16;
        let Some(len) = b.get(name_at..).and_then(|r| r.iter().position(|&c| c == 0)) else { return bad() };
        let raw = &b[name_at..name_at + len];
        // 0x80 の印があれば UTF-8、無ければ OS の文字コード（日本語の Windows なら Shift_JIS とみなす）
        let name = if attr & 0x80 != 0 { String::from_utf8_lossy(raw).into_owned() } else { super::encoding::Encoding::Windows31J.decode(raw).unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned()) };
        files.push(File { name: name.replace('\\', "/"), size: size as usize, offset: offset as usize, folder: folder as usize });
        q = name_at + len + 1;
    }
    Ok(Cab { folders, files, data_reserve })
}

/// フォルダを展開する（`need` バイトあれば、そこで止める）。
fn expand(b: &[u8], cab: &Cab, fi: usize, need: usize) -> Result<Vec<u8>, String> {
    let f = cab.folders.get(fi).ok_or_else(|| crate::lang::tr("フォルダがありません", "Missing folder").to_string())?;
    let kind = f.compress & 0x000F;
    if kind != 0 && kind != 1 {
        let name = if kind == 3 { "LZX" } else { "Quantum" };
        return Err(if crate::lang::is_en() { format!("{name} compression is not supported") } else { format!("{name} の圧縮は読めません") });
    }
    let mut out: Vec<u8> = Vec::new();
    let mut p = f.first_data;
    for _ in 0..f.blocks {
        let (Some(clen), Some(ulen)) = (u16_at(b, p + 4), u16_at(b, p + 6)) else { return bad() };
        let at = p + 8 + cab.data_reserve;
        let Some(data) = b.get(at..at + clen as usize) else { return bad() };
        if kind == 0 {
            out.extend_from_slice(data);
        } else {
            if data.get(..2) != Some(b"CK") {
                return bad();
            }
            let part = super::inflate::inflate_with_dict(&data[2..], &out, ulen as usize)?;
            out.extend_from_slice(&part);
        }
        if out.len() > MAX_FOLDER {
            return Err(crate::lang::tr("展開すると大きすぎます", "Too large when expanded").into());
        }
        if out.len() >= need {
            break;
        }
        p = at + clen as usize;
    }
    Ok(out)
}

fn read_all(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| e.to_string())
}

pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    let cab = parse(b)?;
    Ok(cab.files.iter().map(|f| Entry { path: f.name.clone(), is_dir: false, size: f.size as u64, packed: None }).collect())
}

pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    list_bytes(&read_all(p)?)
}

fn read_bytes(b: &[u8], path: &str, max: u64) -> Result<Vec<u8>, String> {
    let cab = parse(b)?;
    let f = cab.files.iter().find(|f| f.name == path).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
    if f.size as u64 > max {
        return Err(crate::lang::tr("大きすぎます", "Too large").into());
    }
    let data = expand(b, &cab, f.folder, f.offset + f.size)?;
    data.get(f.offset..f.offset + f.size).map(|s| s.to_vec()).ok_or_else(|| crate::lang::tr("cab が途中で切れています", "The cab is truncated").to_string())
}

pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    read_bytes(&read_all(p)?, path, max)
}

pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    let b = read_all(p)?;
    let cab = parse(&b)?;
    // フォルダごとに 1 度だけ展開する
    for fi in 0..cab.folders.len() {
        let Ok(data) = expand(&b, &cab, fi, usize::MAX) else { continue };
        for file in cab.files.iter().filter(|x| x.folder == fi && x.size as u64 <= max_each) {
            if let Some(d) = data.get(file.offset..file.offset + file.size) {
                if !f(&file.name, d.to_vec()) {
                    return Ok(());
                }
            }
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

    /// Windows の makecab で作った実物（MSZIP・ファイル 3 つ・1 つは 100 KiB で複数のブロックにまたがる）。
    fn sample() -> Vec<u8> {
        crate::neo::deflate::gunzip(include_bytes!("testdata/sample.cab.gz"), 1 << 20).unwrap()
    }

    #[test]
    fn reads_a_real_cab() {
        let b = sample();
        let mut names: Vec<String> = list_bytes(&b).unwrap().into_iter().map(|e| e.path).collect();
        names.sort();
        assert_eq!(names, ["a.txt", "big.txt", "日本語.txt"]);
        assert_eq!(read_bytes(&b, "a.txt", 1 << 20).unwrap(), b"hello cab\r\n");
        assert_eq!(String::from_utf8(read_bytes(&b, "日本語.txt", 1 << 20).unwrap()).unwrap(), "日本語のテキスト\r\n");
        let big = read_bytes(&b, "big.txt", 1 << 20).unwrap();
        assert_eq!(big.len(), 100_000);
        assert!(big.iter().enumerate().all(|(i, &c)| c == b"0123456789"[i % 10]));
        for cut in (0..b.len()).step_by(101) {
            let _ = list_bytes(&b[..cut]);
            let _ = read_bytes(&b[..cut], "big.txt", 1 << 20);
        }
    }
}
