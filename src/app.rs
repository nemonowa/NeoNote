// ウィンドウ・イベントループ・状態。
// 第2段階: 編集・行番号・開く/保存・非同期読込・未保存確認付き終了・ドロップ受付。
// 第4段階: ツールバー・右クリックメニュー・Replace / Search / Search In web / Run In CMD・パスのコピー。
// タブ: 文書ごとの状態は Doc に持ち、Ui は文書の一覧と「いま表示している文書」を持つ。
// 検索バー（常駐）・タブの並べ替え・セッション復元: 設計書の「検索バー」「タブの並べ替え」「セッション復元」の節を参照。
use crate::audioplay;
use crate::neo::{unsaved, archive, audiometa, book, nbt, nbtedit::{self, Seg}, region, doctext, htmlview, imgmeta, peek, imagefile, pdftext, markdown, code, config, document::{self, Mode}, dropped, encoding::Encoding, eol::{self, Eol}, goto, hex, history::{self, History}, indent, json, pathlog, zoom, recent, replace, selection, sheet, viewmode};
use crate::ui::{confirm, menu::{self, Action}, nbttree::NbtTree, peekwin, searchbar::{self, FindCmd, SearchBar}, sidepanel::{self, SidePanel}, skin, tabs::{self, TabCmd, TabInfo}, toolbar, treemap};
use crate::{dev, log, platform, ui::theme::{self, *}};
use fltk::{
    app,
    button::Button,
    dialog::{self, FileDialogOptions, FileDialogType, NativeFileChooser},
    enums::{Align, Color, Event, Font, FrameType, Key},
    frame::Frame,
    group::Flex,
    misc::HelpView,
    image::PngImage,
    prelude::*,
    text::{PositionType, StyleTableEntryExt, TextAttr, TextBuffer, TextEditor},
    window::Window,
};
use fltk::prelude::FltkError;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    thread,
};

/// 同梱のアイコンと README（今は仮のファイル。差し替えるには assets/ の同名ファイルを置き換えてビルドし直す）。
static ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
static README_TEXT: &str = include_str!("../assets/README.txt");
/// 第三者のライセンス表示（admin/third_party_licenses.py で作る）。「ライセンス」でタブに開く。
static THIRD_PARTY_LICENSES: &str = include_str!("../assets/THIRD_PARTY_LICENSES.txt");
const TITLE: &str = "NeoNote";
const TAG: &str = "NeoNote";
/// これを超える文書では、入力のたびの文字数更新を省く（保存・読込時に更新）。
const LIVE_COUNT_LIMIT: i32 = 8 * 1024 * 1024;
/// これより大きい文書は折り返さない（折り返しの計算は文書全体に掛かり、100 MB で数秒固まる）。
const WRAP_LIMIT: i32 = 8 * 1024 * 1024;
/// 同梱フォント（NeoFont）を読み込めなかったときの代わりのフォント。
const FALLBACK_FONT: Font = Font::Courier;
/// `App::load_font` が読み込んだフォントを入れる枠の番号（fltk-rs の仕様）。
const LOADED_FONT_SLOT: i32 = 16;
/// Run In CMD を実行前に確認するか。設計上の決定は「確認なし」（Java 版と同じ）。
/// 確認を入れたくなったら true にするだけでよい。
const CONFIRM_RUN_CMD: bool = false;
/// ダイアログに出す出力の上限（文字数）。
const RUN_OUTPUT_LIMIT: usize = 4000;
/// Enable Auto Save が有効なときに、未保存のファイルを自動で上書き保存する間隔（秒）。
const AUTOSAVE_SECS: f64 = 30.0;
/// 未保存の変更の控え（neo/unsaved.rs）を書き直す間隔（秒）。終了時にも書く。変わっていなければ書かない。
const UNSAVED_SECS: f64 = 120.0;
/// Undo All / Redo All が 1 回で巻き戻す・やり直す最大の回数（無限ループの安全策）。
const HISTORY_STEP_LIMIT: usize = 100_000;
/// 検索バーの入力欄の文字の大きさ（本文の拡大縮小には合わせない）。
const FIND_TEXT_SIZE: i32 = 14;
/// 行番号欄が隠れているとき、マウスが左端と見なされる幅。
/// 端の欄を出すまでに留まる回数（0.1 秒ごと。ツールチップと同じくらい待つ）。
const EDGE_DWELL: u32 = 5;
const GUTTER_EDGE: i32 = 10;
/// 行番号欄を広げられる最大の幅。
const GUTTER_EXTRA_MAX: i32 = 120;
/// 左右の欄を出す、本文の端からの範囲（本文の幅に対する割合）。
const EDGE_RATIO: f64 = 0.05;

/// これより大きな文書は、横スクロールバーを出すかどうかを測らない（全文を走査するため）。
const HSCROLL_SCAN_LIMIT: i32 = 512 * 1024;

/// 一番幅の広い行の幅（ピクセル）。タブは 8 桁ぶんの空白として数える。
/// 先に一番長い行の長さを調べ、それより十分短い行（長さが 1/4 未満）は測らない。
fn longest_line_px(text: &str, font: Font, size: i32) -> i32 {
    let max_len = text.split('\n').map(str::len).max().unwrap_or(0);
    fltk::draw::set_font(font, size);
    let mut best = 0.0f64;
    for line in text.split('\n') {
        if line.len() * 4 < max_len {
            continue;
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        let w = if line.contains('\t') { fltk::draw::width(&line.replace('\t', "        ")) } else { fltk::draw::width(line) };
        best = best.max(w);
    }
    best.ceil() as i32
}

/// タイトルに出すパス。実行ファイルのフォルダの中なら、そのフォルダの部分を省く。
fn title_path(path: &Path, exe_dir: Option<&Path>) -> String {
    match exe_dir.and_then(|d| path.strip_prefix(d).ok()) {
        Some(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
        _ => path.display().to_string(),
    }
}

pub fn window_title(path: Option<&Path>, pos: Option<(usize, usize)>, chars: usize, encoding: &str, eol: &str, dirty: bool) -> String {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    let file = path.map(|p| title_path(p, exe_dir)).unwrap_or_else(|| crate::lang::tr("未保存のファイル", "Unsaved File").into());
    let mark = if dirty { "*" } else { "" };
    let at = pos.map(|(l, c)| format!(" | Ln {l}, Col {c}")).unwrap_or_default();
    let eol = if eol.is_empty() { String::new() } else { format!(" | EOL: {eol}") };
    let (path_l, chars_l, enc_l) = if crate::lang::is_en() { ("Path", "Chars", "Encoding") } else { ("パス", "文字数", "文字コード") };
    format!(
        "{mark}[{}] {TITLE} - by nemo | {path_l}: {file}{at} | {chars_l}: {chars} | {enc_l}: {encoding}{eol}",
        dev::version()
    )
}

enum Msg {
    /// 読込の結果（文書の id, パス, 結果）。読込中にタブが閉じられていたら捨てる。
    Loaded(u64, PathBuf, Result<document::Loaded, String>),
    /// ツールバーのボタンが選ばれた。
    Do(Action),
    /// タブバーの操作。
    Tab(TabCmd),
    /// 検索バーの操作。
    Find(FindCmd),
    /// ウィンドウの大きさが変わった（タブの幅を合わせ直す）。
    Resized,
    /// Run In CMD の結果（バックグラウンドで実行して戻ってくる）。
    Ran(Result<String, String>),
    /// 設定ファイル（config.txt）をエディタで保存した。内容を読み直して反映する。
    ConfigSaved,
    /// pdf の文字の取り出し結果（文書の id, ページごとの文字）。
    PdfText(u64, Result<Vec<String>, String>),
    /// xlsx / ods のシートの読込結果（文書の id）。
    Book(u64, Result<Vec<book::Sheet>, String>),
    /// docx / odt / pptx / odp / epub の文字の読込結果（文書の id）。
    DocText(u64, Result<doctext::Text, String>),
    /// URL の画像を保存できた（保存先と URL）、または失敗した理由。
    ImageFetched(Result<(PathBuf, String), String>),
    /// フォルダ・zip の一覧の読込結果（文書の id, パス, 一覧）。
    Archive(u64, PathBuf, Result<Vec<archive::Entry>, String>),
    /// 音声の情報とタグの読込結果（文書の id）。
    Audio(u64, Result<AudioData, String>),
    /// 右端の縮小表示で行が選ばれた（0 始まり）。
    SideJump(i32),
    /// 右端の欄の `Pin` が押された。
    SidePin,
    /// 面積図の四角の右クリック（一覧の中の相対パス, フォルダか）。
    SideMenu(String, bool),
    SideWidth(i32, bool),
    GutterWidth(i32, bool),
}

/// 音声の Preview に出す内容（別スレッドでファイルを読んで作る）。
struct AudioData {
    info: Result<audiometa::Info, String>,
    items: Vec<(String, String)>,
}

/// 音声の情報を読むときの、ファイルの大きさの上限（これを超えるファイルは、情報とタグを出さない）。
const AUDIO_INFO_MAX: u64 = 256 * 1024 * 1024;

/// 文書ごとの状態。
struct State {
    path: Option<PathBuf>,
    /// 読込中のパス。成功するまで現在のファイル（path）は変えない。
    loading_path: Option<PathBuf>,
    /// 保存時に使う文字コード（読込時に判定または指定したもの）。
    encoding: Encoding,
    bom: bool,
    /// HEX モード（text は HEX 表示。保存はバイト列として書く）。
    hex: bool,
    /// HEX モードで上限超過のため先頭だけ読んだ。保存は無効。
    hex_truncated: bool,
    /// 文字コードを自動判定で決めた（手動で選んでいない）。新規の文書も自動。
    auto: bool,
    dirty: bool,
    loading: bool,
    chars: usize,
    /// 開いた（保存した）時点のディスク上の状態。外部で書き換わったか確かめる。
    stamp: Option<document::Stamp>,
    /// 外部変更のため自動保存を見送ったことを、もう知らせた。
    conflict_noted: bool,
    /// 改行コード。開いたとき（と変換したとき）に決める。打鍵のたびには数え直さない。
    eol: Eol,
    /// 画像のメタデータを編集している文書か（値は対象の画像。保存すると画像へ書き戻す）。
    meta_of: Option<PathBuf>,
    /// 保存先の無い文書のタブに出す名前（同梱の README など）。
    title: Option<PathBuf>,
    /// フォルダ・zip の名前の一覧を編集している文書か（値は対象のフォルダ / zip。保存すると名前を変更する）。
    arch_of: Option<PathBuf>,
    /// NBT（level.dat など）を SNBT の文字にして編集している。保存するときは NBT に戻す。
    nbt: Option<nbt::NbtFile>,
    /// リージョンファイル（.mca）のチャンクの一覧（保存しない。行で Enter かダブルクリックでチャンクを開く）。
    region_list: bool,
    /// リージョンのチャンクを編集している（リージョンファイル, チャンクの番号）。保存するとそのチャンクだけ書き換える。
    chunk_of: Option<(PathBuf, usize)>,
}

impl Default for State {
    fn default() -> Self {
        State {
            path: None,
            loading_path: None,
            encoding: Encoding::Utf8,
            bom: false,
            hex: false,
            hex_truncated: false,
            auto: true,
            dirty: false,
            loading: false,
            chars: 0,
            stamp: None,
            conflict_noted: false,
            eol: Eol::platform_default(),
            meta_of: None,
            title: None,
            arch_of: None,
            nbt: None,
            region_list: false,
            chunk_of: None,
        }
    }
}

/// 1 つのタブが持つ文書。本文のバッファを文書ごとに持つので、「元に戻す」の履歴も文書ごとに残る。
struct Doc {
    id: u64,
    buf: TextBuffer,
    /// Search のハイライト用スタイルバッファ。最初に Search したとき（以降は切替のとき）に作る。
    sbuf: RefCell<Option<TextBuffer>>,
    st: RefCell<State>,
    /// プログラムによる set_text 中は変更扱いにしない。
    quiet: Cell<bool>,
    /// 表示していないあいだのカーソル位置。
    cursor: Cell<i32>,
    /// md の Preview（整形表示）で見ているか。本文のバッファはそのまま持つ。
    preview: Cell<bool>,
    /// 拡張子で決まらない文字の文書を、この形式で Preview する（表示のメニューで選ぶ。タブを閉じるまで覚える）。
    preview_as: Cell<Option<viewmode::Kind>>,
    /// 形式を自動で決める（拡張子、無ければ中身から）。表示のメニューで形式を選ぶと外れる。
    format_auto: Cell<bool>,
    /// 前回の未保存の変更（読み込みが終わったら本文に戻す）。
    pending_restore: RefCell<Option<String>>,
    /// pdf の Preview 用に取り出した文字（ページごと）。None は未取得か取得中。
    pdf_text: RefCell<Option<Vec<String>>>,
    /// pdf の文字の取り出しに失敗した理由（あれば Preview に出す）。
    pdf_error: RefCell<Option<String>>,
    /// 文字を取り出している最中か。
    pdf_busy: Cell<bool>,
    /// xlsx / ods の Preview 用に読んだシート（None は未取得か取得中）。
    book: RefCell<Option<Result<Vec<book::Sheet>, String>>>,
    book_busy: Cell<bool>,
    /// docx / odt / pptx / odp / epub の Preview 用に読んだ文字（None は未取得か取得中）。
    doc_text: RefCell<Option<Result<doctext::Text, String>>>,
    doc_busy: Cell<bool>,
    /// フォルダ・zip を開いている文書の一覧（Preview に出す。None はふつうの文書）。
    archive: RefCell<Option<Vec<archive::Entry>>>,
    /// 名前の一覧を編集している文書が、編集前の並びとして持つ一覧。
    arch_old: RefCell<Vec<archive::Entry>>,
    /// 音声の Preview 用の情報とタグ（None は未取得か取得中）。
    audio: RefCell<Option<Result<AudioData, String>>>,
    audio_busy: Cell<bool>,
    /// 再生中（または止めてある）プレーヤー。タブを閉じる・読み直す・別のファイルを再生するときに捨てる。
    player: RefCell<Option<audioplay::Player>>,
    /// Preview に出している再生位置（秒）。変わったときだけ描き直す。
    audio_secs: Cell<i64>,
    /// NBT の Preview の木で開いていた項目（Text と行き来しても開いたままにする）。
    tree_open: RefCell<Option<std::collections::HashSet<Vec<Seg>>>>,
}

impl Doc {
    /// 空で、保存先もなく、何も入力されていない新規タブ（開くときに再利用してよい）。
    fn is_pristine(&self) -> bool {
        let st = self.st.borrow();
        st.path.is_none() && st.loading_path.is_none() && !st.loading && !st.dirty && !st.hex && self.buf.length() == 0
    }
}

struct Ui {
    app: app::App,
    win: Window,
    editor: TextEditor,
    /// md の Preview 用の表示部品（編集領域と同じ場所に出し入れする。使うまでは隠す）。
    preview: HelpView,
    /// NBT の Preview の木（編集領域と同じ場所に出し入れする）。
    nbt_tree: NbtTree,
    /// 編集領域とプレビューを出し入れしたときに並べ直すための、本文を含む縦の列。
    col: Flex,
    /// ツールバー右の余白。短い通知（パスをコピーした等）を出す。
    gap: Frame,
    tabbar: RefCell<Flex>,
    /// タブ行（左端の押しピンの幅を行番号欄に合わせる）。
    tabrow: Flex,
    /// タブバー右端の表示モードのチップ。
    chip: Button,
    /// 表示（Text・Preview・HEX）の切替。chip の左。
    view_chip: Button,
    /// 最後に作ったタブバーの内容と幅。変わらないときは作り直さない。
    tabs_shown: RefCell<(Vec<TabInfo>, i32)>,
    docs: RefCell<Vec<Rc<Doc>>>,
    /// 表示している文書の id。文書は常に 1 つ以上ある。
    active: Cell<u64>,
    next_id: Cell<u64>,
    /// いずれかの文書で Search を使った（以降、切り替えた先の文書にもスタイルバッファを付ける）。
    styled: Cell<bool>,
    /// 編集領域・行番号・ハイライトで使っているフォント。
    font: Cell<Font>,
    count_pending: Cell<bool>,
    dnd_dropped: Cell<bool>,
    /// 最近開いたファイル（config.txt に保存する）。
    recent: RefCell<Vec<PathBuf>>,
    /// 過去に読み込んだファイルのパス履歴（最大 100 件。config.txt に保存する）。View History / Restore From History。
    path_history: RefCell<Vec<PathBuf>>,
    /// Enable Auto Save（config.txt に保存する）。
    autosave: Cell<bool>,
    /// Word Wrap（編集領域を右端で折り返す。config.txt に保存する）。
    wrap: Cell<bool>,
    /// 文字サイズ（config.txt に保存する）。
    size: Cell<i32>,
    /// 行番号欄の「固定」（常に表示。config.txt の gutter_pin に保存する）。
    gutter_pinned: Cell<bool>,
    /// いま行番号欄を出しているか。固定・マウスが左端、のどちらかで出す。
    gutter_shown: Cell<bool>,
    /// 行番号欄を出すときの幅（行数の桁数と文字の幅から決める）と、それを数えたときの (文書の id, 文字数)。
    gutter_w: Cell<i32>,
    /// 行番号欄を桁数で決まる幅より広げる分（左端の押しピンの境目をドラッグ。config.txt の gutter_extra）。
    gutter_extra: Cell<i32>,
    /// 桁数で決まる幅（gutter_extra を足す前）。
    gutter_base: Cell<i32>,
    gutter_key: Cell<(u64, i32)>,
    gutter_digits: Cell<i32>,
    /// 横スクロールバーを出すかどうかを前回決めたときの条件（文書・文字数・幅・文字サイズ・フォント・折り返し・行番号欄の幅）。
    hscroll_key: Cell<(u64, i32, i32, i32, i32, bool, i32)>,
    /// マウスが編集領域の左端（行番号欄の上）にある。
    mouse_left: Cell<bool>,
    /// マウスが端に入ってからの回数（poll_mouse の 0.1 秒ごと）。EDGE_DWELL に届いたら欄を出す。
    edge_dwell: Cell<(u32, u32)>,
    /// 編集領域・プレビューと右端の欄を横に並べる行。
    body: Flex,
    /// 右端の情報・縮小表示の欄。
    side: SidePanel,
    /// 右端の欄の「固定」（config.txt の side_pin に保存する）。
    side_pinned: Cell<bool>,
    /// 右端の欄の幅（config.txt の side_w に保存する）。
    side_w: Cell<i32>,
    /// 続きから（config.txt の resume）。
    resume: Cell<bool>,
    /// 最後に書いた控えの中身の指紋（同じなら書き直さない）。
    unsaved_sig: Cell<u64>,
    /// 本文の値の小窓（peekwin）: 出している値の (文書, 始まり, 終わり) と、マウスが止まっている (x, y, 回数)。
    peek_at: Cell<Option<(u64, i32, i32)>>,
    peek_still: Cell<(i32, i32, u32)>,
    /// マウスが右端（右端の欄の上）にある。
    mouse_right: Cell<bool>,
    /// Alt で両端の欄を開いている（もう一度 Alt で閉じる。保存しない）。
    alt_open: Cell<bool>,
    /// Alt を押してから、ほかのキー・クリックが無い（離したときに開閉する）。
    alt_armed: Cell<bool>,
    /// 右端の欄の情報を最後に作ったときの (文書の id, 文字数, 未保存, 選択の長さ)。変わらなければ作り直さない。
    side_key: Cell<(u64, i32, bool, i32)>,
    /// ツールバーの行番号の固定ボタン（`#`）。
    pin: Button,
    /// 配色の切り替えで色を直すための、ツールバー。
    bar: Flex,
    /// ツールバーの左端の押しピン（ウィンドウを最前面に固定）。
    top_pin: Button,
    /// 再生位置の更新タイマーが動いているか。
    audio_timer: Cell<bool>,
    /// タイトルに出しているカーソルの 行・桁。
    pos_shown: Cell<Option<(usize, usize)>>,
    cursor_pending: Cell<bool>,
    /// 検索バー（ふだんは `Find` ボタンだけ。使うときに開く）。
    find_bar: SearchBar,
    /// いま検索している文字列（検索バーの入力欄と同じ。F3 / Shift+F3 で次・前へ進む）。
    last_search: RefCell<String>,
    /// last_search を大文字小文字をゆるく比べる（スマートケース）か。右クリックの Search は厳密（false）。
    last_smart: Cell<bool>,
    /// このウィンドウがタブの組を config.txt の session に書く役か。引数なしで起動したウィンドウだけ。
    /// ゲームのドロップで起動した（引数ありの）ウィンドウは、保存済みのセッションに触れない。
    session_owner: bool,
    /// Copy to Clipboard History で貯めたテキスト（全文書で共通。この起動中だけ）。
    clip_history: RefCell<History>,
    /// 設定フォルダの config.txt。場所が決められなければ None（保存しない）。
    config_path: Option<PathBuf>,
    /// 選んだフォントのファイル。None は同梱の NeoFont。
    font_path: RefCell<Option<PathBuf>>,
    autosave_armed: Cell<bool>,
    tx: app::Sender<Msg>,
}

/// 画像ファイルの (幅, 高さ)。FLTK で読めなければ None。
fn image_size(p: &Path) -> Option<(i32, i32)> {
    use fltk::image::SharedImage;
    let img = SharedImage::load(p).ok()?;
    Some((img.w(), img.h()))
}

/// 設定ファイルの配色（theme= と custom_*）を選ぶ。知らない名前・色のそろっていない custom は light / dark にする。
fn apply_theme_config(cfg: &config::Config) {
    if let Some([main, sub, accent, text, subtext]) = cfg.custom {
        theme::store_custom(theme::Palette { main, sub, accent, text, subtext });
    }
    if !cfg.theme.as_deref().is_some_and(theme::set_named) {
        theme::set_light(cfg.light);
    }
}

/// 右端の欄に出す、画像・音声の情報（その種類のファイルのときだけ）。
fn file_meta_rows(p: &Path) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if imagefile::is_image_path(p) {
        if let Some((w, h)) = image_size(p) {
            rows.push((crate::lang::tr("画像", "Image").into(), format!("{w} x {h} px")));
        }
    } else if audiometa::supports(p) && std::fs::metadata(p).is_ok_and(|m| m.len() <= AUDIO_INFO_MAX) {
        if let Ok(info) = std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| audiometa::info(&b)) {
            rows.extend(audiometa::info_rows(&info).into_iter().map(|(k, v)| (k.to_string(), v)));
        }
    }
    rows
}

/// URL の画像を一時ファイルに保存して、そのパスを返す。中身が画像でなければ失敗にして消す。
fn fetch_image(url: &str) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind, Read};
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let part = std::env::temp_dir().join(format!("neonote-{}-{stamp}.part", std::process::id()));
    platform::download(url, &part)?;
    let mut head = [0u8; 32];
    let n = std::fs::File::open(&part).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let Some(ext) = imagefile::sniff(&head[..n]) else {
        let _ = std::fs::remove_file(&part);
        return Err(Error::new(ErrorKind::InvalidData, crate::lang::tr("画像ではありませんでした（png / jpg / gif / bmp のみ）", "Not an image (png / jpg / gif / bmp only)")));
    };
    let dest = part.with_extension(ext);
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// メタデータ編集の文書の表示名（`dir/pic.png` → `dir/pic.png.meta`。実在するファイルではない）。
fn meta_label(img: &Path) -> PathBuf {
    let mut n = img.file_name().unwrap_or_default().to_os_string();
    n.push(".meta");
    img.with_file_name(n)
}

/// 音声のタグの書き戻し。先頭・末尾の大きさが変わらないときは、その場で上書きする（大きなファイルを書き直さない）。
/// それ以外は一時ファイルへ書いて置き換える。
fn write_audio_patch(path: &Path, old: &[u8], p: &audiometa::Patch) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    if !p.in_place {
        return document::write_atomic(path, &p.apply(old));
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.write_all(&p.head)?;
    if !p.tail.is_empty() {
        f.seek(SeekFrom::Start(p.tail_from as u64))?;
        f.write_all(&p.tail)?;
    }
    f.sync_all()
}

/// 編集された名前の一覧をフォルダ・zip に適用する。(変えた件数, 適用後の一覧)。
/// 開いたあとに中身が変わっていたら、取り違えを避けるため何もしない。
fn apply_archive_rename(text: &str, root: &Path, old: &[archive::Entry]) -> Result<(usize, Vec<archive::Entry>), String> {
    let new = archive::parse_edit_text(text, old)?;
    apply_archive_names(root, old, new)
}

/// フォルダ・zip の各項目の名前を `new`（`old` と同じ順・同じ数）に変える。(変えた件数, 適用後の一覧)。
fn apply_archive_names(root: &Path, old: &[archive::Entry], new: Vec<String>) -> Result<(usize, Vec<archive::Entry>), String> {
    let now = archive::list_path(root)?;
    if now.iter().map(|e| &e.path).ne(old.iter().map(|e| &e.path)) {
        return Err(crate::lang::tr("開いたあとに、中身が変わっています。いちど開き直してから編集してください。", "The contents changed after opening. Reopen the file before editing.").into());
    }
    let n = archive::changes(old, &new).len();
    if n > 0 {
        if root.is_dir() {
            archive::rename_dir(root, old, &new)?;
        } else {
            let len = std::fs::metadata(root).map_err(|e| format!("{}\n{e}", root.display()))?.len();
            if len > archive::MAX_ZIP {
                return Err(crate::lang::tr("大きすぎる zip です（1 GiB まで）", "Zip is too large (up to 1 GiB)").into());
            }
            let bytes = std::fs::read(root).map_err(|e| format!("{}\n{e}", root.display()))?;
            let out = archive::rename_zip(&bytes, &new)?;
            document::write_atomic(root, &out).map_err(|e| format!("{}\n{e}", root.display()))?;
        }
    }
    Ok((n, archive::list_path(root)?))
}

fn arch_label(root: &Path) -> PathBuf {
    let mut n = root.file_name().unwrap_or_default().to_os_string();
    n.push(".rename");
    root.with_file_name(n)
}

/// リージョンのチャンクの編集のタブ名（`r.0.0.mca [x,z]`）。
fn chunk_label((region, index): &(PathBuf, usize)) -> PathBuf {
    let mut n = region.file_name().unwrap_or_default().to_os_string();
    n.push(format!(" [{},{}]", index % 32, index / 32));
    region.with_file_name(n)
}

fn encoding_label(st: &State) -> String {
    if let Some(f) = &st.nbt {
        return format!("NBT ({})", f.compression.label());
    }
    if st.region_list {
        return "Region".to_string();
    }
    if st.hex {
        if st.hex_truncated { crate::lang::tr("HEX（読み取り専用・一部のみ）", "HEX (read-only, truncated)").to_string() } else { "HEX".to_string() }
    } else if st.bom && !st.encoding.bom_bytes().is_empty() {
        format!("{} BOM", st.encoding.label())
    } else {
        st.encoding.label().to_string()
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

impl Ui {
    // ---- 文書の管理 ----

    /// 表示している文書。
    fn doc(&self) -> Rc<Doc> {
        let id = self.active.get();
        self.find(id).unwrap_or_else(|| Rc::clone(&self.docs.borrow()[0]))
    }

    fn find(&self, id: u64) -> Option<Rc<Doc>> {
        self.docs.borrow().iter().find(|d| d.id == id).cloned()
    }

    /// Preview に使う種類。拡張子で決まるならそれ、文字の文書で形式を選んでいればその形式。
    fn view_kind(&self, d: &Doc) -> viewmode::Kind {
        let k = viewmode::kind_of(d.st.borrow().path.as_deref());
        match (k, d.preview_as.get()) {
            (_, Some(f)) => f,
            (viewmode::Kind::Other, None) if d.format_auto.get() && !d.st.borrow().hex => {
                let buf = &d.buf;
                viewmode::sniff(&buf.text_range(0, buf.length().min(viewmode::SNIFF_BYTES)).unwrap_or_default())
            }
            _ => k,
        }
    }

    fn is_active(&self, d: &Doc) -> bool {
        d.id == self.active.get()
    }

    /// 空の文書（タブ）を作って一覧に加える。表示は切り替えない。
    fn new_doc(self: &Rc<Self>) -> Rc<Doc> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let buf = TextBuffer::default();
        let d = Rc::new(Doc {
            id,
            buf: buf.clone(),
            sbuf: RefCell::new(None),
            st: RefCell::new(State::default()),
            quiet: Cell::new(false),
            cursor: Cell::new(0),
            preview: Cell::new(false),
            preview_as: Cell::new(None),
            format_auto: Cell::new(true),
            pending_restore: RefCell::new(None),
            pdf_text: RefCell::new(None),
            pdf_error: RefCell::new(None),
            pdf_busy: Cell::new(false),
            book: RefCell::new(None),
            book_busy: Cell::new(false),
            doc_text: RefCell::new(None),
            doc_busy: Cell::new(false),
            archive: RefCell::new(None),
            arch_old: RefCell::new(Vec::new()),
            audio: RefCell::new(None),
            audio_busy: Cell::new(false),
            player: RefCell::new(None),
            audio_secs: Cell::new(-1),
            tree_open: RefCell::new(None),
        });
        let u = Rc::clone(self);
        let mut b = buf;
        b.add_modify_callback(move |pos, ins, del, _, _| u.on_modify(id, pos, ins, del));
        self.docs.borrow_mut().push(Rc::clone(&d));
        d
    }

    /// 本文が変わった。変更フラグ・ハイライトの長さ・文字数を合わせる。
    fn on_modify(self: &Rc<Self>, id: u64, pos: i32, ins: i32, del: i32) {
        if ins == 0 && del == 0 {
            return;
        }
        let Some(d) = self.find(id) else { return };
        // ハイライト用のスタイルバッファは、本文と同じ長さを保つ（編集分だけ「通常」を足し引きする）
        if let Some(sb) = d.sbuf.borrow().as_ref() {
            let mut sb = sb.clone();
            if del > 0 {
                sb.remove(pos, pos + del);
            }
            if ins > 0 {
                sb.insert(pos, &"A".repeat(ins as usize));
            }
        }
        if d.quiet.get() {
            return;
        }
        let was_clean = !std::mem::replace(&mut d.st.borrow_mut().dirty, true);
        if was_clean {
            if self.is_active(&d) {
                self.refresh_title();
            }
            self.refresh_tabs();
        }
        if self.is_active(&d) {
            self.schedule_recount();
        }
    }

    /// この文書を表示する。バッファを差し替え、カーソルとハイライトを戻す。
    fn activate(&self, id: u64) {
        let Some(d) = self.find(id) else { return };
        if self.active.get() != id {
            if let Some(old) = self.find(self.active.get()) {
                old.cursor.set(self.editor.insert_position());
            }
        }
        self.active.set(id);
        let mut ed = self.editor.clone();
        self.apply_wrap_for(d.buf.length()); // 付け替える前に（大きい文書を折り返しありで付けると固まる）
        ed.set_buffer(d.buf.clone());
        if self.styled.get() {
            let sb = self.ensure_sbuf(&d);
            ed.set_highlight_data_ext(sb, Self::style_table(self.font.get(), self.size.get()));
        }
        let pos = d.buf.utf8_align(d.cursor.get().clamp(0, d.buf.length()));
        ed.set_insert_position(pos);
        ed.show_insert_position();
        let _ = ed.take_focus();
        ed.redraw();
        self.apply_view(&d);
        self.refresh_title();
        self.refresh_tabs();
        self.refresh_find(); // 検索中なら、切り替えた先の文書にも強調を付ける
    }

    fn activate_relative(&self, delta: isize) {
        let (ids, cur): (Vec<u64>, usize) = {
            let docs = self.docs.borrow();
            let ids: Vec<u64> = docs.iter().map(|d| d.id).collect();
            let cur = ids.iter().position(|i| *i == self.active.get()).unwrap_or(0);
            (ids, cur)
        };
        if ids.len() > 1 {
            let n = ids.len() as isize;
            self.activate(ids[((cur as isize + delta).rem_euclid(n)) as usize]);
        }
    }

    fn new_tab(self: &Rc<Self>) {
        let d = self.new_doc();
        self.activate(d.id);
    }

    /// タブを閉じる。未保存なら 保存 / 破棄 / キャンセル。最後の 1 つなら終了する（メモ帳と同じ）。
    fn close_tab(&self, id: u64) {
        let Some(d) = self.find(id) else { return };
        if self.would_lose(&d) {
            self.activate(id);
            if !self.confirm_discard() {
                return;
            }
        }
        self.remove_doc(id);
        self.backup_unsaved(false); // 破棄したタブの控えを残さない
    }

    /// 確認なしで文書を取り除く。最後の 1 つなら終了する。
    fn remove_doc(&self, id: u64) {
        let (idx, n) = {
            let docs = self.docs.borrow();
            match docs.iter().position(|d| d.id == id) {
                Some(i) => (i, docs.len()),
                None => return,
            }
        };
        if n == 1 {
            // 最後のタブを閉じた: 次の起動で何も開き直さない（×で閉じたときだけ開き直す）
            self.persist_skip(Some(id));
            self.stop_all_audio();
            app::quit();
            return;
        }
        if self.active.get() == id {
            // 隣のタブへ移ってから取り除く（編集領域が消えるバッファを指さないように）
            let next = {
                let docs = self.docs.borrow();
                if idx + 1 < docs.len() { docs[idx + 1].id } else { docs[idx - 1].id }
            };
            self.activate(next);
        }
        let removed = self.docs.borrow_mut().remove(idx);
        drop(removed);
        self.refresh_tabs();
        self.persist();
    }

    /// 終了する。いま開いているタブの組と表示中のタブを config.txt に残してから。
    fn quit(&self) {
        self.stop_all_audio();
        self.persist();
        app::quit();
    }

    /// タブを `to` の位置へ動かす（ドラッグの落とし先）。動かしたタブを表示する。
    fn move_tab(&self, id: u64, to: usize) {
        let moved = {
            let mut docs = self.docs.borrow_mut();
            match docs.iter().position(|d| d.id == id) {
                Some(from) => tabs::reorder(&mut docs, from, to),
                None => false,
            }
        };
        if moved {
            self.persist();
        }
        // ドラッグ中に変えた色・カーソルを元に戻すため、動かなかったときも作り直す
        *self.tabs_shown.borrow_mut() = (Vec::new(), 0);
        self.activate(id);
    }

    /// 表示中のタブを左（-1）・右（+1）へ 1 つ動かす（Ctrl+Shift+PageUp / PageDown）。端では動かない。
    fn move_active(&self, delta: isize) {
        let id = self.active.get();
        let (from, n) = {
            let docs = self.docs.borrow();
            (docs.iter().position(|d| d.id == id).unwrap_or(0), docs.len())
        };
        let to = from as isize + delta;
        if to < 0 || to >= n as isize {
            return;
        }
        self.move_tab(id, to as usize);
    }

    /// 未保存のタブを順に確認する。1 つでもキャンセルされたら false（終了しない）。
    fn confirm_quit(&self) -> bool {
        // 続きからが有効なら聞かない（未保存の変更は控えに書き、次の起動で戻す）
        if self.resume.get() && self.session_owner && self.config_path.is_some() {
            self.backup_unsaved(true);
            return true;
        }
        let dirty: Vec<u64> = self.docs.borrow().iter().filter(|d| self.would_lose(d)).map(|d| d.id).collect();
        for id in dirty {
            self.activate(id);
            if !self.confirm_discard() {
                return false;
            }
        }
        true
    }

    // ---- 表示の更新 ----

    /// カーソルの 行・桁（1 始まり、桁は文字単位）。HEX・読込中・大きすぎる文書では None。
    fn cursor_pos(&self) -> Option<(usize, usize)> {
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.hex || st.loading {
                return None;
            }
        }
        if d.buf.length() > LIVE_COUNT_LIMIT {
            return None;
        }
        let p = d.buf.utf8_align(self.editor.insert_position().clamp(0, d.buf.length()));
        let line = d.buf.count_lines(0, p) as usize + 1;
        let col = d.buf.text_range(d.buf.line_start(p), p).map_or(0, |t| t.chars().count()) + 1;
        Some((line, col))
    }

    /// カーソルが動いたら、少し待ってタイトルの 行・桁 を直す（キーの連打・ドラッグをまとめる）。
    fn schedule_cursor(self: &Rc<Self>) {
        if self.cursor_pending.get() {
            return;
        }
        self.cursor_pending.set(true);
        let ui = Rc::clone(self);
        app::add_timeout3(0.05, move |_| {
            ui.cursor_pending.set(false);
            if ui.cursor_pos() != ui.pos_shown.get() {
                ui.refresh_title();
            }
        });
    }

    fn refresh_title(&self) {
        let d = self.doc();
        let pos = self.cursor_pos();
        let st = d.st.borrow();
        let title = if st.loading {
            format!("{} {}", crate::lang::tr("読み込み中...", "Loading..."), st.loading_path.as_deref().map(|p| log::file_name(&p.to_string_lossy()).to_string()).unwrap_or_default())
        } else {
            window_title(st.path.clone().or(st.meta_of.as_deref().map(meta_label)).or(st.arch_of.as_deref().map(arch_label)).or(st.chunk_of.as_ref().map(chunk_label)).as_deref(), pos, st.chars, &encoding_label(&st), if st.hex { "" } else { st.eol.label() }, st.dirty)
        };
        let chip_label = if st.loading { "…".to_string() } else if st.hex { "-".to_string() } else { encoding_label(&st) };
        let view_now = if st.loading { "…" } else if d.preview.get() { "Preview" } else if st.hex { "HEX" } else { "Text" };
        let chip_tip = if st.loading {
            String::new()
        } else {
            let at = pos.map(|(l, c)| format!("Ln {l}, Col {c}")).unwrap_or_else(|| "Ln -, Col -".into());
            let eol = if st.hex { "-" } else { st.eol.label() };
            let file = st.path.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| crate::lang::tr("未保存のファイル", crate::lang::tr("未保存のファイル", "Unsaved File")).into());
            format!(
                "{file}\n{}: {}\nEOL: {eol}\n{at}\n{}: {}\n{}",
                crate::lang::tr("文字コード", "Encoding"),
                encoding_label(&st),
                crate::lang::tr("文字数", "Chars"),
                st.chars,
                crate::lang::tr("クリックで 文字コード・改行コードを切り替え", "Click to change encoding and line ending")
            )
        };
        drop(st);
        let fmt = if d.format_auto.get() && d.preview_as.get().is_none() { format!("Auto ({view_now})") } else { view_now.to_string() };
        let view_tip = format!("Format: {fmt}\n{}", crate::lang::tr("クリックで 表示（Text・Preview・HEX）と形式を切り替え", "Click to switch view (Text / Preview / HEX) and format"));
        let chip_tip = format!("Encode: {chip_label}\n{chip_tip}");
        {
            let mut chip = self.chip.clone();
            chip.set_tooltip(&chip_tip);
            let mut vc = self.view_chip.clone();
            vc.set_tooltip(&view_tip);
        }
        self.pos_shown.set(pos);
        self.win.clone().set_label(&title);
        self.update_gutter(); // 行数の桁が変わった（タブの切替・読込の後も）
        self.refresh_side();
    }

    /// タブバーを、いまの文書の一覧に合わせる（内容と幅が変わっていなければ何もしない）。
    fn refresh_tabs(&self) {
        let active = self.active.get();
        let infos: Vec<TabInfo> = self
            .docs
            .borrow()
            .iter()
            .map(|d| {
                let st = d.st.borrow();
                let meta = st.meta_of.as_deref().map(meta_label).or_else(|| st.arch_of.as_deref().map(arch_label)).or_else(|| st.chunk_of.as_ref().map(chunk_label));
                let shown = st.path.as_deref().or(st.loading_path.as_deref()).or(meta.as_deref()).or(st.title.as_deref());
                TabInfo {
                    id: d.id,
                    label: tabs::tab_label(shown, st.dirty, st.loading),
                    tip: match (&st.meta_of, &st.arch_of, shown) {
                        (Some(img), _, _) => if crate::lang::is_en() { format!("Metadata of {}", img.display()) } else { format!("{} のメタデータ", img.display()) },
                        (None, Some(root), _) => if crate::lang::is_en() { format!("Rename list of {}", root.display()) } else { format!("{} の名前変更一覧", root.display()) },
                        (None, None, _) if st.chunk_of.is_some() => { let c = st.chunk_of.as_ref().map(|c| c.0.display().to_string()).unwrap_or_default(); if crate::lang::is_en() { format!("Chunk of {c}") } else { format!("{c} のチャンク") } },
                        (None, None, Some(p)) => p.display().to_string(),
                        (None, None, None) => crate::lang::tr("未保存のファイル", "Unsaved File").into(),
                    },
                    active: d.id == active,
                }
            })
            .collect();
        let width = self.win.w() - tabs::CHIP_W - tabs::VIEW_CHIP_W - self.pin.w(); // 左端の押しピンと右端のチップのぶんを除く
        if *self.tabs_shown.borrow() == (infos.clone(), width) {
            return;
        }
        let tx = self.tx.clone();
        let mut bar = self.tabbar.borrow().clone();
        tabs::rebuild(&mut bar, &infos, width, move |c| {
            tx.send(Msg::Tab(c));
            app::awake();
        });
        *self.tabs_shown.borrow_mut() = (infos, width);
    }

    fn recount(&self) {
        let d = self.doc();
        let text = d.buf.text();
        let n = if d.st.borrow().hex { hex::byte_count(&text) } else { document::char_count(&text) };
        d.st.borrow_mut().chars = n;
        self.refresh_title();
        if code::has_syntax(d.st.borrow().path.as_deref()) {
            self.refresh_find(); // 書き換えた所の色分けを付け直す
        }
    }

    /// 入力のたびに全文を数えないよう、0.3 秒に 1 回へまとめる。
    fn schedule_recount(self: &Rc<Self>) {
        if self.doc().buf.length() > LIVE_COUNT_LIMIT || self.count_pending.get() {
            return;
        }
        self.count_pending.set(true);
        let ui = Rc::clone(self);
        app::add_timeout3(0.3, move |_| {
            ui.count_pending.set(false);
            ui.recount();
        });
    }

    fn set_text_quiet(&self, d: &Doc, text: &str) {
        d.quiet.set(true);
        let mut b = d.buf.clone();
        b.can_undo(false); // 読込を「元に戻す」で空にできてしまわないように
        b.set_text(text);
        b.can_undo(true);
        d.quiet.set(false);
    }

    // ---- 開く ----

    /// 非同期読込を始める。この文書が読込中なら受け付けない。
    fn load_into(&self, d: &Rc<Doc>, path: PathBuf, forced: Option<Mode>) {
        if d.st.borrow().loading {
            log::warn(TAG, "読込中のため要求を無視しました");
            return;
        }
        let path = document::absolutize(&path);
        *d.pdf_text.borrow_mut() = None; // 読み直したら、取り出した文字も取り直す
        *d.pdf_error.borrow_mut() = None;
        *d.book.borrow_mut() = None; // シートも読み直す
        *d.doc_text.borrow_mut() = None; // 文書の文字も読み直す
        *d.archive.borrow_mut() = None;
        *d.audio.borrow_mut() = None; // 音声の情報も取り直す。鳴らしていれば止める
        *d.player.borrow_mut() = None;
        if d.preview.get() {
            d.preview.set(false);
            if self.is_active(d) {
                self.apply_view(d);
            }
        }
        log::info(TAG, &format!("open: {}", log::file_name(&path.to_string_lossy())));
        {
            let mut st = d.st.borrow_mut();
            st.loading = true;
            st.loading_path = Some(path.clone());
        }
        if self.is_active(d) {
            self.refresh_title();
        }
        self.refresh_tabs();
        let tx = self.tx.clone();
        let id = d.id;
        // フォルダと zip は中身を読まず、一覧だけを取る（zip として読めないファイルは、ふつうに開く）
        let as_archive = forced.is_none() && archive::is_archive_path(&path);
        // 音声は本文を読まない（大きい・バイナリ）。Preview で情報とタグを別に読む。HEX で見たいときだけ読み直す
        let as_audio = forced.is_none() && !as_archive && viewmode::kind_of(Some(&path)) == viewmode::Kind::Audio;
        thread::spawn(move || {
            if as_audio {
                let result = std::fs::metadata(&path).map_err(|e| format!("{}\n{e}", path.display())).map(|_| document::Loaded {
                    text: String::new(),
                    encoding: Encoding::Utf8,
                    bom: false,
                    auto: true,
                    hex: false,
                    truncated: false,
                    stamp: document::stamp(&path),
                    nbt: None,
                    region: false,
                });
                tx.send(Msg::Loaded(id, path, result));
            } else if as_archive {
                match archive::list_path(&path) {
                    Ok(v) => tx.send(Msg::Archive(id, path, Ok(v))),
                    Err(e) if path.is_dir() => tx.send(Msg::Archive(id, path, Err(e))),
                    Err(_) => {
                        let result = document::load(&path, forced);
                        tx.send(Msg::Loaded(id, path, result))
                    }
                };
            } else {
                let result = document::load(&path, forced);
                tx.send(Msg::Loaded(id, path, result));
            }
            app::awake();
        });
    }

    /// すでに開いている（読込中を含む）ファイルの文書。
    fn find_by_path(&self, path: &Path) -> Option<Rc<Doc>> {
        self.docs
            .borrow()
            .iter()
            .find(|d| {
                let st = d.st.borrow();
                let hit = [st.path.as_deref(), st.loading_path.as_deref()].into_iter().flatten().any(|p| document::same_file(p, path));
                hit
            })
            .cloned()
    }

    /// ファイルを新しいタブで開く。すでに開いていればそのタブ、今のタブが空の新規ならそのタブを使う。
    /// 表示は切り替えない（呼び出し側で activate する）。戻り値は使った文書の id。
    fn open_path(self: &Rc<Self>, path: PathBuf) -> u64 {
        let path = document::absolutize(&path);
        if let Some(d) = self.find_by_path(&path) {
            return d.id;
        }
        let cur = self.doc();
        let d = if cur.is_pristine() { cur } else { self.new_doc() };
        self.load_into(&d, path, None);
        d.id
    }

    /// 複数のパスをそれぞれタブで開き、最初のものを表示する。
    fn open_paths(self: &Rc<Self>, paths: Vec<PathBuf>) {
        let mut first = None;
        for p in paths {
            let id = self.open_path(p);
            first.get_or_insert(id);
        }
        if let Some(id) = first {
            self.activate(id);
        }
    }

    fn unsaved_dir(&self) -> Option<PathBuf> {
        self.config_path.as_deref().and_then(Path::parent).map(Path::to_path_buf)
    }

    /// 未保存のタブの本文を控えに書く（続きからが有効で、引数なしで起動したウィンドウだけ）。
    /// `force` でなければ、前回書いたときから変わっていないと書かない。
    fn backup_unsaved(&self, force: bool) {
        if !self.resume.get() || !self.session_owner {
            return;
        }
        let Some(dir) = self.unsaved_dir() else { return };
        let mut entries = Vec::new();
        for d in self.docs.borrow().iter() {
            let st = d.st.borrow();
            if !st.dirty || st.loading || st.hex || d.buf.length() as usize > unsaved::MAX_TEXT {
                continue;
            }
            entries.push(unsaved::Entry { path: st.path.clone(), text: d.buf.text() });
        }
        let sig = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            entries.len().hash(&mut h);
            for e in &entries {
                e.path.hash(&mut h);
                e.text.hash(&mut h);
            }
            h.finish()
        };
        if !force && sig == self.unsaved_sig.get() {
            return;
        }
        match unsaved::save(&dir, &entries) {
            Ok(()) => self.unsaved_sig.set(sig),
            Err(e) => log::warn(TAG, &format!("未保存の変更を控えられませんでした: {e}")),
        }
    }

    /// UNSAVED_SECS ごとに控えを書く。
    fn arm_unsaved(self: &Rc<Self>) {
        let u = Rc::clone(self);
        app::add_timeout3(UNSAVED_SECS, move |_| {
            u.backup_unsaved(false);
            u.arm_unsaved();
        });
    }

    fn toggle_resume(&self) {
        let on = !self.resume.get();
        self.resume.set(on);
        menu::set_resume_checked(on);
        if on {
            self.backup_unsaved(true);
        } else if let Some(dir) = self.unsaved_dir() {
            let _ = unsaved::clear(&dir);
            self.unsaved_sig.set(0);
        }
        self.persist();
        self.flash(if on { crate::lang::tr("再開: オン", "Resume: on") } else { crate::lang::tr("再開: オフ", "Resume: off") });
    }

    /// 前回の未保存の変更を戻す。元のファイルがあるタブは読み込み後に本文を差し替え、無ければ名前の無いタブにする。
    /// 戻したタブの数と、最初に戻したタブ。
    fn restore_unsaved(self: &Rc<Self>) -> (usize, Option<u64>) {
        let Some(dir) = self.unsaved_dir() else { return (0, None) };
        let entries = unsaved::load(&dir);
        let n = entries.len();
        let mut first = None;
        for e in entries {
            let target = match &e.path {
                Some(p) if p.exists() => Some(self.find_by_path(p).unwrap_or_else(|| {
                    let id = self.open_path(p.clone());
                    self.find(id).expect("開いたタブ")
                })),
                _ => None,
            };
            match target {
                Some(d) => {
                    first.get_or_insert(d.id);
                    if d.st.borrow().loading {
                        *d.pending_restore.borrow_mut() = Some(e.text);
                    } else {
                        d.buf.clone().set_text(&e.text);
                    }
                }
                None => {
                    // 起動したときの空のタブ（名前も中身も変更も無い）があれば、それに入れる
                    let blank = self.docs.borrow().iter().find(|d| {
                        let st = d.st.borrow();
                        st.path.is_none() && !st.dirty && !st.loading && d.buf.length() == 0 && d.pending_restore.borrow().is_none()
                    }).cloned();
                    let d = blank.unwrap_or_else(|| self.new_doc());
                    first.get_or_insert(d.id);
                    d.buf.clone().set_text(&e.text); // 変更として入れる（未保存の印が付く）
                    if let Some(p) = &e.path {
                        log::warn(TAG, &format!("unsaved: 元のファイルが無いため、名前の無いタブにしました: {}", log::file_name(&p.to_string_lossy())));
                    }
                }
            }
        }
        if n > 0 {
            log::info(TAG, &format!("unsaved: {n} tabs restored"));
        }
        (n, first)
    }

    /// 前回の終了時に開いていたタブを開き直す（引数なしで起動したときだけ）。無くなったファイルは飛ばす。
    /// 表示するのは、前回表示していたタブ（無ければ最初のタブ）。
    fn restore_session(self: &Rc<Self>, cfg: &config::Config) {
        if !cfg.resume {
            return;
        }
        let mut ids = Vec::new();
        let mut active = None;
        let mut missing = 0;
        for p in &cfg.session {
            if self.is_config_file(p) {
                continue;
            }
            if !p.exists() {
                missing += 1;
                log::warn(TAG, &format!("session: 見つからないため飛ばしました: {}", log::file_name(&p.to_string_lossy())));
                continue;
            }
            let id = self.open_path(p.clone());
            if cfg.session_active.as_deref().is_some_and(|a| document::same_file(a, p)) {
                active = Some(id);
            }
            ids.push(id);
        }
        let (restored, first_restored) = self.restore_unsaved();
        self.refresh_tabs();
        if let Some(id) = active.or_else(|| ids.first().copied()).or(first_restored) {
            self.activate(id);
        }
        if restored > 0 {
            self.flash(&if crate::lang::is_en() { format!("Restored {restored} unsaved tabs") } else { format!("未保存のタブを {restored} 件復元しました") });
        } else if !ids.is_empty() {
            log::info(TAG, &format!("session: {} tabs restored ({missing} missing)", ids.len()));
            self.flash(&if missing > 0 { if crate::lang::is_en() { format!("Restored {} tabs ({missing} missing)", ids.len()) } else { format!("{} 件のタブを復元しました（{missing} 件は見つかりませんでした）", ids.len()) } } else { if crate::lang::is_en() { format!("Restored {} tabs", ids.len()) } else { format!("{} 件のタブを復元しました", ids.len()) } });
        }
    }

    fn on_loaded(&self, id: u64, path: PathBuf, result: Result<document::Loaded, String>) {
        let Some(d) = self.find(id) else { return }; // 読込中にタブが閉じられた
        match result {
            Ok(loaded) => {
                let path_for_view = path.clone();
                // CRLF だけの文書は LF にして持つ（FLTK は \r を印として描くため）。保存のときに CRLF へ戻す
                let eol = if loaded.hex { Eol::platform_default() } else { eol::detect(&loaded.text, Eol::platform_default()) };
                let text = if !loaded.hex && eol == Eol::Crlf { eol::convert(&loaded.text, Eol::Lf) } else { loaded.text };
                if self.is_active(&d) {
                    self.apply_wrap_for(text.len() as i32);
                }
                self.set_text_quiet(&d, &text);
                log::info(
                    TAG,
                    &format!("encoding: {}{}{}", loaded.encoding.label(), if loaded.bom { " (BOM)" } else { "" }, if loaded.auto { " [auto]" } else { "" }),
                );
                let mut st = d.st.borrow_mut();
                st.loading = false;
                st.loading_path = None;
                st.dirty = false;
                st.path = Some(path.clone());
                st.encoding = loaded.encoding;
                st.bom = loaded.bom;
                st.hex = loaded.hex;
                st.hex_truncated = loaded.truncated;
                st.nbt = loaded.nbt;
                st.region_list = loaded.region;
                st.chunk_of = None;
                st.auto = loaded.auto;
                st.chars = if loaded.hex { hex::byte_count(&text) } else { document::char_count(&text) };
                st.eol = eol;
                st.stamp = loaded.stamp;
                st.conflict_noted = false;
                drop(st);
                if let Some(t) = d.pending_restore.borrow_mut().take() {
                    d.buf.clone().set_text(&t); // 前回の未保存の変更（未保存の印が付く）
                }
                self.remember_recent(path);
                self.refresh_tabs();
                self.refresh_title_if_active(&d);
                if self.is_active(&d) {
                    self.refresh_find();
                }
                // pdf は HEX や文字化けで読むより文字で見たいので、自動判定で開いたときは Preview で始める
                if loaded.auto && matches!(viewmode::kind_of(Some(&path_for_view)), viewmode::Kind::Book | viewmode::Kind::Doc | viewmode::Kind::Pdf | viewmode::Kind::Image | viewmode::Kind::Audio) {
                    self.set_preview_for(&d, true);
                }
                // Markdown は、Format が Auto なら Markdown の表示で始める（編集は Format の Text で）
                if !loaded.hex && d.format_auto.get() && d.preview_as.get().is_none() && self.view_kind(&d) == viewmode::Kind::Markdown {
                    self.set_preview_for(&d, true);
                }
                if loaded.truncated {
                    log::warn(TAG, "HEX: 上限を超えたため先頭だけ読みました（保存は無効）");
                    let mib = hex::MAX_BYTES / 1024 / 1024;
                    confirm::alert(&if crate::lang::is_en() { format!("The file is large, so only the first {mib} MiB is shown in HEX.\nSaving is disabled for this file so it is not damaged.") } else { format!("ファイルが大きいため、先頭の {mib} MiB だけを HEX で表示しています。\n壊さないよう、このファイルへの保存は無効です。") });
                }
            }
            Err(msg) => {
                log::error(TAG, &format!("読込に失敗: {}", msg.replace('\n', " / ")));
                {
                    let mut st = d.st.borrow_mut();
                    st.loading = false;
                    st.loading_path = None; // 現在のファイルとその内容はそのまま残す
                }
                // 開くために作った（まだ何も入っていない）タブは、失敗したら閉じる。最後の 1 つは残す。
                if d.is_pristine() && self.docs.borrow().len() > 1 {
                    self.remove_doc(id);
                } else {
                    self.refresh_tabs();
                }
                self.refresh_title(); // モーダルなダイアログの間に "Loading..." を残さない
                confirm::alert(&if crate::lang::is_en() { format!("Could not open the file.\n\n{msg}") } else { format!("ファイルを開けませんでした。\n\n{msg}") });
            }
        }
        self.refresh_title();
    }

    /// フォルダ・zip の一覧が読めた。本文は空のまま、Preview（階層ツリー）で見せる。
    fn on_archive(&self, id: u64, path: PathBuf, result: Result<Vec<archive::Entry>, String>) {
        let Some(d) = self.find(id) else { return };
        match result {
            Ok(entries) => {
                log::info(TAG, &format!("archive: {} ({} entries)", log::file_name(&path.to_string_lossy()), entries.len()));
                self.set_text_quiet(&d, "");
                *d.archive.borrow_mut() = Some(entries);
                {
                    let mut st = d.st.borrow_mut();
                    st.loading = false;
                    st.loading_path = None;
                    st.dirty = false;
                    st.path = Some(path.clone());
                    st.hex = false;
                    st.hex_truncated = false;
                    st.auto = true;
                    st.chars = 0;
                    st.stamp = None;
                    st.conflict_noted = false;
                }
                self.remember_recent(path);
                self.refresh_tabs();
                self.set_preview_for(&d, true);
                self.refresh_title_if_active(&d);
                if self.is_active(&d) {
                    // 一覧の本文は空で、空のタブと見分けが付かないので、右パネル（面積図）を必ず作り直す
                    self.side_key.set((0, -1, false, 0));
                    self.refresh_side();
                }
            }
            Err(msg) => {
                log::error(TAG, &format!("一覧の読込に失敗: {}", msg.replace('\n', " / ")));
                {
                    let mut st = d.st.borrow_mut();
                    st.loading = false;
                    st.loading_path = None;
                }
                if d.is_pristine() && self.docs.borrow().len() > 1 {
                    self.remove_doc(id);
                } else {
                    self.refresh_tabs();
                }
                self.refresh_title();
                confirm::alert(&if crate::lang::is_en() { format!("Could not open the folder.\n\n{msg}") } else { format!("フォルダを開けませんでした。\n\n{msg}") });
            }
        }
        self.refresh_title();
    }

    fn refresh_title_if_active(&self, d: &Doc) {
        if self.is_active(d) {
            self.refresh_title();
        }
    }

    /// 保存 / 破棄 / キャンセルの3択（表示中の文書について）。続行してよければ true。
    /// 閉じると本当に失う変更があるか。名前の無いタブは空白だけなら無い。
    /// ファイルのタブは、保存したときに書かれる中身が今のファイルと同じなら無い（打ってから元に戻したなど）。
    fn would_lose(&self, d: &Doc) -> bool {
        let st = d.st.borrow();
        if !st.dirty {
            return false;
        }
        let text = d.buf.text();
        let Some(path) = st.path.as_deref() else { return !text.trim().is_empty() };
        if st.hex || st.meta_of.is_some() || st.arch_of.is_some() || st.chunk_of.is_some() || text.len() > unsaved::MAX_TEXT {
            return true; // 書き出し方が違う文書・大きな文書は、比べずに確かめる
        }
        let text = if st.eol == Eol::Crlf { eol::convert(&text, Eol::Crlf) } else { text };
        let encoded = document::encode_for_save(&text, st.encoding, st.bom);
        encoded.lossy_count > 0 || std::fs::read(path).map_or(true, |disk| disk != encoded.bytes)
    }

    fn confirm_discard(&self) -> bool {
        let name = self.doc().st.borrow().path.as_deref().map_or_else(|| crate::lang::tr("無題", "Untitled").to_string(), |p| log::file_name(&p.to_string_lossy()).to_string());
        self.confirm_discard_msg(&if crate::lang::is_en() { format!("\"{name}\" has unsaved changes.") } else { format!("「{name}」の変更が保存されていません。") })
    }

    fn confirm_discard_msg(&self, msg: &str) -> bool {
        if !self.would_lose(&self.doc()) {
            return true;
        }
        match confirm::choice(msg, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("保存", "Save"), crate::lang::tr("破棄", "Discard")) {
            Some(1) => self.save(false),
            Some(2) => true,
            _ => false,
        }
    }

    // ---- 保存 ----

    /// 保存（表示中の文書）。成功したら true。`as_new` なら保存先を選び直す。
    /// 画像のメタデータの文書を保存する: 画像を読み直し、メタデータの部分だけ差し替えて書き戻す。
    fn save_meta(&self, d: &Rc<Doc>, img: PathBuf) -> bool {
        let audio = audiometa::supports(&img);
        if audio {
            self.release_player_for(&img);
        }
        let r = std::fs::read(&img).map_err(|e| format!("{}\n{e}", img.display())).and_then(|b| {
            let text = d.buf.text();
            if audio {
                let patch = audiometa::write(&b, &text)?;
                write_audio_patch(&img, &b, &patch).map_err(|e| format!("{}\n{e}", img.display()))
            } else {
                let out = imgmeta::write(&b, &text)?;
                document::write_atomic(&img, &out).map_err(|e| format!("{}\n{e}", img.display()))
            }
        });
        match r {
            Ok(()) => {
                if audio {
                    self.refresh_audio_views(&img);
                }
                log::info(TAG, &format!("metadata saved: {}", log::file_name(&img.to_string_lossy())));
                d.st.borrow_mut().dirty = false;
                self.refresh_tabs();
                self.refresh_title();
                self.flash(crate::lang::tr("メタデータを保存しました", "Metadata saved"));
                true
            }
            Err(e) => {
                log::warn(TAG, &format!("metadata save failed: {}", e.replace('\n', " / ")));
                confirm::alert(&if crate::lang::is_en() { format!("Could not save the metadata.\n\n{e}") } else { format!("メタデータを保存できませんでした。\n\n{e}") });
                false
            }
        }
    }

    /// 画像のメタデータを、文字の新しいタブで開く（PNG の tEXt / iTXt、JPEG のコメント）。
    fn open_meta(self: &Rc<Self>) {
        let Some(img) = self.doc().st.borrow().path.clone() else { return };
        // 同じ画像のメタデータをすでに開いていれば、そのタブへ移る
        let existing = self.docs.borrow().iter().find(|x| x.st.borrow().meta_of.as_deref() == Some(img.as_path())).map(|x| x.id);
        if let Some(id) = existing {
            self.activate(id);
            return;
        }
        let r = std::fs::read(&img).map_err(|e| e.to_string()).and_then(|b| {
            if audiometa::supports(&img) {
                audiometa::read(&b).map(|(_, t)| t)
            } else {
                imgmeta::read(&b).map(|(_, t)| t)
            }
        });
        match r {
            Ok(text) => {
                let d = self.new_doc();
                self.set_text_quiet(&d, &text);
                {
                    let mut st = d.st.borrow_mut();
                    st.meta_of = Some(img);
                    st.chars = document::char_count(&text);
                }
                self.activate(d.id);
            }
            Err(e) => confirm::alert(&if crate::lang::is_en() { format!("Could not read the metadata.\n\n{e}") } else { format!("メタデータを読めませんでした。\n\n{e}") }),
        }
    }

    /// フォルダ・zip の名前の一覧（1 行 1 パス）を、文字の新しいタブで開く。編集して保存すると名前を変更する。
    fn open_archive_edit(self: &Rc<Self>) {
        let d = self.doc();
        let Some(root) = d.st.borrow().path.clone() else { return };
        if !archive::can_rename(&root) {
            self.flash(crate::lang::tr("tar・gz は一覧を見るだけです（名前の変更は zip とフォルダだけ）", "tar / gz files can only be listed (renaming works for zip and folders only)"));
            return;
        }
        let Some(entries) = d.archive.borrow().clone() else { return };
        let existing = self.docs.borrow().iter().find(|x| x.st.borrow().arch_of.as_deref() == Some(root.as_path())).map(|x| x.id);
        if let Some(id) = existing {
            self.activate(id);
            return;
        }
        let text = archive::to_edit_text(&entries);
        let nd = self.new_doc();
        self.set_text_quiet(&nd, &text);
        *nd.arch_old.borrow_mut() = entries;
        {
            let mut st = nd.st.borrow_mut();
            st.arch_of = Some(root);
            st.chars = document::char_count(&text);
        }
        self.activate(nd.id);
    }

    /// 名前の一覧を、フォルダ・zip に適用する。成功したら、開いている一覧と編集中の文字も新しい状態にそろえる。
    fn save_archive(&self, d: &Rc<Doc>, root: PathBuf) -> bool {
        let old = d.arch_old.borrow().clone();
        match apply_archive_rename(&d.buf.text(), &root, &old) {
            Ok((n, fresh)) => {
                log::info(TAG, &format!("renamed {n} in {}", log::file_name(&root.to_string_lossy())));
                let viewers: Vec<Rc<Doc>> = self
                    .docs
                    .borrow()
                    .iter()
                    .filter(|x| x.archive.borrow().is_some() && x.st.borrow().path.as_deref().is_some_and(|p| document::same_file(p, &root)))
                    .cloned()
                    .collect();
                for v in viewers {
                    *v.archive.borrow_mut() = Some(fresh.clone());
                    if v.preview.get() && self.is_active(&v) {
                        self.render_preview(&v);
                    }
                }
                let text = archive::to_edit_text(&fresh);
                self.set_text_quiet(d, &text);
                *d.arch_old.borrow_mut() = fresh;
                {
                    let mut st = d.st.borrow_mut();
                    st.dirty = false;
                    st.chars = document::char_count(&text);
                }
                self.refresh_tabs();
                self.refresh_title();
                self.flash(&if n == 0 { crate::lang::tr("変更はありません", "No changes").to_string() } else { if crate::lang::is_en() { format!("Renamed {n}") } else { format!("{n} 件の名前を変更しました") } });
                true
            }
            Err(e) => {
                log::warn(TAG, &format!("rename failed: {}", e.replace('\n', " / ")));
                confirm::alert(&if crate::lang::is_en() { format!("Could not rename.\n\n{e}") } else { format!("名前を変更できませんでした。\n\n{e}") });
                false
            }
        }
    }

    /// pdf から取り出した文字を、編集できる新しいタブで開く（元の pdf は変えない。保存は .txt などの別ファイル）。
    fn open_pdf_text(self: &Rc<Self>) {
        let d = self.doc();
        let text = match d.pdf_text.borrow().as_ref() {
            Some(p) => p.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n"),
            None => {
                self.flash(if d.pdf_busy.get() { crate::lang::tr("取り出し中です", "Extracting text...") } else { crate::lang::tr("文字を取り出せていません", "Could not extract text") });
                return;
            }
        };
        let n = self.new_doc();
        self.set_text_quiet(&n, &text);
        n.st.borrow_mut().chars = document::char_count(&text);
        self.activate(n.id);
    }

    /// docx / odt / pptx / odp / epub の文字を、新しいタブに文字として出す（編集用の写し。元のファイルは変えない）。
    fn open_doc_text(self: &Rc<Self>) {
        let d = self.doc();
        let text = match d.doc_text.borrow().as_ref() {
            Some(Ok(t)) => doctext::to_plain(t),
            _ => {
                self.flash(if d.doc_busy.get() { crate::lang::tr("読み込み中です", "Loading...") } else { crate::lang::tr("文字を取り出せていません", "Could not extract text") });
                return;
            }
        };
        let n = self.new_doc();
        self.set_text_quiet(&n, &text);
        n.st.borrow_mut().chars = document::char_count(&text);
        self.activate(n.id);
    }

    /// SNBT の文字を NBT に戻す。書き間違いなら、その場所へカーソルを移して知らせる（None）。
    fn snbt_to_raw(&self, d: &Rc<Doc>) -> Option<Vec<u8>> {
        let file = d.st.borrow().nbt.clone()?;
        let text = d.buf.text();
        match nbt::save_raw(&file, &text) {
            Ok(raw) => Some(raw),
            Err(e) => {
                let at = goto::offset(&text, e.line, Some(e.col)) as i32;
                let mut ed = self.editor.clone();
                ed.set_insert_position(at);
                ed.show_insert_position();
                confirm::alert(&if crate::lang::is_en() { format!("Not saved because the SNBT syntax is wrong.\n{e}") } else { format!("SNBT の書き方が違うため、保存しませんでした。\n{e}") });
                None
            }
        }
    }

    /// NBT のファイル（level.dat など）を、開いたときと同じ包み方（gzip など）で書く。
    fn save_nbt(&self, d: &Rc<Doc>, target: PathBuf) -> bool {
        let Some(raw) = self.snbt_to_raw(d) else { return false };
        let compression = d.st.borrow().nbt.as_ref().map_or(nbt::Compression::Gzip, |f| f.compression);
        match document::write_atomic(&target, &nbt::pack(compression, &raw)) {
            Ok(()) => {
                log::info(TAG, &format!("saved NBT: {} ({})", log::file_name(&target.to_string_lossy()), compression.label()));
                self.remember_recent(target.clone());
                {
                    let stamp = document::stamp(&target);
                    let mut st = d.st.borrow_mut();
                    st.path = Some(target);
                    st.dirty = false;
                    st.stamp = stamp;
                    st.conflict_noted = false;
                }
                self.refresh_tabs();
                self.recount();
                self.persist();
                true
            }
            Err(e) => {
                confirm::alert(&if crate::lang::is_en() { format!("Could not save.\n{}\n{e}", target.display()) } else { format!("保存できませんでした。\n{}\n{e}", target.display()) });
                false
            }
        }
    }

    /// リージョンのチャンク 1 つを書き換える（ほかのチャンクはそのまま。ファイルは読み直してから書く）。
    fn save_chunk(&self, d: &Rc<Doc>, region_path: &Path, index: usize) -> bool {
        let Some(raw) = self.snbt_to_raw(d) else { return false };
        let compression = d.st.borrow().nbt.as_ref().map_or(nbt::Compression::Zlib, |f| f.compression);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32);
        let r = std::fs::read(region_path)
            .map_err(|e| e.to_string())
            .and_then(|b| region::replace_chunk(&b, index, compression, &raw, now))
            .and_then(|out| document::write_atomic(region_path, &out).map_err(|e| e.to_string()));
        match r {
            Ok(()) => {
                log::info(TAG, &format!("saved chunk {},{} of {}", index % 32, index / 32, log::file_name(&region_path.to_string_lossy())));
                d.st.borrow_mut().dirty = false;
                self.refresh_tabs();
                self.recount();
                self.flash(crate::lang::tr("チャンクを保存しました", "Chunk saved"));
                true
            }
            Err(e) => {
                confirm::alert(&if crate::lang::is_en() { format!("Could not save the chunk.\n{}\n{e}", region_path.display()) } else { format!("チャンクを保存できませんでした。\n{}\n{e}", region_path.display()) });
                false
            }
        }
    }

    /// チャンクの一覧の今の行のチャンクを、新しいタブで開く（もう開いていれば、そのタブへ）。一覧でなければ false。
    fn open_chunk_at_cursor(self: &Rc<Self>) -> bool {
        let d = self.doc();
        let (is_list, path) = {
            let st = d.st.borrow();
            (st.region_list, st.path.clone())
        };
        let (true, Some(path)) = (is_list, path) else { return false };
        let line = d.buf.line_text(self.editor.insert_position());
        let Some((x, z)) = region::parse_listing_line(&line) else { return true };
        let index = (z * 32 + x) as usize;
        let existing = self.docs.borrow().iter().find(|o| o.st.borrow().chunk_of.as_ref() == Some(&(path.clone(), index))).map(|o| o.id);
        if let Some(id) = existing {
            self.activate(id);
            return true;
        }
        let opened = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
            let e = region::list(&b)?.into_iter().find(|e| e.index() == index).ok_or(crate::lang::tr("チャンクがありません", "No such chunk"))?;
            let (c, raw) = region::read_chunk(&b, &e)?;
            nbt::open_raw(c, &raw)
        });
        match opened {
            Ok((file, text)) => {
                let n = self.new_doc();
                self.set_text_quiet(&n, &text);
                {
                    let mut st = n.st.borrow_mut();
                    st.nbt = Some(file);
                    st.chunk_of = Some((path, index));
                    st.eol = Eol::Lf;
                    st.chars = document::char_count(&text);
                }
                self.activate(n.id);
            }
            Err(e) => confirm::alert(&if crate::lang::is_en() { format!("Could not open chunk {x},{z}.\n{e}") } else { format!("チャンク {x},{z} を開けませんでした。\n{e}") }),
        }
        true
    }

    fn save(&self, as_new: bool) -> bool {
        let d = self.doc();
        if d.st.borrow().loading {
            return false;
        }
        {
            // 音声の Preview は本文を持たない。空の本文で音声を上書きしないよう、保存を止める
            let st = d.st.borrow();
            if d.preview.get() && !st.hex && viewmode::kind_of(st.path.as_deref()) == viewmode::Kind::Audio {
                drop(st);
                self.flash(crate::lang::tr("音声のプレビューは読み取り専用です（Edit Metadata (text) を使ってください）", "Audio preview is read-only (use Edit Metadata (text))"));
                return false;
            }
        }
        if as_new {
            let mut st = d.st.borrow_mut();
            st.meta_of = None; // 名前を付けて保存は、ふつうの文字として別ファイルに書く
            st.arch_of = None;
        }
        if d.archive.borrow().is_some() {
            self.flash(crate::lang::tr("フォルダ・zip のプレビューは読み取り専用です（Rename (text) を使ってください）", "Folder / zip preview is read-only (use Rename (text))"));
            return false;
        }
        let arch = d.st.borrow().arch_of.clone();
        if let Some(root) = arch {
            return self.save_archive(&d, root);
        }
        let meta = d.st.borrow().meta_of.clone();
        if let Some(img) = meta {
            return self.save_meta(&d, img);
        }
        let chunk = d.st.borrow().chunk_of.clone();
        if let Some((region, index)) = chunk {
            if !as_new {
                return self.save_chunk(&d, &region, index);
            }
            d.st.borrow_mut().chunk_of = None; // 名前を付けて保存は、チャンクを 1 つの NBT ファイルとして書く
        }
        if d.st.borrow().region_list {
            if !as_new {
                self.flash(crate::lang::tr("チャンクの一覧は保存しません（行で Enter かダブルクリックで、チャンクを開いて編集）", "The chunk list is not saved (press Enter or double-click a row to open and edit the chunk)"));
                return false;
            }
            d.st.borrow_mut().region_list = false;
        }
        let current = d.st.borrow().path.clone();
        let target = match current {
            Some(p) if !as_new => p,
            _ => match self.choose_file(FileDialogType::BrowseSaveFile) {
                Some(p) => p,
                None => return false,
            },
        };
        if self.disk_changed(&d, &target) {
            log::warn(TAG, &format!("changed on disk: {}", log::file_name(&target.to_string_lossy())));
            let msg = crate::lang::tr("このファイルは、開いた後にディスク上で書き換えられています。\nそのまま保存すると、その変更を上書きします。", "This file was changed on disk after it was opened.\nSaving as is will overwrite those changes.");
            // Enter で選ばれる真ん中のボタンは、相手の変更を消さない「別名で保存」にする（Esc はキャンセル）
            match confirm::choice(msg, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("別名で保存", "Save As"), crate::lang::tr("上書き保存", "Overwrite")) {
                Some(1) => return self.save(true),
                Some(2) => {}
                _ => return false,
            }
        }
        if d.st.borrow().hex {
            return self.save_hex(&d, target);
        }
        if d.st.borrow().nbt.is_some() {
            return self.save_nbt(&d, target);
        }
        let (enc, bom, eol) = {
            let st = d.st.borrow();
            (st.encoding, st.bom, st.eol)
        };
        let text = if eol == Eol::Crlf { eol::convert(&d.buf.text(), Eol::Crlf) } else { d.buf.text() };
        let mut encoded = document::encode_for_save(&text, enc, bom);
        let mut enc = enc;
        let mut bom = bom;
        if encoded.lossy_count > 0 {
            // 黙って「?」にしない。保存前に必ず確認する。
            let first = encoded.first.as_ref().map(|u| if crate::lang::is_en() { format!(" (first at line {} col {}: \"{}\")", u.line, u.col, u.ch) } else { format!("（最初は {} 行 {} 列の「{}」）", u.line, u.col, u.ch) }).unwrap_or_default();
            let msg = if crate::lang::is_en() { format!("{} characters cannot be saved in {}{}.", encoded.lossy_count, enc.label(), first) } else { format!("{} で表せない文字が {} 個あります{}。", enc.label(), encoded.lossy_count, first) };
            log::warn(TAG, &format!("{} unmappable chars for {}", encoded.lossy_count, enc.label()));
            match confirm::choice(&msg, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("UTF-8 に変えて保存", "Save as UTF-8"), crate::lang::tr("? に置き換えて保存", "Save with ? replaced")) {
                Some(1) => {
                    enc = Encoding::Utf8;
                    bom = false;
                    encoded = document::encode_for_save(&text, enc, bom);
                }
                Some(2) => {}
                _ => return false,
            }
        }
        match document::write_atomic(&target, &encoded.bytes) {
            Ok(()) => {
                log::info(TAG, &format!("saved: {} ({})", log::file_name(&target.to_string_lossy()), enc.label()));
                self.remember_recent(target.clone());
                if self.is_config_file(&target) {
                    // 設定ファイル自身を保存した: メインループで読み直して反映する
                    self.tx.send(Msg::ConfigSaved);
                    app::awake();
                }
                {
                    let stamp = document::stamp(&target);
                    let mut st = d.st.borrow_mut();
                    st.path = Some(target);
                    st.encoding = enc;
                    st.bom = bom;
                    st.dirty = false;
                    st.stamp = stamp;
                    st.conflict_noted = false;
                }
                self.refresh_tabs();
                self.recount();
                if !self.is_config_file(d.st.borrow().path.as_deref().unwrap_or(Path::new(""))) {
                    self.persist(); // 名前を付けて保存でタブのパスが決まった（session に入れる）
                }
                true
            }
            Err(e) => {
                log::error(TAG, &format!("保存に失敗: {e}"));
                confirm::alert(&if crate::lang::is_en() { format!("Could not save.\n\n{}\n{e}", target.display()) } else { format!("保存できませんでした。\n\n{}\n{e}", target.display()) });
                false
            }
        }
    }

    /// 開いた後に、保存先がディスク上で書き換えられたか（同じファイルへ保存するときだけ）。
    fn disk_changed(&self, d: &Doc, target: &Path) -> bool {
        let st = d.st.borrow();
        match st.path.as_deref() {
            Some(p) if document::same_file(p, target) => document::changed_on_disk(target, st.stamp),
            _ => false,
        }
    }

    /// HEX の編集結果をバイト列として保存する。不正な箇所があれば書かずに、そこへカーソルを移す。
    fn save_hex(&self, d: &Rc<Doc>, target: PathBuf) -> bool {
        if d.st.borrow().hex_truncated {
            confirm::alert(crate::lang::tr("先頭だけを読み込んだファイルなので、保存できません（元のファイルを壊さないため）。", "This file was only partly loaded, so it cannot be saved (to protect the original)."));
            return false;
        }
        let text = d.buf.text();
        let bytes = match hex::parse(&text) {
            Ok(b) => b,
            Err(e) => {
                let mut ed = self.editor.clone();
                ed.set_insert_position(e.offset as i32);
                ed.show_insert_position();
                let _ = ed.take_focus();
                log::warn(TAG, &format!("HEX 不正: {} 行 {} 列", e.line, e.col));
                confirm::alert(&if crate::lang::is_en() { format!("Cannot save. Line {} col {} \"{}\"\n{}.\n(The file was not changed.)", e.line, e.col, e.token, e.reason) } else { format!("保存できません。{} 行 {} 列「{}」\n{}。\n（ファイルは書き換えていません）", e.line, e.col, e.token, e.reason) });
                return false;
            }
        };
        match document::write_atomic(&target, &bytes) {
            Ok(()) => {
                log::info(TAG, &format!("saved: {} (HEX, {} bytes)", log::file_name(&target.to_string_lossy()), bytes.len()));
                let pos = self.editor.insert_position();
                {
                    let stamp = document::stamp(&target);
                    let mut st = d.st.borrow_mut();
                    st.path = Some(target);
                    st.dirty = false;
                    st.stamp = stamp;
                    st.conflict_noted = false;
                }
                // オフセットと ASCII 欄を作り直す
                self.set_text_quiet(d, &hex::format(&bytes));
                let mut ed = self.editor.clone();
                ed.set_insert_position(d.buf.utf8_align(pos.min(d.buf.length())));
                self.refresh_tabs();
                self.recount();
                true
            }
            Err(e) => {
                log::error(TAG, &format!("保存に失敗: {e}"));
                confirm::alert(&if crate::lang::is_en() { format!("Could not save.\n\n{}\n{e}", target.display()) } else { format!("保存できませんでした。\n\n{}\n{e}", target.display()) });
                false
            }
        }
    }

    /// ダイアログの初期フォルダ: 表示中のファイルのフォルダ、無ければ ~/Documents。
    fn start_dir(&self) -> Option<PathBuf> {
        self.doc()
            .st
            .borrow()
            .path
            .as_deref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(document::default_open_dir)
    }

    fn choose_file(&self, kind: FileDialogType) -> Option<PathBuf> {
        let mut dlg = NativeFileChooser::new(kind);
        if kind == FileDialogType::BrowseSaveFile {
            dlg.set_option(FileDialogOptions::SaveAsConfirm);
        }
        if let Some(d) = self.start_dir() {
            let _ = dlg.set_directory(&d);
        }
        dlg.show();
        let p = dlg.filename();
        (!p.as_os_str().is_empty()).then_some(p)
    }

    /// フォルダを選んで、階層ツリーの Preview で開く。
    fn open_folder_dialog(self: &Rc<Self>) {
        if let Some(p) = self.choose_file(FileDialogType::BrowseDir) {
            self.open_paths(vec![p]);
        }
    }

    /// 開くダイアログ（複数選択可）。選んだファイルをそれぞれタブで開く。今のタブは閉じない。
    fn open_dialog(self: &Rc<Self>) {
        let mut dlg = NativeFileChooser::new(FileDialogType::BrowseMultiFile);
        if let Some(d) = self.start_dir() {
            let _ = dlg.set_directory(&d);
        }
        dlg.show();
        let files: Vec<PathBuf> = dlg.filenames().into_iter().filter(|p| !p.as_os_str().is_empty()).collect();
        self.open_paths(files);
    }

    /// 文字コードを切り替える。ファイルがあればディスクから読み直す。
    fn change_encoding(&self, enc: Encoding) {
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.loading || (!st.hex && !st.auto && st.encoding == enc) {
                return;
            }
        }
        let path = d.st.borrow().path.clone();
        match path {
            None => {
                {
                    let mut st = d.st.borrow_mut();
                    st.encoding = enc;
                    st.bom = enc.wants_bom(); // 新しい文書で UTF-16 を選んだら、向きが分かるように BOM を付けて保存する
                    st.auto = false;
                }
                self.refresh_title();
            }
            Some(p) => {
                if self.confirm_discard_msg(crate::lang::tr("文字コードを切り替えるとディスクから読み直します。\n変更を保存しますか？", "Changing the encoding reloads the file from disk.\nSave your changes?")) {
                    self.load_into(&d, p, Some(Mode::Text(enc)));
                }
            }
        }
    }

    /// 表示中の文書の Preview の有無に合わせて、編集領域とプレビューのどちらかを出す。
    /// SNBT として編集している NBT の文書か（HEX で開き直したものは違う）。
    fn is_nbt_doc(&self, d: &Rc<Doc>) -> bool {
        let st = d.st.borrow();
        st.nbt.is_some() && !st.hex
    }

    /// NBT の木を本文（SNBT）から作り直す。書き間違いがあれば、その知らせを出す。
    fn render_nbt_tree(&self, d: &Rc<Doc>, select: Option<&[Seg]>) {
        self.nbt_tree.restyle(self.font.get(), self.size.get());
        match nbt::from_snbt(&d.buf.text()) {
            Ok(root) => self.nbt_tree.show(&root, d.tree_open.borrow().as_ref(), select),
            Err(e) => self.nbt_tree.show_message(&if crate::lang::is_en() { format!("Cannot show as a tree because the SNBT syntax is wrong (fix it in Text): {e}") } else { format!("SNBT の書き方が違うため、木で表示できません（Text で直してください）: {e}") }),
        }
    }

    /// 面積図の四角の右クリック: 開く・場所を表示・名前の変更・ごみ箱へ・パスのコピー。
    /// zip の中身は、名前の変更とパスのコピーだけ（取り出し・削除はしない）。
    fn side_menu(self: &Rc<Self>, rel: &str, is_dir: bool) {
        let d = self.doc();
        let Some(root) = d.st.borrow().path.clone() else { return };
        let Some(old) = d.archive.borrow().clone() else { return };
        let in_folder = root.is_dir();
        let full = rel.split('/').fold(root.clone(), |p, c| p.join(c));
        let rows = [
            (crate::lang::tr("NeoNote で開く", "Open in NeoNote"), in_folder),
            (crate::lang::tr("エクスプローラーで表示", "Show in Explorer"), in_folder),
            (crate::lang::tr("名前の変更", "Rename"), true),
            (crate::lang::tr("ごみ箱へ移動", "Move to Recycle Bin"), in_folder && cfg!(windows)),
            (crate::lang::tr("パスをコピー", "Copy Path"), true),
        ];
        let menu_rows: Vec<menu::Row> = rows.iter().enumerate().map(|(i, (l, on))| menu::Row { label: l, active: *on, divider: i == 1 || i == 3, check: None }).collect();
        // 内容の幅で開くので、ウィンドウの右端を超えないように左へ寄せる
        let x = app::event_x().min(self.win.w() - menu::pick_width(&menu_rows)).max(0);
        let Some(i) = menu::pick(&menu_rows, x, app::event_y()) else { return };
        match i {
            0 => self.open_paths(vec![full]),
            1 => {
                if let Err(e) = platform::reveal(&full) {
                    confirm::alert(&format!("{}\n{e}", full.display()));
                }
            }
            2 => {
                let name = rel.rsplit('/').next().unwrap_or(rel);
                let Some(new_name) = dialog::input_default(crate::lang::tr("新しい名前", "New name"), name) else { return };
                let new_name = new_name.trim();
                if new_name.is_empty() || new_name == name || new_name.contains(['/', '\\']) {
                    return;
                }
                let parent = rel.rsplit_once('/').map(|(p, _)| format!("{p}/")).unwrap_or_default();
                let to = format!("{parent}{new_name}");
                // フォルダなら、その下の項目も前の部分を付け替える
                let new: Vec<String> = old
                    .iter()
                    .map(|e| {
                        let p = e.path.trim_end_matches('/');
                        if p == rel {
                            format!("{to}{}", if e.is_dir { "/" } else { "" })
                        } else if is_dir && e.path.starts_with(&format!("{rel}/")) {
                            format!("{to}{}", &e.path[rel.len()..])
                        } else {
                            e.path.clone()
                        }
                    })
                    .collect();
                match apply_archive_names(&root, &old, new) {
                    Ok(_) => self.load_into(&d, root, None),
                    Err(e) => confirm::alert(&e),
                }
            }
            3 => {
                let msg = if crate::lang::is_en() { format!("Move to the Recycle Bin?\n\n{}", full.display()) } else { format!("ごみ箱へ移動しますか？\n\n{}", full.display()) };
                if confirm::choice(&msg, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("ごみ箱へ", "Move"), "") != Some(1) {
                    return;
                }
                match platform::move_to_trash(&full) {
                    Ok(()) => self.load_into(&d, root, None),
                    Err(e) => confirm::alert(&format!("{}\n{e}", full.display())),
                }
            }
            4 => {
                let text = if in_folder { full.display().to_string() } else { rel.to_string() };
                app::copy(&text);
                self.flash(crate::lang::tr("パスをコピーしました", "Path copied"));
            }
            _ => {}
        }
    }

    /// フォルダ・zip の一覧を、開閉できる木で出す。
    fn render_archive_tree(&self, d: &Rc<Doc>) {
        self.nbt_tree.restyle(self.font.get(), self.size.get());
        let path = d.st.borrow().path.clone();
        let Some(p) = path else { return };
        let entries = d.archive.borrow();
        let Some(v) = entries.as_deref() else { return };
        let packed = archive::is_zip_path(&p) || v.iter().any(|e| e.packed.is_some());
        self.nbt_tree.show_archive(log::file_name(&p.to_string_lossy()), v, packed, d.tree_open.borrow().as_ref());
    }

    /// 木での編集: 本文の SNBT を読み、`f` で書き換えて、本文を置き換える（元に戻すで 1 回で戻る）。
    /// `f` は選び直す項目の道筋か、書き換えられなかった理由を返す。
    fn edit_nbt(&self, f: impl FnOnce(&mut nbt::Tag) -> Result<Option<Vec<Seg>>, String>) {
        let d = self.doc();
        let Ok(mut root) = nbt::from_snbt(&d.buf.text()) else { return };
        match f(&mut root) {
            Ok(select) => {
                *d.tree_open.borrow_mut() = Some(self.nbt_tree.open_paths());
                let text = nbt::to_snbt(&root);
                let mut b = d.buf.clone();
                b.replace(0, b.length(), &text);
                self.render_nbt_tree(&d, select.as_deref());
                self.recount();
            }
            Err(e) if e.is_empty() => {} // 取り消した
            Err(e) => confirm::alert(&e),
        }
    }

    /// 選んでいる項目の値を書き換える（複合タグ・リストは開閉する）。
    fn nbt_edit_value(&self) {
        let Some(path) = self.nbt_tree.selected_path() else { return };
        self.edit_nbt(|root| {
            let t = nbtedit::get_mut(root, &path).ok_or(crate::lang::tr("項目が見つかりません", "Item not found"))?;
            let Some(cur) = nbtedit::edit_text(t) else { return Err(String::new()) };
            let name = match path.last() {
                Some(Seg::Key(k)) => k.clone(),
                Some(Seg::Index(i)) => format!("[{i}]"),
                None => String::new(),
            };
            let prompt = if crate::lang::is_en() { format!("Value of {name} ({})", nbtedit::type_name(t.id())) } else { format!("{name}（{}）の値", nbtedit::type_name(t.id())) };
            let Some(input) = dialog::input_default(&prompt, &cur) else { return Err(String::new()) };
            *t = nbtedit::parse_edit(t, &input)?;
            Ok(Some(path.clone()))
        });
    }

    fn nbt_rename(&self) {
        let Some(path) = self.nbt_tree.selected_path() else { return };
        let Some(Seg::Key(old)) = path.last().cloned() else {
            self.flash(crate::lang::tr("名前を変えられるのは、複合タグの中の項目だけです", "Only items inside a compound tag can be renamed"));
            return;
        };
        self.edit_nbt(|root| {
            let Some(name) = dialog::input_default(crate::lang::tr("新しい名前", "New name"), &old) else { return Err(String::new()) };
            nbtedit::rename(root, &path, name.trim())?;
            let mut p = path.clone();
            p.pop();
            p.push(Seg::Key(name.trim().to_string()));
            Ok(Some(p))
        });
    }

    fn nbt_delete(&self) {
        let Some(path) = self.nbt_tree.selected_path() else { return };
        let label = match path.last() {
            Some(Seg::Key(k)) => k.clone(),
            Some(Seg::Index(i)) => format!("[{i}]"),
            None => return,
        };
        if confirm::choice(&if crate::lang::is_en() { format!("Delete \"{label}\"? (You can undo this.)") } else { format!("「{label}」を消しますか？（元に戻すで戻せます）") }, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("消す", "Delete"), "") != Some(1) {
            return;
        }
        self.edit_nbt(|root| {
            nbtedit::remove(root, &path);
            let mut parent = path.clone();
            parent.pop();
            Ok(Some(parent))
        });
    }

    /// 選んでいる複合タグ・リストに子を足す（項目を選んでいなければ根に足す。値の項目なら、その親に足す）。
    fn nbt_add(&self) {
        let mut parent = self.nbt_tree.selected_path().unwrap_or_default();
        self.edit_nbt(|root| {
            if !matches!(nbtedit::get(root, &parent), Some(nbt::Tag::Compound(_) | nbt::Tag::List(..))) {
                parent.pop();
            }
            let p = nbtedit::get(root, &parent).ok_or(crate::lang::tr("項目が見つかりません", "Item not found"))?;
            let in_list = matches!(p, nbt::Tag::List(..));
            let id = match nbtedit::fixed_child_type(p) {
                Some(id) => id,
                None => {
                    let rows: Vec<menu::Row> = nbtedit::TYPES.iter().map(|(n, _)| menu::Row::plain(n)).collect();
                    let Some(i) = menu::pick(&rows, app::event_x(), app::event_y()) else { return Err(String::new()) };
                    nbtedit::TYPES[i].1
                }
            };
            let name = if in_list {
                String::new()
            } else {
                let Some(n) = dialog::input_default(&if crate::lang::is_en() { format!("Name for the new {}", nbtedit::type_name(id)) } else { format!("足す {} の名前", nbtedit::type_name(id)) }, "") else { return Err(String::new()) };
                n.trim().to_string()
            };
            let child = nbtedit::default_of(id).ok_or(crate::lang::tr("知らない型です", "Unknown type"))?;
            let path = nbtedit::add_child(root, &parent, &name, child)?;
            Ok(Some(path))
        });
    }

    /// 木の右クリックのメニュー。
    fn nbt_menu(&self) {
        let path = self.nbt_tree.selected_path();
        let on_key = matches!(path.as_ref().and_then(|p| p.last()), Some(Seg::Key(_)));
        let mut rows = vec![menu::Row::plain(crate::lang::tr("値を編集 (Enter)", "Edit Value (Enter)")), menu::Row::plain(crate::lang::tr("子を追加 (Insert)", "Add Child (Insert)")), menu::Row::plain(crate::lang::tr("名前を変更 (F2)", "Rename (F2)")), menu::Row::plain(crate::lang::tr("削除 (Delete)", "Delete (Delete)"))];
        // 値を書き換えられるのは、数・文字列・配列だけ（複合タグ・リストは中の項目を書き換える）
        let editable = path.as_ref().is_some_and(|p| {
            nbt::from_snbt(&self.doc().buf.text()).ok().as_ref().and_then(|r| nbtedit::get(r, p)).and_then(nbtedit::edit_text).is_some()
        });
        rows[0].active = editable;
        rows[2].active = on_key;
        rows[3].active = path.is_some();
        match menu::pick(&rows, app::event_x(), app::event_y()) {
            Some(0) => self.nbt_edit_value(),
            Some(1) => self.nbt_add(),
            Some(2) => self.nbt_rename(),
            Some(3) => self.nbt_delete(),
            _ => {}
        }
    }

    fn apply_view(&self, d: &Rc<Doc>) {
        let mut ed = self.editor.clone();
        let mut pv = self.preview.clone();
        let mut tree = self.nbt_tree.tree.clone();
        let arch_view = d.preview.get() && d.archive.borrow().is_some();
        let nbt_view = d.preview.get() && self.is_nbt_doc(d) || arch_view;
        if nbt_view {
            if arch_view {
                self.render_archive_tree(d);
            } else {
                self.render_nbt_tree(d, None);
            }
            tree.show();
            let _ = tree.take_focus();
            ed.hide();
            pv.hide();
        } else if tree.visible() && (self.is_nbt_doc(d) || d.archive.borrow().is_some()) {
            *d.tree_open.borrow_mut() = Some(self.nbt_tree.open_paths());
        }
        if nbt_view {
        } else if d.preview.get() {
            self.render_preview(d);
            // 編集領域を隠す前にフォーカスを移す（先に隠すと、フォーカスが検索欄へ移って検索バーが開く）
            pv.show();
            let _ = pv.take_focus();
            ed.hide();
        } else if !ed.visible() {
            ed.show();
            let _ = ed.take_focus();
            pv.hide();
        }
        // 木は、ほかの部品へフォーカスを移してから隠す（先に隠すと、フォーカスが検索欄へ移って検索バーが開く）
        if !nbt_view && tree.visible() {
            if tree.has_focus() {
                let _ = if d.preview.get() { pv.take_focus() } else { ed.take_focus() };
            }
            tree.hide();
        }
        self.col.clone().layout();
        self.body.clone().layout();
        self.win.clone().redraw();
    }

    /// 文書の本文から Preview の中身を作り直す（切り替え・配色・文字サイズの変更のとき）。
    fn render_preview(&self, d: &Rc<Doc>) {
        let hex = |(r, g, b): (u8, u8, u8)| format!("#{r:02x}{g:02x}{b:02x}");
        let (bg, fg) = (hex(color_main()), hex(color_text()));
        let kind = self.view_kind(d);
        let is_pdf = kind == viewmode::Kind::Pdf;
        let path = d.st.borrow().path.clone();
        let max_w = (self.editor.w().max(self.win.w() - 20) - 30).max(100); // 隠れている部品の幅は当てにならないので、編集領域・ウィンドウの幅から決める
        let html = if kind == viewmode::Kind::Archive {
            match (d.archive.borrow().as_ref(), path.as_deref()) {
                (Some(v), Some(p)) => archive::to_html(log::file_name(&p.to_string_lossy()), v, archive::is_zip_path(p) || v.iter().any(|e| e.packed.is_some()), &bg, &fg, &hex(color_subtext())),
                _ => pdftext::message_html("", &bg, &fg),
            }
        } else if kind == viewmode::Kind::Audio {
            let name = path.as_deref().map(|p| log::file_name(&p.to_string_lossy()).to_string()).unwrap_or_default();
            match d.audio.borrow().as_ref() {
                Some(Ok(a)) => {
                    let (dim, accent, track) = (hex(color_subtext()), hex(color_accent()), hex(mix(color_main(), color_text(), 15)));
                    let look = audiometa::PlayerLook { bg: &bg, fg: &fg, dim: &dim, accent: &accent, track: &track };
                    let state = self.audio_state(d);
                    let playing = matches!(state, Some((audioplay::State::Playing, _)));
                    let pos = state.map(|(_, p)| p);
                    audiometa::page_html(&name, &a.info, &a.items, playing, pos, &self.audio_note(), &look)
                }
                Some(Err(e)) => pdftext::message_html(e, &bg, &fg),
                None => pdftext::message_html(crate::lang::tr("読み込み中...", "Loading..."), &bg, &fg),
            }
        } else if kind == viewmode::Kind::Book {
            match d.book.borrow().as_ref() {
                Some(Ok(sheets)) => book::to_html(sheets, &bg, &fg, &hex(color_subtext())),
                Some(Err(e)) => pdftext::message_html(e, &bg, &fg),
                None => pdftext::message_html(crate::lang::tr("読み込み中...", "Loading..."), &bg, &fg),
            }
        } else if kind == viewmode::Kind::Doc {
            match d.doc_text.borrow().as_ref() {
                Some(Ok(t)) => doctext::to_html(t, &bg, &fg, &hex(color_subtext())),
                Some(Err(e)) => pdftext::message_html(e, &bg, &fg),
                None => pdftext::message_html(crate::lang::tr("読み込み中...", "Loading..."), &bg, &fg),
            }
        } else if kind == viewmode::Kind::Html {
            let base = path.as_deref().and_then(|p| p.parent());
            let imgs = markdown::Images { base, max_w, size_of: &image_size };
            htmlview::to_html(&d.buf.text(), &bg, &fg, &hex(color_accent()), &hex(color_subtext()), &imgs)
        } else if kind == viewmode::Kind::Sheet {
            let text = d.buf.text();
            let delim = sheet::delimiter_for(path.as_deref(), &text);
            sheet::to_html(&text, delim, &bg, &fg, &hex(color_subtext()))
        } else if kind == viewmode::Kind::Image {
            match path {
                Some(p) => imagefile::page_html(&p, image_size(&p), max_w, &bg, &fg),
                None => pdftext::message_html("", &bg, &fg),
            }
        } else if !is_pdf {
            let base = path.as_deref().and_then(|p| p.parent());
            let imgs = markdown::Images { base, max_w, size_of: &image_size };
            let (link, subtext) = (hex(color_accent()), hex(color_subtext()));
            let sub = hex(mix(color_main(), color_text(), 7));
            let border = hex(mix(color_main(), color_text(), 25));
            let c = markdown::Colors { bg: &bg, fg: &fg, link: &link, sub: &sub, subtext: &subtext, border: &border };
            markdown::to_html(&d.buf.text(), &c, &imgs)
        } else if let Some(pages) = d.pdf_text.borrow().as_ref() {
            pdftext::to_html(pages, &bg, &fg, &hex(color_subtext()))
        } else if let Some(e) = d.pdf_error.borrow().as_ref() {
            pdftext::message_html(e, &bg, &fg)
        } else {
            pdftext::message_html(crate::lang::tr("読み込み中...", "Loading..."), &bg, &fg)
        };
        let mut pv = self.preview.clone();
        pv.set_color(rgb(color_main()));
        pv.set_text_font(self.font.get());
        pv.set_text_size(self.size.get());
        pv.set_value(&html);
    }

    /// 表示中の文書が Preview なら描き直す（配色・文字サイズ・フォントを変えたあと）。
    fn refresh_preview(&self) {
        let d = self.doc();
        if d.preview.get() && self.is_nbt_doc(&d) {
            *d.tree_open.borrow_mut() = Some(self.nbt_tree.open_paths());
            self.render_nbt_tree(&d, None);
        } else if d.preview.get() {
            self.render_preview(&d);
        }
    }

    /// Preview へ切り替える / Text に戻す（md と pdf）。
    fn set_preview(&self, on: bool) {
        let d = self.doc();
        self.set_preview_for(&d, on);
    }

    fn set_preview_for(&self, d: &Rc<Doc>, on: bool) {
        if d.preview.get() == on {
            return;
        }
        if on {
            let (loading, hex, path) = {
                let st = d.st.borrow();
                (st.loading, st.hex, st.path.clone())
            };
            let kind = if self.is_nbt_doc(d) { viewmode::Kind::Nbt } else { self.view_kind(d) };
            // pdf は HEX で開かれているのが普通。md は文字として開いているときだけ
            let ok = match kind {
                viewmode::Kind::Nbt => self.is_nbt_doc(d),
                viewmode::Kind::Markdown | viewmode::Kind::Html | viewmode::Kind::Sheet => !hex,
                viewmode::Kind::Book | viewmode::Kind::Doc | viewmode::Kind::Pdf | viewmode::Kind::Image | viewmode::Kind::Audio => path.is_some(),
                viewmode::Kind::Archive => d.archive.borrow().is_some(),
                viewmode::Kind::Other => false,
            };
            if loading || !ok {
                return;
            }
            if self.is_active(d) {
                d.cursor.set(self.editor.insert_position());
            }
            if kind == viewmode::Kind::Pdf {
                self.start_pdf_extract(d, path.unwrap());
            } else if kind == viewmode::Kind::Book {
                self.start_book_load(d, path.unwrap());
            } else if kind == viewmode::Kind::Doc {
                self.start_doc_load(d, path.unwrap());
            } else if kind == viewmode::Kind::Audio {
                self.start_audio_info(d, path.unwrap());
            }
        }
        d.preview.set(on);
        if self.is_active(d) {
            self.apply_view(d);
            self.refresh_title();
        }
    }

    /// pdf の文字をバックグラウンドで取り出す（取り出し済み・取り出し中なら何もしない）。
    fn start_pdf_extract(&self, d: &Rc<Doc>, path: PathBuf) {
        if d.pdf_text.borrow().is_some() || d.pdf_busy.get() {
            return;
        }
        d.pdf_busy.set(true);
        *d.pdf_error.borrow_mut() = None;
        let tx = self.tx.clone();
        let id = d.id;
        thread::spawn(move || {
            let r = std::fs::read(&path).map_err(|e| format!("{}\n{e}", path.display())).and_then(|b| pdftext::extract(&b));
            tx.send(Msg::PdfText(id, r));
            app::awake();
        });
    }

    /// xlsx / ods のシートをバックグラウンドで読む（読み込み済み・読み込み中なら何もしない）。
    fn start_book_load(&self, d: &Rc<Doc>, path: PathBuf) {
        if d.book.borrow().is_some() || d.book_busy.get() {
            return;
        }
        d.book_busy.set(true);
        let tx = self.tx.clone();
        let id = d.id;
        thread::spawn(move || {
            let r = std::fs::read(&path).map_err(|e| format!("{}\n{e}", path.display())).and_then(|b| book::read(&b, book::is_ods(&path)));
            tx.send(Msg::Book(id, r));
            app::awake();
        });
    }

    /// docx / odt / pptx / odp / epub の文字をバックグラウンドで読む（読み込み済み・読み込み中なら何もしない）。
    fn start_doc_load(&self, d: &Rc<Doc>, path: PathBuf) {
        if d.doc_text.borrow().is_some() || d.doc_busy.get() {
            return;
        }
        let Some(fmt) = doctext::format_of(&path) else { return };
        d.doc_busy.set(true);
        let tx = self.tx.clone();
        let id = d.id;
        thread::spawn(move || {
            let r = std::fs::read(&path).map_err(|e| format!("{}\n{e}", path.display())).and_then(|b| doctext::read(&b, fmt));
            tx.send(Msg::DocText(id, r));
            app::awake();
        });
    }

    fn on_doc_text(&self, id: u64, r: Result<doctext::Text, String>) {
        let Some(d) = self.find(id) else { return };
        d.doc_busy.set(false);
        if let Err(e) = &r {
            log::warn(TAG, &format!("doc: {}", e.replace('\n', " / ")));
        }
        *d.doc_text.borrow_mut() = Some(r);
        if d.preview.get() && self.is_active(&d) {
            self.render_preview(&d);
        }
    }

    fn on_book(&self, id: u64, r: Result<Vec<book::Sheet>, String>) {
        let Some(d) = self.find(id) else { return };
        d.book_busy.set(false);
        if let Err(e) = &r {
            log::warn(TAG, &format!("book: {}", e.replace('\n', " / ")));
        }
        *d.book.borrow_mut() = Some(r);
        if d.preview.get() && self.is_active(&d) {
            self.render_preview(&d);
        }
    }

    fn on_pdf_text(&self, id: u64, r: Result<Vec<String>, String>) {
        let Some(d) = self.find(id) else { return };
        d.pdf_busy.set(false);
        match r {
            Ok(p) => *d.pdf_text.borrow_mut() = Some(p),
            Err(e) => {
                log::warn(TAG, &format!("pdf: {}", e.replace('\n', " / ")));
                *d.pdf_error.borrow_mut() = Some(e);
            }
        }
        if d.preview.get() && self.is_active(&d) {
            self.render_preview(&d);
        }
    }

    // ---- 音声（情報・タグの表示と、簡易再生） ----

    /// 音声の情報とタグをバックグラウンドで読む（取得済み・取得中なら何もしない）。
    fn start_audio_info(&self, d: &Rc<Doc>, path: PathBuf) {
        if d.audio.borrow().is_some() || d.audio_busy.get() {
            return;
        }
        d.audio_busy.set(true);
        let tx = self.tx.clone();
        let id = d.id;
        thread::spawn(move || {
            let r = std::fs::metadata(&path)
                .map_err(|e| format!("{}\n{e}", path.display()))
                .and_then(|m| if m.len() > AUDIO_INFO_MAX { Err(if crate::lang::is_en() { format!("Too large to read info and tags (up to {} MiB)", AUDIO_INFO_MAX / 1024 / 1024) } else { format!("大きすぎるため、情報とタグを読みません（{} MiB まで）", AUDIO_INFO_MAX / 1024 / 1024) }) } else { Ok(()) })
                .and_then(|_| std::fs::read(&path).map_err(|e| format!("{}\n{e}", path.display())))
                .map(|b| AudioData { info: audiometa::info(&b), items: audiometa::items(&b).map(|(_, v)| v).unwrap_or_default() });
            tx.send(Msg::Audio(id, r));
            app::awake();
        });
    }

    fn on_audio(&self, id: u64, r: Result<AudioData, String>) {
        let Some(d) = self.find(id) else { return };
        d.audio_busy.set(false);
        if let Err(e) = &r {
            log::warn(TAG, &format!("audio: {}", e.replace('\n', " / ")));
        }
        *d.audio.borrow_mut() = Some(r);
        if d.preview.get() && self.is_active(&d) {
            self.render_preview(&d);
        }
        if self.is_active(&d) {
            // 情報とタグは右パネルに出す（本文は空なので、空のタブと見分けが付かない。必ず作り直す）
            self.side_key.set((0, -1, false, 0));
            self.refresh_side();
        }
    }

    /// 再生の状態と位置。プレーヤーが無ければ None。
    fn audio_state(&self, d: &Doc) -> Option<(audioplay::State, f64)> {
        d.player.borrow_mut().as_mut().map(|p| p.poll())
    }

    /// Preview の上に出す 1 行（再生の状態と、操作の案内）。
    /// 音楽プレイヤーの表示の案内。
    fn audio_note(&self) -> String {
        if !audioplay::AVAILABLE {
            return crate::lang::tr("この build には再生機能が含まれていません", "Playback is not included in this build").into();
        }
        crate::lang::tr("クリックか Space で再生 / 停止", "Click or press Space to play / stop").into()
    }

    /// 再生 / 停止（表示中の音声の Preview）。別のファイルを鳴らしていれば、そちらは止めて捨てる。
    fn toggle_audio(self: &Rc<Self>) {
        let d = self.doc();
        let path = d.st.borrow().path.clone();
        let Some(path) = path.filter(|p| viewmode::kind_of(Some(p)) == viewmode::Kind::Audio) else { return };
        if !audioplay::AVAILABLE {
            self.flash(crate::lang::tr("この build には再生機能が含まれていません", "Playback is not included in this build"));
            return;
        }
        let state = self.audio_state(&d).map(|(s, _)| s);
        if state == Some(audioplay::State::Playing) {
            if let Some(p) = d.player.borrow_mut().as_mut() {
                p.stop();
            }
        } else {
            self.drop_other_players(&d);
            let started = if state.is_some() {
                d.player.borrow_mut().as_mut().map(|p| p.play()).unwrap_or(Ok(()))
            } else {
                audioplay::start(&path).map(|p| *d.player.borrow_mut() = Some(p))
            };
            if let Err(e) = started {
                log::warn(TAG, &format!("audio play failed: {}", e.replace('\n', " / ")));
                *d.player.borrow_mut() = None;
                confirm::alert(&if crate::lang::is_en() { format!("Could not play.\n\n{e}") } else { format!("再生できませんでした。\n\n{e}") });
            }
        }
        d.audio_secs.set(-1);
        self.render_preview(&d);
        self.ensure_audio_timer();
    }

    /// `keep` 以外の文書のプレーヤーを止めて捨てる（同時に鳴らすのは 1 つだけ）。
    fn drop_other_players(&self, keep: &Rc<Doc>) {
        let others: Vec<Rc<Doc>> = self.docs.borrow().iter().filter(|x| x.id != keep.id).cloned().collect();
        for o in others {
            if o.player.borrow_mut().take().is_some() && o.preview.get() && self.is_active(&o) {
                self.render_preview(&o);
            }
        }
    }

    /// すべてのプレーヤーを止めて解放する（終了のとき）。
    fn stop_all_audio(&self) {
        let docs: Vec<Rc<Doc>> = self.docs.borrow().iter().cloned().collect();
        for d in docs {
            d.player.borrow_mut().take();
        }
    }

    /// このパスの音声を鳴らしているプレーヤーを捨てる（ファイルを書き換える前に。Windows では開いたままだと置き換えられない）。
    fn release_player_for(&self, path: &Path) {
        let docs: Vec<Rc<Doc>> = self.docs.borrow().iter().cloned().collect();
        for d in docs {
            if d.st.borrow().path.as_deref().is_some_and(|p| document::same_file(p, path)) {
                d.player.borrow_mut().take();
            }
        }
    }

    /// 音声のタグを書き換えたあと、そのファイルを開いている Preview の情報を取り直し、ディスクの状態も合わせる。
    fn refresh_audio_views(&self, path: &Path) {
        let docs: Vec<Rc<Doc>> = self.docs.borrow().iter().cloned().collect();
        for d in docs {
            let hit = d.st.borrow().path.as_deref().is_some_and(|p| document::same_file(p, path));
            if !hit {
                continue;
            }
            d.st.borrow_mut().stamp = document::stamp(path);
            *d.audio.borrow_mut() = None;
            if d.preview.get() {
                self.start_audio_info(&d, path.to_path_buf());
                if self.is_active(&d) {
                    self.render_preview(&d);
                }
            }
        }
    }

    /// 再生位置を 0.5 秒ごとに見る（鳴らし終えたら止める。表示中の Preview は秒が変わったときだけ描き直す）。
    fn ensure_audio_timer(self: &Rc<Self>) {
        if self.audio_timer.get() {
            return;
        }
        self.audio_timer.set(true);
        let ui = Rc::clone(self);
        app::add_timeout3(0.5, move |_| {
            ui.audio_timer.set(false);
            if ui.poll_audio() {
                ui.ensure_audio_timer();
            }
        });
    }

    /// 再生中のものがあれば true。
    fn poll_audio(&self) -> bool {
        let docs: Vec<Rc<Doc>> = self.docs.borrow().iter().cloned().collect();
        let mut any = false;
        for d in docs {
            let mut changed = false;
            if let Some(p) = d.player.borrow_mut().as_mut() {
                let (s, pos) = p.poll();
                match s {
                    audioplay::State::Playing => {
                        any = true;
                        if d.audio_secs.get() != pos as i64 {
                            d.audio_secs.set(pos as i64);
                            changed = true;
                        }
                    }
                    audioplay::State::Finished => {
                        p.stop();
                        changed = true;
                    }
                    audioplay::State::Stopped => {}
                }
            }
            if changed && d.preview.get() && self.is_active(&d) {
                self.render_preview(&d);
            }
        }
        any
    }

    /// 音声の Preview を見ているときの Space（再生 / 停止）。扱ったら true。
    fn audio_space(self: &Rc<Self>) -> bool {
        let d = self.doc();
        let is_audio = d.preview.get() && viewmode::kind_of(d.st.borrow().path.as_deref()) == viewmode::Kind::Audio;
        if is_audio {
            self.toggle_audio();
        }
        is_audio
    }

    /// HEX モードで開き直す（ディスクから読む。ファイルが無ければ何もしない）。
    fn change_to_hex(&self) {
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.loading || (st.hex && !st.auto) {
                return;
            }
        }
        let path = d.st.borrow().path.clone();
        match path {
            None => self.flash(crate::lang::tr("先に保存してください", "Save first")),
            Some(p) => {
                if self.confirm_discard_msg(crate::lang::tr("HEX で開き直します（ディスクから読み直します）。\n変更を保存しますか？", "Reopen as HEX (reloads from disk).\nSave your changes?")) {
                    self.load_into(&d, p, Some(Mode::Hex));
                }
            }
        }
    }

    /// 文字として見られるようにする。HEX・一覧・音声（本文を読んでいない）なら読み直す。
    /// 自動で HEX になったファイルや、Preview で開く種類は、自動判定では文字にならないので UTF-8 で開く。
    fn reopen_as_text(&self, is_hex: bool, is_arch: bool, file_kind: viewmode::Kind) {
        let d = self.doc();
        let (auto, empty) = (d.st.borrow().auto, d.buf.length() == 0);
        if is_hex && !auto && !auto_previews(file_kind) {
            self.change_to_auto(); // 手で HEX にした文字のファイル
        } else if is_hex || is_arch || (empty && auto_previews(file_kind)) {
            self.change_encoding(Encoding::Utf8);
        }
    }

    /// 自動判定に戻す（ディスクから読み直す）。保存先のない文書は、文字コードを UTF-8 の自動に戻すだけ。
    fn change_to_auto(&self) {
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.loading || st.auto {
                return;
            }
        }
        let path = d.st.borrow().path.clone();
        match path {
            None => {
                {
                    let mut st = d.st.borrow_mut();
                    st.encoding = Encoding::Utf8;
                    st.bom = false;
                    st.auto = true;
                }
                self.refresh_title();
            }
            Some(p) => {
                if self.confirm_discard_msg(crate::lang::tr("自動判定で開き直します（ディスクから読み直します）。\n変更を保存しますか？", "Reopen with auto detection (reloads from disk).\nSave your changes?")) {
                    self.load_into(&d, p, None);
                }
            }
        }
    }

    /// マウスがチップの上にあるか（チップから開いたメニューを、チップの真下に出すため）。
    fn chip_under_mouse(&self, c: &Button) -> bool {
        let (mx, my) = (app::event_x(), app::event_y());
        mx >= c.x() && mx < c.x() + c.w() && my >= c.y() && my < c.y() + c.h()
    }

    /// 表示・文字コード・改行コードの切替メニュー（タブバー右端のチップ、右クリックの Change Encoding）。
    /// 上から 表示（Preview は準備中で選べない・HEX）、文字コード（UTF-8 など・自動判定）、改行コード。
    /// 選ばれている行に ● を付ける（自動のときは判定結果も添える）。
    /// `view` が true なら表示のチップ（表示・形式・ファイルごとの操作）、false なら文字コードのチップ（文字コード・改行コード）の分だけ出す。
    fn encoding_menu(self: &Rc<Self>, view: bool) {
        #[derive(Clone, Copy)]
        enum Pick {
            View(viewmode::View),
            As(viewmode::Kind),
            Text(Encoding),
            Auto,
            FormatAuto,
            Eol,
            Meta,
            Play,
            PdfText,
            DocText,
            ArchiveRename,
        }
        let (cur, is_hex, auto, eol, path, in_preview) = {
            let d = self.doc();
            let st = d.st.borrow();
            (st.encoding, st.hex, st.auto, st.eol, st.path.clone(), d.preview.get())
        };
        // 一覧を読めていない .zip（壊れている等）は、ふつうのファイルとして扱う
        let is_arch = self.doc().archive.borrow().is_some();
        // フォルダは一覧しか無い（文字・HEX・文字コードを選べない）
        let is_folder = path.as_deref().is_some_and(Path::is_dir);
        let file_kind = viewmode::kind_of(path.as_deref());
        let forced = self.doc().preview_as.get();
        let fmt_auto = self.doc().format_auto.get() && forced.is_none();
        let kind = match self.view_kind(&self.doc()) {
            _ if self.is_nbt_doc(&self.doc()) => viewmode::Kind::Nbt,
            // 文字や HEX で開き直した zip は Archive のまま（Preview で一覧に戻せる）。一覧を読めない壊れた zip だけ文字のファイル
            viewmode::Kind::Archive if !is_arch && !path.as_deref().is_some_and(archive::is_zip_path) => viewmode::Kind::Other,
            viewmode::Kind::Nbt if !in_preview => viewmode::Kind::Other, // NBT として読めなかった .dat
            k => k,
        };
        // (選択, 表示名, 選ばれている, 選べる, この行の後に区切りを入れる)
        let mut rows: Vec<(Pick, String, bool, bool, bool)> = Vec::new();
        if !is_folder {
            rows.push((Pick::FormatAuto, format!("Auto ({})", viewmode::kind_name(kind, in_preview, is_hex)), fmt_auto, true, true));
        }
        let mut views = viewmode::available_for(kind);
        if is_folder {
            views.retain(|v| *v == viewmode::View::Preview);
        }
        if path.is_none() {
            views.retain(|v| *v != viewmode::View::Hex); // リージョンのチャンクは、HEX で開き直す元のファイルが無い
        }
        // Preview があるファイルは、Preview から戻る道として Text の行も出す
        let has_preview = views.contains(&viewmode::View::Preview) && viewmode::View::Preview.implemented(kind);
        for v in views {
            match v {
                viewmode::View::Text => {
                    if has_preview {
                        rows.push((Pick::View(v), "Text".into(), !fmt_auto && !in_preview && !is_hex, true, false));
                    }
                }
                viewmode::View::Hex => rows.push((Pick::View(v), "HEX".into(), !fmt_auto && !auto && is_hex && !in_preview, true, true)),
                viewmode::View::Preview => {
                    let name = if v.implemented(kind) { "Preview" } else { crate::lang::tr("Preview（準備中）", "Preview (coming soon)") };
                    rows.push((Pick::View(v), name.into(), !fmt_auto && in_preview, v.implemented(kind), false));
                }
            }
        }
        // 文字の文書は、Preview の形式を選べる（拡張子が txt の Markdown など）
        if !is_folder && !self.is_nbt_doc(&self.doc()) {
            if let Some(last) = rows.last_mut() {
                last.4 = true;
            }
            for (k, name) in [(viewmode::Kind::Markdown, crate::lang::tr("Markdown として表示", "Preview as Markdown")), (viewmode::Kind::Html, crate::lang::tr("HTML として表示", "Preview as HTML")), (viewmode::Kind::Sheet, crate::lang::tr("CSV / TSV として表示", "Preview as CSV / TSV"))] {
                // HEX で開いている時は、先に文字で開き直す（選ぶと文字で読み直してから表示する）
                rows.push((Pick::As(k), name.into(), !fmt_auto && in_preview && kind == k && (forced.is_some() || file_kind == k), true, false));
            }
        }
        let is_audio = kind == viewmode::Kind::Audio;
        let view_rows = rows.len();
        if !is_folder {
            let detected = if is_hex { "HEX".to_string() } else { cur.label().to_string() };
            rows.push((Pick::Auto, format!("Auto ({detected})"), auto && !in_preview, true, true));
            for e in [Encoding::Utf8, Encoding::Windows31J, Encoding::EucJp, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Iso8859_1, Encoding::UsAscii] {
                rows.push((Pick::Text(e), e.label().into(), !auto && !is_hex && !in_preview && cur == e, true, false));
            }
            if let Some(last) = rows.last_mut() {
                last.4 = true;
            }
            rows.push((Pick::Eol, format!("{}: {}", crate::lang::tr("改行コード", "Line Ending"), if is_hex { "-" } else { eol.label() }), false, !is_hex, false));
        }
        let enc_end = rows.len();
        if is_arch && path.as_deref().is_some_and(archive::can_rename) {
            rows.push((Pick::ArchiveRename, crate::lang::tr("名前の変更（テキスト）", "Rename (text)").into(), false, true, false));
        }
        // 画像・pdf は、中身を文字として編集する入口を最後に出す
        if is_audio && in_preview && audioplay::AVAILABLE {
            let playing = matches!(self.audio_state(&self.doc()), Some((audioplay::State::Playing, _)));
            rows.push((Pick::Play, String::from(if playing { crate::lang::tr("停止", "Stop") } else { crate::lang::tr("再生", "Play") }), false, true, false));
        }
        if (kind == viewmode::Kind::Image && path.as_deref().is_some_and(imgmeta::supports)) || (is_audio && path.as_deref().is_some_and(audiometa::supports)) {
            rows.push((Pick::Meta, crate::lang::tr("メタデータを編集（テキスト）", "Edit Metadata (text)").into(), false, true, false));
        }
        if kind == viewmode::Kind::Pdf {
            rows.push((Pick::PdfText, crate::lang::tr("新しいタブでテキストを開く", "Text in New Tab").into(), false, true, false));
        }
        if kind == viewmode::Kind::Doc {
            rows.push((Pick::DocText, crate::lang::tr("新しいタブでテキストを開く", "Text in New Tab").into(), false, true, false));
        }
        // 表示のチップは文字コードの行を除き、文字コードのチップは文字コードの行だけにする
        if view {
            rows.drain(view_rows..enc_end);
        } else {
            rows.truncate(enc_end);
            rows.drain(..view_rows);
        }
        if let Some(last) = rows.last_mut() {
            last.4 = false;
        }
        if rows.is_empty() {
            return;
        }
        let labels: Vec<String> =
            rows.iter().map(|(_, name, on, _, _)| format!("{} {name}", if *on { "●" } else { "\u{3000}" })).collect();
        let menu_rows: Vec<menu::Row> = rows
            .iter()
            .zip(&labels)
            .map(|(r, label)| menu::Row { label, active: r.3, divider: r.4, check: None })
            .collect();
        // チップから開いたときは、チップの真下に出す
        let anchor = if view { &self.view_chip } else { &self.chip };
        let (x, y) = if self.chip_under_mouse(anchor) { (anchor.x(), anchor.y() + anchor.h()) } else { (app::event_x(), app::event_y()) };
        // 内容の幅で開くので、ウィンドウの右端を超えないように左へ寄せる
        let x = match self.chip.window() {
            Some(w) => x.min(w.w() - menu::pick_width(&menu_rows)).max(0),
            None => x,
        };
        let Some(i) = menu::pick(&menu_rows, x, y) else { return };
        if matches!(rows[i].0, Pick::View(_) | Pick::As(_)) {
            self.doc().format_auto.set(false);
        }
        match rows[i].0 {
            Pick::FormatAuto => {
                let d = self.doc();
                d.format_auto.set(true);
                d.preview_as.set(None);
                self.set_preview(false);
                if is_hex && !auto_previews(file_kind) {
                    self.change_to_auto();
                }
                if auto_previews(self.view_kind(&d)) || self.view_kind(&d) == viewmode::Kind::Markdown {
                    self.set_preview(true);
                }
                self.refresh_title();
            }
            Pick::View(viewmode::View::Hex) => self.change_to_hex(),
            Pick::View(viewmode::View::Preview) if kind == viewmode::Kind::Archive && !is_arch => self.change_to_auto(), // 一覧で開き直す
            Pick::View(viewmode::View::Preview) => {
                // Auto で中身から決めた形式は、選び直した後もそのまま使う
                self.doc().preview_as.set((file_kind == viewmode::Kind::Other && kind != viewmode::Kind::Other).then_some(kind));
                self.set_preview(true);
            }
            Pick::As(k) => {
                let d = self.doc();
                self.set_preview(false);
                d.preview_as.set(Some(k).filter(|k| *k != file_kind));
                self.reopen_as_text(is_hex, is_arch, file_kind);
                self.set_preview(true);
            }
            Pick::View(viewmode::View::Text) => {
                self.set_preview(false);
                self.reopen_as_text(is_hex, is_arch, file_kind);
            }
            Pick::Text(e) => {
                self.set_preview(false);
                self.change_encoding(e);
            }
            Pick::Auto => {
                self.set_preview(false);
                self.change_to_auto();
            }
            Pick::Eol => self.convert_eol(),
            Pick::Meta => self.open_meta(),
            Pick::Play => self.toggle_audio(),
            Pick::PdfText => self.open_pdf_text(),
            Pick::DocText => self.open_doc_text(),
            Pick::ArchiveRename => self.open_archive_edit(),
        }
    }

    // ---- 第4段階: メニュー・ツールバーの動作 ----

    fn perform(self: &Rc<Self>, a: Action) {
        match a {
            Action::Open => self.open_dialog(),
            Action::OpenFolder => self.open_folder_dialog(),
            Action::Save => {
                self.save(false);
            }
            Action::SaveAs => {
                self.save(true);
            }
            Action::Exit => {
                if self.confirm_quit() {
                    self.quit();
                }
            }
            Action::Undo => self.editor.undo(),
            Action::Redo => self.editor.redo(),
            Action::Cut => self.editor.cut(),
            Action::Copy => self.editor.copy(),
            Action::Paste => self.editor.paste(),
            Action::SelectAll => {
                let mut b = self.doc().buf.clone();
                b.select(0, b.length());
            }
            Action::Replace => {
                self.replace_action();
                return; // 置換欄へ移ったので、編集領域へフォーカスを戻さない
            }
            Action::Search => self.search_selection(),
            Action::SearchWeb => self.search_web(),
            Action::OpenImageUrl => self.open_image_url(),
            Action::RunCmd => self.run_cmd(),
            Action::ChangeEncoding => self.encoding_menu(false),
            Action::ChangeView => self.encoding_menu(true),
            Action::ToggleResume => self.toggle_resume(),
            Action::CopyPath => self.copy_path(),
            Action::ChooseFont => self.choose_font(),
            Action::ResetFont => self.reset_font(),
            Action::OpenReadme => self.open_readme(),
            Action::OpenLicenses => self.open_licenses(),
            Action::NewTab => self.new_tab(),
            Action::CloseTab => self.close_tab(self.active.get()),
            Action::NextTab => self.activate_relative(1),
            Action::PrevTab => self.activate_relative(-1),
            Action::OpenHomePage => self.open_site(platform::HOMEPAGE),
            Action::CheckUpdate => self.open_site(platform::DOWNLOAD_URL),
            Action::MakeShortcut => self.make_shortcut(),
            Action::OpenRecent => self.recent_menu(),
            Action::OpenConfig => self.open_config(),
            Action::ViewHistory => self.view_history(),
            Action::RestoreHistory => self.restore_history(),
            Action::CopyToHistory => self.copy_to_history(),
            Action::PasteFromHistory => self.paste_from_history(),
            Action::FormatJson => self.format_json(),
            Action::ToggleAutoSave => self.toggle_autosave(),
            Action::ToggleWrap => self.set_wrap(!self.wrap.get()),
            Action::GotoLine => self.goto_line(),
            Action::ConvertEol => self.convert_eol(),
            Action::ChooseTheme => self.choose_theme(),
            Action::ToggleLang => {
                let next = if crate::lang::is_en() { crate::lang::Lang::Ja } else { crate::lang::Lang::En };
                self.perform(Action::SetLang(next));
            }
            Action::SetLang(l) => {
                crate::lang::set(l);
                self.apply_lang();
                self.persist();
                self.flash(crate::lang::tr("日本語に切り替えました", "Switched to English"));
            }
            Action::SetTheme(i) => {
                if let Some(p) = theme::PALETTES.get(i) {
                    theme::set_named(p.name);
                    self.apply_theme();
                    self.persist();
                    self.flash(&format!("{}: {}", crate::lang::tr("配色", "Theme"), p.name));
                }
            }
            Action::EditCustomTheme => {
                if theme::custom().is_none() {
                    theme::store_custom(theme::palette());
                }
                theme::set_named(theme::CUSTOM_NAME);
                self.apply_theme();
                self.persist(); // custom_* を書いてから開く
                self.open_config();
                self.flash(crate::lang::tr("custom_* を書き換えて保存すると反映されます", "Edit custom_* and save to apply"));
            }
            Action::ToggleTheme => {
                theme::set_light(!theme::is_light());
                self.apply_theme();
                self.persist();
                self.flash(if theme::is_light() { crate::lang::tr("ライトテーマ", "Light theme") } else { crate::lang::tr("ダークテーマ", "Dark theme") });
            }
            Action::UndoAll => self.history_all(true),
            Action::RedoAll => self.history_all(false),
            Action::SelectWord => self.select_word(),
            Action::MagicSelect => self.magic_select(),
            Action::Deselect => self.doc().buf.clone().unselect(),
            Action::CopySelection => self.copy_selection(),
            Action::CopyLine => self.copy_line(),
            Action::CutLine => self.cut_line(),
            Action::CopyFileName => self.copy_file_name(),
            Action::ChangeSize => self.ask_size(),
            Action::ToggleFullScreen => self.toggle_fullscreen(),
            Action::CountText => self.show_counts(),
            Action::PasteBefore => self.paste_beside(true),
            Action::PasteAfter => self.paste_beside(false),
        }
        let _ = self.editor.clone().take_focus();
    }

    fn context_menu(self: &Rc<Self>) {
        if let Some(a) = menu::popup(menu::CONTEXT, app::event_x(), app::event_y()) {
            self.perform(a);
        }
    }

    /// ツールバー右の余白に短い通知を出し、少ししたら消す。ダイアログは出さない。
    fn flash(&self, msg: &str) {
        let mut g = self.gap.clone();
        g.set_label(msg);
        g.redraw();
        let mut g2 = self.gap.clone();
        app::add_timeout3(1.8, move |_| {
            g2.set_label("");
            g2.redraw();
        });
    }

    fn copy_path(&self) {
        match self.doc().st.borrow().path.as_ref() {
            Some(p) => {
                app::copy(&p.display().to_string());
                self.flash(crate::lang::tr("パスをコピーしました", "Path copied"));
            }
            None => self.flash(crate::lang::tr("未保存のファイル", "Unsaved File")),
        }
    }

    fn copy_file_name(&self) {
        match self.doc().st.borrow().path.as_ref().and_then(|p| p.file_name()) {
            Some(n) => {
                app::copy(&n.to_string_lossy());
                self.flash(crate::lang::tr("ファイル名をコピーしました", "File name copied"));
            }
            None => self.flash(crate::lang::tr("未保存のファイル", "Unsaved File")),
        }
    }

    fn toggle_fullscreen(&self) {
        let mut win = self.win.clone();
        let on = !win.fullscreen_active();
        win.fullscreen(on);
    }

    fn show_counts(&self) {
        let d = self.doc();
        if d.st.borrow().hex {
            return;
        }
        let text = d.buf.selection_text();
        let whole = text.is_empty();
        let (text, scope) = if whole { (d.buf.text(), "") } else { (text, crate::lang::tr("（選択範囲）", " (selection)")) };
        let (cl, wl, ll) = if crate::lang::is_en() { ("Chars", "Words", "Lines") } else { ("文字数", "単語数", "行数") };
        // 全体の行数は行番号と同じ数え方（末尾の改行の後ろの空の行も 1 行）。右端の欄の Lines と合わせる
        let lines = if whole { d.buf.count_lines(0, d.buf.length()) as usize + 1 } else { text.lines().count() };
        confirm::message(&format!(
            "{cl}{scope}: {}\n{wl}{scope}: {}\n{ll}{scope}: {}",
            document::char_count(&text),
            document::word_count(&text),
            lines
        ));
    }

    fn ask_size(&self) {
        let prompt = if crate::lang::is_en() { format!("Font size ({}-{}, default {})", zoom::MIN, zoom::MAX, zoom::DEFAULT) } else { format!("文字の大きさ（{}〜{}、既定 {}）", zoom::MIN, zoom::MAX, zoom::DEFAULT) };
        let Some(input) = dialog::input_default(&prompt, &self.size.get().to_string()) else { return };
        match input.trim().parse::<i32>() {
            Ok(n) => self.set_size(n),
            Err(_) => self.flash(crate::lang::tr("数字を入れてください", "Enter a number")),
        }
    }

    /// 右クリックの Replace / Ctrl+H: 選択が 1 行（か無選択）なら、検索バーの置換欄へ移る。
    /// 複数行の選択は検索欄に入らないので、従来どおり入力ダイアログで聞く。
    fn replace_action(&self) {
        if self.doc().buf.selection_text().contains(['\n', '\r']) {
            self.replace_selection();
            let _ = self.editor.clone().take_focus();
        } else {
            self.focus_replace();
        }
    }

    /// 検索バーの All / 置換欄の Enter: 検索中の文字列（強調している箇所）を全て置換欄の文字列にする。
    /// 全文を 1 回だけ読み、1 回の編集として書き戻す（「元に戻す」1 回で全て戻る）。置換結果の中は再走査しない。
    /// Repl: いま選んでいる一致を置換欄の文字列に置き換えて、次の一致へ進む（選んでいなければ最初の一致を選ぶだけ）。
    fn replace_one_bar(&self) {
        let d = self.doc();
        if d.st.borrow().loading {
            return;
        }
        let needle = self.last_search.borrow().clone();
        if needle.is_empty() {
            self.flash(crate::lang::tr("検索する文字列がありません", "No search text"));
            return;
        }
        let smart = self.last_smart.get();
        let text = d.buf.text();
        let hits = if smart { replace::find_all_smart(&text, &needle) } else { replace::find_all(&text, &needle) };
        let sel = d.buf.selection_position().map(|(a, b)| (a.min(b).max(0) as usize, a.max(b).max(0) as usize));
        if let Some(&(s, e)) = hits.iter().find(|h| Some(**h) == sel) {
            let with = self.find_bar.replace.value();
            let mut b = d.buf.clone();
            b.replace(s as i32, e as i32, &with);
            b.unselect();
            self.editor.clone().set_insert_position((s + with.len()) as i32);
            let n = self.highlight(&needle, smart);
            self.show_find_count(&needle, n);
        }
        self.find_move(true, true);
    }

    fn replace_all_bar(&self) {
        let d = self.doc();
        if d.st.borrow().loading {
            return;
        }
        let needle = self.last_search.borrow().clone();
        if needle.is_empty() {
            self.flash(crate::lang::tr("検索する文字列がありません", "No search text"));
            return;
        }
        let with = self.find_bar.replace.value();
        let text = d.buf.text();
        let hits = if self.last_smart.get() { replace::find_all_smart(&text, &needle) } else { replace::find_all(&text, &needle) };
        if hits.is_empty() {
            self.set_find_count(crate::lang::tr("見つかりません", "Not found"));
            self.flash(crate::lang::tr("見つかりません", "Not found"));
            return;
        }
        let new_text = replace::apply_replace(&text, &hits, &with);
        let pos = self.editor.insert_position();
        let mut b = d.buf.clone();
        b.replace(0, b.length(), &new_text);
        let pos = b.utf8_align(pos.min(b.length()));
        self.editor.clone().set_insert_position(pos);
        log::info(TAG, &format!("replace: {} 件", hits.len()));
        let n = self.highlight(&needle, self.last_smart.get());
        self.show_find_count(&needle, n);
        self.flash(&if crate::lang::is_en() { format!("Replaced {}", hits.len()) } else { format!("{} 件を置換しました", hits.len()) });
    }

    /// 選択文字列を全て別の文字列に置き換える（複数行の選択用）。全文を 1 回だけ読み、1 回の編集として書き戻す
    /// （「元に戻す」1 回で全て戻る）。
    fn replace_selection(&self) {
        let d = self.doc();
        if d.st.borrow().loading {
            return;
        }
        let target = d.buf.selection_text();
        if target.is_empty() {
            confirm::message(crate::lang::tr("先に置換したい文字列を選択してください。", "Select the text to replace first."));
            return;
        }
        let shown: String = target.chars().take(40).collect();
        let more = if target.chars().count() > 40 { "…" } else { "" };
        let Some(with) = dialog::input_default(&if crate::lang::is_en() { format!("Replace \"{shown}{more}\" with what? (replace all)") } else { format!("「{shown}{more}」を何に置き換えますか（全て置換）") }, "") else {
            return;
        };
        let text = d.buf.text();
        match replace::replace_all(&text, &target, &with) {
            Some((new_text, n)) => {
                let pos = self.editor.insert_position();
                let mut b = d.buf.clone();
                b.replace(0, b.length(), &new_text);
                let pos = b.utf8_align(pos.min(b.length()));
                self.editor.clone().set_insert_position(pos);
                log::info(TAG, &format!("replace: {n} 件"));
                self.flash(&if crate::lang::is_en() { format!("Replaced {n}") } else { format!("{n} 件を置換しました") });
            }
            None => self.flash(crate::lang::tr("見つかりません", "Not found")),
        }
    }

    fn style_table(font: Font, size: i32) -> Vec<StyleTableEntryExt> {
        vec![
            StyleTableEntryExt { color: rgb(color_text()), font, size, attr: TextAttr::None, bgcolor: rgb(color_main()) },
            StyleTableEntryExt { color: rgb(color_text()), font, size, attr: TextAttr::BgColor, bgcolor: rgb(mix(color_main(), color_accent(), 40)) },
            // 'C' 以降はコードの色分け（mcfunction.rs の定数と同じ順。code.rs も同じ文字を使う）
            Self::syntax(color_subtext(), font, size),
            Self::syntax(color_accent(), font, size),
            Self::syntax(mix(color_text(), (200, 120, 230), 80), font, size),
            Self::syntax(mix(color_text(), (90, 170, 240), 80), font, size),
            Self::syntax(mix(color_text(), (230, 170, 60), 80), font, size),
            Self::syntax(mix(color_text(), (120, 200, 120), 80), font, size),
            Self::syntax(mix(color_text(), (240, 110, 110), 80), font, size),
            // 'J' 確実な誤り: 色は変えずに下線だけ
            StyleTableEntryExt { color: rgb(mix(color_text(), (240, 80, 80), 70)), font, size, attr: TextAttr::Underline, bgcolor: rgb(color_main()) },
        ]
    }

    fn syntax(color: (u8, u8, u8), font: Font, size: i32) -> StyleTableEntryExt {
        StyleTableEntryExt { color: rgb(color), font, size, attr: TextAttr::None, bgcolor: rgb(color_main()) }
    }

    /// フォントファイル（.ttf / .otf）を選び、編集領域・行番号・ハイライトに適用する。
    /// 選んだフォントは config.txt の `font=` に残り、次の起動でも使う。
    fn choose_font(&self) {
        let mut dlg = NativeFileChooser::new(FileDialogType::BrowseFile);
        dlg.set_title(crate::lang::tr("フォントファイルを選択（.ttf / .otf）", "Select Font File (.ttf or .otf)"));
        dlg.set_filter(crate::lang::tr("フォントファイル\t*.{ttf,otf}", "Font files\t*.{ttf,otf}"));
        if let Some(d) = platform::font_dir() {
            let _ = dlg.set_directory(&d);
        }
        dlg.show();
        let path = dlg.filename();
        if path.as_os_str().is_empty() {
            return; // キャンセル
        }
        // App::load_font は読んだフォントを固定の枠（番号 16）に入れる。一度に使えるのは 1 つだけで、
        // 選び直すと前のフォント（同梱の NeoFont を含む）は置き換わる。
        match self.app.load_font(&path) {
            Ok(name) if Font::by_name(&name).bits() == LOADED_FONT_SLOT => {
                self.apply_font(Font::by_name(&name));
                *self.font_path.borrow_mut() = Some(path.clone());
                self.persist();
                log::info(TAG, &format!("font: {name}"));
                self.flash(crate::lang::tr("フォントを変更しました", "Font changed"));
            }
            other => {
                let why = match other {
                    Ok(name) => if crate::lang::is_en() { format!("could not use \"{name}\"") } else { format!("「{name}」を使えませんでした") },
                    Err(e) => e.to_string(),
                };
                log::error(TAG, &format!("フォントの読込に失敗: {} ({why})", log::file_name(&path.to_string_lossy())));
                confirm::alert(&if crate::lang::is_en() { format!("Could not load the font. Check that it is a valid font file (.ttf / .otf).\n\n{}\n{why}", path.display()) } else { format!("フォントを読み込めませんでした。有効なフォントファイル（.ttf / .otf）か確認してください。\n\n{}\n{why}", path.display()) });
            }
        }
    }

    /// 同梱の NeoFont（既定のフォント）に戻す。
    fn reset_font(&self) {
        let loaded = platform::bundled_font_path()
            .map_err(|e| e.to_string())
            .and_then(|p| self.app.load_font(&p).map_err(|e| e.to_string()));
        match loaded {
            Ok(name) if Font::by_name(&name).bits() == LOADED_FONT_SLOT => {
                self.apply_font(Font::by_name(&name));
                *self.font_path.borrow_mut() = None;
                self.persist();
                log::info(TAG, &format!("font: {name} (default)"));
                self.flash(crate::lang::tr("既定のフォント", "Default font"));
            }
            other => {
                let why = match other {
                    Ok(name) => if crate::lang::is_en() { format!("could not use \"{name}\"") } else { format!("「{name}」を使えませんでした") },
                    Err(e) => e,
                };
                log::error(TAG, &format!("既定フォントに戻せませんでした: {why}"));
                confirm::alert(&if crate::lang::is_en() { format!("Could not restore the default font.\n\n{why}") } else { format!("既定のフォントに戻せませんでした。\n\n{why}") });
            }
        }
    }

    /// 同梱の README を本文のタブとして、Markdown の Preview で開く（すでに開いていればそのタブへ移る）。
    fn open_readme(self: &Rc<Self>) {
        let Some(d) = self.open_bundled("README.md", &markdown::readme_to_md(README_TEXT)) else { return };
        d.format_auto.set(false);
        d.preview_as.set(Some(viewmode::Kind::Markdown));
        self.set_preview_for(&d, true);
    }

    /// 第三者のライセンス文を、本文のタブとして文字のまま開く（長いので Preview にはしない）。
    fn open_licenses(self: &Rc<Self>) {
        self.open_bundled("THIRD_PARTY_LICENSES.txt", THIRD_PARTY_LICENSES);
    }

    /// 同梱の文を、保存先の無いタブとして開く。同じ名前のタブがあれば、そこへ移るだけ（None）。
    fn open_bundled(self: &Rc<Self>, name: &str, text: &str) -> Option<Rc<Doc>> {
        let title = PathBuf::from(name);
        let open = self.docs.borrow().iter().find(|d| d.st.borrow().path.is_none() && d.st.borrow().title.as_ref() == Some(&title)).cloned();
        if let Some(d) = open {
            self.activate(d.id);
            return None;
        }
        let d = self.new_doc();
        self.set_text_quiet(&d, text);
        {
            let mut st = d.st.borrow_mut();
            st.title = Some(title);
            st.chars = document::char_count(text);
        }
        self.activate(d.id);
        self.refresh_tabs();
        Some(d)
    }

    /// 今のパレットに合わせて、画面の色を全部塗り直す（モードの切替・設定ファイルの反映のとき）。
    /// 言語を切り替えたあと、作るときに決めた名前・ツールチップを今の言語に合わせ直す。
    /// メニュー・確認画面・お知らせは出すたびに言語を見るので、ここでは触らない。
    fn apply_lang(&self) {
        toolbar::relabel(&self.bar, &self.top_pin, &self.gap);
        self.refresh_tabs();
        self.refresh_title();
    }

    fn apply_theme(&self) {
        skin::apply_globals();
        skin::style_scrollbars(&self.editor);
        skin::style_scrollbars(&self.preview);
        let mut ed = self.editor.clone();
        ed.set_color(rgb(color_main()));
        ed.set_text_color(rgb(color_text()));
        ed.set_cursor_color(rgb(color_accent()));
        ed.set_selection_color(rgb(color_accent()));
        ed.set_linenumber_bgcolor(rgb(color_sub()));
        ed.set_linenumber_fgcolor(rgb(color_subtext()));
        if self.styled.get() {
            // Search のハイライトの「通常」の文字色と地色もパレットに合わせる
            let sb = self.ensure_sbuf(&self.doc());
            ed.set_highlight_data_ext(sb, Self::style_table(self.font.get(), self.size.get()));
        }
        ed.redraw();
        self.refresh_preview();
        self.win.clone().set_color(rgb(color_main()));
        toolbar::restyle(&self.bar, &self.gap);
        toolbar::refresh_icon_tips(&self.bar);
        searchbar::restyle(&self.find_bar);
        let mut tb = self.tabbar.borrow().clone();
        tb.set_color(rgb(color_sub()));
        tabs::restyle_chip(&mut self.chip.clone());
        tabs::restyle_chip(&mut self.view_chip.clone());
        self.style_pin();
        // タブは色を持った部品の集まりなので、作り直して新しい色にする（メインループの中で呼ぶ）
        *self.tabs_shown.borrow_mut() = (Vec::new(), 0);
        self.refresh_tabs();
        self.win.clone().redraw();
        log::info(TAG, &format!("theme: {}", theme::current_name()));
    }

    fn apply_font(&self, font: Font) {
        self.font.set(font);
        let mut ed = self.editor.clone();
        ed.set_text_font(font);
        ed.set_linenumber_font(font);
        self.find_bar.input.clone().set_text_font(font);
        if self.styled.get() {
            let sb = self.ensure_sbuf(&self.doc());
            ed.set_highlight_data_ext(sb, Self::style_table(font, self.size.get()));
        }
        ed.redraw();
    }

    /// 文書のスタイルバッファ（まだ無ければ本文と同じ長さで作る）。
    fn ensure_sbuf(&self, d: &Doc) -> TextBuffer {
        if let Some(sb) = d.sbuf.borrow().as_ref() {
            return sb.clone();
        }
        let mut sb = TextBuffer::default();
        sb.set_text(&"A".repeat(d.buf.length() as usize));
        *d.sbuf.borrow_mut() = Some(sb.clone());
        sb
    }

    /// 表示中の文書のハイライト用スタイルバッファを（まだ無ければ）作って編集領域に付ける。
    fn style_buffer(&self) -> TextBuffer {
        let d = self.doc();
        self.styled.set(true);
        let sb = self.ensure_sbuf(&d);
        self.editor.clone().set_highlight_data_ext(sb.clone(), Self::style_table(self.font.get(), self.size.get()));
        sb
    }

    /// 右クリックの Search: 選択文字列と同じ箇所を全て黄色にする（厳密）。何も選択していなければ消す。
    /// 検索バーにも同じ文字列を入れる（F3 で次へ進める）。複数行の選択は入力欄に入らないので、強調だけ。
    fn search_selection(&self) {
        let sel = self.doc().buf.selection_text();
        let needle = sel.trim();
        self.last_smart.set(false);
        if !needle.contains(['\n', '\r']) {
            self.find_bar.input.clone().set_value(needle);
            *self.last_search.borrow_mut() = needle.to_string();
        }
        let n = self.highlight(needle, false);
        self.show_find_count(needle, n);
        if !needle.is_empty() && !needle.contains(['\n', '\r']) {
            self.find_bar.set_open(true); // 件数と ← → が見えるように開く（フォーカスは編集領域のまま）
        }
    }

    /// `needle` と同じ箇所を全て黄色にして、その数を返す。空なら強調を消す（0）。
    /// `smart` なら大文字小文字を（小文字だけで探すときは）無視する。
    fn highlight(&self, needle: &str, smart: bool) -> usize {
        let d = self.doc();
        let path = d.st.borrow().path.clone();
        let syntax = code::has_syntax(path.as_deref()) && !d.st.borrow().hex;
        if needle.is_empty() && !syntax {
            if let Some(sb) = d.sbuf.borrow().as_ref() {
                let mut sb = sb.clone();
                sb.set_text(&"A".repeat(d.buf.length() as usize));
            }
            return 0;
        }
        let text = d.buf.text();
        let hits = if needle.is_empty() {
            Vec::new()
        } else if smart {
            replace::find_all_smart(&text, needle)
        } else {
            replace::find_all(&text, needle)
        };
        let mut styles = if syntax { code::syntax_styles(path.as_deref(), &text) } else { None }.unwrap_or_else(|| vec![b'A'; text.len()]);
        for &(s, e) in &hits {
            styles[s..e].fill(b'B');
        }
        let mut sb = self.style_buffer();
        // 英字だけなので常に有効な UTF-8
        sb.set_text(std::str::from_utf8(&styles).unwrap_or(""));
        self.editor.clone().redraw();
        hits.len()
    }

    /// 検索バーの件数の欄。
    /// 開いていれば件数の欄に、いつでもツールチップに出す。
    fn set_find_count(&self, text: &str) {
        self.find_bar.set_status(text);
    }

    /// 見つかった数を出す（0 件は Not found、空の検索は何も出さない）。
    fn show_find_count(&self, needle: &str, n: usize) {
        self.set_find_count(&if needle.is_empty() { String::new() } else if n == 0 { crate::lang::tr("見つかりません", "Not found").to_string() } else { if crate::lang::is_en() { format!("{n} found") } else { format!("{n} 件") } });
    }

    /// 検索バーの文字列が変わった: 全てを強調し、いまの位置から先の最初の 1 件を選ぶ（入力しても飛ばない）。
    fn find_changed(&self) {
        let needle = self.find_bar.input.value();
        *self.last_search.borrow_mut() = needle.clone();
        self.last_smart.set(true);
        let n = self.highlight(&needle, true);
        self.show_find_count(&needle, n);
        if n > 0 {
            self.find_move(true, true);
        }
    }

    /// 検索中なら、いまの文書に強調を付け直す（タブの切替・読込の後）。選択は動かさない。
    fn refresh_find(&self) {
        let needle = self.last_search.borrow().clone();
        let d = self.doc();
        let syntax = code::has_syntax(d.st.borrow().path.as_deref());
        if (needle.is_empty() && !syntax) || d.buf.length() > LIVE_COUNT_LIMIT {
            return;
        }
        let n = self.highlight(&needle, self.last_smart.get());
        self.show_find_count(&needle, n);
    }

    /// Ctrl+F: 検索バーへ移る。編集領域で選んでいる 1 行の文字列があれば、それを検索する（検索中の文字列そのものなら置き換えない）。
    /// 虫眼鏡: 閉じているか空なら開いて検索欄へ。開いていれば全てを強調して次の一致を選ぶ。
    fn search_button(&self) {
        let needle = self.find_bar.input.value();
        if !self.find_bar.is_open() || needle.is_empty() {
            self.focus_find();
        } else if *self.last_search.borrow() != needle {
            self.find_changed();
        } else {
            self.highlight(&needle, true);
            self.find_move(true, false);
        }
    }

    fn focus_find(&self) {
        self.prefill_find();
        let _ = self.find_bar.input.clone().take_focus();
        self.select_all_find();
    }

    /// 検索バーを開く。編集領域で選んでいる 1 行の文字列があれば、それを検索欄に入れて検索する。
    /// 入れたなら true（選択したものと同じ大文字小文字で探すかどうかは、呼び出し側が決める）。
    fn prefill_find(&self) -> bool {
        self.find_bar.set_open(true);
        let sel = self.doc().buf.selection_text();
        let sel = sel.trim();
        let mut input = self.find_bar.input.clone();
        if !sel.is_empty() && !sel.contains(['\n', '\r']) && sel.to_lowercase() != input.value().to_lowercase() {
            input.set_value(sel);
            self.find_changed();
            return true;
        }
        false
    }

    /// Ctrl+H / 右クリックの Replace: 検索バーを開いて置換欄へ移る。選んだ文字列は、選んだとおり（大文字小文字を区別して）探す。
    fn focus_replace(&self) {
        if self.prefill_find() {
            let needle = self.find_bar.input.value();
            self.last_smart.set(false);
            let n = self.highlight(&needle, false);
            self.show_find_count(&needle, n);
        }
        let mut r = self.find_bar.replace.clone();
        let _ = r.take_focus();
        let _ = r.set_mark(0);
        let _ = r.set_position(r.value().len() as i32);
    }

    /// Esc: 検索バーを閉じて（文字列と強調は残す。F3 で続けられる）、編集領域へ戻る。
    fn close_find(&self) {
        self.find_bar.set_open(false);
        let _ = self.editor.clone().take_focus();
    }

    /// 検索バーの入力欄の文字を全て選ぶ（そのまま打ち直せるように）。
    fn select_all_find(&self) {
        let mut input = self.find_bar.input.clone();
        let _ = input.set_mark(0);
        let _ = input.set_position(input.value().len() as i32);
    }

    /// 検索バーの×: 文字列と強調を消して、編集領域へ戻る。
    fn clear_find(&self) {
        self.find_bar.input.clone().set_value("");
        self.last_search.borrow_mut().clear();
        self.highlight("", true);
        self.set_find_count("");
        self.close_find();
    }

    /// 検索バーの入力欄で受けるキー: Enter / Shift+Enter で次・前、Esc と Tab で編集領域へ戻る、Ctrl+F で全選択。
    /// それ以外のショートカット（F3・Ctrl+S・Ctrl+T など）は全体の処理（handle）へ回す。
    fn find_key(self: &Rc<Self>, ev: Event) -> bool {
        if self.dnd_event(ev) {
            return true;
        }
        if ev == Event::Focus {
            self.find_bar.set_open(true); // 検索欄が使われたら、真下に置換の段を出す
            return false;
        }
        if ev != Event::KeyDown && ev != Event::Shortcut {
            return false;
        }
        let key = app::event_key();
        let f3 = Key::from_i32(Key::F1.bits() + 2);
        if app::is_event_command() || app::is_event_ctrl() {
            if key == Key::from_char('f') {
                self.select_all_find();
                return true;
            }
            // Ctrl+Y は、ここでは編集領域のやり直しにしない
            return key != Key::from_char('y') && self.handle(ev);
        }
        match key {
            Key::Enter | Key::KPEnter => {
                self.find_move(!app::is_event_shift(), false);
                true
            }
            Key::Escape => {
                self.close_find();
                true
            }
            Key::Tab => {
                let _ = self.find_bar.replace.clone().take_focus(); // 検索欄 → 置換欄
                true
            }
            k if k == f3 => self.handle(ev),
            _ => false,
        }
    }

    /// 置換欄で受けるキー: Enter で全て置換、Esc で閉じる、Tab で編集領域へ。それ以外のショートカットは全体の処理へ回す。
    fn replace_key(self: &Rc<Self>, ev: Event) -> bool {
        if self.dnd_event(ev) {
            return true;
        }
        if ev == Event::Focus {
            self.find_bar.set_open(true);
            return false;
        }
        if ev != Event::KeyDown && ev != Event::Shortcut {
            return false;
        }
        let key = app::event_key();
        let f3 = Key::from_i32(Key::F1.bits() + 2);
        if app::is_event_command() || app::is_event_ctrl() {
            if key == Key::from_char('h') {
                let mut r = self.find_bar.replace.clone();
                let _ = r.set_mark(0);
                let _ = r.set_position(r.value().len() as i32);
                return true;
            }
            return key != Key::from_char('y') && self.handle(ev);
        }
        match key {
            Key::Enter | Key::KPEnter => {
                self.replace_all_bar();
                true
            }
            Key::Escape => {
                self.close_find();
                true
            }
            Key::Tab => {
                let _ = self.editor.clone().take_focus();
                true
            }
            k if k == f3 => self.handle(ev),
            _ => false,
        }
    }

    /// 検索中の文字列の、次（または前）の 1 件を選ぶ。末尾（先頭）の先は反対側へ戻る。
    /// `keep_current` なら、いま選んでいる一致もそのまま使う（入力のたびに先へ飛ばないように）。
    fn find_move(&self, forward: bool, keep_current: bool) {
        let needle = self.last_search.borrow().clone();
        if needle.is_empty() {
            self.focus_find(); // 何も探していなければ、検索バーへ移る
            return;
        }
        let d = self.doc();
        let text = d.buf.text();
        let hits = if self.last_smart.get() { replace::find_all_smart(&text, &needle) } else { replace::find_all(&text, &needle) };
        if hits.is_empty() {
            self.set_find_count(crate::lang::tr("見つかりません", "Not found"));
            if !self.find_bar.is_open() {
                self.flash(crate::lang::tr("見つかりません", "Not found"));
            }
            return;
        }
        let (cs, ce) = d.buf.selection_position().map_or_else(
            || {
                let p = self.editor.insert_position().max(0) as usize;
                (p, p)
            },
            |(a, b)| (a.min(b).max(0) as usize, a.max(b).max(0) as usize),
        );
        let found = if forward {
            let from = if keep_current { cs } else { ce.max(cs) };
            hits.iter().find(|h| h.0 >= from).or(hits.first())
        } else {
            hits.iter().rev().find(|h| h.1 <= cs).or(hits.last())
        };
        if let Some(&(s, e)) = found {
            self.set_selection(s, e);
            self.editor.clone().show_insert_position();
            let n = hits.iter().position(|h| h.0 == s).map_or(0, |i| i + 1);
            let text = format!("{n}/{}", hits.len());
            self.set_find_count(&text);
            if !self.find_bar.is_open() {
                self.flash(&text); // 閉じているときは、件数を余白に一瞬だけ出す
            }
        }
    }

    /// 選んでいる URL の画像を、curl で取ってきて小さなウィンドウに出す（開いただけでは通信しない。選んだときだけ）。
    fn open_image_url(&self) {
        let sel = self.doc().buf.selection_text();
        let url = sel.trim().to_string();
        if !platform::is_http_url(&url) {
            self.flash(crate::lang::tr("画像の URL を選んでください", "Choose an image URL"));
            return;
        }
        self.flash(crate::lang::tr("画像を取得しています...", "Fetching image..."));
        log::info(TAG, "open image url");
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = fetch_image(&url).map(|p| (p, url)).map_err(|e| e.to_string());
            tx.send(Msg::ImageFetched(r));
            app::awake();
        });
    }

    fn on_image_fetched(self: &Rc<Self>, r: Result<(PathBuf, String), String>) {
        match r {
            // 取ってきた画像は一時ファイルとして、本文のタブで開く
            Ok((path, _url)) => self.open_paths(vec![path]),
            Err(e) => {
                log::warn(TAG, &format!("image url: {}", e.replace('\n', " / ")));
                self.flash(crate::lang::tr("画像を取得できません", "Cannot fetch the image"));
                confirm::alert(&if crate::lang::is_en() { format!("Could not fetch the image.\n\n{e}") } else { format!("画像を取得できませんでした。\n\n{e}") });
            }
        }
    }

    fn search_web(&self) {
        let url = platform::search_url(&self.doc().buf.selection_text());
        log::info(TAG, "search in web");
        if let Err(e) = platform::open_url(&url) {
            log::warn(TAG, &format!("ブラウザを開けませんでした: {e}"));
            // 失敗したらホームページを開く（Java 版と同じ）
            if url == platform::HOMEPAGE || platform::open_url(platform::HOMEPAGE).is_err() {
                confirm::alert(&if crate::lang::is_en() { format!("Could not open the browser.\n\n{e}") } else { format!("ブラウザを開けませんでした。\n\n{e}") });
            }
        }
    }

    /// 選択ありならそれをコマンドとして実行、無ければカレントフォルダを開く。確認なし（設計判断）。
    fn run_cmd(&self) {
        let sel = self.doc().buf.selection_text();
        if sel.is_empty() {
            let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if let Err(e) = platform::open_folder(&dir) {
                self.run_failed(&e.to_string());
            }
            return;
        }
        if CONFIRM_RUN_CMD {
            let shown: String = sel.chars().take(200).collect();
            if confirm::choice(&if crate::lang::is_en() { format!("Run the following?\n\n{shown}") } else { format!("次を実行します。\n\n{shown}") }, crate::lang::tr("キャンセル", "Cancel"), crate::lang::tr("実行", "Run"), "") != Some(1) {
                return;
            }
        }
        log::info(TAG, &format!("run: {} 文字", sel.chars().count()));
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = platform::run_shell(&sel).map_err(|e| e.to_string());
            tx.send(Msg::Ran(r));
            app::awake();
        });
    }

    fn run_failed(&self, e: &str) {
        log::error(TAG, &format!("コマンド実行に失敗: {e}"));
        confirm::alert(&if crate::lang::is_en() { format!("NeoErrorCode:250 | FallbackCrash - An exception occurred while running the command: {e}") } else { format!("NeoErrorCode:250 | FallbackCrash - コマンド実行中に例外が発生しました: {e}") });
    }

    fn on_ran(&self, result: Result<String, String>) {
        match result {
            Ok(out) => {
                let mut shown: String = out.chars().take(RUN_OUTPUT_LIMIT).collect();
                if out.chars().count() > RUN_OUTPUT_LIMIT {
                    shown.push_str(crate::lang::tr("\n…（以下省略）", "\n… (truncated)"));
                }
                if shown.is_empty() {
                    shown = crate::lang::tr("（出力なし）", "(no output)").to_string();
                }
                dialog::message_title(crate::lang::tr("実行結果", "Run Result"));
                confirm::message(&shown);
            }
            Err(e) => self.run_failed(&e),
        }
    }

    // ---- 残りの項目（Open HomePage / Open Recent / Auto Save / 選択系 など）----

    /// ブラウザで URL を開く。開けなければ理由を出す。
    fn open_site(&self, url: &str) {
        log::info(TAG, &format!("open site: {url}"));
        if let Err(e) = platform::open_url(url) {
            log::warn(TAG, &format!("ブラウザを開けませんでした: {e}"));
            confirm::alert(&if crate::lang::is_en() { format!("Could not open the browser.\n\n{e}") } else { format!("ブラウザを開けませんでした。\n\n{e}") });
        }
    }

    fn make_shortcut(&self) {
        match platform::make_shortcut() {
            Ok(p) => {
                log::info(TAG, &format!("shortcut: {}", log::file_name(&p.to_string_lossy())));
                self.flash(crate::lang::tr("ショートカットを作りました", "Shortcut created"));
            }
            Err(e) => {
                log::warn(TAG, &format!("ショートカットを作れませんでした: {e}"));
                confirm::alert(&if crate::lang::is_en() { format!("Could not create the shortcut.\n\n{e}") } else { format!("ショートカットを作れませんでした。\n\n{e}") });
            }
        }
    }

    // ---- 設定ファイル（設定フォルダの config.txt）----

    fn is_config_file(&self, p: &Path) -> bool {
        self.config_path.as_deref().is_some_and(|c| document::same_file(c, p))
    }

    /// 最近開いたファイルに加えて保存する。設定ファイル自身は一覧に入れない（開くたびに書き換わるため）。
    fn remember_recent(&self, p: PathBuf) {
        if self.is_config_file(&p) {
            return;
        }
        pathlog::push(&mut self.path_history.borrow_mut(), p.clone());
        recent::push(&mut self.recent.borrow_mut(), p);
        self.persist();
    }

    /// 終了時に開いているタブ（左から順）と、表示しているタブ。保存先が決まっているものだけ。
    /// 設定ファイル自身は入れない（recent と同じ理由）。`skip` の文書は数えない（いま閉じるところ）。
    fn session_tabs(&self, skip: Option<u64>) -> (Vec<PathBuf>, Option<PathBuf>) {
        let active = self.active.get();
        let mut list: Vec<PathBuf> = Vec::new();
        let mut shown = None;
        for d in self.docs.borrow().iter() {
            if Some(d.id) == skip {
                continue;
            }
            let st = d.st.borrow();
            let Some(p) = st.path.clone().or_else(|| st.loading_path.clone()) else { continue };
            if self.is_config_file(&p) || list.contains(&p) {
                continue;
            }
            if d.id == active {
                shown = Some(p.clone());
            }
            list.push(p);
            if list.len() >= config::SESSION_MAX {
                break;
            }
        }
        (list, shown)
    }

    fn current_config(&self) -> config::Config {
        self.current_config_skip(None)
    }

    fn current_config_skip(&self, skip: Option<u64>) -> config::Config {
        let (session, session_active) = if self.session_owner {
            self.session_tabs(skip)
        } else {
            // 引数ありで起動したウィンドウは、保存済みのセッションをそのまま残す（上書きしない）
            self.config_path.as_deref().map(config::load).map(|c| (c.session, c.session_active)).unwrap_or_default()
        };
        config::Config {
            session,
            session_active,
            autosave: self.autosave.get(),
            wrap: self.wrap.get(),
            gutter_pin: self.gutter_pinned.get(),
            side_pin: self.side_pinned.get(),
            gutter_extra: Some(self.gutter_extra.get()).filter(|w| *w != 0),
            side_w: Some(self.side_w.get()).filter(|w| *w != sidepanel::WIDTH),
            resume: self.resume.get(),
            lang: crate::lang::code(crate::lang::current()).to_string(),
            light: theme::is_light(),
            theme: Some(theme::current_name().to_string()),
            custom: theme::custom().map(|p| [p.main, p.sub, p.accent, p.text, p.subtext]),
            size: (self.size.get() != zoom::DEFAULT).then(|| self.size.get()),
            font: self.font_path.borrow().clone(),
            recent: self.recent.borrow().clone(),
            history: self.path_history.borrow().clone(),
        }
    }

    /// 今の状態を設定ファイルへ書く。キーの行だけを書き換えるので、ユーザーが足したコメントなどは残る。
    /// 書けなくても（インストール先が書込禁止など）編集は止めず、ログに出すだけ。
    fn persist(&self) {
        self.persist_skip(None);
    }

    fn persist_skip(&self, skip: Option<u64>) {
        let Some(path) = self.config_path.clone() else { return };
        match config::save(&path, &self.current_config_skip(skip)) {
            Ok(false) => {}
            Ok(true) => {
                // 設定ファイルをエディタで開いたままなら、古い内容を見せないよう読み直す（編集中なら触らない）
                if let Some(d) = self.find_by_path(&path) {
                    let idle = {
                        let st = d.st.borrow();
                        !st.dirty && !st.loading && !st.hex
                    };
                    if idle {
                        self.load_into(&d, path, None);
                    }
                }
            }
            Err(e) => log::warn(TAG, &format!("設定を保存できませんでした: {} ({e})", path.display())),
        }
    }

    /// 設定ファイルをタブで開く。まだ無ければ今の状態で作ってから開く。
    fn open_config(self: &Rc<Self>) {
        let Some(path) = self.config_path.clone() else {
            confirm::alert(crate::lang::tr("設定ファイルの場所を決められませんでした（実行ファイルの場所を取得できません）。", "Could not decide where the config file is (the program location is unknown)."));
            return;
        };
        if !path.exists() {
            if let Err(e) = config::save(&path, &self.current_config()) {
                log::error(TAG, &format!("設定ファイルを作れませんでした: {} ({e})", path.display()));
                confirm::alert(&if crate::lang::is_en() { format!("Could not create the config file.\n\n{}\n{e}", path.display()) } else { format!("設定ファイルを作れませんでした。\n\n{}\n{e}", path.display()) });
                return;
            }
        }
        self.open_paths(vec![path]);
    }

    /// エディタで保存された設定ファイルを読み直して、起動中の状態に反映する。
    fn apply_config(self: &Rc<Self>) {
        let Some(path) = self.config_path.clone() else { return };
        let cfg = config::load(&path);

        // 最近開いたファイル
        *self.recent.borrow_mut() = cfg.recent.clone();
        *self.path_history.borrow_mut() = cfg.history.clone();

        // Enable Auto Save
        if cfg.autosave != self.autosave.get() {
            self.autosave.set(cfg.autosave);
            menu::set_autosave_checked(cfg.autosave);
            if cfg.autosave && !self.autosave_armed.get() {
                self.arm_autosave();
            }
        }

        // Word Wrap
        if cfg.wrap != self.wrap.get() {
            self.set_wrap_quiet(cfg.wrap);
        }

        // 配色
        let before = (theme::current_name(), theme::palette());
        apply_theme_config(&cfg);
        if let Some(l) = crate::lang::from_code(&cfg.lang).filter(|l| *l != crate::lang::current()) {
            crate::lang::set(l);
            self.apply_lang();
        }
        if before != (theme::current_name(), theme::palette()) {
            self.apply_theme();
        }

        // 行番号欄の固定
        if cfg.gutter_pin != self.gutter_pinned.get() {
            self.gutter_pinned.set(cfg.gutter_pin);
            self.update_gutter();
        }
        if cfg.side_pin != self.side_pinned.get() {
            self.side_pinned.set(cfg.side_pin);
            self.side.state.borrow_mut().pinned = cfg.side_pin;
            self.update_side();
        }
        if cfg.resume != self.resume.get() {
            self.resume.set(cfg.resume);
            menu::set_resume_checked(cfg.resume);
            if !cfg.resume {
                if let Some(dir) = self.unsaved_dir() {
                    let _ = unsaved::clear(&dir);
                }
            }
        }
        let ge = cfg.gutter_extra.unwrap_or(0).clamp(0, GUTTER_EXTRA_MAX);
        if ge != self.gutter_extra.get() {
            self.gutter_extra.set(ge);
            self.set_gutter_w(self.gutter_digits.get());
            self.apply_gutter();
        }
        let sw = sidepanel::clamp_width(cfg.side_w.unwrap_or(sidepanel::WIDTH));
        if sw != self.side_w.get() {
            self.set_side_width(sw, false);
        }

        // 文字サイズ
        let want = cfg.size.unwrap_or(zoom::DEFAULT);
        if want != self.size.get() {
            self.apply_size(want);
        }

        // フォント（変わったときだけ読み込む）
        if cfg.font != *self.font_path.borrow() {
            match &cfg.font {
                Some(f) => match self.app.load_font(f) {
                    Ok(name) if Font::by_name(&name).bits() == LOADED_FONT_SLOT => {
                        self.apply_font(Font::by_name(&name));
                        *self.font_path.borrow_mut() = cfg.font.clone();
                    }
                    other => {
                        let why = match other {
                            Ok(name) => if crate::lang::is_en() { format!("could not use \"{name}\"") } else { format!("「{name}」を使えませんでした") },
                            Err(e) => e.to_string(),
                        };
                        log::warn(TAG, &format!("config: font を読み込めませんでした: {} ({why})", f.display()));
                        confirm::alert(&if crate::lang::is_en() { format!("Could not load the font in config.txt. Keeping the current font.\n\n{}\n{why}", f.display()) } else { format!("config.txt の font を読み込めませんでした。今のフォントのままにします。\n\n{}\n{why}", f.display()) });
                    }
                },
                None => self.reset_font(),
            }
        }
        log::info(TAG, "config: applied");
        self.flash(crate::lang::tr("設定を適用しました", "Config applied"));
    }

    /// 最近開いたファイル（保存した一覧）を表示し、選んだものをタブで開く。
    fn recent_menu(self: &Rc<Self>) {
        let list = self.recent.borrow().clone();
        if list.is_empty() {
            self.flash(crate::lang::tr("最近使ったファイルはありません", "No recent files"));
            return;
        }
        let labels: Vec<String> = list.iter().enumerate().map(|(i, p)| recent::label(i, p)).collect();
        let rows: Vec<menu::Row> = labels.iter().map(|l| menu::Row::plain(l)).collect();
        if let Some(i) = menu::pick(&rows, app::event_x(), app::event_y()) {
            self.open_paths(vec![list[i].clone()]);
        }
    }

    /// Choose Theme: 配色の一覧（今の配色に印）から選ぶ。最後の行は自分の配色（custom）を設定ファイルで編集する。
    /// custom の色がまだ無ければ、今の配色を写して作る（そこから色を変えればよい）。
    fn choose_theme(self: &Rc<Self>) {
        let cur = theme::current_name();
        let mut names: Vec<&str> = theme::PALETTES.iter().map(|p| p.name).collect();
        names.push(theme::CUSTOM_NAME);
        let mut rows: Vec<menu::Row> = names.iter().map(|n| menu::Row { check: Some(*n == cur), ..menu::Row::plain(n) }).collect();
        if let Some(last) = rows.last_mut() {
            last.divider = true;
        }
        rows.push(menu::Row::plain(crate::lang::tr("カスタム配色を編集...", "Edit custom colors...")));
        let Some(i) = menu::pick(&rows, app::event_x(), app::event_y()) else { return };
        if theme::custom().is_none() && i >= names.len() - 1 {
            theme::store_custom(theme::palette());
        }
        if i < names.len() {
            theme::set_named(names[i]);
            self.apply_theme();
            self.persist();
            self.flash(&if crate::lang::is_en() { format!("Theme: {}", names[i]) } else { format!("配色: {}", names[i]) });
        } else {
            self.persist(); // custom_* を書いてから開く
            self.open_config();
            self.flash("custom_* を書き換えて保存すると反映されます（theme=custom で使う）");
        }
    }

    /// 折り返しを切り替えて、設定ファイルに残す。
    fn set_wrap(&self, on: bool) {
        self.set_wrap_quiet(on);
        self.persist();
        self.flash(if on { crate::lang::tr("折り返し: オン", "Word Wrap: ON") } else { crate::lang::tr("折り返し: オフ", "Word Wrap: OFF") });
    }

    /// 折り返しを切り替えるだけ（設定ファイルは触らない。設定の読み直しからも使う）。
    fn set_wrap_quiet(&self, on: bool) {
        self.wrap.set(on);
        menu::set_wrap_checked(on);
        self.apply_wrap_for(self.editor.buffer().map_or(0, |b| b.length()));
        let mut ed = self.editor.clone();
        ed.show_insert_position();
        ed.redraw();
    }

    /// 編集領域の折り返しを、設定と文書の大きさから決める。大きい文書（WRAP_LIMIT 超）は設定がオンでも折り返さない。
    fn apply_wrap_for(&self, len: i32) {
        let want = self.wrap.get() && len <= WRAP_LIMIT;
        self.editor.clone().wrap_mode(if want { fltk::text::WrapMode::AtBounds } else { fltk::text::WrapMode::None }, 0);
    }

    /// 文字サイズを `n` にして（範囲に収める）、設定ファイルに残す。
    fn set_size(&self, n: i32) {
        let n = zoom::clamp(n);
        if n == self.size.get() {
            self.flash(&if crate::lang::is_en() { format!("Size: {n}") } else { format!("文字サイズ: {n}") });
            return;
        }
        self.apply_size(n);
        self.persist();
        self.flash(&if crate::lang::is_en() { format!("Size: {n}") } else { format!("文字サイズ: {n}") });
    }

    /// 編集領域・行番号・ハイライトの文字サイズを変える（設定ファイルは触らない）。
    fn apply_size(&self, n: i32) {
        self.size.set(n);
        let mut ed = self.editor.clone();
        ed.set_text_size(n);
        ed.set_linenumber_size(zoom::linenumber_size(n));
        self.refresh_gutter_size_only();
        self.apply_gutter();
        if self.styled.get() {
            let sb = self.ensure_sbuf(&self.doc());
            ed.set_highlight_data_ext(sb, Self::style_table(self.font.get(), n));
        }
        ed.show_insert_position();
        ed.redraw();
    }

    // ---- 行番号欄（カーソルが左端のときだけ出す。「固定」で常に出す）----

    fn gutter_width(&self) -> i32 {
        if self.gutter_shown.get() { self.gutter_w.get() } else { 0 }
    }

    /// 行番号欄の幅を、行数の桁数（2 桁以上）と行番号の文字の幅に合わせて決め直す。変わったら true。
    /// 行数は文書か文字数が変わったときだけ数える（大きな文書は編集のたびには数えない）。
    fn refresh_gutter_width(&self) -> bool {
        let d = self.doc();
        let len = d.buf.length();
        let key = (d.id, len);
        let (old_id, old_len) = self.gutter_key.get();
        if key == (old_id, old_len) {
            return false; // 変わっていない
        }
        if old_id == d.id && len > LIVE_COUNT_LIMIT {
            return false; // 大きな文書は、編集のたびには数えない
        }
        self.gutter_key.set(key);
        let lines = d.buf.count_lines(0, len) + 1;
        let digits = lines.to_string().len().max(2) as i32;
        self.set_gutter_w(digits)
    }

    /// 大きな文書で数え直さないとき: 前回の桁数のまま、文字サイズの変化だけ反映する。
    fn refresh_gutter_size_only(&self) -> bool {
        let digits = self.gutter_digits.get();
        self.set_gutter_w(digits)
    }

    fn set_gutter_w(&self, digits: i32) -> bool {
        self.gutter_digits.set(digits);
        fltk::draw::set_font(self.font.get(), zoom::linenumber_size(self.size.get()));
        let base = (fltk::draw::width("0") * digits as f64).ceil() as i32 + 10;
        self.gutter_base.set(base);
        let w = base + self.gutter_extra.get();
        let changed = w != self.gutter_w.get();
        self.gutter_w.set(w);
        self.fit_gutter_pin();
        changed
    }

    /// タブ行の左端の押しピンを、行番号欄と同じ幅にする。
    /// ツールバーの押しピンとタブ行の `#` を、行番号欄と同じ幅にする（左端の列をそろえる）。
    fn fit_gutter_pin(&self) {
        let w = self.gutter_w.get();
        if self.pin.w() == w && self.top_pin.w() == w {
            return;
        }
        let mut row = self.tabrow.clone();
        row.fixed(&self.pin, w);
        row.layout();
        let mut bar = self.bar.clone();
        bar.fixed(&self.top_pin, w);
        bar.layout();
        self.refresh_tabs();
    }

    /// ウィンドウの最前面への固定を切り替える。
    fn toggle_on_top(&self) {
        let on = !self.top_pin.value();
        let ok = platform::set_topmost(self.win.raw_handle() as *mut std::ffi::c_void, on);
        if !ok {
            self.flash(crate::lang::tr("この環境では最前面に固定できません", "Cannot keep the window on top in this environment"));
            return;
        }
        let mut b = self.top_pin.clone();
        b.set_value(on);
        b.set_tooltip(if on { crate::lang::tr("最前面に固定中（クリックで解除）", "Pinned on top (click to unpin)") } else { crate::lang::tr("ウィンドウを最前面に固定する", "Keep window on top") });
        b.redraw();
        self.flash(if on { crate::lang::tr("最前面: オン", "Always on top: on") } else { crate::lang::tr("最前面: オフ", "Always on top: off") });
    }

    /// 押しピンの右の境目のドラッグ: 行番号欄の幅を、桁数で決まる幅から GUTTER_EXTRA_MAX まで広げる。
    fn drag_gutter_width(&self, w: i32, done: bool) {
        let extra = (w - self.gutter_base.get()).clamp(0, GUTTER_EXTRA_MAX);
        if extra != self.gutter_extra.get() {
            self.gutter_extra.set(extra);
            self.set_gutter_w(self.gutter_digits.get());
            if self.gutter_shown.get() {
                self.apply_gutter();
            }
        }
        if done {
            self.persist();
        }
    }

    /// 今の `gutter_shown` を画面に反映する: 行番号欄の幅（隠れている間は 0 で、本文が左へ詰まる）。
    fn apply_gutter(&self) {
        let mut ed = self.editor.clone();
        ed.set_linenumber_width(self.gutter_width());
        self.style_pin();
        ed.redraw();
    }

    /// 固定ボタンの見た目: 固定中はアクセント色。
    fn style_pin(&self) {
        let mut pin = self.pin.clone();
        let pinned = self.gutter_pinned.get();
        // 固定中は文字を text にして、アクセント色の下線を引く（下線は toolbar.rs が value を見て描く）
        pin.set_color(rgb(color_main()));
        pin.set_selection_color(rgb(color_main()));
        pin.set_label_color(if pinned { rgb(color_text()) } else { rgb(color_subtext()) });
        pin.set_value(pinned);
        pin.set_tooltip(if pinned { crate::lang::tr("行番号: 常に表示中（クリックで自動表示に戻す。右の境目のドラッグで幅を変える）", "Line numbers: always shown (click for auto; drag the right edge to resize)") } else { crate::lang::tr("行番号: 自動表示（クリックで常に表示する。右の境目のドラッグで幅を変える）", "Line numbers: auto (click to always show; drag the right edge to resize)") });
        pin.redraw();
    }

    /// 出すか隠すかを決め直す。出す条件は 固定 / マウスが左端 / Alt で開いている のどれか（文字カーソルでは出さない。改行のたびにちらつくため）。
    fn update_gutter(&self) {
        let want = self.gutter_pinned.get() || self.mouse_left.get() || self.alt_open.get();
        let resized = self.refresh_gutter_width() && self.gutter_shown.get();
        if resized && want == self.gutter_shown.get() {
            self.apply_gutter(); // 行数の桁が増減した
        }
        if want != self.gutter_shown.get() {
            self.gutter_shown.set(want);
            self.apply_gutter();
        }
    }

    fn toggle_gutter_pin(&self) {
        self.gutter_pinned.set(!self.gutter_pinned.get());
        self.style_pin();
        self.update_gutter();
        self.persist();
        self.flash(if self.gutter_pinned.get() { crate::lang::tr("行番号: 常に表示", "Line numbers: pinned") } else { crate::lang::tr("行番号: 自動", "Line numbers: auto") });
    }

    /// 横スクロールバーは、はみ出す行があるときだけ出す。
    /// FLTK の Text_Display は「縦スクロールバーが出ていれば、横も必ず出す」ので、短い行だけでも全幅のつまみが出てしまう。
    /// そこでスクロールバーの出し方（align）を自分で決める: はみ出すときだけ「下 + 右」、それ以外は「右」だけ。
    /// 折り返し中は横に出ない。大きな文書（HSCROLL_SCAN_LIMIT 超え）は測らず、FLTK 任せ（従来どおり）にする。
    /// 条件が変わったときだけ測り直す（0.1 秒のタイマーから呼ぶ）。
    fn update_hscroll(&self) {
        let d = self.doc();
        let len = d.buf.length();
        let ed = self.editor.clone();
        let key = (d.id, len, ed.w(), self.size.get(), self.font.get().bits(), self.wrap.get(), self.gutter_width());
        if key == self.hscroll_key.get() || !ed.visible() {
            return;
        }
        self.hscroll_key.set(key);
        let both = Align::Bottom | Align::Right;
        let want = if self.wrap.get() {
            false
        } else if len > HSCROLL_SCAN_LIMIT {
            true
        } else {
            // 縦スクロールバーは出ている前提で、本文の幅を少なめに見積もる（出すほうに倒す）
            let sb = if ed.scrollbar_size() > 0 { ed.scrollbar_size() } else { app::scrollbar_size() };
            let avail = ed.w() - 6 - self.gutter_width() - sb;
            let text = d.buf.text();
            longest_line_px(&text, self.font.get(), self.size.get()) + 2 > avail
        };
        let align = if want { both } else { Align::Right };
        if ed.scrollbar_align() != align {
            let mut ed = ed;
            ed.set_scrollbar_align(align);
            ed.resize(ed.x(), ed.y(), ed.w(), ed.h()); // 設定しただけでは並べ直されないので、再計算させる
            ed.redraw();
        }
    }

    /// マウスが本文の左端・右端にあるかを 0.1 秒ごとに見る（ボタンの上に乗っても離れたことにならないよう、座標で判定する）。
    /// 隠れているときは端から幅の 5%（最低 GUTTER_EDGE）、出ているときは欄の幅まで（右は縦スクロールバーの分も含めて、出たり消えたりを防ぐ）。
    fn poll_mouse(self: &Rc<Self>) {
        let u = Rc::clone(self);
        app::add_timeout3(0.1, move |_| {
            let (left, right) = {
                let (mx, my) = app::get_mouse();
                let b = &u.body;
                let (x0, y0) = (u.win.x_root() + b.x(), u.win.y_root() + b.y());
                let in_y = u.win.visible() && my >= y0 && my < y0 + b.h();
                let edge = ((b.w() as f64 * EDGE_RATIO) as i32).max(GUTTER_EDGE);
                let left_zone = if u.gutter_shown.get() { u.gutter_w.get().max(edge) } else { edge };
                let right_zone = if u.side.widget.visible() { u.side_w.get() + edge.max(app::scrollbar_size() + 4) } else { edge };
                (in_y && mx >= x0 && mx < x0 + left_zone, in_y && mx < x0 + b.w() && mx >= x0 + b.w() - right_zone)
            };
            // 出すのは、端にしばらく留まってから（端の文字を選ぼうとして欄が出るのを防ぐ）。消すのはすぐ
            let (dl, dr) = u.edge_dwell.get();
            let (dl, dr) = (if left { dl + 1 } else { 0 }, if right { dr + 1 } else { 0 });
            u.edge_dwell.set((dl, dr));
            let left = left && (u.mouse_left.get() || dl >= EDGE_DWELL);
            let right = right && (u.mouse_right.get() || dr >= EDGE_DWELL);
            if left != u.mouse_left.get() {
                u.mouse_left.set(left);
                u.update_gutter();
            }
            if right != u.mouse_right.get() {
                u.mouse_right.set(right);
                u.update_side();
            }
            u.update_side_view();
            u.update_hscroll();
            u.update_peek();
            u.poll_mouse();
        });
    }

    // ---- 本文の値の小窓（色・文字参照・UNIX 時刻・画像のパスにマウスが 0.3 秒止まったら出す）----

    fn hide_peek(&self) {
        self.peek_at.set(None);
        peekwin::hide();
    }

    /// 0.1 秒ごとの見回りから呼ぶ。マウスが動いている間は何もしない（止まって 3 回目で見分ける）。
    fn update_peek(&self) {
        let (mx, my) = app::get_mouse();
        let (px, py, n) = self.peek_still.get();
        let n = if (px, py) == (mx, my) { n.saturating_add(1) } else { 0 };
        self.peek_still.set((mx, my, n));
        let ed = &self.editor;
        let d = self.doc();
        let usable = self.win.visible() && ed.visible() && !d.preview.get() && !d.st.borrow().hex && !d.st.borrow().loading;
        let (ex, ey) = (mx - self.win.x_root(), my - self.win.y_root());
        let inside = ex >= ed.x() + self.gutter_width() && ex < ed.x() + ed.w() - app::scrollbar_size() && ey >= ed.y() && ey < ed.y() + ed.h();
        if !usable || !inside {
            if peekwin::is_shown() {
                self.hide_peek();
            }
            return;
        }
        let pos = ed.xy_to_position(ex, ey, PositionType::Character);
        // 出している値の上にいる間はそのまま。外れたら消す
        if let Some((id, s, e)) = self.peek_at.get() {
            if id == d.id && pos >= s && pos < e {
                return;
            }
            self.hide_peek();
        }
        if n != 3 {
            return;
        }
        let buf = &d.buf;
        let (ls, le) = (buf.line_start(pos), buf.line_end(pos));
        if pos >= le {
            return; // 行の右の空き（xy_to_position は行末を返す）
        }
        let Some(line) = buf.text_range(ls, le) else { return };
        let path = d.st.borrow().path.clone();
        let names = path.as_deref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| {
            matches!(e.as_str(), "css" | "scss" | "less" | "html" | "htm" | "xhtml" | "svg" | "xml" | "json" | "jsonc")
        });
        let Some((r, p)) = peek::at(&line, (pos - ls) as usize, names) else { return };
        let content = match p {
            peek::Peek::Color(c, label) => peekwin::Content::Swatch(c, label),
            peek::Peek::Char(c, cp) => peekwin::Content::Text { big: c, note: cp },
            peek::Peek::Time(secs, ms) => {
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
                peekwin::Content::Text { big: platform::local_datetime(t), note: if ms { crate::lang::tr("UNIX 時刻（ミリ秒）", "UNIX time (ms)").into() } else { crate::lang::tr("UNIX 時刻（秒）", "UNIX time (s)").into() } }
            }
            peek::Peek::Image(s) => {
                let base = path.as_deref().and_then(|p| p.parent());
                match imagefile::resolve_local(base, &s) {
                    Some(full) => {
                        let note = format!("{}", log::file_name(&full.to_string_lossy()));
                        peekwin::Content::Image(full, note)
                    }
                    None => return, // 見つからない画像は出さない
                }
            }
        };
        self.peek_at.set(Some((d.id, ls + r.start as i32, ls + r.end as i32)));
        peekwin::show(mx, my, content);
    }

    // ---- 右端の欄（マウスが右端・固定・Alt のどれかで出す）----

    fn update_side(&self) {
        let want = self.side_pinned.get() || self.mouse_right.get() || self.alt_open.get();
        let mut w = self.side.widget.clone();
        if want == w.visible() {
            return;
        }
        if want {
            w.show();
            self.side_key.set((0, -1, false, 0)); // 隠れている間は作り直していないので、必ず作る
        } else {
            w.hide();
        }
        self.body.clone().layout();
        self.refresh_side();
        self.update_side_view();
        self.win.clone().redraw();
    }

    fn set_side_width(&self, w: i32, save: bool) {
        if w != self.side_w.get() {
            self.side_w.set(w);
            let mut body = self.body.clone();
            body.fixed(&self.side.widget, w);
            body.layout();
            self.update_side_view();
            self.win.clone().redraw();
        }
        if save {
            self.persist();
        }
    }

    fn toggle_side_pin(&self) {
        let on = !self.side_pinned.get();
        self.side_pinned.set(on);
        self.side.state.borrow_mut().pinned = on;
        self.update_side();
        self.side.widget.clone().redraw();
        self.persist();
        self.flash(if on { crate::lang::tr("サイドパネル: 固定", "Side panel: pinned") } else { crate::lang::tr("サイドパネル: 自動", "Side panel: auto") });
    }

    /// Alt だけを押して離したとき: 左右の欄をまとめて開く・閉じる。
    fn toggle_panels(&self) {
        self.alt_open.set(!self.alt_open.get());
        self.update_gutter();
        self.update_side();
    }

    /// 右端の欄の情報と縮小表示を作り直す。隠れている間と、文書・文字数・未保存・選択が変わっていないときは何もしない。
    fn refresh_side(&self) {
        if !self.side.widget.visible() {
            return;
        }
        let d = self.doc();
        let len = d.buf.length();
        let sel = d.buf.selection_position().map_or(0, |(a, b)| (b - a).abs());
        let (dirty, hex, path) = {
            let st = d.st.borrow();
            (st.dirty, st.hex, st.path.clone())
        };
        let key = (d.id, len, dirty, sel);
        let old = self.side_key.get();
        if key == old || (old.0 == d.id && old.2 == dirty && len > LIVE_COUNT_LIMIT) {
            return; // 大きな文書は、編集のたびには数え直さない（保存・切替のときに数える）
        }
        self.side_key.set(key);
        let mut rows: Vec<(String, String)> = Vec::new();
        let name = path.as_deref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        rows.push((crate::lang::tr("名前", "Name").into(), name.unwrap_or_else(|| crate::lang::tr("未保存のファイル", "Unsaved File").into())));
        if let Some(p) = path.as_deref() {
            if let Some(dir) = p.parent() {
                rows.push((crate::lang::tr("フォルダ", "Folder").into(), dir.display().to_string()));
            }
            if let Ok(m) = std::fs::metadata(p) {
                if m.is_file() {
                    rows.push((crate::lang::tr("サイズ", "Size").into(), archive::human_size(m.len()))); // フォルダの合計は下のツリーマップに出す
                }
                if let Ok(t) = m.modified() {
                    rows.push((crate::lang::tr("更新日時", "Modified").into(), platform::local_datetime(t)));
                }
            }
            rows.extend(file_meta_rows(p));
        }
        let text = d.buf.text();
        let is_arch = d.archive.borrow().is_some();
        let audio_rows = match d.audio.borrow().as_ref() {
            Some(Ok(a)) if !hex => Some(audiometa::side_rows(&a.items)),
            _ => None,
        };
        if let Some(r) = audio_rows {
            rows.extend(r); // 音声は本文を読まないので、文字数の代わりにタグ（情報は上の file_meta_rows）
        } else if is_arch {
            // フォルダ・zip の本文は一覧なので、文字数などは出さない（大きさは下のツリーマップ）
        } else if hex {
            rows.push((crate::lang::tr("バイト数", "Bytes").into(), hex::byte_count(&text).to_string()));
        } else {
            rows.push((crate::lang::tr("文字数", "Chars").into(), document::char_count(&text).to_string()));
            rows.push((crate::lang::tr("単語数", "Words").into(), document::word_count(&text).to_string()));
            rows.push((crate::lang::tr("行数", "Lines").into(), (d.buf.count_lines(0, len) + 1).to_string()));
            if sel > 0 {
                let s = d.buf.selection_text();
                rows.push((crate::lang::tr("選択", "Selected").into(), if crate::lang::is_en() { format!("{} chars, {} words", document::char_count(&s), document::word_count(&s)) } else { format!("{} 文字、{} 単語", document::char_count(&s), document::word_count(&s)) }));
            }
        }
        let tree = d.archive.borrow().as_deref().map(treemap::build);
        let (lines, truncated) = if hex || tree.is_some() { (Vec::new(), false) } else { sidepanel::runs_of(&text) };
        {
            let mut st = self.side.state.borrow_mut();
            st.set_tree(tree);
            st.rows = rows;
            st.lines = lines;
            st.truncated = truncated;
            st.font = Some(self.font.get());
        }
        self.side.widget.clone().redraw();
    }

    /// 縮小表示の「見えている範囲」を、本文のスクロールに合わせる（0.1 秒のタイマーから呼ぶ。変わったときだけ描き直す）。
    fn update_side_view(&self) {
        // 窓を出す前は文字の高さがまだ 0 で、xy_to_position が FLTK の中で 0 除算になる
        if !self.side.widget.visible() || !self.editor.visible() || !self.win.shown() {
            return;
        }
        let ed = &self.editor;
        let d = self.doc();
        let x = ed.x() + self.gutter_width() + 4;
        let top_pos = ed.xy_to_position(x, ed.y() + 2, PositionType::Cursor);
        let bottom_pos = ed.xy_to_position(x, ed.y() + ed.h() - app::scrollbar_size() - 2, PositionType::Cursor);
        let top = d.buf.count_lines(0, top_pos);
        let visible = d.buf.count_lines(top_pos, bottom_pos) + 1;
        let changed = {
            let mut st = self.side.state.borrow_mut();
            let changed = (st.top, st.visible) != (top, visible);
            st.top = top;
            st.visible = visible;
            changed
        };
        if changed {
            self.side.widget.clone().redraw();
        }
    }

    /// 縮小表示で選んだ行が、本文の真ん中あたりに来るようにスクロールする（文字カーソルは動かさない）。
    fn side_jump(&self, line: i32) {
        let d = self.doc();
        let half = self.side.state.borrow().visible.max(1) / 2;
        let target = (line - half).max(0);
        let mut pos = 0;
        for _ in 0..target {
            let next = d.buf.line_end(pos) + 1;
            if next > d.buf.length() {
                break;
            }
            pos = next;
        }
        let mut ed = self.editor.clone();
        // 折り返し中は表示上の行で数える
        let row = ed.count_lines(0, pos, true);
        ed.scroll(row + 1, 0);
        self.update_side_view();
    }

    /// Ctrl+G: 行（`12`）か 行:桁（`12:5`）を聞いて、そこへカーソルを移す。
    /// 改行コードを LF / CRLF にそろえる。全文の置き換え 1 回なので、元に戻すで戻せる。
    fn convert_eol(&self) {
        let d = self.doc();
        let cur = {
            let st = d.st.borrow();
            if st.loading || st.hex {
                return;
            }
            st.eol
        };
        let to = match confirm::choice(&if crate::lang::is_en() { format!("Convert the line ending? (now: {})", cur.label()) } else { format!("改行コードを変換します（今: {}）", cur.label()) }, crate::lang::tr("キャンセル", "Cancel"), "LF", "CRLF") {
            Some(1) => Eol::Lf,
            Some(2) => Eol::Crlf,
            _ => return,
        };
        // LF と CRLF のどちらも、本文は LF で持つ（CRLF は保存のときに付ける）
        let text = d.buf.text();
        let new_text = eol::convert(&text, Eol::Lf);
        if new_text != text {
            let pos = self.editor.insert_position();
            let mut b = d.buf.clone();
            b.replace(0, b.length(), &new_text);
            let pos = b.utf8_align(pos.min(b.length()));
            self.editor.clone().set_insert_position(pos);
            log::info(TAG, &format!("eol: {} -> {}", cur.label(), to.label()));
        }
        d.st.borrow_mut().eol = to;
        self.flash(&if crate::lang::is_en() { format!("Line ending: {}", to.label()) } else { format!("改行コード: {}", to.label()) });
        self.refresh_title();
    }

    fn goto_line(&self) {
        let d = self.doc();
        let text = d.buf.text();
        let total = text.matches('\n').count() + 1;
        let (cur_line, cur_col) = json::line_col(&text, self.editor.insert_position().max(0) as usize);
        let prompt = if crate::lang::is_en() { format!("Line to go to (or line:column)  now: {cur_line}:{cur_col} / {total} lines") } else { format!("移動する行（または 行:桁）  今: {cur_line}:{cur_col} / 全 {total} 行") };
        let Some(input) = dialog::input_default(&prompt, "") else { return };
        let Some((line, col)) = goto::parse(&input) else {
            self.flash(crate::lang::tr("行へ移動: 12 または 12:5 の形で入力", "Go To Line: enter 12 or 12:5"));
            return;
        };
        let at = goto::offset(&text, line, col) as i32;
        let mut b = d.buf.clone();
        b.unselect();
        let mut ed = self.editor.clone();
        ed.set_insert_position(at);
        ed.show_insert_position();
        self.refresh_title();
        if line > total {
            self.flash(&if crate::lang::is_en() { format!("Moved to the last line ({total})") } else { format!("最終行（{total}）へ移動") });
        }
    }

    fn toggle_autosave(self: &Rc<Self>) {
        let on = !self.autosave.get();
        self.autosave.set(on);
        menu::set_autosave_checked(on);
        log::info(TAG, if on { "auto save: on" } else { "auto save: off" });
        self.flash(if on { crate::lang::tr("自動保存: オン", "Auto Save: ON") } else { crate::lang::tr("自動保存: オフ", "Auto Save: OFF") });
        if on && !self.autosave_armed.get() {
            self.arm_autosave();
        }
        self.persist();
    }

    fn arm_autosave(self: &Rc<Self>) {
        self.autosave_armed.set(true);
        let u = Rc::clone(self);
        app::add_timeout3(AUTOSAVE_SECS, move |_| {
            u.autosave_armed.set(false);
            if u.autosave.get() {
                u.autosave_tick();
                u.arm_autosave();
            }
        });
    }

    /// 保存先があり、未保存の変更があるテキストの文書を、ダイアログなしで上書きする。
    /// 勝手に文字を欠落させないよう、変換できない文字があるものは飛ばす。HEX は保存の検証があるので対象外。
    fn autosave_tick(&self) {
        let docs: Vec<Rc<Doc>> = self.docs.borrow().clone();
        for d in docs {
            let (path, enc, bom, known) = {
                let st = d.st.borrow();
                if !st.dirty || st.loading || st.hex {
                    continue;
                }
                match st.path.clone() {
                    Some(p) => (p, st.encoding, st.bom, st.stamp),
                    None => continue,
                }
            };
            if document::changed_on_disk(&path, known) {
                // 他で書き換えられたファイルは、黙って上書きしない。知らせるのは 1 回だけ（手動の保存で確認する）
                if !std::mem::replace(&mut d.st.borrow_mut().conflict_noted, true) {
                    log::warn(TAG, &format!("auto save skipped: changed on disk: {}", log::file_name(&path.to_string_lossy())));
                    self.flash(crate::lang::tr("自動保存をスキップしました: ディスク上で変更されています", "Auto Save skipped: file changed on disk"));
                }
                continue;
            }
            let encoded = document::encode_for_save(&d.buf.text(), enc, bom);
            if encoded.lossy_count > 0 {
                log::warn(TAG, &format!("auto save skipped: {} で表せない文字があります", enc.label()));
                continue;
            }
            match document::write_atomic(&path, &encoded.bytes) {
                Ok(()) => {
                    log::info(TAG, &format!("auto saved: {}", log::file_name(&path.to_string_lossy())));
                    {
                        let stamp = document::stamp(&path);
                        let mut st = d.st.borrow_mut();
                        st.dirty = false;
                        st.stamp = stamp;
                    }
                    self.refresh_tabs();
                    self.refresh_title_if_active(&d);
                }
                Err(e) => log::warn(TAG, &format!("auto save failed: {e}")),
            }
        }
    }

    // ---- 履歴（過去に読み込んだファイルのパス）----

    /// 存在しないファイルを履歴から消し、変わったら設定ファイルへ書く。
    fn prune_history(&self) {
        let removed = pathlog::prune_missing(&mut self.path_history.borrow_mut());
        if removed > 0 {
            log::info(TAG, &format!("history: removed {removed} missing file(s)"));
            self.persist();
        }
    }

    /// Restore From History: 履歴の新しい方から見て、いま開いていない最初のファイルをタブで開く。
    /// 無いファイルは履歴から消す。
    fn restore_history(self: &Rc<Self>) {
        self.prune_history();
        let target = self.path_history.borrow().iter().find(|p| self.find_by_path(&document::absolutize(p)).is_none()).cloned();
        match target {
            Some(p) => {
                self.flash(&format!("{} {}", crate::lang::tr("復元しました:", "Restored"), p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                self.open_paths(vec![p]);
            }
            None => self.flash(crate::lang::tr("復元する履歴がありません（すべて開いています）", "No history to restore (all are open)")),
        }
    }

    /// パス履歴（新しい順・最大 100 件）を別ウィンドウに一覧する。行をダブルクリック（または Enter）すると、
    /// そのファイルをタブで開く。表示のたびに存在確認して、無いものは履歴から消す。
    fn view_history(self: &Rc<Self>) {
        self.prune_history();
        let list = self.path_history.borrow().clone();
        if list.is_empty() {
            self.flash(crate::lang::tr("履歴はありません（開いたファイルが記録されます）", "No history (opened files are recorded)"));
            return;
        }
        // 別ウィンドウは使わず、押したボタンの真下に一覧のメニューを出す
        let labels: Vec<String> = list.iter().enumerate().map(|(i, p)| pathlog::label(i, p)).collect();
        let rows: Vec<menu::Row> = labels.iter().map(|l| menu::Row { label: l, active: true, divider: false, check: None }).collect();
        let (x, y) = match toolbar::last_button_bottom() {
            Some((bx, by)) => (bx - self.win.x_root(), by - self.win.y_root()),
            None => (app::event_x(), app::event_y()),
        };
        toolbar::mark_open(true);
        let picked = menu::pick(&rows, x, y);
        toolbar::mark_open(false);
        if let Some(p) = picked.and_then(|i| list.get(i).cloned()) {
            self.open_paths(vec![p]);
        }
    }

    /// Undo All / Redo All: これ以上できなくなるまで繰り返す。
    fn history_all(&self, undo: bool) {
        let mut b = self.doc().buf.clone();
        let mut n = 0;
        while n < HISTORY_STEP_LIMIT && (if undo { b.undo().is_ok() } else { b.redo().is_ok() }) {
            n += 1;
        }
        self.flash(&format!("{} {}", if undo { crate::lang::tr("元に戻しました", "Undone") } else { crate::lang::tr("やり直しました", "Redone") }, n));
    }

    /// 全文と、いまの選択範囲（無ければカーソル位置）。位置はバイトオフセット。
    fn text_and_selection(&self) -> (String, usize, usize) {
        let d = self.doc();
        let text = d.buf.text();
        let pos = self.editor.insert_position().max(0) as usize;
        let (s, e) = d.buf.selection_position().map_or((pos, pos), |(a, b)| (a.max(0) as usize, b.max(0) as usize));
        (text, s, e)
    }

    fn set_selection(&self, s: usize, e: usize) {
        self.doc().buf.clone().select(s as i32, e as i32);
        self.editor.clone().set_insert_position(e as i32);
    }

    /// カーソル位置の単語を選ぶ。
    fn select_word(&self) {
        let (text, s, _) = self.text_and_selection();
        if let Some((ws, we)) = selection::word_at(&text, s) {
            self.set_selection(ws, we);
        }
    }

    /// 選択を 単語 → 行 → 段落 → 全体 の順に 1 段広げる。
    fn magic_select(&self) {
        let (text, s, e) = self.text_and_selection();
        let (ns, ne) = selection::expand(&text, s, e);
        if (ns, ne) != (s, e) {
            self.set_selection(ns, ne);
        }
    }

    fn copy_selection(&self) {
        let sel = self.doc().buf.selection_text();
        if sel.is_empty() {
            self.flash(crate::lang::tr("選択がありません", "No selection"));
        } else {
            app::copy(&sel);
            self.flash(crate::lang::tr("選択範囲をコピーしました", "Selection copied"));
        }
    }

    /// カーソルのある行を、改行ごとクリップボードへ（貼り付けると 1 行丸ごと入る）。
    fn copy_line(&self) {
        let d = self.doc();
        let pos = self.editor.insert_position();
        let start = d.buf.line_start(pos);
        let end = (d.buf.line_end(pos) + 1).min(d.buf.length());
        if let Some(line) = d.buf.text_range(start, end) {
            app::copy(&line);
            self.flash(crate::lang::tr("行をコピーしました", "Line copied"));
        }
    }

    /// 今の行（改行まで）を切り取る。元に戻すで戻せる。
    fn cut_line(&self) {
        let d = self.doc();
        if d.st.borrow().hex {
            return;
        }
        let pos = self.editor.insert_position();
        let start = d.buf.line_start(pos);
        let end = (d.buf.line_end(pos) + 1).min(d.buf.length());
        if let Some(line) = d.buf.text_range(start, end) {
            app::copy(&line);
            d.buf.clone().remove(start, end);
            self.editor.clone().set_insert_position(start);
            self.flash(crate::lang::tr("行を切り取りました", "Line cut"));
        }
    }

    /// 選択を置き換えずに、選択の前（または後ろ）へ貼り付ける。
    fn paste_beside(&self, before: bool) {
        let d = self.doc();
        let mut b = d.buf.clone();
        let pos = self.editor.insert_position();
        let (s, e) = b.selection_position().unwrap_or((pos, pos));
        b.unselect();
        let mut ed = self.editor.clone();
        ed.set_insert_position(if before { s } else { e });
        ed.paste();
    }

    // ---- クリップボード履歴 ----

    /// 選択範囲（無ければ今の行）をクリップボードへ入れ、履歴にも貯める（最大 10 件、新しい順）。
    fn copy_to_history(&self) {
        let d = self.doc();
        let text = match d.buf.selection_text() {
            t if !t.is_empty() => t,
            _ => {
                let pos = self.editor.insert_position();
                let end = (d.buf.line_end(pos) + 1).min(d.buf.length());
                d.buf.text_range(d.buf.line_start(pos), end).unwrap_or_default()
            }
        };
        if text.trim().is_empty() {
            self.flash(crate::lang::tr("コピーするものがありません", "Nothing to copy"));
            return;
        }
        app::copy(&text);
        let added = self.clip_history.borrow_mut().push(&text);
        self.flash(if added { crate::lang::tr("履歴にコピーしました", "Copied to history") } else { crate::lang::tr("コピー済み（履歴にあります）", "Copied (already in history)") });
    }

    /// 履歴から選んだテキストを、選択を置き換えて（無ければカーソル位置に）貼り付ける。
    fn paste_from_history(&self) {
        let labels: Vec<String> =
            self.clip_history.borrow().items().iter().enumerate().map(|(i, s)| history::clip_label(i, s)).collect();
        if labels.is_empty() {
            self.flash(crate::lang::tr("クリップボードの履歴はありません", "Clipboard history is empty"));
            return;
        }
        let rows: Vec<menu::Row> = labels.iter().map(|l| menu::Row::plain(l)).collect();
        let Some(i) = menu::pick(&rows, app::event_x(), app::event_y()) else { return };
        let Some(text) = self.clip_history.borrow().items().get(i).map(|s| s.text.clone()) else { return };
        let d = self.doc();
        if d.st.borrow().loading {
            return;
        }
        let mut b = d.buf.clone();
        let pos = self.editor.insert_position();
        let (s, e) = b.selection_position().map_or((pos, pos), |(a, z)| (a.min(z), a.max(z)));
        b.unselect();
        b.replace(s, e, &text);
        self.editor.clone().set_insert_position(s + text.len() as i32);
    }

    // ---- JSON 整形 ----

    /// 選択範囲（無ければ全文）を JSON として整形する。置き換えは 1 回の編集なので「元に戻す」で戻せる。
    /// 不正な JSON は書き換えず、問題の位置へカーソルを移して余白に短く出す。
    fn format_json(&self) {
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.loading || st.hex {
                self.flash(crate::lang::tr("ここでは整形できません", "Cannot format here"));
                return;
            }
        }
        let mut b = d.buf.clone();
        let (s, e) = match b.selection_position() {
            Some((a, z)) if a != z => (a.min(z), a.max(z)),
            _ => (0, b.length()),
        };
        let src = b.text_range(s, e).unwrap_or_default();
        match json::format(&src) {
            Ok(mut out) => {
                if s == 0 && e == b.length() && src.ends_with('\n') {
                    out.push('\n'); // 全文のとき、末尾の改行は保つ
                }
                if out == src {
                    self.flash(crate::lang::tr("整形済みです", "Already formatted"));
                    return;
                }
                b.unselect();
                b.replace(s, e, &out);
                self.editor.clone().set_insert_position(s);
                self.flash(crate::lang::tr("JSON を整形しました", "JSON formatted"));
            }
            Err(err) => {
                let at = s + err.pos as i32;
                let (line, col) = json::line_col(&b.text(), at as usize);
                let mut ed = self.editor.clone();
                ed.set_insert_position(at);
                ed.show_insert_position();
                log::warn(TAG, &format!("JSON error {line}:{col} {}", err.msg));
                self.flash(&if crate::lang::is_en() { format!("JSON error {line}:{col} {}", err.msg) } else { format!("JSON の誤り {line}:{col} {}", err.msg) });
            }
        }
    }

    /// ドロップの受け取り。入力欄の上へ落とされても、パスを文字として入れずにタブで開く。
    fn dnd_event(self: &Rc<Self>, ev: Event) -> bool {
        match ev {
            Event::DndEnter | Event::DndDrag | Event::DndLeave => true,
            Event::DndRelease => {
                self.dnd_dropped.set(true);
                true
            }
            Event::Paste if self.dnd_dropped.get() => {
                self.dnd_dropped.set(false);
                self.open_dropped(&app::event_text());
                true
            }
            _ => false,
        }
    }

    /// ドロップされたファイルを全てタブで開く。
    fn open_dropped(self: &Rc<Self>, text: &str) {
        let paths = dropped::parse_dropped_paths(text);
        for p in &paths {
            log::info("Drag&Drop", &p.to_string_lossy());
        }
        self.open_paths(paths);
    }

    /// 編集領域でだけ受けるキー: Tab / Shift+Tab（まとめてインデント）と Enter（オートインデント）。
    /// HEX・読込中は触らない（HEX の本文にインデントを入れると保存できなくなる）。
    fn editor_key(self: &Rc<Self>, ev: Event) -> bool {
        if ev != Event::KeyDown || app::is_event_command() || app::is_event_ctrl() || app::is_event_alt() {
            return false;
        }
        let key = app::event_key();
        if key != Key::Tab && key != Key::Enter && key != Key::KPEnter {
            return false;
        }
        if key != Key::Tab && self.open_chunk_at_cursor() {
            return true;
        }
        let d = self.doc();
        {
            let st = d.st.borrow();
            if st.hex || st.loading {
                return false;
            }
        }
        let mut buf = d.buf.clone();
        // 全文は取らない。必要な行（Tab は選択にかかる行、Enter は今の行と前の行末）だけ取り出す。
        let caret = self.editor.insert_position().max(0);
        let (s, e) = buf.selection_position().map_or((caret, caret), |(a, b)| (a.min(b).max(0), a.max(b).max(0)));
        if key == Key::Tab {
            let shift = app::is_event_shift();
            // Tab は、複数行を選んでいるときだけまとめて字下げする。それ以外は今までどおりタブ文字を入れる。
            if !shift && !buf.text_range(s, e).map_or(false, |t| t.contains('\n')) {
                return false;
            }
            let (ls, le) = (buf.line_start(s), buf.line_end(e));
            let Some(text) = buf.text_range(ls, le) else { return false };
            let (rs, re) = ((s - ls) as usize, (e - ls) as usize);
            let ed = if shift { indent::unindent(&text, rs, re) } else { indent::indent(&text, rs, re) };
            match ed.map(|ed| ed.shifted(ls as usize)) {
                Some(ed) => {
                    buf.replace(ed.start as i32, ed.end as i32, &ed.text);
                    if ed.sel.0 == ed.sel.1 {
                        buf.unselect();
                    } else {
                        buf.select(ed.sel.0 as i32, ed.sel.1 as i32);
                    }
                    self.editor.clone().set_insert_position(ed.sel.1 as i32);
                }
                None => self.flash(if shift { crate::lang::tr("インデントを解除するものがありません", "Nothing to unindent") } else { crate::lang::tr("インデントするものがありません", "Nothing to indent") }),
            }
            true
        } else {
            // 今の行の頭（の 2 バイト手前 = 前の行の改行）から、行末の次の 1 バイトまで
            let ls = buf.line_start(s);
            let from = buf.utf8_align((ls - 2).max(0));
            let to = (buf.line_end(s) + 1).min(buf.length());
            let Some(text) = buf.text_range(from, to) else { return false };
            let (back, ins) = indent::newline_with_indent(&text, (s - from) as usize, false);
            // CRLF の \r と \n の間にいるときは \r の手前へ入れる（選択があるときは \r を消して ins が作り直す）
            let at = s - back as i32;
            buf.replace(at, if s == e { at } else { e }, &ins);
            self.editor.clone().set_insert_position(at + ins.len() as i32);
            self.editor.clone().show_insert_position();
            true
        }
    }

    fn handle(self: &Rc<Self>, ev: Event) -> bool {
        // Alt だけを押して離したら、左右の欄を開く・閉じる（Alt と一緒にほかのキーやクリックがあれば何もしない）
        let alt_key = matches!(app::event_key(), Key::AltL | Key::AltR);
        match ev {
            // 編集領域が受けなかったキーは、ウィンドウへ Shortcut として回ってくる
            Event::KeyDown | Event::Shortcut if alt_key => self.alt_armed.set(true),
            Event::KeyDown | Event::Shortcut | Event::Push | Event::Unfocus => self.alt_armed.set(false),
            _ => {}
        }
        if ev == Event::KeyUp && alt_key && self.alt_armed.replace(false) {
            self.toggle_panels();
            return true;
        }
        if self.dnd_event(ev) {
            return true;
        }
        match ev {
            Event::KeyDown | Event::Shortcut if app::event_key() == Key::from_i32(Key::F1.bits() + 2) => {
                self.find_move(!app::is_event_shift(), false); // F3 / Shift+F3
                true
            }
            Event::KeyDown | Event::Shortcut if app::event_key() == Key::from_i32(Key::F1.bits() + 10) => {
                self.toggle_fullscreen(); // F11
                true
            }
            Event::MouseWheel if app::is_event_command() => {
                // Ctrl+ホイール: 上へ回すと大きく、下へ回すと小さく
                match app::event_dy() {
                    app::MouseWheel::Up => self.set_size(self.size.get() + 1),
                    app::MouseWheel::Down => self.set_size(self.size.get() - 1),
                    _ => {}
                }
                true
            }
            Event::KeyDown | Event::Shortcut if app::is_event_command() || app::is_event_ctrl() => {
                let key = app::event_key();
                let kp = |c: char| Key::from_i32(Key::KP.bits() + c as i32);
                if app::is_event_command() && (key == Key::from_char('=') || key == Key::from_char('+') || key == kp('+')) {
                    self.set_size(self.size.get() + 1);
                    true
                } else if app::is_event_command() && (key == Key::from_char('-') || key == kp('-')) {
                    self.set_size(self.size.get() - 1);
                    true
                } else if app::is_event_command() && (key == Key::from_char('0') || key == kp('0')) {
                    self.set_size(zoom::DEFAULT);
                    true
                } else if app::is_event_command() && key == Key::from_char('f') {
                    self.focus_find();
                    true
                } else if app::is_event_command() && key == Key::from_char('g') {
                    self.goto_line();
                    true
                } else if app::is_event_command() && key == Key::from_char('h') {
                    self.focus_replace();
                    true
                } else if app::is_event_command() && key == Key::from_char('y') {
                    self.editor.clone().redo();
                    true
                } else if app::is_event_command() && key == Key::from_char('n') {
                    self.perform(Action::NewTab);
                    true
                } else if app::is_event_command() && key == Key::from_char('o') {
                    self.open_dialog();
                    true
                } else if app::is_event_command() && key == Key::from_char('e') {
                    self.encoding_menu(false);
                    true
                } else if app::is_event_command() && key == Key::from_char('s') {
                    self.save(app::is_event_shift());
                    true
                } else if app::is_event_command() && key == Key::from_char('t') {
                    self.perform(Action::NewTab);
                    true
                } else if app::is_event_command() && key == Key::from_char('w') {
                    self.perform(Action::CloseTab);
                    true
                } else if app::is_event_ctrl() && key == Key::Tab {
                    self.perform(if app::is_event_shift() { Action::PrevTab } else { Action::NextTab });
                    true
                } else if app::is_event_command() && app::is_event_shift() && (key == Key::PageUp || key == Key::PageDown) {
                    // Ctrl+Shift+PageUp / PageDown: 表示中のタブを左・右へ動かす
                    self.move_active(if key == Key::PageUp { -1 } else { 1 });
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// 同梱の NeoFont を読み込んで既定のフォントにする。失敗しても起動は止めず、OS 標準のフォントに落とす。
fn load_default_font(app: &app::App) -> Font {
    let loaded = platform::bundled_font_path()
        .map_err(|e| e.to_string())
        .and_then(|p| app.load_font(&p).map_err(|e| e.to_string()));
    match loaded {
        Ok(name) if Font::by_name(&name).bits() == LOADED_FONT_SLOT => Font::by_name(&name),
        other => {
            let why = match other {
                Ok(name) => if crate::lang::is_en() { format!("could not use \"{name}\"") } else { format!("「{name}」を使えませんでした") },
                Err(e) => e,
            };
            log::error(TAG, &format!("同梱フォントを読み込めませんでした（OS 標準のフォントで続行）: {why}"));
            FALLBACK_FONT
        }
    }
}

/// config.txt の `font=` を読み込む。指定が無ければ既定のフォント。読めなければ警告して既定のまま（起動は止めない）。
/// 戻り値は（使うフォント, 設定として覚えるフォントのパス）。
fn load_configured_font(app: &app::App, cfg: &config::Config, default_font: Font) -> (Font, Option<PathBuf>) {
    let Some(path) = cfg.font.clone() else { return (default_font, None) };
    match app.load_font(&path) {
        Ok(name) if Font::by_name(&name).bits() == LOADED_FONT_SLOT => {
            log::info(TAG, &format!("font: {name} (config)"));
            (Font::by_name(&name), Some(path))
        }
        other => {
            let why = match other {
                Ok(name) => if crate::lang::is_en() { format!("could not use \"{name}\"") } else { format!("「{name}」を使えませんでした") },
                Err(e) => e.to_string(),
            };
            log::warn(TAG, &format!("config: font を読み込めませんでした（既定のフォントで続行）: {} ({why})", path.display()));
            // 読み込みに失敗すると枠 16 が空のこともあるので、同梱フォントを入れ直す
            let font = load_default_font(app);
            (font, None)
        }
    }
}

pub fn run(paths: Vec<String>) -> Result<(), FltkError> {
    let app = app::App::default();
    let default_font = load_default_font(&app);
    // 設定ファイル（設定フォルダの config.txt）。無ければ既定で始める。
    let config_path = config::path();
    let cfg = config_path.as_deref().map(config::load).unwrap_or_default();
    apply_theme_config(&cfg); // ウィンドウを作る前に決める
    crate::lang::set(crate::lang::from_code(&cfg.lang).unwrap_or(crate::lang::Lang::Ja));
    if let Ok(name) = std::env::var("NEONOTE_THEME") {
        // 見比べ用の候補のパレット（theme.rs の PALETTES）。保存はしない
        if !theme::set_named(&name) {
            log::warn(TAG, &format!("NEONOTE_THEME: 知らない名前です: {name}"));
        }
    }
    skin::apply_globals();
    let (start_font, start_font_path) = load_configured_font(&app, &cfg, default_font);
    let start_size = cfg.size.unwrap_or(zoom::DEFAULT);
    let (tx, rx) = app::channel::<Msg>();

    let mut win = Window::default().with_size(1000, 800);
    let mut col = Flex::default_fill().column();
    col.set_pad(0); // 既定の 5px の隙間が、ツールバー・タブ・本文の間に空いてしまう
    col.set_margin(0);
    let tx_bar = tx.clone();
    let tx_bar2 = tx.clone();
    // 検索バーはツールバーの右端。入力・ボタンの操作はメインループで処理する
    let tx_find = tx;
    let (bar, gap, top_pin, find_bar) = toolbar::build(
        move |a| tx_bar.send(Msg::Do(a)),
        move |bar| {
            searchbar::build(bar, start_font, FIND_TEXT_SIZE, move |c| {
                tx_find.send(Msg::Find(c));
                app::awake();
            })
        },
    );
    col.fixed(&bar, toolbar::HEIGHT);
    let mut tabrow = Flex::default().row(); // 行番号欄の押しピン + タブバー + 右端のチップ
    tabrow.set_pad(0);
    tabrow.set_margin(0);
    let tx_gw = tx_bar2.clone();
    let pin = tabs::build_gutter_pin(move |w, done| {
        tx_gw.send(Msg::GutterWidth(w, done));
        app::awake();
    });
    tabrow.fixed(&pin, zoom::linenumber_width(start_size));
    let mut tabbar = Flex::default().row();
    tabbar.set_color(rgb(color_sub()));
    tabbar.set_frame(FrameType::FlatBox);
    tabbar.set_pad(0);
    tabbar.set_margin(0);
    tabbar.end();
    let tx_view = tx_bar2.clone();
    let mut view_chip = tabs::build_chip();
    view_chip.set_label("Format");
    view_chip.set_tooltip(crate::lang::tr("クリックで 表示（Text・Preview・HEX）と形式を切り替え", "Click to switch view (Text / Preview / HEX) and format"));
    view_chip.set_callback(move |_| tx_view.send(Msg::Do(Action::ChangeView)));
    tabrow.fixed(&view_chip, tabs::VIEW_CHIP_W);
    let tx_chip = tx_bar2.clone();
    let mut chip = tabs::build_chip();
    chip.set_label("Encode");
    chip.set_callback(move |_| tx_chip.send(Msg::Do(Action::ChangeEncoding)));
    tabrow.fixed(&chip, tabs::CHIP_W);
    tabrow.end();
    col.fixed(&tabrow, tabs::HEIGHT);
    // 本文の行: 編集領域（またはプレビュー）と、右端の情報・縮小表示の欄
    let mut body = Flex::default().row();
    body.set_pad(0);
    body.set_margin(0);
    let mut editor = TextEditor::default();
    editor.set_frame(FrameType::FlatBox); // 既定の枠（2px の縁）は本文を狭めるだけ
    editor.set_buffer(TextBuffer::default()); // 最初の文書が来るまでの仮（activate で差し替わる）
    editor.set_color(rgb(color_main()));
    editor.set_text_color(rgb(color_text()));
    editor.set_cursor_color(rgb(color_accent()));
    editor.set_selection_color(rgb(color_accent()));
    editor.set_text_font(start_font);
    editor.set_text_size(start_size);
    editor.set_linenumber_width(zoom::linenumber_width(start_size));
    editor.set_linenumber_bgcolor(rgb(color_sub()));
    editor.set_linenumber_fgcolor(rgb(color_subtext()));
    editor.set_linenumber_font(start_font);
    editor.set_linenumber_size(zoom::linenumber_size(start_size));
    // Preview 用の表示部品。編集領域と同じ列に置き、使うまでは隠す（隠れている部品は並べ直しで場所を取らない）
    let mut preview = HelpView::default();
    preview.set_frame(FrameType::FlatBox);
    preview.set_color(rgb(color_main()));
    preview.set_text_font(start_font);
    preview.set_text_size(start_size);
    preview.hide();
    let nbt_tree = NbtTree::new();
    skin::style_scrollbars(&editor);
    skin::style_scrollbars(&preview);
    let tx_jump = tx_bar2.clone();
    let tx_side_pin = tx_bar2.clone();
    let tx_side_menu = tx_bar2.clone();
    let tx_side_w = tx_bar2.clone();
    let side = sidepanel::build(
        move |line| {
            tx_jump.send(Msg::SideJump(line));
            app::awake();
        },
        move || {
            tx_side_pin.send(Msg::SidePin);
            app::awake();
        },
        move |w, done| {
            tx_side_w.send(Msg::SideWidth(w, done));
            app::awake();
        },
        move |rel, is_dir| {
            tx_side_menu.send(Msg::SideMenu(rel, is_dir));
            app::awake();
        },
    );
    let side_w = sidepanel::clamp_width(cfg.side_w.unwrap_or(sidepanel::WIDTH));
    body.fixed(&side.widget, side_w);
    side.widget.clone().hide();
    body.end();
    col.end();
    win.end();
    win.resizable(&col);
    win.set_color(rgb(color_main()));
    match PngImage::from_data(ICON_PNG) {
        Ok(icon) => win.set_icon(Some(icon)),
        Err(e) => log::error(TAG, &format!("アイコンを読み込めませんでした: {e}")),
    }

    let ui = Rc::new(Ui {
        app,
        win: win.clone(),
        editor: editor.clone(),
        preview: preview.clone(),
        nbt_tree: nbt_tree.clone(),
        col: col.clone(),
        gap,
        tabbar: RefCell::new(tabbar),
        tabrow: tabrow.clone(),
        chip: chip.clone(),
        view_chip: view_chip.clone(),
        tabs_shown: RefCell::new((Vec::new(), 0)),
        docs: RefCell::new(Vec::new()),
        active: Cell::new(0),
        next_id: Cell::new(1),
        styled: Cell::new(false),
        font: Cell::new(start_font),
        count_pending: Cell::new(false),
        dnd_dropped: Cell::new(false),
        recent: RefCell::new(cfg.recent.clone()),
        path_history: RefCell::new(cfg.history.clone()),
        autosave: Cell::new(cfg.autosave),
        wrap: Cell::new(false),
        size: Cell::new(start_size),
        gutter_pinned: Cell::new(cfg.gutter_pin),
        gutter_shown: Cell::new(true),
        gutter_w: Cell::new(zoom::linenumber_width(start_size)),
        gutter_extra: Cell::new(cfg.gutter_extra.unwrap_or(0).clamp(0, GUTTER_EXTRA_MAX)),
        gutter_base: Cell::new(0),
        gutter_key: Cell::new((0, -1)),
        gutter_digits: Cell::new(2),
        hscroll_key: Cell::new((0, -1, 0, 0, 0, false, 0)),
        mouse_left: Cell::new(false),
        edge_dwell: Cell::new((0, 0)),
        body: body.clone(),
        side,
        side_pinned: Cell::new(cfg.side_pin),
        side_w: Cell::new(side_w),
        resume: Cell::new(cfg.resume),
        unsaved_sig: Cell::new(0),
        peek_at: Cell::new(None),
        peek_still: Cell::new((i32::MIN, i32::MIN, 0)),
        mouse_right: Cell::new(false),
        alt_open: Cell::new(false),
        alt_armed: Cell::new(false),
        side_key: Cell::new((0, -1, false, 0)),
        pin: pin.clone(),
        bar: bar.clone(),
        top_pin: top_pin.clone(),
        audio_timer: Cell::new(false),
        pos_shown: Cell::new(None),
        cursor_pending: Cell::new(false),
        autosave_armed: Cell::new(false),
        clip_history: RefCell::new(History::default()),
        find_bar: find_bar.clone(),
        last_search: RefCell::new(String::new()),
        last_smart: Cell::new(true),
        session_owner: paths.is_empty(),
        config_path,
        font_path: RefCell::new(start_font_path),
        tx,
    });
    // 最初のタブは、表示中の文書を見る処理（右パネルの固定など）より先に作る
    let first = ui.new_doc();
    ui.active.set(first.id);
    if cfg.wrap {
        ui.set_wrap_quiet(true);
    }
    if cfg.autosave {
        menu::set_autosave_checked(true);
        ui.arm_autosave();
    }
    // 固定ボタン: 押すたびに 固定 / 解除 を切り替えて、設定ファイルに残す
    {
        let u = Rc::clone(&ui);
        let mut pin = pin.clone();
        pin.set_callback(move |_| u.toggle_gutter_pin());
        let u = Rc::clone(&ui);
        let mut top = top_pin.clone();
        top.set_callback(move |_| u.toggle_on_top());
    }
    ui.apply_gutter();
    ui.side.state.borrow_mut().pinned = ui.side_pinned.get();
    ui.update_side();
    ui.poll_mouse();

    // 音声の Preview では Space で再生 / 停止（HelpView の既定の処理より先に受ける）
    {
        let u = Rc::clone(&ui);
        let mut pv = preview.clone();
        pv.super_handle_first(false);
        pv.handle(move |_, ev| {
            // クリックでも再生 / 停止（音楽プレイヤーの表示）
            (ev == Event::Push && app::event_mouse_button() == app::MouseButton::Left && u.audio_space())
                || (ev == Event::KeyDown
                    && app::event_key() == Key::from_char(' ')
                    && !app::is_event_command()
                    && !app::is_event_ctrl()
                    && !app::is_event_alt()
                    && u.audio_space())
        });
    }

    // ショートカットとドロップは編集領域・ウィンドウの両方で受ける
    // 右クリックは編集領域のメニュー（選択範囲を保ったまま出す）
    // 既定では FLTK 標準の処理（標準の Cut/Copy/Paste メニュー）が先に走るので、自前を先にする
    editor.super_handle_first(false);
    let u = Rc::clone(&ui);
    editor.handle(move |_, ev| {
        if ev == Event::Push && app::event_mouse_button() == app::MouseButton::Right {
            u.context_menu();
            true
        } else if ev == Event::Released && app::event_clicks() && u.doc().st.borrow().region_list {
            // チャンクの一覧のダブルクリック: 押した位置へカーソルが移った後に開く
            u.open_chunk_at_cursor()
        } else {
            let handled = u.editor_key(ev) || u.handle(ev);
            if matches!(ev, Event::KeyDown | Event::KeyUp | Event::Push | Event::Released | Event::Drag) {
                u.schedule_cursor();
            }
            handled
        }
    });
    // NBT の木: ダブルクリック・Enter で値を書き換え、右クリックでメニュー、F2 で名前、Delete で削除、Insert で子を足す
    {
        let u = Rc::clone(&ui);
        let mut tree = nbt_tree.tree.clone();
        tree.handle(move |t, ev| match ev {
            // 一覧（フォルダ・zip）の木は開閉だけ。編集の操作は NBT の文書の時だけ受ける
            _ if !u.is_nbt_doc(&u.doc()) => false,
            Event::Push if app::event_mouse_button() == app::MouseButton::Right => {
                if let Some(it) = t.find_clicked(true) {
                    let _ = t.select_only(&it, false);
                }
                u.nbt_menu();
                true
            }
            Event::Released if app::event_clicks() && app::event_mouse_button() == app::MouseButton::Left => {
                let leaf = t.first_selected_item().is_some_and(|it| !it.has_children());
                if leaf {
                    u.nbt_edit_value();
                }
                leaf
            }
            Event::KeyDown => match app::event_key() {
                Key::Enter | Key::KPEnter => {
                    u.nbt_edit_value();
                    true
                }
                Key::F2 => {
                    u.nbt_rename();
                    true
                }
                Key::Delete => {
                    u.nbt_delete();
                    true
                }
                Key::Insert => {
                    u.nbt_add();
                    true
                }
                _ => false,
            },
            _ => false,
        });
    }
    // 検索バーの置換の段は編集領域に重ねてあるので、編集領域が描き直されるたびに描き直す
    let sb = find_bar.clone();
    editor.super_draw_first(true);
    editor.draw(move |_| sb.redraw_overlay());
    // 検索バーの入力欄のキー（既定の処理より先に受ける）
    let u = Rc::clone(&ui);
    let mut find_input = find_bar.input.clone();
    find_input.super_handle_first(false);
    find_input.handle(move |_, ev| u.find_key(ev));
    let u = Rc::clone(&ui);
    let mut replace_input = find_bar.replace.clone();
    replace_input.super_handle_first(false);
    replace_input.handle(move |_, ev| u.replace_key(ev));
    let u = Rc::clone(&ui);
    // Ctrl+ホイールは編集領域のハンドラが受ける（窓でも受けると、同じ 1 回が 2 回に数えられる）
    win.handle(move |_, ev| ev != Event::MouseWheel && u.handle(ev));

    // 大きさが変わったら、タブの幅を合わせ直す（ボタンの中ではなくメインループで）
    let tx_resize = ui.tx;
    win.resize_callback(move |_, _, _, _, _| {
        tx_resize.send(Msg::Resized);
        app::awake();
    });

    // 閉じる: 未保存なら 保存 / 破棄 / キャンセル（タブごと）。Esc でも呼ばれるので Close だけ見る。
    let u = Rc::clone(&ui);
    win.set_callback(move |_| {
        if app::event() == Event::Close && u.confirm_quit() {
            u.quit();
        }
    });

    ui.activate(first.id);
    win.show();
    log::info(TAG, &format!("started {}", dev::version()));
    if dev::is_dev_user() {
        log::info("alert", "開発環境のためスキップ");
    }
    menu::set_resume_checked(ui.resume.get());
    if ui.session_owner {
        ui.arm_unsaved();
    }
    if paths.is_empty() {
        ui.restore_session(&cfg);
    } else {
        ui.open_paths(paths.into_iter().map(PathBuf::from).collect());
    }

    while app.wait() {
        while let Some(msg) = rx.recv() {
            match msg {
                Msg::Loaded(id, p, r) => ui.on_loaded(id, p, r),
                Msg::Do(a) => ui.perform(a),
                Msg::SideJump(line) => ui.side_jump(line),
                Msg::SidePin => ui.toggle_side_pin(),
                Msg::SideMenu(rel, is_dir) => ui.side_menu(&rel, is_dir),
                Msg::SideWidth(w, done) => ui.set_side_width(w, done),
                Msg::GutterWidth(w, done) => ui.drag_gutter_width(w, done),
                Msg::Tab(TabCmd::Activate(id)) => ui.activate(id),
                Msg::Tab(TabCmd::Close(id)) => ui.close_tab(id),
                Msg::Tab(TabCmd::New) => ui.new_tab(),
                Msg::Tab(TabCmd::Move(id, to)) => ui.move_tab(id, to),
                Msg::Find(FindCmd::Search) => ui.search_button(),
                Msg::Find(FindCmd::ClearReplace) => {
                    let mut r = ui.find_bar.replace.clone();
                    r.set_value("");
                    let _ = r.take_focus();
                }
                Msg::Find(FindCmd::Changed) => ui.find_changed(),
                Msg::Find(FindCmd::ReplaceOne) => ui.replace_one_bar(),
                Msg::Find(FindCmd::ReplaceAll) => ui.replace_all_bar(),
                Msg::Find(FindCmd::Clear) => ui.clear_find(),
                Msg::Resized => {
                    ui.refresh_tabs();
                    ui.find_bar.place();
                }
                Msg::Ran(r) => ui.on_ran(r),
                Msg::ConfigSaved => ui.apply_config(),
                Msg::PdfText(id, r) => ui.on_pdf_text(id, r),
                Msg::Book(id, r) => ui.on_book(id, r),
                Msg::DocText(id, r) => ui.on_doc_text(id, r),
                Msg::Archive(id, path, r) => ui.on_archive(id, path, r),
                Msg::ImageFetched(r) => ui.on_image_fetched(r),
                Msg::Audio(id, r) => ui.on_audio(id, r),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_path_hides_the_exe_folder() {
        let dir = Path::new("/opt/neonote");
        assert_eq!(title_path(Path::new("/opt/neonote/neodir/config.txt"), Some(dir)), Path::new("neodir/config.txt").display().to_string());
        assert_eq!(title_path(Path::new("/home/a/b.txt"), Some(dir)), "/home/a/b.txt");
        assert_eq!(title_path(dir, Some(dir)), "/opt/neonote");
        assert_eq!(title_path(Path::new("/home/a/b.txt"), None), "/home/a/b.txt");
    }
}

/// 形式が Auto のとき、Preview で見る種類（文字として編集できる物は Text のまま）。
fn auto_previews(k: viewmode::Kind) -> bool {
    matches!(k, viewmode::Kind::Book | viewmode::Kind::Doc | viewmode::Kind::Pdf | viewmode::Kind::Image | viewmode::Kind::Audio | viewmode::Kind::Archive)
}
