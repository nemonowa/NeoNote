// 面積図の画像のファイルの四角に出す、縮めた画像（サムネイル）。
// 読み込みと画像の解読・縮小は裏のスレッド 1 本で行い（GUI を止めない）、できた物を画面側が受け取って描く。
// 縮めた画像は長い辺を `THUMB_MAX` 画素までにしてメモリだけに置き、上限を超えたら使われていない物から捨てる。
// 文書（木）が替わったら世代を進め、古い頼みは裏で捨てる。
use crate::neo::archive;
use fltk::image::RgbImage;
use fltk::prelude::ImageExt;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// 縮めた画像の長い辺の上限（画素）。
pub const THUMB_MAX: usize = 256;
/// 読む画像ファイルの大きさの上限。
pub const FILE_MAX: u64 = 32 << 20;
/// 置いておく量の上限（バイト）。
pub const CACHE_BYTES: usize = 64 << 20;
/// 同時に頼んでおく数の上限（描くたびに、大きく見えている物から頼む）。
pub const IN_FLIGHT: usize = 24;

/// どこから読むか。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// フォルダ（道筋はその下の相対パス）。
    Folder(PathBuf),
    /// zip・ディスクイメージなど、一覧で開いたファイル。
    Packed(PathBuf),
}

impl Source {
    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match self {
            Source::Folder(root) => {
                let full = rel.split('/').fold(root.clone(), |p, c| p.join(c));
                if std::fs::metadata(&full).ok()?.len() > FILE_MAX {
                    return None;
                }
                std::fs::read(full).ok()
            }
            Source::Packed(p) => {
                if let Some(kind) = archive::image_kind(p) {
                    return archive::image_read(p, kind, rel, FILE_MAX).ok();
                }
                if archive::is_zip_path(p) && std::fs::metadata(p).is_ok_and(|m| m.len() <= 256 << 20) {
                    let z = std::fs::read(p).ok()?;
                    return archive::read_entry(&z, rel, FILE_MAX as usize).ok();
                }
                None
            }
        }
    }
}

/// 縮めた画像（RGBA、1 画素 4 バイト）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// 画像として扱うファイルか（名前で見る。中身は読んだ時に確かめる）。
pub fn is_image_name(rel: &str) -> bool {
    let p = std::path::Path::new(rel);
    crate::winmedia::is_video_name(rel) || crate::neo::imagefile::is_image_path(p) || p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| matches!(e.as_str(), "ico" | "cur" | "icns" | "dat" | "webp" | "tif" | "tiff" | "tga" | "dds" | "psd" | "svg" | "svgz"))
}

/// 面積の平均で縮める（長い辺を `max` まで。もともと小さければそのまま）。
pub fn downscale(rgba: &[u8], w: usize, h: usize, max: usize) -> Pixels {
    let s = (max as f64 / w.max(h).max(1) as f64).min(1.0);
    let (nw, nh) = (((w as f64 * s).round() as usize).max(1), ((h as f64 * s).round() as usize).max(1));
    if (nw, nh) == (w, h) {
        return Pixels { w, h, rgba: rgba[..w * h * 4].to_vec() };
    }
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1).min(h));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1).min(w));
            let mut acc = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = (sy * w + sx) * 4;
                    let a = rgba[p + 3] as u64;
                    for k in 0..3 {
                        acc[k] += rgba[p + k] as u64 * a;
                    }
                    acc[3] += a;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            let o = (y * nw + x) * 4;
            for k in 0..3 {
                out[o + k] = acc[k].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / n.max(1)) as u8;
        }
    }
    Pixels { w: nw, h: nh, rgba: out }
}

/// 画像（`iw`×`ih`）を、四角 `(x, y, w, h)` に縦横比を保って収めたときの位置と大きさ（真ん中に置く）。
pub fn fit_rect(iw: usize, ih: usize, (x, y, w, h): (i32, i32, i32, i32)) -> Option<(i32, i32, i32, i32)> {
    if iw == 0 || ih == 0 || w < 2 || h < 2 {
        return None;
    }
    let s = (w as f64 / iw as f64).min(h as f64 / ih as f64);
    let (dw, dh) = (((iw as f64 * s).round() as i32).clamp(1, w), ((ih as f64 * s).round() as i32).clamp(1, h));
    Some((x + (w - dw) / 2, y + (h - dh) / 2, dw, dh))
}

/// 画像のバイト列を解読して縮める（裏のスレッドで呼ぶ）。`name` の拡張子も見る（印の無い tga・svg のため）。読めなければ None。
pub fn decode_named(bytes: &[u8], name: &str) -> Option<Pixels> {
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    let p = if crate::neo::imagefile::sniff(bytes).is_some() {
        decode_full(bytes)
    } else if let Some(r) = crate::neo::imgdecode::decode(bytes, &ext) {
        Some(Pixels { w: r.w, h: r.h, rgba: r.data })
    } else if ext == "svg" || ext == "svgz" {
        svg(bytes)
    } else {
        decode_other(bytes)
    }?;
    Some(downscale(&p.rgba, p.w, p.h, THUMB_MAX))
}

/// SVG は FLTK（nanosvg）で描いて読む。svgz は gzip を解いてから。
fn svg(bytes: &[u8]) -> Option<Pixels> {
    use fltk::prelude::ImageExt;
    let text = if bytes.starts_with(&[0x1F, 0x8B]) {
        let (_, raw) = crate::neo::nbt::unpack(bytes).ok()?;
        String::from_utf8(raw).ok()?
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let mut img = fltk::image::SvgImage::from_data(&text).ok()?;
    let (w, h) = (img.data_w(), img.data_h());
    if w <= 0 || h <= 0 {
        return None;
    }
    let s = (THUMB_MAX as f64 / w.max(h) as f64).min(4.0);
    img.scale(((w as f64 * s) as i32).max(1), ((h as f64 * s) as i32).max(1), true, true);
    img.normalize();
    decode_rgb(img.to_rgb_data(), img.data_w(), img.data_h(), img.depth() as usize)
}

fn decode_rgb(data: Vec<u8>, w: i32, h: i32, depth: usize) -> Option<Pixels> {
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    if w == 0 || h == 0 || !(1..=4).contains(&depth) || data.len() < w * h * depth {
        return None;
    }
    let rgba = data
        .chunks_exact(depth)
        .take(w * h)
        .flat_map(|p| match depth {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    Some(Pixels { w, h, rgba })
}

/// 画像のバイト列を、縮めずに RGBA へ解読する（png・jpg・gif・bmp）。読めなければ None。
pub fn decode_full(bytes: &[u8]) -> Option<Pixels> {
    use fltk::image::{BmpImage, GifImage, JpegImage, PngImage};
    use fltk::prelude::ImageExt;
    fn parts<I: ImageExt>(i: &I) -> (Vec<u8>, i32, i32, usize) {
        (i.to_rgb_data(), i.data_w(), i.data_h(), i.depth() as usize)
    }
    let Some(kind) = crate::neo::imagefile::sniff(bytes) else { return decode_other(bytes) };
    let (data, w, h, depth) = match kind {
        "png" => parts(&PngImage::from_data(bytes).ok()?),
        "jpg" => parts(&JpegImage::from_data(bytes).ok()?),
        "bmp" => parts(&BmpImage::from_data(bytes).ok()?),
        // GIF は FLTK では Pixmap（中身は文字列の表）なので、画素の配列として読むと範囲の外を読んで落ちる。RGB に作り直してから読む
        "gif" => parts(&gif_to_rgb(&GifImage::from_data(bytes).ok()?)?),
        _ => return None,
    };
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    if w == 0 || h == 0 || !(1..=4).contains(&depth) || data.len() < w * h * depth {
        return None;
    }
    let rgba: Vec<u8> = data
        .chunks_exact(depth)
        .take(w * h)
        .flat_map(|p| match depth {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    Some(Pixels { w, h, rgba })
}

/// 画像ではないが絵を持つ物: アイコン（ico・cur・icns の一番大きな絵）と Minecraft の地図（map_N.dat）。
fn decode_other(bytes: &[u8]) -> Option<Pixels> {
    if let Some(img) = crate::neo::iconfile::largest_image(bytes) {
        return crate::neo::imagefile::sniff(&img).and_then(|_| decode_full(&img));
    }
    let (_, raw) = crate::neo::nbt::unpack(bytes).ok()?;
    let (_, root) = crate::neo::nbt::read(&raw).ok()?;
    let colors = crate::neo::mapart::colors_of(&root)?;
    decode_full(&crate::neo::mapart::to_png(&colors, 1))
}

fn gif_to_rgb(gif: &fltk::image::GifImage) -> Option<RgbImage> {
    use fltk::prelude::ImageExt;
    let pix = fltk::image::Pixmap::from_dyn_image(gif)?;
    let rgb = RgbImage::from_pixmap(&pix);
    // pix は gif と同じ物を指す別の持ち手。落とすと gif と二重に消すので、消さずに手放す
    std::mem::forget(pix);
    Some(rgb)
}

/// 動画は、フォルダの中の物だけ 1 コマを取り出す（OS の解読に道筋を渡すため。zip の中は読まない）。
fn video_frame(src: &Source, rel: &str) -> Option<Pixels> {
    let Source::Folder(root) = src else { return None };
    if !crate::winmedia::is_video_name(rel) {
        return None;
    }
    let f = crate::winmedia::frame(&rel.split('/').fold(root.clone(), |p, c| p.join(c)))?;
    Some(downscale(&f.rgba, f.w, f.h, THUMB_MAX))
}

type Request = (u64, Arc<Source>, String);
type Done = (u64, String, Option<Pixels>);

/// 縮めた画像の読み込みと、置き場所（画面のスレッドで使う）。
pub struct Thumbs {
    source: Option<Arc<Source>>,
    gen: Arc<AtomicU64>,
    tx: Option<Sender<Request>>,
    rx: Option<Receiver<Done>>,
    asked: HashSet<String>,
    /// 道筋 → (画像。読めなかった物は None, 最後に使った順)
    map: HashMap<String, (Option<RgbImage>, u64)>,
    tick: u64,
    bytes: usize,
}

impl Default for Thumbs {
    fn default() -> Self {
        Thumbs { source: None, gen: Arc::new(AtomicU64::new(0)), tx: None, rx: None, asked: HashSet::new(), map: HashMap::new(), tick: 0, bytes: 0 }
    }
}

impl Thumbs {
    /// 読む所を替える（木が替わった時）。覚えていた物と、頼みかけの物は捨てる。
    pub fn set_source(&mut self, source: Option<Source>) {
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.source = source.map(Arc::new);
        self.asked.clear();
        self.map.clear();
        self.bytes = 0;
    }

    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }

    /// 頼んで、まだ届いていない物があるか（あれば少し後に描き直す）。
    pub fn waiting(&self) -> bool {
        !self.asked.is_empty()
    }

    fn worker(&mut self) -> Option<&Sender<Request>> {
        if self.tx.is_none() {
            // macOS の FLTK は、画像の読み込みの初回に画面を開く。主のスレッドで先に済ませる（裏で初回になると落ちる）
            if fltk::app::lock().is_ok() {
                fltk::app::unlock();
            }
            let (tx, rx) = channel::<Request>();
            let (dtx, drx) = channel::<Done>();
            let gen = Arc::clone(&self.gen);
            let spawned = std::thread::Builder::new().name("map-thumbs".into()).spawn(move || {
                while let Ok((g, src, rel)) = rx.recv() {
                    // 文書が替わった後の古い頼みは読まない
                    if g != gen.load(Ordering::SeqCst) {
                        continue;
                    }
                    let px = video_frame(&src, &rel).or_else(|| src.read(&rel).and_then(|b| decode_named(&b, &rel)));
                    if dtx.send((g, rel, px)).is_err() {
                        break;
                    }
                }
            });
            if spawned.is_err() {
                return None;
            }
            self.tx = Some(tx);
            self.rx = Some(drx);
        }
        self.tx.as_ref()
    }

    /// 届いた物を受け取る。受け取った物があれば true（描き直す）。
    pub fn receive(&mut self) -> bool {
        let mut got = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(d) = rx.try_recv() {
                got.push(d);
            }
        }
        let now = self.gen.load(Ordering::SeqCst);
        let mut any = false;
        for (g, rel, px) in got {
            if g != now {
                continue;
            }
            self.asked.remove(&rel);
            let img = px.and_then(|p| {
                let cost = p.rgba.len();
                RgbImage::new(&p.rgba, p.w as i32, p.h as i32, fltk::enums::ColorDepth::Rgba8).ok().map(|i| (i, cost))
            });
            let cost = img.as_ref().map_or(64, |(_, c)| *c);
            self.tick += 1;
            self.map.insert(rel, (img.map(|(i, _)| i), self.tick));
            self.bytes += cost;
            any = true;
        }
        self.evict();
        any
    }

    fn evict(&mut self) {
        while self.bytes > CACHE_BYTES && !self.map.is_empty() {
            let Some(old) = self.map.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone()) else { break };
            if let Some((img, _)) = self.map.remove(&old) {
                self.bytes = self.bytes.saturating_sub(img.map_or(64, |i| (i.data_w() * i.data_h() * 4) as usize));
            }
        }
    }

    /// 描くのに使う画像（読めなかった・まだの物は None）。
    pub fn get(&mut self, rel: &str) -> Option<&mut RgbImage> {
        self.tick += 1;
        let t = self.tick;
        let e = self.map.get_mut(rel)?;
        e.1 = t;
        e.0.as_mut()
    }

    /// `want`（見えている大きさの順）のうち、まだ頼んでいない物を頼む。
    pub fn request(&mut self, want: &[String]) {
        let Some(src) = self.source.clone() else { return };
        let g = self.gen.load(Ordering::SeqCst);
        for rel in want {
            if self.asked.len() >= IN_FLIGHT {
                break;
            }
            if self.map.contains_key(rel) || self.asked.contains(rel) {
                continue;
            }
            let Some(tx) = self.worker() else { return };
            if tx.send((g, Arc::clone(&src), rel.clone())).is_ok() {
                self.asked.insert(rel.clone());
            }
        }
    }

    pub fn contains(&self, rel: &str) -> bool {
        self.map.contains_key(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_keeps_size_limit_and_average() {
        // 4×2 の白黒の縞を 2×1 へ: 平均で灰色
        let mut px = Vec::new();
        for i in 0..8 {
            let v = if i % 2 == 0 { 255 } else { 0 };
            px.extend([v, v, v, 255]);
        }
        let d = downscale(&px, 4, 2, 2);
        assert_eq!((d.w, d.h), (2, 1));
        assert_eq!(&d.rgba[..4], &[127, 127, 127, 255]);
        let big = downscale(&vec![9u8; 1000 * 500 * 4], 1000, 500, THUMB_MAX);
        assert_eq!((big.w, big.h), (256, 128));
        let small = downscale(&[1, 2, 3, 4], 1, 1, THUMB_MAX);
        assert_eq!(small.rgba, [1, 2, 3, 4], "小さい物はそのまま");
        // 透明な画素は色の平均に入れない
        let d = downscale(&[255, 0, 0, 255, 0, 0, 255, 0], 2, 1, 1);
        assert_eq!(&d.rgba, &[255, 0, 0, 127]);
    }

    #[test]
    fn fit_keeps_aspect_and_centers() {
        assert_eq!(fit_rect(200, 100, (0, 0, 100, 100)), Some((0, 25, 100, 50)));
        assert_eq!(fit_rect(100, 200, (10, 10, 100, 100)), Some((35, 10, 50, 100)));
        assert_eq!(fit_rect(0, 5, (0, 0, 10, 10)), None);
        assert_eq!(fit_rect(5, 5, (0, 0, 1, 10)), None);
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore = "macOS の FLTK は主のスレッド以外で画像を読むと落ちる（テストは主のスレッドで動かない）")]
    fn decodes_a_png_and_ignores_other_files() {
        let mut colors = vec![0u8; crate::neo::mapart::SIZE * crate::neo::mapart::SIZE];
        colors[0] = 34; // 左上だけ白、ほかは透明
        let png = crate::neo::mapart::to_png(&colors, 1);
        let p = decode_named(&png, "").unwrap();
        assert_eq!((p.w, p.h), (128, 128));
        assert_eq!(&p.rgba[..4], &[255, 255, 255, 255]);
        assert_eq!(p.rgba[7], 0, "透明はそのまま");
        assert!(decode_named(b"not an image", "").is_none());
        // 1×1 の赤い GIF（FLTK では Pixmap になる形式）
        let gif: &[u8] = &[71, 73, 70, 56, 57, 97, 1, 0, 1, 0, 128, 0, 0, 255, 0, 0, 0, 0, 0, 33, 249, 4, 0, 0, 0, 0, 0, 44, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 68, 1, 0, 59];
        let g = decode_named(gif, "").unwrap();
        assert_eq!((g.w, g.h), (1, 1));
        assert_eq!(&g.rgba[..3], &[255, 0, 0]);
        assert!(is_image_name("a/b.PNG") && !is_image_name("a/b.txt"));
    }

    #[test]
    fn folder_source_reads_relative_paths() {
        let dir = std::env::temp_dir().join(format!("neonote-thumb-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("x.bin"), b"abc").unwrap();
        let s = Source::Folder(dir.clone());
        assert_eq!(s.read("sub/x.bin"), Some(b"abc".to_vec()));
        assert_eq!(s.read("none"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
