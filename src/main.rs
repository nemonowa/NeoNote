// NeoNote: 設計は docs/DESIGN.md を参照。
// Windows ではダブルクリックで黒い窓が出ないよう GUI アプリとして作る（端末から起動したときは attach_parent_console でその端末へ出力する）。
#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod audioplay;
mod neo;
mod dev;
mod lang;
mod log;
mod platform;
mod ui;

/// `neonote [--log-level <LEVEL>] [--] [パス...]`（`-log-level` も受ける）。
/// パスは全てタブで開く。`--` より後ろは、`-` で始まっていてもパスとして扱う。
fn parse_args(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut level = None;
    let mut paths = Vec::new();
    let mut only_paths = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if only_paths {
            paths.push(a.to_string());
        } else if a == "--" {
            only_paths = true;
        } else if (a == "--log-level" || a == "-log-level") && i + 1 < args.len() {
            level = Some(args[i + 1].clone());
            i += 1;
        } else if a.starts_with('-') && a.len() > 1 {
            // 未知のオプションは無視
        } else {
            paths.push(a.to_string());
        }
        i += 1;
    }
    (level, paths)
}

/// 端末から起動されたときだけ、その端末にログを出す。ダブルクリックでは親の端末が無いので何もしない。
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() {
    #[cfg(windows)]
    attach_parent_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (level, paths) = parse_args(&args);
    log::init(level.as_deref());
    if let Err(e) = app::run(paths) {
        log::error("NeoNote", &format!("fatal: {e}"));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn both_dash_styles_and_all_paths() {
        assert_eq!(parse_args(&v(&["-log-level", "debug", "a.txt", "b.txt"])),
                   (Some("debug".into()), v(&["a.txt", "b.txt"])));
        assert_eq!(parse_args(&v(&["--log-level", "warn"])), (Some("warn".into()), v(&[])));
        assert_eq!(parse_args(&v(&[])), (None, v(&[])));
    }

    #[test]
    fn unknown_options_are_ignored_and_double_dash_ends_options() {
        assert_eq!(parse_args(&v(&["--bogus", "a.txt"])), (None, v(&["a.txt"])));
        assert_eq!(parse_args(&v(&["--", "-weird.txt", "--log-level"])), (None, v(&["-weird.txt", "--log-level"])));
    }
}
