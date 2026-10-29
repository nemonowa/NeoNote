// タブバー（ツールバーの下の 1 行）。ファイル名・未保存の `*`・閉じる `×`・新規 `+`。
// 文書の増減や名前の変化のたびにボタンを作り直す（数が少なく、作り直しで十分軽い）。
// ボタンの中のコールバックで自分自身を消さないよう、操作は TabCmd として呼び出し側へ返し、
// 呼び出し側がメインループで処理する。
use super::theme::*;
use fltk::{
    app,
    button::Button,
    draw,
    enums::{Align, Color, Cursor, Event, FrameType},
    frame::Frame,
    group::{Flex, Group},
    prelude::*,
};
use std::{cell::Cell, path::Path, rc::Rc};

pub const HEIGHT: i32 = 28;
/// 右端の表示モードのチップの幅。
pub const CHIP_W: i32 = 104;
/// 表示（Text・Preview・HEX）のチップの幅。
pub const VIEW_CHIP_W: i32 = 96;
const MIN_W: i32 = 48;
const MAX_W: i32 = 160;
const CLOSE_W: i32 = 24;
const PLUS_W: i32 = 32;
/// これ以上横に動かしたら、クリックではなくドラッグ（並べ替え）と見なす。
const DRAG_PX: i32 = 6;

/// 1 つのタブの表示内容。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TabInfo {
    pub id: u64,
    pub label: String,
    /// ホバー時に出す全文（パス）。
    pub tip: String,
    pub active: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabCmd {
    Activate(u64),
    Close(u64),
    New,
    /// 並べ替え（文書の id, 移動先の位置。全タブの中での 0 始まりの番号）。
    Move(u64, usize),
}

/// タブに出す名前。未保存の新規は "Untitled"、変更ありは先頭に `*`、読込中は `Loading... `。
pub fn tab_label(path: Option<&Path>, dirty: bool, loading: bool) -> String {
    let name = path
        .map(|p| crate::log::file_name(&p.to_string_lossy()).to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| crate::lang::tr("無題", "Untitled").to_string());
    let mark = if dirty { "*" } else { "" };
    if loading {
        if crate::lang::is_en() { format!("Loading... {name}") } else { format!("読込中... {name}") }
    } else {
        format!("{mark}{name}")
    }
}

/// FLTK のラベルは `&`（ショートカット）と `@`（記号）を特別扱いするので、重ねて無効にする。
pub fn escape_label(s: &str) -> String {
    s.replace('&', "&&").replace('@', "@@")
}

/// タブ 1 つ分（名前 + ×）の幅。収まるよう縮め、下限と上限を設ける。
pub fn tab_width(count: usize, avail: i32) -> i32 {
    let n = count.max(1) as i32;
    ((avail - PLUS_W) / n).clamp(MIN_W, MAX_W)
}

/// 幅に収まらないほど多いとき、表示するタブの範囲。選ばれているタブは必ず含める
/// （隠れたタブは Ctrl+Tab や、選ばれて範囲が動いたときに現れる）。
pub fn visible_range(count: usize, active: usize, avail: i32) -> std::ops::Range<usize> {
    let max = (((avail - PLUS_W) / MIN_W).max(1)) as usize;
    if count <= max {
        return 0..count;
    }
    let start = active.saturating_sub(max / 2).min(count - max);
    start..start + max
}

/// ドラッグ中のマウスが、表示しているタブの何番目の上にあるか。`x_rel` はバーの左端からの距離。
/// 端より外は両端のタブに寄せる。
pub fn drop_slot(x_rel: i32, tab_w: i32, visible: usize) -> usize {
    if visible == 0 {
        return 0;
    }
    ((x_rel.max(0) / tab_w.max(1)) as usize).min(visible - 1)
}

/// `from` の要素を取り出して `to` の位置に入れる（`to` は入れ終わったあとの位置）。動いたら true。
pub fn reorder<T>(v: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= v.len() {
        return false;
    }
    let to = to.min(v.len() - 1);
    if from == to {
        return false;
    }
    let item = v.remove(from);
    v.insert(to, item);
    true
}

/// タブバーの右端のチップ（いまの表示モード / 文字コード + ▼）。押すと表示・文字コード・改行の一覧が出る。
/// 詳しい状態（行・桁・文字数・改行コード・パス）はツールチップへ。▼ はフォントに頼らず描く。
pub fn build_chip() -> Button {
    let mut b = Button::default();
    b.set_frame(FrameType::FlatBox);
    b.set_down_frame(FrameType::FlatBox);
    b.set_label_size(12);
    b.clear_visible_focus();
    restyle_chip(&mut b);
    b.draw(|b| {
        draw::draw_box(b.frame(), b.x(), b.y(), b.w(), b.h(), b.color());
        draw::set_draw_color(b.label_color());
        draw::set_font(b.label_font(), b.label_size());
        let label = b.label();
        let mut tx = b.x() + 8;
        if label == "Preview" {
            super::icons::draw(super::icons::Icon::Eye, tx, b.y() + (b.h() - super::icons::SIZE) / 2);
            tx += super::icons::SIZE + 4;
        }
        draw::draw_text2(&label, tx, b.y(), b.x() + b.w() - 18 - tx, b.h(), Align::Left | Align::Inside | Align::Clip);
        let (cx, cy) = (b.x() + b.w() - 14, b.y() + b.h() / 2);
        draw::draw_polygon(cx - 4, cy - 2, cx + 4, cy - 2, cx, cy + 3);
    });
    b
}

/// 配色を切り替えたあと、チップの色を今のパレットに合わせ直す。
/// タブ行の左端、行番号欄の真上に置く `#`（行番号欄を常に出すかの切り替え）。幅は行番号欄に合わせて呼び出し側が決める。
/// 右の境目をドラッグすると `on_resize(幅, 離した)` を呼ぶ（行番号欄の幅の調整）。押すと callback（固定の切り替え）。
pub fn build_gutter_pin(on_resize: impl Fn(i32, bool) + 'static) -> Button {
    const GRIP: i32 = 5;
    let mut b = Button::default();
    b.set_frame(FrameType::FlatBox);
    b.set_down_frame(FrameType::FlatBox);
    b.clear_visible_focus();
    b.super_draw(false);
    b.draw(|b| {
        draw::draw_box(FrameType::FlatBox, b.x(), b.y(), b.w(), b.h(), rgb(color_sub()));
        let pinned = b.value();
        draw::set_draw_color(rgb(if pinned { color_accent() } else { color_subtext() }));
        draw::set_font(fltk::enums::Font::HelveticaBold, 14);
        let tw = draw::width("#") as i32;
        let x = b.x() + (b.w() - tw) / 2;
        draw::draw_text2("#", x, b.y(), tw + 2, b.h(), Align::Left);
        if pinned {
            draw::draw_rect_fill(b.x() + (b.w() - 12) / 2, b.y() + b.h() - 3, 12, 2, rgb(color_accent()));
        }
        draw::draw_rect_fill(b.x() + b.w() - 1, b.y() + 6, 1, b.h() - 12, rgb(mix(color_sub(), color_text(), 14)));
    });
    let resizing = Rc::new(Cell::new(false));
    b.super_handle_first(false);
    b.handle(move |w, ev| {
        let edge = app::event_x() >= w.x() + w.w() - GRIP;
        match ev {
            Event::Push if edge => {
                resizing.set(true);
                true
            }
            Event::Drag | Event::Released if resizing.get() => {
                let done = ev == Event::Released;
                if done {
                    resizing.set(false);
                }
                on_resize(app::event_x() - w.x(), done);
                true
            }
            Event::Move | Event::Enter => {
                if let Some(mut win) = w.window() {
                    win.set_cursor(if edge { Cursor::WE } else { Cursor::Default });
                }
                true
            }
            Event::Leave => {
                if let Some(mut win) = w.window() {
                    win.set_cursor(Cursor::Default);
                }
                true
            }
            _ => false,
        }
    });
    b
}

pub fn restyle_chip(b: &mut Button) {
    b.set_color(rgb(color_sub()));
    b.set_selection_color(rgb(color_accent()));
    b.set_label_color(rgb(color_subtext()));
    b.redraw();
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

fn style(b: &mut Button, active: bool) {
    b.set_frame(FrameType::FlatBox);
    b.set_down_frame(FrameType::FlatBox);
    b.set_color(if active { rgb(color_main()) } else { rgb(color_tab_inactive()) });
    b.set_selection_color(rgb(color_accent()));
    b.set_label_color(if active { rgb(color_text()) } else { rgb(color_subtext()) });
    b.clear_visible_focus(); // キーボードフォーカスは編集領域に残す
    b.super_handle_first(false);
    if active {
        // 選ばれているタブは、上端のアクセント色の細線で示す
        b.draw(|b| draw::draw_rect_fill(b.x(), b.y(), b.w(), 2, rgb(color_accent())));
    }
}

thread_local! {
    /// ドラッグ中に、落とす先へ引く縦線の x（ウィンドウ内の座標）。None なら引かない。
    static DROP_X: Cell<Option<i32>> = const { Cell::new(None) };
}

/// ドラッグ中の見た目: 掴んでいるタブ（`from`）を沈んだ色にし、入る先に縦線を引く。
/// `target` が None なら元に戻す。バーの子は「名前, ×」の組が並ぶ（先頭から `actives.len()` 組）。
fn paint_target(bar: &mut impl GroupExt, actives: &[bool], from: usize, target: Option<usize>, tab_w: i32) {
    for (i, &active) in actives.iter().enumerate() {
        let (bg, fg) = if target.is_some() && i == from {
            (mix(color_sub(), color_accent(), 25), color_text())
        } else if active {
            (color_main(), color_text())
        } else {
            (color_tab_inactive(), color_subtext())
        };
        for k in [2 * i, 2 * i + 1] {
            if let Some(mut c) = bar.child(k as i32) {
                c.set_color(rgb(bg));
                c.set_label_color(rgb(fg));
                c.redraw();
            }
        }
    }
    // 入れ終わったあとの位置が target なので、左へ動かすなら target の左端、右へなら右端に線を引く
    let x = target.filter(|t| *t != from).map(|t| bar.x() + if t < from { t } else { t + 1 } as i32 * tab_w);
    DROP_X.with(|d| d.set(x));
    bar.redraw();
}

/// バーの中身を作り直す。`avail` はバーに使える幅（ウィンドウ幅）。
pub fn rebuild<F: Fn(TabCmd) + Clone + 'static>(bar: &mut Flex, tabs: &[TabInfo], avail: i32, on_cmd: F) {
    bar.clear();
    bar.begin();
    bar.set_pad(0);
    bar.set_margin(0);
    let active = tabs.iter().position(|t| t.active).unwrap_or(0);
    let range = visible_range(tabs.len(), active, avail);
    let first = range.start;
    let tabs = &tabs[range];
    let w = tab_width(tabs.len(), avail);
    let actives: Rc<Vec<bool>> = Rc::new(tabs.iter().map(|t| t.active).collect());
    for (index, t) in tabs.iter().enumerate() {
        let id = t.id;

        let mut name = Button::default().with_label(&escape_label(&format!(" {}", t.label)));
        style(&mut name, t.active);
        name.set_align(Align::Left | Align::Inside | Align::Clip);
        name.set_tooltip(&t.tip);
        bar.fixed(&name, w - CLOSE_W);
        let cb = on_cmd.clone();
        name.set_callback(move |_| cb(TabCmd::Activate(id)));
        let cb = on_cmd.clone();
        // ドラッグで並べ替え。動かしている間はバーを作り直さず（掴んでいるボタンが消えるため）、
        // 落とす先の色を変えるだけにして、離したときに Move を返す。
        let drag: Rc<Cell<Option<(i32, bool)>>> = Rc::new(Cell::new(None)); // (押した x, ドラッグ中か)
        let actives = Rc::clone(&actives);
        let n_shown = tabs.len();
        name.handle(move |btn, ev| {
            let slot_at_mouse = |btn: &Button| {
                let bar_x = btn.parent().map_or(0, |p| p.x());
                drop_slot(app::event_x() - bar_x, w, n_shown)
            };
            match ev {
                Event::Push => match app::event_mouse_button() {
                    app::MouseButton::Middle => {
                        cb(TabCmd::Close(id)); // 中クリックで閉じる
                        true
                    }
                    app::MouseButton::Left => {
                        drag.set(Some((app::event_x(), false)));
                        false // 押した見た目と、動かさなかったときの切替はボタン本来の処理に任せる
                    }
                    _ => false,
                },
                Event::Drag => {
                    let Some((x0, dragging)) = drag.get() else { return false };
                    if !dragging && (app::event_x() - x0).abs() < DRAG_PX {
                        return false;
                    }
                    drag.set(Some((x0, true)));
                    if let (Some(mut bar), Some(mut win)) = (btn.parent(), btn.window()) {
                        win.set_cursor(Cursor::Move);
                        paint_target(&mut bar, &actives, index, Some(slot_at_mouse(btn)), w);
                    }
                    true
                }
                Event::Released => {
                    let was = drag.replace(None);
                    if !matches!(was, Some((_, true))) {
                        return false;
                    }
                    if let Some(mut win) = btn.window() {
                        win.set_cursor(Cursor::Default);
                    }
                    DROP_X.with(|d| d.set(None));
                    cb(TabCmd::Move(id, first + slot_at_mouse(btn)));
                    true
                }
                _ => false,
            }
        });

        let mut close = Button::default().with_label("×");
        style(&mut close, t.active);
        close.set_tooltip(crate::lang::tr("閉じる（Ctrl+W）", "Close (Ctrl+W)"));
        bar.fixed(&close, CLOSE_W);
        let cb = on_cmd.clone();
        close.set_callback(move |_| cb(TabCmd::Close(id)));
    }

    let mut plus = Button::default().with_label("+");
    style(&mut plus, false);
    plus.set_tooltip(crate::lang::tr("新しいタブ（Ctrl+T）", "New tab (Ctrl+T)"));
    bar.fixed(&plus, PLUS_W);
    let cb = on_cmd;
    plus.set_callback(move |_| cb(TabCmd::New));

    let mut rest = Frame::default();
    rest.set_frame(FrameType::FlatBox);
    rest.set_color(rgb(color_sub()));

    bar.end();
    DROP_X.with(|d| d.set(None));
    bar.draw(|b| {
        if let Some(x) = DROP_X.with(|d| d.get()) {
            draw::draw_rect_fill(x - 1, b.y(), 3, b.h(), rgb(color_accent()));
        }
    });
    Group::set_current(None::<&Group>); // 実行中に begin / end した後は、現在のグループを空に戻す
    bar.layout();
    bar.redraw();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_shows_name_dirty_mark_and_loading() {
        assert_eq!(tab_label(None, false, false), "無題");
        assert_eq!(tab_label(None, true, false), "*無題");
        assert_eq!(tab_label(Some(Path::new("/a/b/c.txt")), true, false), "*c.txt");
        assert_eq!(tab_label(Some(Path::new("C:\\x\\y.json")), false, false), "y.json");
        assert_eq!(tab_label(Some(Path::new("/a/b/c.txt")), true, true), "読込中... c.txt");
    }

    #[test]
    fn escape_neutralizes_fltk_label_codes() {
        assert_eq!(escape_label("a&b@c"), "a&&b@@c");
        assert_eq!(escape_label("plain"), "plain");
    }

    #[test]
    fn width_shrinks_to_fit_with_floor_and_ceiling() {
        assert_eq!(tab_width(1, 800), MAX_W);
        assert_eq!(tab_width(6, 800), (800 - PLUS_W) / 6);
        assert_eq!(tab_width(100, 800), MIN_W);
        assert_eq!(tab_width(0, 800), MAX_W);
    }

    #[test]
    fn drop_slot_follows_the_mouse_and_clamps_to_the_ends() {
        assert_eq!(drop_slot(0, 100, 4), 0);
        assert_eq!(drop_slot(99, 100, 4), 0);
        assert_eq!(drop_slot(100, 100, 4), 1);
        assert_eq!(drop_slot(250, 100, 4), 2);
        assert_eq!(drop_slot(-30, 100, 4), 0); // 左の外
        assert_eq!(drop_slot(9999, 100, 4), 3); // 右の外（+ ボタンの上も最後のタブ）
        assert_eq!(drop_slot(10, 100, 0), 0);
        assert_eq!(drop_slot(10, 0, 3), 2); // 幅 0 でも落ちない
    }

    #[test]
    fn reorder_puts_the_item_at_its_final_position() {
        let mut v = vec!['a', 'b', 'c', 'd'];
        assert!(reorder(&mut v, 0, 2));
        assert_eq!(v, ['b', 'c', 'a', 'd']);
        assert!(reorder(&mut v, 3, 0));
        assert_eq!(v, ['d', 'b', 'c', 'a']);
        assert!(reorder(&mut v, 1, 99)); // 範囲外は最後へ
        assert_eq!(v, ['d', 'c', 'a', 'b']);
        assert!(!reorder(&mut v, 2, 2)); // 動かない
        assert!(!reorder(&mut v, 9, 0)); // 元が無い
        assert_eq!(v, ['d', 'c', 'a', 'b']);
        let mut e: Vec<u8> = vec![];
        assert!(!reorder(&mut e, 0, 0));
    }

    #[test]
    fn overflowing_tabs_keep_the_active_one_visible() {
        // 800px なら (800-32)/48 = 16 個まで
        assert_eq!(visible_range(11, 3, 800), 0..11);
        assert_eq!(visible_range(16, 15, 800), 0..16);
        for active in 0..40 {
            let r = visible_range(40, active, 800);
            assert_eq!(r.len(), 16);
            assert!(r.contains(&active) && r.end <= 40, "active={active} range={r:?}");
        }
        assert_eq!(visible_range(40, 0, 800), 0..16);
        assert_eq!(visible_range(40, 39, 800), 24..40);
        // 極端に狭くても 1 個は出す
        assert_eq!(visible_range(5, 2, 10).len(), 1);
    }
}
