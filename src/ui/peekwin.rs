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
    /// 説明の文だけ（メニューの項目のツールチップ。標準のツールチップは最前面のメニューの後ろに隠れるため）。
    Note(String),
    /// 1 行目（ID など）と、その下の案内（本文の `ns:path` にマウスを乗せたとき）。
    Link { title: String, note: String },
    /// Minecraft のテキスト部品の見た目（色・太字・斜体・下線・取り消し線）と、その下の説明。
    Rich(Vec<crate::neo::textcomp::Span>, String),
    /// 1 行目（名前と大きさ）と、その下の複数行（ファイルの先頭・フォルダの中身。等幅で出す）。
    Lines { title: String, lines: Vec<String> },
}

/// テキスト部品の見本の背景（Minecraft のツールチップに近い暗い色。白や黄色の文字が読めるように、配色によらず固定）。
const RICH_BG: (u8, u8, u8) = (0x1a, 0x10, 0x24);

fn rich_font(st: &crate::neo::textcomp::Style) -> Font {
    match (st.bold, st.italic) {
        (true, true) => Font::HelveticaBoldItalic,
        (true, false) => Font::HelveticaBold,
        (false, true) => Font::HelveticaItalic,
        _ => Font::Helvetica,
    }
}

/// 改行で行に分けた、(行ごとの (文字, 書式) の並び)。
fn rich_lines(spans: &[crate::neo::textcomp::Span]) -> Vec<Vec<(String, crate::neo::textcomp::Style)>> {
    let mut lines = vec![Vec::new()];
    for sp in spans {
        for (k, part) in sp.text.split('\n').enumerate() {
            if k > 0 {
                lines.push(Vec::new());
            }
            if !part.is_empty() {
                lines.last_mut().unwrap().push((part.to_string(), sp.style.clone()));
            }
        }
    }
    lines.truncate(12);
    lines
}

fn rich_size(lines: &[Vec<(String, crate::neo::textcomp::Style)>]) -> (i32, i32) {
    let mut w = 0;
    for l in lines {
        let mut lw = 0;
        for (t, st) in l {
            draw::set_font(rich_font(st), TEXT_SIZE);
            lw += draw::width(t).ceil() as i32;
        }
        w = w.max(lw);
    }
    (w, lines.len() as i32 * (TEXT_SIZE + 4))
}

thread_local! {
    static WIN: RefCell<Option<MenuWindow>> = const { RefCell::new(None) };
    /// 出しているのがメニューの項目の説明（Note）か。本文の値の見回りは、これを消さない。
    static NOTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// 本文の値の小窓を出しているか（メニューの項目の説明は含めない）。
pub fn is_shown() -> bool {
    WIN.with(|w| w.borrow().is_some()) && !NOTE.with(|n| n.get())
}

/// 複数行の小窓の幅の上限（長い行は切る）。
const LINES_MAX_W: i32 = 640;

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
    NOTE.with(|n| n.set(matches!(content, Content::Note(_))));
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
        Content::Note(note) => (PAD * 2 + tw(note), PAD * 2 + TEXT_SIZE + 4),
        Content::Lines { title, lines } => {
            draw::set_font(Font::Courier, TEXT_SIZE);
            let lw = lines.iter().map(|l| tw(l)).max().unwrap_or(0);
            draw::set_font(Font::Helvetica, TEXT_SIZE);
            ((PAD * 2 + tw(title).max(lw)).min(LINES_MAX_W), PAD * 3 + (TEXT_SIZE + 4) * (1 + lines.len() as i32))
        }
        Content::Link { title, note } => (PAD * 2 + tw(title).max(tw(note)), PAD * 2 + (TEXT_SIZE + 4) * 2),
        Content::Rich(spans, note) => {
            let (rw, rh) = rich_size(&rich_lines(spans));
            draw::set_font(Font::Helvetica, TEXT_SIZE);
            (PAD * 4 + rw.max(tw(note)), PAD * 4 + rh + TEXT_SIZE + 4)
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
        Content::Note(note) => label_frame(PAD, PAD, w - PAD * 2, TEXT_SIZE + 4, &note, text, TEXT_SIZE),
        Content::Lines { title, lines } => {
            label_frame(PAD, PAD, w - PAD * 2, TEXT_SIZE + 4, &title, text, TEXT_SIZE);
            let mut f = Frame::new(PAD, PAD * 2 + TEXT_SIZE + 4, w - PAD * 2, (TEXT_SIZE + 4) * lines.len() as i32, None);
            f.set_frame(FrameType::NoBox);
            f.draw(move |f| {
                draw::push_clip(f.x(), f.y(), f.w(), f.h());
                draw::set_font(Font::Courier, TEXT_SIZE);
                draw::set_draw_color(rgb(sub));
                for (i, l) in lines.iter().enumerate() {
                    draw::draw_text(l, f.x(), f.y() + (TEXT_SIZE + 4) * i as i32 + TEXT_SIZE);
                }
                draw::pop_clip();
            });
        }
        Content::Link { title, note } => {
            label_frame(PAD, PAD, w - PAD * 2, TEXT_SIZE + 4, &title, text, TEXT_SIZE);
            label_frame(PAD, PAD + TEXT_SIZE + 4, w - PAD * 2, TEXT_SIZE + 4, &note, sub, TEXT_SIZE);
        }
        Content::Rich(spans, note) => {
            let lines = rich_lines(&spans);
            let (_, rh) = rich_size(&lines);
            let mut f = Frame::new(PAD, PAD, w - PAD * 2, rh + PAD * 2, None);
            f.set_frame(FrameType::FlatBox);
            f.set_color(rgb(RICH_BG));
            f.draw(move |f| {
                let mut y = f.y() + PAD + TEXT_SIZE;
                for l in &lines {
                    let mut x = f.x() + PAD;
                    for (t, st) in l {
                        draw::set_font(rich_font(st), TEXT_SIZE);
                        draw::set_draw_color(rgb(st.color.unwrap_or((0xff, 0xff, 0xff))));
                        let tw = draw::width(t).ceil() as i32;
                        draw::draw_text(t, x, y);
                        if st.underlined {
                            draw::draw_line(x, y + 2, x + tw, y + 2);
                        }
                        if st.strikethrough {
                            draw::draw_line(x, y - TEXT_SIZE / 3, x + tw, y - TEXT_SIZE / 3);
                        }
                        x += tw;
                    }
                    y += TEXT_SIZE + 4;
                }
            });
            label_frame(PAD, PAD * 3 + rh, w - PAD * 2, TEXT_SIZE + 4, &note, sub, TEXT_SIZE);
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
    // 本体が最前面に固定されていても後ろに隠れないように
    crate::platform::set_topmost(win.raw_handle() as *mut std::ffi::c_void, true);
    WIN.with(|w| *w.borrow_mut() = Some(win));
}

fn label_frame(x: i32, y: i32, w: i32, h: i32, s: &str, color: (u8, u8, u8), size: i32) {
    let mut f = Frame::new(x, y, w, h, None);
    f.set_label(&s.replace('@', "@@"));
    f.set_label_size(size);
    f.set_label_color(rgb(color));
    f.set_align(Align::Left | Align::Inside);
}
