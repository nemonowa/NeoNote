# README に足す文面（下書き）

`README.md` の末尾近く（「連繫」の前）にそのまま貼れる。全機能の一覧は [FEATURES.md](FEATURES.md)（検索語・対応形式・実装の場所つき）。
貼ったあとに直すもの: README の `tests-278 passed` バッジを `cargo test` の実際の数にする。「機能」の表の「検索」「プレビュー」「右パネル」に、下の太字の追加分を足す。

---

## ❱ 何ができるか【What it can do】

NeoNote is a **lightweight, single-binary text / hex / file editor** (Rust, fltk-rs; Windows, Linux, macOS).
全機能の索引: **[docs/FEATURES.md](docs/FEATURES.md)**

| 分野 | できること（検索語） |
| --- | --- |
| ✏️ 編集 | **起動中ならファイルは別窓にせずタブで開く（メモ帳のように）・タブのドラッグで別ウィンドウへ移動**、タブ、元に戻す/やり直し（一括可）、Magic Select、字下げ・自動字下げ、指定行へ移動（`12:5`）、クリップボード履歴、折り返し、文字サイズ — *notepad alternative, tabs, undo, auto-indent, go to line* |
| 🔍 検索・置換 | 常駐の検索バー、**スマートケース**、**正規表現（`.*`）**、**単語単位（`W`）**、`$1` を使える置換、フォルダ・zip・ext イメージの名前と本文の検索 — *regex search, whole word, smart case, find & replace, capture groups* |
| 🔤 文字コード・改行 | UTF-8 / Shift_JIS(Windows-31J) / EUC-JP / UTF-16LE・BE / ISO-8859-1 / US-ASCII、BOM、自動判定、CRLF/LF/混在の変換 — *Shift_JIS, EUC-JP, UTF-16, auto-detect encoding* |
| 👀 プレビュー | Markdown、HTML、CSV/TSV、xlsx/ods、docx/odt/pptx/odp/epub、PDF（文字）、画像、音声（MP3/FLAC/WAV 再生と情報） — *markdown preview, csv viewer, xlsx, docx, epub, pdf text* |
| 🗂️ フォルダ・アーカイブ・ディスクイメージ | フォルダ（**並列走査**）、zip、tar / tar.gz / tgz / gz、**ext2/3/4 イメージ（MBR/GPT 対応・読み取り専用・依存なし）** — *zip viewer, tar viewer, ext4 image reader, read Linux filesystem on Windows, Linux Reader alternative* |
| 📊 ツリーマップ | フォルダ・zip・イメージの大きさを面積で表示（WizTree 風、squarified、HashMap で高速化） — *treemap, disk usage, WizTree-like* |
| 🧩 HEX | バイナリを自動で HEX 表示、1 行 16 バイト、バイト列のまま保存 — *hex editor, binary editor* |
| ⛏️ Minecraft | NBT（`.dat` `.nbt` `.schem` `.litematic`）のテキスト/ツリー編集、`.mca`、`.mcfunction`、JSON 整形 — *Minecraft NBT editor, level.dat, region file, SNBT* |
| 🌈 コード | Python / JS・TS / Java / C・C++ / Rust / Go / シェル / SQL / CSS / JSON の色分け — *syntax highlighting* |
| 🎯 乗せて確かめる | カラーコード、文字参照・`\u`、UNIX 時刻、画像のパスの縮小画像 — *color preview, unicode escape, unix timestamp* |
| 🏷️ メタデータ | MP3(ID3v1/v2.3/2.4)・FLAC(Vorbis)・WAV(INFO)・PNG(tEXt/iTXt)・JPEG(COM) の読み書き — *ID3 tag editor* |
| 💾 保存・復元 | 自動保存、外部変更の検知、続きから（未保存の変更も）、**表示位置の記憶**、ファイル履歴 — *auto save, restore session, session recovery* |
| 🎨 画面 | 10 の配色＋custom、明暗の切替、日本語/English、行番号・右パネルは端へ寄せたときだけ、全画面、最前面 — *themes, dark mode, minimal UI* |
| 🪟 Windows | **ジャンプリスト**（タスクバー右クリックで最近のファイル）、ドロップ、デスクトップへのショートカット、ポータブル運用 — *jump list, portable* |
| 📏 品質 | Windows / macOS / Linux の CI、バイナリサイズと性能の計測（[MEASURE.md](docs/MEASURE.md)） — *GitHub Actions, benchmark* |

> 検索の語: `text editor` `hex editor` `file editor` `notepad replacement` `regex` `treemap` `disk usage` `ext4` `disk image` `zip` `tar` `markdown preview` `csv viewer` `Shift_JIS` `Minecraft NBT` `jump list` `Rust` `fltk` `single binary` — 実装の場所は [FEATURES.md](docs/FEATURES.md)、ext の仕組みは [src/neo/extfs.rs](src/neo/extfs.rs)。

---

## ❱ 実装メモ【How it works】（短い版）

| 機能 | 場所 | 手法 |
| --- | --- | --- |
| ext2/3/4 の読み取り | `src/neo/extfs.rs` | スーパーブロック → グループ記述子 → inode → エクステント / 間接ブロック。`Read + Seek` で必要な所だけ読む。`mke2fs -d` で作ったイメージで、ext2/3/4・ブロック 1K/4K の中身をバイト単位で照合 |
| ディスクイメージ | 同上 | MBR（4 エントリ）/ GPT（ヘッダーと表）を読み、ext を含む区画だけを開く |
| 走査の並列化 | `src/neo/archive.rs` `list_dir` | 作業キュー + `std::thread::scope`。フォルダ 1 つが 1 仕事。結果は並べ替えて順序を固定 |
| ツリーマップ | `src/ui/treemap.rs` | squarified 配置。子の探しは名前の HashMap（4 万件で約 45 → 5〜9 ms） |
| 検索 | `src/neo/replace.rs` | スマートケースは全文コピーなし。正規表現・単語単位は `regex`（Unicode の `\b` と大文字小文字の表だけ有効にして +1.10 MiB） |
| 表示位置の記憶 | `src/neo/viewstate.rs` | (サイズ, 更新時刻) が一致したときだけ復元。最大 60 件 |
| 単一ウィンドウ・タブの移動 | `src/neo/ipc.rs` | 各ウィンドウが 127.0.0.1 の空きポートで待ち受け、`設定フォルダ/instances/<pid>.txt` に「ポート + 合言葉」を書く。2 つ目の起動は、最後に操作されたウィンドウへ `OPEN` を送って終わる（応答が無ければ自分が窓になる）。タブは `ADOPT`（未保存の本文つき）で渡す。依存なし |
| ジャンプリスト | `src/jumplist.rs` | Windows の COM（`ICustomDestinationList`）を FFI で直接呼ぶ。追加の依存なし |

## ❱ GitHub の About / Topics（リポジトリ設定で入れる）

**About（英語）**: Lightweight cross-platform text, hex and file editor in Rust (fltk-rs): regex search, Markdown/CSV/Office/PDF preview, Minecraft NBT, and a treemap viewer for folders, zip, tar and ext2/3/4 disk images (Linux Reader / WizTree-like).

**About（日本語）**: 軽量・省クリックの超汎用ファイルエディタ（Rust / fltk-rs）。正規表現検索、各種プレビュー、Minecraft NBT、フォルダ・zip・tar・ext4 イメージのツリーマップ閲覧。

**Topics**: `rust` `fltk` `fltk-rs` `text-editor` `hex-editor` `file-editor` `notepad-alternative` `markdown-preview` `csv-viewer` `regex` `treemap` `disk-usage` `wiztree` `ext4` `ext2` `ext3` `disk-image` `filesystem` `linux-reader` `mbr` `gpt` `zip` `tar` `nbt` `minecraft` `shift-jis` `windows-jump-list` `single-binary` `cross-platform`

**Website 欄**: `docs/FEATURES.md` へのリンク（GitHub 上の機能索引）
