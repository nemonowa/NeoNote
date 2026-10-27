// 文字サイズの上限・下限と 1 段の増減。GUI 非依存。
// Ctrl+ホイール / Ctrl++ / Ctrl+- で 1pt ずつ、Ctrl+0 で既定に戻す。

pub const DEFAULT: i32 = 14;
pub const MIN: i32 = 8;
pub const MAX: i32 = 48;

pub fn clamp(n: i32) -> i32 {
    n.clamp(MIN, MAX)
}

/// 今のサイズに `delta` を足して範囲に収める（テスト用。画面側は `clamp` を直接使う）。
#[cfg(test)]
pub fn step(cur: i32, delta: i32) -> i32 {
    clamp(cur.saturating_add(delta))
}

/// 行番号の文字サイズ（本文より 2 小さく、下限あり）。
pub fn linenumber_size(size: i32) -> i32 {
    (size - 2).max(MIN)
}

/// 行番号欄の幅。既定のサイズで 48 になるよう、サイズに比例させる。
pub fn linenumber_width(size: i32) -> i32 {
    48 * size / DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_clamps_at_both_ends() {
        assert_eq!(step(14, 1), 15);
        assert_eq!(step(14, -1), 13);
        assert_eq!(step(MAX, 1), MAX);
        assert_eq!(step(MIN, -1), MIN);
        assert_eq!(step(i32::MAX, 1), MAX);
    }

    #[test]
    fn clamp_fixes_out_of_range_config_values() {
        assert_eq!(clamp(0), MIN);
        assert_eq!(clamp(-5), MIN);
        assert_eq!(clamp(999), MAX);
    }

    #[test]
    fn linenumber_metrics_match_the_original_at_default() {
        assert_eq!(linenumber_size(DEFAULT), 12);
        assert_eq!(linenumber_width(DEFAULT), 48);
        assert_eq!(linenumber_size(MIN), MIN);
        assert!(linenumber_width(MAX) > linenumber_width(DEFAULT));
    }
}
