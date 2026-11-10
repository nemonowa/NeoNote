// ext2 / ext3 / ext4 のディスクイメージの中身の一覧と、ファイルの取り出し（GUI なし・読み取り専用・依存の追加なし）。
// Linux Reader のように、Linux のファイルシステムを Windows などから見るための第一歩。
//
// 読むもの:
//   - 素のファイルシステムのイメージ（先頭から ext2/3/4）と、MBR / GPT のパーティション表つきのディスクイメージ
//     （中の ext パーティションを `partN/` の下に並べる）。
//   - ext2/3 のブロックマップ（直接・1 重・2 重・3 重の間接）と、ext4 のエクステント（深い木も可）。
//   - 64bit（大きなイメージ）・flex_bg・huge_file・スパース（穴は 0 で埋める）。
// 読まないもの（一覧に出さない・読めない旨を返す）:
//   - ジャーナルの再生（再生が要る未マウントのイメージは、再生前の状態を読む）、メタデータのチェックサムの検査、
//     inline_data（小さなファイルを inode に入れる形式）・暗号化・圧縮・meta_bg の配置。
//   - シンボリックリンク・デバイス・FIFO（`list_dir` がシンボリックリンクを出さないのと同じ）。
// イメージは丸ごとメモリに載せず、必要な所だけ `Read + Seek` で読む（数 GB のイメージでも開ける）。
use crate::neo::archive::{Entry, MAX_ENTRIES};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 1 ファイルを読むときの上限（丸ごとメモリに載せるため）。
pub const MAX_FILE: u64 = 256 << 20;
/// ディレクトリ 1 つを読む上限。
const MAX_DIR: u64 = 64 << 20;
const MAGIC: u16 = 0xEF53;
const ROOT_INO: u32 = 2;

const INCOMPAT_COMPRESSION: u32 = 0x1;
const INCOMPAT_META_BG: u32 = 0x10;
const INCOMPAT_64BIT: u32 = 0x80;
const FL_EXTENTS: u32 = 0x80000;
const FL_INLINE: u32 = 0x1000_0000;
const FL_ENCRYPT: u32 = 0x800;

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn u64le(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn err<T>(ja: &'static str, en: &'static str) -> Result<T, String> {
    Err(crate::lang::tr(ja, en).to_string())
}

/// 読む元のファイルの、ある範囲（パーティション）から先頭までのずらし。
fn read_at<R: Read + Seek>(r: &mut R, off: u64, buf: &mut [u8]) -> Result<(), String> {
    r.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    r.read_exact(buf).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ ファイルシステム

#[derive(Clone, Debug)]
struct Inode {
    mode: u16,
    size: u64,
    flags: u32,
    block: [u8; 60],
}

impl Inode {
    fn is_dir(&self) -> bool {
        self.mode & 0xF000 == 0x4000
    }
    fn is_file(&self) -> bool {
        self.mode & 0xF000 == 0x8000
    }
}

pub struct Ext<R: Read + Seek> {
    r: R,
    base: u64,
    bs: u64,
    ipg: u64,
    inode_size: u64,
    inodes: u64,
    /// グループごとの inode 表の先頭ブロック。
    tables: Vec<u64>,
}

/// `base` から始まる領域に ext2/3/4 のスーパーブロックがあるか。
pub fn probe<R: Read + Seek>(r: &mut R, base: u64) -> bool {
    let mut sb = [0u8; 2];
    read_at(r, base + 1024 + 56, &mut sb).is_ok() && u16::from_le_bytes(sb) == MAGIC
}

impl<R: Read + Seek> Ext<R> {
    pub fn open(mut r: R, base: u64) -> Result<Self, String> {
        let mut sb = [0u8; 1024];
        read_at(&mut r, base + 1024, &mut sb).map_err(|_| crate::lang::tr("ext のイメージではありません", "Not an ext image").to_string())?;
        if u16le(&sb, 56) != MAGIC {
            return err("ext のイメージではありません", "Not an ext image");
        }
        let incompat = u32le(&sb, 96);
        if incompat & INCOMPAT_COMPRESSION != 0 {
            return err("圧縮された ext には未対応です", "Compressed ext is not supported");
        }
        if incompat & INCOMPAT_META_BG != 0 {
            return err("meta_bg の ext には未対応です", "meta_bg layout is not supported");
        }
        let log = u32le(&sb, 24);
        if log > 6 {
            return err("ブロックの大きさが不正です", "Invalid block size");
        }
        let bs = 1024u64 << log;
        let (inodes, bpg, ipg) = (u32le(&sb, 0) as u64, u32le(&sb, 32) as u64, u32le(&sb, 40) as u64);
        if bpg == 0 || ipg == 0 {
            return err("スーパーブロックが壊れています", "Broken superblock");
        }
        let first_data = u32le(&sb, 20) as u64;
        let mut blocks = u32le(&sb, 4) as u64;
        let is64 = incompat & INCOMPAT_64BIT != 0;
        if is64 {
            blocks |= (u32le(&sb, 0x150) as u64) << 32;
        }
        let inode_size = if u32le(&sb, 76) == 0 { 128 } else { u16le(&sb, 88) as u64 };
        if !(128..=bs).contains(&inode_size) {
            return err("inode の大きさが不正です", "Invalid inode size");
        }
        let desc_size = if is64 { (u16le(&sb, 0xFE) as u64).max(32) } else { 32 };
        if desc_size > 1024 || desc_size > bs {
            return err("スーパーブロックが壊れています", "Broken superblock");
        }
        let groups = blocks.saturating_sub(first_data).div_ceil(bpg).max(1);
        if groups > 1 << 20 || groups.saturating_mul(ipg) < inodes {
            return err("グループの数が不正です", "Invalid group count");
        }
        // グループ記述子の表は、スーパーブロックの次のブロックから
        let gdt = (first_data + 1) * bs;
        let mut raw = vec![0u8; (groups * desc_size) as usize];
        read_at(&mut r, base + gdt, &mut raw)?;
        let tables = (0..groups as usize)
            .map(|g| {
                let d = &raw[g * desc_size as usize..];
                let mut t = u32le(d, 8) as u64;
                if desc_size >= 64 {
                    t |= (u32le(d, 0x28) as u64) << 32;
                }
                t
            })
            .collect();
        Ok(Ext { r, base, bs, ipg, inode_size, inodes, tables })
    }

    /// ブロック番号からイメージの中の位置。壊れたイメージの大きすぎる番号は Err（桁あふれさせない）。
    fn offset(&self, blk: u64) -> Result<u64, String> {
        match blk.checked_mul(self.bs).and_then(|o| o.checked_add(self.base)) {
            Some(o) => Ok(o),
            None => err("ブロックの番号が範囲外です", "Block number out of range"),
        }
    }

    fn read_block(&mut self, blk: u64, buf: &mut [u8]) -> Result<(), String> {
        let off = self.offset(blk)?;
        read_at(&mut self.r, off, buf)
    }

    fn inode(&mut self, ino: u32) -> Result<Inode, String> {
        let n = ino as u64;
        if n == 0 || n > self.inodes {
            return err("inode の番号が範囲外です", "Inode number out of range");
        }
        let (g, i) = ((n - 1) / self.ipg, (n - 1) % self.ipg);
        let Some(&t) = self.tables.get(g as usize) else {
            return err("inode の番号が範囲外です", "Inode number out of range");
        };
        let mut raw = vec![0u8; self.inode_size as usize];
        let off = self.offset(t)?.checked_add(i * self.inode_size).ok_or_else(|| crate::lang::tr("inode の番号が範囲外です", "Inode number out of range").to_string())?;
        read_at(&mut self.r, off, &mut raw)?;
        let mode = u16le(&raw, 0);
        let mut size = u32le(&raw, 4) as u64;
        if mode & 0xF000 == 0x8000 {
            size |= (u32le(&raw, 0x6C) as u64) << 32; // 通常ファイルだけ、上位 32 ビットが入る
        }
        let mut block = [0u8; 60];
        block.copy_from_slice(&raw[0x28..0x28 + 60]);
        Ok(Inode { mode, size, flags: u32le(&raw, 0x20), block })
    }

    /// 論理ブロック順の、物理ブロック番号（0 は穴）。先頭の `n` ブロックぶん。
    fn blocks_of(&mut self, ino: &Inode, n: u64) -> Result<Vec<u64>, String> {
        let mut out = vec![0u64; n as usize];
        if ino.flags & FL_EXTENTS != 0 {
            let root = ino.block;
            self.walk_extents(&root, 0, &mut out)?;
        } else {
            let p = self.bs / 4;
            let word = |b: &[u8; 60], i: usize| u32le(b, i * 4) as u64;
            for (i, slot) in out.iter_mut().enumerate().take(12) {
                *slot = word(&ino.block, i);
            }
            // 間接ブロックは、n に届く範囲だけ読む
            let mut next = 12u64;
            for (level, start) in [(1u32, 12usize), (2, 13), (3, 14)] {
                if next >= n {
                    break;
                }
                let span = p.pow(level);
                let root = word(&ino.block, start);
                self.walk_indirect(root, level, next, &mut out)?;
                next += span;
            }
        }
        Ok(out)
    }

    fn walk_indirect(&mut self, blk: u64, level: u32, first: u64, out: &mut [u64]) -> Result<(), String> {
        if blk == 0 || first >= out.len() as u64 {
            return Ok(());
        }
        let mut buf = vec![0u8; self.bs as usize];
        self.read_block(blk, &mut buf)?;
        let p = self.bs / 4;
        let child_span = p.pow(level - 1);
        for k in 0..p {
            let at = first + k * child_span;
            if at >= out.len() as u64 {
                break;
            }
            let v = u32le(&buf, (k * 4) as usize) as u64;
            if level == 1 {
                out[at as usize] = v;
            } else {
                self.walk_indirect(v, level - 1, at, out)?;
            }
        }
        Ok(())
    }

    fn walk_extents(&mut self, node: &[u8], depth_seen: u32, out: &mut [u64]) -> Result<(), String> {
        if node.len() < 12 || u16le(node, 0) != 0xF30A || depth_seen > 6 {
            return err("エクステントが壊れています", "Broken extent tree");
        }
        let (entries, depth) = (u16le(node, 2) as usize, u16le(node, 6));
        if 12 + entries * 12 > node.len() {
            return err("エクステントが壊れています", "Broken extent tree");
        }
        for k in 0..entries {
            let e = &node[12 + k * 12..24 + k * 12];
            if depth == 0 {
                let (lblk, mut len) = (u32le(e, 0) as u64, u16le(e, 4) as u64);
                if len > 32768 {
                    len -= 32768; // 未初期化（確保だけで中身は 0）
                }
                let start = u32le(e, 8) as u64 | (u16le(e, 6) as u64) << 32;
                for j in 0..len {
                    let at = lblk + j;
                    if at < out.len() as u64 && !(u16le(e, 4) > 32768) {
                        out[at as usize] = start + j;
                    }
                }
            } else {
                let leaf = u32le(e, 4) as u64 | (u16le(e, 8) as u64) << 32;
                let mut buf = vec![0u8; self.bs as usize];
                self.read_block(leaf, &mut buf)?;
                self.walk_extents(&buf, depth_seen + 1, out)?;
            }
        }
        Ok(())
    }

    fn unsupported(ino: &Inode) -> Option<String> {
        if ino.flags & FL_INLINE != 0 {
            return Some(crate::lang::tr("inline_data のファイルは読めません", "inline_data files are not supported").to_string());
        }
        if ino.flags & FL_ENCRYPT != 0 {
            return Some(crate::lang::tr("暗号化されたファイルは読めません", "Encrypted files are not supported").to_string());
        }
        None
    }

    /// inode の中身を読む（`max` バイトまで。超えるなら Err）。穴は 0 で埋める。
    fn read_inode(&mut self, ino: &Inode, max: u64) -> Result<Vec<u8>, String> {
        if let Some(e) = Self::unsupported(ino) {
            return Err(e);
        }
        if ino.size > max {
            return err("大きすぎて読めません", "Too large to read");
        }
        let n = ino.size.div_ceil(self.bs);
        let blocks = self.blocks_of(ino, n)?;
        let mut out = vec![0u8; ino.size as usize];
        let mut i = 0usize;
        while i < blocks.len() {
            if blocks[i] == 0 {
                i += 1;
                continue;
            }
            // 物理ブロックが連続している間は、1 回の読みにまとめる
            let mut j = i + 1;
            while j < blocks.len() && blocks[j] == blocks[i] + (j - i) as u64 {
                j += 1;
            }
            let (from, to) = (i as u64 * self.bs, (j as u64 * self.bs).min(ino.size));
            let want = (to - from) as usize;
            let mut buf = vec![0u8; want];
            // ファイルの終わりがイメージの終わりを越える壊れたイメージでは、読めた所までにする
            let off = self.offset(blocks[i]);
            if off.is_ok_and(|o| self.r.seek(SeekFrom::Start(o)).is_ok()) {
                let mut got = 0;
                while got < want {
                    match self.r.read(&mut buf[got..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => got += k,
                    }
                }
            }
            out[from as usize..to as usize].copy_from_slice(&buf);
            i = j;
        }
        Ok(out)
    }

    /// 番号 `ino` のファイルの中身。通常ファイルだけ。
    pub fn read_file(&mut self, ino: u32, max: u64) -> Result<Vec<u8>, String> {
        let node = self.inode(ino)?;
        if !node.is_file() {
            return err("通常のファイルではありません", "Not a regular file");
        }
        self.read_inode(&node, max.min(MAX_FILE))
    }

    /// `/` 区切りのパス（先頭の `/` は有っても無くてもよい）から inode の番号を探す。
    pub fn lookup(&mut self, path: &str) -> Result<Option<u32>, String> {
        let mut cur = ROOT_INO;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let node = self.inode(cur)?;
            if !node.is_dir() {
                return Ok(None);
            }
            match self.dir_entries(&node)?.into_iter().find(|(n, _)| n == part) {
                Some((_, i)) => cur = i,
                None => return Ok(None),
            }
        }
        Ok(Some(cur))
    }

    /// ディレクトリの中の (名前, inode 番号)。`.` と `..` は含めない。
    fn dir_entries(&mut self, dir: &Inode) -> Result<Vec<(String, u32)>, String> {
        let data = self.read_inode(dir, MAX_DIR)?;
        let mut out = Vec::new();
        for chunk in data.chunks(self.bs as usize) {
            let mut p = 0usize;
            while p + 8 <= chunk.len() {
                let (ino, rec, nlen) = (u32le(chunk, p), u16le(chunk, p + 4) as usize, chunk[p + 6] as usize);
                if rec < 8 || rec % 4 != 0 || p + rec > chunk.len() {
                    break; // 壊れたブロック（この先は読まない）
                }
                if ino != 0 && 8 + nlen <= rec {
                    let name = String::from_utf8_lossy(&chunk[p + 8..p + 8 + nlen]).into_owned();
                    if name != "." && name != ".." {
                        out.push((name, ino));
                    }
                }
                p += rec;
            }
        }
        Ok(out)
    }

    /// ルートから全て一覧にする。`prefix` は各パスの先頭に付ける（`part1/` など）。
    pub fn list(&mut self, prefix: &str, out: &mut Vec<Entry>) -> Result<(), String> {
        let mut v = Vec::new();
        self.list_inodes(prefix, &mut v)?;
        out.extend(v.into_iter().map(|(e, _)| e));
        Ok(())
    }

    /// `list` と同じで、各項目の inode 番号つき（中身を続けて読むときに、パスをたどり直さずに済む）。
    fn list_inodes(&mut self, prefix: &str, out: &mut Vec<(Entry, u32)>) -> Result<(), String> {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut stack: Vec<(u32, String, u32)> = vec![(ROOT_INO, prefix.to_string(), 0)];
        while let Some((ino, dir, depth)) = stack.pop() {
            if !seen.insert(ino) || depth > 64 {
                continue; // ループ・深すぎる入れ子は読まない
            }
            let node = self.inode(ino)?;
            if !node.is_dir() || Self::unsupported(&node).is_some() {
                continue;
            }
            for (name, child) in self.dir_entries(&node)? {
                let Ok(c) = self.inode(child) else { continue };
                if out.len() >= MAX_ENTRIES {
                    return Err(if crate::lang::is_en() { format!("Too many entries (up to {MAX_ENTRIES})") } else { format!("項目が多すぎます（{MAX_ENTRIES} 件まで）") });
                }
                if c.is_dir() {
                    let rel = format!("{dir}{name}/");
                    out.push((Entry { path: rel.clone(), is_dir: true, size: 0, packed: None }, child));
                    stack.push((child, rel, depth + 1));
                } else if c.is_file() {
                    out.push((Entry { path: format!("{dir}{name}"), is_dir: false, size: c.size, packed: None }, child));
                }
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ パーティション表

/// ディスクイメージの、パーティションの (番号, 先頭のバイト位置)。MBR か GPT（GPT なら保護 MBR は無視）。
pub fn partitions<R: Read + Seek>(r: &mut R) -> Vec<(usize, u64)> {
    let mut s0 = [0u8; 512];
    if read_at(r, 0, &mut s0).is_err() || s0[510] != 0x55 || s0[511] != 0xAA {
        return Vec::new();
    }
    let mut hdr = [0u8; 512];
    let mut out = Vec::new();
    if read_at(r, 512, &mut hdr).is_ok() && &hdr[0..8] == b"EFI PART" {
        let (lba, count, size) = (u64le(&hdr, 72), u32le(&hdr, 80) as usize, u32le(&hdr, 84) as usize);
        if (128..=4096).contains(&size) && count <= 1024 {
            let mut raw = vec![0u8; count * size];
            if read_at(r, lba.saturating_mul(512), &mut raw).is_ok() {
                for i in 0..count {
                    let e = &raw[i * size..];
                    if e[..16].iter().any(|&b| b != 0) {
                        out.push((i + 1, u64le(e, 32).saturating_mul(512)));
                    }
                }
            }
        }
        return out;
    }
    for i in 0..4 {
        let e = &s0[446 + i * 16..462 + i * 16];
        let (kind, start) = (e[4], u32le(e, 8) as u64);
        if kind != 0 && kind != 0x05 && kind != 0x0F && kind != 0xEE && start > 0 {
            out.push((i + 1, start * 512));
        }
    }
    out
}

// ------------------------------------------------------------------ 入り口

/// 拡張子の候補。中身（スーパーブロックかパーティション表）も確かめる。
fn has_ext_name(p: &Path) -> bool {
    p.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "img" | "ext2" | "ext3" | "ext4" | "raw" | "dd"))
}

/// ext のイメージ（か、ext を含むディスクイメージ）として一覧で開くか。
pub fn is_ext_path(p: &Path) -> bool {
    if !has_ext_name(p) || p.is_dir() {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    probe(&mut f, 0) || partitions(&mut f).into_iter().any(|(_, off)| probe(&mut f, off))
}

/// イメージの全てのファイルを一覧にする。素の ext なら直接、パーティション表があれば `partN/` の下に。
pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    let ctx = |e: std::io::Error| format!("{}\n{e}", p.display());
    let mut f = std::fs::File::open(p).map_err(ctx)?;
    let mut out = Vec::new();
    if probe(&mut f, 0) {
        Ext::open(f, 0)?.list("", &mut out)?;
        out.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        return Ok(out);
    }
    let mut found = false;
    for (n, off) in partitions(&mut f) {
        if !probe(&mut f, off) {
            continue;
        }
        found = true;
        let name = format!("part{n}/");
        out.push(Entry { path: name.clone(), is_dir: true, size: 0, packed: None });
        let g = std::fs::File::open(p).map_err(ctx)?;
        Ext::open(g, off)?.list(&name, &mut out)?;
    }
    if !found {
        return err("ext2/3/4 のイメージではありません", "Not an ext2/3/4 image");
    }
    out.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// イメージの中の、大きさが `max_each` 以下のファイルを 1 つずつ読んで `f(パス, 中身)` に渡す（`f` が false を返したら止める）。
/// イメージは 1 回だけ開き、一覧をたどりながら読む（本文の検索用。1 件ごとに開き直さない）。
pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    let ctx = |e: std::io::Error| format!("{}\n{e}", p.display());
    let mut probe_f = std::fs::File::open(p).map_err(ctx)?;
    let roots: Vec<(String, u64)> = if probe(&mut probe_f, 0) {
        vec![(String::new(), 0)]
    } else {
        partitions(&mut probe_f).into_iter().filter(|&(_, off)| probe(&mut probe_f, off)).map(|(n, off)| (format!("part{n}/"), off)).collect()
    };
    for (prefix, base) in roots {
        let mut fs = Ext::open(std::fs::File::open(p).map_err(ctx)?, base)?;
        let mut all = Vec::new();
        fs.list_inodes(&prefix, &mut all)?;
        for (e, ino) in all.into_iter().filter(|(e, _)| !e.is_dir && e.size <= max_each) {
            if let Ok(data) = fs.read_file(ino, max_each) {
                if !f(&e.path, data) {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// イメージの中のファイル 1 つの中身。`path` は `list_file` が返したパス（パーティション表つきなら `partN/` から）。
pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let ctx = |e: std::io::Error| format!("{}\n{e}", p.display());
    let mut f = std::fs::File::open(p).map_err(ctx)?;
    let (base, inner) = if probe(&mut f, 0) {
        (0, path.to_string())
    } else {
        let (head, rest) = path.split_once('/').unwrap_or((path, ""));
        let n: usize = head.strip_prefix("part").and_then(|s| s.parse().ok()).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
        let off = partitions(&mut f).into_iter().find(|&(i, _)| i == n).map(|(_, o)| o).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
        (off, rest.to_string())
    };
    let mut fs = Ext::open(f, base)?;
    match fs.lookup(&inner)? {
        Some(ino) => fs.read_file(ino, max),
        None => err("見つかりません", "Not found"),
    }
}

// ------------------------------------------------------------------ 取り出して開く

/// 一覧から開くときに取り出せるファイルの大きさの上限（取り出しは画面を止めて行うため、`MAX_FILE` より小さくする）。
pub const EXTRACT_MAX: u64 = 64 << 20;

fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("neonote-ext-{}", std::process::id()))
}

/// 取り出し先で使える名前にする（Windows で使えない文字と、`.` `..` を避ける）。
fn safe_name(part: &str) -> String {
    let s: String = part.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\') || c.is_control() { '_' } else { c }).collect();
    if s.is_empty() || s == "." || s == ".." {
        "_".to_string()
    } else {
        s
    }
}

/// イメージの中のファイル `path` を一時フォルダへ取り出して、そのパスを返す（開くため）。
/// 取り出したファイルは読み取り専用にする（編集しても、イメージには書き戻らないため、保存で誤解しないように）。
/// 一時フォルダは終了時に `cleanup_temp` で消す。
pub fn extract_to_temp(image: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    let data = read_path(image, path, EXTRACT_MAX)?;
    // 同じ名前のファイルが別のイメージから来ても混ざらないよう、イメージのパスで分ける
    let tag = image.to_string_lossy().bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let mut out = temp_root().join(format!("{tag:08x}"));
    for part in path.split('/').filter(|p| !p.is_empty()) {
        out.push(safe_name(part));
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if out.exists() {
        make_writable(&out);
    }
    std::fs::write(&out, &data).map_err(|e| format!("{}\n{e}", out.display()))?;
    if let Ok(meta) = std::fs::metadata(&out) {
        let mut perm = meta.permissions();
        perm.set_readonly(true);
        let _ = std::fs::set_permissions(&out, perm);
    }
    Ok(out)
}

fn make_writable(p: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(p) {
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    make_writable(&e.path());
                }
            }
        } else {
            let mut perm = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(p, perm);
        }
    }
}

/// 取り出した一時ファイルを消す（終了時。読み取り専用を外してから。失敗は無視）。
pub fn cleanup_temp() {
    let root = temp_root();
    if root.exists() {
        make_writable(&root);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn have_mke2fs() -> bool {
        Command::new("mke2fs").arg("-V").output().is_ok()
    }

    /// 擬似乱数（毎回同じ中身）。
    fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 40) as u8
            })
            .collect()
    }

    /// 壊れたイメージ（スーパーブロックの印だけ正しく、残りは乱数）でも、落ちずに Err か何らかの一覧を返す。
    #[test]
    fn corrupt_images_do_not_panic() {
        for seed in 0..300u64 {
            let mut img = noise(64 * 1024, seed);
            img[1024 + 56..1024 + 58].copy_from_slice(&MAGIC.to_le_bytes());
            img[1024 + 24..1024 + 28].copy_from_slice(&((seed % 3) as u32).to_le_bytes()); // ブロックの大きさは時々正しく
            if seed % 2 == 0 {
                img[1024 + 96..1024 + 100].copy_from_slice(&(INCOMPAT_64BIT).to_le_bytes());
            }
            if let Ok(mut fs) = Ext::open(std::io::Cursor::new(img), 0) {
                let mut out = Vec::new();
                let _ = fs.list("", &mut out);
                let _ = fs.lookup("a/b");
                for ino in 1..20 {
                    let _ = fs.read_file(ino, 1 << 20);
                }
            }
        }
    }

    struct Fixture {
        dir: std::path::PathBuf,
        files: Vec<(String, Vec<u8>)>,
    }

    /// `mke2fs -d` で、中身つきのイメージを作る（root 不要）。mke2fs が無い環境では None（そのテストは飛ばす）。
    fn make(kind: &str, block: u32, size_mb: u32, tag: &str) -> Option<(Fixture, std::path::PathBuf)> {
        if !have_mke2fs() {
            eprintln!("mke2fs が無いので飛ばす");
            return None;
        }
        let dir = std::env::temp_dir().join(format!("neonote_ext_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("sub/deep")).unwrap();
        let files: Vec<(String, Vec<u8>)> = vec![
            ("small.txt".into(), b"hello ext\n".to_vec()),
            ("empty".into(), Vec::new()),
            ("日本語.txt".into(), "こんにちは".as_bytes().to_vec()),
            ("sub/mid.bin".into(), noise(300_000, 1)), // ブロックサイズ 1K なら 1 重間接に入る
            ("sub/deep/big.bin".into(), noise(5_000_000, 2)), // 2 重間接・複数エクステント
        ];
        for (n, d) in &files {
            std::fs::write(src.join(n), d).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink("small.txt", src.join("link")).unwrap();
        // 穴のあるファイル（先頭と末尾だけ中身がある）
        let mut sparse = vec![0u8; 3_000_000];
        sparse[..4].copy_from_slice(b"HEAD");
        let n = sparse.len();
        sparse[n - 4..].copy_from_slice(b"TAIL");
        std::fs::write(src.join("sparse.bin"), &sparse).unwrap();
        let img = dir.join("fs.img");
        let st = Command::new("mke2fs").args(["-q", "-F", "-t", kind, "-b", &block.to_string(), "-d"]).arg(&src).arg(&img).arg(format!("{size_mb}M")).output().unwrap();
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let mut files = files;
        files.push(("sparse.bin".into(), sparse));
        Some((Fixture { dir, files }, img))
    }

    fn check(kind: &str, block: u32) {
        let Some((fx, img)) = make(kind, block, 24, &format!("{kind}{block}")) else { return };
        assert!(is_ext_path(&img));
        let v = list_file(&img).unwrap();
        let paths: Vec<&str> = v.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.windows(2).all(|w| w[0] < w[1]));
        assert!(paths.contains(&"sub/") && paths.contains(&"sub/deep/") && paths.contains(&"日本語.txt"));
        assert!(!paths.contains(&"link"), "シンボリックリンクは出さない");
        assert!(!paths.iter().any(|p| p.starts_with("lost+found/") && !p.ends_with('/'))); // lost+found 自体は出てよい
        for (name, data) in &fx.files {
            let e = v.iter().find(|e| &e.path == name).unwrap_or_else(|| panic!("{name} が無い"));
            assert_eq!(e.size, data.len() as u64, "{name}");
            let got = read_path(&img, name, MAX_FILE).unwrap();
            assert!(got == *data, "{kind}/{block}: {name} の中身が違う");
        }
        assert!(read_path(&img, "nope", MAX_FILE).is_err());
        assert!(read_path(&img, "sub/deep/big.bin", 1000).is_err(), "上限を超えたら読まない");
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    #[test]
    fn ext4_extents_4k() {
        check("ext4", 4096);
    }

    #[test]
    fn ext4_extents_1k() {
        check("ext4", 1024);
    }

    #[test]
    fn ext2_block_map_with_indirect_blocks() {
        check("ext2", 1024);
    }

    #[test]
    fn ext3_block_map_4k() {
        check("ext3", 4096);
    }

    #[test]
    fn partitioned_images_mbr_and_gpt() {
        let Some((fx, img)) = make("ext4", 4096, 16, "part") else { return };
        let fs = std::fs::read(&img).unwrap();
        let start = 2048u64 * 512;
        // MBR
        let mut disk = vec![0u8; start as usize];
        disk.extend_from_slice(&fs);
        let e = &mut disk[446..462];
        e[4] = 0x83;
        e[8..12].copy_from_slice(&2048u32.to_le_bytes());
        e[12..16].copy_from_slice(&((fs.len() / 512) as u32).to_le_bytes());
        disk[510] = 0x55;
        disk[511] = 0xAA;
        let mbr = fx.dir.join("mbr.img");
        std::fs::write(&mbr, &disk).unwrap();
        assert!(is_ext_path(&mbr));
        let v = list_file(&mbr).unwrap();
        assert!(v.iter().any(|e| e.path == "part1/small.txt"));
        assert_eq!(read_path(&mbr, "part1/small.txt", MAX_FILE).unwrap(), b"hello ext\n");
        // GPT（保護 MBR + ヘッダー + 表）
        let mut g = disk.clone();
        g[446..462].fill(0);
        g[446 + 4] = 0xEE;
        g[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
        g[512..520].copy_from_slice(b"EFI PART");
        g[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        g[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        g[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        let ent = 1024;
        g[ent..ent + 16].fill(0x11); // 種類の GUID（0 でなければ使用中）
        g[ent + 32..ent + 40].copy_from_slice(&2048u64.to_le_bytes());
        let gpt = fx.dir.join("gpt.img");
        std::fs::write(&gpt, &g).unwrap();
        let v = list_file(&gpt).unwrap();
        assert!(v.iter().any(|e| e.path == "part1/sub/mid.bin"));
        assert_eq!(read_path(&gpt, "part1/sub/mid.bin", MAX_FILE).unwrap(), noise(300_000, 1));
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    #[test]
    fn non_ext_files_are_rejected_without_panicking() {
        let dir = std::env::temp_dir().join(format!("neonote_ext_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [("a.img", vec![0u8; 5000]), ("b.img", b"tiny".to_vec()), ("c.img", Vec::new()), ("d.ext4", noise(10_000, 9))] {
            let p = dir.join(name);
            std::fs::write(&p, data).unwrap();
            assert!(!is_ext_path(&p), "{name}");
            assert!(list_file(&p).is_err(), "{name}");
        }
        // 壊れたスーパーブロック（マジックだけ正しい）でも落ちない
        let mut bad = vec![0u8; 8192];
        bad[1024 + 56] = 0x53;
        bad[1024 + 57] = 0xEF;
        let p = dir.join("e.img");
        std::fs::write(&p, &bad).unwrap();
        assert!(list_file(&p).is_err());
        // 拡張子が違えば、中身が ext でも開かない
        assert!(!has_ext_name(Path::new("x.txt")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_directory_blocks_do_not_hang() {
        let Some((fx, img)) = make("ext2", 1024, 16, "corrupt") else { return };
        // イメージの中ほど（データ領域）をでたらめにしても、一覧は有限時間で終わる（Err か一部の一覧）
        let mut b = std::fs::read(&img).unwrap();
        let n = b.len();
        for (i, v) in noise(200_000, 5).into_iter().enumerate() {
            b[n / 3 + i] = v;
        }
        std::fs::write(&img, &b).unwrap();
        let t = std::time::Instant::now();
        let _ = list_file(&img);
        assert!(t.elapsed().as_secs() < 10);
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    /// 実在のフォルダから作ったイメージと、元のフォルダが同じ一覧・同じ中身になるか（手動。`NEONOTE_EXT_SRC` に元のフォルダ）。
    #[test]
    #[ignore]
    fn matches_a_real_directory_tree() {
        let src = std::path::PathBuf::from(std::env::var("NEONOTE_EXT_SRC").expect("NEONOTE_EXT_SRC"));
        let img = std::env::temp_dir().join(format!("neonote_ext_real_{}.img", std::process::id()));
        let st = Command::new("mke2fs").args(["-q", "-F", "-t", "ext4", "-d"]).arg(&src).arg(&img).arg("400M").output().unwrap();
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let t = std::time::Instant::now();
        let mine: Vec<Entry> = list_file(&img).unwrap().into_iter().filter(|e| !e.path.starts_with("lost+found")).collect();
        let want = crate::neo::archive::list_dir(&src).unwrap();
        println!("EXT list {} entries in {:?}", mine.len(), t.elapsed());
        assert_eq!(mine, want);
        let mut bytes = 0u64;
        for e in want.iter().filter(|e| !e.is_dir).step_by(7) {
            assert!(read_path(&img, &e.path, MAX_FILE).unwrap() == std::fs::read(src.join(&e.path)).unwrap(), "{}", e.path);
            bytes += e.size;
        }
        println!("EXT compared contents of every 7th file ({bytes} bytes)");
        let _ = std::fs::remove_file(&img);
    }

    #[test]
    fn each_file_visits_files_once_and_can_stop() {
        let Some((fx, img)) = make("ext4", 4096, 24, "each") else { return };
        let mut seen = Vec::new();
        each_file(&img, 1 << 20, |p, d| {
            seen.push((p.to_string(), d.len()));
            true
        })
        .unwrap();
        assert!(seen.iter().any(|(p, n)| p == "small.txt" && *n == 10));
        assert!(!seen.iter().any(|(p, _)| p == "sub/deep/big.bin"), "上限を超える物は渡さない");
        let mut n = 0;
        each_file(&img, 1 << 20, |_, _| {
            n += 1;
            false
        })
        .unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    #[test]
    fn extract_to_temp_writes_a_read_only_copy_and_cleans_up() {
        let Some((fx, img)) = make("ext4", 4096, 24, "extract") else { return };
        let p = extract_to_temp(&img, "sub/mid.bin").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), noise(300_000, 1));
        assert!(std::fs::metadata(&p).unwrap().permissions().readonly());
        assert!(p.starts_with(temp_root()));
        // 同じファイルをもう一度取り出せる（読み取り専用の前の物を置き換える）
        assert_eq!(extract_to_temp(&img, "sub/mid.bin").unwrap(), p);
        assert!(extract_to_temp(&img, "nope.txt").is_err());
        assert!(extract_to_temp(&img, "sub/deep/big.bin").is_err() == (5_000_000 > EXTRACT_MAX));
        assert_eq!(safe_name("a:b*c"), "a_b_c");
        assert_eq!(safe_name(".."), "_");
        cleanup_temp();
        assert!(!temp_root().exists());
        let _ = std::fs::remove_dir_all(&fx.dir);
    }
}
