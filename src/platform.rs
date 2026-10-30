// OS 別の処理（ブラウザ起動・フォルダを開く・シェル実行）をここに集める。
// Windows 側は Linux 上でビルド確認できていない（README 参照）。
use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

pub const HOMEPAGE: &str = "https://nemonowa.github.io/";
/// 配布ページ（Check Update で開く。設計書「連携情報」の Download）。
pub const DOWNLOAD_URL: &str = "https://drive.google.com/file/d/1zSsNEQi-YDSH6ytb8NSPXDFqjqP862ZH/view?usp=sharing";

pub fn is_http_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://"))
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// URL 用のパーセントエンコード（Java の URLEncoder 相当: 空白は `+`）。
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 選択文字列から開く URL を決める。URL ならそのまま、それ以外は Google 検索、空ならホームページ。
pub fn search_url(selected: &str) -> String {
    let s = selected.trim();
    if s.is_empty() {
        HOMEPAGE.to_string()
    } else if is_http_url(s) {
        s.to_string()
    } else {
        format!("https://www.google.com/search?q={}", percent_encode(s))
    }
}

/// 「フォントを選ぶ」ダイアログの初期フォルダ（ユーザーフォントのフォルダ）。
/// Windows: `%USERPROFILE%\AppData\Local\Microsoft\Windows\Fonts`（Java 版と同じ）、
/// macOS: `~/Library/Fonts`、それ以外: `~/.local/share/fonts`。無ければホーム。
pub fn font_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)?;
    Some(font_dir_in(&home))
}

fn font_dir_in(home: &Path) -> PathBuf {
    let dir = if cfg!(target_os = "windows") {
        home.join("AppData").join("Local").join("Microsoft").join("Windows").join("Fonts")
    } else if cfg!(target_os = "macos") {
        home.join("Library").join("Fonts")
    } else {
        home.join(".local").join("share").join("fonts")
    };
    if dir.is_dir() { dir } else { home.to_path_buf() }
}

fn spawn_detached(mut cmd: Command) -> io::Result<()> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    // 終了を待って回収だけする（画面は待たない）
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// URL の画像を、OS の `curl` で `dest` に保存する（依存を増やさないため、HTTP の部品は持たない）。
/// http(s) 以外・大きすぎるもの（32 MiB 超）・20 秒を超えるものは失敗にする。シェルを通さない。
pub fn download(url: &str, dest: &Path) -> io::Result<()> {
    if !is_http_url(url) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, crate::lang::tr("http(s) の URL ではありません", "Not an http(s) URL")));
    }
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "20", "--max-filesize", "33554432", "--proto", "=http,https", "-o"])
        .arg(dest)
        .args(["--url", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            io::Error::new(io::ErrorKind::NotFound, crate::lang::tr("curl が見つかりません（画像の URL を開くには curl が必要です）", "curl not found (curl is needed to open image URLs)"))
        } else {
            e
        }
    })?;
    if out.status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(dest);
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(io::Error::other(if msg.is_empty() { crate::lang::tr("ダウンロードに失敗しました", "Download failed").to_string() } else { msg }))
    }
}

/// 既定のブラウザで URL を開く。シェルを通さないので `&` などを含んでも安全。
pub fn open_url(url: &str) -> io::Result<()> {
    if !is_http_url(url) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, if crate::lang::is_en() { format!("Not an http(s) URL: {url}") } else { format!("http(s) の URL ではありません: {url}") }));
    }
    #[cfg(target_os = "windows")]
    let cmd = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c
    };
    #[cfg(target_os = "macos")]
    let cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    spawn_detached(cmd)
}

/// Exec 行用に引用符で囲む（Desktop Entry 仕様: `"` `` ` `` `$` `\\` をバックスラッシュで逃がす）。
#[cfg(any(unix, test))]
fn desktop_exec_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(any(unix, test))]
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=NeoNote\nComment=Light, fast, clickless plain text editor\nExec={} %F\nTerminal=false\nCategories=Utility;TextEditor;\nMimeType=text/plain;\n",
        desktop_exec_quote(&exe.to_string_lossy())
    )
}

/// デスクトップに NeoNote のショートカットを作り、作ったファイルの場所を返す。
/// Linux: `NeoNote.desktop` を書く。Windows: PowerShell で `.lnk` を作る。macOS は未対応。
pub fn make_shortcut() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    make_shortcut_for(&exe)
}

#[cfg(target_os = "windows")]
fn make_shortcut_for(exe: &Path) -> io::Result<PathBuf> {
    // 保存先のデスクトップは OS に聞く（OneDrive 配下に移されていても正しい）
    let script = format!(
        "$d=[Environment]::GetFolderPath('Desktop');$p=Join-Path $d 'NeoNote.lnk';\
         $s=(New-Object -ComObject WScript.Shell).CreateShortcut($p);\
         $s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save();Write-Output $p",
        exe.display().to_string().replace('\'', "''"),
        exe.parent().map(|p| p.display().to_string()).unwrap_or_default().replace('\'', "''")
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

#[cfg(target_os = "macos")]
fn make_shortcut_for(_exe: &Path) -> io::Result<PathBuf> {
    Err(io::Error::new(io::ErrorKind::Unsupported, crate::lang::tr("macOS ではまだ対応していません", "Not yet supported on macOS")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn make_shortcut_for(exe: &Path) -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, crate::lang::tr("HOME が未設定です", "HOME is not set")))?;
    let desktop = home.join("Desktop");
    let dir = if desktop.is_dir() { desktop } else { home };
    write_desktop_entry(&dir, exe)
}

#[cfg(unix)]
fn write_desktop_entry(dir: &Path, exe: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("NeoNote.desktop");
    std::fs::write(&path, desktop_entry(exe))?;
    // 実行可能にしておく（デスクトップ環境によっては必要）
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

/// フォルダをファイルマネージャで開く。
pub fn open_folder(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    let cmd = {
        let mut c = Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let cmd = {
        let mut c = Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    spawn_detached(cmd)
}

/// ファイルマネージャで、そのファイル（フォルダ）を選んだ状態で開く。Windows 以外は、入っているフォルダを開く。
pub fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let mut c = Command::new("explorer");
        // explorer は「/select,パス」を 1 つの引数として受ける（空白を含むパスも、そのまま渡せば読める）
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        c.arg(arg);
        spawn_detached(c)
    }
    #[cfg(not(target_os = "windows"))]
    open_folder(path.parent().unwrap_or(path))
}

/// ファイル・フォルダをごみ箱へ移す（元に戻せる消し方）。Windows 以外は対応していない。
#[cfg(windows)]
pub fn move_to_trash(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    #[allow(non_snake_case)]
    struct SHFILEOPSTRUCTW {
        hwnd: *mut std::ffi::c_void,
        wFunc: u32,
        pFrom: *const u16,
        pTo: *const u16,
        fFlags: u16,
        fAnyOperationsAborted: i32,
        hNameMappings: *mut std::ffi::c_void,
        lpszProgressTitle: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(op: *mut SHFILEOPSTRUCTW) -> i32;
    }
    const FO_DELETE: u32 = 3;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOERRORUI: u16 = 0x0400;
    // 終わりは NUL 2 つ（複数のパスを並べる形式）
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 || op.fAnyOperationsAborted != 0 {
        return Err(io::Error::other(format!("SHFileOperation: 0x{r:X}")));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn move_to_trash(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "trash is supported only on Windows"))
}

/// 文字列をシェルのコマンドとして実行し、標準出力と標準エラーをまとめて返す（終わるまで待つ）。
/// GUI を塞がないよう、呼び出し側でスレッドに載せること。
pub fn run_shell(command: &str) -> io::Result<String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/c").raw_arg(command).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };
    let out = cmd.stdin(Stdio::null()).output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        if let Some(code) = out.status.code() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("[exit code {code}]\n"));
        }
    }
    Ok(text)
}

/// バイナリに埋め込んだ既定フォント（NeoFont）。
static BUNDLED_FONT: &[u8] = include_bytes!("../assets/NeoFont.ttf");

/// 埋め込みフォントをディスクに書き出して、そのパスを返す。
/// FLTK はメモリからフォントを読めない（ファイルのパスが要る）ため、一時フォルダに展開する。
/// 同じ内容が既にあれば書き直さない（2 回目以降の起動を速くする）。
pub fn bundled_font_path() -> io::Result<PathBuf> {
    bundled_font_in(&std::env::temp_dir().join(concat!("neonote-font-", env!("CARGO_PKG_VERSION"))))
}

fn bundled_font_in(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("NeoFont.ttf");
    if std::fs::read(&path).map(|b| b == BUNDLED_FONT).unwrap_or(false) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    // 一時ファイル→リネーム（同時に起動した別のプロセスに、書きかけを読ませない）
    let tmp = dir.join(format!("NeoFont.ttf.{}.tmp", std::process::id()));
    std::fs::write(&tmp, BUNDLED_FONT)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        // 別プロセスが先に置いていた（Windows では上書きリネームが失敗しうる）なら、それを使う
        if !std::fs::read(&path).map(|b| b == BUNDLED_FONT).unwrap_or(false) {
            return Err(e);
        }
    }
    Ok(path)
}

/// 時刻を、この PC の時間帯で `YYYY-MM-DD HH:MM` にする（日時の部品を足さないため、OS の関数を直接呼ぶ）。
pub fn local_datetime(t: std::time::SystemTime) -> String {
    let secs = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => return String::new(),
    };
    match local_fields(secs) {
        Some((y, mo, d, h, mi)) => format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}"),
        None => String::new(),
    }
}

#[cfg(windows)]
fn local_fields(secs: i64) -> Option<(i32, u32, u32, u32, u32)> {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn FileTimeToLocalFileTime(ft: *const FileTime, local: *mut FileTime) -> i32;
        fn FileTimeToSystemTime(ft: *const FileTime, st: *mut SystemTime) -> i32;
    }
    // FILETIME は 1601-01-01 からの 100 ナノ秒単位
    let ticks = (secs + 11_644_473_600) as u64 * 10_000_000;
    let ft = FileTime { low: ticks as u32, high: (ticks >> 32) as u32 };
    let mut local = FileTime { low: 0, high: 0 };
    let mut st = SystemTime::default();
    let ok = unsafe { FileTimeToLocalFileTime(&ft, &mut local) != 0 && FileTimeToSystemTime(&local, &mut st) != 0 };
    ok.then(|| (st.year as i32, st.month as u32, st.day as u32, st.hour as u32, st.minute as u32))
}

#[cfg(unix)]
fn local_fields(secs: i64) -> Option<(i32, u32, u32, u32, u32)> {
    // struct tm の先頭 6 項目（秒・分・時・日・月・年）は どの OS でもこの並び。残りは余裕を持って確保する
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        rest: [i64; 8],
    }
    extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, rest: [0; 8] };
    let r = unsafe { localtime_r(&secs, &mut tm) };
    (!r.is_null()).then(|| (tm.year + 1900, (tm.mon + 1) as u32, tm.mday as u32, tm.hour as u32, tm.min as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_is_extracted_once_and_repaired() {
        let dir = std::env::temp_dir().join(format!("neonote_font_test_{}", std::process::id()));
        let p = bundled_font_in(&dir).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), BUNDLED_FONT);
        // 壊れていたら書き直す
        std::fs::write(&p, b"broken").unwrap();
        let p2 = bundled_font_in(&dir).unwrap();
        assert_eq!(std::fs::read(&p2).unwrap(), BUNDLED_FONT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_or_google_or_homepage() {
        assert_eq!(search_url("  https://example.com/a?b=1&c=2 "), "https://example.com/a?b=1&c=2");
        assert_eq!(search_url("httpfoo"), "https://www.google.com/search?q=httpfoo");
        assert_eq!(search_url("rust 日本語"), "https://www.google.com/search?q=rust+%E6%97%A5%E6%9C%AC%E8%AA%9E");
        assert_eq!(search_url("   "), HOMEPAGE);
        // 空白を含む「http…」は URL ではなく検索語として扱う
        assert!(search_url("http://a b").starts_with("https://www.google.com/search?q="));
    }

    #[test]
    fn font_dir_falls_back_to_home_when_missing() {
        let home = std::env::temp_dir().join("neonote_no_such_home_for_font_test");
        assert_eq!(font_dir_in(&home), home);
    }

    #[test]
    fn open_url_refuses_non_http() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("calc.exe").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn run_shell_captures_both_streams_and_exit_code() {
        assert_eq!(run_shell("echo hi").unwrap(), "hi\n");
        let t = run_shell("echo out; echo err 1>&2; exit 3").unwrap();
        assert!(t.contains("out") && t.contains("err") && t.contains("[exit code 3]"));
    }

    #[test]
    fn desktop_exec_is_quoted_and_escaped() {
        assert_eq!(desktop_exec_quote("/opt/my apps/neonote"), "\"/opt/my apps/neonote\"");
        assert_eq!(desktop_exec_quote("/a/$b\"c"), "\"/a/\\$b\\\"c\"");
        let e = desktop_entry(Path::new("/opt/my apps/neonote"));
        assert!(e.contains("Exec=\"/opt/my apps/neonote\" %F\n") && e.starts_with("[Desktop Entry]\n"));
    }

    #[cfg(unix)]
    #[test]
    fn desktop_entry_file_is_written_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("neonote-shortcut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = write_desktop_entry(&d, Path::new("/x/neonote")).unwrap();
        assert_eq!(p.file_name().unwrap(), "NeoNote.desktop");
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(std::fs::read_to_string(&p).unwrap().contains("Exec=\"/x/neonote\" %F"));
    }
}

/// ウィンドウを、ほかのウィンドウより常に手前に出す（on）／戻す（off）。できたら true。
/// `handle` は FLTK の `raw_handle()`（Windows では HWND）。Windows 以外は今は対応していない（false）。
#[cfg(windows)]
pub fn set_topmost(handle: *mut std::ffi::c_void, on: bool) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowPos(hwnd: *mut std::ffi::c_void, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    const HWND_TOPMOST: isize = -1;
    const HWND_NOTOPMOST: isize = -2;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    if handle.is_null() {
        return false;
    }
    unsafe { SetWindowPos(handle, if on { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE) != 0 }
}

#[cfg(not(windows))]
pub fn set_topmost(_handle: *mut std::ffi::c_void, _on: bool) -> bool {
    false
}
