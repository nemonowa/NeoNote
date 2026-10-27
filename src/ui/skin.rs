// FLTK の標準部品（スクロールバー・ダイアログ・ファイル以外の入力欄など）を、今のパレットの色にそろえる。
// 自前で色を付けていない部品は FLTK の既定（灰色の立体）で描かれ、画面の中で浮いて見えるため。
use super::theme::*;
use fltk::{
    app,
    enums::{Color, FrameType},
    group::Group,
    prelude::*,
    valuator::Scrollbar,
};

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// 全体の既定の色（ダイアログ・メッセージ・標準のボタン）。起動時とパレットを変えたときに呼ぶ。
pub fn apply_globals() {
    let (m, s, t) = (color_main(), color_sub(), color_text());
    app::background(s.0, s.1, s.2);
    app::background2(m.0, m.1, m.2);
    app::foreground(t.0, t.1, t.2);
    let a = color_accent();
    app::set_selection_color(a.0, a.1, a.2);
    // ツールチップ（既定は黄色の地）も、一段手前の色の地に本文の文字で出す。余白は 4px の倍数にそろえる
    fltk::misc::Tooltip::set_color(rgb(mix(color_sub(), color_text(), 12)));
    fltk::misc::Tooltip::set_text_color(rgb(color_text()));
    fltk::misc::Tooltip::set_margin_width(8);
    fltk::misc::Tooltip::set_margin_height(4);
    // 立体の枠をやめて平らにする（ダイアログのボタン・入力欄が、自前の部品と同じ見た目になる）。
    // 角の丸い scheme（gtk・plastic・gleam・oxy）は使わない。四角い Base のまま枠の種類だけ差し替える
    if !GLOBALS_DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        app::set_frame_type2(FrameType::UpBox, FrameType::FlatBox);
        app::set_frame_type2(FrameType::ThinUpBox, FrameType::FlatBox);
        app::set_frame_type2(FrameType::DownBox, FrameType::BorderBox);
        app::set_frame_type2(FrameType::ThinDownBox, FrameType::BorderBox);
    }
}

/// 枠の種類の差し替えは 1 回だけ（2 回目は差し替え後の枠を差し替えてしまう）。
static GLOBALS_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// スクロールバーの見た目: 溝は sub、つまみは text を少し混ぜた色、矢印は subtext。
fn style_one(sb: &mut Scrollbar) {
    sb.set_frame(FrameType::FlatBox);
    sb.set_slider_frame(FrameType::FlatBox);
    sb.set_color(rgb(color_sub()));
    sb.set_selection_color(rgb(mix(color_sub(), color_text(), 30)));
    sb.set_label_color(rgb(color_subtext()));
    sb.redraw();
}

/// スクロールバーを中に持つ部品（TextDisplay / TextEditor / HelpView / Browser）のスクロールバーに色を付ける。
/// これらの部品は FLTK の中ではグループで、子はスクロールバーだけ。
pub fn style_scrollbars<W: WidgetExt>(w: &W) {
    // Safety: 上の部品は Fl_Group を継承しているので、グループとして子を辿れる
    let g = unsafe { Group::from_widget_ptr(w.as_widget_ptr() as *mut _) };
    for i in 0..g.children() {
        if let Some(c) = g.child(i) {
            let mut sb = unsafe { Scrollbar::from_widget_ptr(c.as_widget_ptr() as *mut _) };
            style_one(&mut sb);
        }
    }
}
