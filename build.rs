// 簡易再生（`audio` feature）用に、miniaudio の薄い包み（vendor/miniaudio/neo_audio.c）を 1 ファイルだけ C でビルドする。
// feature を外したビルドでは何もしない。
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    windows_icon();
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

// Windows（MSVC）では assets/icon.ico を実行ファイルのリソースに埋め込む（エクスプローラーのアイコン）。
// rc.exe に頼らず .res を直接書いてリンカーへ渡す。窓のアイコンは別に `Window::set_icon` で付けている。
fn windows_icon() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let Ok(ico) = std::fs::read("assets/icon.ico") else { return };
    let u16_at = |i: usize| u16::from_le_bytes([ico[i], ico[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([ico[i], ico[i + 1], ico[i + 2], ico[i + 3]]);
    if ico.len() < 6 || u16_at(2) != 1 {
        return;
    }
    let n = u16_at(4) as usize;
    if ico.len() < 6 + 16 * n {
        return;
    }

    fn entry(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(32u32.to_le_bytes());
        for v in [0xFFFF, kind, 0xFFFF, id] {
            out.extend((v as u16).to_le_bytes());
        }
        out.extend(0u32.to_le_bytes()); // DataVersion
        out.extend(0x1030u16.to_le_bytes()); // MOVEABLE | PURE | DISCARDABLE
        out.extend(0x0409u16.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    let mut res = Vec::new();
    entry(&mut res, 0, 0, &[]); // .res の先頭に置く空の項目
    let mut group = Vec::new();
    group.extend([0u8, 0, 1, 0]);
    group.extend((n as u16).to_le_bytes());
    for i in 0..n {
        let d = 6 + 16 * i;
        let (size, off) = (u32_at(d + 8) as usize, u32_at(d + 12) as usize);
        let Some(img) = ico.get(off..off + size) else { return };
        entry(&mut res, 3, (i + 1) as u16, img);
        group.extend(&ico[d..d + 12]);
        group.extend(((i + 1) as u16).to_le_bytes());
    }
    entry(&mut res, 14, 1, &group);

    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icon.res");
    if std::fs::write(&out, res).is_ok() {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    }
}
