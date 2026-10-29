// ツールバーなどの小さなアイコン。フォントや画像に頼らず、線と塗りで描く（どの配色でも文字色で描ける）。
// どれも (x, y) を左上とする 14x14 の中に描く。色は呼ぶ前に draw::set_draw_color で決める。
use fltk::draw::{self, LineStyle};

pub const SIZE: i32 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Folder,
    Save,
    Undo,
    Redo,
    Select,
    Copy,
    Paste,
    Run,
    Eye,
    Pin,
    Flag,
    Moon,
}

pub fn draw(icon: Icon, x: i32, y: i32) {
    draw::set_line_style(LineStyle::Solid | LineStyle::CapFlat | LineStyle::JoinMiter, 1);
    match icon {
        Icon::Folder => {
            draw::draw_rect(x + 1, y + 4, 12, 8);
            draw::draw_line(x + 1, y + 4, x + 1, y + 2);
            draw::draw_line(x + 1, y + 2, x + 5, y + 2);
            draw::draw_line(x + 5, y + 2, x + 6, y + 4);
        }
        Icon::Save => {
            draw::draw_rect(x + 1, y + 1, 12, 12);
            draw::draw_rect(x + 4, y + 1, 6, 4); // シャッター
            draw::draw_rectf(x + 8, y + 2, 1, 2);
            draw::draw_rect(x + 3, y + 8, 8, 5); // ラベル
        }
        Icon::Undo | Icon::Redo => {
            let redo = icon == Icon::Redo;
            let fx = |dx: i32| if redo { x + SIZE - 1 - dx } else { x + dx };
            // 弧（右へ回り込んで戻る）と、左上の矢じり
            draw::draw_line(fx(3), y + 5, fx(9), y + 5);
            draw::draw_line(fx(9), y + 5, fx(11), y + 7);
            draw::draw_line(fx(11), y + 7, fx(11), y + 9);
            draw::draw_line(fx(11), y + 9, fx(9), y + 11);
            draw::draw_line(fx(9), y + 11, fx(5), y + 11);
            draw::draw_polygon(fx(1), y + 5, fx(5), y + 2, fx(5), y + 8);
        }
        Icon::Select => {
            // 文字カーソル（I）と、両側へ広げる矢じり
            draw::draw_line(x + 7, y + 2, x + 7, y + 12);
            draw::draw_line(x + 5, y + 2, x + 9, y + 2);
            draw::draw_line(x + 5, y + 12, x + 9, y + 12);
            draw::draw_polygon(x, y + 7, x + 3, y + 4, x + 3, y + 10);
            draw::draw_polygon(x + 14, y + 7, x + 11, y + 4, x + 11, y + 10);
        }
        Icon::Copy => {
            draw::draw_rect(x + 1, y + 1, 8, 9);
            draw::draw_rect(x + 5, y + 4, 8, 9);
        }
        Icon::Paste => {
            draw::draw_rect(x + 2, y + 2, 10, 11);
            draw::draw_rectf(x + 5, y + 1, 4, 3);
            draw::draw_line(x + 4, y + 7, x + 9, y + 7);
            draw::draw_line(x + 4, y + 10, x + 9, y + 10);
        }
        Icon::Run => draw::draw_polygon(x + 3, y + 1, x + 3, y + 13, x + 12, y + 7),
        Icon::Pin => {
            // 押しピン（頭・胴・針）
            draw::draw_rectf(x + 4, y + 1, 6, 2);
            draw::draw_rectf(x + 5, y + 3, 4, 4);
            draw::draw_rectf(x + 3, y + 7, 8, 2);
            draw::draw_line(x + 7, y + 9, x + 7, y + 13);
        }
        Icon::Flag => {
            // 竿と、日の丸の旗
            draw::draw_line(x + 1, y + 1, x + 1, y + 13);
            draw::draw_rect(x + 2, y + 2, 11, 8);
            draw::draw_pie(x + 5, y + 4, 5, 5, 0.0, 360.0);
        }
        Icon::Moon => {
            // 三日月（外の弧と、右上へずらした内の弧）
            draw::draw_arc(x + 1, y + 1, 12, 12, 70.0, 340.0);
            draw::draw_arc(x + 4, y - 1, 11, 11, 150.0, 290.0);
        }
        Icon::Eye => {
            draw::draw_arc(x, y + 3, 14, 8, 0.0, 360.0);
            draw::draw_pie(x + 5, y + 5, 4, 4, 0.0, 360.0);
        }
    }
    draw::set_line_style(LineStyle::Solid, 0);
}
