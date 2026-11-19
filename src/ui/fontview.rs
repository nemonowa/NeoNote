// フォントの見本の画面: 名前・文字の数・見本の文と、入っている文字を Unicode の順に並べた格子（番号つき）。
// フォントは FLTK に読み込ませて専用の枠（FONT_SLOT）に入れ、その枠で描く。

use crate::neo::fontfile::{self, FontInfo};
use crate::ui::theme::{color_accent, color_main, color_sub, color_subtext, color_text};
use fltk::{
    app, draw,
    enums::{Align, Color, Event, Font, FrameType},
    group::Group,
    prelude::*,
    valuator::Scrollbar,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// 見本のフォントを入れる枠の番号（編集のフォントの 16 とは別）。
const FONT_SLOT: usize = 17;
const CELL_W: i32 = 64;
const CELL_H: i32 = 76;
const GLYPH: i32 = 36;
const HEAD_H: i32 = 150;
const SCROLL_W: i32 = 14;

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

struct State {
    path: Option<PathBuf>,
    info: Option<FontInfo>,
    font: Option<Font>,
    top: i32,
    hover: Option<u32>,
}

#[derive(Clone)]
pub struct FontView {
    pub group: Group,
    bar: Scrollbar,
    st: Rc<RefCell<State>>,
}

impl FontView {
    pub fn new() -> FontView {
        let mut group = Group::default();
        group.set_frame(FrameType::FlatBox);
        let mut bar = Scrollbar::default();
        bar.set_type(fltk::valuator::ScrollbarType::Vertical);
        group.end();
        group.hide();
        let st = Rc::new(RefCell::new(State { path: None, info: None, font: None, top: 0, hover: None }));
        let v = FontView { group, bar, st };
        {
            let v2 = v.clone();
            v.bar.clone().set_callback(move |b| {
                v2.st.borrow_mut().top = b.value() as i32;
                v2.group.clone().redraw();
            });
        }
        {
            let v2 = v.clone();
            v.group.clone().draw(move |g| v2.draw_all(g));
        }
        {
            let v2 = v.clone();
            v.group.clone().resize_callback(move |_, _, _, _, _| v2.place_bar());
        }
        {
            let v2 = v.clone();
            v.group.clone().handle(move |g, ev| match ev {
                Event::MouseWheel => {
                    v2.scroll_by(match app::event_dy() { app::MouseWheel::Down => CELL_H, app::MouseWheel::Up => -CELL_H, _ => 0 });
                    true
                }
                Event::Move | Event::Enter => {
                    let h = v2.cell_at(app::event_x(), app::event_y());
                    if h != v2.st.borrow().hover {
                        v2.st.borrow_mut().hover = h;
                        g.redraw();
                    }
                    true
                }
                Event::Push if app::event_x() < v2.bar.x() => {
                    let _ = g.take_focus();
                    // 文字をクリックするとその文字を写す
                    if let Some(c) = v2.cell_at(app::event_x(), app::event_y()).and_then(char::from_u32) {
                        app::copy(&c.to_string());
                    }
                    true
                }
                Event::Focus | Event::Unfocus => true,
                Event::KeyDown => match app::event_key() {
                    fltk::enums::Key::PageDown => {
                        v2.scroll_by(g.h() - HEAD_H);
                        true
                    }
                    fltk::enums::Key::PageUp => {
                        v2.scroll_by(-(g.h() - HEAD_H));
                        true
                    }
                    fltk::enums::Key::Down => {
                        v2.scroll_by(CELL_H);
                        true
                    }
                    fltk::enums::Key::Up => {
                        v2.scroll_by(-CELL_H);
                        true
                    }
                    fltk::enums::Key::Home => {
                        v2.scroll_by(-i32::MAX / 2);
                        true
                    }
                    fltk::enums::Key::End => {
                        v2.scroll_by(i32::MAX / 2);
                        true
                    }
                    _ => false,
                },
                _ => false,
            });
        }
        v
    }

    pub fn visible(&self) -> bool {
        self.group.visible()
    }

    pub fn hide(&self) {
        self.group.clone().hide();
    }

    /// 表示して `path` のフォントを読む（同じファイルならそのまま）。
    pub fn open(&self, path: &Path) {
        let same = self.st.borrow().path.as_deref() == Some(path);
        if !same {
            let info = std::fs::read(path).ok().and_then(|b| fontfile::info(&b));
            let font = Font::load_font(path).ok().map(|name| {
                let f = Font::by_index(FONT_SLOT);
                Font::set_font(f, &name);
                f
            });
            let mut st = self.st.borrow_mut();
            st.path = Some(path.to_path_buf());
            st.info = info;
            st.font = font;
            st.top = 0;
            st.hover = None;
        }
        let mut g = self.group.clone();
        g.show();
        self.place_bar();
        let _ = g.take_focus();
        g.redraw();
    }

    fn cols(&self) -> i32 {
        ((self.group.w() - SCROLL_W - 16) / CELL_W).max(1)
    }

    fn content_h(&self) -> i32 {
        let n = self.st.borrow().info.as_ref().map_or(0, |i| i.chars.len()) as i32;
        let rows = (n + self.cols() - 1) / self.cols();
        HEAD_H + rows * CELL_H + 8
    }

    fn place_bar(&self) {
        let g = &self.group;
        let mut bar = self.bar.clone();
        bar.resize(g.x() + g.w() - SCROLL_W, g.y(), SCROLL_W, g.h());
        let max = (self.content_h() - g.h()).max(0);
        let top = self.st.borrow().top.clamp(0, max);
        self.st.borrow_mut().top = top;
        bar.set_bounds(0.0, max as f64);
        bar.set_slider_size((g.h() as f32 / self.content_h().max(1) as f32).clamp(0.05, 1.0));
        bar.set_step(CELL_H as f64, 1);
        bar.set_value(top as f64);
    }

    fn scroll_by(&self, dy: i32) {
        let max = (self.content_h() - self.group.h()).max(0);
        let top = (self.st.borrow().top.saturating_add(dy)).clamp(0, max);
        self.st.borrow_mut().top = top;
        self.bar.clone().set_value(top as f64);
        self.group.clone().redraw();
    }

    /// 画面の位置にある文字（格子の外は None）。
    fn cell_at(&self, x: i32, y: i32) -> Option<u32> {
        let g = &self.group;
        let st = self.st.borrow();
        let info = st.info.as_ref()?;
        let cy = y - g.y() + st.top - HEAD_H;
        let cx = x - g.x() - 8;
        if cy < 0 || cx < 0 || cx >= self.cols() * CELL_W {
            return None;
        }
        let i = (cy / CELL_H) * self.cols() + cx / CELL_W;
        info.chars.get(i as usize).copied()
    }

    fn draw_all(&self, g: &mut Group) {
        let (x, y, w, h) = (g.x(), g.y(), g.w(), g.h());
        draw::draw_rect_fill(x, y, w, h, rgb(color_main()));
        draw::push_clip(x, y, w - SCROLL_W, h);
        let st = self.st.borrow();
        let ui = Font::Helvetica;
        let top = st.top;
        match (&st.info, st.font) {
            (Some(info), font) => {
                let font = font.unwrap_or(ui);
                let hy = y - top;
                draw::set_draw_color(rgb(color_text()));
                draw::set_font(ui, 18);
                let title = if info.style.is_empty() || info.name.contains(&info.style) { info.name.clone() } else { format!("{} {}", info.name, info.style) };
                draw::draw_text2(&title, x + 10, hy + 6, w - 30, 26, Align::Left);
                draw::set_draw_color(rgb(color_subtext()));
                draw::set_font(ui, 13);
                let line = if crate::lang::is_en() { format!("{} characters / {} glyphs   Click a character to copy it", info.chars.len(), info.glyphs) } else { format!("{} 文字 / {} グリフ　　文字をクリックすると写します", info.chars.len(), info.glyphs) };
                draw::draw_text2(&line, x + 10, hy + 32, w - 30, 20, Align::Left);
                draw::set_draw_color(rgb(color_text()));
                draw::set_font(font, 28);
                draw::draw_text2("あいうえお 永遠 Hello, World! 0123456789", x + 10, hy + 58, w - 30, 40, Align::Left);
                draw::set_font(font, 16);
                draw::draw_text2("The quick brown fox jumps over the lazy dog. いろはにほへと", x + 10, hy + 100, w - 30, 26, Align::Left);
                // 乗せている文字の番号と区画
                if let Some(c) = st.hover {
                    draw::set_draw_color(rgb(color_accent()));
                    draw::set_font(ui, 13);
                    let ch = char::from_u32(c).map(|c| c.to_string()).unwrap_or_default();
                    draw::draw_text2(&format!("U+{c:04X}  {ch}  {}", fontfile::block_name(c)), x + 10, hy + 124, w - 30, 18, Align::Left);
                }
                draw::set_draw_color(rgb(color_sub()));
                draw::draw_line(x + 8, hy + HEAD_H - 8, x + w - SCROLL_W - 8, hy + HEAD_H - 8);
                // 見えている行だけ描く
                let cols = self.cols();
                let first_row = ((top - HEAD_H).max(0)) / CELL_H;
                let last_row = (top + h - HEAD_H) / CELL_H + 1;
                for row in first_row..=last_row {
                    for col in 0..cols {
                        let i = (row * cols + col) as usize;
                        let Some(&c) = info.chars.get(i) else { break };
                        let cx = x + 8 + col * CELL_W;
                        let cy = y + HEAD_H + row * CELL_H - top;
                        let hovered = st.hover == Some(c);
                        draw::draw_rect_fill(cx + 1, cy + 1, CELL_W - 2, CELL_H - 2, if hovered { rgb(color_accent()) } else { rgb(color_sub()) });
                        if let Some(ch) = char::from_u32(c) {
                            draw::set_draw_color(rgb(color_text()));
                            draw::set_font(font, GLYPH);
                            draw::draw_text2(&ch.to_string(), cx, cy + 4, CELL_W, CELL_H - 22, Align::Center);
                        }
                        draw::set_draw_color(rgb(color_subtext()));
                        draw::set_font(ui, 10);
                        draw::draw_text2(&format!("{c:04X}"), cx, cy + CELL_H - 20, CELL_W, 16, Align::Center);
                    }
                }
            }
            (None, _) => {
                draw::set_draw_color(rgb(color_text()));
                draw::set_font(ui, 14);
                draw::draw_text2(crate::lang::tr("フォントを読めませんでした。", "Could not read the font."), x + 10, y + 10, w - 30, 24, Align::Left);
            }
        }
        draw::pop_clip();
        drop(st);
        g.draw_children();
    }
}
