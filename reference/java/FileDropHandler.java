// 命名：FileDropHandler
// >
// =neo/client/screen/FileDropHandler
package neo.client.screen;

import neo.util.Log;
import org.lwjgl.glfw.GLFWDropCallback;
import org.lwjgl.glfw.GLFWDropCallbackI;

import neo.tool.SwingNeoNote;

import java.util.Locale;
import javax.swing.SwingUtilities;

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
  public static void open(String path) {
    // macOSでは -XstartOnFirstThread 付きでJVMが起動されるため、
    // SwingUtilities.invokeLater()経由のAWT/Swing呼び出しはクラッシュする
    // この機能自体をmacOS上では無効化する。
    if (neo.util.PlatformInfo.IS_MAC) {
      Log.info("Drag&Drop", "macOS非対応のためテキストエディタ起動をスキップしました: " + path);
      // 他のmacOS非対応案内と同じく postSystem() に統一する。
      neo.client.screen.NeoChat.postSystem(
          neo.util.Lang.tr("gui.drop_open_mac_unsupported",
              "この機能はmacOS非対応です（テキストエディタ起動不可）。"));
      return;
    }
    SwingUtilities.invokeLater(() -> {
      SwingNeoNote editor = new SwingNeoNote();
      editor.setVisible(true);
      editor.open(path); // インスタンスメソッド
    });
  }

  // 未知の拡張子
  private static void onUnknown(String path) {
    Log.info("Drag&Drop", "[UNKNOWN] " + path);
  }
}
