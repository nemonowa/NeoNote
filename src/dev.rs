// 開発者判定。アカウント名が開発者名（大文字小文字無視）ならバージョン表示を差し替える。
// 判定条件は設計書以外の配布物には記載しない。
const DEV_USER_NAME: &str = "nemo";

pub const VERSION: &str = "v1.1.47";
pub const DEV_VERSION: &str = "v999.99.9";

pub fn is_dev_user() -> bool {
    ["USER", "USERNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .any(|v| v.eq_ignore_ascii_case(DEV_USER_NAME))
}

pub fn version() -> &'static str {
    if is_dev_user() { DEV_VERSION } else { VERSION }
}
