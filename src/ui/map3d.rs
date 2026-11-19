// 面積図の 3D 表示（斜めから見た押し出し）の形を決める。GUI なし。
//
// 階層と大きさを「高さ」で表す。フォルダは板、その中の項目は親の板の上に載り、ファイルは自分の大きさ（バイト数）に応じて
// 盛り上がる（重いファイルほど高い柱）。深い項目ほど高い所にあり、入れ子が段になって見える。
// 高さは画面上の画素で決める（四角の短い辺と大きさの比に応じ、上限あり）ので、拡大しても、ひとつひとつの形は同じに見える。
//
// 見る向き: 高いほど「右上」へずれて見える（斜投影）。上の面と、見えている 2 つの側面（手前と左）を出す。
// 描く順: 親が先、子が後。同じ親の兄弟は、奥（上・右）から手前（下・左）へ（高い兄弟が奥の兄弟を隠すので）。
// 画面への接続（sidepanel）で使う。
#![allow(dead_code)]
use super::mapview::{Camera, Kind, VCell};
use super::treemap::Rect;

/// 高さ 1 画素あたりの、右へのずれと上へのずれ（画素）。
pub const DX: f64 = 0.4;
pub const DY: f64 = 0.6;
/// 積み上がる高さの上限（画素）。これを超える段は、ほぼ同じ高さにとどめる。
pub const Z_CAP: f64 = 48.0;

/// 上の面が右上へずれる最大の量（画素）。面積図の枠は、この分だけ上と右を空けて並べる（はみ出して切れないように）。
pub fn margins() -> (i32, i32) {
    ((Z_CAP * DX).ceil() as i32 + 2, (Z_CAP * DY).ceil() as i32 + 2)
}

/// 押し出した四角 1 つ。座標は画面。
#[derive(Debug, Clone)]
pub struct Solid {
    /// `cells` の番号。
    pub idx: usize,
    /// 地面の四角（z = 0 での位置）。
    pub foot: Rect,
    /// 底と上の高さ（画素）。
    pub z0: f64,
    pub z1: f64,
    /// 上の面（地面の四角を、高さ z1 だけ右上へずらした物）。
    pub top: Rect,
}

impl Solid {
    fn shifted(&self, z: f64) -> (f64, f64) {
        (z * DX, -z * DY)
    }

    /// 手前の側面（下の辺の側）。左下から時計回り。
    pub fn front(&self) -> [(f64, f64); 4] {
        let f = &self.foot;
        let (a, b) = (self.shifted(self.z0), self.shifted(self.z1));
        let y = f.y + f.h;
        [(f.x + a.0, y + a.1), (f.x + f.w + a.0, y + a.1), (f.x + f.w + b.0, y + b.1), (f.x + b.0, y + b.1)]
    }

    /// 左の側面。
    pub fn left(&self) -> [(f64, f64); 4] {
        let f = &self.foot;
        let (a, b) = (self.shifted(self.z0), self.shifted(self.z1));
        [(f.x + a.0, f.y + a.1), (f.x + a.0, f.y + f.h + a.1), (f.x + b.0, f.y + f.h + b.1), (f.x + b.0, f.y + b.1)]
    }

    /// 側面を描いてよい大きさか（深く拡大すると座標が整数に収まらないので、そのときは上の面だけ出す）。
    pub fn faces_in_range(&self) -> bool {
        const LIM: f64 = 1.0e5;
        self.foot.x.abs() < LIM && self.foot.y.abs() < LIM && self.foot.w < LIM && self.foot.h < LIM
    }
}

/// 厚み（画素）。`side` は四角の短い辺（画素）、`ratio` は大きさ（0〜1。見えている最大のファイルに対する割合の平方根）。
fn thickness(kind: Kind, side: f64, ratio: f64) -> f64 {
    match kind {
        Kind::Dir => (0.06 * side).clamp(2.5, 7.0),
        // 重いファイルほど高い。小さな四角が細い塔にならないよう、短い辺の 0.35 倍までにとどめる
        Kind::File => (0.35 * side).min(4.0 + 26.0 * ratio.clamp(0.0, 1.0)).max(2.0),
        Kind::More(_) => (0.1 * side).clamp(1.5, 5.0),
    }
}

/// `layout_view` の結果（親が先の順）を、描く順の押し出しの並びにする。
pub fn solids(cells: &[VCell], cam: &Camera) -> Vec<Solid> {
    let n = cells.len();
    // 親を探す（深さの 1 つ上で、いちばん近い前の四角）。「ほか N 件」は同じ深さの兄弟。
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut last_at: Vec<usize> = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        let d = c.depth as usize;
        parent[i] = if d == 0 { None } else { last_at.get(d - 1).copied() };
        last_at.truncate(d);
        last_at.resize(d, usize::MAX);
        last_at.push(i);
    }
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots: Vec<usize> = Vec::new();
    for (i, p) in parent.iter().enumerate() {
        match p {
            Some(p) if *p != usize::MAX => kids[*p].push(i),
            _ => roots.push(i),
        }
    }
    let foot: Vec<Rect> = cells.iter().map(|c| cam.screen_rect(c.r)).collect();
    let max_file = cells.iter().filter(|c| c.kind == Kind::File).map(|c| c.size).max().unwrap_or(1).max(1) as f64;
    // 兄弟の順: 奥（下の辺が上にある）から手前へ。同じ高さなら、右から左へ（高い物は右上へずれるので、右が奥）。
    let order = |v: &mut Vec<usize>| {
        v.sort_by(|&a, &b| {
            let (ya, yb) = (foot[a].y + foot[a].h, foot[b].y + foot[b].h);
            ya.total_cmp(&yb).then(foot[b].x.total_cmp(&foot[a].x))
        })
    };
    order(&mut roots);
    let mut z1 = vec![0.0f64; n];
    let mut out: Vec<Solid> = Vec::with_capacity(n);
    // 親が先、子が後（子の列は、兄弟の順に並べてから積む）
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(i) = stack.pop() {
        let f = foot[i];
        let z0 = parent[i].filter(|p| *p != usize::MAX).map_or(0.0, |p| z1[p]);
        let t = thickness(cells[i].kind, f.w.min(f.h), (cells[i].size as f64 / max_file).sqrt());
        z1[i] = (z0 + t).min(Z_CAP).max(z0 + 0.5);
        let top = Rect { x: f.x + z1[i] * DX, y: f.y - z1[i] * DY, w: f.w, h: f.h };
        out.push(Solid { idx: i, foot: f, z0, z1: z1[i], top });
        let mut ks = std::mem::take(&mut kids[i]);
        order(&mut ks);
        stack.extend(ks.into_iter().rev());
    }
    out
}

/// 画面の (x, y) にある、いちばん手前の四角（上の面で調べる）の `cells` の番号。
pub fn hit(solids: &[Solid], x: f64, y: f64) -> Option<usize> {
    solids.iter().rposition(|s| x >= s.top.x && x < s.top.x + s.top.w && y >= s.top.y && y < s.top.y + s.top.h).map(|i| solids[i].idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mapview::{layout_view, Opts};
    use crate::ui::treemap::Node;

    fn file(name: &str, size: u64) -> Node {
        Node { name: name.into(), size, is_dir: false, children: Vec::new() }
    }

    fn dir(name: &str, mut kids: Vec<Node>) -> Node {
        kids.sort_by_key(|k| std::cmp::Reverse(k.size));
        Node { name: name.into(), size: kids.iter().map(|k| k.size).sum(), is_dir: true, children: kids }
    }

    const FRAME: Rect = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };

    fn sample() -> (Node, Vec<VCell>) {
        let root = dir(
            "",
            vec![
                dir("a", vec![file("a1", 900), file("a2", 600), dir("deep", vec![file("d1", 300), file("d2", 200)])]),
                dir("b", vec![file("b1", 800), file("b2", 500)]),
                file("c", 700),
            ],
        );
        let cells = layout_view(&root, FRAME, &Camera::FIT, &Opts::default());
        (root, cells)
    }

    fn pos(s: &[Solid], cells: &[VCell], path: &str) -> usize {
        s.iter().position(|x| cells[x.idx].path == path).unwrap_or_else(|| panic!("{path} が無い"))
    }

    #[test]
    fn every_cell_becomes_a_solid_and_parents_come_first() {
        let (_, cells) = sample();
        let s = solids(&cells, &Camera::FIT);
        assert_eq!(s.len(), cells.len());
        for p in ["a/a1", "a/deep/d1", "b/b1"] {
            let parent = p.rsplit_once('/').unwrap().0;
            assert!(pos(&s, &cells, parent) < pos(&s, &cells, p), "{p} は {parent} より後");
        }
    }

    #[test]
    fn children_sit_on_the_parent_and_deeper_means_higher() {
        let (_, cells) = sample();
        let s = solids(&cells, &Camera::FIT);
        let get = |path: &str| &s[pos(&s, &cells, path)];
        assert_eq!(get("a/a1").z0, get("a").z1, "子の底は、親の上");
        assert_eq!(get("a/deep/d1").z0, get("a/deep").z1);
        assert!(get("a/deep/d1").z1 > get("a/a1").z1 - 1e-9 || get("a/deep").z1 > get("a").z1);
        assert!(get("a").z0 == 0.0);
        for x in &s {
            assert!(x.z1 > x.z0 && x.z1.is_finite());
            // 上の面は、地面の四角を高さ z1 だけ右上へずらした物
            assert!((x.top.x - (x.foot.x + x.z1 * DX)).abs() < 1e-9 && (x.top.y - (x.foot.y - x.z1 * DY)).abs() < 1e-9);
            assert_eq!((x.top.w, x.top.h), (x.foot.w, x.foot.h));
        }
    }

    #[test]
    fn heavier_files_are_taller_and_the_stack_stops_near_the_cap() {
        let (_, cells) = sample();
        let s = solids(&cells, &Camera::FIT);
        let h = |path: &str| {
            let x = &s[pos(&s, &cells, path)];
            x.z1 - x.z0
        };
        // 同じくらいの四角どうしなら、重い方が高い（a1: 900、a2: 600）
        assert!(h("a/a1") >= h("a/a2") - 1e-9);
        let (mr, mu) = margins();
        assert!(mu as f64 >= Z_CAP * DY && mr as f64 >= Z_CAP * DX);
        // 深い入れ子を作っても、上の面のずれは余白に収まる
        let mut node = file("leaf", 10);
        for i in 0..40 {
            node = dir(&format!("d{i}"), vec![node, file(&format!("x{i}"), 500)]);
        }
        let root = dir("", vec![node, file("big", 5000)]);
        let big = Rect { x: 0.0, y: 0.0, w: 600.0, h: 600.0 };
        let cells = layout_view(&root, big, &Camera::FIT, &Opts { pad: 2.0, min: 1.0 });
        for x in solids(&cells, &Camera::FIT) {
            assert!(x.z1 <= Z_CAP + 0.5 * (cells.len() as f64), "{}", x.z1);
            assert!(x.z1 > x.z0);
        }
        let top = solids(&cells, &Camera::FIT).iter().map(|x| x.z1).fold(0.0, f64::max);
        assert!(top < Z_CAP + 40.0, "段を重ねても、積み上がりは止まる: {top}");
    }

    #[test]
    fn thickness_is_bounded_so_zooming_does_not_blow_up() {
        for side in [0.0, 1.0, 10.0, 1.0e3, 1.0e9] {
            for k in [Kind::Dir, Kind::File, Kind::More(3)] {
                for ratio in [0.0, 0.5, 1.0, 7.0] {
                    let t = thickness(k, side, ratio);
                    assert!((1.5..=30.0).contains(&t), "{k:?} {side} {ratio}: {t}");
                }
            }
        }
    }

    #[test]
    fn siblings_go_from_the_back_to_the_front() {
        // 横に並んだ 2 つは、右が奥なので先。縦に並んだ 2 つは、上が奥なので先。
        let side = dir("", vec![file("left", 100), file("right", 100)]);
        let wide = Rect { x: 0.0, y: 0.0, w: 400.0, h: 100.0 };
        let cells = layout_view(&side, wide, &Camera::FIT, &Opts::default());
        let s = solids(&cells, &Camera::FIT);
        let (l, r) = (cells.iter().position(|c| c.path == "left").unwrap(), cells.iter().position(|c| c.path == "right").unwrap());
        let (xl, xr) = (cells[l].r.x, cells[r].r.x);
        let first = &cells[s[0].idx];
        assert_eq!(first.r.x, xl.max(xr), "右にある方が先に描かれる");
        let tall = Rect { x: 0.0, y: 0.0, w: 100.0, h: 400.0 };
        let cells = layout_view(&side, tall, &Camera::FIT, &Opts::default());
        let s = solids(&cells, &Camera::FIT);
        let ys: Vec<f64> = s.iter().map(|x| cells[x.idx].r.y).collect();
        assert!(ys[0] < ys[1], "上にある方が先に描かれる: {ys:?}");
    }

    #[test]
    fn hit_uses_the_top_face_and_prefers_the_front() {
        let (_, cells) = sample();
        let s = solids(&cells, &Camera::FIT);
        // どの四角も、自分の上の面の中心では、自分（か、その上に載った子）が当たる
        for x in &s {
            let (cx, cy) = (x.top.x + x.top.w / 2.0, x.top.y + x.top.h / 2.0);
            let h = hit(&s, cx, cy).expect("当たる");
            let owner = &cells[h];
            let me = &cells[x.idx];
            assert!(owner.path == me.path || owner.path.starts_with(&format!("{}/", me.path)) || owner.depth >= me.depth, "{} → {}", me.path, owner.path);
        }
        assert!(hit(&s, -500.0, -500.0).is_none());
    }

    #[test]
    fn side_faces_follow_the_foot_and_the_lift() {
        let (_, cells) = sample();
        let s = solids(&cells, &Camera::FIT);
        let x = &s[pos(&s, &cells, "a/a1")];
        let f = x.front();
        // 手前の面: 下の辺（y が大きい側）。底の辺は、上の辺より下・左にある
        assert!(f[0].1 > f[3].1 && f[0].0 < f[3].0);
        assert!((f[0].1 - (x.foot.y + x.foot.h - x.z0 * DY)).abs() < 1e-9);
        let l = x.left();
        assert!(l[0].0 < l[3].0 || (x.z1 - x.z0).abs() < 1e-9);
        assert!(x.faces_in_range());
        let mut huge = x.clone();
        huge.foot.w = 1.0e9;
        assert!(!huge.faces_in_range());
    }

    #[test]
    fn deep_zoom_stays_finite() {
        let (_, _) = sample();
        let root = dir("", (0..30).map(|i| file(&format!("f{i}"), 1000 + i)).collect());
        for z in [1.0, 100.0, 1.0e6] {
            let mut cam = Camera::FIT;
            cam.zoom_at(200.0, 150.0, z);
            cam.clamp(FRAME);
            let cells = layout_view(&root, FRAME, &cam, &Opts::default());
            for s in solids(&cells, &cam) {
                assert!(s.z1.is_finite() && s.top.x.is_finite() && s.top.y.is_finite(), "zoom {z}");
            }
        }
    }
}
