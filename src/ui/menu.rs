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
    ToggleLang,
    SetLang(crate::lang::Lang),
    /// theme::PALETTES の番号の配色にする。
    SetTheme(usize),
    EditCustomTheme,
}

pub struct MenuEntry {
    pub label: &'static str,
    /// 日本語の表記（1 語が基本）。
    pub ja: &'static str,
    pub action: Option<Action>,
    /// この項目の下に区切り線を引く。
    pub divider: bool,
}

impl MenuEntry {
    /// 今の言語での表記。
    pub fn text(&self) -> &'static str {
        crate::lang::tr(self.ja, self.label)
    }
}

const fn e(label: &'static str, ja: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, ja, action: Some(action), divider: false }
}
const fn ed(label: &'static str, ja: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, ja, action: Some(action), divider: true }
}

/// 右クリックメニュー（編集領域）
pub const CONTEXT: &[MenuEntry] = &[
    e("Cut", "切り取り", Action::Cut),
    e("Copy", "コピー", Action::Copy),
    e("Paste", "貼付", Action::Paste),
    ed("Replace", "置換", Action::Replace),
    e("Search", "検索", Action::Search),
    e("Search In web", "Web検索", Action::SearchWeb),
    ed("Open Image URL", "画像URL", Action::OpenImageUrl),
    ed("Select All", "全選択", Action::SelectAll),
    e("Change View", "表示", Action::ChangeView),
    e("Change Encoding", "文字コード", Action::ChangeEncoding),
    ed("Format JSON", "JSON整形", Action::FormatJson),
    e("Run In CMD", "実行", Action::RunCmd),
    ed("Word Wrap", "折り返し", Action::ToggleWrap),
    e("Go To Line", "行へ移動", Action::GotoLine),
    e("Convert Line Ending", "改行コード", Action::ConvertEol),
];

/// 言語のボタンのメニュー（lang::ALL と同じ順）。
pub const LANG: &[MenuEntry] = &[e("日本語", "日本語", Action::SetLang(crate::lang::Lang::Ja)), e("English", "English", Action::SetLang(crate::lang::Lang::En))];

/// 配色のボタンのメニュー（theme::PALETTES と同じ順）。
pub const THEME: &[MenuEntry] = &[
    e("Nureba", "濡羽", Action::SetTheme(0)),
    ed("Geppaku", "月白", Action::SetTheme(1)),
    e("Classic Dark", "旧ダーク", Action::SetTheme(2)),
    e("Classic Light", "旧ライト", Action::SetTheme(3)),
    e("Nord", "ノルド", Action::SetTheme(4)),
    e("Abyss", "アビス", Action::SetTheme(5)),
    e("Paper", "ペーパー", Action::SetTheme(6)),
    e("Sakura", "サクラ", Action::SetTheme(7)),
    e("Discord-like", "Discord 風", Action::SetTheme(8)),
    ed("GitHub-like", "GitHub 風", Action::SetTheme(9)),
    e("Custom", "カスタム", Action::EditCustomTheme),
];

pub const LOAD: &[MenuEntry] = &[
    e("New", "新規", Action::NewTab),
    e("Open", "開く", Action::Open),
    e("Folder", "フォルダ", Action::OpenFolder),
    ed("Close", "閉じる", Action::CloseTab),
    e("Recent", "最近", Action::OpenRecent),
    e("Readme", "説明", Action::OpenReadme), // 中身は assets/README.txt
    e("Homepage", "サイト", Action::OpenHomePage),
    e("Font", "フォント", Action::ChooseFont),
    e("Config", "設定", Action::OpenConfig),
    e("Resume", "続きから", Action::ToggleResume),   // config.txt をタブで開く
    e("Shortcut", "ショートカット", Action::MakeShortcut),
    ed("Update", "更新", Action::CheckUpdate),
    e("Exit", "終了", Action::Exit),
];
pub const SAVE: &[MenuEntry] = &[
    ed("Save", "保存", Action::Save),
    e("Save As", "別名保存", Action::SaveAs),
    e("Autosave", "自動保存", Action::ToggleAutoSave),
];
pub const UNDO: &[MenuEntry] = &[
    ed("Undo", "取消", Action::Undo),
    e("Last", "直前", Action::Undo), // Undo と同じ 1 回分（Java 版にある名前なので残している）
    e("All", "全部", Action::UndoAll),
    e("History", "履歴", Action::ViewHistory),
];
pub const REDO: &[MenuEntry] = &[
    ed("Redo", "やり直し", Action::Redo),
    e("Last", "直前", Action::Redo), // Redo と同じ 1 回分
    e("All", "全部", Action::RedoAll),
    e("Restore", "復元", Action::RestoreHistory),
];
pub const EDIT: &[MenuEntry] = &[
    ed("Word", "単語", Action::SelectWord),
    e("All", "全部", Action::SelectAll),
    e("Magic", "拡張", Action::MagicSelect),
    e("Deselect", "解除", Action::Deselect),
    e("Copy", "コピー", Action::CopySelection),
];
pub const COPY: &[MenuEntry] = &[
    ed("Copy", "コピー", Action::Copy),
    e("Line", "行", Action::CopyLine),
    e("Cut", "切取", Action::CutLine),
    e("Name", "名前", Action::CopyFileName),
    e("Stash", "履歴", Action::CopyToHistory),
];
pub const PASTE: &[MenuEntry] = &[
    ed("Paste", "貼付", Action::Paste),
    e("Before", "前", Action::PasteBefore),
    e("After", "後", Action::PasteAfter),
    e("History", "履歴", Action::PasteFromHistory),
];
pub const RUN: &[MenuEntry] = &[
    ed("Run", "実行", Action::RunCmd),
    e("JSON", "JSON", Action::FormatJson),
    e("View", "表示", Action::ChangeView),
    e("Encoding", "文字コード", Action::ChangeEncoding),
    e("Font", "フォント", Action::ChooseFont),
    e("Reset Font", "初期フォント", Action::ResetFont),
    e("Size", "大きさ", Action::ChangeSize),
    e("Fullscreen", "全画面", Action::ToggleFullScreen),
    e("Count", "文字数", Action::CountText),
    e("Mode", "明暗", Action::ToggleTheme),
    e("Theme", "配色", Action::ChooseTheme),
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
        Action::SetLang(l) => Some(crate::lang::current() == l),
        Action::SetTheme(i) => Some(crate::ui::theme::PALETTES.get(i).is_some_and(|p| p.name == crate::ui::theme::current_name())),
        Action::EditCustomTheme => Some(crate::ui::theme::current_name() == crate::ui::theme::CUSTOM_NAME),
        _ => None,
    }
}

/// メニューの項目のツールチップ（その項目で何が起きるか）。
pub fn describe(a: Action) -> &'static str {
    use Action::*;
    match a {
        Open => crate::lang::tr("ファイルを選んで開く（Ctrl+O）", "Choose a file to open (Ctrl+O)"),
        OpenFolder => crate::lang::tr("フォルダを選んで、中身の一覧を開く", "Choose a folder and open its file list"),
        Save => crate::lang::tr("上書き保存する（Ctrl+S）", "Save over the current file (Ctrl+S)"),
        SaveAs => crate::lang::tr("名前を付けて保存する", "Save under a new name"),
        Exit => crate::lang::tr("NeoNote を終了する", "Quit NeoNote"),
        Undo => crate::lang::tr("直前の編集を 1 回取り消す（Ctrl+Z）", "Undo the last edit (Ctrl+Z)"),
        Redo => crate::lang::tr("取り消した編集を 1 回やり直す（Ctrl+Y）", "Redo the last undone edit (Ctrl+Y)"),
        UndoAll => crate::lang::tr("開いてから今までの編集を全部取り消す", "Undo all edits since the file was opened"),
        RedoAll => crate::lang::tr("取り消した編集を全部やり直す", "Redo all undone edits"),
        ViewHistory => crate::lang::tr("過去に読み込んだファイルの一覧を見る", "View the list of previously loaded files"),
        RestoreHistory => crate::lang::tr("履歴から以前の内容を選んで戻す", "Pick an earlier version from history and restore it"),
        Cut => crate::lang::tr("選択範囲を切り取る", "Cut the selection"),
        Copy => crate::lang::tr("選択範囲をコピーする（Ctrl+C）", "Copy the selection (Ctrl+C)"),
        Paste => crate::lang::tr("クリップボードの内容を貼り付ける（Ctrl+V）", "Paste from the clipboard (Ctrl+V)"),
        PasteBefore => crate::lang::tr("今の行の前に貼り付ける", "Paste before the current line"),
        PasteAfter => crate::lang::tr("今の行の後に貼り付ける", "Paste after the current line"),
        PasteFromHistory => crate::lang::tr("コピーした履歴から選んで貼り付ける", "Pick from copy history and paste"),
        CopyLine => crate::lang::tr("今の行をまるごとコピーする", "Copy the whole current line"),
        CutLine => crate::lang::tr("今の行をまるごと切り取る", "Cut the whole current line"),
        CopyFileName => crate::lang::tr("開いているファイルの名前をコピーする", "Copy the open file's name"),
        CopyPath => crate::lang::tr("開いているファイルの場所をコピーする", "Copy the open file's location"),
        CopyToHistory => crate::lang::tr("選択範囲をコピーの履歴に残す", "Keep the selection in copy history"),
        CopySelection => crate::lang::tr("選択範囲をコピーする", "Copy the selection"),
        SelectAll => crate::lang::tr("全文を選択する（Ctrl+A）", "Select all text (Ctrl+A)"),
        SelectWord => crate::lang::tr("カーソルのある単語を選択する", "Select the word at the cursor"),
        MagicSelect => crate::lang::tr("押すたびに選択を 単語 → 行 → 段落 → 全体 と広げる", "Each press widens the selection: word, line, paragraph, all"),
        Deselect => crate::lang::tr("選択を外す", "Clear the selection"),
        Replace => crate::lang::tr("検索欄の文字を置換する", "Replace with the text in the search box"),
        Search => crate::lang::tr("選択範囲の文字で検索する", "Search for the selected text"),
        SearchWeb => crate::lang::tr("選択範囲の文字をブラウザで検索する", "Search the selected text in the browser"),
        OpenImageUrl => crate::lang::tr("選択範囲の画像の URL を開く", "Open the image URL in the selection"),
        RunCmd => crate::lang::tr("選択範囲をコマンドとして実行する。選択が無ければ今のフォルダを開く", "Run the selection as a command. With no selection, open the current folder"),
        ChangeView => crate::lang::tr("表示（Text・Preview・HEX）と Preview の形式を切り替える", "Switch the view (Text, Preview, HEX) and the Preview format"),
        ChangeEncoding => crate::lang::tr("文字コード・改行コードを切り替える（Ctrl+E）", "Change the text encoding and line endings (Ctrl+E)"),
        ChangeSize => crate::lang::tr("文字の大きさを変える（Ctrl+ホイールでも変えられる）", "Change the text size (Ctrl+wheel also works)"),
        ToggleFullScreen => crate::lang::tr("全画面表示を切り替える（F11）", "Toggle full screen (F11)"),
        CountText => crate::lang::tr("文字数・単語数・行数を数える", "Count characters, words and lines"),
        ChooseTheme => crate::lang::tr("配色を一覧から選ぶ", "Pick a color theme from the list"),
        ToggleTheme => crate::lang::tr("明るい配色と暗い配色を切り替える", "Switch between light and dark themes"),
        ChooseFont => crate::lang::tr("表示に使うフォントを選ぶ", "Choose the display font"),
        ResetFont => crate::lang::tr("フォントを同梱の NeoFont に戻す", "Reset the font to the bundled NeoFont"),
        OpenReadme => crate::lang::tr("使い方の説明（README）を開く", "Open the README"),
        NewTab => crate::lang::tr("新しい空のタブを開く", "Open a new empty tab"),
        CloseTab => crate::lang::tr("今のタブを閉じる", "Close the current tab"),
        NextTab => crate::lang::tr("右隣のタブへ移る", "Go to the tab on the right"),
        PrevTab => crate::lang::tr("左隣のタブへ移る", "Go to the tab on the left"),
        OpenHomePage => crate::lang::tr("NeoNote のホームページをブラウザで開く", "Open the NeoNote home page in the browser"),
        CheckUpdate => crate::lang::tr("新しい版が出ているか確かめる", "Check whether a new version is available"),
        MakeShortcut => crate::lang::tr("デスクトップに NeoNote のショートカットを作る", "Create a NeoNote shortcut on the desktop"),
        OpenRecent => crate::lang::tr("最近開いたファイルから選んで開く", "Pick a recently opened file"),
        OpenConfig => crate::lang::tr("設定ファイル（config.txt）をタブで開く。保存すると反映される", "Open config.txt in a tab. Changes apply when saved"),
        FormatJson => crate::lang::tr("選択範囲（無ければ全文）の JSON を整形する", "Format the JSON in the selection (or the whole text)"),
        ToggleAutoSave => crate::lang::tr("編集を自動で保存するかを切り替える", "Turn automatic saving on or off"),
        ToggleWrap => crate::lang::tr("右端で折り返すかを切り替える", "Turn line wrapping at the right edge on or off"),
        GotoLine => crate::lang::tr("行番号を入れて、その行へ移る", "Enter a line number and jump to it"),
        ConvertEol => crate::lang::tr("改行コード（CRLF・LF）を変換する", "Convert line endings (CRLF, LF)"),
        ToggleResume => crate::lang::tr("次の起動で、前回のタブと未保存の変更を戻す（切ると、終了時に未保存の変更の保存を確認する）", "Restore last tabs and unsaved changes at next launch (when off, ask about unsaved changes on exit)"),
        ToggleLang => crate::lang::tr("日本語と英語を切り替える", "Switch between Japanese and English"),
        SetLang(_) => crate::lang::tr("画面の言葉をこの言語にする", "Use this language for the interface"),
        SetTheme(_) => crate::lang::tr("この配色にする", "Use this color theme"),
        EditCustomTheme => crate::lang::tr("自分で決めた配色（config.txt の custom_*）にして、設定を開く", "Use your custom theme (custom_* in config.txt) and open settings"),
    }
}

/// ポップアップを出し、選ばれた Action を返す。グレーの項目は選べない。
pub fn popup(entries: &[MenuEntry], x: i32, y: i32) -> Option<Action> {
    let rows: Vec<Row> = entries
        .iter()
        .map(|m| Row {
            label: m.text(),
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
                assert!(!m.ja.contains(['/', '&']), "bad char in {}", m.ja);
            }
        }
    }
}
