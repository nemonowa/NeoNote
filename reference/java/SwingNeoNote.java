// 命名：SwingNeoNote
// 説明：https://discord.com/channels/1233036571243188296/1360947282106777712/1360947282106777712
// 説明：
// 説明：[開発環境判定について] 本クラスは他ツールと依存を共有しない単体クラスとして運用
// 説明：しているため、neo.server.DevAccess とは判定条件を共有せず、このクラス内だけで
// 説明：完結させている（共通クラス化はしない設計判断。DevAccessのJavadoc参照）。
// 説明：当初は user.home（例：C:\Users\nemo）というWindows形式のフルパスを直接比較して
// 説明：いたが、OSごとにパス形式が異なり開発者自身の環境が変わると壊れる弱点があったため、
// 説明：OSに依存しない user.name（アカウント名部分のみ）を見る方式に変更した。
// 説明：[今後の拡張方針] 単一の決め打ち条件では共同開発者・貸与機・CI環境等に対応しきれ
// 説明：ないケースが出てきた場合は、将来的に制作者が管理する配布先（GitHub Raw等）へ
// 説明：ホワイトリストJSONを置き、起動時に取得して照合する方式へ拡張することも視野に
// 説明：入れている。ただし判定条件そのものはREADME等に文書化せず配布物から読み取れない
// 説明：状態を維持すること（過去に検討した -debug 起動引数方式は「引数を知ってさえいれば
// 説明：誰でも使える」弱点があり不採用とした経緯がある）。
// >
// =neo/tool/SwingNeoNote
package neo.tool;

import neo.util.Log;
import java.awt.*;
import java.awt.datatransfer.*;
import java.awt.dnd.*;
import java.awt.event.*;
import java.io.*;
import java.net.URI;
import java.net.URLEncoder;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import javax.swing.*;
import javax.swing.event.DocumentEvent;
import javax.swing.event.DocumentListener;
import javax.swing.text.BadLocationException;
import javax.swing.text.DefaultHighlighter;
import javax.swing.text.Highlighter;
import javax.swing.undo.*;
import java.util.List;
import java.util.ArrayList;

public class SwingNeoNote extends JFrame {
  private static final long serialVersionUID = 1L;
  public static final String TITLE = "SwingNeoNote"; // 書き換え禁止固定値外部からアクセス可能
  private static String version = "v1.1.47"; // 参照するならLog.info("SwingNeoNote", player.version);
  private JTextPane textPane;
  private JTextArea lineNumbers;
  private JFileChooser fileChooser;
  private boolean isBinaryMode = false;
  private String encoding = "UTF-8";
  private File currentFile = null;
  private boolean isModified = false;
  private UndoManager undoManager;

  // === Color Theme ===
  private static final Color COLOR_MAIN = new Color(43, 43, 43);
  private static final Color COLOR_SUB = new Color(30, 30, 30);
  private static final Color COLOR_TEXT = new Color(128, 128, 128);
  private static final Color COLOR_ACCENT = new Color(0, 120, 215);

  /**
   * ウィジェットグループ単位の initXxx() に分けて組み立てる。textPane/lineNumbers/fileChooser/
   * undoManager/isModified はインスタンスフィールドなので、各 initXxx() 内で完結する。
   * 順序の注意：initTextAreas() を initFrameAndFont() より先に呼ぶこと。逆だと、customFont を
   * 適用する時点で textPane/lineNumbers が null で NullPointerException になる
   * （catch(Exception) で飲み込まれ、カスタムフォントが黙って無効になる）。
   * それ以外の順序は フレーム設定→テキストエリア→右クリックメニュー→D&D/スクロール→ツールバー→ウィンドウ終了処理。
   */
  public SwingNeoNote() {
    initTextAreas();
    initFrameAndFont();
    JPopupMenu contextMenu = buildContextMenu();
    textPane.setComponentPopupMenu(contextMenu);
    undoManager = new UndoManager();
    textPane.getDocument().addUndoableEditListener(undoManager);
    initDropAndScrollLayout();
    initToolBar();
    initWindowChrome();
    title();
  }

  /** 開発者判定・ウィンドウサイズ/アイコン・カスタムフォント/ツールチップUIManager設定。 */
  private void initFrameAndFont() {
    // 開発者アカウント名の決め打ち条件。user.home（OSごとにフォーマットが異なる
    // フルパス）ではなく、OSに依存しない user.name（アカウント名部分のみ）を見る。
    final String DEV_USER_NAME = "nemo";
    String userName = System.getProperty("user.name");
    if (userName != null && DEV_USER_NAME.equalsIgnoreCase(userName)) {
      version = "v999.99.9";
      Log.info("alert", "開発環境のためスキップ");
    }
    setSize(800, 800);
    setLocationRelativeTo(null);
    // アイコンの読み込み（存在しない場合はスキップ）
    try {
      java.net.URL iconUrl = getClass().getResource("/icon.png");
      if (iconUrl != null) {
        setIconImage(new ImageIcon(iconUrl).getImage());
      }
    } catch (Exception e) {
    }

    // カスタムフォントの指定（リソースから読み込み）
    try (InputStream fontStream = getClass()
        .getResourceAsStream("/source/font.ttf")) {
      if (fontStream != null) {
        Font customFont = Font.createFont(Font.TRUETYPE_FONT, fontStream);
        textPane.setFont(customFont.deriveFont(Font.PLAIN, 16f));
        lineNumbers.setFont(customFont.deriveFont(Font.PLAIN, 16f));
        UIManager.put("ToolTip.background", COLOR_SUB);
        UIManager.put("ToolTip.foreground", COLOR_MAIN);
        UIManager.put("ToolTip.border",
            BorderFactory.createLineBorder(COLOR_ACCENT));

        UIManager.put("ToolTip.font", customFont.deriveFont(Font.PLAIN, 24f));
      }
    } catch (Exception e) {
    }
  }

  /** テキスト編集エリア・行番号エリア・ツールチップUIManager設定。 */
  private void initTextAreas() {
    // テキスト編集エリアの設定
    textPane = new JTextPane();
    textPane.setFont(textPane.getFont().deriveFont(16f));
    textPane.setBackground(COLOR_MAIN);
    textPane.setForeground(COLOR_TEXT);
    textPane.setCaretColor(COLOR_ACCENT);

    // 行番号表示エリアの設定
    lineNumbers = new JTextArea("1\n");
    lineNumbers.setEditable(false);
    lineNumbers.setFont(lineNumbers.getFont().deriveFont(16f));
    lineNumbers.setBackground(COLOR_SUB);
    lineNumbers.setForeground(COLOR_TEXT);
    lineNumbers.setMargin(new Insets(3, 5, 0, 5));

    // ツールチップの設定
    UIManager.put("ToolTip.font", textPane.getFont().deriveFont(24f));
    ToolTipManager.sharedInstance().setInitialDelay(0); // 0ミリ秒で即表示
    ToolTipManager.sharedInstance().setDismissDelay(60000); // 1分後に消える

    UIManager.put("Menu.font", new Font("Arial", Font.PLAIN, 24));
    UIManager.put("MenuItem.font", new Font("Arial", Font.PLAIN, 24));
    UIManager.put("PopupMenu.font", new Font("Arial", Font.PLAIN, 24));
  }

  /** 右クリックコンテキストメニュー（Cut/Copy/Paste/Replace/Search/Web/Select All/Run In CMD）の組み立て。 */
  private JPopupMenu buildContextMenu() {
    // 右クリックメニューの追加
    JPopupMenu contextMenu = new JPopupMenu();
    JMenuItem cut = new JMenuItem("Cut");
    JMenuItem copy = new JMenuItem("Copy");
    JMenuItem paste = new JMenuItem("Paste");
    JMenuItem replace = new JMenuItem("Replace");
    JMenuItem search = new JMenuItem("Search");
    JMenuItem web = new JMenuItem("Search In web");
    JMenuItem all = new JMenuItem("Select All");
    JMenuItem cmd = new JMenuItem("Run In CMD");

    // 基本操作
    cut.addActionListener(e -> textPane.cut());
    copy.addActionListener(e -> textPane.copy());
    paste.addActionListener(e -> textPane.paste());
    all.addActionListener(e -> textPane.selectAll());

    // 編集操作
    replace.addActionListener(e -> {
      String target = textPane.getSelectedText();
      if (target == null || target.isEmpty()) {
        JOptionPane.showMessageDialog(null,
            "First select the string you want to replace", "Info",
            JOptionPane.INFORMATION_MESSAGE);
        return;
      }

      String replaceWith = JOptionPane.showInputDialog(null, target,
          "Replace All", JOptionPane.PLAIN_MESSAGE);
      if (replaceWith == null)
        return;

      try {
        var doc = textPane.getDocument();
        String content = doc.getText(0, doc.getLength());

        int index = 0;
        while ((index = content.indexOf(target, index)) >= 0) {
          doc.remove(index, target.length());
          doc.insertString(index, replaceWith, null);

          // 次の検索位置を調整
          index += replaceWith.length();
          content = doc.getText(0, doc.getLength());
        }
      } catch (BadLocationException ex) {
        neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
      }
    });

    // 「検索」メニューアイテムの追加
    search.addActionListener(e -> {
      String selected = textPane.getSelectedText(); // 選択されたテキストを取得

      if (selected != null && !selected.isEmpty()) {
        try {
          String searchText = selected.trim(); // 余分な空白を取り除く
          Highlighter highlighter = textPane.getHighlighter();
          Highlighter.HighlightPainter painter = new DefaultHighlighter.DefaultHighlightPainter(
              Color.YELLOW);

          highlighter.removeAllHighlights(); // 既存のハイライトを削除

          String content = textPane.getText(); // テキスト全体を取得
          int index = content.indexOf(searchText); // 最初の一致位置を検索

          // 一致箇所すべてをハイライト
          while (index >= 0) {
            highlighter.addHighlight(index, index + searchText.length(),
                painter);
            index = content.indexOf(searchText, index + searchText.length()); // 次の一致を検索
          }

        } catch (BadLocationException ex) {
          neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
          JOptionPane.showMessageDialog(null, "Error while highlighting text",
              "Error", JOptionPane.ERROR_MESSAGE);
        }
      }
    });

    web.addActionListener(e -> {
      String selected = textPane.getSelectedText();
      String urlToOpen = "https://nemonowa.github.io/"; // デフォルト

      try {
        if (selected != null && !selected.isEmpty()) {
          selected = selected.trim();
          if (selected.startsWith("http")) {
            urlToOpen = selected; // そのまま開く
          } else {
            // Google検索用 URL に変換
            String query = URLEncoder.encode(selected, "UTF-8");
            urlToOpen = "https://www.google.com/search?q=" + query;
          }
        }

        // Desktop.browse が使えるか確認して実行
        if (Desktop.isDesktopSupported()
            && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
          Desktop.getDesktop().browse(new URI(urlToOpen));
        } else {
          JOptionPane.showMessageDialog(null,
              "Desktop browse is not supported.", "Error",
              JOptionPane.ERROR_MESSAGE);
        }

      } catch (Exception ex) {
        // ここまでで何か失敗した場合はホームページを開く
        try {
          if (Desktop.isDesktopSupported()
              && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
            Desktop.getDesktop().browse(new URI("https://nemonowa.github.io/"));
          }
        } catch (Exception ignore) {
        }
      }
    });

    cmd.addActionListener(e -> {
      String selected = textPane.getSelectedText();

      try {
        List<String> command = new ArrayList<>();
        String os = System.getProperty("os.name").toLowerCase();

        if (selected != null && !selected.isEmpty()) {
          // 選択テキストをコマンドとして実行
          if (os.contains("win")) {
            command.add("cmd");
            command.add("/c");
            command.add(selected);
          } else {
            command.add("/bin/sh");
            command.add("-c");
            command.add(selected);
          }
        } else {
          // カレントディレクトリを開く
          String currentPath = System.getProperty("user.dir");

          if (os.contains("win")) {
            command.add("explorer");
            command.add(currentPath);
          } else if (os.contains("mac")) {
            command.add("open");
            command.add(currentPath);
          } else {
            command.add("xdg-open");
            command.add(currentPath);
          }
        }

        ProcessBuilder pb = new ProcessBuilder(command);
        pb.redirectErrorStream(true);
        Process process = pb.start();

        StringBuilder output = new StringBuilder();
        String line;

        // 読み込み途中の例外でもプロセス出力ストリームを閉じる。
        try (BufferedReader reader = new BufferedReader(
            new InputStreamReader(process.getInputStream()))) {
          while ((line = reader.readLine()) != null) {
            output.append(line).append("\n");
          }
        }

        process.waitFor();

        JOptionPane.showMessageDialog(null, output.toString(), "実行結果",
            JOptionPane.INFORMATION_MESSAGE);

      } catch (IOException | InterruptedException ex) {
        neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
        JOptionPane.showMessageDialog(null,
            "NeoErrorCode:250 | FallbackCrash - コマンド実行中に例外が発生しました: " + ex.getMessage(),
            "Error", JOptionPane.ERROR_MESSAGE);
      }
    });

    contextMenu.add(cut);
    contextMenu.add(copy);
    contextMenu.add(paste);
    contextMenu.add(replace);
    contextMenu.addSeparator();
    contextMenu.add(search);
    contextMenu.add(web);
    contextMenu.addSeparator();
    contextMenu.add(all);
    contextMenu.addSeparator();
    contextMenu.add(cmd);
    return contextMenu;
  }

  /** ドラッグ&ドロップ受付・スクロールペイン・行番号更新リスナーのレイアウト組み込み。 */
  private void initDropAndScrollLayout() {
    DropTargetAdapter dtl = new DropTargetAdapter() {
      @SuppressWarnings("unchecked")
      public void drop(DropTargetDropEvent evt) {
        try {
          evt.acceptDrop(DnDConstants.ACTION_COPY);
          java.util.List<File> droppedFiles = (java.util.List<File>) evt
              .getTransferable().getTransferData(DataFlavor.javaFileListFlavor);
          if (!droppedFiles.isEmpty()) {
            File file = droppedFiles.get(0);
            load(file);
          }
        } catch (Exception ex) {
          neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
        }
      }
    };
    new DropTarget(textPane, dtl);

    JScrollPane scrollPane = new JScrollPane(textPane);
    scrollPane.getVerticalScrollBar().setBackground(COLOR_SUB);
    scrollPane.setRowHeaderView(lineNumbers);
    scrollPane.getViewport().setBackground(COLOR_MAIN);

    // 行番号更新
    DocumentListener listener = new DocumentListener() {
      public void insertUpdate(DocumentEvent e) {
        isModified = true;
        lines();
      }

      public void removeUpdate(DocumentEvent e) {
        isModified = true;
        lines();
      }

      public void changedUpdate(DocumentEvent e) {
      }
    };
    textPane.getDocument().addDocumentListener(listener);

    add(scrollPane, BorderLayout.CENTER);
  }

  /** ツールバー（アイコンボタン群・右クリックプレビューメニュー・アクションリスナー）の組み立て。 */
  private void initToolBar() {
    JMenuBar toolBar = new JMenuBar();
    toolBar.setBackground(COLOR_SUB);
    toolBar.setToolTipText(
        "<html><B>ToolBar</B><br>(RMB) 現在のパスをコピー<br>(LMB) ツールバーをカスタマイズ</html>");
    toolBar.addMouseListener(new MouseAdapter() {
      @Override
      public void mouseClicked(MouseEvent e) {
        path();
      }
    });

    JButton[] buttons = {button("📂", "Load"), button("💾", "Save"),
        button("↩️", "Undo"), button("↪️", "Redo"), button("⚓", "Edit"),
        button("📋", "Copy"), button("📑", "Paste"), button("▶️", "Run")};

    for (JButton btn : buttons) {
      btn.addMouseListener(new MouseAdapter() {
        @Override
        public void mousePressed(MouseEvent e) {
          // 右クリック（コンテキストメニュー）
          if (SwingUtilities.isRightMouseButton(e)) {
            String command = btn.getToolTipText();
            JPopupMenu popup = new JPopupMenu();

            switch (command) {
              case "Load" :
                popup.add(new JMenuItem("Open..."));
                popup.addSeparator();
                popup.add(new JMenuItem("Open Recent"));
                popup.add(new JMenuItem("Open README") {
                  private static final long serialVersionUID = -4300879614999092447L;
                  {
                    addActionListener(e -> read("/source/README"));
                  }
                });
                popup.add(new JMenuItem("Open HomePage"));
                popup.add(new JMenuItem("Open Settings") {
                  private static final long serialVersionUID = 6476272661605008021L;

                  {
                    addActionListener(e -> font());
                  }
                });
                popup.add(new JMenuItem("Make Shortcut"));
                popup.add(new JMenuItem("Check Update") {
                  private static final long serialVersionUID = 7818377586003842361L;

                  {
                    addActionListener(e -> JOptionPane.showMessageDialog(null,
                        "Latest", "Info", JOptionPane.INFORMATION_MESSAGE));
                  }
                });

                popup.addSeparator();
                popup.add(new JMenuItem("Exit") {
                  private static final long serialVersionUID = 820536590627181691L;

                  {
                    addActionListener(e -> {
                      dispose();
                    });
                  }
                });
                break;
              case "Save" :
                popup.add(new JMenuItem("Save"));
                popup.addSeparator();
                popup.add(new JMenuItem("Save As..."));
                popup.add(new JMenuItem("Enable Auto Save"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Undo" :
                popup.add(new JMenuItem("Undo"));
                popup.addSeparator();
                popup.add(new JMenuItem("Undo Last"));
                popup.add(new JMenuItem("Undo All"));
                popup.add(new JMenuItem("View History"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Redo" :
                popup.add(new JMenuItem("Redo"));
                popup.addSeparator();
                popup.add(new JMenuItem("Redo Last"));
                popup.add(new JMenuItem("Redo All"));
                popup.add(new JMenuItem("Restore From History"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Edit" :
                popup.add(new JMenuItem("Select"));
                popup.addSeparator();
                popup.add(new JMenuItem("Select All"));
                popup.add(new JMenuItem("Magic Select"));
                popup.add(new JMenuItem("Deselect"));
                popup.add(new JMenuItem("Copy Selection"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Copy" :
                popup.add(new JMenuItem("Copy"));
                popup.addSeparator();
                popup.add(new JMenuItem("Copy Line"));
                popup.add(new JMenuItem("Copy to Clipboard History"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Paste" :
                popup.add(new JMenuItem("Paste"));
                popup.addSeparator();
                popup.add(new JMenuItem("Paste Before"));
                popup.add(new JMenuItem("Paste After"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "cmd" :
                popup.add(new JMenuItem("Choose Font"));
                popup.addSeparator();
                popup.add(new JMenuItem("Change Size"));
                popup.add(new JMenuItem("Toggle Style"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              case "Change Encoding" :
                popup.add(new JMenuItem("Encode"));
                popup.addSeparator();
                popup.add(new JMenuItem("Set to UTF-8"));
                popup.add(new JMenuItem("Set to Shift_JIS"));
                popup.add(new JMenuItem("Switch to Binary Mode"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
              default :
                popup.add(new JMenuItem("Undefined Menu"));
                popup.addSeparator();
                popup.add(new JMenuItem("Undefined Menu"));
                popup.addSeparator();
                popup.add(new JMenuItem("Cancel"));
                break;
            }
            popup.show(btn, e.getX(), e.getY());
          }
        }
      });
      btn.addActionListener(e -> action(e));
      toolBar.add(btn);
    }

    setJMenuBar(toolBar);
  }

  /** ファイル選択ダイアログの初期化と、未保存変更確認つきウィンドウクローズ処理。 */
  private void initWindowChrome() {
    fileChooser = new JFileChooser();
    fileChooser.setAcceptAllFileFilterUsed(true);

    setDefaultCloseOperation(JFrame.DO_NOTHING_ON_CLOSE);
    addWindowListener(new WindowAdapter() {
      @Override
      public void windowClosing(WindowEvent e) {
        try {
          if (isModified) {
            int result = JOptionPane.showConfirmDialog(SwingNeoNote.this,
                "Save or Delete. Save the world?", "Unsaved changes!",
                JOptionPane.YES_NO_OPTION, // キャンセルボタンは表示しない
                JOptionPane.WARNING_MESSAGE);

            if (result == JOptionPane.CLOSED_OPTION) {
              // ×ボタンが押された場合はキャンセル
              return;
            } else if (result == JOptionPane.YES_OPTION) {
              save(); // 保存
            }
            // NO_OPTION の場合は保存せず終了
          }
          // 変更なしの場合や NO/YES ならここで終了
          dispose();
        } catch (Exception ex) {
          neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
        }
      }
    });
  }

  private JButton button(String emoji, String tooltip) {
    JButton button = new JButton(emoji);
    button.setToolTipText(tooltip);
    button.setBackground(COLOR_MAIN);
    button.setForeground(COLOR_TEXT);
    button.setFocusPainted(false);
    return button;
  }

  // 以下は独立関数
  private void say(String... message) {
    String msg = (message.length > 0) ? message[0] : "エラー 404: アクセス権限なし";
    JOptionPane.showMessageDialog(this, msg, "通知",
        JOptionPane.INFORMATION_MESSAGE);
  }

  private void lines() {
    int lines = textPane.getDocument().getDefaultRootElement()
        .getElementCount();
    StringBuilder lineNumbersText = new StringBuilder();
    for (int i = 1; i <= lines; i++) {
      lineNumbersText.append(i).append("\n");
    }
    lineNumbers.setText(lineNumbersText.toString());
    title();
  }

  private void title() {
    String fileName = (currentFile != null)
        ? currentFile.getAbsolutePath()
        : "Unsaved File";
    int charCount = textPane.getText().length();
    setTitle("[" + version + "] " + TITLE + " - by nemo | Path: " + fileName
        + " | Words: " + charCount + " | Encoding: " + encoding);
  }

  private void path() {
    String contentToCopy = (currentFile != null)
        ? currentFile.getAbsolutePath()
        : "https://nemonowa.github.io/";
    StringSelection stringSelection = new StringSelection(contentToCopy);
    Clipboard clipboard = Toolkit.getDefaultToolkit().getSystemClipboard();
    clipboard.setContents(stringSelection, null);
    JOptionPane.showMessageDialog(this,
        "Path copied to clipboard: " + contentToCopy);
  }

  public void action(ActionEvent e) {
    JButton source = (JButton) e.getSource();
    String command = source.getToolTipText();
    switch (command) {
      case "Load" :
        open();
        break;
      case "Save" :
        save();
        break;
      case "Undo" :
        if (undoManager.canUndo()) {
          undoManager.undo();
        }
        break;
      case "Redo" :
        if (undoManager.canRedo()) {
          undoManager.redo();
        }
        break;
      case "Edit" :
        say();
        break;
      case "Copy" :
        textPane.copy();
        break;
      case "Paste" :
        textPane.paste();
        break;
      case "Run" :
        font();
        break;
      case "Change Encoding" :
        encode();
        break;
    }
  }

  // ツールバーの関数
  private void open() {
    open(null); // 引数なしで自動的にフォルダ選択
  }

  // ファイル選択UIはOSネイティブに
  public void open(String path) {
    if (path != null && !path.trim().isEmpty()) {
      File file = new File(path);
      if (file.exists()) {
        load(file);
        return;
      }

      JOptionPane.showMessageDialog(this,
          "File or resource not found:\n" + path, "Error",
          JOptionPane.ERROR_MESSAGE);
      return;
    }

    // ネイティブのファイル選択ダイアログ (Windows ならエクスプローラー)
    FileDialog dialog = new FileDialog(
        (Frame) SwingUtilities.getWindowAncestor(this), "Open",
        FileDialog.LOAD);
    // デフォルトを「ドキュメント」に設定
    String documents = System.getProperty("user.home") + File.separator
        + "Documents";
    dialog.setDirectory(documents);
    dialog.setVisible(true);

    String selected = dialog.getFile();
    if (selected != null) {
      File file = new File(dialog.getDirectory(), selected);
      load(file);
    }
  }

  // 内部リソース（ZIPなど）読み込み用
  private void read(String path) {
    try (InputStream in = getClass().getResourceAsStream(path)) {
      if (in != null) {
        String content = new String(in.readAllBytes(),
            Charset.forName(encoding));
        textPane.setText(content); // ← 読み込んだ内容を表示
        setTitle("Viewing: " + new File(path).getName()); // タイトル表示など（任意）
        currentFile = null;
        isModified = false;
        lines(); // 行番号表示更新など
        title(); // タイトルバー更新（既存メソッド）
      }
    } catch (IOException e) {
    }
  }

  private void save() {
    File fileToSave = currentFile;

    if (fileToSave == null) {
      FileDialog dialog = new FileDialog(this, "Save", FileDialog.SAVE);
      dialog.setVisible(true);

      String dir = dialog.getDirectory();
      String name = dialog.getFile();

      if (dir == null || name == null) {
        return; // キャンセル
      }

      fileToSave = new File(dir, name);
    }

    try {
      if (isBinaryMode) {
        Files.write(fileToSave.toPath(),
            textPane.getText().getBytes(StandardCharsets.UTF_8));
      } else {
        Files.write(fileToSave.toPath(),
            textPane.getText().getBytes(Charset.forName(encoding)));
      }
      currentFile = fileToSave;
      isModified = false;
      title();
    } catch (IOException ex) {
      JOptionPane.showMessageDialog(this, "ファイルを保存できませんでした。", "エラー",
          JOptionPane.ERROR_MESSAGE);
    }
  }

  private void load(File file) {
    setTitle("Loading... " + (file != null ? file.getName() : ""));
    repaint();

    new SwingWorker<String, Void>() {
      @Override
      protected String doInBackground() throws Exception {
        byte[] data = Files.readAllBytes(file.toPath());
        return isBinaryMode
            ? hex(data)
            : new String(data, Charset.forName(encoding));
      }

      @Override
      protected void done() {
        try {
          String content = get();
          textPane.setText(content);
          lines(); // 行数やタイトル更新
          currentFile = file;
          isModified = false;
          title();
        } catch (Exception ex) {
          neo.util.Log.error("SwingNeoNote", "エラーが発生しました: " + ex.getMessage(), ex);
          JOptionPane.showMessageDialog(SwingNeoNote.this, "Failed to load file.",
              "Error", JOptionPane.ERROR_MESSAGE);
        }
      }
    }.execute();
  }

  private String hex(byte[] bytes) {
    StringBuilder sb = new StringBuilder();
    for (byte b : bytes) {
      sb.append(String.format("%02X ", b));
    }
    return sb.toString();
  }

  private void font() {
    // ユーザーフォントフォルダ
    String userFonts = System.getenv("USERPROFILE")
        + "\\AppData\\Local\\Microsoft\\Windows\\Fonts";

    FileDialog dialog = new FileDialog(
        (Frame) SwingUtilities.getWindowAncestor(this),
        "Select Font File (.ttf or .otf)", FileDialog.LOAD);
    dialog.setDirectory(userFonts);

    dialog.setVisible(true);

    String selected = dialog.getFile();
    if (selected == null)
      return;

    File fontFile = new File(dialog.getDirectory(), selected);

    try {
      Font baseFont = textPane.getFont();
      Font font = Font.createFont(Font.TRUETYPE_FONT, fontFile)
          .deriveFont(baseFont.getStyle(), baseFont.getSize());

      textPane.setFont(font);
      lineNumbers.setFont(font);
      title();

    } catch (Exception ex) {
      JOptionPane.showMessageDialog(this,
          "Failed to load the font. Make sure it's a valid font file.", "Error",
          JOptionPane.ERROR_MESSAGE);
    }
  }

  private void encode() {
    String[] options = {"UTF-8", "Shift_JIS", "ISO-8859-1", "US-ASCII",
        "HEX (Binary Mode)"};
    String selected = (String) JOptionPane.showInputDialog(this,
        "Select Encoding:", "Encoding Mode", JOptionPane.PLAIN_MESSAGE, null,
        options, encoding);
    if (selected != null) {
      if (selected.equals("HEX (Binary Mode)")) {
        isBinaryMode = true;
      } else {
        isBinaryMode = false;
        encoding = selected;
      }
      if (currentFile != null) {
        load(currentFile);
      }
      title();
    }
  }

  public static void main(String[] args) {
    SwingUtilities.invokeLater(() -> {
      SwingNeoNote editor = new SwingNeoNote();
      editor.setVisible(true);
    });
  }
}