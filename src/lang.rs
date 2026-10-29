// 画面の言葉（日本語・英語）。文字列は使う所に両方を書き、tr で今の言語の方を選ぶ。
// 言語を足すときは Lang と ALL に 1 行足し、tr の呼び出しに引数を足す。
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ja,
    En,
}

/// (言語, config.txt の値, 一覧に出す名前)
pub const ALL: &[(Lang, &str, &str)] = &[(Lang::Ja, "ja", "日本語"), (Lang::En, "en", "English")];

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(l: Lang) {
    CURRENT.store(l as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::En as u8 {
        Lang::En
    } else {
        Lang::Ja
    }
}

pub fn is_en() -> bool {
    current() == Lang::En
}

/// 今の言語の方を返す。
pub fn tr(ja: &'static str, en: &'static str) -> &'static str {
    if is_en() {
        en
    } else {
        ja
    }
}

pub fn code(l: Lang) -> &'static str {
    ALL.iter().find(|a| a.0 == l).map_or("ja", |a| a.1)
}

pub fn from_code(s: &str) -> Option<Lang> {
    ALL.iter().find(|a| a.1.eq_ignore_ascii_case(s.trim())).map(|a| a.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trip() {
        for (l, c, _) in ALL {
            assert_eq!(from_code(c), Some(*l));
            assert_eq!(code(*l), *c);
        }
        assert_eq!(from_code("fr"), None);
    }
}
