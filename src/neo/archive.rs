// フォルダと zip の中身を一覧にし、パスの名前変更を適用する（GUI なし）。tar / tar.gz / gz の一覧は `tarlist`（読み取り専用）。
// 依存は encoding_rs（Shift_JIS の名前を読むため。すでに本体が使っている）だけ。zip は目録だけを自前で読み書きする。
//
// 名前変更の考え方: 「パス一覧」を 1 行 1 パスの文字にして編集し、同じ行数で戻す（`to_edit_text` → `parse_edit_text`）。
// 行を足す・消すことはできない（名前の変更だけ）。zip は中身（圧縮データ）に一切触れず、名前と目録だけを書き換える。
use std::collections::HashSet;
use crate::neo::tarlist;
use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 50_000;
pub const MAX_ZIP: u64 = 1 << 30; // 1 GiB。丸ごとメモリに載せるので上限を置く

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `/` 区切りの相対パス。フォルダは末尾が `/`。
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// zip の圧縮後サイズ（フォルダ・zip 以外は None）。
    pub packed: Option<u64>,
}

pub fn is_zip_path(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().eq_ignore_ascii_case("zip")).unwrap_or(false)
}

/// 一覧（階層ツリーの Preview）で開く対象か。フォルダ・zip・tar・tar.gz・tgz・gz。
/// 拡張子だけで決めるので、中身が違えば（一覧を読めなければ）ふつうのファイルとして開き直される。
pub fn is_archive_path(p: &Path) -> bool {
    p.is_dir() || is_zip_path(p) || tarlist::kind_of(p).is_some()
}

/// 名前の変更（`Rename (text)`）ができる対象か。フォルダと zip だけ（tar 系は一覧を見るだけ）。
pub fn can_rename(p: &Path) -> bool {
    p.is_dir() || is_zip_path(p)
}

// ------------------------------------------------------------------ フォルダ

/// フォルダの中を再帰で一覧にする。シンボリックリンクはたどらず、一覧にも出さない。
pub fn list_dir(root: &Path) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}\n{e}", dir.display()))?;
        for ent in rd {
            let ent = ent.map_err(|e| e.to_string())?;
            let ft = ent.file_type().map_err(|e| e.to_string())?;
            if ft.is_symlink() {
                continue;
            }
            let name = ent
                .file_name()
                .into_string()
                .map_err(|_| if crate::lang::is_en() { format!("Name is not UTF-8: {}", ent.path().display()) } else { format!("UTF-8 でない名前があります: {}", ent.path().display()) })?;
            if out.len() >= MAX_ENTRIES {
                return Err(if crate::lang::is_en() { format!("Too many entries (up to {MAX_ENTRIES})") } else { format!("項目が多すぎます（{MAX_ENTRIES} 件まで）") });
            }
            if ft.is_dir() {
                let rel = format!("{prefix}{name}/");
                out.push(Entry { path: rel.clone(), is_dir: true, size: 0, packed: None });
                stack.push((ent.path(), rel));
            } else if ft.is_file() {
                let size = ent.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(Entry { path: format!("{prefix}{name}"), is_dir: false, size, packed: None });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

// ------------------------------------------------------------------ zip の読み取り

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

struct Eocd {
    pos: usize,
    count: usize,
    cd_size: usize,
    cd_off: usize,
    comment: Vec<u8>,
}

struct Cd {
    /// 目録の 1 件分（固定部 46 バイト + 名前 + extra + コメント）が bytes のどこにあるか。
    start: usize,
    name_len: usize,
    extra_len: usize,
    comment_len: usize,
    flags: u16,
    lh_off: usize,
    csize: u64,
    usize_: u64,
}

/// `b` はファイルの全体、または末尾の一部。`base` は b の先頭がファイルの何バイト目か。
fn find_eocd(b: &[u8], base: usize) -> Result<Eocd, String> {
    if b.len() < 22 {
        return Err(crate::lang::tr("zip ではありません", "Not a zip file").into());
    }
    let lowest = b.len().saturating_sub(22 + 65_535);
    let mut i = b.len() - 22;
    loop {
        if u32le(b, i) == 0x0605_4b50 && i + 22 + u16le(b, i + 20) as usize == b.len() {
            let (disk, cd_disk) = (u16le(b, i + 4), u16le(b, i + 6));
            let (n_here, n_all) = (u16le(b, i + 8), u16le(b, i + 10));
            let (cd_size, cd_off) = (u32le(b, i + 12), u32le(b, i + 16));
            if n_all == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF || (i >= 20 && u32le(b, i - 20) == 0x0706_4b50) {
                return Err(crate::lang::tr("ZIP64 の zip には対応していません", "ZIP64 zip files are not supported").into());
            }
            if disk != 0 || cd_disk != 0 || n_here != n_all {
                return Err(crate::lang::tr("分割された zip には対応していません", "Split zip files are not supported").into());
            }
            return Ok(Eocd {
                pos: base + i,
                count: n_all as usize,
                cd_size: cd_size as usize,
                cd_off: cd_off as usize,
                comment: b[i + 22..].to_vec(),
            });
        }
        if i == lowest {
            return Err(crate::lang::tr("zip ではありません（目録の終端が見つかりません）", "Not a zip file (end of central directory not found)").into());
        }
        i -= 1;
    }
}

/// 目録の範囲（ファイル全体から切り出す）。
fn cd_slice<'a>(b: &'a [u8], e: &Eocd) -> Result<&'a [u8], String> {
    match e.cd_off.checked_add(e.cd_size) {
        Some(end) if end <= e.pos && end <= b.len() => Ok(&b[e.cd_off..end]),
        _ => Err(crate::lang::tr("zip が壊れています（目録の位置が不正です）", "Broken zip (invalid central directory position)").into()),
    }
}

/// 目録（`cd_slice` の結果）を読む。`Cd::start` は目録の先頭からの位置。
fn read_cd(b: &[u8], e: &Eocd) -> Result<Vec<Cd>, String> {
    let mut v = Vec::with_capacity(e.count.min(MAX_ENTRIES));
    let mut p = 0;
    for _ in 0..e.count {
        if p + 46 > b.len() || u32le(b, p) != 0x0201_4b50 {
            return Err(crate::lang::tr("zip が壊れています（目録が途中で切れています）", "Broken zip (central directory is truncated)").into());
        }
        let flags = u16le(b, p + 8);
        if flags & 0x2000 != 0 {
            return Err(crate::lang::tr("目録が暗号化された zip には対応していません", "Zip files with an encrypted central directory are not supported").into());
        }
        let (csize, usize_, lho) = (u32le(b, p + 20), u32le(b, p + 24), u32le(b, p + 42));
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF || lho == 0xFFFF_FFFF {
            return Err(crate::lang::tr("ZIP64 の zip には対応していません", "ZIP64 zip files are not supported").into());
        }
        let (n, x, c) = (u16le(b, p + 28) as usize, u16le(b, p + 30) as usize, u16le(b, p + 32) as usize);
        if p + 46 + n + x + c > b.len() {
            return Err(crate::lang::tr("zip が壊れています（目録の項目がはみ出しています）", "Broken zip (central directory entry overruns)").into());
        }
        v.push(Cd { start: p, name_len: n, extra_len: x, comment_len: c, flags, lh_off: lho as usize, csize: csize as u64, usize_: usize_ as u64 });
        if v.len() > MAX_ENTRIES {
            return Err(if crate::lang::is_en() { format!("Too many entries (up to {MAX_ENTRIES})") } else { format!("項目が多すぎます（{MAX_ENTRIES} 件まで）") });
        }
        p += 46 + n + x + c;
    }
    Ok(v)
}

/// zip の名前を文字にする。UTF-8 フラグ付き・UTF-8 として正しいものは UTF-8、
/// そうでなければ Shift_JIS として読めれば Shift_JIS、だめなら Latin-1。`\` は `/` にそろえる。
fn decode_name(raw: &[u8], flags: u16) -> String {
    let s = if flags & 0x800 != 0 {
        String::from_utf8_lossy(raw).into_owned()
    } else if let Ok(s) = std::str::from_utf8(raw) {
        s.to_string()
    } else if let Some(s) = encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(raw) {
        s.into_owned()
    } else {
        raw.iter().map(|&c| c as char).collect()
    };
    s.replace('\\', "/")
}

fn entries_of(b: &[u8], cds: &[Cd]) -> Vec<Entry> {
    cds.iter()
        .map(|c| {
            let path = decode_name(&b[c.start + 46..c.start + 46 + c.name_len], c.flags);
            Entry { is_dir: path.ends_with('/'), path, size: c.usize_, packed: Some(c.csize) }
        })
        .collect()
}

/// zip の目録を読む（中身は展開しない）。テストが使う。実際の読み込みは `list_zip_file`。
#[cfg(test)]
pub fn list_zip(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let e = find_eocd(bytes, 0)?;
    let cd = cd_slice(bytes, &e)?;
    let cds = read_cd(cd, &e)?;
    Ok(entries_of(cd, &cds))
}

/// ファイルの末尾と目録だけを読んで一覧にする（大きな zip でも全体は読まない）。
pub fn list_zip_file(path: &Path) -> Result<Vec<Entry>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let ctx = |e: std::io::Error| format!("{}\n{e}", path.display());
    let mut f = std::fs::File::open(path).map_err(ctx)?;
    let len = f.metadata().map_err(ctx)?.len();
    let tail_len = len.min(22 + 65_535);
    let mut tail = vec![0u8; tail_len as usize];
    f.seek(SeekFrom::Start(len - tail_len)).map_err(ctx)?;
    f.read_exact(&mut tail).map_err(ctx)?;
    let e = find_eocd(&tail, (len - tail_len) as usize)?;
    if e.cd_off as u64 + e.cd_size as u64 > e.pos as u64 || e.cd_size > 64 << 20 {
        return Err(crate::lang::tr("zip が壊れています（目録の位置が不正です）", "Broken zip (invalid central directory position)").into());
    }
    let mut cd = vec![0u8; e.cd_size];
    f.seek(SeekFrom::Start(e.cd_off as u64)).map_err(ctx)?;
    f.read_exact(&mut cd).map_err(ctx)?;
    let cds = read_cd(&cd, &e)?;
    Ok(entries_of(&cd, &cds))
}

/// フォルダなら中を、tar 系ならその一覧を、そうでなければ zip の目録を一覧にする。
pub fn list_path(p: &Path) -> Result<Vec<Entry>, String> {
    if p.is_dir() {
        list_dir(p)
    } else if let Some(k) = tarlist::kind_of(p) {
        tarlist::list_file(p, k)
    } else {
        list_zip_file(p)
    }
}

/// zip（ファイル全体のバイト列）から、名前が `name` の項目 1 つの中身を取り出す。`max` は展開後の上限（超えたらエラー）。
/// 無圧縮（0）と deflate（8）だけ読む。暗号化・ZIP64・その他の圧縮方式はエラー。名前は `/` 区切り・大文字小文字を区別して比べる。
pub fn read_entry(bytes: &[u8], name: &str, max: usize) -> Result<Vec<u8>, String> {
    let e = find_eocd(bytes, 0)?;
    let cd = cd_slice(bytes, &e)?;
    let cds = read_cd(cd, &e)?;
    let c = cds
        .iter()
        .find(|c| decode_name(&cd[c.start + 46..c.start + 46 + c.name_len], c.flags) == name)
        .ok_or_else(|| if crate::lang::is_en() { format!("No {name} in the zip") } else { format!("zip の中に {name} がありません") })?;
    if c.flags & 1 != 0 {
        return Err(crate::lang::tr("暗号化された zip には対応していません", "Encrypted zip files are not supported").into());
    }
    if c.usize_ > max as u64 {
        return Err(if crate::lang::is_en() { format!("{name} is too large") } else { format!("{name} が大きすぎます") });
    }
    let method = u16le(cd, c.start + 10);
    let lh = c.lh_off;
    if lh.saturating_add(30) > bytes.len() || u32le(bytes, lh) != 0x0403_4b50 {
        return Err(crate::lang::tr("zip が壊れています（項目の先頭が不正です）", "Broken zip (invalid entry start)").into());
    }
    let start = lh + 30 + u16le(bytes, lh + 26) as usize + u16le(bytes, lh + 28) as usize;
    let end = start.checked_add(c.csize as usize).filter(|&end| end <= bytes.len());
    let Some(end) = end else { return Err(crate::lang::tr("zip が壊れています（項目がはみ出しています）", "Broken zip (entry overruns)").into()) };
    let data = &bytes[start..end];
    match method {
        0 => Ok(data.to_vec()),
        8 => crate::neo::inflate::inflate(data, max),
        m => Err(if crate::lang::is_en() { format!("Unsupported compression method ({m})") } else { format!("対応していない圧縮方式です（{m}）") }),
    }
}

// ------------------------------------------------------------------ 名前の検査と、編集用の文字

pub fn to_edit_text(entries: &[Entry]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&e.path);
        s.push('\n');
    }
    s
}

/// 編集された文字を、元の一覧と同じ並びの新しいパスに直す。行数が違う・使えない名前があればエラー。
/// フォルダの行は、末尾の `/` が抜けていても補う。
pub fn parse_edit_text(text: &str, old: &[Entry]) -> Result<Vec<String>, String> {
    let mut lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.len() != old.len() {
        return Err(if crate::lang::is_en() { format!(
            "The line count differs from the original (original {} lines, now {} lines). Lines cannot be added or removed; change only the names.",
            old.len(),
            lines.len()
        ) } else { format!(
            "行数が元と違います（元 {} 行 / 今 {} 行）。行の追加・削除はできません。名前だけを直してください。",
            old.len(),
            lines.len()
        ) });
    }
    let new: Vec<String> = lines
        .iter()
        .zip(old)
        .map(|(l, o)| if o.is_dir && !l.is_empty() && !l.ends_with('/') { format!("{l}/") } else { l.to_string() })
        .collect();
    check_new_paths(old, &new)?;
    let new = follow_dir_renames(old, new);
    check_new_paths(old, &new)?;
    Ok(new)
}

/// フォルダの行だけを書き換えたとき、その中の（行を直していない）項目も一緒に移す。
/// 一番近い「名前を変えたフォルダ」の新しい名前を使う。子の行を自分で直したものはそのまま。
fn follow_dir_renames(old: &[Entry], new: Vec<String>) -> Vec<String> {
    let moved: Vec<(&str, &str)> =
        old.iter().zip(&new).filter(|(o, n)| o.is_dir && o.path != **n).map(|(o, n)| (o.path.as_str(), n.as_str())).collect();
    if moved.is_empty() {
        return new;
    }
    old.iter()
        .zip(&new)
        .map(|(o, n)| {
            if o.path != *n {
                return n.clone();
            }
            match moved.iter().filter(|(od, _)| o.path.len() > od.len() && o.path.starts_with(od)).max_by_key(|(od, _)| od.len()) {
                Some((od, nd)) => format!("{nd}{}", &o.path[od.len()..]),
                None => n.clone(),
            }
        })
        .collect()
}

fn bad_component(c: &str) -> Option<&'static str> {
    if c.is_empty() {
        Some(crate::lang::tr("空の階層があります", "Empty path component"))
    } else if c == "." || c == ".." {
        Some(crate::lang::tr("`.` と `..` は使えません", "`.` and `..` cannot be used"))
    } else if c.chars().any(|ch| ch == '\\' || ch == ':' || ch.is_control()) {
        Some(crate::lang::tr("`\\` `:` や制御文字は使えません", "`\\` `:` and control characters cannot be used"))
    } else {
        None
    }
}

/// 変更された行だけを厳しく調べる（元からある変わった名前は、触らなければそのまま通す）。
pub fn check_new_paths(old: &[Entry], new: &[String]) -> Result<(), String> {
    if old.len() != new.len() {
        return Err(if crate::lang::is_en() { format!("The line count differs from the original (original {} lines, now {} lines)", old.len(), new.len()) } else { format!("行数が元と違います（元 {} 行 / 今 {} 行）", old.len(), new.len()) });
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for (i, (o, n)) in old.iter().zip(new).enumerate() {
        let line = i + 1;
        if o.path != *n {
            if n.is_empty() || n == "/" {
                return Err(if crate::lang::is_en() { format!("Line {line}: name is empty") } else { format!("{line} 行目: 名前が空です") });
            }
            if n.starts_with('/') {
                return Err(if crate::lang::is_en() { format!("Line {line}: absolute paths starting with `/` cannot be used") } else { format!("{line} 行目: `/` で始まる絶対パスは使えません") });
            }
            if o.is_dir != n.ends_with('/') {
                return Err(if crate::lang::is_en() { format!(
                    "Line {line}: {}",
                    if o.is_dir { crate::lang::tr("フォルダの名前は `/` で終わらせてください", "Folder names must end with `/`") } else { crate::lang::tr("ファイルの名前を `/` で終わらせることはできません", "File names cannot end with `/`") }
                ) } else { format!(
                    "{line} 行目: {}",
                    if o.is_dir { crate::lang::tr("フォルダの名前は `/` で終わらせてください", "Folder names must end with `/`") } else { crate::lang::tr("ファイルの名前を `/` で終わらせることはできません", "File names cannot end with `/`") }
                ) });
            }
            for c in n.trim_end_matches('/').split('/') {
                if let Some(why) = bad_component(c) {
                    return Err(if crate::lang::is_en() { format!("Line {line}: {why} ({n})") } else { format!("{line} 行目: {why}（{n}）") });
                }
            }
            if n.len() > 1024 {
                return Err(if crate::lang::is_en() { format!("Line {line}: path is too long") } else { format!("{line} 行目: パスが長すぎます") });
            }
        }
        if !seen.insert(n.as_str()) {
            return Err(if crate::lang::is_en() { format!("Line {line}: two items have the same name ({n})") } else { format!("{line} 行目: 同じ名前が 2 つあります（{n}）") });
        }
    }
    // ファイルの名前が、別の項目の親フォルダの名前と衝突していないか
    let files: HashSet<&str> = new.iter().zip(old).filter(|(_, o)| !o.is_dir).map(|(n, _)| n.as_str()).collect();
    for n in new {
        for (idx, ch) in n.trim_end_matches('/').char_indices() {
            if ch == '/' && files.contains(&n[..idx]) {
                return Err(if crate::lang::is_en() { format!("File '{}' is also used as a folder ({n})", &n[..idx]) } else { format!("ファイル「{}」はフォルダとしても使われています（{n}）", &n[..idx]) });
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ zip の書き換え

/// extra 領域から Info-ZIP の Unicode Path（0x7075）を除く。名前を変えたのに古い名前が優先されるのを防ぐ。
fn strip_unicode_path(extra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(extra.len());
    let mut i = 0;
    while i + 4 <= extra.len() {
        let id = u16le(extra, i);
        let len = u16le(extra, i + 2) as usize;
        let end = (i + 4 + len).min(extra.len());
        if id != 0x7075 {
            out.extend_from_slice(&extra[i..end]);
        }
        i = end;
    }
    out.extend_from_slice(&extra[i..]);
    out
}

/// zip の名前だけを書き換えた新しいバイト列を返す。`new` は `list_zip` と同じ並び。
/// 圧縮データは 1 バイトも変えずにコピーする。変えた名前は UTF-8（フラグ付き）で書く。
pub fn rename_zip(bytes: &[u8], new: &[String]) -> Result<Vec<u8>, String> {
    let e = find_eocd(bytes, 0)?;
    let cd = cd_slice(bytes, &e)?;
    let cds = read_cd(cd, &e)?;
    let old = entries_of(cd, &cds);
    check_new_paths(&old, new)?;

    // ローカルヘッダーの位置を並べ、各項目が占める範囲（ヘッダー + データ + あれば data descriptor）を決める
    let mut order: Vec<usize> = (0..cds.len()).collect();
    order.sort_by_key(|&i| cds[i].lh_off);
    for w in order.windows(2) {
        if cds[w[0]].lh_off == cds[w[1]].lh_off {
            return Err(crate::lang::tr("対応していない zip です（同じ位置を指す項目があります）", "Unsupported zip (entries point to the same location)").into());
        }
    }
    if let Some(&f) = order.first() {
        if cds[f].lh_off != 0 {
            return Err(crate::lang::tr("対応していない zip です（先頭に余分なデータがあります）", "Unsupported zip (extra data at the start)").into());
        }
    }
    let mut span_end = vec![0usize; cds.len()];
    for (k, &i) in order.iter().enumerate() {
        span_end[i] = order.get(k + 1).map_or(e.cd_off, |&j| cds[j].lh_off);
    }

    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 64);
    let mut new_off = vec![0usize; cds.len()];
    for &i in &order {
        let c = &cds[i];
        let (s, end) = (c.lh_off, span_end[i]);
        if s + 30 > end || u32le(bytes, s) != 0x0403_4b50 {
            return Err(crate::lang::tr("zip が壊れています（ローカルヘッダーが見つかりません）", "Broken zip (local header not found)").into());
        }
        let (ln, lx) = (u16le(bytes, s + 26) as usize, u16le(bytes, s + 28) as usize);
        let body = s + 30 + ln + lx;
        if body > end {
            return Err(crate::lang::tr("zip が壊れています（ローカルヘッダーがはみ出しています）", "Broken zip (local header overruns)").into());
        }
        new_off[i] = out.len();
        if old[i].path == new[i] {
            out.extend_from_slice(&bytes[s..end]);
            continue;
        }
        let name = new[i].as_bytes();
        let extra = strip_unicode_path(&bytes[s + 30 + ln..body]);
        let mut hdr = bytes[s..s + 30].to_vec();
        if !new[i].is_ascii() {
            let f = u16le(&hdr, 6) | 0x800;
            hdr[6..8].copy_from_slice(&f.to_le_bytes());
        }
        hdr[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        hdr[28..30].copy_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&hdr);
        out.extend_from_slice(name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&bytes[body..end]);
    }

    let cd_start = out.len();
    for (i, c) in cds.iter().enumerate() {
        let p = c.start;
        let mut hdr = cd[p..p + 46].to_vec();
        let name_raw = &cd[p + 46..p + 46 + c.name_len];
        let extra_raw = &cd[p + 46 + c.name_len..p + 46 + c.name_len + c.extra_len];
        let comment = &cd[p + 46 + c.name_len + c.extra_len..p + 46 + c.name_len + c.extra_len + c.comment_len];
        let (name, extra): (Vec<u8>, Vec<u8>) = if old[i].path == new[i] {
            (name_raw.to_vec(), extra_raw.to_vec())
        } else {
            if !new[i].is_ascii() {
                let f = u16le(&hdr, 8) | 0x800;
                hdr[8..10].copy_from_slice(&f.to_le_bytes());
            }
            (new[i].as_bytes().to_vec(), strip_unicode_path(extra_raw))
        };
        if name.len() > 0xFFFF || extra.len() > 0xFFFF {
            return Err(if crate::lang::is_en() { format!("Item {}: name is too long", i + 1) } else { format!("{} 件目: 名前が長すぎます", i + 1) });
        }
        hdr[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        hdr[30..32].copy_from_slice(&(extra.len() as u16).to_le_bytes());
        hdr[42..46].copy_from_slice(&(new_off[i] as u32).to_le_bytes());
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(comment);
    }
    let cd_size = out.len() - cd_start;
    if out.len() as u64 >= 0xFFFF_FFFF {
        return Err(crate::lang::tr("結果が 4 GiB を超えるため書けません（ZIP64 未対応）", "Cannot write: the result exceeds 4 GiB (ZIP64 is not supported)").into());
    }
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(cds.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cds.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd_size as u32).to_le_bytes());
    out.extend_from_slice(&(cd_start as u32).to_le_bytes());
    out.extend_from_slice(&(e.comment.len() as u16).to_le_bytes());
    out.extend_from_slice(&e.comment);
    Ok(out)
}

// ------------------------------------------------------------------ フォルダの名前変更

fn rel_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for c in rel.trim_end_matches('/').split('/') {
        p.push(c);
    }
    p
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// 一覧の名前変更をフォルダに適用する。変えたファイルの数を返す。
/// 途中で失敗したら、動かしたファイルを元に戻す（作ったフォルダは残ることがある）。
pub fn rename_dir(root: &Path, old: &[Entry], new: &[String]) -> Result<usize, String> {
    check_new_paths(old, new)?;
    let moving: Vec<(usize, &str, &str)> = old
        .iter()
        .zip(new)
        .enumerate()
        .filter(|(_, (o, n))| !o.is_dir && o.path != **n)
        .map(|(i, (o, n))| (i, o.path.as_str(), n.as_str()))
        .collect();
    let old_files: HashSet<&str> = moving.iter().map(|m| m.1).collect();

    // 事前の検査: 行き先が既にある・リンクの中へ入る、は全部ここで止める
    for &(_, _, to) in &moving {
        if exists(&rel_path(root, to)) && !old_files.contains(to) {
            return Err(if crate::lang::is_en() { format!("Already exists: {to}") } else { format!("すでに存在します: {to}") });
        }
    }
    let mut targets: Vec<&str> = moving.iter().map(|m| m.2).collect();
    targets.extend(old.iter().zip(new).filter(|(o, n)| o.is_dir && o.path != **n).map(|(_, n)| n.as_str()));
    for t in targets {
        let mut acc = String::new();
        for c in t.trim_end_matches('/').split('/') {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(c);
            if let Ok(m) = std::fs::symlink_metadata(rel_path(root, &acc)) {
                let last = acc.len() == t.trim_end_matches('/').len();
                if m.file_type().is_symlink() || (!last && !m.is_dir()) || (last && t.ends_with('/') && !m.is_dir()) {
                    return Err(if crate::lang::is_en() { format!("Destination cannot be used: {t}") } else { format!("行き先に使えない場所です: {t}") });
                }
            }
        }
    }

    // 1) 一時の置き場へ全部動かす（入れ替えのような衝突を避ける） 2) 行き先へ動かす
    let tmp = root.join(format!(".neonote_rename_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new(); // (今の場所, 元の場所)
    let mut fail: Option<String> = None;
    for &(i, from, _) in &moving {
        let (src, dst) = (rel_path(root, from), tmp.join(i.to_string()));
        match std::fs::rename(&src, &dst) {
            Ok(()) => done.push((dst, src)),
            Err(e) => {
                fail = Some(format!("{from}: {e}"));
                break;
            }
        }
    }
    if fail.is_none() {
        for (k, &(_, from, to)) in moving.iter().enumerate() {
            let dst = rel_path(root, to);
            let step = dst.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::rename(&done[k].0, &dst));
            match step {
                Ok(()) => done[k] = (dst, rel_path(root, from)),
                Err(e) => {
                    fail = Some(format!("{to}: {e}"));
                    break;
                }
            }
        }
    }
    if let Some(msg) = fail {
        for (now, orig) in done.iter().rev() {
            let _ = std::fs::rename(now, orig);
        }
        let _ = std::fs::remove_dir(&tmp);
        return Err(if crate::lang::is_en() { format!("Could not rename (moved items were put back)\n{msg}") } else { format!("名前を変更できませんでした（動かしたものは元に戻しました）\n{msg}") });
    }
    let _ = std::fs::remove_dir(&tmp);

    // フォルダ: 新しい名前を作り（空のフォルダも動いたことになる）、使われなくなった元のフォルダを、空なら消す
    for (o, n) in old.iter().zip(new) {
        if o.is_dir && o.path != *n {
            let _ = std::fs::create_dir_all(rel_path(root, n));
        }
    }
    let kept: HashSet<&str> = new.iter().map(String::as_str).collect();
    let mut dirs: Vec<&str> = old.iter().filter(|o| o.is_dir && !kept.contains(o.path.as_str())).map(|o| o.path.as_str()).collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in dirs {
        let _ = std::fs::remove_dir(rel_path(root, d)); // 空でなければ失敗する。それでよい
    }
    Ok(moving.len())
}

// ------------------------------------------------------------------ 階層の表示

struct Node {
    name: String,
    is_dir: bool,
    size: u64,
    packed: u64,
    children: Vec<Node>,
}

fn build_tree(entries: &[Entry]) -> Node {
    let mut root = Node { name: String::new(), is_dir: true, size: 0, packed: 0, children: Vec::new() };
    for e in entries {
        let comps: Vec<&str> = e.path.trim_end_matches('/').split('/').filter(|c| !c.is_empty()).collect();
        let mut cur = &mut root;
        for (k, c) in comps.iter().enumerate() {
            let last = k + 1 == comps.len();
            let is_dir = !last || e.is_dir;
            let at = match cur.children.iter().position(|n| n.name == *c && n.is_dir == is_dir) {
                Some(a) => a,
                None => {
                    cur.children.push(Node { name: (*c).to_string(), is_dir, size: 0, packed: 0, children: Vec::new() });
                    cur.children.len() - 1
                }
            };
            cur = &mut cur.children[at];
            if last && !e.is_dir {
                cur.size = e.size;
                cur.packed = e.packed.unwrap_or(0);
            }
        }
    }
    fn sum(n: &mut Node) {
        for c in &mut n.children {
            sum(c);
        }
        if n.is_dir {
            n.size = n.children.iter().map(|c| c.size).sum();
            n.packed = n.children.iter().map(|c| c.packed).sum();
        }
        n.children.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
    }
    sum(&mut root);
    root
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < U.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", U[u])
}

fn esc(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// 階層ツリー（フォルダが先・名前順・フォルダの大きさは中身の合計）を HelpView 向けの HTML にする。
/// `packed` が true なら圧縮後サイズの列も出す（zip）。色は `#rrggbb`。
pub fn to_html(title: &str, entries: &[Entry], packed: bool, bg: &str, fg: &str, dim: &str) -> String {
    let root = build_tree(entries);
    let files = entries.iter().filter(|e| !e.is_dir).count();
    let mut dirs = HashSet::new();
    for e in entries {
        let comps: Vec<&str> = e.path.trim_end_matches('/').split('/').collect();
        let upto = if e.is_dir { comps.len() } else { comps.len() - 1 };
        for k in 1..=upto {
            dirs.insert(comps[..k].join("/"));
        }
    }
    let mut head = String::new();
    esc(title, &mut head);
    let mut body = format!("<p><b>{head}</b><br><font color=\"{dim}\">{files} files, {} folders, {}", dirs.len(), human_size(root.size));
    if packed {
        body.push_str(&format!(" (packed {})", human_size(root.packed)));
    }
    body.push_str("</font></p>");
    if entries.is_empty() {
        body.push_str(crate::lang::tr("<p>（空です）</p>", "<p>(Empty)</p>"));
    } else {
        body.push_str("<table border=\"0\" cellspacing=\"0\" cellpadding=\"1\">");
        fn walk(n: &Node, depth: usize, packed: bool, dim: &str, out: &mut String) {
            for c in &n.children {
                out.push_str("<tr><td>");
                for _ in 0..depth {
                    out.push_str("&nbsp;&nbsp;&nbsp;&nbsp;");
                }
                out.push_str(if c.is_dir { "[+] " } else { "&nbsp;&nbsp;&nbsp;" }); // ファイルも名前の位置をそろえる
                esc(&c.name, out);
                if c.is_dir {
                    out.push('/');
                }
                out.push_str(&format!("&nbsp;&nbsp;</td><td align=\"right\"><font color=\"{dim}\">{}</font></td>", human_size(c.size)));
                if packed {
                    out.push_str(&format!("<td align=\"right\"><font color=\"{dim}\">&nbsp;&nbsp;{}</font></td>", human_size(c.packed)));
                }
                out.push_str("</tr>");
                walk(c, depth + 1, packed, dim, out);
            }
        }
        walk(&root, 0, packed, dim, &mut body);
        body.push_str("</table>");
    }
    format!("<html><body bgcolor=\"{bg}\" text=\"{fg}\">{body}</body></html>")
}

/// 名前の変更を一覧の形で見せるための、元 → 先の対応（変わった行だけ）。確認表示用。
pub fn changes<'a>(old: &'a [Entry], new: &'a [String]) -> Vec<(&'a str, &'a str)> {
    old.iter().zip(new).filter(|(o, n)| o.path != **n).map(|(o, n)| (o.path.as_str(), n.as_str())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn crc32(d: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in d {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }

    /// テスト用の zip（無圧縮）。(名前の生バイト, フラグ, 中身)
    fn make_zip(items: &[(&[u8], u16, &[u8])], comment: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, flags, data) in items {
            let off = out.len() as u32;
            let crc = crc32(data);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0]);
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name);
            out.extend_from_slice(data);
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0]);
            cd.extend_from_slice(&flags.to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name);
        }
        let start = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    fn sample() -> Vec<u8> {
        make_zip(&[(b"docs/", 0, b""), (b"docs/a.txt", 0, b"hello"), (b"b.txt", 0, b"world!!"), ("日本語/メモ.txt".as_bytes(), 0x800, b"x")], b"cmt")
    }

    fn e(path: &str) -> Entry {
        Entry { path: path.into(), is_dir: path.ends_with('/'), size: 0, packed: None }
    }

    #[test]
    fn lists_a_zip() {
        let v = list_zip(&sample()).unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(names, ["docs/", "docs/a.txt", "b.txt", "日本語/メモ.txt"]);
        assert!(v[0].is_dir && !v[1].is_dir);
        assert_eq!((v[1].size, v[1].packed), (5, Some(5)));
    }

    #[test]
    fn list_zip_file_reads_only_the_directory() {
        let d = std::env::temp_dir().join(format!("neonote_zipfile_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let big = vec![7u8; 300_000];
        let z = make_zip(&[(b"big.bin", 0, &big), (b"s/t.txt", 0, b"t")], &vec![b'c'; 1000]);
        let f = d.join("a.zip");
        std::fs::write(&f, &z).unwrap();
        assert_eq!(list_zip_file(&f).unwrap(), list_zip(&z).unwrap());
        std::fs::write(&f, b"not a zip at all").unwrap();
        assert!(list_zip_file(&f).is_err());
        std::fs::write(&f, b"").unwrap();
        assert!(list_zip_file(&f).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shift_jis_names_are_read() {
        let (sj, _, _) = encoding_rs::SHIFT_JIS.encode("資料/日本語.txt");
        let z = make_zip(&[(&sj, 0, b"1")], b"");
        assert_eq!(list_zip(&z).unwrap()[0].path, "資料/日本語.txt");
    }

    #[test]
    fn not_a_zip_and_zip64_are_errors() {
        assert!(list_zip(b"hello").is_err());
        assert!(list_zip(&[0u8; 100]).is_err());
        let mut z = sample();
        let n = z.len();
        z[n - 3 - 22 + 10..n - 3 - 22 + 12].copy_from_slice(&[0xFF, 0xFF]);
        assert!(list_zip(&z).unwrap_err().contains("ZIP64"));
    }

    #[test]
    fn rename_keeps_data_and_comment_and_reparses() {
        let z = sample();
        let old = list_zip(&z).unwrap();
        let mut new: Vec<String> = old.iter().map(|x| x.path.clone()).collect();
        new[0] = "文書/".into();
        new[1] = "文書/b.txt".into();
        new[2] = "top.txt".into();
        let out = rename_zip(&z, &new).unwrap();
        let v = list_zip(&out).unwrap();
        assert_eq!(v.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), new);
        assert_eq!(v[1].size, 5);
        assert!(out.ends_with(b"cmt"));
        // 圧縮データ（ここでは中身そのもの）はそのまま残る
        assert!(out.windows(5).any(|w| w == b"hello") && out.windows(7).any(|w| w == b"world!!"));
        // 変えていない項目は、名前のバイトもそのまま
        assert!(out.windows("日本語/メモ.txt".len()).any(|w| w == "日本語/メモ.txt".as_bytes()));
        // 変えなければバイト列まで同じ
        let same: Vec<String> = old.iter().map(|x| x.path.clone()).collect();
        assert_eq!(rename_zip(&z, &same).unwrap(), z);
    }

    #[test]
    fn unicode_path_extra_is_dropped_only_for_renamed_names() {
        // 2 件の名前変更が通ることと、0x7075 だけが取り除かれること（data descriptor 付きの実物は別途 unzip -t と Python で確認済み）
        let mut z = make_zip(&[(b"a.txt", 0, b"AAAA"), (b"b.txt", 0, b"BBBB")], b"");
        let old = list_zip(&z).unwrap();
        assert_eq!(old.len(), 2);
        z = rename_zip(&z, &["x/a.txt".to_string(), "b.txt".to_string()]).unwrap();
        assert_eq!(list_zip(&z).unwrap()[0].path, "x/a.txt");
        assert_eq!(strip_unicode_path(&[0x75, 0x70, 2, 0, 9, 9, 1, 0, 1, 0, 7]), vec![1, 0, 1, 0, 7]);
    }

    #[test]
    fn bad_names_are_rejected() {
        let old = vec![e("d/"), e("d/a.txt"), e("b.txt")];
        let ok = |n: &[&str]| check_new_paths(&old, &n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(ok(&["d/", "d/a.txt", "b.txt"]).is_ok());
        assert!(ok(&["d/", "d/a.txt", "d/a.txt"]).unwrap_err().contains("同じ名前"));
        assert!(ok(&["d/", "d/a.txt", "../b.txt"]).is_err());
        assert!(ok(&["d/", "d/a.txt", "/abs.txt"]).is_err());
        assert!(ok(&["d/", "d/a.txt", "x//y.txt"]).is_err());
        assert!(ok(&["d/", "d/a.txt", "C:/y.txt"]).is_err());
        assert!(ok(&["d/", "d/a.txt", ""]).is_err());
        assert!(ok(&["d/", "d/a.txt", "dir/"]).is_err()); // ファイルをフォルダにはできない
        assert!(ok(&["d", "d/a.txt", "b.txt"]).is_err()); // フォルダの `/` 抜け（parse では補う）
        assert!(ok(&["d/", "d/a.txt", "d/a.txt/x"]).is_err());
        assert!(ok(&["d/", "b.txt/c", "b.txt"]).is_err()); // ファイル名が親フォルダ名と衝突
    }

    #[test]
    fn renaming_a_folder_line_carries_its_unedited_children() {
        let old = vec![e("a/"), e("a/b/"), e("a/b/f.txt"), e("a/g.txt"), e("z.txt")];
        // a/ → x/ だけ直した: 子はすべてついていく
        assert_eq!(parse_edit_text("x/\na/b/\na/b/f.txt\na/g.txt\nz.txt", &old).unwrap(), ["x/", "x/b/", "x/b/f.txt", "x/g.txt", "z.txt"]);
        // 子も自分で直した行は、その通りにする
        assert_eq!(parse_edit_text("x/\na/b/\na/b/F.txt\na/g.txt\nz.txt", &old).unwrap(), ["x/", "x/b/", "a/b/F.txt", "x/g.txt", "z.txt"]);
        // 内側のフォルダも直したら、近い方を使う
        assert_eq!(parse_edit_text("x/\nx/c/\na/b/f.txt\na/g.txt\nz.txt", &old).unwrap(), ["x/", "x/c/", "x/c/f.txt", "x/g.txt", "z.txt"]);
        // 前方一致だけの別名（ab/）には及ばない
        let old2 = vec![e("a/"), e("ab/"), e("ab/f.txt")];
        assert_eq!(parse_edit_text("x/\nab/\nab/f.txt", &old2).unwrap(), ["x/", "ab/", "ab/f.txt"]);
    }

    #[test]
    fn renaming_a_folder_moves_its_files_on_disk_and_in_zip() {
        let r = tmpdir("follow");
        fs::create_dir_all(r.join("d/in")).unwrap();
        fs::write(r.join("d/in/f.txt"), "F").unwrap();
        fs::write(r.join("d/g.txt"), "G").unwrap();
        let old = list_dir(&r).unwrap();
        let text = to_edit_text(&old).replacen("d/\n", "e/\n", 1);
        let new = parse_edit_text(&text, &old).unwrap();
        rename_dir(&r, &old, &new).unwrap();
        assert!(!r.join("d").exists());
        assert_eq!(fs::read_to_string(r.join("e/in/f.txt")).unwrap(), "F");
        assert_eq!(fs::read_to_string(r.join("e/g.txt")).unwrap(), "G");
        let _ = fs::remove_dir_all(&r);
        let z = make_zip(&[(b"d/", 0, b""), (b"d/f.txt", 0, b"F")], b"");
        let zold = list_zip(&z).unwrap();
        let znew = parse_edit_text("e/\nd/f.txt\n", &zold).unwrap();
        let out = rename_zip(&z, &znew).unwrap();
        assert_eq!(list_zip(&out).unwrap().iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["e/", "e/f.txt"]);
    }

    #[test]
    fn edit_text_roundtrip_and_errors() {
        let old = vec![e("d/"), e("d/a.txt")];
        assert_eq!(to_edit_text(&old), "d/\nd/a.txt\n");
        assert_eq!(parse_edit_text("d/\r\nd/a.txt\r\n", &old).unwrap(), ["d/", "d/a.txt"]);
        assert_eq!(parse_edit_text("e\nd/a.txt", &old).unwrap(), ["e/", "e/a.txt"]); // `/` を補い、中身もついていく
        assert!(parse_edit_text("d/\n", &old).unwrap_err().contains("行数"));
        assert!(parse_edit_text("d/\nd/a.txt\nextra\n", &old).is_err());
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("neonote_archive_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn lists_and_renames_a_folder_including_swap() {
        let r = tmpdir("dir");
        fs::create_dir_all(r.join("a/sub")).unwrap();
        fs::create_dir_all(r.join("empty")).unwrap();
        fs::write(r.join("a/x.txt"), "X").unwrap();
        fs::write(r.join("a/sub/y.txt"), "YY").unwrap();
        fs::write(r.join("top.txt"), "T").unwrap();
        let old = list_dir(&r).unwrap();
        assert_eq!(old.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["a/", "a/sub/", "a/sub/y.txt", "a/x.txt", "empty/", "top.txt"]);
        assert_eq!(old[2].size, 2);
        let new: Vec<String> = ["b/", "b/sub/", "b/sub/y.txt", "b/x.txt", "vacant/", "top.txt"].iter().map(|s| s.to_string()).collect();
        assert_eq!(rename_dir(&r, &old, &new).unwrap(), 2);
        assert_eq!(fs::read_to_string(r.join("b/sub/y.txt")).unwrap(), "YY");
        assert!(!r.join("a").exists() && !r.join("empty").exists() && r.join("vacant").is_dir());
        assert_eq!(fs::read_to_string(r.join("top.txt")).unwrap(), "T");
        // 入れ替え（x ↔ top）
        let old = list_dir(&r).unwrap();
        let new: Vec<String> = old.iter().map(|o| match o.path.as_str() { "b/x.txt" => "top.txt".into(), "top.txt" => "b/x.txt".into(), p => p.to_string() }).collect();
        // 並びの対応は元の一覧の順なので、同じ集合になる入れ替えは重複にならない
        assert_eq!(rename_dir(&r, &old, &new).unwrap(), 2);
        assert_eq!(fs::read_to_string(r.join("top.txt")).unwrap(), "X");
        assert_eq!(fs::read_to_string(r.join("b/x.txt")).unwrap(), "T");
        assert!(!r.join(format!(".neonote_rename_{}", std::process::id())).exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn folder_rename_refuses_to_overwrite() {
        let r = tmpdir("clobber");
        fs::write(r.join("a.txt"), "A").unwrap();
        fs::write(r.join("b.txt"), "B").unwrap();
        let old = vec![e("a.txt")]; // b.txt は一覧に無い（あとから増えた）想定
        let err = rename_dir(&r, &old, &["b.txt".to_string()]).unwrap_err();
        assert!(err.contains("すでに存在"));
        assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "A");
        assert_eq!(fs::read_to_string(r.join("b.txt")).unwrap(), "B");
        let _ = fs::remove_dir_all(&r);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_and_not_entered() {
        let r = tmpdir("link");
        let outside = tmpdir("outside");
        fs::write(r.join("f.txt"), "F").unwrap();
        std::os::unix::fs::symlink(&outside, r.join("lnk")).unwrap();
        let old = list_dir(&r).unwrap();
        assert_eq!(old.len(), 1);
        let err = rename_dir(&r, &old, &["lnk/f.txt".to_string()]).unwrap_err();
        assert!(err.contains("使えない"));
        assert!(r.join("f.txt").exists() && !outside.join("f.txt").exists());
        let _ = fs::remove_dir_all(&r);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn tree_html_shows_hierarchy_sizes_and_escapes() {
        let mut v = vec![e("z.txt"), e("<d>/"), e("<d>/a&b.txt"), e("deep/x/y.txt")];
        v[0].size = 2048;
        v[2].size = 10;
        v[3].size = 5;
        let h = to_html("t<>", &v, false, "#000000", "#ffffff", "#888888");
        assert!(h.contains("t&lt;&gt;") && h.contains("&lt;d&gt;/") && h.contains("a&amp;b.txt"));
        assert!(h.contains("3 files, 3 folders, 2.0 KB") || h.contains("3 files, 3 folders, 2.0 KB"), "{h}");
        // フォルダが先、深さの分だけ字下げ、フォルダの大きさは中身の合計
        assert!(h.find("deep/").unwrap() < h.find("z.txt").unwrap());
        assert!(h.contains("&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;y.txt"));
        assert!(!h.contains("packed"));
        assert!(to_html("z", &[], true, "#0", "#f", "#8").contains("空です"));
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KB");
    }

    #[test]
    fn missing_parent_dirs_are_synthesized_in_the_tree() {
        let z = make_zip(&[(b"only/deep/file.txt", 0, b"abc")], b"");
        let v = list_zip(&z).unwrap();
        let h = to_html("z", &v, true, "#0", "#f", "#8");
        assert!(h.contains("only/") && h.contains("deep/") && h.contains("1 files, 2 folders") && h.contains("packed 3 B"));
    }
}
