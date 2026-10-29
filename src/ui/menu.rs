// メニューの表（MenuEntry）。新機能は表に 1 行足すだけで、ボタンと右クリックの両方に出せる。
// ツールバーのサブメニューの表記はなるべく 1 語にし、何をするかはツールチップ（describe）で伝える。
// `action: None` の項目は「見た目だけ」で、グレーアウトして押せない。
// ラベルに `/` と `&` は使わない（FLTK のメニューが階層・ショートカットとして解釈する）。
use crate::ui::theme::{color_main, color_text, mix};
use fltk::{
    app,
    enums::{Color, Font, FrameType, Shortcut},
    group::Group,
    menu::{MenuButton, MenuFlag},
    prelude::*,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Enable Auto Save の状態（メニューのチェック表示用）。App が切り替えるたびに更新する。
static AUTOSAVE_ON: AtomicBool = AtomicBool::new(false);
/// Word Wrap の状態（同上）。
static WRAP_ON: AtomicBool = AtomicBool::new(false);
/// 続きから（前回のタブと未保存の変更を戻す）の状態（同上）。
static RESUME_ON: AtomicBool = AtomicBool::new(true);

pub fn set_resume_checked(on: bool) {
    RESUME_ON.store(on, Ordering::Relaxed);
}

pub fn set_wrap_checked(on: bool) {
    WRAP_ON.store(on, Ordering::Relaxed);
}

pub fn set_autosave_checked(on: bool) {
    AUTOSAVE_ON.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open,
    OpenFolder,
    Save,
    SaveAs,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Replace,
    Search,
    SearchWeb,
    OpenImageUrl,
    RunCmd,
    ChangeEncoding,
    ChangeView,
    CopyPath,
    CopyFileName,
    CutLine,
    ChangeSize,
    ToggleFullScreen,
    CountText,
    ChooseTheme,
    ChooseFont,
    ResetFont,
    OpenReadme,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    OpenHomePage,
    CheckUpdate,
    MakeShortcut,
    OpenRecent,
    OpenConfig,
    ViewHistory,
    RestoreHistory,
    CopyToHistory,
    PasteFromHistory,
    FormatJson,
    ToggleAutoSave,
    ToggleWrap,
    GotoLine,
    ConvertEol,
    ToggleTheme,
    UndoAll,
    RedoAll,
    SelectWord,
    MagicSelect,
    Deselect,
    CopySelection,
    CopyLine,
    PasteBefore,
    PasteAfter,
    ToggleResume,
}

pub struct MenuEntry {
    pub label: &'static str,
    pub action: Option<Action>,
    /// この項目の下に区切り線を引く。
    pub divider: bool,
}

const fn e(label: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, action: Some(action), divider: false }
}
const fn ed(label: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, action: Some(action), divider: true }
}

/// 右クリックメニュー（編集領域）
pub const CONTEXT: &[MenuEntry] = &[
    e("Cut", Action::Cut),
    e("Copy", Action::Copy),
    e("Paste", Action::Paste),
    ed("Replace", Action::Replace),
    e("Search", Action::Search),
    e("Search In web", Action::SearchWeb),
    ed("Open Image URL", Action::OpenImageUrl),
    ed("Select All", Action::SelectAll),
    e("Change View", Action::ChangeView),
    e("Change Encoding", Action::ChangeEncoding),
    ed("Format JSON", Action::FormatJson),
    e("Run In CMD", Action::RunCmd),
    ed("Word Wrap", Action::ToggleWrap),
    e("Go To Line", Action::GotoLine),
    e("Convert Line Ending", Action::ConvertEol),
];

pub const LOAD: &[MenuEntry] = &[
    e("New", Action::NewTab),
    e("Open", Action::Open),
    e("Folder", Action::OpenFolder),
    ed("Close", Action::CloseTab),
    e("Recent", Action::OpenRecent),
    e("Readme", Action::OpenReadme), // 中身は assets/README.txt
    e("Homepage", Action::OpenHomePage),
    e("Font", Action::ChooseFont),
    e("Config", Action::OpenConfig),
    e("Resume", Action::ToggleResume),   // config.txt をタブで開く
    e("Shortcut", Action::MakeShortcut),
    ed("Update", Action::CheckUpdate),
    e("Exit", Action::Exit),
];
pub const SAVE: &[MenuEntry] = &[
    ed("Save", Action::Save),
    e("Save As", Action::SaveAs),
    e("Autosave", Action::ToggleAutoSave),
];
pub const UNDO: &[MenuEntry] = &[
    ed("Undo", Action::Undo),
    e("Last", Action::Undo), // Undo と同じ 1 回分（Java 版にある名前なので残している）
    e("All", Action::UndoAll),
    e("History", Action::ViewHistory),
];
pub const REDO: &[MenuEntry] = &[
    ed("Redo", Action::Redo),
    e("Last", Action::Redo), // Redo と同じ 1 回分
    e("All", Action::RedoAll),
    e("Restore", Action::RestoreHistory),
];
pub const EDIT: &[MenuEntry] = &[
    ed("Word", Action::SelectWord),
    e("All", Action::SelectAll),
    e("Magic", Action::MagicSelect),
    e("Deselect", Action::Deselect),
    e("Copy", Action::CopySelection),
];
pub const COPY: &[MenuEntry] = &[
    ed("Copy", Action::Copy),
    e("Line", Action::CopyLine),
    e("Cut", Action::CutLine),
    e("Name", Action::CopyFileName),
    e("Stash", Action::CopyToHistory),
];
pub const PASTE: &[MenuEntry] = &[
    ed("Paste", Action::Paste),
    e("Before", Action::PasteBefore),
    e("After", Action::PasteAfter),
    e("History", Action::PasteFromHistory),
];
pub const RUN: &[MenuEntry] = &[
    ed("Run", Action::RunCmd),
    e("JSON", Action::FormatJson),
    e("View", Action::ChangeView),
    e("Encoding", Action::ChangeEncoding),
    e("Font", Action::ChooseFont),
    e("Reset Font", Action::ResetFont),
    e("Size", Action::ChangeSize),
    e("Fullscreen", Action::ToggleFullScreen),
    e("Count", Action::CountText),
    e("Mode", Action::ToggleTheme),
    e("Theme", Action::ChooseTheme),
];

/// ポップアップの 1 行分。ラベルはそのまま表示される（`/` `&` などは中で退避するので、パスを含んでいてもよい）。
pub struct Row<'a> {
    pub label: &'a str,
    /// false ならグレーアウトして選べない。
    pub active: bool,
    /// この行の下に区切り線を引く。
    pub divider: bool,
    /// Some なら、チェック付きの行（中身は今の状態）。
    pub check: Option<bool>,
}

impl<'a> Row<'a> {
    pub fn plain(label: &'a str) -> Row<'a> {
        Row { label, active: true, divider: false, check: None }
    }
}

/// FLTK のメニュー記法（`& / \\ _`）を、文字としてそのまま出すために退避する。
fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '&' | '/' | '\\' | '_') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// アプリの配色（main / text / accent）をまとめたメニューを `(x, y)`（ウィンドウ内の座標）の真下に出し、
/// 選ばれた行の番号を返す（選ばずに閉じたら None）。
/// `MenuItem::popup` は部品の色を持てず FLTK 標準の明るい灰色で出るので、色を持てる `MenuButton` を一時的に作って使う。
pub fn pick(rows: &[Row], x: i32, y: i32) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    Group::set_current(None::<&Group>); // どのグループにも入れない（ウィンドウの子にしない）
    let rgb = |(r, g, b): (u8, u8, u8)| Color::from_rgb(r, g, b);
    let mut mb = MenuButton::new(x, y, 1, 1, None);
    mb.set_frame(FrameType::FlatBox); // 枠が無いと、メニューは「ボタンの下」ではなくマウス位置に出てしまう
    mb.set_down_frame(FrameType::FlatBox); // メニュー本体と選択行の枠（BorderBox だと選択行に黒い枠が付く）
    // 編集領域と同じ色だと境目が分からないので、背景を text 側へ少し寄せる
    let (main, text) = (color_main(), color_text());
    let bg = mix(main, text, 8);
    mb.set_color(rgb(bg));
    mb.set_text_color(rgb(text));
    // 選択行は、アクセント色だと FLTK が文字を黒く選んで読みにくいので、History と同じ控えめなグレー系にする
    mb.set_selection_color(rgb(mix(bg, text, 22)));
    for r in rows {
        let mut flags = MenuFlag::Normal;
        if !r.active {
            flags |= MenuFlag::Inactive;
        }
        if r.divider && std::env::var_os("NN_NODIV").is_none() {
            flags |= MenuFlag::MenuDivider;
        }
        if let Some(on) = r.check {
            flags |= MenuFlag::Toggle;
            if on {
                flags |= MenuFlag::Value;
            }
        }
        mb.add(&escape(r.label), Shortcut::None, flags, |_| {});
    }
    // 区切り線は Dark3 + Light3、チェックの四角は Background2 で描かれ、標準では白くて目立つ。出している間だけ、パレットに合う色にする
    let line = mix(bg, text, 30);
    let boxbg = mix(bg, text, 15);
    let swap = [(Color::Dark3, line), (Color::Light3, bg), (Color::Background2, boxbg)];
    let saved = swap.map(|(c, _)| c.to_rgb());
    for (c, (r, g, b)) in swap {
        app::set_color(c, r, g, b);
    }
    let chosen = mb.popup().is_some();
    for ((c, _), (r, g, b)) in swap.into_iter().zip(saved) {
        app::set_color(c, r, g, b);
    }
    let idx = mb.value();
    MenuButton::delete(mb);
    if chosen && idx >= 0 && (idx as usize) < rows.len() { Some(idx as usize) } else { None }
}

/// `pick` で開くメニューのおおよその幅（ピクセル）。ウィンドウの端からはみ出さない位置を決めるのに使う。
pub fn pick_width(rows: &[Row]) -> i32 {
    fltk::draw::set_font(Font::Helvetica, 14); // MenuButton の既定のフォントと大きさ
    let w = rows.iter().map(|r| fltk::draw::width(r.label)).fold(0.0f64, f64::max);
    w.ceil() as i32 + 30 // 左右の余白（チェック欄を含む）
}

/// チェックの付く項目の今の状態（チェックの無い項目は None）。
pub fn checked(a: Action) -> Option<bool> {
    match a {
        Action::ToggleAutoSave => Some(AUTOSAVE_ON.load(Ordering::Relaxed)),
        Action::ToggleWrap => Some(WRAP_ON.load(Ordering::Relaxed)),
        Action::ToggleResume => Some(RESUME_ON.load(Ordering::Relaxed)),
        _ => None,
    }
}

/// メニューの項目のツールチップ（その項目で何が起きるか）。
pub fn describe(a: Action) -> &'static str {
    use Action::*;
    match a {
        Open => "ファイルを選んで開く（Ctrl+O）",
        OpenFolder => "フォルダを選んで、中身の一覧を開く",
        Save => "上書き保存する（Ctrl+S）",
        SaveAs => "名前を付けて保存する",
        Exit => "NeoNote を終了する",
        Undo => "直前の編集を 1 回取り消す（Ctrl+Z）",
        Redo => "取り消した編集を 1 回やり直す（Ctrl+Y）",
        UndoAll => "開いてから今までの編集を全部取り消す",
        RedoAll => "取り消した編集を全部やり直す",
        ViewHistory => "過去に読み込んだファイルの一覧を見る",
        RestoreHistory => "履歴から以前の内容を選んで戻す",
        Cut => "選択範囲を切り取る",
        Copy => "選択範囲をコピーする（Ctrl+C）",
        Paste => "クリップボードの内容を貼り付ける（Ctrl+V）",
        PasteBefore => "今の行の前に貼り付ける",
        PasteAfter => "今の行の後に貼り付ける",
        PasteFromHistory => "コピーした履歴から選んで貼り付ける",
        CopyLine => "今の行をまるごとコピーする",
        CutLine => "今の行をまるごと切り取る",
        CopyFileName => "開いているファイルの名前をコピーする",
        CopyPath => "開いているファイルの場所をコピーする",
        CopyToHistory => "選択範囲をコピーの履歴に残す",
        CopySelection => "選択範囲をコピーする",
        SelectAll => "全文を選択する（Ctrl+A）",
        SelectWord => "カーソルのある単語を選択する",
        MagicSelect => "押すたびに選択を 単語 → 行 → 段落 → 全体 と広げる",
        Deselect => "選択を外す",
        Replace => "検索欄の文字を置換する",
        Search => "選択範囲の文字で検索する",
        SearchWeb => "選択範囲の文字をブラウザで検索する",
        OpenImageUrl => "選択範囲の画像の URL を開く",
        RunCmd => "選択範囲をコマンドとして実行する。選択が無ければ今のフォルダを開く",
        ChangeView => "表示（Text・Preview・HEX）と Preview の形式を切り替える",
        ChangeEncoding => "文字コード・改行コードを切り替える（Ctrl+E）",
        ChangeSize => "文字の大きさを変える（Ctrl+ホイールでも変えられる）",
        ToggleFullScreen => "全画面表示を切り替える（F11）",
        CountText => "文字数・単語数・行数を数える",
        ChooseTheme => "配色を一覧から選ぶ",
        ToggleTheme => "明るい配色と暗い配色を切り替える",
        ChooseFont => "表示に使うフォントを選ぶ",
        ResetFont => "フォントを同梱の NeoFont に戻す",
        OpenReadme => "使い方の説明（README）を開く",
        NewTab => "新しい空のタブを開く",
        CloseTab => "今のタブを閉じる",
        NextTab => "右隣のタブへ移る",
        PrevTab => "左隣のタブへ移る",
        OpenHomePage => "NeoNote のホームページをブラウザで開く",
        CheckUpdate => "新しい版が出ているか確かめる",
        MakeShortcut => "デスクトップに NeoNote のショートカットを作る",
        OpenRecent => "最近開いたファイルから選んで開く",
        OpenConfig => "設定ファイル（config.txt）をタブで開く。保存すると反映される",
        FormatJson => "選択範囲（無ければ全文）の JSON を整形する",
        ToggleAutoSave => "編集を自動で保存するかを切り替える",
        ToggleWrap => "右端で折り返すかを切り替える",
        GotoLine => "行番号を入れて、その行へ移る",
        ConvertEol => "改行コード（CRLF・LF）を変換する",
        ToggleResume => "次の起動で、前回のタブと未保存の変更を戻す（切ると、終了時に未保存の変更の保存を確認する）",
    }
}

/// ポップアップを出し、選ばれた Action を返す。グレーの項目は選べない。
pub fn popup(entries: &[MenuEntry], x: i32, y: i32) -> Option<Action> {
    let rows: Vec<Row> = entries
        .iter()
        .map(|m| Row {
            label: m.label,
            active: m.action.is_some(),
            divider: m.divider,
            check: m.action.and_then(checked),
        })
        .collect();
    entries.get(pick(&rows, x, y)?).and_then(|m| m.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_protects_fltk_menu_syntax() {
        assert_eq!(escape("a/b&c_d\\e"), "a\\/b\\&c\\_d\\\\e");
        assert_eq!(escape("plain"), "plain");
    }

    #[test]
    fn labels_are_unique_per_menu_and_safe_for_fltk() {
        for t in [CONTEXT, LOAD, SAVE, UNDO, REDO, EDIT, COPY, PASTE, RUN] {
            let mut seen = std::collections::HashSet::new();
            for m in t {
                assert!(seen.insert(m.label), "duplicate label {}", m.label);
                assert!(!m.label.contains(['/', '&']), "bad char in {}", m.label);
            }
        }
    }
}
