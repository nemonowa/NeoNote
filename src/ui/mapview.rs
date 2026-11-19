// 面積図を「見る」側の計算（GUI なし）。拡大・移動・小さな項目の畳み込み・詳細度を決める。
//
// 方針は 2 つ。
// 1. 並べ方（`treemap::squarify_rows`）は倍率に依らず一定。倍率で変わるのは「描く・描かない」だけなので、
//    拡大しても四角の位置が揺れない（カメラを動かすだけで済み、毎フレームの並べ直しがいらない）。
// 2. 画面に出る分だけ作る。画面の外の枝は降りず、小さすぎる項目は 1〜2 個の「ほか N 件」にまとめる。
//    作る四角の数は、見えている画素の数で抑えられる（ファイルが何十万あっても）。
//
// 座標: 「世界」は倍率 1 のときの画面（面積図の枠の中）。画面 = 世界 × scale + (ox, oy)。
//
// 画面への接続（ホイール・ドラッグ・描画）は次の段階。それまでは使われないので dead_code を許す。
#![allow(dead_code)]
use super::treemap::{squarify_rows, Node, Rect};

/// 倍率の下限（枠に全体が収まる大きさ）と上限。上限は f64 の精度に十分な余裕がある範囲（テストで確かめる）。
pub const MIN_SCALE: f64 = 1.0;
pub const MAX_SCALE: f64 = 1.0e7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scale: f64,
    pub ox: f64,
    pub oy: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::FIT
    }
}

impl Camera {
    /// 全体を枠に収めた状態。
    pub const FIT: Camera = Camera { scale: 1.0, ox: 0.0, oy: 0.0 };

    pub fn screen_point(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.ox, y * self.scale + self.oy)
    }

    pub fn world_point(&self, sx: f64, sy: f64) -> (f64, f64) {
        ((sx - self.ox) / self.scale, (sy - self.oy) / self.scale)
    }

    pub fn screen_rect(&self, r: Rect) -> Rect {
        Rect { x: r.x * self.scale + self.ox, y: r.y * self.scale + self.oy, w: r.w * self.scale, h: r.h * self.scale }
    }

    /// 画面の (sx, sy) の下にある世界の点を動かさずに、`factor` 倍する（ホイールでカーソルを中心に拡大）。
    pub fn zoom_at(&mut self, sx: f64, sy: f64, factor: f64) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let next = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let f = next / self.scale;
        self.ox = sx - (sx - self.ox) * f;
        self.oy = sy - (sy - self.oy) * f;
        self.scale = next;
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.ox += dx;
        self.oy += dy;
    }

    /// 枠 `frame`（倍率 1 の面積図の置き場所）の外へ、画面を出させない。
    pub fn clamp(&mut self, frame: Rect) {
        let s = self.scale;
        // 内容 = frame を s 倍して (ox, oy) だけずらしたもの。これが frame を覆い続ける範囲に収める。
        self.ox = self.ox.clamp((frame.x + frame.w) * (1.0 - s), frame.x * (1.0 - s));
        self.oy = self.oy.clamp((frame.y + frame.h) * (1.0 - s), frame.y * (1.0 - s));
    }

    /// 画面 `frame` に映っている世界の範囲。
    pub fn visible_world(&self, frame: Rect) -> Rect {
        let (x, y) = self.world_point(frame.x, frame.y);
        Rect { x, y, w: frame.w / self.scale, h: frame.h / self.scale }
    }

    /// 世界の四角 `target` が `frame` いっぱいに（`margin` 画素の余白つきで）映るカメラ（ダブルクリックで合わせる）。
    pub fn focus(frame: Rect, target: Rect, margin: f64) -> Camera {
        let tw = target.w.max(1e-9);
        let th = target.h.max(1e-9);
        let scale = ((frame.w - 2.0 * margin).max(1.0) / tw).min((frame.h - 2.0 * margin).max(1.0) / th).clamp(MIN_SCALE, MAX_SCALE);
        let (cx, cy) = (target.x + target.w / 2.0, target.y + target.h / 2.0);
        let mut cam = Camera { scale, ox: frame.x + frame.w / 2.0 - cx * scale, oy: frame.y + frame.h / 2.0 - cy * scale };
        cam.clamp(frame);
        cam
    }
}

/// ホイール 1 目盛り（`notches`。上向きが正）の倍率。
pub fn wheel_factor(notches: f64) -> f64 {
    1.25f64.powf(notches)
}

/// 四角の種類。`More(n)` は、小さすぎて 1 つずつは描けない n 個の項目をまとめた四角。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    More(usize),
}

/// 描く四角 1 つ。`r` は世界の座標。`path` は `/` 区切り（`More` は、まとめた項目の親のパス）。
#[derive(Debug)]
pub struct VCell {
    pub r: Rect,
    pub kind: Kind,
    pub depth: u32,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// フォルダの枠の太さ（世界の単位。小さなフォルダでは自動で細くなる）。
    pub pad: f64,
    /// 画面でこれより小さい四角は描かない（画素）。
    pub min: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { pad: 2.0, min: 3.0 }
    }
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn bbox(rs: &[Rect]) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for r in rs {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x + r.w);
        y1 = y1.max(r.y + r.h);
    }
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// 木を世界の枠 `frame` に並べ、カメラ `cam` で見えている四角だけを返す（親が先、子が後）。
/// 並べ方は倍率に依らないので、同じ項目の `r` は倍率が違っても同じ値になる。
pub fn layout_view(root: &Node, frame: Rect, cam: &Camera, opts: &Opts) -> Vec<VCell> {
    let mut out = Vec::new();
    let vis = cam.visible_world(frame);
    place(root, frame, cam, &vis, opts, 0, "", &mut out);
    out
}

fn is_tiny(r: &Rect, scale: f64, min: f64) -> bool {
    r.w * scale < min || r.h * scale < min
}

#[allow(clippy::too_many_arguments)]
fn place(n: &Node, r: Rect, cam: &Camera, vis: &Rect, opts: &Opts, depth: u32, prefix: &str, out: &mut Vec<VCell>) {
    if n.children.is_empty() {
        return;
    }
    let s = cam.scale;
    let sizes: Vec<u64> = n.children.iter().map(|c| c.size).collect();
    let rows = squarify_rows(&sizes, r);

    // 後ろから見て、小さすぎる四角が続く所までを「小さな項目」にする（大きい順なので、尾の側）。
    let mut k = rows.len();
    while k > 0 && is_tiny(&rows[k - 1].0, s, opts.min) {
        k -= 1;
    }

    for (c, (cr, _)) in n.children[..k].iter().zip(&rows[..k]) {
        if !overlaps(cr, vis) || is_tiny(cr, s, opts.min) {
            continue;
        }
        let path = if prefix.is_empty() { c.name.clone() } else { format!("{prefix}/{}", c.name) };
        out.push(VCell { r: *cr, kind: if c.is_dir { Kind::Dir } else { Kind::File }, depth, path: path.clone(), size: c.size });
        if c.is_dir {
            // 枠は自分の短い辺の 6% まで。小さなフォルダでも、拡大すれば中が現れる。
            let pad = opts.pad.min(cr.w.min(cr.h) * 0.06);
            let inner = Rect { x: cr.x + pad, y: cr.y + pad, w: cr.w - 2.0 * pad, h: cr.h - 2.0 * pad };
            if !is_tiny(&inner, s, opts.min) {
                place(c, inner, cam, vis, opts, depth + 1, &path, out);
            }
        }
    }

    // 尾は、最初の列の途中から始まるなら「その列の残り」と「それより後ろの列（1 つの長方形）」の最大 2 つにまとめる。
    if rows.len() - k >= 2 {
        let first_row = rows[k].1;
        let split = k > 0 && rows[k - 1].1 == first_row;
        let groups: [std::ops::Range<usize>; 2] = if split {
            let m = rows[k..].iter().position(|(_, row)| *row != first_row).map_or(rows.len(), |p| k + p);
            [k..m, m..rows.len()]
        } else {
            [k..rows.len(), rows.len()..rows.len()]
        };
        for g in groups {
            if g.is_empty() {
                continue;
            }
            let rects: Vec<Rect> = rows[g.clone()].iter().map(|(c, _)| *c).collect();
            let b = bbox(&rects);
            if !overlaps(&b, vis) || is_tiny(&b, s, opts.min) {
                continue;
            }
            let size: u64 = n.children[g.clone()].iter().map(|c| c.size).sum();
            out.push(VCell { r: b, kind: Kind::More(g.len()), depth, path: prefix.to_string(), size });
        }
    }
}

/// `path`（`/` 区切り）の項目が、世界の枠 `frame` に並べたときに占める四角（`layout_view` と同じ並べ方）。
/// 見つからない（大きさ 0 で並ばない項目など）ときは None。
pub fn rect_of(root: &Node, frame: Rect, path: &str, opts: &Opts) -> Option<Rect> {
    let mut n = root;
    let mut r = frame;
    let mut parts = path.split('/').filter(|s| !s.is_empty()).peekable();
    parts.peek()?;
    while let Some(part) = parts.next() {
        let i = n.children.iter().position(|c| c.name == part)?;
        let sizes: Vec<u64> = n.children.iter().map(|c| c.size).collect();
        let cr = squarify_rows(&sizes, r).get(i)?.0;
        if parts.peek().is_none() {
            return Some(cr);
        }
        let pad = opts.pad.min(cr.w.min(cr.h) * 0.06);
        r = Rect { x: cr.x + pad, y: cr.y + pad, w: cr.w - 2.0 * pad, h: cr.h - 2.0 * pad };
        n = &n.children[i];
    }
    None
}

/// 画面の (sx, sy) にある、いちばん奥の四角の番号。
pub fn hit(cells: &[VCell], cam: &Camera, sx: f64, sy: f64) -> Option<usize> {
    let (x, y) = cam.world_point(sx, sy);
    cells.iter().rposition(|c| x >= c.r.x && x < c.r.x + c.r.w && y >= c.r.y && y < c.r.y + c.r.h)
}

/// 四角の画面上の大きさから決める、見せ方の段階。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lod {
    /// 色だけ。
    Dot,
    /// 名前と大きさ。
    Name,
    /// 中身の質感（文字の行を細い棒で、画像は縮小）。
    Texture,
    /// 実際の文字。
    Text,
}

impl Lod {
    /// 画面上の幅 `w`・高さ `h`（画素）と、文字の高さ `font_h`（画素）から段階を決める。数値は提案値で、実機で調整する。
    /// `Text` は 8 行・約 30 文字が入る大きさ（かつ `Texture` の下限以上）。これより小さいと、文字では先頭の数文字しか
    /// 見えないので、全体の形が分かる棒（`Texture`）のほうが役に立つ。
    pub fn of(w: f64, h: f64, font_h: f64) -> Lod {
        if h >= (8.0 * font_h).max(60.0) && w >= (15.0 * font_h).max(120.0) {
            Lod::Text
        } else if w >= 120.0 && h >= 60.0 {
            Lod::Texture
        } else if w >= 60.0 && h >= 14.0 {
            Lod::Name
        } else {
            Lod::Dot
        }
    }
}

/// 全文が文字の大きさで収まる、おおよその倍率（長さの倍率）。面積はバイト数に比例するので、
/// 1 バイトを 1 文字分（`glyph_px2` 画素²）で並べられる倍率になる。短い行の余白の分、実際はもう少し要る。
pub fn readable_scale(total_bytes: u64, area_px2: f64, glyph_px2: f64) -> f64 {
    if area_px2 <= 0.0 {
        return MIN_SCALE;
    }
    (glyph_px2 * total_bytes as f64 / area_px2).sqrt().max(MIN_SCALE)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rect_of_matches_the_cells_of_layout_view() {
        let t = crate::ui::treemap::build(&[
            crate::neo::archive::Entry { path: "a/x.txt".into(), is_dir: false, size: 300, packed: None },
            crate::neo::archive::Entry { path: "a/y.txt".into(), is_dir: false, size: 100, packed: None },
            crate::neo::archive::Entry { path: "b.bin".into(), is_dir: false, size: 200, packed: None },
        ]);
        let frame = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };
        let cells = layout_view(&t, frame, &Camera::FIT, &Opts::default());
        for c in cells.iter().filter(|c| !matches!(c.kind, Kind::More(_))) {
            assert_eq!(rect_of(&t, frame, &c.path, &Opts::default()), Some(c.r), "{}", c.path);
        }
        assert_eq!(rect_of(&t, frame, "a/none", &Opts::default()), None);
        assert_eq!(rect_of(&t, frame, "", &Opts::default()), None);
    }

    use super::*;

    // 乱数（テスト用。依存を足さない）。
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    fn file(name: String, size: u64) -> Node {
        Node { name, size, is_dir: false, children: Vec::new() }
    }

    /// 重い裾野（小さいファイルが多い）を持つ、深さ `depth` の木。
    fn tree(rng: &mut Lcg, depth: u32, fan: u64) -> Node {
        let n = 1 + rng.below(fan) as usize;
        let mut kids: Vec<Node> = Vec::with_capacity(n);
        for i in 0..n {
            if depth > 0 && rng.below(4) == 0 {
                let mut d = tree(rng, depth - 1, fan);
                d.name = format!("d{i}");
                kids.push(d);
            } else {
                let e = 1 + rng.below(17); // 大小の差が大きい（1 〜 2^17 バイト）
                kids.push(file(format!("f{i}"), 1 + rng.next() % (1u64 << e)));
            }
        }
        let mut dir = Node { name: String::new(), size: 0, is_dir: true, children: kids };
        finish(&mut dir);
        dir
    }

    fn finish(n: &mut Node) -> u64 {
        if n.is_dir {
            n.size = n.children.iter_mut().map(finish).sum();
            n.children.sort_by_key(|a| std::cmp::Reverse(a.size));
        }
        n.size
    }

    const FRAME: Rect = Rect { x: 10.0, y: 20.0, w: 400.0, h: 300.0 };

    fn inside(inner: &Rect, outer: &Rect) -> bool {
        inner.x >= outer.x - 1e-6 && inner.y >= outer.y - 1e-6 && inner.x + inner.w <= outer.x + outer.w + 1e-6 && inner.y + inner.h <= outer.y + outer.h + 1e-6
    }

    fn overlap_area(a: &Rect, b: &Rect) -> f64 {
        let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
        let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }

    /// 同じ親の直下の四角（親のパスが同じで、深さが同じ）。
    fn siblings<'a>(cells: &'a [VCell], parent: &str, depth: u32) -> Vec<&'a VCell> {
        cells
            .iter()
            .filter(|c| c.depth == depth && match c.kind {
                Kind::More(_) => c.path == parent,
                _ => c.path.rsplit_once('/').map_or("", |(p, _)| p) == parent,
            })
            .collect()
    }

    #[test]
    fn camera_zoom_keeps_point_under_cursor() {
        let mut cam = Camera::FIT;
        let (sx, sy) = (123.0, 77.0);
        let w0 = cam.world_point(sx, sy);
        for f in [1.25, 3.0, 0.5, 8.0, 1.25] {
            cam.zoom_at(sx, sy, f);
            let w = cam.world_point(sx, sy);
            assert!((w.0 - w0.0).abs() < 1e-9 && (w.1 - w0.1).abs() < 1e-9);
        }
    }

    #[test]
    fn camera_scale_is_limited_and_bad_factors_are_ignored() {
        let mut cam = Camera::FIT;
        cam.zoom_at(0.0, 0.0, 0.001);
        assert_eq!(cam.scale, MIN_SCALE);
        cam.zoom_at(0.0, 0.0, 1e30);
        assert_eq!(cam.scale, MAX_SCALE);
        let before = cam;
        cam.zoom_at(0.0, 0.0, f64::NAN);
        cam.zoom_at(0.0, 0.0, -2.0);
        cam.zoom_at(0.0, 0.0, 0.0);
        assert_eq!(cam, before);
    }

    #[test]
    fn camera_clamp_keeps_the_frame_covered() {
        let mut cam = Camera::FIT;
        cam.zoom_at(200.0, 150.0, 4.0);
        cam.pan(1e6, -1e6);
        cam.clamp(FRAME);
        let c = cam.screen_rect(FRAME);
        assert!(c.x <= FRAME.x + 1e-9 && c.y <= FRAME.y + 1e-9);
        assert!(c.x + c.w >= FRAME.x + FRAME.w - 1e-9 && c.y + c.h >= FRAME.y + FRAME.h - 1e-9);
        let mut fit = Camera::FIT;
        fit.pan(50.0, 50.0);
        fit.clamp(FRAME);
        assert_eq!(fit, Camera::FIT, "倍率 1 では動かせない");
    }

    #[test]
    fn camera_focus_shows_the_target() {
        let target = Rect { x: 100.0, y: 80.0, w: 40.0, h: 30.0 };
        let cam = Camera::focus(FRAME, target, 4.0);
        let t = cam.screen_rect(target);
        assert!(inside(&t, &FRAME));
        assert!(cam.scale > 5.0);
    }

    #[test]
    fn deep_zoom_stays_precise() {
        let mut cam = Camera::FIT;
        let (sx, sy) = (205.0, 170.0);
        let w0 = cam.world_point(sx, sy);
        cam.zoom_at(sx, sy, 1.0e7);
        assert_eq!(cam.scale, MAX_SCALE);
        let w = cam.world_point(sx, sy);
        // 世界の誤差を画素に直しても 1e-3 画素に収まる。
        assert!(((w.0 - w0.0) * cam.scale).abs() < 1e-3 && ((w.1 - w0.1) * cam.scale).abs() < 1e-3);
    }

    #[test]
    fn layout_is_inside_parent_and_siblings_do_not_overlap() {
        for seed in 1..=8u64 {
            let mut rng = Lcg(seed);
            let t = tree(&mut rng, 3, 60);
            let cells = layout_view(&t, FRAME, &Camera::FIT, &Opts::default());
            assert!(!cells.is_empty());
            for c in &cells {
                assert!(inside(&c.r, &FRAME), "seed {seed}: {} が枠の外", c.path);
            }
            // 兄弟どうしは重ならない（浮動小数点の誤差だけ許す）。
            let mut keys: Vec<(String, u32)> = cells.iter().map(|c| (match c.kind { Kind::More(_) => c.path.clone(), _ => c.path.rsplit_once('/').map_or(String::new(), |(p, _)| p.to_string()) }, c.depth)).collect();
            keys.sort();
            keys.dedup();
            for (parent, depth) in keys {
                let sib = siblings(&cells, &parent, depth);
                for (i, a) in sib.iter().enumerate() {
                    for b in &sib[i + 1..] {
                        assert!(overlap_area(&a.r, &b.r) < 1e-6, "seed {seed}: {} と {} が重なる", a.path, b.path);
                    }
                }
            }
            // 子は親の枠の内側。
            for c in cells.iter().filter(|c| c.kind != Kind::File) {
                if c.kind == Kind::Dir {
                    for k in cells.iter().filter(|k| k.depth == c.depth + 1 && k.path.starts_with(&format!("{}/", c.path)) && k.path[c.path.len() + 1..].find('/').is_none()) {
                        assert!(inside(&k.r, &c.r), "seed {seed}: {} が親 {} の外", k.path, c.path);
                    }
                }
            }
        }
    }

    #[test]
    fn layout_is_independent_of_zoom() {
        // 倍率が違っても、両方に出る四角の位置は同じ（拡大しても揺れない）。
        let mut rng = Lcg(42);
        let t = tree(&mut rng, 3, 80);
        let a = layout_view(&t, FRAME, &Camera::FIT, &Opts::default());
        let mut cam = Camera::FIT;
        cam.zoom_at(FRAME.x + 100.0, FRAME.y + 80.0, 6.0);
        cam.clamp(FRAME);
        let b = layout_view(&t, FRAME, &cam, &Opts::default());
        let mut same = 0;
        for cb in b.iter().filter(|c| c.kind == Kind::File || c.kind == Kind::Dir) {
            if let Some(ca) = a.iter().find(|c| c.path == cb.path && c.kind == cb.kind) {
                assert_eq!(ca.r, cb.r, "{} の位置が倍率で変わった", cb.path);
                same += 1;
            }
        }
        assert!(same > 5, "比べられる四角が少なすぎる: {same}");
        // 拡大すると、倍率 1 では描かれなかった四角が現れる。
        assert!(b.iter().any(|c| (c.kind == Kind::File || c.kind == Kind::Dir) && !a.iter().any(|x| x.path == c.path)));
    }

    #[test]
    fn only_visible_cells_are_made() {
        let mut rng = Lcg(7);
        let t = tree(&mut rng, 3, 80);
        let mut cam = Camera::FIT;
        cam.zoom_at(FRAME.x + 20.0, FRAME.y + 20.0, 10.0);
        cam.clamp(FRAME);
        let vis = cam.visible_world(FRAME);
        let cells = layout_view(&t, FRAME, &cam, &Opts::default());
        assert!(!cells.is_empty());
        for c in &cells {
            assert!(overlaps(&c.r, &vis), "{} は画面の外", c.path);
        }
    }

    #[test]
    fn tiny_items_are_folded_into_at_most_two_cells() {
        // 大きいファイル 1 つ + 小さいファイル 10 万個。
        let mut kids = vec![file("big.bin".into(), 1_000_000_000)];
        kids.extend((0..100_000).map(|i| file(format!("s{i}"), 1 + (i % 7) as u64)));
        let mut root = Node { name: String::new(), size: 0, is_dir: true, children: kids };
        finish(&mut root);
        let cells = layout_view(&root, FRAME, &Camera::FIT, &Opts::default());
        assert!(cells.len() <= 3, "四角が多すぎる: {}", cells.len());
        let more: Vec<&VCell> = cells.iter().filter(|c| matches!(c.kind, Kind::More(_))).collect();
        let counted: usize = more.iter().map(|c| if let Kind::More(n) = c.kind { n } else { 0 }).sum();
        assert!(counted <= 100_000);
        for m in &more {
            assert!(inside(&m.r, &FRAME));
        }
        // 「ほか」は大きいファイルと重ならない。
        let big = cells.iter().find(|c| c.path == "big.bin").unwrap();
        for m in &more {
            assert!(overlap_area(&big.r, &m.r) < 1e-6);
        }
    }

    #[test]
    fn folded_count_and_size_add_up() {
        // 全部が小さいと、全体が 1 つの「ほか」になり、件数と大きさが合う。
        let kids: Vec<Node> = (0..5000).map(|i| file(format!("s{i}"), 10)).collect();
        let mut root = Node { name: String::new(), size: 0, is_dir: true, children: kids };
        finish(&mut root);
        let tiny_frame = Rect { x: 0.0, y: 0.0, w: 20.0, h: 20.0 };
        let cells = layout_view(&root, tiny_frame, &Camera::FIT, &Opts::default());
        let (mut n, mut size) = (0usize, 0u64);
        for c in &cells {
            match c.kind {
                Kind::More(k) => {
                    n += k;
                    size += c.size;
                }
                _ => {
                    n += 1;
                    size += c.size;
                }
            }
        }
        assert_eq!(n, 5000);
        assert_eq!(size, 50_000);
    }

    #[test]
    fn folding_keeps_cell_count_bounded_by_pixels() {
        // 20 万ファイルでも、四角の数は画素で抑えられる（枠 400×300 = 12 万画素）。
        let mut rng = Lcg(99);
        let mut kids: Vec<Node> = Vec::new();
        for d in 0..200 {
            let files: Vec<Node> = (0..1000)
                .map(|i| {
                    let e = 1 + rng.below(16);
                    file(format!("f{i}"), 1 + rng.below(1 << e))
                })
                .collect();
            let mut dir = Node { name: format!("d{d}"), size: 0, is_dir: true, children: files };
            finish(&mut dir);
            kids.push(dir);
        }
        let mut root = Node { name: String::new(), size: 0, is_dir: true, children: kids };
        finish(&mut root);
        let cells = layout_view(&root, FRAME, &Camera::FIT, &Opts::default());
        assert!(cells.len() < 12_000, "四角が多すぎる: {}", cells.len());
    }

    #[test]
    fn small_folders_reveal_their_contents_when_zoomed() {
        // 倍率 1 では 3 画素に満たない小さなフォルダも、拡大すれば中のファイルが出る。
        let mut kids: Vec<Node> = (0..30).map(|i| file(format!("f{i}"), 1_000_000)).collect();
        for i in 0..50 {
            let mut d = Node { name: format!("t{i}"), size: 0, is_dir: true, children: vec![file("a.txt".into(), 600), file("b.txt".into(), 400)] };
            finish(&mut d);
            kids.push(d);
        }
        let mut root = Node { name: String::new(), size: 0, is_dir: true, children: kids };
        finish(&mut root);
        let at_fit = layout_view(&root, FRAME, &Camera::FIT, &Opts::default());
        assert!(!at_fit.iter().any(|c| c.path == "t0" || c.path == "t0/a.txt"), "倍率 1 では描かれないはず");
        // 並べ方は倍率に依らない。描かれていなくても、位置は並べ方から分かる。
        let sizes: Vec<u64> = root.children.iter().map(|c| c.size).collect();
        let idx = root.children.iter().position(|c| c.name == "t0").unwrap();
        let target = squarify_rows(&sizes, FRAME)[idx].0;
        assert!(target.w.max(target.h) / target.w.min(target.h) < 4.0, "前提: 細長い帯ではない");
        assert!(target.w < 3.0 && target.h < 3.0, "前提: 倍率 1 では 3 画素に満たない");
        let cam = Camera::focus(FRAME, target, 2.0);
        let zoomed = layout_view(&root, FRAME, &cam, &Opts::default());
        assert!(zoomed.iter().any(|c| c.path == "t0"));
        assert!(zoomed.iter().any(|c| c.path == "t0/a.txt"), "拡大しても中が出ない");
        assert!(zoomed.iter().any(|c| c.path == "t0/b.txt"));
    }

    #[test]
    fn a_dominant_file_leaves_a_sliver_that_focus_cannot_fit() {
        // 1 つが全体のほとんどだと、残りは細長い帯になる（squarified の性質）。focus は帯の全体を映す倍率にするので、
        // 倍率が上がらない。帯の中を見るにはホイールで拡大する。画面側で扱いを決める材料として、事実をテストに残す。
        let mut root = Node { name: String::new(), size: 0, is_dir: true, children: vec![file("big".into(), 20_000_000), file("small".into(), 1000)] };
        finish(&mut root);
        let rows = squarify_rows(&[20_000_000, 1000], FRAME);
        let sliver = rows[1].0;
        assert!(sliver.h / sliver.w > 100.0);
        assert_eq!(Camera::focus(FRAME, sliver, 2.0).scale, MIN_SCALE);
        let mut cam = Camera::FIT;
        cam.zoom_at(sliver.x + sliver.w / 2.0, sliver.y + sliver.h / 2.0, 1000.0);
        cam.clamp(FRAME);
        let cells = layout_view(&root, FRAME, &cam, &Opts::default());
        assert!(cells.iter().any(|c| c.path == "small"), "ホイールで拡大すれば帯が出る");
    }

    #[test]
    fn squarified_cells_stay_reasonably_square() {
        // 縦横比がきわめて細長い四角は、小さな項目の尾に限られる（尾は「ほか」にまとめる）。
        let mut rng = Lcg(5);
        let t = tree(&mut rng, 2, 120);
        let cells = layout_view(&t, FRAME, &Camera::FIT, &Opts::default());
        let drawn: Vec<&VCell> = cells.iter().filter(|c| c.kind != Kind::Dir && !matches!(c.kind, Kind::More(_))).collect();
        assert!(!drawn.is_empty());
        let worst = drawn.iter().map(|c| (c.r.w / c.r.h).max(c.r.h / c.r.w)).fold(0.0, f64::max);
        assert!(worst < 25.0, "細長すぎる四角がある: {worst}");
    }

    #[test]
    fn hit_finds_the_innermost_cell() {
        let mut rng = Lcg(3);
        let t = tree(&mut rng, 3, 40);
        let cam = Camera::FIT;
        let cells = layout_view(&t, FRAME, &cam, &Opts::default());
        let deepest = cells.iter().max_by_key(|c| c.depth).unwrap();
        let (cx, cy) = (deepest.r.x + deepest.r.w / 2.0, deepest.r.y + deepest.r.h / 2.0);
        let i = hit(&cells, &cam, cx, cy).unwrap();
        assert!(cells[i].depth >= deepest.depth);
        assert!(hit(&cells, &cam, -1000.0, -1000.0).is_none());
    }

    #[test]
    fn lod_grows_with_size() {
        let f = 14.0;
        assert_eq!(Lod::of(5.0, 5.0, f), Lod::Dot);
        assert_eq!(Lod::of(80.0, 20.0, f), Lod::Name);
        assert_eq!(Lod::of(150.0, 70.0, f), Lod::Texture);
        assert_eq!(Lod::of(300.0, 200.0, f), Lod::Text);
        // どんな文字の大きさでも、段階は大きさの順（小さい文字で Text が Texture より先に来ない）。
        for fh in [6.0, 10.0, 14.0, 24.0] {
            let mut prev = Lod::Dot;
            for k in 1..=400 {
                let now = Lod::of(k as f64 * 2.0, k as f64, fh);
                assert!(now >= prev, "font {fh}, {k}: {now:?} < {prev:?}");
                prev = now;
            }
        }
        assert!(Lod::Dot < Lod::Name && Lod::Name < Lod::Texture && Lod::Texture < Lod::Text);
    }

    #[test]
    fn readable_scale_matches_the_worked_example() {
        // 100 MB のフォルダを 1000×700 画素で見る。1 バイト = 6×12 画素²。長さで約 100 倍。
        let s = readable_scale(100_000_000, 1000.0 * 700.0, 72.0);
        assert!((s - 101.4).abs() < 0.5, "{s}");
        assert_eq!(readable_scale(10, 1.0e6, 72.0), MIN_SCALE);
        assert_eq!(readable_scale(10, 0.0, 72.0), MIN_SCALE);
    }

    #[test]
    fn wheel_factor_is_symmetric() {
        assert!((wheel_factor(3.0) * wheel_factor(-3.0) - 1.0).abs() < 1e-12);
        assert_eq!(wheel_factor(0.0), 1.0);
    }
}
