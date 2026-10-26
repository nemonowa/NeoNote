// 命名：Log
// 説明：プロジェクト全体で使う極小の統一ログ層。
// 説明：各所で System.out.println("[Tag] ...") / e.printStackTrace() をバラバラに呼ぶと、
// 説明：ログレベルの一括制御やタグでの絞り込みができない。
// 説明：この Log クラスは既存の "[Tag] メッセージ" という表記規約をそのまま TAG 引数として
// 説明：引き継げるようにしてあり、置き換えは機械的に行える
// 説明：（例: System.out.println("[NetServer] " + msg) → Log.info("NetServer", msg)）。
// 説明：ログレベルは優先度の高い順に以下のいずれかで決定する：
// 説明：  1. 起動引数 -log-level <LEVEL>（Log.init(args) を起動直後に呼んだ場合）
// 説明：  2. mod/log.properties の log.level=<LEVEL>
// 説明：  3. VM引数 -Dneo.log.level / 環境変数 NEO_LOG_LEVEL
// 説明：  4. 既定値 INFO
// 説明：java.util.logging は起動が重く設定も煩雑なため使わず、自前の小さな実装で済ませる。
// >
// =neo/util/Log
package neo.util;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Properties;

public final class Log {

  public enum Level { DEBUG, INFO, WARN, ERROR, NONE }

  private static volatile Level minLevel = resolveMinLevel();

  private Log() {}

  /**
   * ログ表示用に、フルパス／フルURLから末尾のファイル名部分だけを取り出す。
   * Windowsのフルパス（例：C:\Users\...\entity\foo.bbmodel）や長いURLを
   * そのままログに出すと可読性を著しく損なう（admin/skill.md参照）ため、表示専用に
   * このヘルパーへ一本化した。'/'・'\\'どちらの区切りにも対応し、区切りが無ければ
   * そのまま返す。**あくまでログ表示専用**——実処理（ファイルI/O・キャッシュキー・
   * activeSources等の識別子）には絶対にこの戻り値を使わないこと（同名ファイルが
   * 別フォルダに存在すると衝突するため、識別子としての一意性は保証されない）。
   */
  public static String fileName(String pathOrUrl) {
    if (pathOrUrl == null) return null;
    int cut = Math.max(pathOrUrl.lastIndexOf('/'), pathOrUrl.lastIndexOf('\\'));
    return (cut >= 0) ? pathOrUrl.substring(cut + 1) : pathOrUrl;
  }

  @LaunchArg(params = "<レベル>", choices = {"DEBUG", "INFO", "WARN", "ERROR"}, help = "ログの出力レベル")
  public static final String LOG_LEVEL = "-log-level";

  /** 起動時（main の先頭）で一度だけ呼ぶこと。-log-level 起動引数を反映する。 */
  public static void init(String[] args) {
    for (int i = 0; i < args.length; i++) {
      if (LaunchArgs.is(LOG_LEVEL, args[i]) && i + 1 < args.length) {
        setMinLevel(parse(args[i + 1]));
        return;
      }
    }
  }

  private static Level resolveMinLevel() {
    Level fromFile = readPropertiesFile();
    if (fromFile != null) return fromFile;

    String v = System.getProperty("neo.log.level", System.getenv("NEO_LOG_LEVEL"));
    return (v != null) ? parse(v) : Level.INFO;
  }

  private static Level readPropertiesFile() {
    Path path = java.nio.file.Paths.get("mod", "log.properties");
    if (!Files.isReadable(path)) return null;
    try (InputStream in = Files.newInputStream(path)) {
      Properties p = new Properties();
      p.load(in);
      String v = p.getProperty("log.level");
      return (v != null) ? parse(v) : null;
    } catch (IOException ex) {
      return null;
    }
  }

  private static Level parse(String v) {
    try {
      return Level.valueOf(v.trim().toUpperCase());
    } catch (IllegalArgumentException ex) {
      return Level.INFO;
    }
  }

  /** テストやツールから実行時にログレベルを変更したい場合に使う。 */
  public static void setMinLevel(Level level) {
    minLevel = (level != null) ? level : Level.INFO;
  }

  /** DEBUGログが出力される設定か。ホットパスで、ログ本文の組み立て（文字列連結・診断用オブジェクトの生成）ごと省くための判定。 */
  public static boolean isDebugEnabled() {
    return minLevel.ordinal() <= Level.DEBUG.ordinal();
  }

  public static void debug(String tag, String message) {
    log(Level.DEBUG, tag, message, null);
  }

  public static void info(String tag, String message) {
    log(Level.INFO, tag, message, null);
  }

  public static void warn(String tag, String message) {
    log(Level.WARN, tag, message, null);
  }

  public static void warn(String tag, String message, Throwable t) {
    log(Level.WARN, tag, message, t);
  }

  public static void error(String tag, String message) {
    log(Level.ERROR, tag, message, null);
  }

  public static void error(String tag, String message, Throwable t) {
    log(Level.ERROR, tag, message, t);
  }

  private static void log(Level level, String tag, String message, Throwable t) {
    if (level.ordinal() < minLevel.ordinal()) return;

    java.io.PrintStream out = (level == Level.WARN || level == Level.ERROR)
        ? System.err : System.out;

    String prefix = (tag != null && !tag.isEmpty()) ? "[" + tag + "] " : "";
    out.println(levelTag(level) + prefix + message);

    if (t != null) {
      t.printStackTrace(out);
    }
  }

  private static String levelTag(Level level) {
    switch (level) {
      case DEBUG: return "";      // 既存表記との互換を優先し、DEBUG/INFOは接頭辞を付けない
      case INFO:  return "";
      case WARN:  return "[WARN] ";
      case ERROR: return "[ERROR] ";
      default:    return "";
    }
  }
}
