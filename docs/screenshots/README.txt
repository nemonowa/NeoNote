使い方の説明用の画面（Linux + Xvfb で撮影。ウィンドウ枠やタイトルバーは写っていない）。

history_window.png            View History（Undo の右クリック → View History）。開いた元のボタンが青くなり、上端に同じ色の帯が付く
history_tooltip_fullpath.png  行にマウスを乗せるとフルパスがツールチップで出る
history_window_few_entries.png 履歴が少ないときは小さく開く（幅 240〜560 px、高さは行数に合わせる）
gutter_hidden.png             行番号欄が隠れている状態（文字カーソルが行頭にもマウスが左端にもない）。本文が左へ詰まる
gutter_hover.png              マウスが左端にあると行番号欄と Pin ボタンが出る
gutter_pinned.png             Pin を押して固定した状態（ボタンが青。次回起動も引き継ぐ）

theme_dark.png / theme_light.png: ダークとライト（Run の右クリック → Toggle Theme で切り替え。行番号欄を固定した状態）。

compact_idle.png              ツールバーとタブと本文の間の隙間をなくした状態。検索欄は右端に常時表示（左に虫眼鏡）
compact_search_active.png     検索欄が使われると、真下に置換欄、右に ← → × と Repl / All が出る（編集領域に重ねるので縦は増えない）
compact_replace_done.png      All で置換したところ（余白に Replaced 1。見つからない検索は文字が赤）
compact_search_tooltip.png    件数などの状態は、検索欄のツールチップに出す
gutter_fit_digits.png         行番号欄の幅を行数の桁数に合わせた（左: 5 行 = 2 桁、右: 1200 行 = 4 桁）
mode_chip_menu.png            タブバー右端の表示モードのチップ（上: 通常 / ツールチップ / 一覧）。Preview は準備中で選べない
mode_chip_hex.png             バイナリのタブではチップが HEX になる

ctx_menu_dark.png / ctx_menu_light.png  右クリックメニューをパレットの色で出した状態（選択行はグレー系）。ライトは Toggle Theme の後
ctx_menu_checked.png          Word Wrap がオンのときのチェック表示
hscroll_overflow.png          はみ出す行（120 文字）があり、幅が足りないときだけ横スクロールバーが出る
hscroll_none.png              同じファイルでも、幅が足りていれば横スクロールバーは出ない（縦だけ）
readme_no_wm.png              ウィンドウマネージャーの無い Xvfb で、README がメインウィンドウの中央に出る
