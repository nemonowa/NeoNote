// deflate（RFC 1951）の圧縮と、gzip（RFC 1952）・zlib（RFC 1950）の包み（GUI なし・依存なし）。
// NBT（level.dat は gzip、リージョンのチャンクは zlib）を保存するために使う。解凍は inflate.rs。
// 圧縮は LZ77（ハッシュの鎖で一致を探す）と固定ハフマン符号の 1 ブロックだけ。zlib の既定ほどは縮まないが、
// 短く正しいことを優先した（level.dat のような数 KB〜数百 KB のデータには十分）。
use super::inflate;

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// 一致を探すときに辿る鎖の長さの上限（長いほど縮むが遅い）。
const MAX_CHAIN: usize = 64;
const HASH_BITS: u32 = 15;

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// 下位ビットから詰めるビット列。
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, len: u32) {
        self.acc |= value << self.n;
        self.n += len;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// ハフマン符号は上位ビットから書く決まりなので、反転して詰める。
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0;
        for i in 0..len {
            r |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(r, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// 固定ハフマンの文字・長さの記号（0〜287）を書く。
fn put_litlen(b: &mut Bits, sym: u32) {
    match sym {
        0..=143 => b.code(0x30 + sym, 8),
        144..=255 => b.code(0x190 + sym - 144, 9),
        256..=279 => b.code(sym - 256, 7),
        _ => b.code(0xc0 + sym - 280, 8),
    }
}

fn put_match(b: &mut Bits, len: usize, dist: usize) {
    let li = LEN_BASE.iter().rposition(|&x| x as usize <= len).unwrap_or(0);
    put_litlen(b, 257 + li as u32);
    b.put((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
    let di = DIST_BASE.iter().rposition(|&x| x as usize <= dist).unwrap_or(0);
    b.code(di as u32, 5);
    b.put((dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
}

fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) << 16 | (d[i + 1] as u32) << 8 | d[i + 2] as u32;
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// 生の deflate に圧縮する。
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut b = Bits { out: Vec::with_capacity(data.len() / 2 + 16), acc: 0, n: 0 };
    b.put(1, 1); // 最後のブロック
    b.put(1, 2); // 固定ハフマン
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; WINDOW];
    let mut i = 0;
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= data.len() {
            let h = hash3(data, i);
            prev[i % WINDOW] = head[h];
            head[h] = i;
        }
    };
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0, 0);
        if i + MIN_MATCH <= data.len() {
            let mut cand = head[hash3(data, i)];
            let limit = (data.len() - i).min(MAX_MATCH);
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                let mut l = 0;
                while l < limit && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == limit {
                        break;
                    }
                }
                let p = prev[cand % WINDOW];
                if p == usize::MAX || p >= cand {
                    break; // 鎖が窓の外（古い位置の上書き）へ回った
                }
                cand = p;
                chain += 1;
            }
        }
        if best_len >= MIN_MATCH {
            put_match(&mut b, best_len, best_dist);
            for k in 0..best_len {
                insert(&mut head, &mut prev, i + k);
            }
            i += best_len;
        } else {
            put_litlen(&mut b, data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    put_litlen(&mut b, 256);
    b.finish()
}

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, e) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    t
}

pub fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    !data.iter().fold(!0u32, |c, &b| t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    out.extend(deflate(data));
    out.extend(crc32(data).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate(data));
    out.extend(adler32(data).to_be_bytes());
    out
}

/// gzip を解凍する（ヘッダの付加情報は読み飛ばす）。
pub fn gunzip(b: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b || b[2] != 8 {
        return Err("gzip ではありません".into());
    }
    let flags = b[3];
    let mut p = 10;
    if flags & 4 != 0 {
        let n = *b.get(p).ok_or("gzip が途中で切れています")? as usize | (*b.get(p + 1).ok_or("gzip が途中で切れています")? as usize) << 8;
        p += 2 + n;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            while *b.get(p).ok_or("gzip が途中で切れています")? != 0 {
                p += 1;
            }
            p += 1;
        }
    }
    if flags & 2 != 0 {
        p += 2;
    }
    inflate::inflate(b.get(p..).ok_or("gzip が途中で切れています")?, max_out)
}

/// zlib を解凍する。
pub fn unzlib(b: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    if b.len() < 6 || b[0] & 0x0f != 8 || (u16::from(b[0]) << 8 | u16::from(b[1])) % 31 != 0 {
        return Err("zlib ではありません".into());
    }
    if b[1] & 0x20 != 0 {
        return Err("zlib の辞書付きの形式には対応していません".into());
    }
    inflate::inflate(&b[2..], max_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = b"Minecraft level.dat abc abc abc abcabcabc ".repeat(40);
        v.extend((0..5000u32).map(|i| (i * 7 % 251) as u8));
        v.extend(vec![0u8; 3000]);
        v
    }

    #[test]
    fn deflate_round_trips_through_inflate() {
        for data in [Vec::new(), b"a".to_vec(), b"aaaaaaaaaaaaaaaaaaaaaaaa".to_vec(), sample()] {
            let packed = deflate(&data);
            assert_eq!(inflate::inflate(&packed, 1 << 24).unwrap(), data);
        }
        let s = sample();
        assert!(deflate(&s).len() < s.len() / 2, "繰り返しの多いデータは縮む");
    }

    #[test]
    fn long_inputs_beyond_the_window_round_trip() {
        let data: Vec<u8> = (0..200_000u32).map(|i| ((i / 3) % 97) as u8 ^ (i >> 11) as u8).collect();
        assert_eq!(inflate::inflate(&deflate(&data), 1 << 24).unwrap(), data);
    }

    #[test]
    fn gzip_and_zlib_wrappers_round_trip() {
        let data = sample();
        assert_eq!(gunzip(&gzip(&data), 1 << 24).unwrap(), data);
        assert_eq!(unzlib(&zlib(&data), 1 << 24).unwrap(), data);
        assert!(gunzip(b"not gzip at all, really", 100).is_err());
        assert!(unzlib(b"xx", 100).is_err());
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }
}
