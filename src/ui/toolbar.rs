// ツールバー: 左端の押しピン（ウィンドウを最前面に固定。幅は行番号欄にそろえる）+ 8 ボタン + 余白 + 右端の検索バー（searchbar.rs。使うときだけ開く）。
// 左クリック = そのボタンの主な動作。マウスを乗せる（または右クリック）と、そのボタンのメニューが真下に出る（hovermenu.rs）。
// 余白をクリックすると現在のパスをコピーする（ダイアログは出さない）。
use crate::lang;
use super::{hovermenu, icons::{self, Icon}, menu::{self, Action, MenuEntry}, theme::*};
use fltk::{
    app,
    button::Button,
    draw,
    enums::{Align, Color, Event, FrameType},
    frame::Frame,
    group::Flex,
    prelude::*,
};

pub const HEIGHT: i32 = 32;

thread_local! {
    /// 最後に右クリックされたボタンの、画面上の左下（x, y）。そのボタンの真下に別ウィンドウを出すのに使う。
    static LAST_BUTTON_BOTTOM: std::cell::Cell<Option<(i32, i32)>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// 最後に右クリックされたボタン（別ウィンドウを開いている間、強調表示するのに使う）。
    static LAST_BUTTON: std::cell::RefCell<Option<Button>> = const { std::cell::RefCell::new(None) };
}

/// 最後に右クリックされたボタンを、アクセント色で強調する（on）／元の色に戻す（off）。
/// 別ウィンドウが「どのボタンから開いたか」を示すため。
pub fn mark_open(on: bool) {
    LAST_BUTTON.with(|b| {
        if let Some(b) = b.borrow_mut().as_mut() {
            b.set_color(if on { Color::from_rgb(color_accent().0, color_accent().1, color_accent().2) } else { Color::from_rgb(color_main().0, color_main().1, color_main().2) });
            b.set_label_color(if on { Color::from_rgb(color_on_accent().0, color_on_accent().1, color_on_accent().2) } else { Color::from_rgb(color_subtext().0, color_subtext().1, color_subtext().2) });
            b.redraw();
        }
    });
}

/// 最後に右クリックされたボタンの真下（画面座標）。まだ無ければ None。
pub fn last_button_bottom() -> Option<(i32, i32)> {
    LAST_BUTTON_BOTTOM.with(|c| c.get())
}
const BUTTON_W: i32 = 64;

struct Spec {
    label: &'static str,
    label_ja: &'static str,
    icon: Icon,
    tip: &'static str,
    tip_ja: &'static str,
    /// 左クリックの動作。None はグレー表示で何も起きない。
    main: Option<Action>,
    menu: &'static [MenuEntry],
}

impl Spec {
    fn label(&self) -> &'static str {
        lang::tr(self.label_ja, self.label)
    }
    fn tip(&self) -> &'static str {
        lang::tr(self.tip_ja, self.tip)
    }
}

const SPECS: [Spec; 8] = [
    Spec { label: "Load", label_ja: "開く", icon: Icon::Folder, tip: "Load: open a file (hover for menu)", tip_ja: "開く: ファイルを開く（乗せるとメニュー）", main: Some(Action::Open), menu: menu::LOAD },
    Spec { label: "Save", label_ja: "保存", icon: Icon::Save, tip: "Save: save the file (hover for menu)", tip_ja: "保存: 上書き保存（乗せるとメニュー）", main: Some(Action::Save), menu: menu::SAVE },
    Spec { label: "Undo", label_ja: "戻す", icon: Icon::Undo, tip: "Undo: undo the last edit (hover for menu)", tip_ja: "戻す: 元に戻す（乗せるとメニュー）", main: Some(Action::Undo), menu: menu::UNDO },
    Spec { label: "Redo", label_ja: "進む", icon: Icon::Redo, tip: "Redo: redo the undone edit (hover for menu)", tip_ja: "進む: やり直し（乗せるとメニュー）", main: Some(Action::Redo), menu: menu::REDO },
    // Edit は選択系操作の入口。左クリックは Magic Select（選択を単語→行→段落→全体へ広げる）
    Spec { label: "Edit", label_ja: "選択", icon: Icon::Select, tip: "Edit: expand the selection (hover for menu)", tip_ja: "選択: 選択範囲を広げる（乗せるとメニュー）", main: Some(Action::MagicSelect), menu: menu::EDIT },
    Spec { label: "Copy", label_ja: "複写", icon: Icon::Copy, tip: "Copy: copy (hover for menu)", tip_ja: "複写: コピー（乗せるとメニュー）", main: Some(Action::Copy), menu: menu::COPY },
    Spec { label: "Paste", label_ja: "貼付", icon: Icon::Paste, tip: "Paste: paste (hover for menu)", tip_ja: "貼付: 貼り付け（乗せるとメニュー）", main: Some(Action::Paste), menu: menu::PASTE },
    Spec { label: "Run", label_ja: "実行", icon: Icon::Run, tip: "Run: run the selection as a command, or open the current folder", tip_ja: "実行: 選択範囲をコマンド実行 / 無ければ現在のフォルダを開く", main: Some(Action::RunCmd), menu: menu::RUN },
];

/// 検索欄の左の、アイコンだけのボタン（言語・配色）。左クリックで切り替え、乗せると一覧。
struct IconSpec {
    icon: Icon,
    main: Action,
    menu: &'static [MenuEntry],
}

const ICON_SPECS: [IconSpec; 2] = [
    IconSpec { icon: Icon::Flag, main: Action::ToggleLang, menu: menu::LANG },
    IconSpec { icon: Icon::Moon, main: Action::ToggleTheme, menu: menu::THEME },
];
const ICON_W: i32 = 32;
/// バーの子の番号: 押しピン(0)・SPECS・余白の次から ICON_SPECS。
const ICON_FIRST: i32 = SPECS.len() as i32 + 2;

fn icon_tip(i: usize) -> String {
    match i {
        0 => {
            let name = lang::ALL.iter().find(|a| a.0 == lang::current()).map_or("", |a| a.2);
            format!("{}: {name}\n{}", lang::tr("言語", "Language"), lang::tr("クリックで English に切り替え（乗せると一覧）", "Click to switch to 日本語 (hover for list)"))
        }
        _ => format!("{}: {}\n{}", lang::tr("配色", "Theme"), current_name(), lang::tr("クリックで明るい・暗いを切り替え（乗せると一覧）", "Click to switch light / dark (hover for list)")),
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// ボタンの右端の 1px の区切り線（上下を少し空ける）。
fn draw_divider(b: &mut Button) {
    draw::draw_rect_fill(b.x() + b.w() - 1, b.y() + 8, 1, b.h() - 16, rgb(mix(color_main(), color_text(), 14)));
}

/// アイコンと名前を横に並べて、真ん中に描く。
fn draw_button(b: &mut Button, icon: Icon) {
    let bg = if b.value() { b.selection_color() } else { b.color() };
    draw::draw_box(b.frame(), b.x(), b.y(), b.w(), b.h(), bg);
    draw::set_font(b.label_font(), b.label_size());
    let label = b.label();
    let tw = draw::width(&label) as i32;
    let gap = 4;
    let x0 = b.x() + (b.w() - icons::SIZE - gap - tw) / 2;
    draw::set_draw_color(b.label_color());
    icons::draw(icon, x0, b.y() + (b.h() - icons::SIZE) / 2);
    draw::draw_text2(&label, x0 + icons::SIZE + gap, b.y(), tw + 2, b.h(), Align::Left | Align::Inside);
    draw_divider(b);
}

/// 言語を切り替えたあと、ボタンの名前とツールチップを今の言語に合わせ直す。
pub fn relabel(bar: &Flex, top: &Button, gap: &Frame) {
    for (i, spec) in SPECS.iter().enumerate() {
        if let Some(w) = bar.child(i as i32 + 1) {
            let mut b = unsafe { Button::from_widget_ptr(w.as_widget_ptr()) };
            b.set_label(spec.label());
            b.set_tooltip(spec.tip());
        }
    }
    refresh_icon_tips(bar);
    let mut top = top.clone();
    if !top.value() {
        top.set_tooltip(lang::tr("ウィンドウを最前面に固定する", "Keep the window on top"));
    }
    gap.clone().set_tooltip(lang::tr("クリックで現在のパスをコピー", "Click to copy the current path"));
    bar.clone().redraw();
}

/// 言語・配色のボタンのツールチップを、今の状態に合わせる。
pub fn refresh_icon_tips(bar: &Flex) {
    for i in 0..ICON_SPECS.len() {
        if let Some(w) = bar.child(ICON_FIRST + i as i32) {
            let mut b = unsafe { Button::from_widget_ptr(w.as_widget_ptr()) };
            b.set_tooltip(&icon_tip(i));
        }
    }
}

/// 配色を切り替えたあと、ツールバーの色を今のパレットに合わせ直す。
pub fn restyle(bar: &Flex, gap: &Frame) {
    let mut bar = bar.clone();
    bar.set_color(rgb(color_sub()));
    for i in 0..SPECS.len() {
        if let Some(w) = bar.child(i as i32 + 1) {
            let mut b = unsafe { Button::from_widget_ptr(w.as_widget_ptr()) };
            b.set_color(rgb(color_main()));
            b.set_selection_color(rgb(color_accent()));
            b.set_label_color(rgb(color_subtext()));
        }
    }
    let mut gap = gap.clone();
    gap.set_color(rgb(color_sub()));
    gap.set_label_color(rgb(color_subtext()));
    bar.redraw();
}

/// Flex（高さ HEIGHT の横並び）を作って返す。`on_action` は選ばれた Action を受け取る。
/// 戻り値は (バー, 余白, 最前面の押しピン, extra の結果)。余白はパスコピーの結果表示に使う。
/// 固定ボタンは行番号欄を常に出す切り替え（押したときの動作と見た目は呼び出し側が決める）。
/// `extra` は、余白の右に足す部品を作る（検索バー）。
pub fn build<F: Fn(Action) + Clone + 'static, T>(on_action: F, extra: impl FnOnce(&mut Flex) -> T) -> (Flex, Frame, Button, T) {
    let mut bar = Flex::default().row();
    bar.set_color(rgb(color_sub()));
    bar.set_frame(FrameType::FlatBox);
    bar.set_pad(0); // ボタンの間は隙間ではなく 1px の区切り線で分ける（draw_divider）

    // 左端: ウィンドウを最前面に固定する押しピン（押したときの動作と幅は呼び出し側が決める。value が固定中）
    let mut top = Button::default();
    top.set_frame(FrameType::FlatBox);
    top.set_down_frame(FrameType::FlatBox);
    top.clear_visible_focus();
    top.set_tooltip(lang::tr("ウィンドウを最前面に固定する", "Keep the window on top"));
    top.super_draw(false);
    top.draw(|b| {
        draw::draw_box(FrameType::FlatBox, b.x(), b.y(), b.w(), b.h(), rgb(color_sub()));
        let on = b.value();
        draw::set_draw_color(rgb(if on { color_accent() } else { color_subtext() }));
        let s = icons::SIZE;
        let (x, y) = (b.x() + (b.w() - s) / 2, b.y() + (b.h() - s) / 2);
        icons::draw(Icon::Pin, x, y);
        if on {
            draw::draw_rect_fill(x, b.y() + b.h() - 3, s, 2, rgb(color_accent()));
        }
        draw::draw_rect_fill(b.x() + b.w() - 1, b.y() + 8, 1, b.h() - 16, rgb(mix(color_sub(), color_text(), 14)));
    });
    bar.fixed(&top, 32);

    for spec in SPECS.iter() {
        let mut b = Button::default().with_label(spec.label());
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(FrameType::FlatBox);
        b.set_color(rgb(color_main()));
        b.set_selection_color(rgb(color_accent()));
        b.set_label_color(if spec.main.is_some() { rgb(color_subtext()) } else { rgb(color_subtext()).darker() });
        b.set_tooltip(spec.tip());
        b.clear_visible_focus(); // キーボードフォーカスは編集領域に残す
        let icon = spec.icon;
        b.super_draw(false);
        b.draw(move |b| draw_button(b, icon));
        bar.fixed(&b, BUTTON_W);

        let cb = on_action.clone();
        let main = spec.main;
        b.set_callback(move |_| {
            if let Some(a) = main {
                cb(a);
            }
        });
        let cb = on_action.clone();
        let menu = spec.menu;
        b.super_handle_first(false); // 右クリックでボタン標準の動作（左クリック扱い）を起こさない
        let enabled = spec.main.is_some();
        b.handle(move |w, ev| {
            // 文字はふだん控えめ（subtext）にして、マウスを乗せたときだけ text にする
            let remember = |w: &Button| {
                LAST_BUTTON.with(|l| *l.borrow_mut() = Some(w.clone()));
                if let Some(win) = w.window() {
                    LAST_BUTTON_BOTTOM.with(|c| c.set(Some((win.x_root() + w.x(), win.y_root() + w.y() + w.h()))));
                }
            };
            if matches!(ev, Event::Enter | Event::Leave) {
                if enabled {
                    w.set_label_color(rgb(if ev == Event::Enter { color_text() } else { color_subtext() }));
                    w.redraw();
                }
                if ev == Event::Enter {
                    remember(w);
                    let cb = cb.clone();
                    let w2 = w.clone();
                    hovermenu::hover(w, menu, main, move |a| {
                        remember(&w2);
                        cb(a)
                    });
                }
                return ev == Event::Enter; // Enter を受けると Leave も届く
            }
            if ev == Event::Push && app::event_mouse_button() == app::MouseButton::Right {
                remember(w);
                let cb = cb.clone();
                let w2 = w.clone();
                hovermenu::open(w, menu, main, move |a| {
                    remember(&w2);
                    cb(a)
                });
                return true; // 右クリックでは左クリックの動作を起こさない
            }
            if ev == Event::Push {
                hovermenu::close(); // 左クリックは主な動作。開いているメニューは閉じる
            }
            false
        });
    }


    let mut gap = Frame::default();
    gap.set_frame(FrameType::FlatBox);
    gap.set_color(rgb(color_sub()));
    gap.set_label_color(rgb(color_subtext()));
    gap.set_align(Align::Inside | Align::Right);
    gap.set_tooltip(lang::tr("クリックで現在のパスをコピー", "Click to copy the current path"));
    let cb = on_action.clone();
    gap.handle(move |_, ev| {
        if ev == Event::Push {
            cb(Action::CopyPath);
            return true;
        }
        false
    });
    for (i, spec) in ICON_SPECS.iter().enumerate() {
        let mut b = Button::default();
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(FrameType::FlatBox);
        b.clear_visible_focus();
        b.set_tooltip(&icon_tip(i));
        let icon = spec.icon;
        b.super_draw(false);
        b.draw(move |b| {
            draw::draw_box(FrameType::FlatBox, b.x(), b.y(), b.w(), b.h(), rgb(color_sub()));
            let hovered = app::belowmouse::<fltk::widget::Widget>().is_some_and(|w| w.as_widget_ptr() == b.as_widget_ptr());
            draw::set_draw_color(rgb(if hovered || hovermenu::is_open_for(b) { color_text() } else { color_subtext() }));
            let s = icons::SIZE;
            icons::draw(icon, b.x() + (b.w() - s) / 2, b.y() + (b.h() - s) / 2);
        });
        bar.fixed(&b, ICON_W);
        let cb = on_action.clone();
        let main = spec.main;
        b.set_callback(move |_| cb(main));
        let cb = on_action.clone();
        let menu = spec.menu;
        b.super_handle_first(false);
        b.handle(move |w, ev| {
            if matches!(ev, Event::Enter | Event::Leave) {
                w.redraw();
                if ev == Event::Enter {
                    let cb = cb.clone();
                    hovermenu::hover(w, menu, Some(main), move |a| cb(a));
                }
                return ev == Event::Enter;
            }
            if ev == Event::Push && app::event_mouse_button() == app::MouseButton::Right {
                let cb = cb.clone();
                hovermenu::open(w, menu, Some(main), move |a| cb(a));
                return true;
            }
            if ev == Event::Push {
                hovermenu::close();
            }
            false
        });
    }
    let extra = extra(&mut bar);
    bar.end();
    (bar, gap, top, extra)
}
