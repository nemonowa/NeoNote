// 右端の欄。上にファイルの情報（名前・場所・大きさ・更新日時・文字数・画像や音声の情報）、
// 下に本文全体の縮小表示（VSCode のミニマップのように、空白でない文字を小さな四角で描く）。
// 縮小表示をクリック・ドラッグすると、その行が見えるように本文を動かす。
// 行番号欄と同じく、マウスが右端にあるときだけ出す（`Pin` で常に出す）。出し方の判定は呼び出し側。
use super::theme::*;
use fltk::{
    draw,
    enums::{Align, Color, Event, Font},
    prelude::*,
    widget::Widget,
};
use std::{cell::RefCell, rc::Rc};

pub const WIDTH: i32 = 200;
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

/// `on_jump(行)` は縮小表示のクリック・ドラッグ、`on_pin` は `Pin` の押下で呼ぶ。
pub fn build(on_jump: impl Fn(i32) + 'static, on_pin: impl Fn() + 'static) -> SidePanel {
    let state = Rc::new(RefCell::new(State::default()));
    let mut w = Widget::default();
    {
        let state = Rc::clone(&state);
        w.draw(move |w| {
            let st = state.borrow();
            let font = st.font.unwrap_or(Font::Helvetica);
            let (x, w_w) = (w.x(), w.w());
            draw::draw_rect_fill(x, w.y(), w_w, w.h(), rgb(color_sub()));
            draw::draw_rect_fill(x, w.y(), 1, w.h(), rgb(mix(color_sub(), color_text(), 15)));
            draw::push_clip(x, w.y(), w_w, w.h());
            draw::set_font(font, INFO_SIZE);
            let mut y = w.y() + PAD;
            draw::set_draw_color(rgb(color_subtext()));
            draw::draw_text2("Info", x + PAD, y, 60, ROW_H, Align::Left);
            draw::set_draw_color(rgb(if st.pinned { color_accent() } else { color_subtext() }));
            draw::draw_text2(if st.pinned { "Pinned" } else { "Pin" }, x + w_w - 60, y, 60 - PAD, ROW_H, Align::Right);
            if st.pinned {
                // 固定中はツールバーの `#` と同じく、アクセント色の下線で示す
                let tw = draw::width("Pinned") as i32;
                draw::draw_rect_fill(x + w_w - PAD - tw, y + ROW_H - 1, tw, 2, rgb(color_accent()));
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
            Event::Push if pin_hit(w, fltk::app::event_x(), fltk::app::event_y()) => {
                on_pin();
                true
            }
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
            Event::Released | Event::Enter | Event::Leave | Event::Move => true,
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
