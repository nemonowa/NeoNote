// フォルダ・zip の中身を、大きさに比例した面積の四角で描く（WizTree のツリーマップ）。
// フォルダは入れ子の枠、ファイルは拡張子ごとの色。並べ方は squarified（細長くなりにくい）。
use crate::neo::archive::Entry;

#[derive(Debug, Default)]
pub struct Node {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

/// 一覧から木を作る（フォルダの大きさは中のファイルの合計）。子は大きい順。
pub fn build(entries: &[Entry]) -> Node {
    let mut root = Node { is_dir: true, ..Default::default() };
    for e in entries.iter().filter(|e| !e.is_dir) {
        let parts: Vec<&str> = e.path.split('/').filter(|s| !s.is_empty()).collect();
        let Some((last, dirs)) = parts.split_last() else { continue };
        let mut cur = &mut root;
        for d in dirs {
            let i = match cur.children.iter().position(|c| c.is_dir && c.name == *d) {
                Some(i) => i,
                None => {
                    cur.children.push(Node { name: d.to_string(), is_dir: true, ..Default::default() });
                    cur.children.len() - 1
                }
            };
            cur = &mut cur.children[i];
        }
        cur.children.push(Node { name: last.to_string(), size: e.size, ..Default::default() });
    }
    finish(&mut root);
    root
}

fn finish(n: &mut Node) -> u64 {
    if n.is_dir {
        n.size = n.children.iter_mut().map(finish).sum();
        n.children.retain(|c| c.size > 0);
        n.children.sort_by(|a, b| b.size.cmp(&a.size));
    }
    n.size
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 大きい順の `sizes` を `r` の中へ squarified で並べた四角（sizes と同じ順）。
pub fn squarify(sizes: &[u64], r: Rect) -> Vec<Rect> {
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    if total <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return vec![Rect { w: 0.0, h: 0.0, ..r }; sizes.len()];
    }
    let scale = r.w * r.h / total;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();
    let mut out = Vec::with_capacity(sizes.len());
    let mut rest = r;
    let mut i = 0;
    while i < areas.len() {
        let side = rest.w.min(rest.h);
        let mut j = i + 1;
        while j < areas.len() && worst(&areas[i..=j], side) <= worst(&areas[i..j], side) {
            j += 1;
        }
        let row = &areas[i..j];
        let sum: f64 = row.iter().sum();
        if rest.w >= rest.h {
            let cw = sum / rest.h;
            let mut y = rest.y;
            for a in row {
                let h = a / cw;
                out.push(Rect { x: rest.x, y, w: cw, h });
                y += h;
            }
            rest = Rect { x: rest.x + cw, w: (rest.w - cw).max(0.0), ..rest };
        } else {
            let rh = sum / rest.w;
            let mut x = rest.x;
            for a in row {
                let w = a / rh;
                out.push(Rect { x, y: rest.y, w, h: rh });
                x += w;
            }
            rest = Rect { y: rest.y + rh, h: (rest.h - rh).max(0.0), ..rest };
        }
        i = j;
    }
    out
}

/// 1 列に並べたときの、いちばん細長い四角の縦横比。
fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (mx, mn) = row.iter().fold((0f64, f64::MAX), |(a, b), &x| (a.max(x), b.min(x)));
    let s2 = side * side;
    (s2 * mx / (sum * sum)).max(sum * sum / (s2 * mn))
}

/// 描く四角 1 つ。`depth` はフォルダの深さ、`path` は `/` 区切り。
#[derive(Debug)]
pub struct Cell {
    pub r: Rect,
    pub is_dir: bool,
    pub depth: u32,
    pub path: String,
    pub size: u64,
}

/// 木を `r` に並べる。フォルダは枠の内側へ `pad` 詰めて子を並べ、`min` より小さい四角は描かない。
pub fn layout(root: &Node, r: Rect, pad: f64, min: f64) -> Vec<Cell> {
    let mut out = Vec::new();
    place(root, r, pad, min, 0, "", &mut out);
    out
}

fn place(n: &Node, r: Rect, pad: f64, min: f64, depth: u32, prefix: &str, out: &mut Vec<Cell>) {
    let sizes: Vec<u64> = n.children.iter().map(|c| c.size).collect();
    for (c, cr) in n.children.iter().zip(squarify(&sizes, r)) {
        if cr.w < min || cr.h < min {
            continue;
        }
        let path = if prefix.is_empty() { c.name.clone() } else { format!("{prefix}/{}", c.name) };
        out.push(Cell { r: cr, is_dir: c.is_dir, depth, path: path.clone(), size: c.size });
        if c.is_dir {
            let inner = Rect { x: cr.x + pad, y: cr.y + pad, w: cr.w - 2.0 * pad, h: cr.h - 2.0 * pad };
            if inner.w >= min && inner.h >= min {
                place(c, inner, pad, min, depth + 1, &path, out);
            }
        }
    }
}

/// 拡張子から決まる色（同じ拡張子はいつも同じ色）。
pub fn color_of(path: &str) -> (u8, u8, u8) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    let h = ext.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    hsv(((h % 360) as f64) / 360.0, 0.55, 0.85)
}

fn hsv(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(p: &str, s: u64) -> Entry {
        Entry { path: p.into(), is_dir: p.ends_with('/'), size: s, packed: None }
    }

    #[test]
    fn folder_size_is_sum_and_children_sorted() {
        let t = build(&[e("a/", 0), e("a/x.txt", 10), e("a/y.png", 30), e("b.txt", 5)]);
        assert_eq!(t.size, 45);
        assert_eq!(t.children[0].name, "a");
        assert_eq!(t.children[0].size, 40);
        assert_eq!(t.children[0].children[0].name, "y.png");
    }

    #[test]
    fn squarify_fills_area_in_proportion() {
        let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        let v = squarify(&[60, 30, 10], r);
        let areas: Vec<f64> = v.iter().map(|c| c.w * c.h).collect();
        assert!((areas[0] - 3000.0).abs() < 1e-6 && (areas[1] - 1500.0).abs() < 1e-6 && (areas[2] - 500.0).abs() < 1e-6);
        for c in &v {
            assert!(c.x >= -1e-9 && c.y >= -1e-9 && c.x + c.w <= 100.0 + 1e-6 && c.y + c.h <= 50.0 + 1e-6);
        }
    }

    #[test]
    fn layout_skips_tiny_cells() {
        let t = build(&[e("big.bin", 1_000_000), e("tiny.txt", 1)]);
        let cells = layout(&t, Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, 1.0, 2.0);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].path, "big.bin");
    }
}
