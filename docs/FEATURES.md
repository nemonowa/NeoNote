# NeoNote 機能一覧 / Feature catalog

> NeoNote is a lightweight, cross-platform (Windows / Linux / macOS) single-binary **text, hex and file editor** written in **Rust (fltk-rs)**.
> It opens plain text in any common encoding, previews Markdown / HTML / CSV / Office / PDF / images / audio, browses **folders, zip, tar and ext2/3/4 disk images** as a tree + **treemap (WizTree-like)**, edits **Minecraft NBT / .mca**, and has **regex search & replace**.
> 軽く・速く・省クリックで動く、単一実行ファイルの超汎用ファイルエディタ。メモ帳の代わりに使える軽さで、バイナリ・文書・ディスクイメージまで開ける。

このファイルは、機能を **言葉（検索語）・対応形式・実装の場所（ファイル）** で引けるようにした一覧。人にも、リポジトリを検索する AI エージェントにも向けて書いている。
詳しい操作は [USAGE.md](USAGE.md)、設計は [DESIGN.md](DESIGN.md)、実測値は [MEASURE.md](MEASURE.md)、未実装の案は [IDEAS.md](IDEAS.md)。
`src/neo/` は GUI を import しない層（単体テストできる）、`src/ui/` は fltk の画面、`src/app.rs` が全体をつなぐ。

## 目次 / Index

1. [編集 Editing](#編集-editing)
2. [検索・置換 Search & replace](#検索置換-search--replace)
3. [文字コード・改行 Encodings & line endings](#文字コード改行-encodings--line-endings)
4. [プレビュー Preview](#プレビュー-preview)
5. [フォルダ・アーカイブ・ディスクイメージ Folders, archives, disk images](#フォルダアーカイブディスクイメージ-folders-archives-disk-images)
6. [ツリーマップ Treemap](#ツリーマップ-treemap)
7. [HEX・バイナリ Hex & binary](#hexバイナリ-hex--binary)
8. [Minecraft（NBT・mca・mcfunction）](#minecraftnbtmcamcfunction)
9. [コード・データ Code & data](#コードデータ-code--data)
10. [乗せて確かめる Hover peek](#乗せて確かめる-hover-peek)
11. [メタデータ Metadata](#メタデータ-metadata)
12. [保存・復元 Saving & session](#保存復元-saving--session)
13. [画面・配色・言語 UI, themes, language](#画面配色言語-ui-themes-language)
14. [Windows 連携・起動 Windows integration & launching](#windows-連携起動-windows-integration--launching)
15. [品質・計測・ビルド Quality, measurement, build](#品質計測ビルド-quality-measurement-build)
16. [やらないこと Non-goals](#やらないこと-non-goals)

---

## 編集 Editing
Keywords: notepad alternative, lightweight text editor, tabs, undo redo, auto-indent, go to line, clipboard history, メモ帳の代わり, タブ, 元に戻す, 字下げ

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| タブ | 複数ファイルを 1 窓で。ドラッグで並べ替え（入る場所に線）、`Ctrl+Tab` で切替、`Ctrl+Shift+PageUp/Down` で左右へ移動 | `src/ui/tabs.rs`, `src/app.rs` |
| **単一ウィンドウ（メモ帳のように）** | 起動中に別のファイルを開くと、新しいウィンドウを作らず、起動中のウィンドウのタブで開く（`--new-window` / `Ctrl+Shift+N` で別窓）。タブをウィンドウの外へドラッグして別のウィンドウの上で離すと、そのウィンドウへ移る（未保存の変更ごと）。localhost の TCP + 合言葉つきの登録ファイル。依存なし | `src/neo/ipc.rs`, `src/main.rs`, `src/app.rs` |
| 元に戻す / やり直し | 1 回ずつと一括（Undo All / Redo All） | `src/app.rs` |
| Magic Select | 単語 → 行 → 段落 → 全体と、押すたびに選択を広げる | `src/neo/selection.rs` |
| 字下げ | `Tab` / `Shift+Tab` で複数行をまとめて字下げ。`Enter` で前の行の字下げを引き継ぐ（CRLF も保つ） | `src/neo/indent.rs` |
| 行単位の操作 | 行の切り取り・コピー、前後への貼り付け（Paste Before / After）、Copy Selection | `src/app.rs` |
| 指定行へ移動 | `Ctrl+G`。`12` か `12:5`（行:桁）。JSON のエラー位置をそのまま貼れる | `src/neo/goto.rs` |
| クリップボード履歴 | 直近 10 件（起動中のみ）から貼り付け | `src/neo/history.rs` |
| 折り返し・文字サイズ・文字数 | 折り返しは既定で有効、`Ctrl+ホイール` / `Ctrl++` / `Ctrl+-` / `Ctrl+0`、文字数・行数・単語数 | `src/app.rs`, `src/neo/zoom.rs` |

## 検索・置換 Search & replace
Keywords: regex search, regular expression, whole word, smart case, find and replace, capture groups, search in files, 正規表現, 単語単位, 置換

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 常駐する検索バー | ツールバー右端。入力するたびに全一致を強調して最初の一致を選ぶ。`Enter` / `F3` で次、`Shift+Enter` / `Shift+F3` で前、件数はツールチップ | `src/ui/searchbar.rs`, `src/app.rs` |
| スマートケース | 小文字だけで打つと大文字小文字を無視、大文字を含めると区別（操作を増やさない）。全文のコピーを作らない | `src/neo/replace.rs` `find_all_smart` |
| 正規表現 | 検索欄の `.*` で切替。入力の長さに比例する時間（`regex` crate。危険なパターンで固まらない）。不正なら赤字で「正規表現が不正です」 | `src/neo/replace.rs` `find_all_regex` |
| 単語単位 | 検索欄の `W` で切替（Unicode の `\b`。正規表現オフなら文字どおり） | 同上 |
| 置換 | `Ctrl+H`。置換欄 `Enter` か All で全置換（元に戻す 1 回で戻る）、Repl で 1 件ずつ。正規表現では `$1` `${name}` `$$` | `src/neo/replace.rs` `regex_replacements` |
| 選択した文字列の検索 | 右クリックの Search（選んだ文字列を全て強調。文字どおり・大文字小文字を区別） | `src/app.rs` |
| Web で検索 | 選択した文字列をブラウザで検索 | `src/app.rs` |
| フォルダ・zip・ext イメージの検索 | 一覧の中の名前と、文字のファイルの本文を探し、一致する項目と親のフォルダだけに絞る（`/` を含めると道筋全体） | `src/neo/archive.rs` `search_contents` |

## 文字コード・改行 Encodings & line endings
Keywords: Shift_JIS, Windows-31J, EUC-JP, UTF-16, BOM, auto-detect encoding, CRLF LF, 文字化け, 文字コード, 改行コード

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 文字コード | UTF-8（BOM あり/なし）・Windows-31J（Shift_JIS）・EUC-JP・UTF-16LE/BE・ISO-8859-1・US-ASCII。自動判定、`Ctrl+E` で切替、保存できない文字があれば事前に警告 | `src/neo/encoding.rs` |
| 改行コード | CRLF / LF / 混在の表示と変換。開いた形式は保存してもそのまま | `src/neo/eol.rs` |

## プレビュー Preview
Keywords: markdown preview, html viewer, csv viewer, xlsx viewer, docx viewer, pptx, epub reader, pdf text, image viewer, audio player, mp3 flac wav

`Format`（タブ行の右端）で表示を選ぶ（拡張子が txt でも選べる）。読み取り専用の簡易表示。

| 形式 | 内容 | 場所 |
| --- | --- | --- |
| Markdown | GitHub に近い見た目（コード・引用・注記・表・チェックボックス） | `src/neo/markdown.rs` |
| HTML / HTM | HelpView が描ける形に作り直して表示（色と揃えも再現） | `src/neo/htmlview.rs`, `htmlstyle.rs` |
| CSV / TSV | 罫線つきの表。区切り（`,` `;` タブ `|`）を自動判定、引用符・引用符内の改行に対応 | `src/neo/sheet.rs` |
| xlsx / ods | シートごとの表（数式は計算済みの値） | `src/neo/book.rs` |
| docx / odt / pptx / odp / epub | 見出し・段落・リスト・表の文字だけの文書（pptx はスライドごと、epub は章ごと） | `src/neo/doctext.rs` |
| PDF | 文字だけの簡易表示（レイアウト・画像は再現しない。画像だけのスキャンは文字が出ない） | `src/neo/pdftext.rs`（`lopdf`） |
| 画像 | 表示。画像のパスへマウスを乗せると縮小画像 | `src/neo/imagefile.rs` |
| 音声 | MP3 / FLAC / WAV の情報表示と簡易再生（`audio` feature、既定で有効） | `src/audioplay.rs`, `vendor/miniaudio/` |
| フォルダ・zip・tar・gz・ext イメージ | 一覧（次の節） | `src/neo/archive.rs` |

## フォルダ・アーカイブ・ディスクイメージ Folders, archives, disk images
Keywords: zip viewer, tar viewer, tar.gz, ext4 image reader, ext2 ext3, read Linux filesystem on Windows, Linux Reader, disk image browser, MBR GPT partitions, read-only, ディスクイメージ, パーティション

| 対象 | 内容 | 場所 |
| --- | --- | --- |
| フォルダ | 再帰で一覧。**フォルダ単位の並列走査**（最大 8 本）。シンボリックリンクはたどらない。上限 5 万件 | `src/neo/archive.rs` `list_dir` |
| zip | 目録を読んで一覧（圧縮前後の大きさ）。名前の変更（`Rename (text)`）は zip とフォルダだけ | `src/neo/archive.rs`, `inflate.rs`, `deflate.rs` |
| tar / tar.gz / tgz / gz | ustar・GNU・PAX の名前と大きさの一覧（読み取り専用）。gzip は自前の inflate | `src/neo/tarlist.rs` |
| **ext2 / ext3 / ext4 イメージ** | `.img` `.ext2` `.ext3` `.ext4` `.raw` `.dd`。ブロックマップ・エクステント・64bit・スパースに対応。**MBR / GPT つきのディスクイメージ**は `part1/` のように分けて表示。必要な所だけ読む（数 GB でも開ける）。本文の検索も可。一覧のファイルをダブルクリック（右クリックの「NeoNote で開く」）で、一時フォルダへ読み取り専用で取り出して開く（64 MiB まで。終了時に削除）。**読み取り専用・依存クレートなし**。ジャーナルの再生・暗号化・inline_data・meta_bg は読まない | `src/neo/extfs.rs` |

## ツリーマップ Treemap
Keywords: treemap, disk usage, WizTree, folder size visualization, squarified, 面積図, フォルダの大きさ

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 面積図 | 右パネルに、フォルダ・zip・イメージの中身を大きさに比例した四角で表示（WizTree のような表示）。フォルダは入れ子の枠、ファイルは拡張子ごとの色。squarified 配置、小さすぎる四角は描かない | `src/ui/treemap.rs`, `src/ui/sidepanel.rs` |
| 高速化 | 木づくりは名前の HashMap（4 万件で約 45 → 5〜9 ms）、走査は並列（約 50 → 34 ms）。WizTree の調査と見送った案は IDEAS.md | `docs/MEASURE.md`, `docs/IDEAS.md` |

## HEX・バイナリ Hex & binary
Keywords: hex editor, binary editor, byte editing, 16 bytes per line, バイナリ編集

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| HEX 編集 | バイナリは自動で HEX 表示で開き、`Ctrl+E` で切替。1 行 16 バイト、バイト列のまま保存、不正な箇所は位置を示して保存を中止。16 MiB で打ち切り | `src/neo/hex.rs` |

## Minecraft（NBT・mca・mcfunction）
Keywords: Minecraft NBT editor, level.dat editor, region file .mca editor, SNBT, litematic, schematic, mcfunction, マイクラ, ワールド編集

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| NBT 編集 | `.dat` `.nbt` `.schem` `.litematic` をテキスト（SNBT）でもツリー形式でも編集。編集した部分だけ書き換える | `src/neo/nbt.rs`, `nbtedit.rs`, `src/ui/nbttree.rs` |
| `.mca`（リージョン） | チャンクの一覧から開く。gzip / zlib / 無圧縮 / LZ4 | `src/neo/region.rs` |
| `.mcfunction` | 色分け、括弧と引用符の閉じ忘れに細い下線、呼び出し元ヘッダーの作成 | `src/neo/mcfunction.rs`, `mcheader.rs` |
| JSON の整形 | 厳密な JSON だけ受ける自前の整形（数値・キーの順を変えない。エラーは位置つき）。言語ファイルの修正用 | `src/neo/json.rs` |

## コード・データ Code & data
Keywords: syntax highlighting, Python, JavaScript, TypeScript, Java, C, C++, Rust, Go, shell, SQL, CSS, JSON

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 色分け | Python・JavaScript / TypeScript・Java・C / C++・Rust・Go・シェル・SQL・CSS・JSON など（軽量な字句の色分けで、IDE 化はしない） | `src/neo/code.rs` |

## 乗せて確かめる Hover peek
Keywords: color preview, hex color code tooltip, unicode escape, unix timestamp, image path preview

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 小窓 | カラーコード（`#rgb` `#rrggbb` `rgb()` 色の名前）の見本、文字参照（`&amp;` `&#x3042;`）と `あ` の文字、UNIX 時刻（10 桁の秒・13 桁のミリ秒）の日時、画像のパスの縮小画像。URL の画像は選んだときだけ取得 | `src/neo/peek.rs`, `src/ui/peekwin.rs` |

## メタデータ Metadata
Keywords: ID3 tag editor, FLAC Vorbis comment, WAV INFO, PNG tEXt, JPEG comment, タグ編集

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 音声 | MP3（ID3v2.3 / 2.4・ID3v1）、FLAC（Vorbis コメント）、WAV（LIST/INFO）の情報表示と読み書き | `src/neo/audiometa.rs` |
| 画像 | PNG（tEXt / 圧縮なしの iTXt）・JPEG（COM）の文字のメタデータを `key=value` で読み書き | `src/neo/imgmeta.rs` |

## 保存・復元 Saving & session
Keywords: auto save, restore session, unsaved changes recovery, file history, external change detection, view position memory, 続きから, 自動保存

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 自動保存・外部変更の検知 | Enable Auto Save。外部で変わったファイルを検知 | `src/app.rs` |
| 続きから | 引数なしの起動で前回のタブを開き直す。未保存の変更は控え（設定フォルダの `unsaved/`）から戻す | `src/neo/unsaved.rs` |
| **表示位置の記憶** | 閉じて開き直すと前の位置から。(サイズ, 更新時刻) が一致したときだけ復元、最大 60 件 | `src/neo/viewstate.rs` |
| ファイル履歴・最近使った | View / Restore From History、Open Recent | `src/neo/pathlog.rs`, `src/neo/recent.rs` |
| 安全な保存 | 失う変更があるときだけ閉じる前に確認。文字コード・改行は開いたまま保つ | `src/neo/document.rs` |

## 画面・配色・言語 UI, themes, language
Keywords: dark mode, light theme, custom theme, Japanese English UI, side panel, minimap, line numbers on hover, fullscreen, always on top

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| 最小の画面 | 行番号・右パネルはマウスを端へ寄せるか `Alt` で表示。ツールバーは押せば主な操作、乗せると関連の一覧 | `src/ui/` |
| 右パネル | ファイル情報・ミニマップ（フォルダ・zip・イメージはツリーマップ） | `src/ui/sidepanel.rs` |
| 配色 | 10 のテーマ（ダーク・ライト（既定）・濡羽・月白・nord・abyss・paper・sakura・Discord 風・GitHub 風ほか）と custom（5 色）。月のボタンで明暗。コントラスト比をテストで確認 | `src/ui/theme.rs`, `src/ui/skin.rs` |
| 言語 | 日本語 / English（旗のボタン） | `src/lang.rs` |
| 表示 | 全画面 `F11`・最前面に固定（Windows）・行番号と右パネルの幅の調整・同梱フォント NeoFont | `src/ui/` |

## Windows 連携・起動 Windows integration & launching
Keywords: Windows jump list, taskbar right-click, recent files, command line, drag and drop, portable, USB

| 機能 | 内容 | 場所 |
| --- | --- | --- |
| ジャンプリスト | タスクバーのアイコンを右クリックして最近のファイルを開く（COM を直接呼ぶ。Windows での動作は未確認） | `src/jumplist.rs` |
| コマンドライン | `neonote [--log-level LEVEL] [--] [パス...]`。全てタブで開く | `src/main.rs` |
| ドロップ・ショートカット | ウィンドウへドロップで開く、デスクトップへのショートカット、パスのコピー、Run In CMD | `src/neo/dropped.rs`, `src/app.rs` |
| ポータブル | 実行ファイルの隣に `neonote` フォルダを作ると、そこへ設定を保存（USB メモリ向け） | `src/neo/config.rs` |
| 配布 | 単一の実行ファイル。ビルドして `target/` の不要物を消し、exe を `build.cmd` と同じ場所へ出す Windows 用の `build.cmd` は、リポジトリとは別のファイルで配る | `build.cmd`（別ファイル） |

## 品質・計測・ビルド Quality, measurement, build
Keywords: GitHub Actions CI, Rust cross-platform CI, binary size, performance benchmark

| 項目 | 内容 | 場所 |
| --- | --- | --- |
| テスト | `cargo test`（GUI なしの層を単体テスト。ext は `mke2fs` で作ったイメージとバイト照合） | `src/neo/*.rs` |
| 計測 | 検索・TextBuffer・フォルダ走査の時間（`cargo test --release perf_ -- --ignored --nocapture --test-threads=1`）。基準値は MEASURE.md | `src/neo/perf.rs`, `docs/MEASURE.md` |
| CI | Windows / macOS / Linux でテスト・リリースビルド・バイナリサイズ・計測を記録 | `.github/workflows/ci.yml` |
| 大きさ | リリースビルドで約 16.1 MB（約 10 MB は同梱フォント）。正規表現を入れて +1.10 MiB | `docs/MEASURE.md` |

## やらないこと Non-goals
IDE 化しない。AI 機能・拡張機構・メモ帳に不要な煩雑な機能は足さない。拡張の入口は「ツールバーのボタン」と「右クリックメニューの項目」だけ。
