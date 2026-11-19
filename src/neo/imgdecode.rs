// FLTK が読めない画像（webp・tiff・tga・dds・psd）を RGBA に解読する。面積図の縮めた画像に使う。
// 壊れた・途中で切れたデータは None（落ちない）。大きすぎる絵は読まない。

/// 解読した絵（RGBA、1 画素 4 バイト）。
pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// 縦横どちらかがこれを超える絵は読まない（メモリを食いすぎないため）。
const MAX_SIDE: usize = 16384;
const MAX_PIXELS: usize = 64 * 1024 * 1024;

fn sized(w: usize, h: usize) -> bool {
    w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE && w * h <= MAX_PIXELS
}

/// RGBA を PNG にする（Preview で見せるため。圧縮は控えめ）。
pub fn to_png(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&c);
        out.extend_from_slice(&super::deflate::crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &super::deflate::zlib(&raw));
    chunk(b"IEND", &[]);
    out
}

/// 拡張子でここが読む形式か（Preview で PNG に直して見せる物）。
pub fn is_decodable_path(p: &std::path::Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| matches!(e.as_str(), "webp" | "tif" | "tiff" | "tga" | "dds" | "psd"))
}

/// 先頭のバイト列から、ここで読める形式か（拡張子の名前）を判定する。tga は印が無いので拡張子で判定する。
pub fn sniff(b: &[u8], ext: &str) -> Option<&'static str> {
    if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        Some("tiff")
    } else if b.starts_with(b"DDS ") {
        Some("dds")
    } else if b.starts_with(b"8BPS") {
        Some("psd")
    } else if ext.eq_ignore_ascii_case("tga") {
        Some("tga")
    } else {
        None
    }
}

/// `ext` はファイルの拡張子（tga の判定に使う）。
pub fn decode(b: &[u8], ext: &str) -> Option<Rgba> {
    let r = match sniff(b, ext)? {
        "webp" => webp(b),
        "tiff" => tiff(b),
        "dds" => dds(b),
        "psd" => psd(b),
        _ => tga(b),
    }?;
    (sized(r.w, r.h) && r.data.len() == r.w * r.h * 4).then_some(r)
}

fn webp(b: &[u8]) -> Option<Rgba> {
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(b)).ok()?;
    let (w, h) = d.dimensions();
    let (w, h) = (w as usize, h as usize);
    if !sized(w, h) {
        return None;
    }
    let alpha = d.has_alpha();
    let mut buf = vec![0u8; d.output_buffer_size()?];
    d.read_image(&mut buf).ok()?;
    let data = if alpha { buf } else { buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    Some(Rgba { w, h, data })
}

fn tiff(b: &[u8]) -> Option<Rgba> {
    use tiff::decoder::{Decoder, DecodingResult, Limits};
    use tiff::ColorType;
    let mut d = Decoder::new(std::io::Cursor::new(b)).ok()?.with_limits(Limits::default());
    let (w, h) = d.dimensions().ok()?;
    let (w, h) = (w as usize, h as usize);
    if !sized(w, h) {
        return None;
    }
    let color = d.colortype().ok()?;
    let px: Vec<u8> = match d.read_image().ok()? {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.iter().map(|&x| (x >> 8) as u8).collect(),
        _ => return None,
    };
    let data: Vec<u8> = match color {
        ColorType::Gray(_) => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        ColorType::GrayA(_) => px.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        ColorType::RGB(_) => px.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        ColorType::RGBA(_) => px,
        ColorType::CMYK(_) => px.as_chunks::<4>().0.iter().flat_map(|p| {
            let k = 255 - p[3] as u32;
            let c = |v: u8| ((255 - v as u32) * k / 255) as u8;
            [c(p[0]), c(p[1]), c(p[2]), 255]
        }).collect(),
        _ => return None,
    };
    (data.len() >= w * h * 4).then(|| Rgba { w, h, data: data[..w * h * 4].to_vec() })
}

fn u16le(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u32le(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}
fn u16be(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u32be(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

/// TGA: 色の表なし・あり、白黒、RLE。16・24・32 ビット。
fn tga(b: &[u8]) -> Option<Rgba> {
    let id_len = *b.first()? as usize;
    let cmap_type = *b.get(1)?;
    let kind = *b.get(2)?;
    let (cmap_first, cmap_len, cmap_bits) = (u16le(b, 3)?, u16le(b, 5)?, *b.get(7)? as usize);
    let (w, h) = (u16le(b, 12)?, u16le(b, 14)?);
    let bpp = *b.get(16)? as usize;
    let desc = *b.get(17)?;
    if !sized(w, h) || !matches!(kind, 1 | 2 | 3 | 9 | 10 | 11) {
        return None;
    }
    let mut p = 18 + id_len;
    let mut cmap = Vec::new();
    if cmap_type == 1 {
        let cb = cmap_bits.div_ceil(8);
        for i in 0..cmap_len {
            cmap.push(tga_color(b.get(p + i * cb..p + (i + 1) * cb)?, cmap_bits)?);
        }
        p += cmap_len * cb;
    }
    let pb = bpp.div_ceil(8);
    if pb == 0 || pb > 4 {
        return None;
    }
    let pixel = |s: &[u8]| -> Option<[u8; 4]> {
        match kind {
            1 | 9 => {
                let i = if pb == 1 { s[0] as usize } else { u16le(s, 0)? };
                cmap.get(i.checked_sub(cmap_first)?).copied()
            }
            3 | 11 => Some([s[0], s[0], s[0], 255]),
            _ => tga_color(s, bpp),
        }
    };
    let n = w * h;
    let mut out = Vec::with_capacity(n * 4);
    if kind >= 9 {
        while out.len() < n * 4 {
            let head = *b.get(p)?;
            p += 1;
            let count = (head & 0x7F) as usize + 1;
            if head & 0x80 != 0 {
                let c = pixel(b.get(p..p + pb)?)?;
                p += pb;
                for _ in 0..count {
                    out.extend_from_slice(&c);
                }
            } else {
                for _ in 0..count {
                    out.extend_from_slice(&pixel(b.get(p..p + pb)?)?);
                    p += pb;
                }
            }
        }
        out.truncate(n * 4);
    } else {
        let body = b.get(p..p + n * pb)?;
        for s in body.chunks_exact(pb) {
            out.extend_from_slice(&pixel(s)?);
        }
    }
    // 既定は下から上へ並ぶ（desc の bit5 が立っていれば上から）。右から左（bit4）は使われないので見ない
    if desc & 0x20 == 0 {
        let row = w * 4;
        let mut flipped = Vec::with_capacity(out.len());
        for y in (0..h).rev() {
            flipped.extend_from_slice(&out[y * row..(y + 1) * row]);
        }
        out = flipped;
    }
    Some(Rgba { w, h, data: out })
}

fn tga_color(s: &[u8], bits: usize) -> Option<[u8; 4]> {
    Some(match bits {
        15 | 16 => {
            let v = u16le(s, 0)?;
            let c = |x: usize| ((x & 31) * 255 / 31) as u8;
            [c(v >> 10), c(v >> 5), c(v), 255]
        }
        24 => [*s.get(2)?, s[1], s[0], 255],
        32 => [*s.get(2)?, s[1], s[0], s[3]],
        8 => [s[0], s[0], s[0], 255],
        _ => return None,
    })
}

/// DDS: BC1（DXT1）・BC2（DXT3）・BC3（DXT5）と、圧縮なしの 32・24 ビット。最初の絵（一番大きな段）だけ。
fn dds(b: &[u8]) -> Option<Rgba> {
    let (h, w) = (u32le(b, 12)?, u32le(b, 16)?);
    if !sized(w, h) {
        return None;
    }
    let pf_flags = u32le(b, 80)?;
    let four = b.get(84..88)?;
    let data = b.get(128..)?;
    let mut out = vec![0u8; w * h * 4];
    if pf_flags & 4 != 0 {
        let block = match four {
            b"DXT1" => 8,
            b"DXT2" | b"DXT3" | b"DXT4" | b"DXT5" => 16,
            _ => return None,
        };
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        for by in 0..bh {
            for bx in 0..bw {
                let blk = data.get((by * bw + bx) * block..(by * bw + bx + 1) * block)?;
                let px = dxt_block(blk, four)?;
                for (i, c) in px.iter().enumerate() {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    if x < w && y < h {
                        out[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(c);
                    }
                }
            }
        }
    } else {
        let bits = u32le(b, 88)?;
        let (rm, gm, bm, am) = (u32le(b, 92)? as u32, u32le(b, 96)? as u32, u32le(b, 100)? as u32, u32le(b, 104)? as u32);
        let pb = bits / 8;
        if !(pb == 3 || pb == 4) {
            return None;
        }
        let take = |v: u32, m: u32| -> u8 {
            if m == 0 {
                return 0;
            }
            let s = m.trailing_zeros();
            let max = m >> s;
            (((v & m) >> s) * 255 / max.max(1)) as u8
        };
        for i in 0..w * h {
            let s = data.get(i * pb..i * pb + pb)?;
            let v = if pb == 4 { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_le_bytes([s[0], s[1], s[2], 0]) };
            let a = if pf_flags & 1 != 0 { take(v, am) } else { 255 };
            out[i * 4..i * 4 + 4].copy_from_slice(&[take(v, rm), take(v, gm), take(v, bm), a]);
        }
    }
    Some(Rgba { w, h, data: out })
}

fn rgb565(v: u16) -> [u8; 3] {
    let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

/// 4×4 の 1 区画を、左上から行ごとに 16 画素へ。
fn dxt_block(blk: &[u8], four: &[u8]) -> Option<[[u8; 4]; 16]> {
    let (alpha, color) = if blk.len() == 16 { (&blk[..8], &blk[8..]) } else { (&[][..], blk) };
    let c0 = u16::from_le_bytes([color[0], color[1]]);
    let c1 = u16::from_le_bytes([color[2], color[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |p: u32, q: u32, d: u32| -> [u8; 3] { [0, 1, 2].map(|k| ((a[k] as u32 * p + b[k] as u32 * q) / d) as u8) };
    let opaque = c0 > c1 || blk.len() == 16;
    let palette: [[u8; 4]; 4] = {
        let (m2, m3) = if opaque { (mix(2, 1, 3), mix(1, 2, 3)) } else { (mix(1, 1, 2), [0, 0, 0]) };
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [m2[0], m2[1], m2[2], 255], [m3[0], m3[1], m3[2], if opaque { 255 } else { 0 }]]
    };
    let bits = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
    let mut out = [[0u8; 4]; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = palette[((bits >> (i * 2)) & 3) as usize];
    }
    if !alpha.is_empty() {
        if matches!(four, b"DXT2" | b"DXT3") {
            for (i, o) in out.iter_mut().enumerate() {
                let n = (alpha[i / 2] >> ((i % 2) * 4)) & 15;
                o[3] = n * 17;
            }
        } else {
            let (a0, a1) = (alpha[0] as u32, alpha[1] as u32);
            let table: [u32; 8] = if a0 > a1 {
                [a0, a1, (6 * a0 + a1) / 7, (5 * a0 + 2 * a1) / 7, (4 * a0 + 3 * a1) / 7, (3 * a0 + 4 * a1) / 7, (2 * a0 + 5 * a1) / 7, (a0 + 6 * a1) / 7]
            } else {
                [a0, a1, (4 * a0 + a1) / 5, (3 * a0 + 2 * a1) / 5, (2 * a0 + 3 * a1) / 5, (a0 + 4 * a1) / 5, 0, 255]
            };
            let mut idx = 0u64;
            for (k, &v) in alpha[2..8].iter().enumerate() {
                idx |= (v as u64) << (8 * k);
            }
            for (i, o) in out.iter_mut().enumerate() {
                o[3] = table[((idx >> (3 * i)) & 7) as usize] as u8;
            }
        }
    }
    Some(out)
}

/// PSD: 保存された合成済みの絵（最後の区画）だけ。8 ビットの RGB・白黒、圧縮なしと RLE。
fn psd(b: &[u8]) -> Option<Rgba> {
    if u16be(b, 4)? != 1 {
        return None; // 2 は PSB（大きな版）。今は読まない
    }
    let channels = u16be(b, 12)?;
    let (h, w) = (u32be(b, 14)?, u32be(b, 18)?);
    let depth = u16be(b, 22)?;
    let mode = u16be(b, 24)?;
    if !sized(w, h) || depth != 8 || channels == 0 || !matches!(mode, 1 | 3) {
        return None;
    }
    let mut p: usize = 26;
    for _ in 0..3 {
        // 色の表・画像の資源・層と覆いの区画を飛ばす
        p = p.checked_add(4)?.checked_add(u32be(b, p)?)?;
    }
    let comp = u16be(b, p)?;
    p += 2;
    let n = w * h;
    let mut planes: Vec<Vec<u8>> = Vec::new();
    match comp {
        0 => {
            for c in 0..channels {
                planes.push(b.get(p + c * n..p + (c + 1) * n)?.to_vec());
            }
        }
        1 => {
            let counts = p;
            p += channels * h * 2;
            for c in 0..channels {
                let mut plane = Vec::with_capacity(n);
                for y in 0..h {
                    let len = u16be(b, counts + (c * h + y) * 2)?;
                    let row = b.get(p..p + len)?;
                    p += len;
                    let start = plane.len();
                    let mut i = 0;
                    while i < row.len() && plane.len() - start < w {
                        let k = row[i] as i8;
                        i += 1;
                        if k >= 0 {
                            let m = k as usize + 1;
                            plane.extend_from_slice(row.get(i..i + m)?);
                            i += m;
                        } else if k != -128 {
                            let v = *row.get(i)?;
                            i += 1;
                            plane.extend(std::iter::repeat_n(v, (1 - k as isize) as usize));
                        }
                    }
                    plane.resize(start + w, 0);
                }
                planes.push(plane);
            }
        }
        _ => return None,
    }
    let get = |c: usize, i: usize| planes.get(c).map_or(255, |pl| pl[i]);
    let data = (0..n)
        .flat_map(|i| if mode == 1 { let g = get(0, i); [g, g, g, if channels > 1 { get(1, i) } else { 255 }] } else { [get(0, i), get(1, i), get(2, i), if channels > 3 { get(3, i) } else { 255 }] })
        .collect();
    Some(Rgba { w, h, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tga_uncompressed_and_rle_top_left() {
        // 2×1、24 ビット、上から並ぶ（desc の bit5）。赤・青
        let mut b = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0x20];
        b.extend_from_slice(&[0, 0, 255, 255, 0, 0]);
        let r = decode(&b, "tga").unwrap();
        assert_eq!(r.data, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        // RLE: 2 画素とも緑
        let mut c = vec![0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0x20];
        c.extend_from_slice(&[0x81, 0, 255, 0]);
        assert_eq!(decode(&c, "tga").unwrap().data, vec![0, 255, 0, 255, 0, 255, 0, 255]);
        assert!(decode(&c[..20], "tga").is_none(), "途中で切れたら None");
    }

    #[test]
    fn dds_dxt1_solid_block() {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        b[12..16].copy_from_slice(&4u32.to_le_bytes());
        b[16..20].copy_from_slice(&4u32.to_le_bytes());
        b[80..84].copy_from_slice(&4u32.to_le_bytes());
        b[84..88].copy_from_slice(b"DXT1");
        // 色 0 = 赤（0xF800）、全画素が 0 番
        b.extend_from_slice(&[0x00, 0xF8, 0x00, 0x00, 0, 0, 0, 0]);
        let r = decode(&b, "dds").unwrap();
        assert_eq!((r.w, r.h), (4, 4));
        assert_eq!(&r.data[..4], &[255, 0, 0, 255]);
        assert!(decode(&b[..130], "dds").is_none());
    }

    #[test]
    fn psd_raw_rgb() {
        let mut b = b"8BPS".to_vec();
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&3u16.to_be_bytes()); // 3 チャンネル
        b.extend_from_slice(&1u32.to_be_bytes()); // 高さ
        b.extend_from_slice(&2u32.to_be_bytes()); // 幅
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes()); // RGB
        b.extend_from_slice(&[0; 12]); // 空の区画 3 つ
        b.extend_from_slice(&0u16.to_be_bytes());
        b.extend_from_slice(&[255, 0, 0, 0, 0, 255]); // R 面・G 面・B 面
        let r = decode(&b, "psd").unwrap();
        assert_eq!(r.data, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(decode(&b[..b.len() - 1], "psd").is_none());
    }

    #[test]
    fn garbage_is_none() {
        assert!(decode(b"RIFF\0\0\0\0WEBPjunk", "webp").is_none());
        assert!(decode(b"II*\0garbage", "tif").is_none());
        assert!(decode(b"hello", "txt").is_none());
    }
}
