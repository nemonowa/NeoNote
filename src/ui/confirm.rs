// 確認・お知らせの小さな画面（保存しますか・保存できませんでした など）。
// FLTK の標準のダイアログは OS の枠の付いた別のウィンドウで、配色も合わないので、
// 枠の無いウィンドウをアプリのウィンドウの中央に重ね、配色（theme）で描く。閉じるまで次へ進まない（モーダル）。
// Esc・閉じるはキャンセル、Enter は最初のボタン（いちばん勧める操作）。
use super::theme::*;
use fltk::{
    app,
    button::Button,
    draw,
    enums::{Align, Color, Event, Font, FrameType, Key},
    frame::Frame,
    group::Group,
    prelude::*,
    window::Window,
};
use std::{cell::Cell, rc::Rc};

const W: i32 = 440;
const PAD: i32 = 16;
const BTN_H: i32 = 28;
const TEXT_SIZE: i32 = 14;
const LINE_H: i32 = 20;

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// `dialog::choice2_default` と同じ使い方: 0 = `cancel`、1 = `b1`、2 = `b2`。閉じたら Some(0)。
/// 空の文字のボタンは出さない。
pub fn choice(message: &str, cancel: &str, b1: &str, b2: &str) -> Option<i32> {
    let mut buttons: Vec<(i32, &str)> = Vec::new();
    for (i, l) in [(1, b1), (2, b2)] {
        if !l.is_empty() {
            buttons.push((i, l));
        }
    }
    if !cancel.is_empty() {
        buttons.push((0, cancel));
    }
    Some(run(message, &buttons))
}

/// お知らせ（エラーなど）。OK で閉じる。
pub fn alert(message: &str) {
    run(message, &[(0, "OK")]);
}

pub fn message(message: &str) {
    alert(message);
}

/// 折り返したときの行数（幅 `w` で、文字の幅を測って数える）。
fn line_count(text: &str, w: i32) -> i32 {
    draw::set_font(Font::Helvetica, TEXT_SIZE);
    text.split('\n').map(|l| ((draw::width(l) as i32 + w - 1) / w.max(1)).max(1)).sum()
}

/// 画面を出して、押されたボタンの番号を返す（Esc・閉じるは 0）。
fn run(message: &str, buttons: &[(i32, &str)]) -> i32 {
    let text_w = W - PAD * 2;
    let lines = line_count(message, text_w).min(20);
    let h = PAD + lines * LINE_H + PAD + BTN_H + PAD;
    let (x, y) = match app::first_window() {
        Some(p) => (p.x_root() + (p.w() - W) / 2, p.y_root() + (p.h() - h) / 3),
        None => {
            let (sx, sy, sw, sh) = app::screen_xywh(0);
            (sx + (sw - W) / 2, sy + (sh - h) / 3)
        }
    };
    Group::set_current(None::<&Group>);
    let mut win = Window::new(x, y, W, h, None);
    win.set_border(false);
    let (sub, text, accent) = (color_sub(), color_text(), color_accent());
    let main = color_main();
    let bg = mix(main, text, 6);
    win.set_color(rgb(mix(bg, text, 25))); // 1px の縁
    let mut fill = Frame::new(1, 1, W - 2, h - 2, None);
    fill.set_frame(FrameType::FlatBox);
    fill.set_color(rgb(bg));
    let mut msg = Frame::new(PAD, PAD, text_w, lines * LINE_H, None);
    msg.set_label(&message.replace('@', "@@"));
    msg.set_label_size(TEXT_SIZE);
    msg.set_label_color(rgb(text));
    msg.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);

    let chosen = Rc::new(Cell::new(0));
    // ボタンは右寄せ。最初のボタンだけアクセント色（勧める操作）
    draw::set_font(Font::Helvetica, TEXT_SIZE);
    let mut bx = W - PAD;
    let by = h - PAD - BTN_H;
    for (k, (id, label)) in buttons.iter().enumerate().rev() {
        let bw = (draw::width(label) as i32 + 28).max(72);
        bx -= bw;
        let mut b = Button::new(bx, by, bw, BTN_H, None).with_label(label);
        bx -= 8;
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(FrameType::FlatBox);
        b.clear_visible_focus();
        b.set_label_size(TEXT_SIZE);
        let primary = k == 0 && buttons.len() > 1;
        let (base, fg) = if primary { (accent, color_on_accent()) } else { (sub, text) };
        b.set_color(rgb(base));
        b.set_selection_color(rgb(mix(base, text, 20)));
        b.set_label_color(rgb(fg));
        b.handle(move |w, ev| match ev {
            Event::Enter | Event::Leave => {
                w.set_color(rgb(if ev == Event::Enter { mix(base, text, 12) } else { base }));
                w.redraw();
                true
            }
            _ => false,
        });
        let chosen = Rc::clone(&chosen);
        let id = *id;
        b.set_callback(move |w| {
            chosen.set(id);
            if let Some(mut win) = w.window() {
                win.hide();
            }
        });
    }
    win.end();
    let first = buttons.first().map_or(0, |b| b.0);
    {
        let chosen = Rc::clone(&chosen);
        win.handle(move |w, ev| {
            if ev == Event::KeyDown || ev == Event::Shortcut {
                let k = app::event_key();
                if k == Key::Escape {
                    chosen.set(0);
                    w.hide();
                    return true;
                }
                if k == Key::Enter || k == Key::KPEnter {
                    chosen.set(first);
                    w.hide();
                    return true;
                }
            }
            false
        });
    }
    // Esc の既定の動作（ウィンドウを閉じる）もキャンセル
    {
        let chosen = Rc::clone(&chosen);
        win.set_callback(move |w| {
            chosen.set(0);
            w.hide();
        });
    }
    win.make_modal(true);
    win.show();
    // 本体が最前面に固定されていても後ろに隠れないように
    crate::platform::set_topmost(win.raw_handle() as *mut std::ffi::c_void, true);
    while win.shown() {
        app::wait();
    }
    Window::delete(win);
    chosen.get()
}
