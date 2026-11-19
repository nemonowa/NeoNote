// Windows のショートカット（.lnk）の中身を読む（GUI なし）。右の欄の情報と「リンク先を開く」に使う。
// 形式: 76 バイトの見出し（印 0x4C）→ 印に応じて、場所の ID の並び → リンクの情報（ローカルのパス）→ 文字の項目
// （説明・相対パス・作業フォルダ・引数・アイコン）。後ろの追加の塊（PC の名前などが入る）は読まない。
use std::path::Path;

const HAS_ID_LIST: u32 = 0x01;
const HAS_LINK_INFO: u32 = 0x02;
const HAS_NAME: u32 = 0x04;
const HAS_RELATIVE_PATH: u32 = 0x08;
const HAS_WORKING_DIR: u32 = 0x10;
const HAS_ARGUMENTS: u32 = 0x20;
const HAS_ICON: u32 = 0x40;
const IS_UNICODE: u32 = 0x80;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub target: Option<String>,
    pub relative: Option<String>,
    pub working_dir: Option<String>,
    pub arguments: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn is_lnk_path(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

/// 0 で終わる、OS の文字コードの文字列（日本語の Windows なら Shift_JIS とみなす）。
fn ansi_z(b: &[u8], p: usize) -> Option<String> {
    let rest = b.get(p..)?;
    let raw = &rest[..rest.iter().position(|&c| c == 0)?];
    Some(super::encoding::Encoding::Windows31J.decode(raw).unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned()))
}

/// 0 で終わる UTF-16 の文字列。
fn utf16_z(b: &[u8], p: usize) -> Option<String> {
    let mut units = Vec::new();
    let mut q = p;
    loop {
        let c = u16_at(b, q)?;
        if c == 0 {
            break;
        }
        units.push(c);
        q += 2;
    }
    Some(String::from_utf16_lossy(&units))
}

pub fn parse(b: &[u8]) -> Result<Link, String> {
    let bad = || crate::lang::tr("ショートカットの形が読めません", "Cannot read the shortcut").to_string();
    if u32_at(b, 0) != Some(0x4C) {
        return Err(bad());
    }
    let flags = u32_at(b, 0x14).ok_or_else(bad)?;
    let mut p = 0x4C;
    if flags & HAS_ID_LIST != 0 {
        p += 2 + u16_at(b, p).ok_or_else(bad)? as usize;
    }
    let mut link = Link::default();
    if flags & HAS_LINK_INFO != 0 {
        let size = u32_at(b, p).ok_or_else(bad)? as usize;
        let header = u32_at(b, p + 4).ok_or_else(bad)? as usize;
        let info_flags = u32_at(b, p + 8).ok_or_else(bad)?;
        if info_flags & 1 != 0 {
            // ローカルのパス（Unicode の版があればそちら）と、後ろに付く部分
            let unicode = (header >= 0x24).then(|| u32_at(b, p + 28)).flatten().filter(|o| *o != 0).and_then(|o| utf16_z(b, p + o as usize));
            let base = unicode.or_else(|| u32_at(b, p + 16).and_then(|o| ansi_z(b, p + o as usize)));
            let suffix = (header >= 0x24).then(|| u32_at(b, p + 32)).flatten().filter(|o| *o != 0).and_then(|o| utf16_z(b, p + o as usize)).or_else(|| u32_at(b, p + 24).and_then(|o| ansi_z(b, p + o as usize))).unwrap_or_default();
            link.target = base.map(|s| s + &suffix).filter(|s| !s.is_empty());
        }
        p += size;
    }
    let unicode = flags & IS_UNICODE != 0;
    let mut read = |on: u32| -> Option<String> {
        if flags & on == 0 {
            return None;
        }
        let n = u16_at(b, p)? as usize;
        let s = if unicode {
            let units: Vec<u16> = (0..n).filter_map(|k| u16_at(b, p + 2 + k * 2)).collect();
            p += 2 + n * 2;
            String::from_utf16_lossy(&units)
        } else {
            let raw = b.get(p + 2..p + 2 + n)?;
            p += 2 + n;
            super::encoding::Encoding::Windows31J.decode(raw).unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned())
        };
        Some(s)
    };
    link.description = read(HAS_NAME);
    link.relative = read(HAS_RELATIVE_PATH);
    link.working_dir = read(HAS_WORKING_DIR);
    link.arguments = read(HAS_ARGUMENTS);
    link.icon = read(HAS_ICON);
    Ok(link)
}

/// リンク先。ローカルのパスが無ければ、ショートカットの場所からの相対パスで決める。
pub fn resolve_target(lnk: &Path, link: &Link) -> Option<std::path::PathBuf> {
    if let Some(t) = &link.target {
        return Some(t.into());
    }
    let rel = link.relative.as_deref()?;
    Some(lnk.parent()?.join(rel.replace('\\', std::path::MAIN_SEPARATOR_STR)))
}

/// 右の欄に出す行。
pub fn info_rows(link: &Link) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    let mut add = |k: &'static str, v: &Option<String>| {
        if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
            rows.push((k, v.to_string()));
        }
    };
    add(crate::lang::tr("リンク先", "Target"), &link.target);
    add(crate::lang::tr("相対パス", "Relative path"), &link.relative);
    add(crate::lang::tr("引数", "Arguments"), &link.arguments);
    add(crate::lang::tr("作業フォルダ", "Working folder"), &link.working_dir);
    add(crate::lang::tr("説明", "Description"), &link.description);
    add(crate::lang::tr("アイコン", "Icon"), &link.icon);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// リンクの情報（ANSI のパス）と、Unicode の文字の項目を持つショートカットを作る。
    fn sample() -> Vec<u8> {
        let mut b = vec![0u8; 0x4C];
        b[0] = 0x4C;
        let flags = HAS_LINK_INFO | HAS_NAME | HAS_WORKING_DIR | HAS_ARGUMENTS | IS_UNICODE;
        b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        // LinkInfo: 大きさ・見出しの大きさ 0x1C・印 1・VolumeID の位置・パスの位置・ネットワークの位置・後ろの部分の位置
        let path = b"C:\\Windows\\notepad.exe\0";
        let header = 0x1C;
        let size = header + path.len() + 1;
        let mut info = Vec::new();
        for v in [size as u32, header as u32, 1, 0, header as u32, 0, (header + path.len()) as u32] {
            info.extend(v.to_le_bytes());
        }
        info.extend(path);
        info.push(0);
        b.extend(info);
        for s in ["メモ帳", "C:\\Windows", "-x あ"] {
            let u: Vec<u16> = s.encode_utf16().collect();
            b.extend((u.len() as u16).to_le_bytes());
            b.extend(u.iter().flat_map(|c| c.to_le_bytes()));
        }
        b.extend([0u8; 4]);
        b
    }

    #[test]
    fn reads_target_and_strings() {
        let l = parse(&sample()).unwrap();
        assert_eq!(l.target.as_deref(), Some("C:\\Windows\\notepad.exe"));
        assert_eq!(l.description.as_deref(), Some("メモ帳"));
        assert_eq!(l.working_dir.as_deref(), Some("C:\\Windows"));
        assert_eq!(l.arguments.as_deref(), Some("-x あ"));
        assert_eq!(info_rows(&l).len(), 4);
    }

    #[test]
    fn relative_target_and_broken_input() {
        let l = Link { relative: Some("..\\a\\b.txt".into()), ..Default::default() };
        let t = resolve_target(Path::new("/x/y/s.lnk"), &l).unwrap();
        assert!(t.ends_with(format!("..{}a{}b.txt", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR)));
        let b = sample();
        for cut in 0..b.len() {
            let _ = parse(&b[..cut]);
        }
        assert!(parse(b"").is_err());
    }
}
