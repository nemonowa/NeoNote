// 面積図の立体感（クッション陰影。van Wijk と van de Wetering、1999）。GUI なし。
//
// 四角を入れ子の「枕」として扱う。フォルダも中のファイルも、自分の四角の上に山（放物面）を 1 枚足す。
// 子の山は親の山の上に重なるので、階層が深いほど高く見え、四角の縁に陰ができて、どこが入れ子かが分かる。
// 山の高さは四角の短い辺に比例させる（小さな四角も大きな四角も、同じくらい膨らんで見える）。
// 深さごとに少しずつ低くする（`DECAY`）。
//
// 面は「世界」の座標（倍率 1 のときの画面）で決めるので、拡大しても見た目の形は変わらない（拡大されるだけ）。
// 画素ごとの計算は、並べ直したとき（拡大・移動・枠の変更）にだけ行い、結果の画像を使い回す（マウスを乗せるたびには計算しない）。
// 画面への接続（sidepanel）で使う。
#![allow(dead_code)]
use super::mapview::{Camera, VCell};
use super::treemap::Rect;

/// 山の高さ（短い辺に対する割合）。縁での傾きは 4 × これ（0.2 で約 39 度）。
const RIDGE: f32 = 0.2;
/// 深さが 1 つ増えるごとの、山の高さの倍率。
const DECAY: f32 = 0.78;
/// 暗い所の明るさ（環境光）。
const AMBIENT: f32 = 0.42;
/// 光の向き（左上から）。
const LIGHT: (f32, f32, f32) = (-0.45, -0.6, 0.9);

/// 枠 `frame`（x, y, 幅, 高さ。画素）の中の、四角 `s`（画面の座標）の描く範囲。枠の外は切る。無ければ None。
/// 深く拡大すると四角の座標は整数に収まらないほど大きくなるので、整数にする前に切る。
pub fn pixel_rect(s: Rect, frame: (i32, i32, i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let (fx, fy, fw, fh) = (frame.0 as f64, frame.1 as f64, frame.2 as f64, frame.3 as f64);
    let (x0, y0, x1, y1) = (s.x.max(fx), s.y.max(fy), (s.x + s.w).min(fx + fw), (s.y + s.h).min(fy + fh));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (x, y) = (x0.round() as i32, y0.round() as i32);
    let (w, h) = (x1.round() as i32 - x, y1.round() as i32 - y);
    (w > 0 && h > 0).then_some((x, y, w, h))
}

/// 四角の山の係数: 高さ z = a1·x² + a2·x + a3·y² + a4·y（+ 定数。傾きだけ使うので要らない）。
type Surface = [f32; 4];

fn ridge(parent: Surface, r: Rect, depth: u32) -> Surface {
    let (w, h) = (r.w as f32, r.h as f32);
    if w <= 0.0 || h <= 0.0 {
        return parent;
    }
    let height = RIDGE * w.min(h) * DECAY.powi(depth as i32);
    let (x1, x2, y1, y2) = (r.x as f32, (r.x + r.w) as f32, r.y as f32, (r.y + r.h) as f32);
    // 両端で 0、中央で height の放物面: z = -4·height·(x - x1)(x - x2) / w²（y も同じ）。縁での傾きは 4·height / w
    let (kx, ky) = (4.0 * height / (w * w), 4.0 * height / (h * h));
    [parent[0] - kx, parent[1] + kx * (x2 + x1), parent[2] - ky, parent[3] + ky * (y2 + y1)]
}

/// 枠いっぱいの RGB 画像（幅 × 高さ × 3 バイト）を作る。`color` は四角の地の色。四角が無い所は `bg`。
/// `cells` は `layout_view` の順（親が先、子が後。「ほか N 件」は親と同じ深さの兄弟）。
pub fn render(cells: &[VCell], cam: &Camera, frame: (i32, i32, i32, i32), bg: (u8, u8, u8), color: &dyn Fn(&VCell) -> (u8, u8, u8)) -> Vec<u8> {
    let (fw, fh) = (frame.2.max(0) as usize, frame.3.max(0) as usize);
    let mut img = Vec::with_capacity(fw * fh * 3);
    for _ in 0..fw * fh {
        img.extend_from_slice(&[bg.0, bg.1, bg.2]);
    }
    let norm = (LIGHT.0 * LIGHT.0 + LIGHT.1 * LIGHT.1 + LIGHT.2 * LIGHT.2).sqrt();
    let (lx, ly, lz) = (LIGHT.0 / norm, LIGHT.1 / norm, LIGHT.2 / norm);
    let flat = AMBIENT + (1.0 - AMBIENT) * lz; // 山の無い（傾きが 0 の）面の明るさ。これを地の色の明るさにそろえる
    let mut stack: Vec<Surface> = Vec::new();
    let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
    for c in cells {
        let d = c.depth as usize;
        let parent = if d == 0 { [0.0; 4] } else { stack.get(d - 1).copied().unwrap_or([0.0; 4]) };
        let surf = ridge(parent, c.r, c.depth);
        stack.truncate(d);
        stack.resize(d, [0.0; 4]);
        stack.push(surf);
        let Some((x, y, w, h)) = pixel_rect(cam.screen_rect(c.r), frame) else { continue };
        let base = color(c);
        // 画素の中心の世界座標を、列と行ごとに 1 回だけ計算する
        xs.clear();
        ys.clear();
        xs.extend((0..w).map(|i| cam.world_point((x + i) as f64 + 0.5, 0.0).0 as f32));
        ys.extend((0..h).map(|j| cam.world_point(0.0, (y + j) as f64 + 0.5).1 as f32));
        for (j, wy) in ys.iter().enumerate() {
            let ny = -(2.0 * surf[2] * wy + surf[3]);
            let row = ((y - frame.1) as usize + j) * fw + (x - frame.0) as usize;
            for (i, wx) in xs.iter().enumerate() {
                let nx = -(2.0 * surf[0] * wx + surf[1]);
                let shade = (nx * lx + ny * ly + lz) / (nx * nx + ny * ny + 1.0).sqrt();
                let k = (AMBIENT + (1.0 - AMBIENT) * shade.max(0.0)) / flat;
                let o = (row + i) * 3;
                img[o] = (base.0 as f32 * k).min(255.0) as u8;
                img[o + 1] = (base.1 as f32 * k).min(255.0) as u8;
                img[o + 2] = (base.2 as f32 * k).min(255.0) as u8;
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mapview::{layout_view, Kind, Opts};
    use crate::ui::treemap::Node;

    const FRAME: (i32, i32, i32, i32) = (10, 20, 120, 80);
    const BG: (u8, u8, u8) = (30, 30, 30);

    fn cell(r: Rect, kind: Kind, depth: u32) -> VCell {
        VCell { r, kind, depth, path: "x".into(), size: 1 }
    }

    fn lum(img: &[u8], w: usize, x: usize, y: usize) -> f32 {
        let o = (y * w + x) * 3;
        (img[o] as f32 + img[o + 1] as f32 + img[o + 2] as f32) / 3.0
    }

    fn square() -> (Vec<VCell>, Camera) {
        let r = Rect { x: 10.0, y: 20.0, w: 120.0, h: 80.0 };
        (vec![cell(r, Kind::File, 0)], Camera::FIT)
    }

    #[test]
    fn image_has_the_frame_size_and_background_where_nothing_is_drawn() {
        let cells = vec![cell(Rect { x: 10.0, y: 20.0, w: 40.0, h: 80.0 }, Kind::File, 0)];
        let img = render(&cells, &Camera::FIT, FRAME, BG, &|_| (200, 100, 50));
        assert_eq!(img.len(), 120 * 80 * 3);
        let o = (40 * 120 + 100) * 3; // 四角の外
        assert_eq!(&img[o..o + 3], &[30, 30, 30]);
        assert_eq!(render(&[], &Camera::FIT, (0, 0, 0, 0), BG, &|_| BG).len(), 0);
    }

    #[test]
    fn the_flat_top_keeps_the_base_colour_and_the_edges_are_lit_from_the_top_left() {
        let (cells, cam) = square();
        let img = render(&cells, &cam, FRAME, BG, &|_| (200, 120, 60));
        let o = (40 * 120 + 60) * 3; // 中央は傾きが 0 → 地の色のまま
        for (got, want) in img[o..o + 3].iter().zip([200u8, 120, 60]) {
            assert!((*got as i32 - want as i32).abs() <= 2, "{got} vs {want}");
        }
        // 左上の縁は明るく、右下の縁は暗い
        let tl = lum(&img, 120, 3, 40);
        let br = lum(&img, 120, 116, 40);
        let top = lum(&img, 120, 60, 2);
        let bottom = lum(&img, 120, 60, 77);
        assert!(tl > lum(&img, 120, 60, 40) && br < lum(&img, 120, 60, 40), "左右: {tl} {br}");
        assert!(top > bottom, "上下: {top} {bottom}");
    }

    #[test]
    fn deeper_ridges_are_lower_and_nesting_changes_the_shading() {
        let parent = cell(Rect { x: 10.0, y: 20.0, w: 120.0, h: 80.0 }, Kind::Dir, 0);
        let child = cell(Rect { x: 14.0, y: 24.0, w: 50.0, h: 72.0 }, Kind::File, 1);
        let child_r = child.r;
        let alone = render(&[cell(child_r, Kind::File, 1)], &Camera::FIT, FRAME, BG, &|_| (100, 100, 100));
        let nested = render(&[parent, child], &Camera::FIT, FRAME, BG, &|_| (100, 100, 100));
        // 子の左側の画素: 親の山の傾き（左は上り）が足されて、子だけのときと値が変わる
        let (x, y) = (20usize, 40usize);
        assert_ne!(lum(&alone, 120, x, y), lum(&nested, 120, x, y));
        // 深いほど山が低い（縁の傾きが小さい）
        let s0 = ridge([0.0; 4], child_r, 0);
        let s3 = ridge([0.0; 4], child_r, 3);
        assert!(s3[0].abs() < s0[0].abs() && s3[2].abs() < s0[2].abs());
    }

    #[test]
    fn zooming_only_magnifies_the_same_shape() {
        // 倍率が違っても、同じ世界の点は同じ明るさ（形が変わらず、拡大されるだけ）
        let (cells, _) = square();
        let a = render(&cells, &Camera::FIT, FRAME, BG, &|_| (200, 200, 200));
        let mut cam = Camera::FIT;
        cam.zoom_at(70.0, 60.0, 2.0);
        let b = render(&cells, &cam, FRAME, BG, &|_| (200, 200, 200));
        // FIT の (x, y) は、2 倍では (70 + (x - 70) * 2, 60 + (y - 60) * 2) に写る
        let (px, py) = (50.0f64, 45.0f64);
        let (qx, qy) = (70.0 + (px - 70.0) * 2.0, 60.0 + (py - 60.0) * 2.0);
        let la = lum(&a, 120, (px - 10.0) as usize, (py - 20.0) as usize);
        let lb = lum(&b, 120, (qx - 10.0) as usize, (qy - 20.0) as usize);
        assert!((la - lb).abs() < 4.0, "{la} vs {lb}");
    }

    #[test]
    fn deep_zoom_and_cells_outside_the_frame_are_safe() {
        let mut kids = vec![];
        for i in 0..30 {
            kids.push(Node { name: format!("f{i}"), size: 1000 + i, is_dir: false, children: vec![] });
        }
        kids.sort_by_key(|k| std::cmp::Reverse(k.size));
        let root = Node { name: String::new(), size: kids.iter().map(|k| k.size).sum(), is_dir: true, children: kids };
        let frame = Rect { x: 10.0, y: 20.0, w: 120.0, h: 80.0 };
        for z in [1.0, 50.0, 1.0e4, 1.0e7] {
            let mut cam = Camera::FIT;
            cam.zoom_at(60.0, 50.0, z);
            cam.clamp(frame);
            let cells = layout_view(&root, frame, &cam, &Opts::default());
            let img = render(&cells, &cam, FRAME, BG, &|_| (90, 160, 120));
            assert_eq!(img.len(), 120 * 80 * 3, "zoom {z}");
        }
        assert!(pixel_rect(Rect { x: 1.0e12, y: 0.0, w: 5.0, h: 5.0 }, FRAME).is_none());
        let huge = pixel_rect(Rect { x: -1.0e12, y: -1.0e12, w: 3.0e12, h: 3.0e12 }, FRAME).unwrap();
        assert_eq!(huge, FRAME, "枠より大きい四角は、枠いっぱいに切る");
    }

    #[test]
    fn the_same_input_gives_the_same_image() {
        let (cells, cam) = square();
        let a = render(&cells, &cam, FRAME, BG, &|_| (10, 200, 90));
        let b = render(&cells, &cam, FRAME, BG, &|_| (10, 200, 90));
        assert_eq!(a, b);
    }
}
