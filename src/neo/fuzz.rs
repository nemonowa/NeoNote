// 壊れた入力で落ちないことの確かめ（テストだけ）。ファイルの中身を読む処理（tar・gz・zip・mca・NBT・音声・画像・
// 表・文書・PDF・inflate）へ、乱数と「先頭の印だけ正しい乱数」と「正しいデータを 1〜8 バイト壊した物」を渡し、
// panic（範囲外・桁あふれ・巨大な確保）が起きないことを見る。結果（Ok / Err）は問わない。
#![cfg(test)]

use super::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// 正しいデータの 1〜8 か所を壊した写し。
fn mutate(r: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    if v.is_empty() {
        return v;
    }
    for _ in 0..1 + r.next() % 8 {
        let i = (r.next() as usize) % v.len();
        v[i] = match r.next() % 4 {
            0 => 0xFF,
            1 => 0,
            _ => r.next() as u8,
        };
    }
    if r.next().is_multiple_of(4) {
        let cut = (r.next() as usize) % v.len();
        v.truncate(cut); // 途中で切れたファイル
    }
    v
}

/// 入力の組を作る: 乱数、印 + 乱数、正しいデータを壊した物。
fn inputs(seed: u64, magics: &[&[u8]], valid: &[Vec<u8>], n: usize) -> Vec<Vec<u8>> {
    let mut r = Rng(seed);
    let mut out = Vec::new();
    for k in 0..n {
        let len = (r.next() % 4096) as usize;
        let mut v = r.bytes(len);
        if !magics.is_empty() && k % 3 == 1 {
            let m = magics[k % magics.len()];
            let mut w = m.to_vec();
            w.extend_from_slice(&v);
            v = w;
        }
        if !valid.is_empty() && k % 3 == 2 {
            v = mutate(&mut r, &valid[k % valid.len()]);
        }
        out.push(v);
    }
    out
}

/// `f` を全部の入力で呼び、panic した入力の番号を返す。
fn run(name: &str, data: &[Vec<u8>], f: impl Fn(&[u8]) + std::panic::RefUnwindSafe) -> Vec<String> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut bad = Vec::new();
    for (i, d) in data.iter().enumerate() {
        if std::panic::catch_unwind(|| f(d)).is_err() {
            bad.push(format!("{name}#{i} (len {})", d.len()));
        }
    }
    std::panic::set_hook(prev);
    bad
}

#[test]
fn parsers_do_not_panic_on_broken_input() {
    let text = b"hello world, hello neonote\n".repeat(20);
    let gz = deflate::gzip(&text);
    let zl = deflate::zlib(&text);
    let mut bad = Vec::new();
    let n = 600;

    bad.extend(run("inflate", &inputs(1, &[], &[deflate::deflate(&text)], n), |b| {
        let _ = inflate::inflate(b, 1 << 20);
    }));
    bad.extend(run("gunzip", &inputs(2, &[b"\x1f\x8b\x08"], std::slice::from_ref(&gz), n), |b| {
        let _ = deflate::gunzip(b, 1 << 20);
    }));
    bad.extend(run("unzlib", &inputs(3, &[b"\x78\x9c"], std::slice::from_ref(&zl), n), |b| {
        let _ = deflate::unzlib(b, 1 << 20);
    }));
    bad.extend(run("tar", &inputs(4, &[], &[], n), |b| {
        let _ = tarlist::list_tar(b);
    }));
    bad.extend(run("tar.gz", &inputs(5, &[b"\x1f\x8b\x08"], std::slice::from_ref(&gz), n), |b| {
        let _ = tarlist::list_gz(b, "x");
    }));
    bad.extend(run("zip", &inputs(6, &[b"PK\x03\x04", b"PK\x05\x06"], &[], n), |b| {
        let _ = archive::list_zip(b);
        let _ = archive::read_entry(b, "a.txt", 1 << 20);
        let _ = archive::each_zip_file(b, 1 << 20, |_, _| true);
    }));
    bad.extend(run("mca", &inputs(7, &[], &[], n / 3), |b| {
        // 区画の表（8 KiB）を持つ大きさにして、表の中身を乱数にする
        let mut v = b.to_vec();
        v.resize(8192 + b.len(), 0);
        if let Ok(list) = region::list(&v) {
            for e in list.iter().take(8) {
                let _ = region::read_chunk(&v, e);
            }
        }
    }));
    bad.extend(run("nbt", &inputs(8, &[b"\x0a\x00\x00", b"\x1f\x8b\x08"], std::slice::from_ref(&gz), n), |b| {
        let _ = nbt::open(b);
        let _ = nbt::read(b);
    }));
    bad.extend(run("audio", &inputs(9, &[b"ID3\x03\x00", b"ID3\x04\x00", b"fLaC", b"RIFF\x00\x00\x00\x00WAVE"], &[], n), |b| {
        let _ = audiometa::info(b);
        let _ = audiometa::read(b);
        let _ = audiometa::write(b, "title=x");
    }));
    bad.extend(run("image", &inputs(10, &[b"\x89PNG\r\n\x1a\n", b"\xff\xd8\xff"], &[], n), |b| {
        let _ = imgmeta::read(b);
        let _ = imgmeta::write(b, "comment=x");
        let _ = imagefile::sniff(b);
    }));
    bad.extend(run("office", &inputs(12, &[b"PK\x03\x04"], &[], n / 3), |b| {
        let _ = book::read(b, false);
        let _ = book::read(b, true);
        for f in [doctext::Format::Docx, doctext::Format::Odt, doctext::Format::Pptx, doctext::Format::Odp, doctext::Format::Epub] {
            let _ = doctext::read(b, f);
        }
    }));
    bad.extend(run("pdf", &inputs(11, &[b"%PDF-1.4\n"], &[], n / 3), |b| {
        let _ = pdftext::extract(b);
    }));
    // UDF: 実物のイメージの、記述子のある所（先頭 300 セクター）を壊す。大きいので数を減らす
    for (seed, name) in [(14, "102"), (15, "250")] {
        let img = udffs::tests::fixture(name);
        let mut r = Rng(seed);
        let mut data = Vec::new();
        for _ in 0..n / 6 {
            let mut v = img.clone();
            for _ in 0..1 + r.next() % 8 {
                let i = (r.next() as usize) % (300 * 2048).min(v.len());
                v[i] = r.next() as u8;
            }
            if r.next().is_multiple_of(5) {
                v.truncate((r.next() as usize) % v.len());
            }
            data.push(v);
        }
        bad.extend(run(&format!("udf{name}"), &data, |b| {
            let _ = udffs::list(std::io::Cursor::new(b));
            let _ = udffs::read_path(std::io::Cursor::new(b), "資料/data.bin", 1 << 20);
        }));
    }
    assert!(bad.is_empty(), "panicked on: {}", bad.join(", "));
}
