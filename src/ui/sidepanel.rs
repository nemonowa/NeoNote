// 右端の欄。上にファイルの情報（名前・場所・大きさ・更新日時・文字数・画像や音声の情報）、
// 下に本文全体の縮小表示（VSCode のミニマップのように、空白でない文字を小さな四角で描く）。
// フォルダ・zip のときは縮小表示の代わりに、中身の大きさのツリーマップ（treemap.rs）を描く。
// 縮小表示をクリック・ドラッグすると、その行が見えるように本文を動かす。
// 左の境目をドラッグすると幅を変えられる（WIDTH_MIN〜WIDTH_MAX）。
// 行番号欄と同じく、マウスが右端にあるときだけ出す（`Pin` で常に出す）。出し方の判定は呼び出し側。
use super::{theme::*, treemap};
use fltk::{
    draw,
    enums::{Align, Color, Cursor, Event, Font},
    prelude::*,
    widget::Widget,
};
use std::{cell::RefCell, rc::Rc};

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
    /// 並べた四角と、並べたときの枠（枠が変わったら並べ直す）。
    cells: Vec<treemap::Cell>,
    cells_for: (i32, i32, i32, i32),
    /// マウスが乗っている四角（cells の番号）。
    hover: Option<usize>,
}

impl State {
    /// 木を差し替える（並べ直しは次に描くとき）。
    pub fn set_tree(&mut self, tree: Option<treemap::Node>) {
        self.tree = tree;
        self.cells.clear();
        self.cells_for = (0, 0, 0, 0);
        self.hover = None;
    }

    fn ensure_cells(&mut self, r: (i32, i32, i32, i32)) {
        if self.cells_for == r {
            return;
        }
        self.cells_for = r;
        self.hover = None;
        self.cells = match &self.tree {
            Some(t) => treemap::layout(t, treemap::Rect { x: r.0 as f64, y: r.1 as f64, w: r.2 as f64, h: r.3 as f64 }, 2.0, 3.0),
            None => Vec::new(),
        };
    }
}

/// ツリーマップを描く枠（縮小表示と同じ場所。下の 1 行は乗っている四角の名前）。
fn tree_rect(w: &Widget, rows: usize) -> (i32, i32, i32, i32) {
    let my = map_top(w, rows);
    (w.x() + PAD, my, w.w() - 2 * PAD, (w.y() + w.h() - my - ROW_H - PAD).max(0))
}

fn draw_tree(st: &mut State, w: &Widget) {
    let r = tree_rect(w, st.rows.len());
    st.ensure_cells(r);
    let bg = color_sub();
    for (i, c) in st.cells.iter().enumerate() {
        let (x, y) = (c.r.x.round() as i32, c.r.y.round() as i32);
        let (cw, ch) = ((c.r.x + c.r.w).round() as i32 - x, (c.r.y + c.r.h).round() as i32 - y);
        let base = if c.is_dir { mix(bg, color_text(), 8 + 6 * c.depth.min(5)) } else { mix(treemap::color_of(&c.path), bg, 30) }; // 拡張子の色を、欄の地の色へ少し寄せる（配色から浮かないように）
        let col = if st.hover == Some(i) { mix(base, color_text(), 35) } else { base };
        draw::draw_rect_fill(x, y, cw, ch, rgb(col));
        if !c.is_dir && cw > 2 && ch > 2 {
            draw::set_draw_color(rgb(mix(base, (0, 0, 0), 30)));
            draw::draw_rect(x, y, cw, ch);
        }
    }
    let label = match st.hover.and_then(|i| st.cells.get(i)) {
        Some(c) => format!("{}  {}", crate::neo::archive::human_size(c.size), c.path),
        None => st.tree.as_ref().map_or(String::new(), |t| format!("Total  {}", crate::neo::archive::human_size(t.size))),
    };
    draw::set_draw_color(rgb(color_subtext()));
    draw::draw_text2(&label, r.0, r.1 + r.3 + 2, r.2, ROW_H, Align::Left);
}

/// (x, y) にある、いちばん奥の四角。
fn tree_hit(st: &State, x: i32, y: i32) -> Option<usize> {
    let (x, y) = (x as f64, y as f64);
    st.cells.iter().rposition(|c| x >= c.r.x && x < c.r.x + c.r.w && y >= c.r.y && y < c.r.y + c.r.h)
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
pub fn build(on_jump: impl Fn(i32) + 'static, on_pin: impl Fn() + 'static, on_resize: impl Fn(i32, bool) + 'static) -> SidePanel {
    let state = Rc::new(RefCell::new(State::default()));
    let mut w = Widget::default();
    {
        let state = Rc::clone(&state);
        w.draw(move |w| {
            let mut st = state.borrow_mut();
            let font = st.font.unwrap_or(Font::Helvetica);
            let (x, w_w) = (w.x(), w.w());
            draw::draw_rect_fill(x, w.y(), w_w, w.h(), rgb(color_sub()));
            draw::draw_rect_fill(x, w.y(), 1, w.h(), rgb(mix(color_sub(), color_text(), 15)));
            draw::push_clip(x, w.y(), w_w, w.h());
            draw::set_font(font, INFO_SIZE);
            let mut y = w.y() + PAD;
            draw::set_draw_color(rgb(color_subtext()));
            draw::draw_text2("Info", x + PAD, y, 60, ROW_H, Align::Left);
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
            Event::Push if fltk::app::event_x() < w.x() + GRIP => {
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
                let edge = fltk::app::event_x() < w.x() + GRIP;
                {
                    let mut st = state.borrow_mut();
                    if st.tree.is_some() {
                        let h = tree_hit(&st, fltk::app::event_x(), fltk::app::event_y());
                        if h != st.hover {
                            st.hover = h;
                            drop(st);
                            w.redraw();
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
                }
                if let Some(mut win) = w.window() {
                    win.set_cursor(Cursor::Default);
                }
                true
            }
            Event::Push if pin_hit(w, fltk::app::event_x(), fltk::app::event_y()) => {
                on_pin();
                true
            }
            Event::Push | Event::Drag if state.borrow().tree.is_some() => true,
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
}
