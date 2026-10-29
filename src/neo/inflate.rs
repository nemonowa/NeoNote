// deflate（RFC 1951）の解凍（GUI なし・依存なし）。zip の中身（xlsx / ods の xml）を読むためだけに使う。
// 作りは zlib 付属の puff.c と同じ（ビットを 1 つずつ読む正準ハフマン復号）。速さより、短さと正しさを優先した。
// 圧縮はしない。zlib / gzip のヘッダは扱わない（zip の中身は生の deflate）。

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// 動的ハフマンで、符号長の符号の長さが並ぶ順。
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

type R<T> = Result<T, &'static str>;

/// 下位ビットから読むビット列。1 回の `bits` が終わった時点で、持ち越しのビットは常に 8 未満（今読んでいるバイトの残りだけ）。
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    n: u32,
}

impl Bits<'_> {
    fn bits(&mut self, need: u32) -> R<u32> {
        while self.n < need {
            let b = *self.d.get(self.pos).ok_or(crate::lang::tr("データが途中で切れています", "Data is truncated"))?;
            self.pos += 1;
            self.buf |= (b as u32) << self.n;
            self.n += 8;
        }
        let v = self.buf & ((1u32 << need) - 1);
        self.buf >>= need;
        self.n -= need;
        Ok(v)
    }

    fn decode(&mut self, h: &Huff) -> R<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= self.bits(1)? as i32;
            let count = h.count[len] as i32;
            if code - count < first {
                return Ok(h.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(crate::lang::tr("不正な符号です", "Invalid code"))
    }
}

/// 正準ハフマン符号。`count[len]` は長さ len の符号の数、`symbol` は符号の順に並べた記号。
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huff {
    /// 符号長（0 は使わない記号）から作る。符号が多すぎる（過剰に割り当てた）表は壊れているので断る。足りない表は許す。
    fn new(lengths: &[u8]) -> R<Huff> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut left: i32 = 1;
        for c in &count[1..] {
            left = (left << 1) - *c as i32;
            if left < 0 {
                return Err(crate::lang::tr("ハフマン表が壊れています", "Broken Huffman table"));
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huff { count, symbol })
    }
}

/// 記号を読んで、出力に書く。ブロックの終わり（256）で戻る。
fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Huff, dist: &Huff, max: usize) -> R<()> {
    loop {
        let sym = b.decode(lit)? as usize;
        if sym < 256 {
            if out.len() >= max {
                return Err(crate::lang::tr("展開後が大きすぎます", "Decompressed data is too large"));
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = sym - 257;
            if s >= 29 {
                return Err(crate::lang::tr("不正な長さの符号です", "Invalid length code"));
            }
            let len = LBASE[s] as usize + b.bits(LEXT[s] as u32)? as usize;
            let ds = b.decode(dist)? as usize;
            if ds >= 30 {
                return Err(crate::lang::tr("不正な距離の符号です", "Invalid distance code"));
            }
            let d = DBASE[ds] as usize + b.bits(DEXT[ds] as u32)? as usize;
            if d > out.len() {
                return Err(crate::lang::tr("距離が出力より前を指しています", "Distance points before the start of the output"));
            }
            if out.len() + len > max {
                return Err(crate::lang::tr("展開後が大きすぎます", "Decompressed data is too large"));
            }
            let start = out.len() - d;
            for i in 0..len {
                let c = out[start + i]; // 重なるコピー（d < len）もあるので 1 バイトずつ
                out.push(c);
            }
        }
    }
}

fn stored(b: &mut Bits, out: &mut Vec<u8>, max: usize) -> R<()> {
    b.buf = 0; // バイトの残りのビットは捨てる
    b.n = 0;
    let h = b.d.get(b.pos..b.pos + 4).ok_or(crate::lang::tr("データが途中で切れています", "Data is truncated"))?;
    let (len, nlen) = (u16::from_le_bytes([h[0], h[1]]), u16::from_le_bytes([h[2], h[3]]));
    if len != !nlen {
        return Err(crate::lang::tr("無圧縮ブロックの長さが壊れています", "Broken uncompressed block length"));
    }
    b.pos += 4;
    let len = len as usize;
    let data = b.d.get(b.pos..b.pos + len).ok_or(crate::lang::tr("データが途中で切れています", "Data is truncated"))?;
    if out.len() + len > max {
        return Err(crate::lang::tr("展開後が大きすぎます", "Decompressed data is too large"));
    }
    out.extend_from_slice(data);
    b.pos += len;
    Ok(())
}

fn dynamic(b: &mut Bits, out: &mut Vec<u8>, max: usize) -> R<()> {
    let nlen = b.bits(5)? as usize + 257;
    let ndist = b.bits(5)? as usize + 1;
    let ncode = b.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(crate::lang::tr("符号の数が不正です", "Invalid number of codes"));
    }
    let mut cl = [0u8; 19];
    for &o in CL_ORDER.iter().take(ncode) {
        cl[o] = b.bits(3)? as u8;
    }
    let clh = Huff::new(&cl)?;
    let total = nlen + ndist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = b.decode(&clh)?;
        if sym < 16 {
            lengths[i] = sym as u8;
            i += 1;
            continue;
        }
        let (val, rep) = match sym {
            16 => {
                if i == 0 {
                    return Err(crate::lang::tr("繰り返す符号長がありません", "No code length to repeat"));
                }
                (lengths[i - 1], 3 + b.bits(2)? as usize)
            }
            17 => (0, 3 + b.bits(3)? as usize),
            _ => (0, 11 + b.bits(7)? as usize),
        };
        if i + rep > total {
            return Err(crate::lang::tr("符号長の繰り返しがはみ出しています", "Code length repeat overruns"));
        }
        for _ in 0..rep {
            lengths[i] = val;
            i += 1;
        }
    }
    if lengths[256] == 0 {
        return Err(crate::lang::tr("ブロックの終わりの符号がありません", "No end-of-block code"));
    }
    let lit = Huff::new(&lengths[..nlen])?;
    let dist = Huff::new(&lengths[nlen..])?;
    codes(b, out, &lit, &dist, max)
}

fn fixed(b: &mut Bits, out: &mut Vec<u8>, max: usize) -> R<()> {
    let mut l = [8u8; 288];
    l[144..256].fill(9);
    l[256..280].fill(7);
    let lit = Huff::new(&l)?;
    let dist = Huff::new(&[5u8; 30])?;
    codes(b, out, &lit, &dist, max)
}

/// 生の deflate を解凍する。`max_out` を超えて出そうになったら（壊れた・悪意ある入力による暴走を避けて）エラー。
pub fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    let run = || -> R<Vec<u8>> {
        let mut b = Bits { d: data, pos: 0, buf: 0, n: 0 };
        let mut out = Vec::new();
        loop {
            let last = b.bits(1)?;
            match b.bits(2)? {
                0 => stored(&mut b, &mut out, max_out)?,
                1 => fixed(&mut b, &mut out, max_out)?,
                2 => dynamic(&mut b, &mut out, max_out)?,
                _ => return Err(crate::lang::tr("不正なブロックの種類です", "Invalid block type")),
            }
            if last == 1 {
                return Ok(out);
            }
        }
    };
    run().map_err(|e| if crate::lang::is_en() { format!("Cannot expand the compressed data: {e}") } else { format!("圧縮データを展開できません: {e}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 以下のバイト列は Python の zlib（wbits=-15）で作った。
    const STORED: &[u8] = &[0x01, 0x03, 0x00, 0xfc, 0xff, 0x61, 0x62, 0x63];
    const FIXED: &[u8] = &[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01];
    const DYNAMIC: &[u8] = &[
        0x65, 0x51, 0xd1, 0x0e, 0x84, 0x20, 0x0c, 0xfb, 0x15, 0x7e, 0x6d, 0x44, 0xa2, 0xe6, 0xe0, 0x24, 0x39, 0x7c, 0xe1, 0xeb, 0x6f, 0xae, 0x93, 0x0d, 0x4d,
        0x14, 0xb0, 0xeb, 0xda, 0x3a, 0x56, 0x2a, 0x85, 0xc2, 0x87, 0x6a, 0xa5, 0x10, 0x53, 0xa3, 0x90, 0xea, 0x6f, 0xcf, 0xc7, 0x17, 0x1f, 0x6d, 0x7b, 0xae,
        0x99, 0x4a, 0x5c, 0x98, 0xc5, 0xc7, 0x25, 0xe5, 0x46, 0x9e, 0x47, 0xb9, 0x6e, 0x28, 0x5d, 0x2f, 0x34, 0x81, 0x95, 0x53, 0x29, 0xb7, 0x3a, 0x03, 0x68,
        0x77, 0xce, 0xdd, 0x34, 0xfc, 0xaa, 0x8e, 0xfb, 0x31, 0x39, 0x28, 0x0a, 0x91, 0x0b, 0x60, 0x45, 0x54, 0x85, 0x08, 0xdc, 0x07, 0x77, 0x70, 0xb7, 0xf4,
        0x93, 0xcc, 0x1c, 0xd1, 0xbc, 0xa4, 0x55, 0x99, 0x92, 0x74, 0x95, 0xa1, 0x29, 0xc2, 0xc6, 0xd3, 0xcc, 0xf8, 0xbb, 0xeb, 0xce, 0x8f, 0xf4, 0x3e, 0x45,
        0x34, 0xb5, 0x76, 0xdd, 0x3b, 0x46, 0xe1, 0xd2, 0x3e, 0xa7, 0x88, 0x12, 0x9a, 0x2f, 0xd7, 0x36, 0x8d, 0x02, 0xa1, 0xfa, 0xe8, 0x66, 0x86, 0x25, 0x14,
        0x38, 0xbe, 0xae, 0x51, 0x88, 0xee, 0x97, 0x86, 0x6d, 0x37, 0xea, 0x18, 0xec, 0x74, 0xc9, 0x76, 0x8f, 0x48, 0xe8, 0xd7, 0x57, 0x28, 0x27, 0x8e, 0xf4,
        0x10, 0xc1, 0x59, 0x60, 0xab, 0xfd, 0x01,
    ];
    const DYNAMIC_TEXT: &str = "gamma kappa beta epsilon beta theta theta theta lambda eta delta beta theta alpha eta eta kappa alpha mu theta epsilon mu delta kappa beta zeta alpha alpha alpha lambda iota alpha eta lambda delta eta mu alpha iota delta theta theta iota delta zeta delta lambda delta theta epsilon alpha eta iota lambda beta gamma lambda mu epsilon beta mu zeta mu mu iota eta iota lambda delta epsilon epsilon kappa theta iota eta kappa alpha theta delta mu eta eta lambda gamma zeta iota mu lambda mu zeta beta theta lambda iota beta gamma iota eta zeta theta mu alpha theta alpha epsilon mu kappa kappa kappa eta lambda gamma gamma iota delta alpha delta iota iota delta";

    #[test]
    fn stored_block() {
        assert_eq!(inflate(STORED, 100).unwrap(), b"abc");
    }

    #[test]
    fn fixed_huffman_block_with_overlapping_copy() {
        // 「hello 」の繰り返しは、距離 6・長さ 17 の重なるコピーになる
        assert_eq!(inflate(FIXED, 100).unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn dynamic_huffman_block() {
        assert_eq!(inflate(DYNAMIC, 10_000).unwrap(), DYNAMIC_TEXT.as_bytes());
    }

    #[test]
    fn output_limit_is_enforced() {
        assert!(inflate(FIXED, 22).is_err());
        assert!(inflate(FIXED, 23).is_ok());
        assert!(inflate(STORED, 2).is_err());
        assert!(inflate(DYNAMIC, 100).is_err());
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        assert!(inflate(&[], 10).is_err());
        assert!(inflate(&[0x07], 10).is_err()); // 不正なブロックの種類（BTYPE=3）
        assert!(inflate(&STORED[..6], 10).is_err()); // 途中で切れている
        assert!(inflate(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x61, 0x62, 0x63], 10).is_err()); // LEN と NLEN が合わない
        assert!(inflate(&FIXED[..5], 100).is_err());
        for cut in 0..DYNAMIC.len() {
            let _ = inflate(&DYNAMIC[..cut], 10_000); // どこで切っても落ちない
        }
        let mut bad = DYNAMIC.to_vec();
        for i in 0..bad.len() {
            bad[i] ^= 0xff;
            let _ = inflate(&bad, 10_000); // 1 バイトずつ壊しても落ちない
            bad[i] ^= 0xff;
        }
    }
}
