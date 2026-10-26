// 命名：FileDropHandler
// >
// =neo/client/screen/FileDropHandler
package neo.client.screen;

import neo.util.Log;
import org.lwjgl.glfw.GLFWDropCallback;
import org.lwjgl.glfw.GLFWDropCallbackI;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Locale;

// ドラッグ＆ドロップコールバック設定クラス
public class FileDropHandler {

  // runメソッドでGLFWDropCallbackIインスタンスを返す
  public static GLFWDropCallbackI run(long window) {

    return (win, count, names) -> {
      for (int i = 0; i < count; i++) {
        String path = GLFWDropCallback.getName(names, i);
        Log.info("Drag&Drop", path);
        String lower = path.toLowerCase(Locale.ROOT);
        if (lower.endsWith(".txt") || lower.endsWith(".md")
            || lower.endsWith(".json")) {
          open(path);
        } else if (lower.endsWith(".mca")) {
          neo.client.DrawRegion.load(path);
        }

        else if (lower.endsWith(".obj")) {
          neo.client.ObjViewer2 viewer = new neo.client.ObjViewer2();
          viewer.run(); // 別ウィンドウで起動
        } else {
          onUnknown(path);
        }
      }
    };
  }

  // 外部起動用（Drag & Drop 等）
  // Swing の SwingNeoNote ではなく、別プロセスの NeoNote（Rust 製）を `neonote <パス>` で起動する。
  // ドロップごとに新しいプロセス・新しいウィンドウになる。Swing を使わないので、macOS の
  // -XstartOnFirstThread 問題（以前の IS_MAC ガード）も、EDT 経由の invokeLater も不要。
  // 実行ファイルの場所: -Dneo.neonote.path=<パス> → 環境変数 NEONOTE_PATH → mod/neonote(.exe) → PATH 上の neonote
  public static void open(String path) {
    // 先頭が '-' のパスをオプションと取り違えないよう、必ず絶対パスにして渡す
    String abs = Paths.get(path).toAbsolutePath().toString();
    String exe = resolveNeoNote();
    try {
      ProcessBuilder pb = new ProcessBuilder(exe, abs);
      // NeoNote のログ（[Tag] msg / [WARN] ...）をゲームの出力にそのまま流す。
      // 作業フォルダと環境変数（NEO_LOG_LEVEL）はそのまま引き継ぐので mod/log.properties も効く。
      pb.redirectOutput(ProcessBuilder.Redirect.INHERIT);
      pb.redirectError(ProcessBuilder.Redirect.INHERIT);
      Process p = pb.start();
      p.getOutputStream().close(); // 標準入力は使わない
      Log.info("Drag&Drop", "NeoNote 起動: " + Log.fileName(abs));
    } catch (java.io.IOException e) {
      Log.error("Drag&Drop", "NeoNote を起動できませんでした: " + exe, e);
      neo.client.screen.NeoChat.postSystem(
          neo.util.Lang.tr("gui.drop_open_failed",
              "テキストエディタ（NeoNote）を起動できませんでした: ") + exe);
    }
  }

  // NeoNote の実行ファイルを探す。見つからなければ PATH に任せる名前をそのまま返す。
  static String resolveNeoNote() {
    String configured = System.getProperty("neo.neonote.path", System.getenv("NEONOTE_PATH"));
    if (configured != null && !configured.trim().isEmpty()) {
      return configured.trim();
    }
    boolean windows = System.getProperty("os.name", "").toLowerCase(Locale.ROOT).contains("win");
    String name = windows ? "neonote.exe" : "neonote";
    Path bundled = Paths.get("mod", name);
    return Files.isRegularFile(bundled) ? bundled.toAbsolutePath().toString() : name;
  }

  // 未知の拡張子
  private static void onUnknown(String path) {
    Log.info("Drag&Drop", "[UNKNOWN] " + path);
  }
}
