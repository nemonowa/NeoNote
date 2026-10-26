// 配色。基本の 5 色（main / sub / accent / text / subtext）を 1 組のパレットにして、ダークとライトを持つ。
// 画面の色は、必ずここの関数（color_*）から取る。色を直したいときはこのファイルの DARK / LIGHT だけを書き換える。
//
//   main     編集領域・ボタン・選択中のタブの背景（いちばん広い面）
//   sub      ツールバー・タブバー・行番号欄の背景（main より一段奥）
//   accent   選択範囲・カーソル・押したボタン・固定ボタン・強調の帯
//   text     本文と、選ばれているものの文字
//   subtext  行番号・ツールバーの文字・選ばれていないタブの文字（控えめな文字）
//
// 上の 5 色から決まる色（タブの背景など）は、下の関数で混ぜて作る（色を足さない）。
// 検索のハイライト（黄色の地に黒）は、どちらのモードでも読めるので固定。
use std::sync::atomic::{AtomicBool, Ordering};

/// (R, G, B)
pub type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub main: Rgb,
    pub sub: Rgb,
    pub accent: Rgb,
    pub text: Rgb,
    pub subtext: Rgb,
}

/// ダーク（SwingNeoNote 由来の配色。本文の文字だけ、subtext から text に変えた）。
pub const DARK: Palette = Palette {
    main: (43, 43, 43),
    sub: (30, 30, 30),
    accent: (0, 120, 215),
    text: (200, 200, 200),
    subtext: (128, 128, 128),
};

/// ライト。
pub const LIGHT: Palette = Palette {
    main: (250, 250, 250),
    sub: (232, 232, 232),
    accent: (0, 120, 215),
    text: (32, 32, 32),
    subtext: (105, 105, 105),
};

static LIGHT_ON: AtomicBool = AtomicBool::new(false);

pub fn set_light(on: bool) {
    LIGHT_ON.store(on, Ordering::Relaxed);
}

pub fn is_light() -> bool {
    LIGHT_ON.load(Ordering::Relaxed)
}

pub fn palette() -> Palette {
    if is_light() { LIGHT } else { DARK }
}

pub fn color_main() -> Rgb {
    palette().main
}
pub fn color_sub() -> Rgb {
    palette().sub
}
pub fn color_accent() -> Rgb {
    palette().accent
}
pub fn color_text() -> Rgb {
    palette().text
}
pub fn color_subtext() -> Rgb {
    palette().subtext
}

/// `a` に `b` を `percent`%（0〜100）混ぜた色。
pub fn mix(a: Rgb, b: Rgb, percent: u32) -> Rgb {
    let m = |x: u8, y: u8| ((x as u32 * (100 - percent) + y as u32 * percent) / 100) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// 選ばれていないタブの背景（sub と main の中間）。
pub fn color_tab_inactive() -> Rgb {
    let p = palette();
    mix(p.sub, p.main, 50)
}

/// 一覧で選んでいる行の背景（main に text を少し混ぜる）。
pub fn color_selected_row() -> Rgb {
    let p = palette();
    mix(p.main, p.text, 20)
}

/// accent の上に載せる文字（押したボタン・固定ボタン）。
pub fn color_on_accent() -> Rgb {
    (255, 255, 255)
}

/// 「見つからない」など、うまくいかなかったことを示す文字色（検索バーの入力欄）。
/// 場所を取らずに結果を伝えるための色なので、どちらのモードでも main の上で読める赤にする。
pub fn color_error() -> Rgb {
    if is_light() { ERROR_LIGHT } else { ERROR_DARK }
}

const ERROR_DARK: Rgb = (240, 110, 110);
const ERROR_LIGHT: Rgb = (190, 30, 30);

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG のコントラスト比。
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let lum = |(r, g, b): Rgb| {
            let f = |c: u8| {
                let c = c as f64 / 255.0;
                if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
        };
        let (la, lb) = (lum(a), lum(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn both_palettes_are_readable() {
        for (name, p) in [("dark", DARK), ("light", LIGHT)] {
            assert!(contrast(p.text, p.main) >= 7.0, "{name}: text on main");
            assert!(contrast(p.text, p.sub) >= 7.0, "{name}: text on sub");
            assert!(contrast(p.subtext, p.main) >= 3.5, "{name}: subtext on main");
            assert!(contrast(p.subtext, p.sub) >= 3.5, "{name}: subtext on sub");
            assert!(contrast(color_on_accent(), p.accent) >= 4.0, "{name}: on accent");
        }
        for (name, p, err) in [("dark", DARK, ERROR_DARK), ("light", LIGHT, ERROR_LIGHT)] {
            assert!(contrast(err, p.main) >= 4.5, "{name}: error on main");
        }
    }

    #[test]
    fn dark_is_dark_and_light_is_light() {
        let sum = |c: Rgb| c.0 as u32 + c.1 as u32 + c.2 as u32;
        assert!(sum(DARK.main) < sum(DARK.text));
        assert!(sum(LIGHT.main) > sum(LIGHT.text));
        // sub は main より一段奥（ダークは暗く、ライトも暗く）
        assert!(sum(DARK.sub) < sum(DARK.main));
        assert!(sum(LIGHT.sub) < sum(LIGHT.main));
    }

    #[test]
    fn mix_is_between_and_exact_at_ends() {
        assert_eq!(mix((0, 0, 0), (100, 200, 50), 0), (0, 0, 0));
        assert_eq!(mix((0, 0, 0), (100, 200, 50), 100), (100, 200, 50));
        assert_eq!(mix((0, 0, 0), (100, 200, 50), 50), (50, 100, 25));
    }

    #[test]
    fn derived_colors_match_the_old_dark_look() {
        set_light(false);
        assert_eq!(color_tab_inactive(), (36, 36, 36));
        assert_eq!(color_selected_row(), (74, 74, 74)); // 以前は 75（誤差 1）
    }
}
