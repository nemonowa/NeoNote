// 起動中の NeoNote どうしの連絡（GUI なし・依存なし）。
//   - 2 つ目の起動が、ファイルを起動中のウィンドウへ渡してタブで開かせる（メモ帳のように、ウィンドウを増やさない）。
//   - タブを別のウィンドウへ移す（ドラッグで、ほかのウィンドウの上に落とす）。
//
// 仕組み: 各ウィンドウ（プロセス）が 127.0.0.1 の空いているポートで待ち受け、`設定フォルダ/instances/<pid>.txt` に
// 「ポート<TAB>合言葉」を書く。連絡したい側は、そのフォルダのファイルを順に読んで接続する。合言葉が合わない接続は
// 何も答えずに切る（同じ PC の別の利用者・別のプログラムが勝手に操作できないように、ファイルは本人だけが読める権限で作る）。
// 終了するとファイルを消す。消えずに残った古いファイルは、接続できないことで見つけて消す。
//
// 通信は 1 接続 1 往復。最初の行は `NEONOTE1 <合言葉>`、次の行が命令。応答は `OK ...` か `ERR 理由` か `YES` / `NO`。
//   PING                                  -> OK <最後に操作された時刻(ms)> <pid>
//   RAISE                                 -> OK    （ウィンドウを前に出す）
//   OPEN <件数>  + パスを 1 行ずつ          -> OK    （タブで開いて前に出す）
//   HIT <x> <y>                           -> YES / NO（画面の座標が、このウィンドウの中か）
//   ADOPT <変更ありか 0/1> <パスのバイト数> <本文のバイト数>  + パス + 本文  -> OK / ERR 理由（タブを受け取る）
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DIR: &str = "instances";
const HELLO: &str = "NEONOTE1";
/// 命令の行の長さの上限。
const MAX_LINE: u64 = 8 * 1024;
/// OPEN で一度に渡せるパスの数。
const MAX_PATHS: usize = 500;
/// ADOPT の本文の上限（未保存の控えの上限と同じ）。
pub const MAX_TEXT: usize = crate::neo::unsaved::MAX_TEXT;
/// つなぐときの待ち時間。ローカルなので短くてよい（応答しない相手で、起動を待たせない）。
const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
/// 応答を待つ時間（ウィンドウが確認の窓を出している間は、すぐに答えられない）。
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// 別のウィンドウへ渡すタブ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    /// 元のファイル（名前の無いタブは None）。
    pub path: Option<PathBuf>,
    /// 未保存の変更があるか。あるときだけ `text` を使う（無ければ受け取った側がファイルから読む）。
    pub dirty: bool,
    pub text: String,
}

/// 待ち受け側が、画面の側（メインのスレッド）へ頼む内容。
#[derive(Debug)]
pub enum Cmd {
    Raise,
    Open(Vec<PathBuf>),
    /// 画面の座標（x, y）が、このウィンドウの中か。
    Hit(i32, i32),
    Adopt(Tab),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Ok,
    Yes,
    No,
    /// 依頼を断る（理由つき）。いまの画面の側は使わない（通信の決まりとして、テストで確かめている）。
    #[allow(dead_code)]
    Err(String),
}

/// 画面の側へ渡す依頼。`cancel` が立っていたら、待たせた側が諦めたので、何もしない。
pub struct Request {
    pub cmd: Cmd,
    pub reply: mpsc::SyncSender<Reply>,
    pub cancel: Arc<AtomicBool>,
}


/// このプロセスの待ち受け。持っている間は、ほかの起動から見つけられる。捨てるとファイルを消す。
pub struct Server {
    file: PathBuf,
    focus: Arc<AtomicU64>,
}

fn dir_of(config_dir: &Path) -> PathBuf {
    config_dir.join(DIR)
}

/// 推測できない合言葉（OS が起動ごとに決める乱数の鍵を使う。外部の乱数の部品は使わない）。
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt ^ now_ms());
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", part(1), part(2))
}

/// 起動の順番待ち。ほぼ同時に 2 つ起動すると、どちらも相手を見つけられずにウィンドウが 2 つになるので、
/// 「起動中のウィンドウを探す〜自分の待ち受けを始める」までを 1 つずつにする。`instances/starting.lock` を作れた起動だけが進み、
/// ほかは消えるまで待つ（最大 `START_WAIT`）。落ちて残った古い印（`START_STALE` より前）は消して進む。
static START_LOCK: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
const START_WAIT: Duration = Duration::from_secs(5);
const START_STALE: Duration = Duration::from_secs(10);

/// 順番を取る。取れなくても（待ちすぎ・作れない）そのまま進む（ウィンドウが 2 つになるだけで、壊れはしない）。
pub fn acquire_start_lock(config_dir: &Path) {
    let dir = dir_of(config_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("starting.lock");
    let begin = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                *START_LOCK.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
                return;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > START_STALE);
                if stale {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if begin.elapsed() > START_WAIT {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return,
        }
    }
}

/// 順番を返す（待ち受けを始めた後・始められなかった後・ほかへ渡し終えた後）。持っていなければ何もしない。
pub fn release_start_lock() {
    if let Some(p) = START_LOCK.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = std::fs::remove_file(p);
    }
}

impl Server {
    /// `notify` は、別のスレッドから呼ばれる。依頼を画面の側へ渡し（メインのループを起こす）、答えは `Request::answer` で返させる。
    pub fn start(config_dir: &Path, notify: impl Fn(Request) + Send + 'static) -> io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = token();
        let dir = dir_of(config_dir);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join(format!("{}.txt", std::process::id()));
        write_private(&file, format!("{port}\t{token}\n").as_bytes())?;
        let focus = Arc::new(AtomicU64::new(now_ms()));
        let f2 = Arc::clone(&focus);
        std::thread::Builder::new().name("neonote-ipc".into()).spawn(move || {
            for conn in listener.incoming().flatten() {
                // 1 接続ずつ順に扱う（ローカルの短いやりとり。遅い相手は、読み取りの待ち時間で切る）
                let _ = serve(conn, &token, &f2, &notify);
            }
        })?;
        Ok(Server { file, focus })
    }

    /// このウィンドウが操作された（前面に来た）ことを覚える。どのウィンドウへ渡すかの目安になる。
    pub fn touch(&self) {
        self.focus.store(now_ms(), Ordering::Relaxed);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

#[cfg(unix)]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(data)
}

#[cfg(not(unix))]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    // Windows では、設定フォルダ（利用者のフォルダの下）の権限に従う
    std::fs::write(path, data)
}

fn read_line(r: &mut impl BufRead) -> io::Result<String> {
    let mut s = String::new();
    r.by_ref().take(MAX_LINE).read_line(&mut s)?;
    if !s.ends_with('\n') {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long or cut"));
    }
    Ok(s.trim_end_matches(['\n', '\r']).to_string())
}

fn serve(conn: TcpStream, token: &str, focus: &AtomicU64, notify: &impl Fn(Request)) -> io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(2)))?;
    conn.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut out = conn.try_clone()?;
    let mut r = BufReader::new(conn);
    if read_line(&mut r)? != format!("{HELLO} {token}") {
        return Ok(()); // 合言葉が違う: 何も答えない
    }
    let line = read_line(&mut r)?;
    let mut it = line.split(' ');
    let verb = it.next().unwrap_or("");
    let nums: Vec<&str> = it.collect();
    let num = |i: usize| nums.get(i).and_then(|s| s.parse::<i64>().ok());
    let cmd = match verb {
        "PING" => {
            return writeln!(out, "OK {} {}", focus.load(Ordering::Relaxed), std::process::id());
        }
        "RAISE" => Cmd::Raise,
        "OPEN" => {
            let n = num(0).filter(|n| (0..=MAX_PATHS as i64).contains(n)).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "count"))?;
            let mut paths = Vec::new();
            for _ in 0..n {
                paths.push(PathBuf::from(read_line(&mut r)?));
            }
            Cmd::Open(paths)
        }
        "HIT" => match (num(0), num(1)) {
            (Some(x), Some(y)) => Cmd::Hit(x as i32, y as i32),
            _ => return writeln!(out, "ERR bad"),
        },
        "ADOPT" => {
            let (dirty, plen, tlen) = match (num(0), num(1), num(2)) {
                (Some(d), Some(p), Some(t)) if (0..=1).contains(&d) && (0..=4096).contains(&p) && (0..=MAX_TEXT as i64).contains(&t) => (d == 1, p as usize, t as usize),
                _ => return writeln!(out, "ERR bad"),
            };
            let mut buf = vec![0u8; plen + tlen];
            r.read_exact(&mut buf)?;
            let (p, t) = buf.split_at(plen);
            let (Ok(p), Ok(t)) = (String::from_utf8(p.to_vec()), String::from_utf8(t.to_vec())) else { return writeln!(out, "ERR utf8") };
            Cmd::Adopt(Tab { path: if p.is_empty() { None } else { Some(PathBuf::from(p)) }, dirty, text: t })
        }
        _ => return writeln!(out, "ERR unknown"),
    };
    let (tx, rx) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    notify(Request { cmd, reply: tx, cancel: Arc::clone(&cancel) });
    match rx.recv_timeout(REPLY_TIMEOUT) {
        Ok(Reply::Ok) => writeln!(out, "OK"),
        Ok(Reply::Yes) => writeln!(out, "YES"),
        Ok(Reply::No) => writeln!(out, "NO"),
        Ok(Reply::Err(e)) => writeln!(out, "ERR {}", e.replace('\n', " ")),
        Err(_) => {
            cancel.store(true, Ordering::Relaxed); // 待ちきれない: 後から来ても、何もさせない
            writeln!(out, "ERR busy")
        }
    }
}

// ------------------------------------------------------------------ 連絡する側

/// 起動中の別のウィンドウ。
#[derive(Clone, Debug)]
pub struct Peer {
    pub pid: u32,
    addr: SocketAddr,
    token: String,
    /// 最後に操作された時刻（ms）。大きいほど新しい。
    pub last_focus: u64,
}

fn talk(peer_addr: SocketAddr, token: &str, request: &[u8]) -> io::Result<String> {
    let mut s = TcpStream::connect_timeout(&peer_addr, CONNECT_TIMEOUT)?;
    s.set_read_timeout(Some(REPLY_TIMEOUT + Duration::from_secs(1)))?;
    s.set_write_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(format!("{HELLO} {token}\n").as_bytes())?;
    s.write_all(request)?;
    let mut line = String::new();
    BufReader::new(s).take(MAX_LINE).read_line(&mut line)?;
    if line.is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no reply"));
    }
    Ok(line.trim_end().to_string())
}

fn parse_file(text: &str) -> Option<(u16, String)> {
    let (port, token) = text.trim().split_once('\t')?;
    Some((port.parse().ok()?, token.to_string()))
}

/// 自分以外の、起動中のウィンドウ。最後に操作された順（新しい方が先）。接続できない古いファイルは消す。
pub fn peers(config_dir: &Path, my_pid: u32) -> Vec<Peer> {
    let Ok(rd) = std::fs::read_dir(dir_of(config_dir)) else { return Vec::new() };
    let mut out = Vec::new();
    for ent in rd.flatten() {
        let path = ent.path();
        let Some(pid) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == my_pid {
            continue;
        }
        let Some((port, token)) = std::fs::read_to_string(&path).ok().as_deref().and_then(parse_file) else { continue };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        match talk(addr, &token, b"PING\n") {
            Ok(reply) => {
                let mut it = reply.split(' ');
                if it.next() == Some("OK") {
                    let last_focus = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    out.push(Peer { pid, addr, token, last_focus });
                }
            }
            // 接続できない・合言葉が合わない（ポートを別のプログラムが使っている）: 古いファイル
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    out.sort_by(|a, b| b.last_focus.cmp(&a.last_focus));
    out
}

fn expect_ok(reply: io::Result<String>) -> Result<(), String> {
    match reply {
        Ok(r) if r == "OK" => Ok(()),
        Ok(r) => Err(r.strip_prefix("ERR ").unwrap_or(&r).to_string()),
        Err(e) => Err(e.to_string()),
    }
}

impl Peer {
    pub fn raise(&self) -> Result<(), String> {
        expect_ok(talk(self.addr, &self.token, b"RAISE\n"))
    }

    /// `paths` を、このウィンドウでタブとして開かせる。パスは絶対パスで渡すこと（相手の作業フォルダは違う）。
    pub fn open(&self, paths: &[PathBuf]) -> Result<(), String> {
        let mut req = format!("OPEN {}\n", paths.len());
        for p in paths {
            let s = p.to_str().filter(|s| !s.contains(['\n', '\r'])).ok_or_else(|| "path".to_string())?;
            req.push_str(s);
            req.push('\n');
        }
        expect_ok(talk(self.addr, &self.token, req.as_bytes()))
    }

    /// 画面の座標が、このウィンドウの中か。答えられなければ false。
    pub fn hit(&self, x: i32, y: i32) -> bool {
        talk(self.addr, &self.token, format!("HIT {x} {y}\n").as_bytes()).is_ok_and(|r| r == "YES")
    }

    /// タブを渡す。Ok なら、相手がタブを作った（渡した側は、自分のタブを閉じてよい）。
    pub fn adopt(&self, tab: &Tab) -> Result<(), String> {
        let p = tab.path.as_deref().and_then(Path::to_str).unwrap_or("");
        if p.len() > 4096 || tab.text.len() > MAX_TEXT {
            return Err("too large".into());
        }
        let mut req = format!("ADOPT {} {} {}\n", u8::from(tab.dirty), p.len(), tab.text.len()).into_bytes();
        req.extend_from_slice(p.as_bytes());
        req.extend_from_slice(tab.text.as_bytes());
        expect_ok(talk(self.addr, &self.token, &req))
    }
}

/// 相対パスを、この起動の作業フォルダを基準にした絶対パスにする（別のプロセスへ渡すため）。
pub fn absolute(p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        return path;
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neonote_ipc_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 画面の側の代わり: 受けた命令を記録して、決めた答えを返す。
    fn start(dir: &Path, log: Arc<Mutex<Vec<String>>>, hit: bool) -> Server {
        Server::start(dir, move |req| {
            let (line, reply) = match &req.cmd {
                Cmd::Raise => ("raise".to_string(), Reply::Ok),
                Cmd::Open(p) => (format!("open {p:?}"), Reply::Ok),
                Cmd::Hit(x, y) => (format!("hit {x} {y}"), if hit { Reply::Yes } else { Reply::No }),
                Cmd::Adopt(t) if t.text == "refuse" => ("adopt refuse".to_string(), Reply::Err("no\nway".into())),
                Cmd::Adopt(t) => (format!("adopt {:?} {} {:?}", t.path, t.dirty, t.text), Reply::Ok),
            };
            log.lock().unwrap().push(line);
            let _ = req.reply.send(reply);
        })
        .unwrap()
    }

    // 待ち受けは同じプロセスの中で立てるので、`peers` に自分の pid を除かせず、別の pid を渡して見つける
    const OTHER: u32 = 1;

    #[test]
    fn start_lock_serialises_and_clears_stale_marks() {
        let dir = tmp("lock");
        acquire_start_lock(&dir);
        let lock = dir_of(&dir).join("starting.lock");
        assert!(lock.exists());
        // 別の起動は、印が消えるまで待つ
        let d2 = dir.clone();
        let waiter = std::thread::spawn(move || {
            let t = std::time::Instant::now();
            // 同じプロセスの中なので、静的な持ち主は上書きされるが、ファイルの順番待ちは働く
            let path = dir_of(&d2).join("starting.lock");
            while std::fs::OpenOptions::new().write(true).create_new(true).open(&path).is_err() {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = std::fs::remove_file(&path);
            t.elapsed()
        });
        std::thread::sleep(Duration::from_millis(200));
        release_start_lock();
        assert!(waiter.join().unwrap() >= Duration::from_millis(150));
        // 古い印は消して進む
        std::fs::write(&lock, "1").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        acquire_start_lock(&dir);
        assert!(lock.exists());
        release_start_lock();
        assert!(!lock.exists());
    }

    #[test]
    fn open_raise_hit_and_adopt_round_trip() {
        let dir = tmp("rt");
        let log = Arc::new(Mutex::new(Vec::new()));
        let _srv = start(&dir, Arc::clone(&log), true);
        let ps = peers(&dir, OTHER);
        assert_eq!(ps.len(), 1);
        let p = &ps[0];
        p.open(&[PathBuf::from("/a/b.txt"), PathBuf::from("/c d/日本語.txt")]).unwrap();
        p.raise().unwrap();
        assert!(p.hit(10, -20));
        let tab = Tab { path: Some(PathBuf::from("/x/y.txt")), dirty: true, text: "line1\nline2 あ\n".into() };
        p.adopt(&tab).unwrap();
        p.adopt(&Tab { path: None, dirty: true, text: String::new() }).unwrap();
        assert_eq!(p.adopt(&Tab { path: None, dirty: true, text: "refuse".into() }), Err("no way".to_string()));
        let l = log.lock().unwrap().clone();
        assert_eq!(l[0], r#"open ["/a/b.txt", "/c d/日本語.txt"]"#);
        assert_eq!(l[1], "raise");
        assert_eq!(l[2], "hit 10 -20");
        assert_eq!(l[3], r#"adopt Some("/x/y.txt") true "line1\nline2 あ\n""#);
        assert_eq!(l[4], r#"adopt None true """#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn own_process_is_skipped_and_files_go_away_on_drop() {
        let dir = tmp("own");
        let srv = start(&dir, Arc::default(), false);
        assert!(peers(&dir, std::process::id()).is_empty());
        let ps = peers(&dir, OTHER);
        assert!(!ps[0].hit(0, 0));
        drop(srv);
        assert!(std::fs::read_dir(dir.join(DIR)).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_files_are_removed_and_wrong_tokens_get_no_answer() {
        let dir = tmp("stale");
        let inst = dir.join(DIR);
        std::fs::create_dir_all(&inst).unwrap();
        // 誰も待っていないポートを指す古いファイル
        let dead = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = dead.local_addr().unwrap().port();
        drop(dead);
        std::fs::write(inst.join("4242.txt"), format!("{port}\tabc\n")).unwrap();
        std::fs::write(inst.join("junk.txt"), "x").unwrap();
        std::fs::write(inst.join("5151.txt"), "壊れた内容").unwrap();
        assert!(peers(&dir, OTHER).is_empty());
        assert!(!inst.join("4242.txt").exists());
        // 合言葉の違う接続は、答えをもらえない
        let _srv = start(&dir, Arc::default(), true);
        let ps = peers(&dir, OTHER);
        let bad = Peer { token: "wrong".into(), ..ps[0].clone() };
        assert!(bad.raise().is_err());
        assert!(ps[0].raise().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn most_recently_used_window_comes_first() {
        let dir_a = tmp("order");
        let log = Arc::new(Mutex::new(Vec::new()));
        let a = start(&dir_a, Arc::clone(&log), false);
        // 同じフォルダに、時刻の違う 2 つ目の待ち受けを作る（pid の名前のファイルを別名にして並べる）
        let b = start(&dir_a, Arc::clone(&log), false);
        let inst = dir_a.join(DIR);
        let file_b = std::fs::read_to_string(&b.file).unwrap();
        std::fs::write(inst.join("7.txt"), file_b).unwrap();
        a.focus.store(100, Ordering::Relaxed);
        b.focus.store(900, Ordering::Relaxed);
        let ps = peers(&dir_a, OTHER);
        assert!(ps.len() >= 2);
        assert!(ps.windows(2).all(|w| w[0].last_focus >= w[1].last_focus));
        assert_eq!(ps[0].last_focus, 900);
        let _ = std::fs::remove_dir_all(&dir_a);
    }

    #[test]
    fn unanswered_requests_time_out_and_are_cancelled() {
        let dir = tmp("busy");
        let seen: Arc<Mutex<Vec<Arc<AtomicBool>>>> = Arc::default();
        let s2 = Arc::clone(&seen);
        // 画面の側が答えない（確認の窓を出している間など）
        let _srv = Server::start(&dir, move |req| {
            s2.lock().unwrap().push(Arc::clone(&req.cancel));
            std::mem::forget(req.reply);
        })
        .unwrap();
        let ps = peers(&dir, OTHER);
        let t = std::time::Instant::now();
        assert_eq!(ps[0].raise(), Err("busy".to_string()));
        assert!(t.elapsed() < Duration::from_secs(6));
        assert!(seen.lock().unwrap()[0].load(Ordering::Relaxed), "諦めた依頼は取り消される");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_requests_do_not_crash_the_server() {
        let dir = tmp("bad");
        let log = Arc::new(Mutex::new(Vec::new()));
        let _srv = start(&dir, Arc::clone(&log), false);
        let ps = peers(&dir, OTHER);
        let send = |raw: &[u8]| talk(ps[0].addr, &ps[0].token, raw);
        assert!(send(b"OPEN 99999\n").is_err());
        assert!(send(b"OPEN 2\nonly-one\n").is_err());
        assert_eq!(send(b"HIT x y\n").unwrap(), "ERR bad");
        assert_eq!(send(b"ADOPT 1 99999 1\n").unwrap(), "ERR bad");
        assert_eq!(send(b"NOPE\n").unwrap(), "ERR unknown");
        let mut junk = vec![0xffu8; 64];
        junk.insert(0, b'\n');
        let _ = send(&junk);
        assert!(ps[0].raise().is_ok(), "不正な要求のあとも動く");
        assert!(log.lock().unwrap().iter().all(|l| l == "raise"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absolute_paths_are_made_from_the_working_directory() {
        assert_eq!(absolute(if cfg!(windows) { "C:\\a\\b" } else { "/a/b" }), PathBuf::from(if cfg!(windows) { "C:\\a\\b" } else { "/a/b" }));
        let p = absolute("rel/x.txt");
        assert!(p.is_absolute());
        assert!(p.ends_with("rel/x.txt"));
    }
}
