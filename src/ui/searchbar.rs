// 検索バー（ツールバーの右端）。検索欄は常に出しておく（左に虫眼鏡）。
// 検索欄が使われる（フォーカスが入る・Ctrl+F / Ctrl+H・右クリックの Search / Replace）と、
//   ツールバー: 🔍 [検索欄] ← → ×
//   その真下  :    [置換欄] Repl All      （編集領域に重ねて出す。縦の場所は取らない）
// を出す。閉じる: Esc・×。入力するたびに全てを強調して最初の 1 件を選び、Enter / F3 で次、
// Shift+Enter / Shift+F3 で前へ進む。置換欄で Enter（か All）を押すと、検索中の文字列を全て置き換える。
// 説明や状態（キー操作・件数）は、場所を取らないようツールチップに出す（件数の欄は持たない）。
// ボタンと入力欄の操作は FindCmd として呼び出し側へ返す（メインループで処理する）。
// キー（Enter・Esc など）は呼び出し側が `input.handle` で受ける。
use super::theme::*;
use fltk::{
    button::Button,
    draw,
    enums::{CallbackTrigger, Color, FrameType},
    group::{Flex, Group},
    input::Input,
    prelude::*,
};

const OPEN_W: i32 = 28;
const INPUT_W: i32 = 168;
const STEP_W: i32 = 24;
/// 検索欄の右、← → × を並べる幅（置換欄の右の Repl / All の幅と同じ）。
const SIDE_W: i32 = 96;
const REPL_W: i32 = 56;
const ALL_W: i32 = SIDE_W - REPL_W;
const PANEL_H: i32 = 28;

const FIND_TIP: &str = "検索（入力すると強調して最初の一致を選ぶ。Enter / F3 で次、Shift+Enter / Shift+F3 で前、Esc で閉じる）";
const OPEN_TIP: &str = "検索・置換（Ctrl+F / Ctrl+H）";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindCmd {
    /// 虫眼鏡が押された（開いて検索欄へ移る）。
    Open,
    /// 検索欄の文字列が変わった。
    Changed,
    Next,
    Prev,
    /// いま選んでいる一致を置換欄の文字列に置き換えて、次の一致へ進む。
    ReplaceOne,
    /// 検索中の文字列を、置換欄の文字列に全て置き換える。
    ReplaceAll,
    /// 文字列と強調を消して、閉じる。
    Clear,
}

#[derive(Clone)]
pub struct SearchBar {
    bar: Flex,
    /// 虫眼鏡のボタン（押すと検索欄へ移る）。
    open_btn: Button,
    pub input: Input,
    pub replace: Input,
    buttons: Vec<Button>,
    /// ツールバーの検索欄の右に、開いているときだけ出す部品（← → ×）。
    side: Vec<fltk::widget::Widget>,
    /// 検索欄の真下に重ねる置換の段（置換欄・Repl・All）。ウィンドウの直下に置く。
    panel: Flex,
    /// 検索欄の右（← → × の地）。配色を切り替えたときに塗り直す。
    side_row: Flex,
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

fn style_button(b: &mut Button, tip: &str) {
    b.set_frame(FrameType::FlatBox);
    b.set_down_frame(FrameType::FlatBox);
    b.set_color(rgb(color_main()));
    b.set_selection_color(rgb(color_accent()));
    b.set_label_color(rgb(color_subtext()));
    b.set_tooltip(tip);
    b.clear_visible_focus(); // キーボードフォーカスは編集領域に残す
}

fn style_input(i: &mut Input) {
    i.set_frame(FrameType::FlatBox);
    i.set_color(rgb(color_main()));
    i.set_text_color(rgb(color_text()));
    i.set_cursor_color(rgb(color_accent()));
    i.set_selection_color(rgb(color_accent()));
}

/// 検索バーを、作りかけのツールバー `bar` の最後に足す。`font` は入力欄の文字（編集領域と同じにする）。
/// 作った直後は閉じている（虫眼鏡のボタンだけ）。
pub fn build<F: Fn(FindCmd) + Clone + 'static>(bar: &mut Flex, font: fltk::enums::Font, size: i32, on_cmd: F) -> SearchBar {
    let mut open_btn = Button::default();
    style_button(&mut open_btn, OPEN_TIP);
    open_btn.draw(draw_magnifier);
    bar.fixed(&open_btn, OPEN_W);
    let cb = on_cmd.clone();
    open_btn.set_callback(move |_| cb(FindCmd::Open));

    let mut input = Input::default();
    style_input(&mut input);
    input.set_text_font(font);
    input.set_text_size(size);
    input.set_trigger(CallbackTrigger::Changed);
    input.set_tooltip(FIND_TIP);
    bar.fixed(&input, INPUT_W);
    let cb = on_cmd.clone();
    input.set_callback(move |_| cb(FindCmd::Changed));

    // 検索欄の右: ← → ×（閉じているときは空白）。幅は固定にして、開閉で検索欄が動かないようにする
    let mut side_row = Flex::default().row();
    side_row.set_frame(FrameType::FlatBox);
    side_row.set_color(rgb(color_sub()));
    bar.fixed(&side_row, SIDE_W);
    let mut buttons = vec![open_btn.clone()];
    let mut side: Vec<fltk::widget::Widget> = Vec::new();
    for (label, tip, cmd) in [
        ("←", "前の一致（Shift+F3 / Shift+Enter）", FindCmd::Prev),
        ("→", "次の一致（F3 / Enter）", FindCmd::Next),
        ("×", "検索を消して閉じる（Esc は閉じるだけ）", FindCmd::Clear),
    ] {
        let mut b = Button::default().with_label(label);
        style_button(&mut b, tip);
        side_row.fixed(&b, STEP_W);
        let cb = on_cmd.clone();
        b.set_callback(move |_| cb(cmd));
        side.push(b.as_base_widget());
        buttons.push(b);
    }
    side_row.end();

    // 置換の段は、ツールバーの部品ではなくウィンドウの直下に置く（編集領域の上に重ねる）
    let win: Option<Group> = bar.parent().and_then(|col| col.parent()); // ツールバー → 縦の並び → ウィンドウ
    Group::set_current(win.as_ref());
    let mut panel = Flex::default().row();
    panel.set_frame(FrameType::FlatBox);
    panel.set_color(rgb(color_sub()));
    panel.resize(0, 0, INPUT_W + SIDE_W, PANEL_H);
    let mut replace = Input::default();
    style_input(&mut replace);
    replace.set_text_font(font);
    replace.set_text_size(size);
    replace.set_tooltip("置換後の文字列（Enter か All で、検索中の文字列を全て置換。元に戻すで 1 回で戻る）");
    panel.fixed(&replace, INPUT_W);
    for (label, tip, cmd, w) in [
        ("Repl", "選んでいる一致を置換して、次の一致へ進む", FindCmd::ReplaceOne, REPL_W),
        ("All", "検索中の文字列を全て置換", FindCmd::ReplaceAll, ALL_W),
    ] {
        let mut b = Button::default().with_label(label);
        style_button(&mut b, tip);
        b.set_label_size(12);
        panel.fixed(&b, w);
        let cb = on_cmd.clone();
        b.set_callback(move |_| cb(cmd));
        buttons.push(b);
    }
    panel.end();
    bar.begin(); // 作りかけのツールバーに戻る（呼び出し側が end する）

    let sb = SearchBar { bar: bar.clone(), open_btn, input, replace, buttons, side, panel, side_row };
    sb.set_open(false);
    sb
}

impl SearchBar {
    pub fn is_open(&self) -> bool {
        self.panel.visible()
    }

    /// 開く（← → × と、検索欄の真下の置換の段を出す）／閉じる（検索欄だけにする）。
    pub fn set_open(&self, open: bool) {
        for w in &self.side {
            let mut w = w.clone();
            if open {
                w.show();
            } else {
                w.hide();
            }
        }
        let mut bar = self.bar.clone();
        bar.layout();
        bar.redraw();
        let mut panel = self.panel.clone();
        if open {
            self.place();
            panel.show();
        } else {
            panel.hide();
        }
        if let Some(mut win) = bar.window() {
            win.redraw();
        }
    }

    /// 置換の段を描き直す。編集領域が自分の範囲だけ描き直すと、重ねてある置換の段が上書きされて消えるので、
    /// 編集領域の描画のたびに呼ぶ。
    pub fn redraw_overlay(&self) {
        if self.panel.visible() {
            self.panel.clone().redraw();
        }
    }

    /// 置換の段を、検索欄の真下へ置く（ウィンドウの大きさが変わったときにも呼ぶ）。
    pub fn place(&self) {
        let mut panel = self.panel.clone();
        panel.resize(self.input.x(), self.bar.y() + self.bar.h(), INPUT_W + SIDE_W, PANEL_H);
        panel.layout();
        panel.redraw();
    }

    /// 今の状態（件数など）をツールチップに出す。見つからないときは検索欄の文字を赤くして知らせる。
    pub fn set_status(&self, text: &str) {
        let tip = if text.is_empty() { FIND_TIP.to_string() } else { format!("{text}\n{FIND_TIP}") };
        let mut input = self.input.clone();
        input.set_tooltip(&tip);
        input.set_text_color(if text == "Not found" { rgb(color_error()) } else { rgb(color_text()) });
        input.redraw();
        let open_tip = if text.is_empty() { OPEN_TIP.to_string() } else { format!("検索中: {text}\n{OPEN_TIP}") };
        self.open_btn.clone().set_tooltip(&open_tip);
    }
}

/// 虫眼鏡（フォントに依存しないよう、線で描く）。
fn draw_magnifier(b: &mut Button) {
    draw::draw_box(b.frame(), b.x(), b.y(), b.w(), b.h(), b.color());
    let fg = b.label_color();
    draw::set_draw_color(fg);
    draw::set_line_style(draw::LineStyle::Solid, 2);
    let (cx, cy) = (b.x() + b.w() / 2 - 2, b.y() + b.h() / 2 - 2);
    let r = 5;
    draw::draw_arc(cx - r, cy - r, 2 * r, 2 * r, 0.0, 360.0);
    draw::draw_line(cx + 4, cy + 4, cx + 9, cy + 9);
    draw::set_line_style(draw::LineStyle::Solid, 0);
}

/// 配色を切り替えたあと、色を今のパレットに合わせ直す（ツールバー本体の地色は toolbar::restyle が直す）。
pub fn restyle(sb: &SearchBar) {
    sb.panel.clone().set_color(rgb(color_sub()));
    let mut side_row = sb.side_row.clone();
    side_row.set_color(rgb(color_sub()));
    side_row.redraw();
    for b in &sb.buttons {
        let mut b = b.clone();
        b.set_color(rgb(color_main()));
        b.set_selection_color(rgb(color_accent()));
        b.set_label_color(rgb(color_subtext()));
        b.redraw();
    }
    for i in [&sb.input, &sb.replace] {
        let mut i = i.clone();
        style_input(&mut i);
        i.redraw();
    }
}
