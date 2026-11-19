// Windows の Media Foundation で動画を読む。今は、面積図の縮めた画像に使う 1 コマと、長さ・大きさだけ。
// 解読は OS に入っている部品なので、読める形式は OS による（H.264・mp4 は標準。HEVC・AV1 は拡張が要る）。

/// 動画として扱うファイルか（名前で見る）。
pub fn is_video_name(name: &str) -> bool {
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "wmv" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "ts" | "m2ts" | "3gp" | "flv")
}

/// 動画の 1 コマ（RGBA）。
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

#[cfg(windows)]
pub use imp::{frame, Player};

#[cfg(not(windows))]
pub fn frame(_path: &std::path::Path) -> Option<Frame> {
    None
}

/// Windows 以外は再生できない（作れない）。
#[cfg(not(windows))]
pub struct Player;
#[cfg(not(windows))]
impl Player {
    pub fn new(_hwnd: *mut std::ffi::c_void, _path: &std::path::Path) -> Option<Player> {
        None
    }
    pub fn play(&self) {}
    pub fn pause(&self) {}
    pub fn is_paused(&self) -> bool {
        true
    }
    pub fn is_ended(&self) -> bool {
        false
    }
    pub fn time(&self) -> f64 {
        0.0
    }
    pub fn duration(&self) -> f64 {
        0.0
    }
    pub fn seek(&self, _t: f64) {}
    pub fn set_volume(&self, _v: f64) {}
    pub fn resize(&self, _w: i32, _h: i32) {}
    pub fn video_size(&self) -> Option<(u32, u32)> {
        None
    }
    pub fn error(&self) -> bool {
        false
    }
}

#[cfg(windows)]
mod imp {
    use super::Frame;
    use std::ffi::c_void;
    use std::path::Path;

    type Raw = *mut c_void;

    #[repr(C)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    /// PROPVARIANT（8 バイトの値だけを使う）。
    #[repr(C)]
    struct PropVariant {
        vt: u16,
        reserved: [u16; 3],
        value: u64,
        pad: usize,
    }

    const fn g(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Guid {
        Guid { d1, d2, d3, d4 }
    }
    const MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING: Guid = g(0xfb394f3d, 0xccf1, 0x42ee, [0xbb, 0xb3, 0xf9, 0xb8, 0x45, 0xd5, 0x68, 0x1d]);
    const MF_MT_MAJOR_TYPE: Guid = g(0x48eba18e, 0xf8c9, 0x4687, [0xbf, 0x11, 0x0a, 0x74, 0xc9, 0xf9, 0x6a, 0x8f]);
    const MF_MT_SUBTYPE: Guid = g(0xf7e34c9a, 0x42e8, 0x4714, [0xb7, 0x4b, 0xcb, 0x29, 0xd7, 0x2c, 0x35, 0xe5]);
    const MF_MT_FRAME_SIZE: Guid = g(0x1652c33d, 0xd6b2, 0x4012, [0xb8, 0x34, 0x72, 0x03, 0x08, 0x49, 0xa3, 0x7d]);
    const MF_MT_DEFAULT_STRIDE: Guid = g(0x644b4e48, 0x1e02, 0x4516, [0xb0, 0xeb, 0xc0, 0x1c, 0xa9, 0xd4, 0x9a, 0xc6]);
    const MF_PD_DURATION: Guid = g(0x6c990d33, 0xbb8e, 0x477a, [0x85, 0x98, 0x0d, 0x5d, 0x96, 0xfc, 0xd8, 0x8a]);
    const MF_MEDIA_TYPE_VIDEO: Guid = g(0x73646976, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
    const MF_VIDEO_FORMAT_RGB32: Guid = g(0x00000016, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
    const GUID_NULL: Guid = g(0, 0, 0, [0; 8]);

    const MF_VERSION: u32 = 0x0002_0070;
    const COINIT_MULTITHREADED: u32 = 0;
    const FIRST_VIDEO_STREAM: u32 = 0xFFFF_FFFC;
    const ALL_STREAMS: u32 = 0xFFFF_FFFE;
    const MEDIA_SOURCE: u32 = 0xFFFF_FFFF;
    const READERF_END_OF_STREAM: u32 = 2;
    const VT_I8: u16 = 20;
    /// 黒い始まりを避けて、長さの 1 割（最大 10 秒）の所のコマを取る。
    const SEEK_MAX: u64 = 10 * 10_000_000;
    const MAX_SIDE: usize = 8192;

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: Raw, flags: u32) -> i32;
        fn CoUninitialize();
        fn PropVariantClear(p: *mut PropVariant) -> i32;
        fn CoCreateInstance(clsid: *const Guid, outer: Raw, context: u32, iid: *const Guid, out: *mut Raw) -> i32;
    }
    #[link(name = "mfplat")]
    extern "system" {
        fn MFStartup(version: u32, flags: u32) -> i32;
        fn MFShutdown() -> i32;
        fn MFCreateAttributes(out: *mut Raw, size: u32) -> i32;
        fn MFCreateMediaType(out: *mut Raw) -> i32;
    }
    #[link(name = "mfreadwrite")]
    extern "system" {
        fn MFCreateSourceReaderFromURL(url: *const u16, attrs: Raw, out: *mut Raw) -> i32;
    }

    macro_rules! method {
        ($com:expr, $index:expr, fn($($arg:ty),*) -> $ret:ty) => {
            std::mem::transmute::<usize, unsafe extern "system" fn(Raw $(, $arg)*) -> $ret>($com.method($index))
        };
    }

    struct Com(Raw);
    impl Com {
        unsafe fn method(&self, index: usize) -> usize {
            *(*(self.0 as *const *const usize)).add(index)
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { method!(self, 2, fn() -> u32)(self.0) };
            }
        }
    }

    // IMFAttributes の関数表の番号（IMFMediaType・IMFSample も同じ並びを持つ）
    const GET_UINT32: usize = 7;
    const GET_UINT64: usize = 8;
    const SET_UINT32: usize = 21;
    const SET_GUID: usize = 24;

    /// 動画の 1 コマを RGBA で取り出す。読めなければ None。裏のスレッドで呼ぶ。
    pub fn frame(path: &Path) -> Option<Frame> {
        unsafe {
            let init = CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED);
            let started = MFStartup(MF_VERSION, 0) >= 0;
            let r = if started { read(path) } else { None };
            if started {
                MFShutdown();
            }
            if init >= 0 {
                CoUninitialize();
            }
            r
        }
    }

    unsafe fn read(path: &Path) -> Option<Frame> {
        let mut a: Raw = std::ptr::null_mut();
        if MFCreateAttributes(&mut a, 1) < 0 {
            return None;
        }
        let attrs = Com(a);
        method!(attrs, SET_UINT32, fn(*const Guid, u32) -> i32)(attrs.0, &MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
        let url: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let mut r: Raw = std::ptr::null_mut();
        if MFCreateSourceReaderFromURL(url.as_ptr(), attrs.0, &mut r) < 0 || r.is_null() {
            return None;
        }
        let reader = Com(r);
        method!(reader, 4, fn(u32, i32) -> i32)(reader.0, ALL_STREAMS, 0);
        method!(reader, 4, fn(u32, i32) -> i32)(reader.0, FIRST_VIDEO_STREAM, 1);
        let mut t: Raw = std::ptr::null_mut();
        if MFCreateMediaType(&mut t) < 0 {
            return None;
        }
        let want = Com(t);
        method!(want, SET_GUID, fn(*const Guid, *const Guid) -> i32)(want.0, &MF_MT_MAJOR_TYPE, &MF_MEDIA_TYPE_VIDEO);
        method!(want, SET_GUID, fn(*const Guid, *const Guid) -> i32)(want.0, &MF_MT_SUBTYPE, &MF_VIDEO_FORMAT_RGB32);
        if method!(reader, 7, fn(u32, *mut u32, Raw) -> i32)(reader.0, FIRST_VIDEO_STREAM, std::ptr::null_mut(), want.0) < 0 {
            return None;
        }
        let mut c: Raw = std::ptr::null_mut();
        if method!(reader, 6, fn(u32, *mut Raw) -> i32)(reader.0, FIRST_VIDEO_STREAM, &mut c) < 0 || c.is_null() {
            return None;
        }
        let cur = Com(c);
        let mut size = 0u64;
        if method!(cur, GET_UINT64, fn(*const Guid, *mut u64) -> i32)(cur.0, &MF_MT_FRAME_SIZE, &mut size) < 0 {
            return None;
        }
        let (w, h) = ((size >> 32) as usize, (size & 0xFFFF_FFFF) as usize);
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return None;
        }
        let mut stride_raw = 0u32;
        let stride = if method!(cur, GET_UINT32, fn(*const Guid, *mut u32) -> i32)(cur.0, &MF_MT_DEFAULT_STRIDE, &mut stride_raw) >= 0 { stride_raw as i32 } else { (w * 4) as i32 };

        // 長さの 1 割の所へ（短い・長さの分からない動画は先頭のまま）
        let mut dur = PropVariant { vt: 0, reserved: [0; 3], value: 0, pad: 0 };
        if method!(reader, 12, fn(u32, *const Guid, *mut PropVariant) -> i32)(reader.0, MEDIA_SOURCE, &MF_PD_DURATION, &mut dur) >= 0 {
            let at = (dur.value / 10).min(SEEK_MAX);
            PropVariantClear(&mut dur);
            if at > 0 {
                let pos = PropVariant { vt: VT_I8, reserved: [0; 3], value: at, pad: 0 };
                method!(reader, 8, fn(*const Guid, *const PropVariant) -> i32)(reader.0, &GUID_NULL, &pos);
            }
        }

        for _ in 0..64 {
            let (mut index, mut flags, mut time) = (0u32, 0u32, 0i64);
            let mut s: Raw = std::ptr::null_mut();
            if method!(reader, 9, fn(u32, u32, *mut u32, *mut u32, *mut i64, *mut Raw) -> i32)(reader.0, FIRST_VIDEO_STREAM, 0, &mut index, &mut flags, &mut time, &mut s) < 0 {
                return None;
            }
            if s.is_null() {
                if flags & READERF_END_OF_STREAM != 0 {
                    return None;
                }
                continue;
            }
            let sample = Com(s);
            let mut b: Raw = std::ptr::null_mut();
            if method!(sample, 41, fn(*mut Raw) -> i32)(sample.0, &mut b) < 0 || b.is_null() {
                return None;
            }
            let buf = Com(b);
            let (mut p, mut max, mut len): (*mut u8, u32, u32) = (std::ptr::null_mut(), 0, 0);
            if method!(buf, 3, fn(*mut *mut u8, *mut u32, *mut u32) -> i32)(buf.0, &mut p, &mut max, &mut len) < 0 || p.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(p, len as usize);
            let row = stride.unsigned_abs() as usize;
            let out = if row >= w * 4 && bytes.len() >= row * h {
                let mut rgba = Vec::with_capacity(w * h * 4);
                for y in 0..h {
                    let sy = if stride < 0 { h - 1 - y } else { y };
                    for px in bytes[sy * row..sy * row + w * 4].as_chunks::<4>().0 {
                        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]); // BGRX
                    }
                }
                Some(Frame { w, h, rgba })
            } else {
                None
            };
            method!(buf, 4, fn() -> i32)(buf.0);
            return out;
        }
        None
    }

    // ---- 再生（IMFMediaEngine）。映像は渡した窓へ OS が描き、音も OS が鳴らす。時間合わせ・シークも OS ----

    const CLSID_MEDIA_ENGINE_CLASS_FACTORY: Guid = g(0xb44392da, 0x499b, 0x446b, [0xa4, 0xcb, 0x00, 0x5f, 0xea, 0xd0, 0xe6, 0xd5]);
    const IID_MEDIA_ENGINE_CLASS_FACTORY: Guid = g(0x4d645ace, 0x26aa, 0x4688, [0x9b, 0xe1, 0xdf, 0x35, 0x16, 0x99, 0x0b, 0x93]);
    const IID_MEDIA_ENGINE_EX: Guid = g(0x83015ead, 0xb1e6, 0x40d0, [0xa9, 0x8a, 0x37, 0x14, 0x5f, 0xfe, 0x1a, 0xd1]);
    const IID_MEDIA_ENGINE_NOTIFY: Guid = g(0xfee7c112, 0xe776, 0x42b5, [0x9b, 0xbf, 0x00, 0x48, 0x52, 0x4e, 0x2b, 0xd5]);
    const IID_UNKNOWN: Guid = g(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const MF_MEDIA_ENGINE_CALLBACK: Guid = g(0xc60381b8, 0x83a4, 0x41f8, [0xa3, 0xd0, 0xde, 0x05, 0x07, 0x68, 0x49, 0xa9]);
    const MF_MEDIA_ENGINE_PLAYBACK_HWND: Guid = g(0xd988879b, 0x67c9, 0x4d92, [0xba, 0xa7, 0x6e, 0xad, 0xd4, 0x46, 0x03, 0x9d]);
    const SET_UINT64: usize = 22;
    const SET_UNKNOWN: usize = 27;
    const COINIT_APARTMENTTHREADED: u32 = 2;
    const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

    #[link(name = "oleaut32")]
    extern "system" {
        fn SysAllocString(s: *const u16) -> *mut u16;
        fn SysFreeString(s: *mut u16);
    }

    fn same(a: &Guid, b: &Guid) -> bool {
        a.d1 == b.d1 && a.d2 == b.d2 && a.d3 == b.d3 && a.d4 == b.d4
    }

    // 知らせを受けるだけの COM オブジェクト（何もしない。状態は呼ぶ側が都度たずねる）。静的に 1 つだけ置き、数えない
    #[repr(C)]
    struct Notify {
        vtbl: *const Table,
    }
    unsafe impl Sync for Notify {}
    #[repr(C)]
    struct Table([*const (); 4]);
    unsafe impl Sync for Table {}
    unsafe extern "system" fn n_qi(this: Raw, iid: *const Guid, out: *mut Raw) -> i32 {
        if same(&*iid, &IID_UNKNOWN) || same(&*iid, &IID_MEDIA_ENGINE_NOTIFY) {
            *out = this;
            0
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
    unsafe extern "system" fn n_ref(_: Raw) -> u32 {
        1
    }
    unsafe extern "system" fn n_event(_: Raw, _: u32, _: usize, _: u32) -> i32 {
        0
    }
    static NOTIFY_TABLE: Table = Table([n_qi as *const (), n_ref as *const (), n_ref as *const (), n_event as *const ()]);
    static NOTIFY: Notify = Notify { vtbl: &NOTIFY_TABLE };

    /// 再生 1 つ。捨てると止めて片付ける。画面のスレッドだけで使う。
    pub struct Player {
        engine: Com,
        ex: Option<Com>,
    }

    impl Player {
        /// `hwnd` の窓（子の窓）へ映像を描く再生を作り、`path` を読み始める（止まった状態）。
        pub fn new(hwnd: Raw, path: &Path) -> Option<Player> {
            unsafe {
                let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED); // 画面のスレッドは閉じるまで COM を使う
                if MFStartup(MF_VERSION, 0) < 0 {
                    return None;
                }
                let r = Self::create(hwnd, path);
                if r.is_none() {
                    MFShutdown();
                }
                r
            }
        }

        unsafe fn create(hwnd: Raw, path: &Path) -> Option<Player> {
            let mut f: Raw = std::ptr::null_mut();
            if CoCreateInstance(&CLSID_MEDIA_ENGINE_CLASS_FACTORY, std::ptr::null_mut(), 1, &IID_MEDIA_ENGINE_CLASS_FACTORY, &mut f) < 0 || f.is_null() {
                return None;
            }
            let factory = Com(f);
            let mut a: Raw = std::ptr::null_mut();
            if MFCreateAttributes(&mut a, 2) < 0 {
                return None;
            }
            let attrs = Com(a);
            method!(attrs, SET_UNKNOWN, fn(*const Guid, Raw) -> i32)(attrs.0, &MF_MEDIA_ENGINE_CALLBACK, &NOTIFY as *const Notify as Raw);
            method!(attrs, SET_UINT64, fn(*const Guid, u64) -> i32)(attrs.0, &MF_MEDIA_ENGINE_PLAYBACK_HWND, hwnd as u64);
            let mut e: Raw = std::ptr::null_mut();
            if method!(factory, 3, fn(u32, Raw, *mut Raw) -> i32)(factory.0, 0, attrs.0, &mut e) < 0 || e.is_null() {
                return None;
            }
            let engine = Com(e);
            let ex = {
                let mut x: Raw = std::ptr::null_mut();
                (method!(engine, 0, fn(*const Guid, *mut Raw) -> i32)(engine.0, &IID_MEDIA_ENGINE_EX, &mut x) >= 0 && !x.is_null()).then(|| Com(x))
            };
            let url: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
            let b = SysAllocString(url.as_ptr());
            let ok = method!(engine, 6, fn(*mut u16) -> i32)(engine.0, b) >= 0;
            SysFreeString(b);
            if !ok {
                return None;
            }
            method!(engine, 12, fn() -> i32)(engine.0); // Load
            Some(Player { engine, ex })
        }

        pub fn play(&self) {
            unsafe { method!(self.engine, 32, fn() -> i32)(self.engine.0) };
        }
        pub fn pause(&self) {
            unsafe { method!(self.engine, 33, fn() -> i32)(self.engine.0) };
        }
        pub fn is_paused(&self) -> bool {
            unsafe { method!(self.engine, 20, fn() -> i32)(self.engine.0) != 0 }
        }
        pub fn is_ended(&self) -> bool {
            unsafe { method!(self.engine, 27, fn() -> i32)(self.engine.0) != 0 }
        }
        /// いまの位置（秒）。
        pub fn time(&self) -> f64 {
            unsafe { method!(self.engine, 16, fn() -> f64)(self.engine.0) }
        }
        /// 長さ（秒）。分からない間は 0。
        pub fn duration(&self) -> f64 {
            let d = unsafe { method!(self.engine, 19, fn() -> f64)(self.engine.0) };
            if d.is_finite() { d } else { 0.0 }
        }
        pub fn seek(&self, t: f64) {
            unsafe { method!(self.engine, 17, fn(f64) -> i32)(self.engine.0, t.max(0.0)) };
        }
        /// 音量（0〜1）。
        pub fn set_volume(&self, v: f64) {
            unsafe { method!(self.engine, 37, fn(f64) -> i32)(self.engine.0, v.clamp(0.0, 1.0)) };
        }
        /// 映像の元の大きさ（読み込みが進むまでは None）。
        pub fn video_size(&self) -> Option<(u32, u32)> {
            let (mut w, mut h) = (0u32, 0u32);
            let ok = unsafe { method!(self.engine, 40, fn(*mut u32, *mut u32) -> i32)(self.engine.0, &mut w, &mut h) } >= 0;
            (ok && w > 0 && h > 0).then_some((w, h))
        }
        /// 読めなかった（形式に対応していない・壊れている）。
        pub fn error(&self) -> bool {
            let mut e: Raw = std::ptr::null_mut();
            unsafe {
                let r = method!(self.engine, 3, fn(*mut Raw) -> i32)(self.engine.0, &mut e);
                let has = r >= 0 && !e.is_null();
                if has {
                    drop(Com(e));
                }
                has
            }
        }
        /// 描く窓の大きさが変わった時。縦横比を保って真ん中に描く。
        pub fn resize(&self, w: i32, h: i32) {
            let Some(ex) = &self.ex else { return };
            #[repr(C)]
            struct Rect {
                l: i32,
                t: i32,
                r: i32,
                b: i32,
            }
            let (dw, dh) = match self.video_size() {
                Some((vw, vh)) => {
                    let s = (w as f64 / vw as f64).min(h as f64 / vh as f64);
                    ((vw as f64 * s) as i32, (vh as f64 * s) as i32)
                }
                None => (w, h),
            };
            let rect = Rect { l: (w - dw) / 2, t: (h - dh) / 2, r: (w - dw) / 2 + dw, b: (h - dh) / 2 + dh };
            let black: u32 = 0xFF00_0000;
            unsafe { method!(ex, 47, fn(Raw, *const Rect, *const u32) -> i32)(ex.0, std::ptr::null_mut(), &rect, &black) };
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            unsafe {
                method!(self.engine, 42, fn() -> i32)(self.engine.0); // Shutdown
                self.ex = None;
                MFShutdown();
            }
        }
    }
}
