// tar / tar.gz / tgz / gz の中身の一覧（GUI なし）。読み取り専用: 名前・大きさを階層ツリーで見るだけで、
// 中身の取り出し・書き換え・名前変更はしない。依存の追加なし（gzip の本体は自前の `inflate`、名前の読みは encoding_rs）。
//
// 読むもの: ustar / GNU / PAX の tar（ファイルとフォルダ。長い名前は ustar の prefix・GNU の `L`・PAX の `path`、
//   大きなファイルは GNU の 2 進表記・PAX の `size`）。gzip は 1 つ目のメンバーだけ。展開した中身が tar なら tar として、
//   そうでなければ 1 ファイルとして出す（元の名前は gzip のヘッダー、無ければ拡張子を外した名前）。
// 読まないもの: リンク（シンボリック・ハード）・デバイス・FIFO（`list_dir` がシンボリックリンクを出さないのと同じ）、
//   bz2 / xz / zstd（デコーダーが重い）、複数ボリューム・スパース（`S`）の実際の大きさ、gzip の CRC の検査。
use crate::neo::archive::{Entry, MAX_ENTRIES};
use crate::neo::inflate::inflate;
use std::path::Path;

/// 読むファイルの大きさの上限（丸ごとメモリに載せるため）。
pub const MAX_FILE: u64 = 256 << 20;
/// gzip を展開した後の大きさの上限。
pub const MAX_OUT: usize = 256 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Tar,
    /// `.gz` / `.tgz`（`.tar.gz` の最後の拡張子も `gz`）。中身が tar かどうかは展開して決める。
    Gz,
}

/// 拡張子から形式を決める（大文字小文字を区別しない）。
pub fn kind_of(p: &Path) -> Option<Kind> {
    let ext = p.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "tar" => Some(Kind::Tar),
        "gz" | "tgz" => Some(Kind::Gz),
        _ => None,
    }
}

// ------------------------------------------------------------------ tar

/// NUL までの部分。
fn field(f: &[u8]) -> &[u8] {
    let n = f.iter().position(|&c| c == 0).unwrap_or(f.len());
    &f[..n]
}

/// 名前を文字にする。UTF-8 として正しければ UTF-8、だめなら Shift_JIS として読めれば Shift_JIS、さらにだめなら Latin-1。
/// （zip と違い `\` は `/` に直さない。tar では `\` も名前の一部）
fn decode(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    if let Some(s) = encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(raw) {
        return s.into_owned();
    }
    raw.iter().map(|&c| c as char).collect()
}

/// 8 進（前後の空白・NUL 可）または GNU の 2 進（先頭ビットが立っている）の数。空は 0。読めなければ None。
fn octal(f: &[u8]) -> Option<u64> {
    if f.first().is_some_and(|c| c & 0x80 != 0) {
        if f[0] == 0xff {
            return None; // 負の数
        }
        let mut v = (f[0] & 0x7f) as u64;
        for &c in &f[1..] {
            v = v.checked_mul(256)?.checked_add(c as u64)?;
        }
        return Some(v);
    }
    let digits = field(f);
    let t = std::str::from_utf8(digits).ok()?.trim();
    if t.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(t, 8).ok()
}

/// ヘッダー（512 バイト）の検査和が合っているか。符号なし・符号あり（古い tar）のどちらでもよい。
fn header_ok(h: &[u8]) -> bool {
    let Some(want) = octal(&h[148..156]) else { return false };
    let (mut unsigned, mut signed) = (0u64, 0i64);
    for (i, &c) in h.iter().enumerate() {
        let c = if (148..156).contains(&i) { b' ' } else { c };
        unsigned += c as u64;
        signed += (c as i8) as i64;
    }
    unsigned == want || signed == want as i64
}

/// PAX の拡張ヘッダー（`<長さ> <キー>=<値>\n` の並び）から、path と size を取る。
fn parse_pax(d: &[u8]) -> (Option<Vec<u8>>, Option<u64>) {
    let (mut path, mut size) = (None, None);
    let mut i = 0;
    while i < d.len() {
        let Some(sp) = d[i..].iter().position(|&c| c == b' ') else { break };
        let Some(len) = std::str::from_utf8(&d[i..i + sp]).ok().and_then(|s| s.parse::<usize>().ok()) else { break };
        if len < sp + 1 || i + len > d.len() {
            break;
        }
        let rec = &d[i + sp + 1..i + len];
        let rec = rec.strip_suffix(b"\n").unwrap_or(rec);
        if let Some(eq) = rec.iter().position(|&c| c == b'=') {
            let (k, v) = (&rec[..eq], &rec[eq + 1..]);
            if k == b"path" {
                path = Some(v.to_vec());
            } else if k == b"size" {
                size = std::str::from_utf8(v).ok().and_then(|s| s.trim().parse::<u64>().ok());
            }
        }
        i += len;
    }
    (path, size)
}

/// 先頭の `/` と `./` を外す。空・`.` は None（ルート自身）。戻り値は (パス, フォルダか)。フォルダは末尾が `/`。
fn clean(name: &str, dir: bool) -> Option<(String, bool)> {
    let mut s = name;
    loop {
        let t = s.trim_start_matches('/');
        match t.strip_prefix("./") {
            Some(r) => s = r,
            None => {
                s = t;
                break;
            }
        }
    }
    if s.is_empty() || s == "." {
        return None;
    }
    let is_dir = dir || s.ends_with('/');
    let mut path = s.to_string();
    if is_dir && !path.ends_with('/') {
        path.push('/');
    }
    Some((path, is_dir))
}

/// tar（ファイル全体のバイト列）の項目を一覧にする。
pub fn list_tar(b: &[u8]) -> Result<Vec<Entry>, String> {
    let cut: &str = crate::lang::tr("tar が途中で切れています", "tar is truncated");
    if b.is_empty() {
        return Err(crate::lang::tr("空のファイルです", "Empty file").into());
    }
    let mut out: Vec<Entry> = Vec::new();
    let mut p = 0usize;
    let mut long_name: Option<Vec<u8>> = None; // GNU の `L`
    let mut pax_path: Option<Vec<u8>> = None;
    let mut pax_size: Option<u64> = None;
    let mut seen = false;
    while p + 512 <= b.len() {
        let h = &b[p..p + 512];
        if h.iter().all(|&c| c == 0) {
            break; // 終端
        }
        if !header_ok(h) {
            let msg = if seen { crate::lang::tr("tar が壊れています（ヘッダーの検査が合いません）", "Broken tar (header checksum mismatch)") } else { crate::lang::tr("tar ではありません（ヘッダーの検査が合いません）", "Not a tar file (header checksum mismatch)") };
            return Err(msg.into());
        }
        seen = true;
        let typ = h[156];
        let is_file = matches!(typ, 0 | b'0' | b'7');
        let mut size = octal(&h[124..136]).ok_or(crate::lang::tr("tar が壊れています（大きさを読めません）", "Broken tar (cannot read the size)"))?;
        if is_file {
            if let Some(s) = pax_size.take() {
                size = s;
            }
        }
        // リンク・フォルダ・デバイス・FIFO は本文を持たない（大きさ欄は当てにしない）
        let data_len = if matches!(typ, b'1'..=b'6') { 0 } else { size };
        let data_start = p + 512;
        let end = (data_start as u64).checked_add(data_len).ok_or(cut)?;
        if end > b.len() as u64 {
            return Err(cut.into());
        }
        let data = &b[data_start..end as usize];
        match typ {
            b'L' => {
                if data.len() <= 64 << 10 {
                    long_name = Some(field(data).to_vec());
                }
            }
            b'x' => {
                if data.len() <= 1 << 20 {
                    let (pp, ps) = parse_pax(data);
                    if pp.is_some() {
                        pax_path = pp;
                    }
                    if ps.is_some() {
                        pax_size = ps;
                    }
                }
            }
            b'g' | b'K' => {}
            _ if is_file || typ == b'5' => {
                let mut raw = field(&h[0..100]).to_vec();
                if &h[257..263] == b"ustar\0" {
                    let pre = field(&h[345..500]);
                    if !pre.is_empty() {
                        let mut v = pre.to_vec();
                        v.push(b'/');
                        v.extend_from_slice(&raw);
                        raw = v;
                    }
                }
                if let Some(l) = long_name.take() {
                    raw = l;
                }
                if let Some(pp) = pax_path.take() {
                    raw = pp;
                }
                if let Some((path, is_dir)) = clean(&decode(&raw), typ == b'5') {
                    out.push(Entry { path, is_dir, size: if is_dir { 0 } else { size }, packed: None });
                    if out.len() > MAX_ENTRIES {
                        return Err(if crate::lang::is_en() { format!("Too many entries (up to {MAX_ENTRIES})") } else { format!("項目が多すぎます（{MAX_ENTRIES} 件まで）") });
                    }
                }
            }
            _ => {} // リンク・デバイス・FIFO などは出さない
        }
        // 長い名前・PAX の値は、直後の 1 項目だけに効く
        if !matches!(typ, b'L' | b'K' | b'x' | b'g') {
            long_name = None;
            pax_path = None;
            pax_size = None;
        }
        p = data_start + (data_len as usize).div_ceil(512) * 512;
    }
    Ok(out)
}

// ------------------------------------------------------------------ gzip

/// gzip の先頭（10 バイト + 任意項目）を読み、(圧縮データの開始位置, ヘッダーにある元のファイル名) を返す。
fn gz_header(b: &[u8]) -> Result<(usize, Option<String>), String> {
    let bad: &str = crate::lang::tr("gzip が壊れています（ヘッダーが途中で切れています）", "Broken gzip (header is truncated)");
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b {
        return Err(crate::lang::tr("gzip ではありません", "Not a gzip file").into());
    }
    if b[2] != 8 {
        return Err(crate::lang::tr("gzip の圧縮方式に対応していません", "gzip compression method is not supported").into());
    }
    let flg = b[3];
    let mut p = 10usize;
    if flg & 4 != 0 {
        if p + 2 > b.len() {
            return Err(bad.into());
        }
        p += 2 + u16::from_le_bytes([b[p], b[p + 1]]) as usize;
    }
    let mut name = None;
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            let rest = b.get(p..).ok_or(bad)?;
            let n = rest.iter().position(|&c| c == 0).ok_or(bad)?;
            if bit == 8 {
                name = Some(decode(&rest[..n]));
            }
            p += n + 1;
        }
    }
    if flg & 2 != 0 {
        p += 2;
    }
    if p + 8 > b.len() {
        return Err(bad.into());
    }
    Ok((p, name))
}

/// gzip（ファイル全体）を展開して一覧にする。`fallback` は、ヘッダーに名前が無い 1 ファイルのときの名前。
pub fn list_gz(b: &[u8], fallback: &str) -> Result<Vec<Entry>, String> {
    let (p, name) = gz_header(b)?;
    let out = inflate(&b[p..b.len() - 8], MAX_OUT).map_err(|e| if crate::lang::is_en() { format!("Cannot expand the gzip: {e}") } else { format!("gzip を展開できません: {e}") })?;
    if out.len() >= 512 && header_ok(&out[..512]) {
        return list_tar(&out); // tar.gz。途中で壊れていればそのエラーを返す
    }
    let name = name
        .and_then(|n| n.rsplit('/').next().map(str::to_string))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback.to_string());
    Ok(vec![Entry { path: name, is_dir: false, size: out.len() as u64, packed: Some(b.len() as u64) }])
}

/// ファイルを読んで一覧にする。大きすぎる・読めない・その形式でないときは Err。
pub fn list_file(path: &Path, kind: Kind) -> Result<Vec<Entry>, String> {
    let ctx = |e: std::io::Error| format!("{}\n{e}", path.display());
    let len = std::fs::metadata(path).map_err(ctx)?.len();
    if len > MAX_FILE {
        return Err(if crate::lang::is_en() { format!("Too large (up to {} MiB)", MAX_FILE >> 20) } else { format!("大きすぎます（{} MiB まで）", MAX_FILE >> 20) });
    }
    let b = std::fs::read(path).map_err(ctx)?;
    match kind {
        Kind::Tar => list_tar(&b),
        Kind::Gz => {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            list_gz(&b, &stem)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(h: &mut [u8]) {
        h[148..156].fill(b' ');
        let sum: u32 = h.iter().map(|&c| c as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    }

    fn hdr(name: &str, size: u64, typ: u8) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[156] = typ;
        fix(&mut h);
        h
    }

    /// 本文を 512 バイトの倍数に埋める。
    fn blk(data: &[u8]) -> Vec<u8> {
        let mut v = data.to_vec();
        v.resize(data.len().div_ceil(512) * 512, 0);
        v
    }

    fn tar(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut t: Vec<u8> = parts.concat();
        t.extend(vec![0u8; 1024]);
        t
    }

    fn file(name: &str, data: &[u8]) -> Vec<u8> {
        [hdr(name, data.len() as u64, b'0'), blk(data)].concat()
    }

    fn e(path: &str, is_dir: bool, size: u64) -> Entry {
        Entry { path: path.into(), is_dir, size, packed: None }
    }

    /// 無圧縮ブロックだけの gzip（CRC は検査しないので 0）。
    fn gz(name: Option<&str>, data: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1f, 0x8b, 8, if name.is_some() { 8 } else { 0 }, 0, 0, 0, 0, 0, 3];
        if let Some(n) = name {
            v.extend_from_slice(n.as_bytes());
            v.push(0);
        }
        v.push(1); // BFINAL=1, 無圧縮
        v.extend_from_slice(&(data.len() as u16).to_le_bytes());
        v.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
        v.extend_from_slice(data);
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v
    }

    #[test]
    fn kind_follows_the_extension() {
        for (p, k) in [("a.tar", Some(Kind::Tar)), ("A.TAR", Some(Kind::Tar)), ("a.tar.gz", Some(Kind::Gz)), ("a.TGZ", Some(Kind::Gz)), ("a.gz", Some(Kind::Gz)), ("a.zip", None), ("a.txt", None), ("tar", None)] {
            assert_eq!(kind_of(Path::new(p)), k, "{p}");
        }
    }

    #[test]
    fn octal_and_binary_sizes() {
        assert_eq!(octal(b"00000000017\0"), Some(15));
        assert_eq!(octal(b"       17 "), Some(15));
        assert_eq!(octal(b"\0\0\0"), Some(0));
        assert_eq!(octal(b"8"), None);
        let mut f = [0u8; 12];
        f[0] = 0x80;
        f[10] = 1; // 0x0100
        assert_eq!(octal(&f), Some(256));
        f[0] = 0xff;
        assert_eq!(octal(&f), None);
    }

    #[test]
    fn lists_files_and_folders() {
        let t = tar(&[hdr("d/", 0, b'5'), file("d/a.txt", b"hello"), file("b.txt", b"")]);
        assert_eq!(list_tar(&t).unwrap(), vec![e("d/", true, 0), e("d/a.txt", false, 5), e("b.txt", false, 0)]);
    }

    #[test]
    fn leading_dot_slash_and_the_root_are_dropped() {
        let t = tar(&[hdr("./", 0, b'5'), file("./x", b"1"), hdr("./sub", 0, b'5')]);
        assert_eq!(list_tar(&t).unwrap(), vec![e("x", false, 1), e("sub/", true, 0)]);
    }

    #[test]
    fn ustar_prefix_is_joined() {
        let mut h = hdr("f", 0, b'0');
        h[257..263].copy_from_slice(b"ustar\0");
        h[345..352].copy_from_slice(b"pre/fix");
        fix(&mut h);
        assert_eq!(list_tar(&tar(&[h])).unwrap(), vec![e("pre/fix/f", false, 0)]);
    }

    #[test]
    fn gnu_long_names_apply_to_the_next_entry_only() {
        let long = "a/very/long/".repeat(12) + "name.txt";
        let mut data = long.as_bytes().to_vec();
        data.push(0);
        let t = tar(&[hdr("././@LongLink", data.len() as u64, b'L'), blk(&data), file("short", b"xy"), file("plain", b"")]);
        assert_eq!(list_tar(&t).unwrap(), vec![e(&long, false, 2), e("plain", false, 0)]);
    }

    #[test]
    fn pax_path_and_size_apply_to_the_next_entry() {
        let rec = " path=very/long/name.txt\n";
        let one = |body: &str| -> String {
            let base = body.len();
            let mut n = base + 1;
            for d in 1..=4 {
                if (base + d).to_string().len() == d {
                    n = base + d;
                }
            }
            format!("{n}{body}")
        };
        let mut data = one(rec).into_bytes();
        data.extend_from_slice(one(" size=7\n").as_bytes());
        let t = tar(&[hdr("PaxHeader", data.len() as u64, b'x'), blk(&data), hdr("short", 0, b'0'), blk(b"1234567"), file("next", b"")]);
        assert_eq!(list_tar(&t).unwrap(), vec![e("very/long/name.txt", false, 7), e("next", false, 0)]);
    }

    #[test]
    fn links_and_devices_are_not_listed() {
        let t = tar(&[hdr("link", 0, b'2'), hdr("hard", 0, b'1'), hdr("dev", 0, b'3'), file("real", b"x")]);
        assert_eq!(list_tar(&t).unwrap(), vec![e("real", false, 1)]);
    }

    #[test]
    fn bad_and_cut_archives_are_errors() {
        assert!(list_tar(b"").is_err());
        let text = vec![b'a'; 1024];
        assert!(list_tar(&text).unwrap_err().contains("tar ではありません"));
        let mut t = tar(&[file("a", b"hello")]);
        t.truncate(512 + 3); // 本文の途中で切れる
        assert!(list_tar(&t).unwrap_err().contains("途中で切れて"));
        // 2 つ目のヘッダーが壊れている
        let mut t = tar(&[file("a", b"x"), file("b", b"y")]);
        t[1024 + 5] ^= 0xff;
        assert!(list_tar(&t).unwrap_err().contains("壊れて"));
        // 全部 0 は空の tar
        assert_eq!(list_tar(&vec![0u8; 1024]).unwrap(), vec![]);
    }

    #[test]
    fn names_that_are_not_utf8_fall_back_to_shift_jis() {
        let mut h = hdr("x", 0, b'0');
        h[..4].copy_from_slice(&[0x83, 0x65, 0x83, 0x58]); // テス（Shift_JIS）
        fix(&mut h);
        assert_eq!(list_tar(&tar(&[h])).unwrap(), vec![e("テス", false, 0)]);
    }

    #[test]
    fn gzip_of_a_tar_lists_the_tar() {
        let t = tar(&[file("a.txt", b"hello")]);
        assert_eq!(list_gz(&gz(None, &t), "x").unwrap(), vec![e("a.txt", false, 5)]);
    }

    #[test]
    fn gzip_of_a_plain_file_is_one_entry() {
        let g = gz(Some("dir/log.txt"), b"hello world");
        let r = list_gz(&g, "fallback").unwrap();
        assert_eq!(r, vec![Entry { path: "log.txt".into(), is_dir: false, size: 11, packed: Some(g.len() as u64) }]);
        assert_eq!(list_gz(&gz(None, b"abc"), "fallback").unwrap()[0].path, "fallback");
    }

    #[test]
    fn broken_gzip_is_an_error() {
        assert!(list_gz(b"not gzip at all, really not", "x").unwrap_err().contains("gzip ではありません"));
        let mut g = gz(None, b"abc");
        g[2] = 7;
        assert!(list_gz(&g, "x").unwrap_err().contains("対応していません"));
        assert!(list_gz(&gz(None, b"abc")[..12], "x").is_err());
    }

    #[test]
    fn lists_from_a_file_on_disk() {
        let dir = std::env::temp_dir().join(format!("neonote_tarlist_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = tar(&[file("a.txt", b"hi")]);
        let (p1, p2, p3) = (dir.join("a.tar"), dir.join("a.tar.gz"), dir.join("b.txt.gz"));
        std::fs::write(&p1, &t).unwrap();
        std::fs::write(&p2, gz(None, &t)).unwrap();
        std::fs::write(&p3, gz(None, b"plain")).unwrap();
        assert_eq!(list_file(&p1, Kind::Tar).unwrap(), vec![e("a.txt", false, 2)]);
        assert_eq!(list_file(&p2, Kind::Gz).unwrap(), vec![e("a.txt", false, 2)]);
        assert_eq!(list_file(&p3, Kind::Gz).unwrap()[0].path, "b.txt"); // 拡張子を外した名前
        assert!(list_file(&dir.join("none.tar"), Kind::Tar).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
