// 配色。基本の 5 色（main / sub / accent / text / subtext）を 1 組のパレットにして、ダーク・ライトと見比べ用の候補を持つ（PALETTES）。
// 画面の色は、必ずここの関数（color_*）から取る。色を直したいときはこのファイルの PALETTES だけを書き換える。
// FLTK の標準部品（スクロールバー・ダイアログ）も skin.rs がここの色で塗る。
//
//   main     編集領域・ボタン・選択中のタブの背景（いちばん広い面）
//   sub      ツールバー・タブバー・行番号欄の背景（main より一段奥）
//   accent   選択範囲・カーソル・押したボタン・固定ボタン・強調の帯
//   text     本文と、選ばれているものの文字
//   subtext  行番号・ツールバーの文字・選ばれていないタブの文字（控えめな文字）
//
// 上の 5 色から決まる色（タブの背景など）は、下の関数で混ぜて作る（色を足さない）。
// 検索のハイライト（黄色の地に黒）は、どちらのモードでも読めるので固定。
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

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

/// ダークの既定「濡羽色」（#000B00 を地に、緑の文字とアクセント）。
pub const DARK: Palette = Palette {
    main: (0, 11, 0),
    sub: (10, 32, 16), // 地が黒に近く一段暗くできないので、バーは一段明るい緑にする
    accent: (40, 230, 120),
    text: (140, 228, 150),
    subtext: (72, 150, 90),
};

/// ライトの既定「月白色」（#EAF4FC を地に、紺の文字と青のアクセント）。
pub const LIGHT: Palette = Palette {
    main: (234, 244, 252),
    sub: (214, 229, 242),
    accent: (30, 100, 190),
    text: (22, 34, 52),
    subtext: (84, 102, 126),
};

/// 以前のダーク（SwingNeoNote 由来の配色。本文の文字だけ、subtext から text に変えた）。
pub const CLASSIC_DARK: Palette = Palette {
    main: (43, 43, 43),
    sub: (30, 30, 30),
    accent: (0, 120, 215),
    text: (200, 200, 200),
    subtext: (128, 128, 128),
};

/// 以前のライト。
pub const CLASSIC_LIGHT: Palette = Palette {
    main: (250, 250, 250),
    sub: (232, 232, 232),
    accent: (0, 120, 215),
    text: (32, 32, 32),
    subtext: (105, 105, 105),
};

/// 名前の付いたパレット。`light` は明るい地か（エラーの色・Toggle Theme の行き先を決める）。
pub struct Named {
    pub name: &'static str,
    pub palette: Palette,
    pub light: bool,
}

/// 使えるパレット。先頭の 2 つがダークとライト（Toggle Theme はこの 2 つを行き来する）。
/// 3 つ目からは見比べ用の候補（起動時に環境変数 NEONOTE_THEME=名前 で選べる）。
pub const PALETTES: &[Named] = &[
    Named { name: "nureba", palette: DARK, light: false },
    Named { name: "geppaku", palette: LIGHT, light: true },
    Named { name: "classic_dark", palette: CLASSIC_DARK, light: false },
    Named { name: "classic_light", palette: CLASSIC_LIGHT, light: true },
    Named { name: "nord", palette: Palette { main: (38, 42, 51), sub: (29, 32, 40), accent: (94, 129, 172), text: (229, 233, 240), subtext: (140, 150, 170) }, light: false },
    Named { name: "abyss", palette: Palette { main: (22, 27, 34), sub: (13, 17, 23), accent: (0, 122, 112), text: (214, 222, 230), subtext: (125, 135, 148) }, light: false },
    Named { name: "paper", palette: Palette { main: (247, 241, 227), sub: (233, 224, 204), accent: (166, 90, 30), text: (52, 44, 36), subtext: (110, 96, 80) }, light: true },
    Named { name: "sakura", palette: Palette { main: (253, 246, 248), sub: (242, 226, 232), accent: (196, 64, 110), text: (48, 34, 40), subtext: (120, 96, 106) }, light: true },
    Named { name: "discord", palette: Palette { main: (43, 45, 49), sub: (30, 31, 34), accent: (88, 101, 242), text: (219, 222, 225), subtext: (148, 155, 164) }, light: false },
    Named { name: "github", palette: Palette { main: (13, 17, 23), sub: (1, 4, 9), accent: (31, 111, 235), text: (230, 237, 243), subtext: (125, 133, 144) }, light: false },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// 自分で決めた配色（config.txt の custom_main などから作る）。CURRENT がこの番号のときに使う。
static CUSTOM: Mutex<Option<Palette>> = Mutex::new(None);
const CUSTOM_INDEX: usize = usize::MAX;
pub const CUSTOM_NAME: &str = "custom";

/// 自分で決めた配色を覚える（選ぶのは set_named("custom")）。
pub fn store_custom(p: Palette) {
    *CUSTOM.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

pub fn custom() -> Option<Palette> {
    *CUSTOM.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_light(on: bool) {
    CURRENT.store(usize::from(on), Ordering::Relaxed);
}

/// 名前でパレットを選ぶ（大文字小文字は区別しない）。知らない名前なら false。
pub fn set_named(name: &str) -> bool {
    if name.trim().eq_ignore_ascii_case(CUSTOM_NAME) {
        if custom().is_none() {
            return false;
        }
        CURRENT.store(CUSTOM_INDEX, Ordering::Relaxed);
        return true;
    }
    match PALETTES.iter().position(|p| p.name.eq_ignore_ascii_case(name.trim())) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn current_name() -> &'static str {
    PALETTES.get(CURRENT.load(Ordering::Relaxed)).map_or(CUSTOM_NAME, |p| p.name)
}

/// 明るい地か。自分で決めた配色は、地が文字より明るいかで決める。
pub fn is_light() -> bool {
    match PALETTES.get(CURRENT.load(Ordering::Relaxed)) {
        Some(p) => p.light,
        None => {
            let p = palette();
            let sum = |c: Rgb| c.0 as u32 + c.1 as u32 + c.2 as u32;
            sum(p.main) > sum(p.text)
        }
    }
}

pub fn palette() -> Palette {
    match PALETTES.get(CURRENT.load(Ordering::Relaxed)) {
        Some(p) => p.palette,
        None => custom().unwrap_or(DARK),
    }
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

/// accent の上に載せる文字（押したボタン・固定ボタン）。白で読めない明るい accent のときは黒。
pub fn color_on_accent() -> Rgb {
    on_accent(palette().accent)
}

fn on_accent((r, g, b): Rgb) -> Rgb {
    if r as u32 * 299 + g as u32 * 587 + b as u32 * 114 > 150_000 { (0, 0, 0) } else { (255, 255, 255) }
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
    fn all_palettes_are_readable() {
        for n in PALETTES {
            let (name, p) = (n.name, n.palette);
            assert!(contrast(p.text, p.main) >= 7.0, "{name}: text on main");
            assert!(contrast(p.text, p.sub) >= 7.0, "{name}: text on sub");
            assert!(contrast(p.subtext, p.main) >= 3.5, "{name}: subtext on main");
            assert!(contrast(p.subtext, p.sub) >= 3.5, "{name}: subtext on sub");
            assert!(contrast(on_accent(p.accent), p.accent) >= 4.0, "{name}: on accent");
            let err = if n.light { ERROR_LIGHT } else { ERROR_DARK };
            assert!(contrast(err, p.main) >= 4.5, "{name}: error on main");
            let sum = |c: Rgb| c.0 as u32 + c.1 as u32 + c.2 as u32;
            assert_eq!(n.light, sum(p.main) > sum(p.text), "{name}: light flag");
            // 濡羽は地が黒に近いので、バーを一段明るくして分ける
            assert!(sum(p.sub) < sum(p.main) || n.name == "nureba", "{name}: sub is one step back");
        }
    }

    #[test]
    fn dark_is_dark_and_light_is_light() {
        let sum = |c: Rgb| c.0 as u32 + c.1 as u32 + c.2 as u32;
        assert!(sum(DARK.main) < sum(DARK.text));
        assert!(sum(LIGHT.main) > sum(LIGHT.text));
        // sub は main より一段奥（ダークは暗く、ライトも暗く）
        assert!(sum(DARK.sub) != sum(DARK.main));
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
        assert!(set_named("classic_dark"));
        assert_eq!(color_tab_inactive(), (36, 36, 36));
    }
}
