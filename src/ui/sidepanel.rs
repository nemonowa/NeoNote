// 右端の欄。上にファイルの情報（名前・場所・大きさ・更新日時・文字数・画像や音声の情報）、
// 下に本文全体の縮小表示（VSCode のミニマップのように、空白でない文字を小さな四角で描く）。
// フォルダ・zip のときは縮小表示の代わりに、中身の大きさのツリーマップ（treemap.rs）を描く。
// 縮小表示をクリック・ドラッグすると、その行が見えるように本文を動かす。
// 左の境目をドラッグすると幅を変えられる（WIDTH_MIN〜WIDTH_MAX）。
// 行番号欄と同じく、マウスが右端にあるときだけ出す（`Pin` で常に出す）。出し方の判定は呼び出し側。
use super::{
    cushion, map3d, mapprev,
    mapview::{self, Camera, Kind, Lod},
    theme::*,
    treemap,
};
use fltk::{
    draw,
    enums::{Align, Color, ColorDepth, Cursor, Event, Font},
    prelude::*,
    widget::Widget,
};
use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};

/// フォルダ・zip の Preview の見せ方。ツリー（既定）では、メインがツリーで右の欄が面積図。
/// 面積図・面積図 3D では、メインが面積図で、右の欄がツリー。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MapMode {
    #[default]
    Tree,
    Map,
    Map3D,
}

impl MapMode {
    /// config.txt の archive_view に書く名前（ツリーは既定なので書かない）。
    pub fn config_name(self) -> Option<&'static str> {
        match self {
            MapMode::Tree => None,
            MapMode::Map => Some("map"),
            MapMode::Map3D => Some("map3d"),
        }
    }

    pub fn from_config(v: Option<&str>) -> MapMode {
        match v {
            Some("map") => MapMode::Map,
            Some("map3d") => MapMode::Map3D,
            _ => MapMode::Tree,
        }
    }
}

pub const WIDTH: i32 = 200;
pub const WIDTH_MIN: i32 = 140;
pub const WIDTH_MAX: i32 = 800;
/// 左の境目から、この幅の内側を掴むと幅を変える。
const GRIP: i32 = 5;
const PAD: i32 = 8;
const ROW_H: i32 = 16;
const INFO_SIZE: i32 = 11;
/// 情報の項目名の欄の幅。
const KEY_W: i32 = 56;
/// 縮小表示の 1 行の高さと 1 文字の幅（px）。
const LINE_H: i32 = 3;
const CHAR_W: f64 = 1.25;
/// 縮小表示で読む行と桁の上限（大きな文書でも描くのを軽く保つ）。
pub const MAX_LINES: usize = 50_000;
const MAX_COLS: usize = 150;
const TAB_COLS: usize = 4;

/// 1 行ぶんの、空白でない文字の連続（開始の桁, 長さ）。
pub type Runs = Vec<(u16, u16)>;

#[derive(Default)]
pub struct State {
    /// 情報の行（項目名, 値）。
    pub rows: Vec<(String, String)>,
    pub lines: Vec<Runs>,
    /// 縮小表示が行の上限で切れている。
    pub truncated: bool,
    /// 本文で見えている最初の行（0 始まり）と行数。
    pub top: i32,
    pub visible: i32,
    pub pinned: bool,
    pub font: Option<Font>,
    /// 境目を掴んでドラッグしている。
    pub resizing: bool,
    /// フォルダ・zip の中身の木（あればツリーマップを描く）。
    pub tree: Option<treemap::Node>,
    /// 並べた四角と、並べたときの枠（枠が変わったら全体に戻して並べ直す）。
    cells: Vec<mapview::VCell>,
    cells_for: (i32, i32, i32, i32),
    /// 拡大・移動の状態。`cells` を並べたときのカメラが違えば、並べ直す（None は未だ並べていない）。
    cam: Camera,
    cells_cam: Option<Camera>,
    /// 並べ直すたびに増える番号と、立体感の画像（番号・枠・カメラ・配色のどれかが変わったときだけ作り直す）。
    cells_gen: u64,
    shade: Option<Shade>,
    /// 四角の中のプレビュー。`head` は「道筋 → 先頭のバイト」を返す読み込み（アプリが付ける。zip の中などは None）。
    /// `pending` は、読み込みの途中（次の描画で続きを読む）。
    previews: mapprev::Cache,
    pub head: Option<HeadReader>,
    pending: bool,
    /// メインの面積図として使う（情報欄・縮小表示・境目・押しピンは無く、部品の全面に面積図を描く）。
    pub full: bool,
    /// 面積図を別の場所（メイン）に出している間、右の欄は情報の行だけにして、面積図は描かない（そこへツリーを重ねる）。
    pub overlay: bool,
    /// 3D 表示（斜めから見た押し出し）。`solids` はそのときの描く順の形。
    mode3d: bool,
    solids: Vec<map3d::Solid>,
    /// 最大まで寄ったファイルをダブルクリックしたときに、開く（道筋を渡す）。メインの面積図だけ。
    pub on_open: Option<Box<dyn Fn(String)>>,
    /// マウスが乗っている四角（cells の番号）。
    hover: Option<usize>,
    /// ツリーで選んだ項目の道筋（枠で示す）。
    selected: Option<String>,
    /// 枠がまだ決まっていない間に頼まれた寄せ先（最初に並べるときに合わせる）。
    focus_to: Option<String>,
    /// 左ボタンを押している間の状態（クリックとドラッグの見分け）。
    press: Option<Press>,
}

/// 「道筋 → ファイルの先頭のバイト」を返す読み込み（読めなければ None）。
pub type HeadReader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

type ShadeKey = ((i32, i32, i32, i32), Camera, (u8, u8, u8), (u8, u8, u8), u64);

struct Shade {
    key: ShadeKey,
    img: Vec<u8>,
}

/// 左ボタンを押した位置と、直前の位置。3 画素以上動いたらドラッグ（クリックではない）。
#[derive(Clone, Copy)]
struct Press {
    sx: i32,
    sy: i32,
    lx: i32,
    ly: i32,
    moved: bool,
}

const DRAG_MIN: i32 = 3;
/// 中身のプレビューを読むのは、1 回の描画で数個・数 ms まで（残りは少し後の描画で続ける。画面を止めない）。
const PREVIEW_PER_DRAW: usize = 4;
const PREVIEW_BUDGET: Duration = Duration::from_millis(8);

impl State {
    /// 木を差し替える（並べ直しは次に描くとき）。拡大は全体に戻る。
    pub fn set_tree(&mut self, tree: Option<treemap::Node>) {
        self.tree = tree;
        self.cells.clear();
        self.cells_for = (0, 0, 0, 0);
        self.cam = Camera::FIT;
        self.cells_cam = None;
        self.shade = None;
        self.previews.clear();
        self.pending = false;
        self.hover = None;
        self.press = None;
        self.selected = None;
        self.focus_to = None;
    }

    /// ツリーで選んだ項目を枠で示し、見える所へ寄せる。フォルダはそのフォルダ、ファイルは入っているフォルダを枠いっぱいに映す
    /// （一番上の階層のファイルは全体）。見つからない項目（大きさ 0 など）は、枠だけ消して動かさない。
    pub fn focus_path(&mut self, path: &str) {
        let path = path.trim_end_matches('/').to_string();
        self.selected = Some(path.clone()).filter(|p| !p.is_empty());
        if self.cells_for.2 <= 0 || self.cells_for.3 <= 0 {
            self.focus_to = self.selected.clone();
            return;
        }
        self.apply_focus(&path);
    }

    /// 枠で示すだけ（寄せない。面積図で押した項目に使う）。
    pub fn focus_mark(&mut self, path: &str) {
        self.selected = Some(path.trim_end_matches('/').to_string()).filter(|p| !p.is_empty());
    }

    fn apply_focus(&mut self, path: &str) {
        let Some(tree) = &self.tree else { return };
        let frame = frame_of(self.cells_for);
        let opts = mapview::Opts::default();
        let Some(r) = mapview::rect_of(tree, frame, path, &opts) else { return };
        let is_dir = tree_node(tree, path).is_some_and(|n| n.is_dir);
        let target = if is_dir {
            Some(r)
        } else {
            match path.rsplit_once('/') {
                Some((parent, _)) => mapview::rect_of(tree, frame, parent, &opts),
                None => None,
            }
        };
        self.cam = match target {
            Some(t) => Camera::focus(frame, t, 6.0),
            None => Camera::FIT,
        };
    }

    /// 枠かカメラが変わっていたら、見えている四角だけを並べ直す。枠が変わったときは、全体に戻す。
    fn ensure_cells(&mut self, r: (i32, i32, i32, i32)) {
        if self.cells_for != r {
            self.cells_for = r;
            self.cam = Camera::FIT;
            self.cells_cam = None;
            if let Some(p) = self.focus_to.take() {
                if r.2 > 0 && r.3 > 0 {
                    self.apply_focus(&p);
                } else {
                    self.focus_to = Some(p);
                }
            }
        }
        if self.cells_cam == Some(self.cam) {
            return;
        }
        self.cells_cam = Some(self.cam);
        self.cells_gen += 1;
        self.hover = None;
        self.cells = match &self.tree {
            Some(t) => mapview::layout_view(t, frame_of(r), &self.cam, &mapview::Opts::default()),
            None => Vec::new(),
        };
        self.solids = if self.mode3d { map3d::solids(&self.cells, &self.cam) } else { Vec::new() };
    }

    /// 面積図を操作できる状態か（木があり、別の場所に出していない）。
    fn active(&self) -> bool {
        self.tree.is_some() && !self.overlay
    }

    /// 3D 表示の入り切り（並べ直す）。
    pub fn set_mode3d(&mut self, on: bool) {
        if self.mode3d != on {
            self.mode3d = on;
            self.cells_cam = None;
            self.shade = None;
            self.hover = None;
        }
    }

    /// (x, y) の下の四角の番号（3D は上の面で調べる）。枠の外は None。
    fn hit_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.in_frame(x, y) {
            return None;
        }
        if self.mode3d {
            map3d::hit(&self.solids, x as f64, y as f64)
        } else {
            mapview::hit(&self.cells, &self.cam, x as f64, y as f64)
        }
    }

    fn in_frame(&self, x: i32, y: i32) -> bool {
        let r = self.cells_for;
        x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
    }

    /// (x, y) の下の四角を調べ直す。変わっていたら、乗っている四角の（道筋, フォルダか）を返す（「ほか N 件」は None）。
    fn update_hover(&mut self, x: i32, y: i32) -> Option<Option<(String, bool)>> {
        let h = self.hit_at(x, y);
        if h == self.hover {
            return None;
        }
        self.hover = h;
        Some(h.and_then(|i| self.cells.get(i)).and_then(peek_of))
    }

    /// (x, y) の下の四角の（道筋, フォルダか）。枠の外や「ほか N 件」は None。
    fn peek_at(&self, x: i32, y: i32) -> Option<(String, bool)> {
        self.hit_at(x, y).and_then(|i| self.cells.get(i)).and_then(peek_of)
    }

    /// (x, y) の下の四角を、画面いっぱいに映す。すでにそれ以上寄っているときは、全体へ戻す。
    /// ただしメインの面積図では、すでに寄っているファイルは戻さず、開くために道筋を返す。
    fn focus_at(&mut self, x: i32, y: i32) -> Option<String> {
        let target = self.hit_at(x, y).and_then(|i| self.cells.get(i)).filter(|c| !matches!(c.kind, Kind::More(_))).map(|c| (c.r, c.kind, c.path.clone()));
        let frame = frame_of(self.cells_for);
        let Some((rect, kind, path)) = target else {
            self.cam = Camera::FIT;
            return None;
        };
        let cam = Camera::focus(frame, rect, 6.0);
        if cam.scale > self.cam.scale * 1.05 {
            self.cam = cam;
            None
        } else if self.full && kind == Kind::File {
            Some(path)
        } else {
            self.cam = Camera::FIT;
            None
        }
    }
}

/// 木の中の `path`（`/` 区切り）の節。
fn tree_node<'a>(root: &'a treemap::Node, path: &str) -> Option<&'a treemap::Node> {
    path.split('/').filter(|s| !s.is_empty()).try_fold(root, |n, part| n.children.iter().find(|c| c.name == part))
}

fn frame_of(r: (i32, i32, i32, i32)) -> treemap::Rect {
    treemap::Rect { x: r.0 as f64, y: r.1 as f64, w: r.2 as f64, h: r.3 as f64 }
}

fn peek_of(c: &mapview::VCell) -> Option<(String, bool)> {
    match c.kind {
        Kind::More(_) => None,
        k => Some((c.path.clone(), k == Kind::Dir)),
    }
}

fn more_label(n: usize) -> String {
    if crate::lang::is_en() { format!("{n} more") } else { format!("ほか {n} 件") }
}

/// 地の色の上に出す文字の色（明るい地には暗い字、暗い地には明るい字）。
fn text_on((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    if (299 * r as u32 + 587 * g as u32 + 114 * b as u32) / 1000 > 140 { (20, 20, 20) } else { (235, 235, 235) }
}

/// ツリーマップを描く枠（縮小表示と同じ場所。下の 1 行は乗っている四角の名前）。
fn tree_rect(w: &Widget, rows: usize, full: bool, mode3d: bool) -> (i32, i32, i32, i32) {
    if full {
        // メインの面積図: 部品の全面（下の 1 行は乗っている四角の名前）。3D は、上の面が右上へずれる分の余白を上と右に空ける
        let (mr, mu) = if mode3d { map3d::margins() } else { (0, 0) };
        return (w.x() + PAD, w.y() + PAD + mu, (w.w() - 2 * PAD - mr).max(0), (w.h() - ROW_H - 3 * PAD - mu).max(0));
    }
    let my = map_top(w, rows);
    (w.x() + PAD, my, w.w() - 2 * PAD, (w.y() + w.h() - my - ROW_H - PAD).max(0))
}

/// 四角の地の色（フォルダは深さで少しずつ明るく、ファイルは拡張子の色、「ほか N 件」は無彩色）。
fn base_color(c: &mapview::VCell, bg: (u8, u8, u8)) -> (u8, u8, u8) {
    match c.kind {
        Kind::Dir => mix(bg, color_text(), 8 + 6 * c.depth.min(5)),
        Kind::File => mix(treemap::color_of(&c.path), bg, 30), // 拡張子の色を、欄の地の色へ少し寄せる（配色から浮かないように）
        Kind::More(_) => mix(bg, color_text(), 22),
    }
}

/// 見えていて十分大きな（質感以上の）ファイルのうち、まだ読んでいないものを、大きい四角から数個だけ読む。
fn load_previews(st: &mut State, r: (i32, i32, i32, i32)) {
    st.pending = false;
    if st.head.is_none() {
        return;
    }
    let mut want: Vec<(f64, usize)> = Vec::new();
    for (i, c) in st.cells.iter().enumerate() {
        if c.kind != Kind::File || st.previews.contains(&c.path) {
            continue;
        }
        let s = st.cam.screen_rect(c.r);
        if Lod::of(s.w, s.h, INFO_SIZE as f64) >= Lod::Texture && cushion::pixel_rect(s, r).is_some() {
            want.push((s.w * s.h, i));
        }
    }
    want.sort_by(|a, b| b.0.total_cmp(&a.0));
    let t0 = Instant::now();
    for (n, (_, i)) in want.iter().enumerate() {
        if n >= PREVIEW_PER_DRAW || t0.elapsed() > PREVIEW_BUDGET {
            st.pending = true;
            break;
        }
        let path = st.cells[*i].path.clone();
        let p = match st.head.as_ref().and_then(|f| f(&path)) {
            Some(bytes) => mapprev::Preview::from_bytes(&bytes),
            None => mapprev::Preview::Unavailable,
        };
        st.previews.put(path, p);
    }
}

/// 四角 `(x, y, 幅, 高さ)` の中、縦 `top` から下に、中身を出す。`Texture` は単語の並びを細い棒で（本文の縮小表示と同じ規格）、
/// `Text` は実際の文字の行。字の色は地の色から決める。
fn draw_preview(p: &mapprev::Preview, lod: Lod, (x, y, cw, ch): (i32, i32, i32, i32), top: i32, base: (u8, u8, u8)) {
    let mapprev::Preview::Text { lines, runs } = p else { return };
    let bottom = y + ch;
    if top + LINE_H >= bottom {
        return;
    }
    let ink = text_on(base);
    draw::push_clip(x, y, cw, ch);
    if lod >= Lod::Text {
        draw::set_draw_color(rgb(mix(base, ink, 85)));
        for (i, line) in lines.iter().enumerate() {
            let ly = top + i as i32 * ROW_H;
            if ly >= bottom {
                break;
            }
            draw::draw_text2(line, x + 4, ly, cw - 8, ROW_H, Align::Left);
        }
    } else {
        draw::set_draw_color(rgb(mix(base, ink, 55)));
        for (i, line) in runs.iter().enumerate() {
            let ry = top + i as i32 * LINE_H;
            if ry + LINE_H - 1 > bottom {
                break;
            }
            for &(start, len) in line {
                let rx = x + 4 + (start as f64 * CHAR_W) as i32;
                let rw = ((len as f64 * CHAR_W) as i32).max(1).min(x + cw - 3 - rx);
                if rw > 0 {
                    draw::draw_rectf(rx, ry, rw, LINE_H - 1);
                }
            }
        }
    }
    draw::pop_clip();
}

/// 四角の 1 つの面（画面の四角 `s`）に、名前・大きさ・中身のプレビューを出す（平らな表示でも 3D でも同じ）。
fn draw_face_content(previews: &mut mapprev::Cache, c: &mapview::VCell, s: treemap::Rect, frame: (i32, i32, i32, i32), base: (u8, u8, u8)) {
    let Some((x, y, cw, ch)) = cushion::pixel_rect(s, frame) else { return };
    let lod = Lod::of(s.w, s.h, INFO_SIZE as f64);
    // 十分大きければ、名前（と大きさ）を出す。フォルダは中の四角に隠れるので出さない。
    if c.kind != Kind::Dir && lod >= Lod::Name {
        let name = match c.kind {
            Kind::More(n) => more_label(n),
            _ => c.path.rsplit('/').next().unwrap_or(&c.path).to_string(),
        };
        draw::push_clip(x, y, cw, ch);
        draw::set_draw_color(rgb(text_on(base)));
        draw::draw_text2(&name, x + 3, y + 1, cw - 6, ROW_H, Align::Left | Align::Top);
        if ch >= 2 * ROW_H {
            draw::draw_text2(&crate::neo::archive::human_size(c.size), x + 3, y + 1 + ROW_H, cw - 6, ROW_H, Align::Left | Align::Top);
        }
        draw::pop_clip();
    }
    // 十分大きなファイルの四角には、中身を出す（質感 → 実際の文字）。読み込み前や、バイナリは何も出さない。
    if c.kind == Kind::File && lod >= Lod::Texture {
        if let Some(p) = previews.get(&c.path) {
            let rows = if ch >= 2 * ROW_H { 2 } else { 1 };
            draw_preview(p, lod, (x, y, cw, ch), y + 3 + rows * ROW_H, base);
        }
    }
}

/// 乗っている四角は、文字の色の枠で囲む（画像は使い回すので、色を塗り替えない）。
fn draw_outline(s: treemap::Rect, frame: (i32, i32, i32, i32)) {
    if let Some((x, y, cw, ch)) = cushion::pixel_rect(s, frame) {
        draw::set_draw_color(rgb(color_text()));
        draw::draw_rect(x, y, cw, ch);
        if cw > 2 && ch > 2 {
            draw::draw_rect(x + 1, y + 1, cw - 2, ch - 2);
        }
    }
}

fn fill_quad(q: [(f64, f64); 4]) {
    draw::begin_polygon();
    for (x, y) in q {
        draw::vertex(x, y);
    }
    draw::end_polygon();
}

/// 平らな表示（立体感はクッション陰影の画像）。
fn draw_flat(st: &mut State, r: (i32, i32, i32, i32), bg: (u8, u8, u8)) {
    // 並べ直したときと配色が変わったときだけ作り、あとは使い回す
    let key: ShadeKey = (r, st.cam, bg, color_text(), st.cells_gen);
    if r.2 > 0 && r.3 > 0 && st.shade.as_ref().map(|s| s.key) != Some(key) {
        let img = cushion::render(&st.cells, &st.cam, r, bg, &|c| base_color(c, bg));
        st.shade = Some(Shade { key, img });
    }
    if let Some(sh) = &st.shade {
        let _ = draw::draw_image(&sh.img, r.0, r.1, r.2, r.3, ColorDepth::Rgb8);
    }
    let State { cells, previews, hover, cam, selected, .. } = st;
    for (i, c) in cells.iter().enumerate() {
        let s = cam.screen_rect(c.r);
        draw_face_content(previews, c, s, r, base_color(c, bg));
        if *hover == Some(i) || is_selected(selected, c) {
            draw_outline(s, r);
        }
    }
}

/// 3D 表示。奥から手前の順に、側面・上の面・中身を 1 つずつ描く（手前が奥を隠す）。
fn draw_solids(st: &mut State, r: (i32, i32, i32, i32), bg: (u8, u8, u8)) {
    let State { cells, solids, previews, hover, selected, .. } = st;
    for sol in solids.iter() {
        let c = &cells[sol.idx];
        let base = base_color(c, bg);
        if sol.faces_in_range() {
            draw::set_draw_color(rgb(mix(base, (0, 0, 0), 42))); // 手前（光が当たらない）
            fill_quad(sol.front());
            draw::set_draw_color(rgb(mix(base, (0, 0, 0), 22))); // 左
            fill_quad(sol.left());
        }
        if let Some((x, y, w, h)) = cushion::pixel_rect(sol.top, r) {
            draw::draw_rect_fill(x, y, w, h, rgb(base));
            if w > 2 && h > 2 {
                // 上の面の左と上の縁は光が当たって明るい
                draw::set_draw_color(rgb(mix(base, (255, 255, 255), 24)));
                draw::draw_line(x, y, x + w - 1, y);
                draw::draw_line(x, y, x, y + h - 1);
            }
        }
        draw_face_content(previews, c, sol.top, r, base);
        if *hover == Some(sol.idx) || is_selected(selected, c) {
            draw_outline(sol.top, r);
        }
    }
}

fn is_selected(selected: &Option<String>, c: &mapview::VCell) -> bool {
    !matches!(c.kind, Kind::More(_)) && selected.as_deref() == Some(c.path.as_str())
}

fn draw_tree(st: &mut State, w: &Widget) {
    let r = tree_rect(w, st.rows.len(), st.full, st.mode3d);
    st.ensure_cells(r);
    let bg = if st.full { color_main() } else { color_sub() };
    load_previews(st, r);
    draw::push_clip(r.0, r.1, r.2, r.3);
    if st.mode3d {
        draw_solids(st, r, bg);
    } else {
        draw_flat(st, r, bg);
    }
    draw::pop_clip();
    let label = match st.hover.and_then(|i| st.cells.get(i)) {
        Some(c) => {
            let what = match c.kind {
                Kind::More(n) if c.path.is_empty() => more_label(n),
                Kind::More(n) => format!("{} ({})", more_label(n), c.path),
                _ => c.path.clone(),
            };
            format!("{}  {}", crate::neo::archive::human_size(c.size), what)
        }
        None => st.tree.as_ref().map_or(String::new(), |t| {
            let size = crate::neo::archive::human_size(t.size);
            if crate::lang::is_en() { format!("Total  {size}") } else { format!("合計  {size}") }
        }),
    };
    // 拡大中は、倍率を右端に別に出す（長い道筋で倍率が切れないように、名前のほうを詰める）。
    let zoom = (st.cam.scale > 1.0).then(|| if st.cam.scale < 10.0 { format!("×{:.1}", st.cam.scale) } else { format!("×{:.0}", st.cam.scale) });
    let zoom_w = zoom.as_ref().map_or(0, |z| draw::width(z).ceil() as i32 + 6);
    let ly = r.1 + r.3 + 2;
    draw::set_draw_color(rgb(color_subtext()));
    draw::push_clip(r.0, ly, (r.2 - zoom_w).max(0), ROW_H);
    draw::draw_text2(&label, r.0, ly, r.2 - zoom_w, ROW_H, Align::Left);
    draw::pop_clip();
    if let Some(z) = zoom {
        draw::draw_text2(&z, r.0 + r.2 - zoom_w, ly, zoom_w, ROW_H, Align::Right);
    }
    // 読み込みの続きがあるなら、少し後にもう一度描く（読み込みは数個ずつ。画面を止めない）
    if st.pending {
        let mut again = w.clone();
        fltk::app::add_timeout3(0.02, move |_| again.redraw());
    }
}

/// カメラを変えた後の仕上げ: 並べ直し・乗っている四角の調べ直し・再描画。乗っている四角が変わったら（道筋, フォルダか）を返す。
fn camera_changed(w: &mut Widget, state: &Rc<RefCell<State>>, x: i32, y: i32) -> Option<Option<(String, bool)>> {
    let changed = {
        let mut st = state.borrow_mut();
        let r = tree_rect(w, st.rows.len(), st.full, st.mode3d);
        st.ensure_cells(r);
        st.update_hover(x, y)
    };
    w.redraw();
    changed
}

pub struct SidePanel {
    pub widget: Widget,
    pub state: Rc<RefCell<State>>,
}

/// 本文から縮小表示の行を作る。行が上限を超えたら true も返す。
pub fn runs_of(text: &str) -> (Vec<Runs>, bool) {
    let mut out = Vec::new();
    for line in text.split('\n') {
        if out.len() >= MAX_LINES {
            return (out, true);
        }
        let mut runs: Runs = Vec::new();
        let mut col = 0usize;
        let mut start: Option<usize> = None;
        for c in line.chars() {
            if col >= MAX_COLS {
                break;
            }
            let w = if c == '\t' { TAB_COLS - col % TAB_COLS } else { 1 };
            if c.is_whitespace() {
                if let Some(s) = start.take() {
                    runs.push((s as u16, (col - s) as u16));
                }
            } else if start.is_none() {
                start = Some(col);
            }
            col += w;
        }
        if let Some(s) = start {
            runs.push((s as u16, (col.min(MAX_COLS) - s) as u16));
        }
        out.push(runs);
    }
    (out, false)
}

/// 縮小表示に出す最初の行。全体が入るなら 0。入らなければ、見えている範囲の位置に比例させる。
pub fn map_start(total: i32, top: i32, visible: i32, capacity: i32) -> i32 {
    if total <= capacity || capacity <= 0 {
        return 0;
    }
    let scrollable = (total - visible).max(1);
    let ratio = (top as f64 / scrollable as f64).clamp(0.0, 1.0);
    (ratio * (total - capacity) as f64).round() as i32
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// 縮小表示が始まる y（情報の行の下）。
fn map_top(w: &Widget, rows: usize) -> i32 {
    w.y() + PAD + ROW_H * (rows as i32 + 1) + PAD
}

fn pin_hit(w: &Widget, x: i32, y: i32) -> bool {
    x >= w.x() + w.w() - 60 && y >= w.y() + PAD && y < w.y() + PAD + ROW_H
}

pub fn clamp_width(w: i32) -> i32 {
    w.clamp(WIDTH_MIN, WIDTH_MAX)
}

/// `on_jump(行)` は縮小表示のクリック・ドラッグ、`on_pin` は `Pin` の押下、
/// `on_resize(幅, 離した)` は境目のドラッグ中と離したときに呼ぶ。
/// `on_menu` は面積図の四角を右クリックした時に、その道筋（一覧の中の相対パス）とフォルダかを受け取る。
pub fn build(on_jump: impl Fn(i32) + 'static, on_pin: impl Fn() + 'static, on_resize: impl Fn(i32, bool) + 'static, on_menu: impl Fn(String, bool) + 'static, on_peek: impl Fn(Option<(String, bool)>, bool) + 'static) -> SidePanel {
    let state = Rc::new(RefCell::new(State::default()));
    let mut w = Widget::default();
    {
        let state = Rc::clone(&state);
        w.draw(move |w| {
            let mut st = state.borrow_mut();
            let font = st.font.unwrap_or(Font::Helvetica);
            if st.full {
                // メインの面積図: 情報欄は無く、全面に面積図を描く
                draw::draw_rect_fill(w.x(), w.y(), w.w(), w.h(), rgb(color_main()));
                draw::set_font(font, INFO_SIZE);
                if st.tree.is_some() {
                    draw_tree(&mut st, w);
                }
                return;
            }
            let (x, w_w) = (w.x(), w.w());
            draw::draw_rect_fill(x, w.y(), w_w, w.h(), rgb(color_sub()));
            draw::draw_rect_fill(x, w.y(), 1, w.h(), rgb(mix(color_sub(), color_text(), 15)));
            draw::push_clip(x, w.y(), w_w, w.h());
            draw::set_font(font, INFO_SIZE);
            let mut y = w.y() + PAD;
            draw::set_draw_color(rgb(color_subtext()));
            draw::draw_text2(crate::lang::tr("情報", "Info"), x + PAD, y, 60, ROW_H, Align::Left);
            // 固定中はアクセント色の押しピンと下線で示す（行番号欄の押しピンと同じ）
            draw::set_draw_color(rgb(if st.pinned { color_accent() } else { color_subtext() }));
            let px = x + w_w - PAD - super::icons::SIZE;
            super::icons::draw(super::icons::Icon::Pin, px, y + 1);
            if st.pinned {
                draw::draw_rect_fill(px, y + ROW_H, super::icons::SIZE, 2, rgb(color_accent()));
            }
            y += ROW_H;
            for (k, v) in &st.rows {
                draw::set_draw_color(rgb(color_subtext()));
                draw::draw_text2(k, x + PAD, y, KEY_W, ROW_H, Align::Left);
                draw::set_draw_color(rgb(color_text()));
                draw::push_clip(x + PAD + KEY_W, y, w_w - 2 * PAD - KEY_W, ROW_H);
                draw::draw_text2(v, x + PAD + KEY_W, y, w_w - 2 * PAD - KEY_W, ROW_H, Align::Left);
                draw::pop_clip();
                y += ROW_H;
            }
            let my = map_top(w, st.rows.len());
            if st.overlay {
                // 面積図はメインに出している。この下にはツリーが重なる
                draw::pop_clip();
                return;
            }
            if st.tree.is_some() {
                draw_tree(&mut st, w);
                draw::pop_clip();
                return;
            }
            draw::draw_rect_fill(x + PAD, my - PAD / 2, w_w - 2 * PAD, 1, rgb(mix(color_sub(), color_text(), 15)));
            let mh = w.y() + w.h() - my;
            let cap = mh / LINE_H;
            let total = st.lines.len() as i32;
            let start = map_start(total, st.top, st.visible, cap);
            // 見えている範囲の帯
            let vy = my + (st.top - start) * LINE_H;
            draw::draw_rect_fill(x + 1, vy, w_w - 1, (st.visible.max(1) * LINE_H).min(mh), rgb(mix(color_sub(), color_text(), 12)));
            draw::set_draw_color(rgb(mix(color_sub(), color_text(), 55)));
            for i in 0..cap.min(total - start) {
                let ly = my + i * LINE_H;
                for &(c, n) in &st.lines[(start + i) as usize] {
                    let rx = x + PAD + (c as f64 * CHAR_W) as i32;
                    let rw = ((n as f64 * CHAR_W).ceil() as i32).max(1).min(x + w_w - PAD - rx);
                    if rw > 0 {
                        draw::draw_rectf(rx, ly, rw, LINE_H - 1);
                    }
                }
            }
            draw::pop_clip();
        });
    }
    {
        let state = Rc::clone(&state);
        w.handle(move |w, ev| match ev {
            Event::Push if fltk::app::event_x() < w.x() + GRIP && !state.borrow().full => {
                state.borrow_mut().resizing = true;
                true
            }
            Event::Drag | Event::Released if state.borrow().resizing => {
                let done = ev == Event::Released;
                if done {
                    state.borrow_mut().resizing = false;
                }
                on_resize(clamp_width(w.x() + w.w() - fltk::app::event_x()), done);
                true
            }
            Event::Move | Event::Enter => {
                let edge = fltk::app::event_x() < w.x() + GRIP && !state.borrow().full;
                {
                    let mut st = state.borrow_mut();
                    if st.active() {
                        if let Some(cell) = st.update_hover(fltk::app::event_x(), fltk::app::event_y()) {
                            drop(st);
                            w.redraw();
                            on_peek(cell, false);
                        }
                    }
                }
                if let Some(mut win) = w.window() {
                    win.set_cursor(if edge { Cursor::WE } else { Cursor::Default });
                }
                true
            }
            Event::Leave => {
                if state.borrow_mut().hover.take().is_some() {
                    w.redraw();
                    on_peek(None, false);
                }
                if let Some(mut win) = w.window() {
                    win.set_cursor(Cursor::Default);
                }
                true
            }
            Event::Push if !state.borrow().full && pin_hit(w, fltk::app::event_x(), fltk::app::event_y()) => {
                on_pin();
                true
            }
            Event::Push if state.borrow().active() && fltk::app::event_mouse_button() == fltk::app::MouseButton::Right => {
                let hit = state.borrow().peek_at(fltk::app::event_x(), fltk::app::event_y());
                if let Some((path, is_dir)) = hit {
                    on_menu(path, is_dir);
                }
                true
            }
            // 真ん中のボタンで、全体に戻す。
            Event::Push if state.borrow().active() && fltk::app::event_mouse_button() == fltk::app::MouseButton::Middle => {
                let (ex, ey) = (fltk::app::event_x(), fltk::app::event_y());
                state.borrow_mut().cam = Camera::FIT;
                if let Some(c) = camera_changed(w, &state, ex, ey) {
                    on_peek(c, false);
                }
                true
            }
            // ホイールで、カーソルの下を動かさずに拡大・縮小（Ctrl+ホイールは文字の大きさなので、ここでは受けない）。
            Event::MouseWheel if state.borrow().active() && !fltk::app::is_event_command() => {
                let (ex, ey) = (fltk::app::event_x(), fltk::app::event_y());
                let notches = match fltk::app::event_dy() {
                    fltk::app::MouseWheel::Up => 1.0,
                    fltk::app::MouseWheel::Down => -1.0,
                    _ => 0.0,
                };
                if notches == 0.0 || !state.borrow().in_frame(ex, ey) {
                    return false;
                }
                {
                    let mut st = state.borrow_mut();
                    let frame = frame_of(st.cells_for);
                    st.cam.zoom_at(ex as f64, ey as f64, mapview::wheel_factor(notches));
                    st.cam.clamp(frame);
                }
                if let Some(c) = camera_changed(w, &state, ex, ey) {
                    on_peek(c, false);
                }
                true
            }
            // 左ボタン: 押したときは位置を覚えるだけ。動かさずに離せばクリック（プレビュー）、動かせばドラッグ（移動）。
            // ダブルクリックは、その四角へ合わせる（もう寄っているときは全体へ戻す）。
            Event::Push if state.borrow().active() && fltk::app::event_mouse_button() == fltk::app::MouseButton::Left => {
                let (ex, ey) = (fltk::app::event_x(), fltk::app::event_y());
                if !state.borrow().in_frame(ex, ey) {
                    return true;
                }
                if fltk::app::event_clicks() {
                    let open = {
                        let mut st = state.borrow_mut();
                        let open = st.focus_at(ex, ey);
                        st.press = Some(Press { sx: ex, sy: ey, lx: ex, ly: ey, moved: true }); // 離したときのクリックは無効
                        open
                    };
                    if let Some(path) = open {
                        if let Some(f) = &state.borrow().on_open {
                            f(path);
                        }
                    }
                    if let Some(c) = camera_changed(w, &state, ex, ey) {
                        on_peek(c, false);
                    }
                } else {
                    state.borrow_mut().press = Some(Press { sx: ex, sy: ey, lx: ex, ly: ey, moved: false });
                }
                true
            }
            Event::Drag if state.borrow().press.is_some() => {
                let (ex, ey) = (fltk::app::event_x(), fltk::app::event_y());
                let moved = {
                    let mut st = state.borrow_mut();
                    let zoomed = st.cam.scale > 1.0;
                    let frame = frame_of(st.cells_for);
                    let Some(p) = st.press.as_mut() else { return true };
                    if !p.moved && (ex - p.sx).abs().max((ey - p.sy).abs()) < DRAG_MIN {
                        return true;
                    }
                    p.moved = true;
                    let (dx, dy) = (ex - p.lx, ey - p.ly);
                    p.lx = ex;
                    p.ly = ey;
                    if zoomed {
                        st.cam.pan(dx as f64, dy as f64);
                        st.cam.clamp(frame);
                    }
                    zoomed
                };
                if moved {
                    if let Some(c) = camera_changed(w, &state, ex, ey) {
                        on_peek(c, false);
                    }
                }
                true
            }
            Event::Released if state.borrow().press.is_some() => {
                let press = state.borrow_mut().press.take();
                if let Some(Press { moved: false, sx, sy, .. }) = press {
                    let hit = state.borrow().peek_at(sx, sy);
                    if hit.is_some() {
                        on_peek(hit, true);
                    }
                }
                true
            }
            Event::Push | Event::Drag if state.borrow().active() => true,
            Event::Push | Event::Drag => {
                let line = {
                    let st = state.borrow();
                    let my = map_top(w, st.rows.len());
                    let cap = (w.y() + w.h() - my) / LINE_H;
                    let total = st.lines.len() as i32;
                    let ey = fltk::app::event_y();
                    if ev == Event::Push && ey < my {
                        return true;
                    }
                    let start = map_start(total, st.top, st.visible, cap);
                    (start + (ey - my).max(0) / LINE_H).clamp(0, (total - 1).max(0))
                };
                on_jump(line);
                true
            }
            Event::Released => true,
            _ => false,
        });
    }
    SidePanel { widget: w, state }
}

/// メインの面積図（部品の全面に描く）。操作（ホイール・ドラッグ・ダブルクリック・中ボタン・右クリック・乗せる）は、右の欄と同じ。
pub fn build_main(on_menu: impl Fn(String, bool) + 'static, on_peek: impl Fn(Option<(String, bool)>, bool) + 'static) -> SidePanel {
    let sp = build(|_| {}, || {}, |_, _| {}, on_menu, on_peek);
    sp.state.borrow_mut().full = true;
    sp
}

impl SidePanel {
    /// 面積図をメインに出している間、右の欄の下に重ねるツリーの置き場所（情報の行の下から、下端まで。左の境目は空ける）。
    pub fn tree_area(&self) -> (i32, i32, i32, i32) {
        let rows = self.state.borrow().rows.len();
        let w = &self.widget;
        let my = map_top(w, rows);
        (w.x() + GRIP, my, (w.w() - GRIP).max(0), (w.y() + w.h() - my).max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_skip_spaces_and_expand_tabs() {
        let (lines, cut) = runs_of("ab  cd\n\tx\n\n");
        assert!(!cut);
        assert_eq!(lines, vec![vec![(0, 2), (4, 2)], vec![(4, 1)], vec![], vec![]]);
    }

    #[test]
    fn long_lines_and_many_lines_are_capped() {
        let (lines, _) = runs_of(&"x".repeat(MAX_COLS + 50));
        assert_eq!(lines[0], vec![(0, MAX_COLS as u16)]);
        let (lines, cut) = runs_of(&"a\n".repeat(MAX_LINES + 5));
        assert!(cut);
        assert_eq!(lines.len(), MAX_LINES);
    }

    #[test]
    fn map_start_follows_the_view_proportionally() {
        assert_eq!(map_start(50, 10, 20, 100), 0); // 全体が入る
        assert_eq!(map_start(1000, 0, 50, 100), 0);
        assert_eq!(map_start(1000, 950, 50, 100), 900); // 末尾
        assert_eq!(map_start(1000, 475, 50, 100), 450); // 真ん中
    }

    // ---- ツリーマップの拡大（State だけ。画面の部品は使わない）----

    fn sample_state() -> State {
        use crate::neo::archive::Entry;
        let mut ents = Vec::new();
        for d in 0..6u64 {
            for i in 0..40u64 {
                ents.push(Entry { path: format!("dir{d}/file{i}.txt"), is_dir: false, size: 1 + (d * 40 + i) * 997 % 50_000, packed: None });
            }
        }
        let mut st = State::default();
        st.set_tree(Some(treemap::build(&ents)));
        st.ensure_cells((10, 20, 300, 200));
        st
    }

    #[test]
    fn focus_path_zooms_to_the_folder_and_marks_the_item() {
        let mut st = sample_state();
        st.focus_path("dir3/file5.txt");
        assert_eq!(st.selected.as_deref(), Some("dir3/file5.txt"));
        assert!(st.cam.scale > 1.0, "ファイルは入っているフォルダへ寄る");
        st.ensure_cells(st.cells_for);
        assert!(st.cells.iter().any(|c| c.path == "dir3/file5.txt"), "選んだ項目が見えている");
        let before = st.cam;
        st.focus_path("dir9");
        assert_eq!(st.cam, before, "見つからない項目では動かない");
        // 枠が決まる前に頼まれた寄せ先は、最初に並べるときに合わせる。
        let mut fresh = State::default();
        fresh.set_tree(st.tree.take());
        fresh.focus_path("dir1");
        assert_eq!(fresh.cam, Camera::FIT);
        fresh.ensure_cells((10, 20, 300, 200));
        assert!(fresh.cam.scale > 1.0);
    }

    #[test]
    fn zoom_relays_out_and_a_new_frame_resets_the_view() {
        let mut st = sample_state();
        let at_fit = st.cells.len();
        assert!(at_fit > 0);
        st.cam.zoom_at(100.0, 80.0, 6.0);
        st.cam.clamp(treemap::Rect { x: 10.0, y: 20.0, w: 300.0, h: 200.0 });
        st.ensure_cells((10, 20, 300, 200));
        assert!(st.cells.len() < at_fit, "拡大すると、見えている四角だけになる");
        assert_eq!(st.cells_cam, Some(st.cam));
        // 枠が変わると全体に戻る。
        st.ensure_cells((10, 20, 320, 200));
        assert_eq!(st.cam, Camera::FIT);
        assert_eq!(st.cells_for, (10, 20, 320, 200));
        assert!(!st.cells.is_empty() && st.cells.iter().all(|c| c.r.x + c.r.w <= 330.0 + 1e-6 && c.r.x >= 10.0 - 1e-6), "新しい枠の中に並べ直される");
        // 木を替えても全体に戻る。
        st.cam.zoom_at(100.0, 80.0, 3.0);
        st.set_tree(None);
        assert_eq!(st.cam, Camera::FIT);
        assert!(st.cells.is_empty());
    }

    #[test]
    fn hover_and_peek_ignore_the_outside_of_the_frame() {
        let mut st = sample_state();
        assert!(st.update_hover(5, 5).is_none(), "枠の外では何も乗らない");
        assert!(st.peek_at(5, 5).is_none());
        let inside = st.update_hover(150, 120);
        assert!(inside.is_some(), "枠の中では乗っている四角が決まる");
        assert!(st.update_hover(150, 120).is_none(), "同じ四角なら変わらない");
        // 拡大して枠の外へはみ出す四角にも、枠の外では乗らない。
        st.cam.zoom_at(150.0, 120.0, 50.0);
        st.ensure_cells((10, 20, 300, 200));
        assert!(st.peek_at(150, 10).is_none());
    }

    #[test]
    fn double_click_focuses_then_returns_to_fit() {
        let mut st = sample_state();
        st.focus_at(150, 120);
        assert!(st.cam.scale > 1.5, "その四角へ寄る: {}", st.cam.scale);
        st.ensure_cells((10, 20, 300, 200));
        // 寄った四角の上でもう一度 → もう寄れないので全体へ戻る。
        st.focus_at(160, 120);
        assert_eq!(st.cam, Camera::FIT);
        // 枠の外では何もせず全体のまま。
        st.focus_at(5, 5);
        assert_eq!(st.cam, Camera::FIT);
    }

    #[test]
    fn labels_use_a_readable_text_colour() {
        assert_eq!(text_on((250, 250, 250)), (20, 20, 20));
        assert_eq!(text_on((10, 20, 30)), (235, 235, 235));
        assert_eq!(more_label(3), if crate::lang::is_en() { "3 more" } else { "ほか 3 件" });
    }

    #[test]
    fn previews_load_only_for_big_cells_a_few_at_a_time_and_failures_are_remembered() {
        let frame = (10, 20, 300, 200);
        let mut st = sample_state();
        st.head = Some(Box::new(|p| Some(format!("// {p}\nfn main() {{}}\n").into_bytes())));
        load_previews(&mut st, frame);
        assert_eq!(st.previews.len(), 0, "全体では、どの四角も中身を出すほど大きくない");
        assert!(!st.pending);
        st.cam.zoom_at(150.0, 120.0, 20.0);
        st.cam.clamp(frame_of(frame));
        st.ensure_cells(frame);
        load_previews(&mut st, frame);
        let n = st.previews.len();
        assert!((1..=PREVIEW_PER_DRAW).contains(&n), "1 回の描画で読むのは数個まで: {n}");
        // 続けて呼ぶと、読み終わって pending が消える。読んだ物は読み直さない
        for _ in 0..30 {
            if !st.pending {
                break;
            }
            load_previews(&mut st, frame);
        }
        assert!(!st.pending);
        let loaded = st.previews.len();
        load_previews(&mut st, frame);
        assert_eq!(st.previews.len(), loaded);
        // 読めなかった物（None）も覚える。読み込みが無いとき（zip の中など）は何も読まない
        let mut st2 = sample_state();
        st2.head = Some(Box::new(|_| None));
        st2.cam.zoom_at(150.0, 120.0, 20.0);
        st2.cam.clamp(frame_of(frame));
        st2.ensure_cells(frame);
        load_previews(&mut st2, frame);
        assert!(st2.previews.len() >= 1);
        let first = st2.cells.iter().find(|c| st2.previews.contains(&c.path)).unwrap().path.clone();
        assert_eq!(st2.previews.get(&first), Some(&mapprev::Preview::Unavailable));
        let mut st3 = sample_state();
        st3.cam.zoom_at(150.0, 120.0, 20.0);
        st3.cam.clamp(frame_of(frame));
        st3.ensure_cells(frame);
        load_previews(&mut st3, frame);
        assert_eq!(st3.previews.len(), 0);
        // 木を替えると、置いてあるプレビューは捨てる
        st.set_tree(None);
        assert_eq!(st.previews.len(), 0);
    }

    // ---- メインの面積図（全面・3D・ファイルを開く）----

    fn main_state() -> State {
        let mut st = sample_state();
        st.full = true;
        st
    }

    #[test]
    fn main_map_double_click_opens_a_file_that_is_already_focused() {
        let frame = (10, 20, 300, 200);
        let mut st = main_state();
        // 1 回目: その四角へ寄る（開かない）
        assert!(st.focus_at(150, 120).is_none());
        assert!(st.cam.scale > 1.5);
        st.ensure_cells(frame);
        // 2 回目: もう寄っているファイルなので、道筋を返して開く（戻らない）
        let before = st.cam;
        let open = st.focus_at(150, 120);
        assert!(open.as_deref().is_some_and(|p| p.ends_with(".txt")), "{open:?}");
        assert_eq!(st.cam, before, "開くときは、拡大を戻さない");
        // 右の欄（全面ではない）では、これまでどおり全体へ戻る
        let mut side = sample_state();
        side.focus_at(150, 120);
        side.ensure_cells(frame);
        assert!(side.focus_at(150, 120).is_none());
        assert_eq!(side.cam, Camera::FIT);
    }

    #[test]
    fn mode_3d_lays_out_solids_and_hits_the_top_face() {
        let frame = (10, 20, 300, 200);
        let mut st = main_state();
        assert!(st.solids.is_empty());
        st.set_mode3d(true);
        st.ensure_cells(frame);
        assert_eq!(st.solids.len(), st.cells.len(), "四角ごとに押し出しがある");
        // どの四角も、上の面の中心では、その四角か、上に載った子が当たる
        let top = st.solids.iter().find(|s| st.cells[s.idx].kind == Kind::File).unwrap().clone();
        let (cx, cy) = ((top.top.x + top.top.w / 2.0) as i32, (top.top.y + top.top.h / 2.0) as i32);
        if st.in_frame(cx, cy) {
            assert!(st.hit_at(cx, cy).is_some());
        }
        // 元に戻すと、押し出しは空になる（並べ直し）
        st.set_mode3d(false);
        st.ensure_cells(frame);
        assert!(st.solids.is_empty());
    }

    #[test]
    fn overlay_state_is_not_active_so_the_tree_widget_gets_the_events() {
        let mut st = sample_state();
        assert!(st.active());
        st.overlay = true;
        assert!(!st.active(), "面積図をメインに出している間、右の欄の面積図は操作を受けない");
        st.overlay = false;
        st.set_tree(None);
        assert!(!st.active());
    }

    #[test]
    fn map_mode_defaults_to_the_tree() {
        assert_eq!(MapMode::default(), MapMode::Tree);
    }
}
