// ツールバー: 8 ボタン + 行番号の固定ボタン（#）+ 余白 + 右端の検索バー（searchbar.rs。使うときだけ開く）。
// 左クリック = そのボタンの主な動作、右クリック = そのボタンのメニュー。
// 余白をクリックすると現在のパスをコピーする（ダイアログは出さない）。
use super::{menu::{self, Action, MenuEntry}, theme::*};
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
const BUTTON_W: i32 = 48;
/// 行番号欄の固定ボタン（`#`）の幅。
const PIN_W: i32 = 24;

struct Spec {
    label: &'static str,
    tip: &'static str,
    /// 左クリックの動作。None はグレー表示で何も起きない。
    main: Option<Action>,
    menu: &'static [MenuEntry],
}

const SPECS: [Spec; 8] = [
    Spec { label: "Load", tip: "Load: 開く（右クリックでメニュー）", main: Some(Action::Open), menu: menu::LOAD },
    Spec { label: "Save", tip: "Save: 保存（右クリックでメニュー）", main: Some(Action::Save), menu: menu::SAVE },
    Spec { label: "Undo", tip: "Undo: 元に戻す（右クリックでメニュー）", main: Some(Action::Undo), menu: menu::UNDO },
    Spec { label: "Redo", tip: "Redo: やり直し（右クリックでメニュー）", main: Some(Action::Redo), menu: menu::REDO },
    // Edit は選択系操作の入口。左クリックは Magic Select（選択を単語→行→段落→全体へ広げる）
    Spec { label: "Edit", tip: "Edit: 選択範囲を広げる（右クリックでメニュー）", main: Some(Action::MagicSelect), menu: menu::EDIT },
    Spec { label: "Copy", tip: "Copy: コピー（右クリックでメニュー）", main: Some(Action::Copy), menu: menu::COPY },
    Spec { label: "Paste", tip: "Paste: 貼り付け（右クリックでメニュー）", main: Some(Action::Paste), menu: menu::PASTE },
    Spec { label: "Run", tip: "Run: 選択範囲をコマンド実行 / 無ければ現在のフォルダを開く", main: Some(Action::RunCmd), menu: menu::RUN },
];

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// ボタンの右端の 1px の区切り線（上下を少し空ける）。
fn draw_divider(b: &mut Button) {
    draw::draw_rect_fill(b.x() + b.w() - 1, b.y() + 8, 1, b.h() - 16, rgb(mix(color_main(), color_text(), 14)));
}

/// 配色を切り替えたあと、ツールバーの色を今のパレットに合わせ直す。
pub fn restyle(bar: &Flex, gap: &Frame) {
    let mut bar = bar.clone();
    bar.set_color(rgb(color_sub()));
    for i in 0..SPECS.len() {
        if let Some(w) = bar.child(i as i32) {
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
/// 戻り値は (バー, 余白, 固定ボタン, extra の結果)。余白はパスコピーの結果表示に使う。
/// 固定ボタンは行番号欄を常に出す切り替え（押したときの動作と見た目は呼び出し側が決める）。
/// `extra` は、余白の右に足す部品を作る（検索バー）。
pub fn build<F: Fn(Action) + Clone + 'static, T>(on_action: F, extra: impl FnOnce(&mut Flex) -> T) -> (Flex, Frame, Button, T) {
    let mut bar = Flex::default().row();
    bar.set_color(rgb(color_sub()));
    bar.set_frame(FrameType::FlatBox);
    bar.set_pad(0); // ボタンの間は隙間ではなく 1px の区切り線で分ける（draw_divider）

    for spec in SPECS.iter() {
        let mut b = Button::default().with_label(spec.label);
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(FrameType::FlatBox);
        b.set_color(rgb(color_main()));
        b.set_selection_color(rgb(color_accent()));
        b.set_label_color(if spec.main.is_some() { rgb(color_subtext()) } else { rgb(color_subtext()).darker() });
        b.set_tooltip(spec.tip);
        b.clear_visible_focus(); // キーボードフォーカスは編集領域に残す
        b.draw(draw_divider);
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
            if enabled && matches!(ev, Event::Enter | Event::Leave) {
                w.set_label_color(rgb(if ev == Event::Enter { color_text() } else { color_subtext() }));
                w.redraw();
                return ev == Event::Enter; // Enter を受けると Leave も届く
            }
            if ev == Event::Push && app::event_mouse_button() == app::MouseButton::Right {
                LAST_BUTTON.with(|l| *l.borrow_mut() = Some(w.clone()));
                LAST_BUTTON_BOTTOM.with(|c| c.set(Some((app::event_x_root() - app::event_x() + w.x(), app::event_y_root() - app::event_y() + w.y() + w.h()))));
                if let Some(a) = menu::popup(menu, app::event_x(), app::event_y()) {
                    cb(a);
                }
                return true; // 右クリックでは左クリックの動作を起こさない
            }
            false
        });
    }

    // 行番号欄の固定ボタン。以前は編集領域の上に専用の帯を置いていたが、縦が無駄なのでツールバーへ移した
    let mut pin = Button::default().with_label("#");
    pin.set_frame(FrameType::FlatBox);
    pin.set_down_frame(FrameType::FlatBox);
    pin.clear_visible_focus();
    // 固定中（value が true）はアクセント色の下線で示す。地の色は変えない
    pin.draw(|b| {
        draw_divider(b);
        if b.value() {
            draw::draw_rect_fill(b.x() + 4, b.y() + b.h() - 2, b.w() - 8, 2, rgb(color_accent()));
        }
    });
    bar.fixed(&pin, PIN_W);

    let mut gap = Frame::default();
    gap.set_frame(FrameType::FlatBox);
    gap.set_color(rgb(color_sub()));
    gap.set_label_color(rgb(color_subtext()));
    gap.set_align(Align::Inside | Align::Right);
    gap.set_tooltip("クリックで現在のパスをコピー");
    let cb = on_action;
    gap.handle(move |_, ev| {
        if ev == Event::Push {
            cb(Action::CopyPath);
            return true;
        }
        false
    });
    let extra = extra(&mut bar);
    bar.end();
    (bar, gap, pin, extra)
}
