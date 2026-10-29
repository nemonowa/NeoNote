// 検索バー（ツールバーの右端）。検索欄は常に出しておく（左に虫眼鏡）。
// 検索欄が使われる（フォーカスが入る・Ctrl+F / Ctrl+H・右クリックの Search / Replace）と、
//   ツールバー: 🔍 [検索欄 ×]
//   その真下  : ⇄ [置換欄 ×]      （編集領域に重ねて出す。縦の場所は取らない）
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
const INPUT_W: i32 = 228;
/// 欄の中の × の幅。
const CLEAR_W: i32 = 22;
const PANEL_H: i32 = 28;
/// 検索の段と置換の段の間（つながった 1 つの箱に見えないように空ける）。
const PANEL_GAP: i32 = 4;

/// 「見つかりません」の印（呼び出し側は lang::tr のどちらかを渡す）。
fn is_not_found(text: &str) -> bool {
    text == "Not found" || text == "見つかりません"
}

fn find_tip() -> &'static str {
    crate::lang::tr(
        "検索（入力すると強調して最初の一致を選ぶ。Enter / F3 で次、Shift+Enter / Shift+F3 で前、Esc で閉じる）",
        "Find (typing highlights and selects the first match. Enter / F3 next, Shift+Enter / Shift+F3 previous, Esc closes)",
    )
}
fn open_tip() -> &'static str {
    crate::lang::tr("検索・置換（Ctrl+F / Ctrl+H）", "Find and replace (Ctrl+F / Ctrl+H)")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindCmd {
    /// 虫眼鏡が押された（閉じていれば開いて検索欄へ移る。開いていれば全てを強調して次の一致へ）。
    Search,
    /// 検索欄の文字列が変わった。
    Changed,
    /// いま選んでいる一致を置換欄の文字列に置き換えて、次の一致へ進む。
    ReplaceOne,
    /// 検索中の文字列を、置換欄の文字列に全て置き換える。
    ReplaceAll,
    /// 文字列と強調を消して、閉じる。
    Clear,
    /// 置換欄の文字列を消す。
    ClearReplace,
}

#[derive(Clone)]
pub struct SearchBar {
    bar: Flex,
    /// 虫眼鏡のボタン（検索）。
    open_btn: Button,
    pub input: Input,
    pub replace: Input,
    buttons: Vec<Button>,
    /// 開いているときだけ出す部品（検索欄の中の ×）。
    side: Vec<fltk::widget::Widget>,
    /// 検索欄の真下に重ねる置換の段（置換ボタン・置換欄）。ウィンドウの直下に置く。
    panel: Flex,
    /// 入力欄と × をまとめた枠（欄の地色で塗る）。配色を切り替えたときに塗り直す。
    fields: Vec<Flex>,
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

/// 入力欄の地（ツールバーより少し明るくして、欄の形が見えるようにする）。
fn field_bg() -> (u8, u8, u8) {
    mix(color_main(), color_text(), 8)
}

fn style_input(i: &mut Input) {
    i.set_frame(FrameType::FlatBox);
    i.set_color(rgb(field_bg()));
    i.set_text_color(rgb(color_text()));
    i.set_cursor_color(rgb(color_accent()));
    i.set_selection_color(rgb(color_accent()));
}

/// 入力欄と、その中の右端の × を 1 つの枠にまとめて `parent` に足す。× は入力欄と同じ地色にする。
fn field<F: Fn(FindCmd) + Clone + 'static>(parent: &mut Flex, font: fltk::enums::Font, size: i32, tip: &str, clear_tip: &str, clear: FindCmd, on_cmd: &F) -> (Flex, Input, Button) {
    let mut row = Flex::default().row();
    row.set_frame(FrameType::FlatBox);
    row.set_color(rgb(field_bg()));
    row.set_pad(0);
    let mut input = Input::default();
    style_input(&mut input);
    input.set_text_font(font);
    input.set_text_size(size);
    input.set_tooltip(tip);
    let mut x = Button::default().with_label("×");
    style_button(&mut x, clear_tip);
    x.set_frame(FrameType::NoBox); // 欄の地をそのまま見せる
    x.set_down_frame(FrameType::NoBox);
    row.fixed(&x, CLEAR_W);
    let cb = on_cmd.clone();
    x.set_callback(move |_| cb(clear));
    row.end();
    parent.fixed(&row, INPUT_W);
    (row, input, x)
}

/// 検索バーを、作りかけのツールバー `bar` の最後に足す。`font` は入力欄の文字（編集領域と同じにする）。
/// 作った直後は閉じている（虫眼鏡と検索欄だけ）。
pub fn build<F: Fn(FindCmd) + Clone + 'static>(bar: &mut Flex, font: fltk::enums::Font, size: i32, on_cmd: F) -> SearchBar {
    let mut open_btn = Button::default();
    style_button(&mut open_btn, open_tip());
    open_btn.draw(draw_magnifier);
    bar.fixed(&open_btn, OPEN_W);
    let cb = on_cmd.clone();
    open_btn.set_callback(move |_| cb(FindCmd::Search));

    let (find_row, mut input, find_x) =
        field(bar, font, size, find_tip(), crate::lang::tr("検索を消して閉じる（Esc は閉じるだけ）", "Clear the search and close (Esc only closes)"), FindCmd::Clear, &on_cmd);
    input.set_trigger(CallbackTrigger::Changed);
    let cb = on_cmd.clone();
    input.set_callback(move |_| cb(FindCmd::Changed));
    let mut buttons = vec![open_btn.clone(), find_x.clone()];
    let side: Vec<fltk::widget::Widget> = vec![find_x.as_base_widget()];

    // 置換の段は、ツールバーの部品ではなくウィンドウの直下に置く（編集領域の上に重ねる）
    let win: Option<Group> = bar.parent().and_then(|col| col.parent()); // ツールバー → 縦の並び → ウィンドウ
    Group::set_current(win.as_ref());
    let mut panel = Flex::default().row();
    panel.set_frame(FrameType::FlatBox);
    panel.set_color(rgb(color_sub()));
    panel.set_pad(0);
    panel.resize(0, 0, OPEN_W + INPUT_W, PANEL_H);
    let mut repl_btn = Button::default();
    style_button(
        &mut repl_btn,
        crate::lang::tr("置換: 選んでいる一致を置き換えて次へ（ダブルクリックで全て置換）", "Replace: replace the selected match and go to the next (double-click replaces all)"),
    );
    repl_btn.draw(draw_replace);
    panel.fixed(&repl_btn, OPEN_W);
    let cb = on_cmd.clone();
    repl_btn.set_callback(move |_| cb(if fltk::app::event_clicks() { FindCmd::ReplaceAll } else { FindCmd::ReplaceOne }));
    let (repl_row, replace, repl_x) = field(
        &mut panel,
        font,
        size,
        crate::lang::tr("置換後の文字列（Enter で、検索中の文字列を全て置換。元に戻すで 1 回で戻る）", "Replacement text (Enter replaces all matches. Undo reverts in one step)"),
        crate::lang::tr("置換欄を消す", "Clear the replacement"),
        FindCmd::ClearReplace,
        &on_cmd,
    );
    buttons.push(repl_btn);
    buttons.push(repl_x);
    panel.end();
    bar.begin(); // 作りかけのツールバーに戻る（呼び出し側が end する）

    let sb = SearchBar { bar: bar.clone(), open_btn, input, replace, buttons, side, panel, fields: vec![find_row, repl_row] };
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
        for f in &self.fields {
            f.clone().layout();
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
        panel.resize(self.open_btn.x(), self.bar.y() + self.bar.h() + PANEL_GAP, OPEN_W + INPUT_W, PANEL_H);
        panel.layout();
        panel.redraw();
    }

    /// 今の状態（件数など）をツールチップに出す。見つからないときは検索欄の文字を赤くして知らせる。
    pub fn set_status(&self, text: &str) {
        let tip = if text.is_empty() { find_tip().to_string() } else { format!("{text}\n{}", find_tip()) };
        let mut input = self.input.clone();
        input.set_tooltip(&tip);
        input.set_text_color(if is_not_found(text) { rgb(color_error()) } else { rgb(color_text()) });
        input.redraw();
        let open_tip = if text.is_empty() {
            open_tip().to_string()
        } else if crate::lang::is_en() {
            format!("Searching: {text}\n{}", open_tip())
        } else {
            format!("検索中: {text}\n{}", open_tip())
        };
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

/// 置換（左右に入れ替わる 2 本の矢印）。
fn draw_replace(b: &mut Button) {
    draw::draw_box(b.frame(), b.x(), b.y(), b.w(), b.h(), b.color());
    draw::set_draw_color(b.label_color());
    let (x, y) = (b.x() + (b.w() - 14) / 2, b.y() + (b.h() - 14) / 2);
    draw::draw_line(x + 1, y + 4, x + 12, y + 4);
    draw::draw_polygon(x + 13, y + 4, x + 9, y + 1, x + 9, y + 7);
    draw::draw_line(x + 2, y + 10, x + 13, y + 10);
    draw::draw_polygon(x, y + 10, x + 4, y + 7, x + 4, y + 13);
}

/// 配色を切り替えたあと、色を今のパレットに合わせ直す（ツールバー本体の地色は toolbar::restyle が直す）。
pub fn restyle(sb: &SearchBar) {
    sb.panel.clone().set_color(rgb(color_sub()));
    for f in &sb.fields {
        let mut f = f.clone();
        f.set_color(rgb(field_bg()));
        f.redraw();
    }
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
