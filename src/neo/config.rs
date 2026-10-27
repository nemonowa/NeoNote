// 状態の保存先: 実行ファイルと同じ階層の `neodir/config.txt`（普通のテキスト）。
// NeoNote 自身で開いて編集でき、保存すると反映される（Load の右クリック → Open Config）。
//
// 形式は `キー=値` を 1 行ずつ。`#` で始まる行と空行は無視する。
//   autosave=true                 Enable Auto Save の状態
//   wrap=true                     Word Wrap（編集領域を右端で折り返す）の状態
//   size=16                       文字サイズ（空なら既定の 14。範囲は zoom::MIN〜MAX）
//   font=C:\Fonts\Mono.ttf        選んだフォント（空なら同梱の NeoFont）
//   recent=C:\a\b.txt             最近開いたファイル（新しい順に 1 行ずつ。最大 recent::MAX 件）
//   gutter_pin=true               行番号欄を常に表示（固定）する。false なら、カーソルが左端のときだけ出す
//   side_pin=true                 右端の情報・縮小表示の欄を常に表示（固定）する。false なら、マウスが右端のときだけ出す
//   theme=dark                    配色。dark（既定）・light・nord・abyss・paper・sakura・custom
//   custom_main=#2b2b2b           theme=custom の色（custom_main・custom_sub・custom_accent・custom_text・custom_subtext）
//   history=C:\a\b.txt            過去に読み込んだファイルのパス履歴（新しい順に 1 行ずつ。最大 pathlog::MAX 件）
//   session=C:\a\b.txt            終了時に開いていたタブ（左から順に 1 行ずつ。最大 SESSION_MAX 件）。引数なしで起動したとき開き直す
//   session_active=C:\a\b.txt     終了時に表示していたタブ（session のどれか）
// アプリが書き戻すときは、キーの行だけを書き換え、コメントや知らない行はそのまま残す。
use super::{document, pathlog, recent, zoom};
use std::{
    io,
    path::{Path, PathBuf},
};

/// 実行ファイル（Linux / macOS では `neonote`）と同名にならないよう、フォルダ名は `neodir`。
pub const DIR_NAME: &str = "neodir";
pub const FILE_NAME: &str = "config.txt";
/// 覚えておくタブの最大数（読み込みと書き出しの両方で切る）。
pub const SESSION_MAX: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub autosave: bool,
    /// 編集領域を右端で折り返す（既定は折り返す）。
    pub wrap: bool,
    /// 行番号欄を常に表示する（固定）。false なら、カーソルが左端にあるときだけ出す。
    pub gutter_pin: bool,
    /// 右端の情報・縮小表示の欄を常に表示する（固定）。
    pub side_pin: bool,
    /// ライトモード（false ならダーク）。theme に知らない名前が書かれていたときの行き先。
    pub light: bool,
    /// theme= に書かれた配色の名前（無ければ light / dark だけで決める）。
    pub theme: Option<String>,
    /// theme=custom の色（main, sub, accent, text, subtext）。5 つそろったときだけ Some。
    pub custom: Option<[(u8, u8, u8); 5]>,
    /// None なら既定（zoom::DEFAULT）。
    pub size: Option<i32>,
    /// None なら同梱の NeoFont。
    pub font: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    /// 過去に読み込んだファイルのパス履歴（View History）。
    pub history: Vec<PathBuf>,
    /// 終了時に開いていたタブのファイル（左から順）。次に引数なしで起動したとき開き直す。
    pub session: Vec<PathBuf>,
    /// 終了時に表示していたタブ。
    pub session_active: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            autosave: false,
            wrap: true,
            gutter_pin: false,
            side_pin: false,
            light: false,
            theme: Some("dark".into()),
            custom: None,
            size: None,
            font: None,
            recent: Vec::new(),
            history: Vec::new(),
            session: Vec::new(),
            session_active: None,
        }
    }
}

/// 実行ファイルの隣の `neodir/config.txt`。
pub fn path_for_exe(exe: &Path) -> Option<PathBuf> {
    Some(exe.parent()?.join(DIR_NAME).join(FILE_NAME))
}

/// この実行ファイルの設定ファイルの場所。
pub fn path() -> Option<PathBuf> {
    path_for_exe(&std::env::current_exe().ok()?)
}

fn parse_bool(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "on" | "yes" | "1")
}

/// custom_* のキーの並び（Config::custom の順）。
pub const CUSTOM_KEYS: [&str; 5] = ["custom_main", "custom_sub", "custom_accent", "custom_text", "custom_subtext"];

/// `#rrggbb`（# は省略可）を色にする。
pub fn parse_hex(v: &str) -> Option<(u8, u8, u8)> {
    let h = v.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

pub fn hex_of((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn parse(text: &str) -> Config {
    let mut cfg = Config::default();
    let mut custom: [Option<(u8, u8, u8)>; 5] = [None; 5];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "autosave" => cfg.autosave = parse_bool(v),
            "wrap" => cfg.wrap = parse_bool(v),
            "gutter_pin" => cfg.gutter_pin = parse_bool(v),
            "side_pin" => cfg.side_pin = parse_bool(v),
            "theme" => {
                cfg.light = v.eq_ignore_ascii_case("light");
                cfg.theme = (!v.is_empty()).then(|| v.to_ascii_lowercase());
            }
            k if CUSTOM_KEYS.contains(&k) => {
                let i = CUSTOM_KEYS.iter().position(|c| *c == k).unwrap_or(0);
                custom[i] = parse_hex(v);
            }
            "size" => cfg.size = v.parse::<i32>().ok().map(zoom::clamp),
            "font" if !v.is_empty() => cfg.font = Some(PathBuf::from(v)),
            "recent" if !v.is_empty() && cfg.recent.len() < recent::MAX => cfg.recent.push(PathBuf::from(v)),
            "history" if !v.is_empty() && cfg.history.len() < pathlog::MAX => cfg.history.push(PathBuf::from(v)),
            "session" if !v.is_empty() && cfg.session.len() < SESSION_MAX => {
                let p = PathBuf::from(v);
                if !cfg.session.contains(&p) {
                    cfg.session.push(p);
                }
            }
            "session_active" if !v.is_empty() => cfg.session_active = Some(PathBuf::from(v)),
            _ => {}
        }
    }
    if custom.iter().all(Option::is_some) {
        cfg.custom = Some(custom.map(|c| c.unwrap_or_default()));
    }
    cfg
}

const HEADER: &str = "# NeoNote の設定。NeoNote で開いて編集し、保存すると反映されます。\n\
# autosave=true|false   wrap=true|false   gutter_pin=true|false   theme=dark|light   size=文字サイズ（空なら既定）   font=フォントファイルのパス（空なら既定）   recent=最近開いたファイル（新しい順）   session=終了時のタブ（自動で書かれます）\n";

/// 1 行に収まらない値は書かない（行が壊れるので）。
fn value(p: &Path) -> Option<String> {
    let s = p.to_str()?;
    (!s.contains(['\n', '\r'])).then(|| s.to_string())
}

/// `existing`（今のファイルの中身）のうち、キーの行だけを `cfg` で置き換えた全文を返す。
/// コメント・空行・知らないキーはそのまま。無いキーは末尾に足す。
pub fn render(existing: &str, cfg: &Config) -> String {
    let recent_lines: Vec<String> = cfg.recent.iter().take(recent::MAX).filter_map(|p| value(p)).map(|s| format!("recent={s}")).collect();
    let history_lines: Vec<String> = cfg.history.iter().take(pathlog::MAX).filter_map(|p| value(p)).map(|s| format!("history={s}")).collect();
    let session_lines: Vec<String> = cfg.session.iter().take(SESSION_MAX).filter_map(|p| value(p)).map(|s| format!("session={s}")).collect();
    let session_active_line = cfg.session_active.as_deref().and_then(value).map(|s| format!("session_active={s}"));
    let font_line = format!("font={}", cfg.font.as_deref().and_then(value).unwrap_or_default());
    let auto_line = format!("autosave={}", cfg.autosave);
    let wrap_line = format!("wrap={}", cfg.wrap);
    let pin_line = format!("gutter_pin={}", cfg.gutter_pin);
    let side_line = format!("side_pin={}", cfg.side_pin);
    let mut seen_side = false;
    // dark / light は light の値で決める（名前の付いた配色のときだけ、その名前を書く）
    let named = cfg.theme.as_deref().filter(|t| !matches!(*t, "dark" | "light"));
    let theme_line = format!("theme={}", named.unwrap_or(if cfg.light { "light" } else { "dark" }));
    let custom_lines: Vec<String> = cfg.custom.iter().flat_map(|c| CUSTOM_KEYS.iter().zip(c.iter()).map(|(k, v)| format!("{k}={}", hex_of(*v)))).collect();
    let mut seen_custom = false;
    let size_line = format!("size={}", cfg.size.map(|n| n.to_string()).unwrap_or_default());

    let mut out: Vec<String> = Vec::new();
    let (mut seen_auto, mut seen_font, mut seen_recent, mut seen_history) = (false, false, false, false);
    let (mut seen_wrap, mut seen_size, mut seen_pin, mut seen_theme) = (false, false, false, false);
    let (mut seen_session, mut seen_session_active) = (false, false);
    for line in existing.lines() {
        let key = if line.trim_start().starts_with('#') { None } else { line.split_once('=').map(|(k, _)| k.trim()) };
        match key {
            Some("autosave") => {
                if !seen_auto {
                    out.push(auto_line.clone());
                }
                seen_auto = true;
            }
            Some("wrap") => {
                if !seen_wrap {
                    out.push(wrap_line.clone());
                }
                seen_wrap = true;
            }
            Some("gutter_pin") => {
                if !seen_pin {
                    out.push(pin_line.clone());
                }
                seen_pin = true;
            }
            Some(k) if CUSTOM_KEYS.contains(&k) => {
                if !seen_custom {
                    out.extend(custom_lines.iter().cloned());
                }
                seen_custom = true;
            }
            Some("side_pin") => {
                if !seen_side {
                    out.push(side_line.clone());
                }
                seen_side = true;
            }
            Some("theme") => {
                if !seen_theme {
                    out.push(theme_line.clone());
                }
                seen_theme = true;
            }
            Some("size") => {
                if !seen_size {
                    out.push(size_line.clone());
                }
                seen_size = true;
            }
            Some("font") => {
                if !seen_font {
                    out.push(font_line.clone());
                }
                seen_font = true;
            }
            Some("recent") => {
                if !seen_recent {
                    out.extend(recent_lines.iter().cloned());
                }
                seen_recent = true;
            }
            Some("history") => {
                if !seen_history {
                    out.extend(history_lines.iter().cloned());
                }
                seen_history = true;
            }
            Some("session") => {
                if !seen_session {
                    out.extend(session_lines.iter().cloned());
                }
                seen_session = true;
            }
            Some("session_active") => {
                if !seen_session_active {
                    out.extend(session_active_line.iter().cloned());
                }
                seen_session_active = true;
            }
            _ => out.push(line.to_string()),
        }
    }
    if out.is_empty() {
        out.extend(HEADER.lines().map(str::to_string));
    }
    if !seen_auto {
        out.push(auto_line);
    }
    if !seen_wrap {
        out.push(wrap_line);
    }
    if !seen_size {
        out.push(size_line);
    }
    if !seen_theme {
        out.push(theme_line);
    }
    if !seen_pin {
        out.push(pin_line);
    }
    if !seen_custom {
        out.extend(custom_lines);
    }
    // 既定（false）のときは行を足さない（今までの設定ファイルを増やさない）
    if !seen_side && cfg.side_pin {
        out.push(side_line);
    }
    if !seen_font {
        out.push(font_line);
    }
    if !seen_recent {
        out.extend(recent_lines);
    }
    if !seen_history {
        out.extend(history_lines);
    }
    if !seen_session {
        out.extend(session_lines);
    }
    if !seen_session_active {
        out.extend(session_active_line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// 読めない・無いときは既定の設定（起動を止めない）。
pub fn load(path: &Path) -> Config {
    std::fs::read(path).map(|b| parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

/// 今のファイルに `cfg` を反映して書く。中身が変わらないときは書かない。
/// フォルダが無ければ作る。成功したら、書いたか（true）を返す。
pub fn save(path: &Path, cfg: &Config) -> io::Result<bool> {
    let existing = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let next = render(&existing, cfg);
    if next == existing {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    document::write_atomic(path, next.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config { autosave: true, wrap: true, gutter_pin: true, side_pin: false, light: true, theme: Some("light".into()), custom: None, size: Some(18), font: Some(PathBuf::from("/f/Mono.ttf")), recent: vec![PathBuf::from("/a/1.txt"), PathBuf::from("/a/2.txt")], history: vec![PathBuf::from("/h/1.txt"), PathBuf::from("/h/2.txt"), PathBuf::from("/h/3.txt")], session: vec![PathBuf::from("/s/1.txt"), PathBuf::from("/s/2.txt")], session_active: Some(PathBuf::from("/s/2.txt")) }
    }

    #[test]
    fn theme_defaults_dark_and_round_trips() {
        assert!(!parse("").light);
        assert!(parse("theme=light\n").light);
        assert!(parse("theme= LIGHT \n").light);
        assert!(!parse("theme=dark\n").light);
        assert!(!parse("theme=blue\n").light); // 知らない値はダーク
        let out = render("theme=dark\n# memo\n", &Config { light: true, ..Default::default() });
        assert!(out.starts_with("theme=light\n# memo\n"));
        assert!(render("", &Config { light: true, ..Default::default() }).contains("theme=light\n"));
        assert!(render("", &Config::default()).contains("theme=dark\n"));
    }

    #[test]
    fn wrap_defaults_on_and_round_trips() {
        assert!(parse("").wrap);
        assert!(Config::default().wrap);
        assert!(parse("wrap=true\n").wrap);
        assert!(!parse("wrap=false\n").wrap);
        let out = render("wrap=true\n", &Config { wrap: false, ..Default::default() });
        assert!(out.starts_with("wrap=false\n"));
    }

    #[test]
    fn side_pin_is_written_only_when_on_or_present() {
        assert!(!parse("").side_pin);
        assert!(parse("side_pin=true\n").side_pin);
        assert!(!render("", &Config::default()).contains("side_pin"));
        assert!(render("", &Config { side_pin: true, ..Default::default() }).contains("side_pin=true\n"));
        assert!(render("side_pin=true\n", &Config::default()).starts_with("side_pin=false\n"));
    }

    #[test]
    fn gutter_pin_defaults_off_and_round_trips() {
        assert!(!parse("").gutter_pin);
        assert!(parse("gutter_pin=true\n").gutter_pin);
        assert!(!parse("gutter_pin=false\n").gutter_pin);
        let out = render("gutter_pin=false\n", &Config { gutter_pin: true, ..Default::default() });
        assert!(out.starts_with("gutter_pin=true\n"));
    }

    #[test]
    fn size_is_clamped_and_empty_means_default() {
        assert_eq!(parse("size=\n").size, None);
        assert_eq!(parse("size=abc\n").size, None);
        assert_eq!(parse("size=20\n").size, Some(20));
        assert_eq!(parse("size=1\n").size, Some(zoom::MIN));
        assert_eq!(parse("size=999\n").size, Some(zoom::MAX));
        assert!(render("", &Config::default()).contains("size=\n"));
    }

    #[test]
    fn path_is_next_to_the_executable() {
        assert_eq!(path_for_exe(Path::new("/opt/x/neonote")), Some(PathBuf::from("/opt/x/neodir/config.txt")));
    }

    #[test]
    fn custom_colors_need_all_five_and_round_trip() {
        assert_eq!(parse_hex("#2B2b2b"), Some((43, 43, 43)));
        assert_eq!(parse_hex("2b2b2b"), Some((43, 43, 43)));
        assert_eq!(parse_hex("#2b2b2"), None);
        assert_eq!(parse_hex("#gggggg"), None);
        assert!(parse("custom_main=#000000").custom.is_none()); // 5 つそろわない
        let c = [(1, 2, 3), (4, 5, 6), (7, 8, 9), (250, 251, 252), (100, 101, 102)];
        let cfg = Config { theme: Some("custom".into()), custom: Some(c), ..Default::default() };
        let back = parse(&render("", &cfg));
        assert_eq!(back.custom, Some(c));
        assert_eq!(back.theme.as_deref(), Some("custom"));
    }

    #[test]
    fn render_then_parse_round_trips() {
        assert_eq!(parse(&render("", &cfg())), cfg());
        assert_eq!(parse(&render("", &Config::default())), Config::default());
    }

    #[test]
    fn parse_ignores_comments_junk_and_caps_recent() {
        let mut t = String::from("# c\n\nautosave = ON\nbogus line\nfont=\nunknown=1\n");
        for i in 0..15 {
            t.push_str(&format!("recent=/r/{i}.txt\n"));
        }
        let c = parse(&t);
        assert!(c.autosave);
        assert_eq!(c.font, None);
        assert_eq!(c.recent.len(), recent::MAX);
        assert_eq!(c.recent[0], PathBuf::from("/r/0.txt"));
    }

    #[test]
    fn history_keeps_up_to_100_and_survives_round_trip() {
        let mut t = String::new();
        for i in 0..130 {
            t.push_str(&format!("history=/h/{i}.txt\n"));
        }
        let c = parse(&t);
        assert_eq!(c.history.len(), pathlog::MAX);
        assert_eq!(c.history[0], PathBuf::from("/h/0.txt"));
        assert_eq!(parse(&render("", &c)).history, c.history);
        assert!(c.recent.is_empty()); // recent とは別のキー
    }

    #[test]
    fn session_round_trips_dedupes_caps_and_replaces_in_place() {
        // 重複は 1 つにまとめ、上限で切る
        let mut t = String::from("session=/a.txt\nsession=/a.txt\nsession=/b.txt\nsession_active=/b.txt\n");
        for i in 0..80 {
            t.push_str(&format!("session=/s/{i}.txt\n"));
        }
        let c = parse(&t);
        assert_eq!(c.session.len(), SESSION_MAX);
        assert_eq!(c.session[..2], [PathBuf::from("/a.txt"), PathBuf::from("/b.txt")]);
        assert_eq!(c.session_active, Some(PathBuf::from("/b.txt")));
        // 空の値は無視。session が無ければ空
        assert!(parse("session=\nsession_active=\n").session.is_empty());
        assert_eq!(parse("session_active=\n").session_active, None);
        // 書き戻しは、最初の session の行の位置に全部を置き、残りの古い行は消す
        let out = render("# m\nsession=/old1\nautosave=false\nsession=/old2\nsession_active=/old1\n", &Config { session: vec![PathBuf::from("/n1"), PathBuf::from("/n2")], session_active: Some(PathBuf::from("/n2")), ..Default::default() });
        assert!(out.starts_with("# m\nsession=/n1\nsession=/n2\nautosave=false\nsession_active=/n2\n"), "{out}");
        // 空にすると行ごと消える（最後のタブを閉じたとき）
        let out = render(&out, &Config::default());
        assert!(!out.contains("session"), "{out}");
    }

    #[test]
    fn paths_may_contain_equals_and_spaces() {
        let c = Config { recent: vec![PathBuf::from(r"C:\a b\x=y.txt")], ..Default::default() };
        assert_eq!(parse(&render("", &c)).recent, c.recent);
    }

    #[test]
    fn render_keeps_comments_unknown_lines_and_position() {
        let existing = "# mine\nautosave=false\n\nmemo=keep me\nrecent=/old/1.txt\nrecent=/old/2.txt\nrecent=/old/3.txt\n# tail\n";
        let out = render(existing, &cfg());
        assert_eq!(
            out,
            "# mine\nautosave=true\n\nmemo=keep me\nrecent=/a/1.txt\nrecent=/a/2.txt\n# tail\nwrap=true\nsize=18\ntheme=light\ngutter_pin=true\nfont=/f/Mono.ttf\nhistory=/h/1.txt\nhistory=/h/2.txt\nhistory=/h/3.txt\nsession=/s/1.txt\nsession=/s/2.txt\nsession_active=/s/2.txt\n"
        );
        // 2 回目は変化しない
        assert_eq!(render(&out, &cfg()), out);
    }

    #[test]
    fn render_drops_duplicates_and_commented_keys_are_untouched() {
        let out = render("autosave=true\n#autosave=false\nautosave=false\n", &Config::default());
        assert!(out.starts_with("autosave=false\n#autosave=false\n"));
        assert_eq!(out.matches("autosave=").count(), 2); // 実キー 1 行 + コメント 1 行
    }

    #[test]
    fn multiline_values_are_not_written() {
        let c = Config { recent: vec![PathBuf::from("/a\n/b.txt"), PathBuf::from("/ok.txt")], ..Default::default() };
        assert_eq!(parse(&render("", &c)).recent, vec![PathBuf::from("/ok.txt")]);
    }

    #[test]
    fn save_creates_folder_and_skips_identical_writes() {
        let dir = std::env::temp_dir().join(format!("neonote-cfg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("neonote").join("config.txt");
        assert_eq!(load(&p), Config::default()); // 無いファイルは既定
        assert!(save(&p, &cfg()).unwrap());
        assert!(!save(&p, &cfg()).unwrap());
        assert_eq!(load(&p), cfg());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
