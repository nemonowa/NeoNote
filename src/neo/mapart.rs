// 画像を Minecraft（Java 版）の地図アイテムのデータ（map_N.dat）にする（GUI なし）。
// 画像を「横 cols × 縦 rows 枚」の 128×128 に縮め（縦横比を保ち、余りは透明）、地図の色（基本色 × 明るさ 4 段）へ減らす。
// 減色は、誤差拡散（Floyd–Steinberg）か、一番近い色だけ。色の近さは、人の目に合わせた重みつきの RGB の距離。
// 書き出すのは gzip の NBT: { data: { colors[16384], scale 0, dimension, locked 1, ... }, DataVersion }。
use super::nbt::{self, Tag};

pub const SIZE: usize = 128;
/// 書き出す DataVersion（Java 26.3）。
pub const DATA_VERSION: i32 = 5023;
/// 一度に作る枚数の上限（横・縦それぞれ）。
pub const MAX_TILES: usize = 16;

/// 地図の基本色（番号 1〜61。0 は透明）。Minecraft の MapColor の値。
const BASE: [u32; 62] = [
    0x000000, 0x7FB238, 0xF7E9A3, 0xC7C7C7, 0xFF0000, 0xA0A0FF, 0xA7A7A7, 0x007C00, 0xFFFFFF, 0xA4A8B8, 0x976D4D, 0x707070, 0x4040FF, 0x8F7748, 0xFFFCF5, 0xD87F33,
    0xB24CD8, 0x6699D8, 0xE5E533, 0x7FCC19, 0xF27FA5, 0x4C4C4C, 0x999999, 0x4C7F99, 0x7F3FB2, 0x334CB2, 0x664C33, 0x667F33, 0x993333, 0x191919, 0xFAEE4D, 0x5CDBD5,
    0x4A80FF, 0x00D93A, 0x815631, 0x700200, 0xD1B1A1, 0x9F5224, 0x95576C, 0x706C8A, 0xBA8524, 0x677535, 0xA04D4E, 0x392923, 0x876B62, 0x575C5C, 0x7A4958, 0x4C3E5C,
    0x4C3223, 0x4C522A, 0x8E3C2E, 0x251610, 0xBD3031, 0x943F61, 0x5C191D, 0x167E86, 0x3A8E8C, 0x562C3E, 0x14B485, 0x646464, 0xD8AF93, 0x7FA796,
];
/// 明るさの 4 段（色の番号 = 基本色 × 4 + 段）。
const SHADE: [u32; 4] = [180, 220, 255, 135];

/// 地図の色の番号（4〜247）と、その RGB の一覧。
pub fn palette() -> Vec<(u8, [f32; 3])> {
    let mut out = Vec::with_capacity((BASE.len() - 1) * 4);
    for (i, &c) in BASE.iter().enumerate().skip(1) {
        for (s, &m) in SHADE.iter().enumerate() {
            let ch = |shift: u32| (((c >> shift) & 0xFF) * m / 255) as f32;
            out.push(((i * 4 + s) as u8, [ch(16), ch(8), ch(0)]));
        }
    }
    out
}

/// 地図の色の番号の RGB（0〜3 は透明なので None）。
pub fn color_of(id: u8) -> Option<[u8; 3]> {
    let (base, s) = (id as usize / 4, id as usize % 4);
    let c = *BASE.get(base).filter(|_| base > 0)?;
    let ch = |shift: u32| (((c >> shift) & 0xFF) * SHADE[s] / 255) as u8;
    Some([ch(16), ch(8), ch(0)])
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    // 赤の平均で重みを変える近似（"redmean"）。単純な RGB の距離より、人の目の見え方に近い
    let rm = (a[0] + b[0]) / 2.0;
    let (dr, dg, db) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db
}

fn nearest(pal: &[(u8, [f32; 3])], c: [f32; 3]) -> (u8, [f32; 3]) {
    let mut best = pal[0];
    let mut bd = f32::MAX;
    for &p in pal {
        let d = dist(c, p.1);
        if d < bd {
            bd = d;
            best = p;
        }
    }
    best
}

/// RGBA の画像（`w`×`h`、1 画素 4 バイト）を、横 `cols` × 縦 `rows` 枚の地図の色にする。返すのは左上から行の順の各枚の 128×128 の色の番号。
/// 縦横比を保って収め、余りと、半分以上透けている画素は透明（0）。
pub fn convert(rgba: &[u8], w: usize, h: usize, cols: usize, rows: usize, dither: bool) -> Result<Vec<Vec<u8>>, String> {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return Err("画像を読めませんでした".into());
    }
    if !(1..=MAX_TILES).contains(&cols) || !(1..=MAX_TILES).contains(&rows) {
        return Err(format!("枚数は横・縦とも 1〜{MAX_TILES} です"));
    }
    let (tw, th) = (cols * SIZE, rows * SIZE);
    // 収める大きさと、余りの位置（真ん中に置く）
    let scale = (tw as f64 / w as f64).min(th as f64 / h as f64);
    let (dw, dh) = (((w as f64 * scale).round() as usize).clamp(1, tw), ((h as f64 * scale).round() as usize).clamp(1, th));
    let (ox, oy) = ((tw - dw) / 2, (th - dh) / 2);
    // 縮める（面積の平均。拡大のときは最も近い画素）。work[i] = (r, g, b, 不透明か)
    let mut work: Vec<Option<[f32; 3]>> = vec![None; tw * th];
    for y in 0..dh {
        let (sy0, sy1) = (y * h / dh, ((y + 1) * h / dh).max(y * h / dh + 1).min(h));
        for x in 0..dw {
            let (sx0, sx1) = (x * w / dw, ((x + 1) * w / dw).max(x * w / dw + 1).min(w));
            let (mut acc, mut alpha, mut n) = ([0f64; 3], 0f64, 0f64);
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let p = &rgba[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4];
                    let a = p[3] as f64 / 255.0;
                    for k in 0..3 {
                        acc[k] += p[k] as f64 * a;
                    }
                    alpha += a;
                    n += 1.0;
                }
            }
            if n > 0.0 && alpha / n >= 0.5 {
                work[(y + oy) * tw + x + ox] = Some([(acc[0] / alpha) as f32, (acc[1] / alpha) as f32, (acc[2] / alpha) as f32]);
            }
        }
    }
    // 減色（全体で続けて誤差を拡散するので、枚の境目に継ぎ目が出ない）
    let pal = palette();
    let mut ids = vec![0u8; tw * th];
    for y in 0..th {
        for x in 0..tw {
            let Some(c) = work[y * tw + x] else { continue };
            let c = [c[0].clamp(0.0, 255.0), c[1].clamp(0.0, 255.0), c[2].clamp(0.0, 255.0)];
            let (id, got) = nearest(&pal, c);
            ids[y * tw + x] = id;
            if dither {
                let err = [c[0] - got[0], c[1] - got[1], c[2] - got[2]];
                let mut spread = |dx: isize, dy: usize, f: f32| {
                    let nx = x as isize + dx;
                    if nx < 0 || nx as usize >= tw || y + dy >= th {
                        return;
                    }
                    if let Some(p) = work[(y + dy) * tw + nx as usize].as_mut() {
                        for k in 0..3 {
                            p[k] += err[k] * f;
                        }
                    }
                };
                spread(1, 0, 7.0 / 16.0);
                spread(-1, 1, 3.0 / 16.0);
                spread(0, 1, 5.0 / 16.0);
                spread(1, 1, 1.0 / 16.0);
            }
        }
    }
    let mut tiles = Vec::with_capacity(cols * rows);
    for ty in 0..rows {
        for tx in 0..cols {
            let mut t = Vec::with_capacity(SIZE * SIZE);
            for y in 0..SIZE {
                let row = (ty * SIZE + y) * tw + tx * SIZE;
                t.extend_from_slice(&ids[row..row + SIZE]);
            }
            tiles.push(t);
        }
    }
    Ok(tiles)
}

/// 1 枚の地図のファイル（gzip の NBT）。動かない地図（locked）にして、ワールドの地形で上書きされないようにする。
pub fn map_file(colors: &[u8]) -> Vec<u8> {
    let b = |v: i8| Tag::Byte(v);
    let data = Tag::Compound(vec![
        ("scale".into(), b(0)),
        ("dimension".into(), Tag::String("minecraft:overworld".into())),
        ("trackingPosition".into(), b(0)),
        ("unlimitedTracking".into(), b(0)),
        ("locked".into(), b(1)),
        ("xCenter".into(), Tag::Int(0)),
        ("zCenter".into(), Tag::Int(0)),
        ("banners".into(), Tag::List(0, Vec::new())),
        ("frames".into(), Tag::List(0, Vec::new())),
        ("colors".into(), Tag::ByteArray(colors.iter().map(|&c| c as i8).collect())),
    ]);
    let root = Tag::Compound(vec![("data".into(), data), ("DataVersion".into(), Tag::Int(DATA_VERSION))]);
    nbt::pack(nbt::Compression::Gzip, &nbt::write("", &root))
}

/// 地図のファイルの NBT（`data` の下の `colors`、16384 バイト）から、色の番号を取り出す。地図でなければ None。
pub fn colors_of(root: &Tag) -> Option<Vec<u8>> {
    let field = |t: &'_ Tag, name: &str| -> Option<Tag> {
        match t {
            Tag::Compound(v) => v.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
            _ => None,
        }
    };
    let data = field(root, "data")?;
    match field(&data, "colors")? {
        Tag::ByteArray(b) if b.len() == SIZE * SIZE => Some(b.iter().map(|&c| c as u8).collect()),
        _ => None,
    }
}

/// 色の番号（128×128）を PNG にする（透明は透明のまま。1 画素を `zoom` 倍の四角で描く）。
pub fn to_png(colors: &[u8], zoom: usize) -> Vec<u8> {
    let zoom = zoom.clamp(1, 8);
    let side = SIZE * zoom;
    let mut raw = Vec::with_capacity((side * 4 + 1) * side);
    for y in 0..side {
        raw.push(0); // 行ごとのフィルタ: なし
        for x in 0..side {
            let id = colors.get((y / zoom) * SIZE + x / zoom).copied().unwrap_or(0);
            match color_of(id) {
                Some([r, g, b]) => raw.extend_from_slice(&[r, g, b, 255]),
                None => raw.extend_from_slice(&[0, 0, 0, 0]),
            }
        }
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&c);
        out.extend_from_slice(&super::deflate::crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(side as u32).to_be_bytes());
    ihdr.extend_from_slice(&(side as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 ビット・RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &super::deflate::zlib(&raw));
    chunk(b"IEND", &[]);
    out
}

/// 地図を渡すコマンド（番号ごとに 1 行）。
pub fn give_commands(first: i32, count: usize) -> String {
    (0..count).map(|i| format!("give @p minecraft:filled_map[minecraft:map_id={}]", first + i as i32)).collect::<Vec<_>>().join("\n")
}

/// 「2x3」「2×3」「2 3」「2」などを (横, 縦) に。
pub fn parse_tiles(s: &str) -> Option<(usize, usize)> {
    let parts: Vec<usize> = s.split(['x', 'X', '×', '*', ',', ' ']).filter(|p| !p.trim().is_empty()).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    let (c, r) = match parts.as_slice() {
        [n] => (*n, *n),
        [c, r] => (*c, *r),
        _ => return None,
    };
    ((1..=MAX_TILES).contains(&c) && (1..=MAX_TILES).contains(&r)).then_some((c, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        c.iter().copied().cycle().take(w * h * 4).collect()
    }

    #[test]
    fn palette_has_the_known_shades() {
        let p = palette();
        assert_eq!(p.len(), 61 * 4);
        assert_eq!(color_of(8 * 4 + 2), Some([255, 255, 255]), "雪の明るい段は白");
        assert_eq!(color_of(1 * 4 + 0), Some([(0x7Fu32 * 180 / 255) as u8, (0xB2u32 * 180 / 255) as u8, (0x38u32 * 180 / 255) as u8]));
        assert_eq!(color_of(3), None, "0〜3 は透明");
        assert_eq!(color_of(62 * 4), None);
    }

    #[test]
    fn solid_colors_map_to_the_exact_palette_entry() {
        let t = convert(&solid(10, 10, [255, 255, 255, 255]), 10, 10, 1, 1, true).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].len(), SIZE * SIZE);
        assert!(t[0].iter().all(|&c| c == 34), "白は雪の明るい段（8×4+2）");
        let t = convert(&solid(4, 4, [0, 0, 0, 0]), 4, 4, 1, 1, true).unwrap();
        assert!(t[0].iter().all(|&c| c == 0), "透明は 0");
    }

    #[test]
    fn aspect_is_kept_and_the_rest_is_transparent() {
        // 横長（2:1）を 1 枚へ: 上下 32 行ずつが透明
        let t = convert(&solid(200, 100, [255, 0, 0, 255]), 200, 100, 1, 1, false).unwrap();
        assert!(t[0][..32 * SIZE].iter().all(|&c| c == 0));
        assert!(t[0][64 * SIZE..65 * SIZE].iter().all(|&c| c != 0));
        // 2×1 枚なら隙間なく埋まる
        let t = convert(&solid(200, 100, [255, 0, 0, 255]), 200, 100, 2, 1, false).unwrap();
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|tile| tile.iter().all(|&c| c != 0)));
    }

    #[test]
    fn dithering_mixes_colors_for_an_in_between_gray() {
        // パレットに無い灰色: 誤差拡散では 2 色以上が混ざり、無しでは 1 色
        let g = solid(128, 128, [0x80, 0x80, 0x80, 255]);
        let plain = convert(&g, 128, 128, 1, 1, false).unwrap();
        let mixed = convert(&g, 128, 128, 1, 1, true).unwrap();
        let kinds = |t: &[u8]| t.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(kinds(&plain[0]), 1);
        assert!(kinds(&mixed[0]) >= 2);
    }

    #[test]
    fn map_file_is_a_readable_gzip_nbt() {
        let colors = vec![34u8; SIZE * SIZE];
        let f = map_file(&colors);
        let (c, raw) = nbt::unpack(&f).unwrap();
        assert_eq!(c, nbt::Compression::Gzip);
        let (_, root) = nbt::read(&raw).unwrap();
        let Tag::Compound(top) = root else { panic!() };
        assert!(top.iter().any(|(k, v)| k == "DataVersion" && *v == Tag::Int(DATA_VERSION)));
        let Some((_, Tag::Compound(data))) = top.iter().find(|(k, _)| k == "data") else { panic!() };
        let Some((_, Tag::ByteArray(b))) = data.iter().find(|(k, _)| k == "colors") else { panic!() };
        assert_eq!(b.len(), SIZE * SIZE);
        assert_eq!(b[0], 34);
    }

    #[test]
    fn map_files_read_back_and_become_a_png() {
        let mut colors = vec![0u8; SIZE * SIZE];
        colors[0] = 34;
        let (_, raw) = nbt::unpack(&map_file(&colors)).unwrap();
        let (_, root) = nbt::read(&raw).unwrap();
        assert_eq!(colors_of(&root), Some(colors.clone()));
        assert_eq!(colors_of(&Tag::Compound(vec![])), None);
        let png = to_png(&colors, 2);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&png[16..24], &[0, 0, 1, 0, 0, 0, 1, 0], "256×256");
        // IDAT を戻すと、左上は白・その右（3 画素目）は透明
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let px = super::super::deflate::unzlib(&png[41..41 + len], 1 << 20).unwrap();
        assert_eq!(&px[1..5], &[255, 255, 255, 255]);
        assert_eq!(&px[1 + 8..1 + 12], &[0, 0, 0, 0]);
    }

    #[test]
    fn tiles_and_commands_parse() {
        assert_eq!(parse_tiles("2x3"), Some((2, 3)));
        assert_eq!(parse_tiles("2×3"), Some((2, 3)));
        assert_eq!(parse_tiles(" 4 "), Some((4, 4)));
        assert_eq!(parse_tiles("0x1"), None);
        assert_eq!(parse_tiles("17"), None);
        assert_eq!(parse_tiles("a"), None);
        assert_eq!(give_commands(5, 2), "give @p minecraft:filled_map[minecraft:map_id=5]\ngive @p minecraft:filled_map[minecraft:map_id=6]");
    }
}
