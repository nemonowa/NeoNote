// ツールバーのボタンにマウスを乗せると、真下に出るメニュー（右クリックでもすぐ出る）。
// FLTK の標準のポップアップは開いている間ほかを触れず、項目ごとのツールチップも出せないので、
// 枠の無い小さなウィンドウに項目ごとのボタンを並べて作る（各ボタンに説明のツールチップを付ける）。
// マウスがボタンとメニューのどちらからも離れて少し経つと閉じる。項目を押すと閉じてから動作する。
// ボタンの真下にすき間なく、ボタンと同じ地の色で出す（ボタンから下へ伸びたように見せる）。
// ボタンそのものを押したときの動作と同じ項目は出さない（呼び出し側が `skip` で渡す）。
use super::menu::{self, Action, MenuEntry};
use super::theme::*;
use fltk::{
    app,
    button::Button,
    enums::{Align, Color, Event, FrameType},
    frame::Frame,
    group::Group,
    prelude::*,
    window::MenuWindow,
};
use std::cell::{Cell, RefCell};

/// マウスを乗せてから開くまで（秒）。通り過ぎただけで開かないように。
const OPEN_DELAY: f64 = 0.25;
/// 離れてから閉じるまで（見回りの回数。1 回 0.1 秒）。ボタンとメニューの間を移る間に閉じないように。
const CLOSE_TICKS: u32 = 3;
const ROW_H: i32 = 24;
const PAD_X: i32 = 12;

struct Open {
    win: MenuWindow,
    owner: Button,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    /// 開くたび・閉じるたびに増やす。古い待ち（開く前の遅延・見回り）を見分けて止めるため。
    static GEN: Cell<u64> = const { Cell::new(0) };
}

fn bump() -> u64 {
    GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    })
}

fn current_gen() -> u64 {
    GEN.with(|g| g.get())
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// 部品の画面上の四角（x, y, w, h）。
fn screen_rect<W: WidgetExt>(w: &W) -> Option<(i32, i32, i32, i32)> {
    let win = w.window()?;
    Some((win.x_root() + w.x(), win.y_root() + w.y(), w.w(), w.h()))
}

fn mouse_in((x, y, w, h): (i32, i32, i32, i32), margin: i32) -> bool {
    let (mx, my) = app::get_mouse();
    mx >= x - margin && mx < x + w + margin && my >= y - margin && my < y + h + margin
}

pub fn is_open_for(owner: &Button) -> bool {
    OPEN.with(|o| o.borrow().as_ref().is_some_and(|o| o.owner.as_widget_ptr() == owner.as_widget_ptr()))
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

/// 開いているメニューを閉じる（無ければ何もしない）。ウィンドウは、今のイベントの処理が終わってから消す。
pub fn close() {
    bump(); // 開く前の待ちも取り消す
    let Some(open) = OPEN.with(|o| o.borrow_mut().take()) else { return };
    super::peekwin::hide(); // 項目の説明も消す
    let mut win = open.win;
    win.hide();
    app::add_timeout3(0.0, move |_| MenuWindow::delete(win.clone()));
}

/// マウスがボタンに乗ったとき: 少し待って、まだ乗っていれば開く。別のメニューが開いていれば、すぐ切り替える。
pub fn hover(owner: &Button, entries: &'static [MenuEntry], skip: Option<Action>, on: impl Fn(Action) + Clone + 'static) {
    if is_open_for(owner) {
        return;
    }
    if is_open() {
        open(owner, entries, skip, on);
        return;
    }
    let gen = bump();
    let owner = owner.clone();
    app::add_timeout3(OPEN_DELAY, move |_| {
        let still = current_gen() == gen && screen_rect(&owner).is_some_and(|r| mouse_in(r, 0));
        if still {
            open(&owner, entries, skip, on.clone());
        }
    });
}

/// `owner` の真下にメニューを開く。
pub fn open(owner: &Button, entries: &'static [MenuEntry], skip: Option<Action>, on: impl Fn(Action) + Clone + 'static) {
    close();
    let Some((bx, by, bw, bh)) = screen_rect(owner) else { return };
    let entries = sub_entries(entries, skip);
    if entries.is_empty() {
        return;
    }
    let labels: Vec<String> = entries.iter().map(|(m, _)| label_of(m)).collect();
    fltk::draw::set_font(fltk::enums::Font::Helvetica, 14);
    let w = (labels.iter().map(|l| fltk::draw::width(l)).fold(0.0f64, f64::max).ceil() as i32 + 2 * PAD_X).max(bw);
    let h = entries.len() as i32 * ROW_H + entries.iter().filter(|e| e.1).count() as i32 * 5 + 1;
    // 画面の右端・下端からはみ出さないように寄せる
    let (sx, sy, sw, sh) = app::screen_xywh(app::screen_num(bx, by));
    let x = bx.min(sx + sw - w).max(sx);
    let y = if by + bh + h > sy + sh { (by - h).max(sy) } else { by + bh };

    Group::set_current(None::<&Group>);
    let mut win = MenuWindow::new(x, y, w, h, None);
    let (main, text) = (color_main(), color_text());
    // ボタンと同じ地の色。左右と下にだけ細い縁を引き、上はボタンとつなげる
    let bg = main;
    let hover_bg = mix(main, text, 14);
    let edge = mix(main, text, 14);
    win.set_color(rgb(edge));
    win.set_frame(FrameType::FlatBox);
    let mut fill = Frame::new(1, 0, w - 2, h - 1, None);
    fill.set_frame(FrameType::FlatBox);
    fill.set_color(rgb(bg));
    let mut yy = 0;
    for ((m, divider), label) in entries.iter().zip(&labels) {
        let mut b = Button::new(1, yy, w - 2, ROW_H, None).with_label(label);
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(FrameType::FlatBox);
        b.set_color(rgb(bg));
        b.set_selection_color(rgb(hover_bg));
        b.set_label_color(rgb(text));
        b.set_label_size(14);
        b.set_align(Align::Left | Align::Inside);
        b.set_label_color(rgb(color_subtext()));
        b.clear_visible_focus();
        let tip = m.action.map_or("", menu::describe);
        match m.action {
            Some(a) => {
                let on = on.clone();
                b.set_callback(move |_| {
                    close();
                    on(a);
                });
                b.handle(move |w, ev| match ev {
                    Event::Enter | Event::Leave => {
                        w.set_color(rgb(if ev == Event::Enter { hover_bg } else { bg }));
                        w.set_label_color(rgb(if ev == Event::Enter { color_text() } else { color_subtext() }));
                        w.redraw();
                        super::peekwin::hide();
                        if ev == Event::Enter && !tip.is_empty() {
                            show_tip_later(w.clone(), tip);
                        }
                        ev == Event::Enter
                    }
                    _ => false,
                });
            }
            None => b.deactivate(),
        }
        yy += ROW_H;
        if *divider {
            let mut line = Frame::new(PAD_X / 2, yy + 2, w - PAD_X, 1, None);
            line.set_frame(FrameType::FlatBox);
            line.set_color(rgb(edge));
            yy += 5;
        }
    }
    win.end();
    win.set_border(false);
    win.show();
    // 本体が最前面に固定されていても後ろに隠れないように
    crate::platform::set_topmost(win.raw_handle() as *mut std::ffi::c_void, true);
    let gen = bump();
    OPEN.with(|o| *o.borrow_mut() = Some(Open { win: win.clone(), owner: owner.clone() }));
    watch(gen, owner.clone(), win, 0);
}

/// 少し留まったら、項目の説明を最前面の小さな窓で出す（まだその項目の上にいる時だけ）。
fn show_tip_later(row: Button, tip: &'static str) {
    let gen = current_gen();
    app::add_timeout3(0.6, move |_| {
        if current_gen() != gen || !row.visible_r() {
            return;
        }
        let Some(r) = screen_rect(&row) else { return };
        if mouse_in(r, 0) {
            let (mx, my) = app::get_mouse();
            super::peekwin::show(mx, my, super::peekwin::Content::Note(tip.to_string()));
        }
    });
}

/// 0.1 秒ごとに、マウスがボタンかメニューの上にあるかを見て、離れて少し経ったら閉じる。
fn watch(gen: u64, owner: Button, win: MenuWindow, away: u32) {
    app::add_timeout3(0.1, move |_| {
        if current_gen() != gen {
            return; // 閉じた・別のメニューを開いた
        }
        let over = screen_rect(&owner).is_some_and(|r| mouse_in(r, 2)) || mouse_in((win.x(), win.y(), win.w(), win.h()), 2);
        let away = if over { 0 } else { away + 1 };
        if away >= CLOSE_TICKS {
            close();
        } else {
            watch(gen, owner.clone(), win.clone(), away);
        }
    });
}

/// ボタンの動作（`skip`）と同じ項目を除いた、サブの一覧と、各項目の下に区切り線を引くか。
/// 消した項目に付いていた区切り線は、1 つ上の項目へ移す（最後の項目の下には引かない）。
fn sub_entries(entries: &'static [MenuEntry], skip: Option<Action>) -> Vec<(&'static MenuEntry, bool)> {
    let mut out: Vec<(&'static MenuEntry, bool)> = Vec::new();
    for m in entries {
        if skip.is_some() && m.action == skip {
            if let (true, Some(last)) = (m.divider, out.last_mut()) {
                last.1 = true;
            }
            continue;
        }
        out.push((m, m.divider));
    }
    if let Some(last) = out.last_mut() {
        last.1 = false;
    }
    out
}

/// 表示する文字。チェックのある項目は、今の状態を ● で添える。左に少し余白を入れる。
fn label_of(m: &MenuEntry) -> String {
    let label = m.text().replace('@', "@@"); // FLTK のラベルの @ 記法（記号の描画）を避ける
    match m.action.and_then(menu::checked) {
        Some(true) => format!("  ● {label}"),
        Some(false) => format!("  \u{3000}{label}"),
        None => format!("  {label}"),
    }
}
