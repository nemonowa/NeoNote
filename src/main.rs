// NeoNote: 設計は docs/DESIGN.md を参照。
// Windows ではダブルクリックで黒い窓が出ないよう GUI アプリとして作る（端末から起動したときは attach_parent_console でその端末へ出力する）。
#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod audioplay;
mod neo;
mod dev;
mod jumplist;
mod lang;
mod log;
mod platform;
mod ui;

/// 起動の引数。
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    level: Option<String>,
    paths: Vec<String>,
    /// 起動中のウィンドウがあっても、新しいウィンドウで開く（`--new-window` / `-n`）。
    new_window: bool,
}

/// `neonote [--log-level <LEVEL>] [--new-window] [--] [パス...]`（`-log-level` も受ける）。
/// パスは全てタブで開く。`--` より後ろは、`-` で始まっていてもパスとして扱う。
fn parse_args(args: &[String]) -> Args {
    let mut out = Args::default();
    let mut only_paths = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if only_paths {
            out.paths.push(a.to_string());
        } else if a == "--" {
            only_paths = true;
        } else if (a == "--log-level" || a == "-log-level") && i + 1 < args.len() {
            out.level = Some(args[i + 1].clone());
            i += 1;
        } else if a == "--new-window" || a == "-new-window" || a == "-n" {
            out.new_window = true;
        } else if a.starts_with('-') && a.len() > 1 {
            // 未知のオプションは無視
        } else {
            out.paths.push(a.to_string());
        }
        i += 1;
    }
    out
}

/// すでに起動しているウィンドウがあれば、そこへ頼んで（パスをタブで開かせる。パスが無ければ前に出す）true を返す。
/// 頼めなければ（起動中のウィンドウが無い・応答しない）false: 自分が新しいウィンドウになる。
/// 最後に操作されたウィンドウから順に試す。
fn forward_to_running(paths: &[String]) -> bool {
    let Some(dir) = neo::config::path().and_then(|p| p.parent().map(std::path::Path::to_path_buf)) else { return false };
    neo::ipc::acquire_start_lock(&dir); // 自分が窓になるときは、待ち受けを始めた所で返す（app::start_ipc）
    let peers = neo::ipc::peers(&dir, std::process::id());
    if peers.is_empty() {
        return false;
    }
    platform::allow_foreground_window(); // 渡した先が、前に出られるようにする（Windows）
    let abs: Vec<std::path::PathBuf> = paths.iter().map(|p| neo::ipc::absolute(p)).collect();
    for peer in peers {
        let r = if abs.is_empty() { peer.raise() } else { peer.open(&abs) };
        match r {
            Ok(()) => {
                neo::ipc::release_start_lock();
                log::info("NeoNote", &format!("forwarded to the running window (pid {})", peer.pid));
                return true;
            }
            Err(e) => log::warn("NeoNote", &format!("forward failed (pid {}): {e}", peer.pid)),
        }
    }
    false
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
    let Args { level, paths, new_window } = parse_args(&args);
    log::init(level.as_deref());
    if !new_window && forward_to_running(&paths) {
        return; // 起動中のウィンドウが開いた（ウィンドウは増やさない）
    }
    if let Err(e) = app::run(paths, new_window) {
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
        let args = |level: Option<&str>, paths: &[&str], new_window| Args { level: level.map(Into::into), paths: v(paths), new_window };
        assert_eq!(parse_args(&v(&["-log-level", "debug", "a.txt", "b.txt"])), args(Some("debug"), &["a.txt", "b.txt"], false));
        assert_eq!(parse_args(&v(&["--log-level", "warn"])), args(Some("warn"), &[], false));
        assert_eq!(parse_args(&v(&[])), Args::default());
    }

    #[test]
    fn unknown_options_are_ignored_and_double_dash_ends_options() {
        assert_eq!(parse_args(&v(&["--bogus", "a.txt"])), Args { paths: v(&["a.txt"]), ..Args::default() });
        assert_eq!(parse_args(&v(&["--", "-weird.txt", "--log-level"])), Args { paths: v(&["-weird.txt", "--log-level"]), ..Args::default() });
    }

    #[test]
    fn new_window_flag_is_recognised_only_before_double_dash() {
        assert!(parse_args(&v(&["--new-window", "a.txt"])).new_window);
        assert!(parse_args(&v(&["-n"])).new_window);
        assert!(!parse_args(&v(&["--", "-n"])).new_window);
        assert_eq!(parse_args(&v(&["--", "-n"])).paths, v(&["-n"]));
    }
}
