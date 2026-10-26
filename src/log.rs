// Log.java 互換の極小ロガー。INFO/DEBUG は `[Tag] msg`（stdout）、
// WARN/ERROR は `[WARN] [Tag] msg` / `[ERROR] [Tag] msg`（stderr）。
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

static MIN_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

fn parse(v: &str) -> Level {
    match v.trim().to_ascii_uppercase().as_str() {
        "DEBUG" => Level::Debug,
        "WARN" => Level::Warn,
        "ERROR" => Level::Error,
        _ => Level::Info,
    }
}

/// `mod/log.properties` の `log.level=` を読む（無ければ None）。
fn read_properties() -> Option<Level> {
    let text = std::fs::read_to_string("mod/log.properties").ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "log.level").then(|| parse(v))
    })
}

/// 起動直後に一度だけ呼ぶ。決定順: 起動引数 → mod/log.properties → NEO_LOG_LEVEL → INFO
pub fn init(cli_level: Option<&str>) {
    let level = cli_level
        .map(parse)
        .or_else(read_properties)
        .or_else(|| std::env::var("NEO_LOG_LEVEL").ok().map(|v| parse(&v)))
        .unwrap_or(Level::Info);
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// ログ表示専用: パスや URL から末尾のファイル名だけを取り出す（`/` と `\` の両方）。
pub fn file_name(path_or_url: &str) -> &str {
    match path_or_url.rfind(['/', '\\']) {
        Some(i) => &path_or_url[i + 1..],
        None => path_or_url,
    }
}

pub fn log(level: Level, tag: &str, msg: &str) {
    if (level as u8) < MIN_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let prefix = if tag.is_empty() { String::new() } else { format!("[{tag}] ") };
    match level {
        Level::Debug | Level::Info => println!("{prefix}{msg}"),
        Level::Warn => eprintln!("[WARN] {prefix}{msg}"),
        Level::Error => eprintln!("[ERROR] {prefix}{msg}"),
    }
}

#[allow(dead_code)] // ログ API として debug レベルも揃えておく
pub fn debug(tag: &str, msg: &str) { log(Level::Debug, tag, msg) }
pub fn info(tag: &str, msg: &str) { log(Level::Info, tag, msg) }
pub fn warn(tag: &str, msg: &str) { log(Level::Warn, tag, msg) }
pub fn error(tag: &str, msg: &str) { log(Level::Error, tag, msg) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_handles_both_separators() {
        assert_eq!(file_name(r"C:\Users\x\foo.json"), "foo.json");
        assert_eq!(file_name("/home/x/foo.md"), "foo.md");
        assert_eq!(file_name("plain.txt"), "plain.txt");
    }

    #[test]
    fn parse_is_case_insensitive_and_defaults_to_info() {
        assert_eq!(parse(" debug "), Level::Debug);
        assert_eq!(parse("nonsense"), Level::Info);
    }
}
