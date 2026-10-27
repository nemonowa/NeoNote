<div align="center">

<img src="assets/icon.png" width="96" alt="NeoNote">

# ⚓｜NeoNote - FileEditor

**説明：** 艦長が自分用に開発したコードエディタ。メモ帳替わり。<br>ファイルの構造を解析して、文字コードを自動で選ぶ（バイナリも読めます）。

*Only for Writing. Light. Fast. Clickless. Tuned to be yours.*

![Rust](https://img.shields.io/badge/Rust-fltk--rs-b7410e?logo=rust)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-555)
![Binary](https://img.shields.io/badge/binary-single%20exe-2ea44f)
![Tests](https://img.shields.io/badge/tests-278%20passed-brightgreen)

![NeoNote](docs/screenshots/main.png)

</div>

---

## ❱ 情報【Information】

設計理念は「**とにかく軽く、早く、省クリック数で動作する**」こと。<br>
主な用途はメモ書きから軽いコーディングまで。数クリックで済む作業に、VSCode や Eclipse のような重いツールを立ち上げたくない時に使う。<br>
特にマイクラ民向け（JSON の整形、`level.dat`・`.mca` の編集）。

```text
App      : NeoNote
Version  : 2.1.77
Language : Rust (fltk-rs)
Tested   : Windows 11（Linux・macOS）
Author   : argonaute_nemo
```

---

## ❱ 特徴【Highlights】

- ⚡ **単一の実行ファイル**：インストール不要。ダウンロードしてすぐ使える
- 🖱️ **少ないクリック**：ツールバーは左クリックで主な操作、右クリックで関連する操作
- 🧹 **画面に余計なものを出さない**：行番号と右パネルは、マウスを画面の端へ寄せた時か `Alt` を押した時だけ表示
- 🎨 **千変万化のカラーテーマ**：UIの構成配色を全て自由に変更可能。6つのカラーテーマと無限の自由配色。
- 🔤 **幅広い対応性**：あらゆる文字コード、フォーマットファイル形式など自動で読み込みプレビューできます。UTF-8・Shift_JIS・EUC-JP・UTF-16 + 改行コード（CRLF / LF）など。
- 👀 **プレビュー**：Markdown・HTML・CSV・Excel・Word・zip などをその場で表示
- ⛏️ **Minecraft の NBT を編集**：`level.dat` や `.mca` を、テキストでもツリー形式でも編集できる
- 🧩 **HEX 編集**：バイナリは自動で HEX 表示で開き、そのまま保存できる

---

## ❱ 導入【Introduction】

1. [Releases](../../releases) から OS に合った実行ファイルをダウンロードする
2. `neonote.exe` をダブルクリックで起動する
3. ファイルをウィンドウへドロップするか、`Ctrl+O` で開く

```text
neonote [パス...]        # 引数を付けると、そのファイルをタブで開く
```

自分でビルドする場合：

```text
cargo build --release
```

---

## ❱ 画面【Gallery】

| | |
|:---:|:---:|
| ![右パネルとミニマップ](docs/screenshots/side_panel.png) | ![行番号はホバー時のみ表示](docs/screenshots/gutter_hover.png) |
| 右パネル：ファイル情報とミニマップ | 行番号はマウスを左端へ寄せた時だけ表示 |
| ![NBT のツリー](docs/screenshots/nbt_tree.png) | ![SNBT で編集](docs/screenshots/nbt_snbt.png) |
| level.dat をツリー形式で編集 | level.dat をテキスト（SNBT）で編集 |

---

## ❱ 配色【Themes】

| dark | light | nord |
|:---:|:---:|:---:|
| ![dark](docs/screenshots/theme_dark.png) | ![light](docs/screenshots/theme_light.png) | ![nord](docs/screenshots/theme_nord.png) |
| **abyss** | **paper** | **sakura** |
| ![abyss](docs/screenshots/theme_abyss.png) | ![paper](docs/screenshots/theme_paper.png) | ![sakura](docs/screenshots/theme_sakura.png) |

`Run` の右クリック → `Choose Theme` で選ぶ。**custom** を選ぶと、設定ファイルに書いた5色（`#rrggbb`）で独自の配色を作れる。
どの配色も、文字と背景のコントラスト比をテストで確認済み。

---

## ❱ 機能【Features】

| 分類 | 内容 |
|---|---|
| ✏️ 編集 | タブ・元に戻す / やり直し（一括も可）・Magic Select（単語→行→段落→全体）・字下げ・自動字下げ・行単位の切り取り / コピー・前後への貼り付け |
| 🔍 検索 | 常駐する検索バー・スマートケース・置換・Web で検索 |
| 💾 保存 | 自動保存・外部での変更の検知・前回のタブを開き直す・ファイル履歴 |
| 🔤 文字コード | UTF-8 / Windows-31J / EUC-JP / UTF-16LE / UTF-16BE / ISO-8859-1 / US-ASCII・保存できない文字があれば事前に警告 |
| ↩️ 改行 | CRLF / LF / 混在の表示と変換 |
| 👀 プレビュー | Markdown・HTML・CSV / TSV・xlsx / ods・docx / pptx / epub・zip / tar / gz の一覧・画像・音声 |
| ⛏️ NBT | `.dat`・`.nbt`・`.schem`・`.litematic`・`.mca`（チャンクの一覧から開く） |
| 🧩 HEX | 1行16バイト・不正な箇所があれば位置を示して保存を中止 |
| 🖥️ 表示 | 折り返し（既定で有効）・文字サイズ・全画面（`F11`）・文字数 / 行数 / 単語数 |
| 🛠️ ほか | JSON の整形・コマンドプロンプトでの実行・パスのコピー・デスクトップへのショートカット |

---

## ❱ 信条【Philosophy】

- ✍️ **「書く」ことに集中できるデザイン**
  - 行番号や情報パネルは、必要な時だけそっと現れる。普段は文字だけが並ぶ静かな画面。
  - 折り返しを標準にして、長い行でも横にスクロールせず読み進められる。
- 🪶 **軽さを守るための引き算**
  - 多機能な開発ツールにはしない。メモ帳に必要なものだけを、ツールバーと右クリックメニューに収める。
- 🛡️ **ファイルを守る安全な設計**
  - 開いたファイルの文字コードや改行の形式はそのまま。保存しても、意図しない変化を起こさない。
  - Minecraft のワールドも安心して編集できる。編集した部分だけを書き換え、それ以外には手を触れない。
  - 書き間違いがあれば保存を止めて、その場所を知らせる。
- 📐 **細部まで整えた、すっきりした見た目**
  - 余白と大きさに一定のリズムを持たせ、どの画面も同じ感覚で使える。
  - 選択中の項目は控えめなラインで示し、目に優しい配色で長時間でも疲れにくい。
- 📦 **ひとつのファイルで完結**：インストールも追加の部品も不要。どこへ置いてもすぐ使える。
- 🔤 **オリジナルフォント**：作者自作の NeoFont を同梱。

---

## ❱ 操作【Controls】

| キー | 動作 | キー | 動作 |
|---|---|---|---|
| `Ctrl+O` | 開く | `Ctrl+S` | 保存 |
| `Ctrl+T` | 新しいタブ | `Ctrl+W` | タブを閉じる |
| `Ctrl+F` | 検索 | `Ctrl+H` | 置換 |
| `Ctrl+G` | 指定行へ移動 | `Ctrl+E` | 文字コード / HEX |
| `Alt` | 左右のパネルを開閉 | `F11` | 全画面 |
| `Ctrl+ホイール` | 文字サイズ | `Ctrl+Tab` | タブの切り替え |

その他の操作は [docs/USAGE.md](docs/USAGE.md) を参照。

---

## ❱ 更新履歴【ChangeLog】

- **最新（v2.1.77）：** Minecraft の NBT をテキストとツリー形式で編集できるようにし、配色の追加と右パネルを加えた。
- **すべての履歴：** [changelog.md](changelog.md)
- **Discord：** [更新履歴チャンネル](https://discord.com/channels/1233036571243188296/1363604094895521944)

---

## ❱ 連繫【Link】

- **README：** [Discord](https://discord.com/channels/1233036571243188296/1360947282106777712/1360947282106777712)
- **Download：** [Releases](../../releases)
- **Usage：** [docs/USAGE.md](docs/USAGE.md)
- **Design：** [docs/DESIGN.md](docs/DESIGN.md)
- **Contributing：** [CONTRIBUTING.md](CONTRIBUTING.md)
