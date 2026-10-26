// 簡易再生（`audio` feature）用に、miniaudio の薄い包み（vendor/miniaudio/neo_audio.c）を 1 ファイルだけ C でビルドする。
// feature を外したビルドでは何もしない。
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_AUDIO").is_none() {
        return;
    }
    println!("cargo:rerun-if-changed=vendor/miniaudio/neo_audio.c");
    println!("cargo:rerun-if-changed=vendor/miniaudio/miniaudio.h");
    cc::Build::new()
        .file("vendor/miniaudio/neo_audio.c")
        .include("vendor/miniaudio")
        .opt_level(2)
        .warnings(false)
        .compile("neo_audio");
    // miniaudio は実行時に PulseAudio / ALSA（Linux）や各フレームワーク（macOS）を読み込む。リンクが要るのは基本の部品だけ
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        for l in ["dl", "pthread", "m"] {
            println!("cargo:rustc-link-lib={l}");
        }
    }
    // macOS は、実行時に読み込めなかった場合に備えて、フレームワークもリンクしておく（未確認）
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for f in ["CoreFoundation", "CoreAudio", "AudioToolbox"] {
            println!("cargo:rustc-link-lib=framework={f}");
        }
    }
}
