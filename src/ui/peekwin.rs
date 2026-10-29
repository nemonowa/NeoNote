// 本文の値にマウスを乗せたときの小窓（色の見本・文字・日時・画像の縮小）。中身は neo/peek.rs が見分ける。
// 枠の無い小さなウィンドウを、マウスの右下に 1 つだけ出す（出し直すときは前のものを消す）。
use super::theme::*;
use fltk::{
    app, draw,
    enums::{Align, Color, Font, FrameType},
    frame::Frame,
    group::Group,
    image::SharedImage,
    prelude::*,
    window::MenuWindow,
};
use std::cell::RefCell;
use std::path::Path;

/// 画像の縮小の最大の辺（ピクセル）。
const THUMB: i32 = 160;
const PAD: i32 = 6;
const SWATCH: i32 = 28;
const TEXT_SIZE: i32 = 13;

pub enum Content {
    /// 色の見本と、その表記。
    Swatch((u8, u8, u8), String),
    /// 大きく出す文字（`big`）と、その下の説明。
    Text { big: String, note: String },
    /// 画像の縮小と、その下の説明（読めなければ説明だけ）。
    Image(std::path::PathBuf, String),
}

thread_local! {
    static WIN: RefCell<Option<MenuWindow>> = const { RefCell::new(None) };
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

pub fn hide() {
    if let Some(mut w) = WIN.with(|w| w.borrow_mut().take()) {
        w.hide();
        app::add_timeout3(0.0, move |_| MenuWindow::delete(w.clone()));
    }
}

pub fn is_shown() -> bool {
    WIN.with(|w| w.borrow().is_some())
}

/// 画像を縮小して読む。読めない・大きすぎる・壊れているときは None。
fn thumb(p: &Path) -> Option<SharedImage> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
        return None;
    }
    let mut img = SharedImage::load(p).ok()?;
    let (w, h) = (img.data_w(), img.data_h());
    if w <= 0 || h <= 0 {
        return None;
    }
    let k = (THUMB as f64 / w.max(h) as f64).min(1.0);
    img.scale(((w as f64 * k).round() as i32).max(1), ((h as f64 * k).round() as i32).max(1), true, true);
    Some(img)
}

/// 画面の (x, y)（マウスの位置）の右下に出す。画面の端からはみ出すときは左上へ寄せる。
pub fn show(x: i32, y: i32, content: Content) {
    hide();
    Group::set_current(None::<&Group>);
    let (bg, text, sub) = (mix(color_main(), color_text(), 8), color_text(), color_subtext());
    let img = match &content {
        Content::Image(p, _) => thumb(p),
        _ => None,
    };
    draw::set_font(Font::Helvetica, TEXT_SIZE);
    let tw = |s: &str| draw::width(s).ceil() as i32;
    // 中身ごとの大きさを先に決める
    let (w, h) = match &content {
        Content::Swatch(_, label) => (PAD * 3 + SWATCH + tw(label), PAD * 2 + SWATCH),
        Content::Text { big, note } => {
            draw::set_font(Font::Helvetica, TEXT_SIZE * 2);
            let bw = tw(big);
            draw::set_font(Font::Helvetica, TEXT_SIZE);
            (PAD * 2 + bw.max(tw(note)), PAD * 2 + TEXT_SIZE * 2 + 6 + TEXT_SIZE + 4)
        }
        Content::Image(_, note) => match &img {
            Some(img) => (PAD * 2 + img.w().max(tw(note)), PAD * 3 + img.h() + TEXT_SIZE + 4),
            None => (PAD * 2 + tw(note), PAD * 2 + TEXT_SIZE + 4),
        },
    };
    let (sx, sy, sw, sh) = app::screen_xywh(app::screen_num(x, y));
    let mut px = x + 16;
    let mut py = y + 20;
    if px + w > sx + sw {
        px = (x - w - 8).max(sx);
    }
    if py + h > sy + sh {
        py = (y - h - 8).max(sy);
    }
    let mut win = MenuWindow::new(px, py, w, h, None);
    win.set_border(false);
    win.set_color(rgb(mix(bg, text, 25)));
    win.set_frame(FrameType::FlatBox);
    let mut inner = Frame::new(1, 1, w - 2, h - 2, None);
    inner.set_frame(FrameType::FlatBox);
    inner.set_color(rgb(bg));
    match content {
        Content::Swatch(c, label) => {
            let mut sw = Frame::new(PAD, PAD, SWATCH, SWATCH, None);
            sw.set_frame(FrameType::FlatBox);
            sw.set_color(rgb(c));
            // 背景に近い色でも見本の形がわかるよう、細い縁を引く
            sw.draw(move |f| {
                draw::set_draw_color(rgb(mix(bg, text, 40)));
                draw::draw_rect(f.x(), f.y(), f.w(), f.h());
            });
            label_frame(PAD * 2 + SWATCH, PAD, w - PAD * 3 - SWATCH, SWATCH, &label, text, TEXT_SIZE);
        }
        Content::Text { big, note } => {
            label_frame(PAD, PAD, w - PAD * 2, TEXT_SIZE * 2 + 6, &big, text, TEXT_SIZE * 2);
            label_frame(PAD, PAD + TEXT_SIZE * 2 + 6, w - PAD * 2, TEXT_SIZE + 4, &note, sub, TEXT_SIZE);
        }
        Content::Image(_, note) => {
            let mut y0 = PAD;
            if let Some(img) = img {
                let mut f = Frame::new(PAD, PAD, img.w(), img.h(), None);
                f.set_frame(FrameType::NoBox);
                y0 += img.h() + PAD;
                f.set_image(Some(img));
            }
            label_frame(PAD, y0, w - PAD * 2, TEXT_SIZE + 4, &note, sub, TEXT_SIZE);
        }
    }
    win.end();
    win.show();
    WIN.with(|w| *w.borrow_mut() = Some(win));
}

fn label_frame(x: i32, y: i32, w: i32, h: i32, s: &str, color: (u8, u8, u8), size: i32) {
    let mut f = Frame::new(x, y, w, h, None);
    f.set_label(&s.replace('@', "@@"));
    f.set_label_size(size);
    f.set_label_color(rgb(color));
    f.set_align(Align::Left | Align::Inside);
}
