// Windows の「開く」画面（IFileOpenDialog）。ファイルを選ぶほかに、「フォルダを開く」ボタンで選んだフォルダ
// （何も選んでいなければ今いるフォルダ）を返す。FLTK の画面ではファイルとフォルダを一緒に選べないため。

#[cfg(windows)]
pub use imp::open;

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    type Raw = *mut c_void;

    #[repr(C)]
    #[derive(PartialEq)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    const CLSID_FILE_OPEN_DIALOG: Guid = Guid { d1: 0xDC1C5A9C, d2: 0xE88A, d3: 0x4DDE, d4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7] };
    const IID_FILE_OPEN_DIALOG: Guid = Guid { d1: 0xD57C7288, d2: 0xD4AD, d3: 0x4768, d4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60] };
    const IID_FILE_DIALOG_CUSTOMIZE: Guid = Guid { d1: 0xE6FDD21A, d2: 0x163F, d3: 0x4975, d4: [0x9C, 0x8C, 0xA6, 0x9F, 0x1B, 0xA3, 0x70, 0x34] };
    const IID_FILE_DIALOG_EVENTS: Guid = Guid { d1: 0x973510DB, d2: 0x7D7F, d3: 0x452B, d4: [0x89, 0x75, 0x74, 0xA8, 0x58, 0x28, 0xD3, 0x54] };
    const IID_FILE_DIALOG_CONTROL_EVENTS: Guid = Guid { d1: 0x36116642, d2: 0xD713, d3: 0x4B97, d4: [0x9B, 0x83, 0x74, 0x84, 0xA9, 0xD0, 0x04, 0x33] };
    const IID_UNKNOWN: Guid = Guid { d1: 0, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    const IID_SHELL_ITEM: Guid = Guid { d1: 0x43826D1E, d2: 0xE718, d3: 0x42EE, d4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE] };

    const CLSCTX_INPROC_SERVER: u32 = 1;
    const COINIT_APARTMENTTHREADED: u32 = 2;
    const FOS_ALLOWMULTISELECT: u32 = 0x200;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_PATHMUSTEXIST: u32 = 0x800;
    const FOS_FILEMUSTEXIST: u32 = 0x1000;
    const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
    const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
    const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
    const BUTTON_ID: u32 = 1;

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: Raw, flags: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(clsid: *const Guid, outer: Raw, context: u32, iid: *const Guid, out: *mut Raw) -> i32;
        fn CoTaskMemFree(p: Raw);
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHCreateItemFromParsingName(path: *const u16, bc: Raw, iid: *const Guid, out: *mut Raw) -> i32;
    }

    macro_rules! method {
        ($com:expr, $index:expr, fn($($arg:ty),*) -> $ret:ty) => {
            std::mem::transmute::<usize, unsafe extern "system" fn(Raw $(, $arg)*) -> $ret>($com.method($index))
        };
    }

    /// COM オブジェクト 1 つ。捨てるときに Release を呼ぶ。
    struct Com(Raw);

    impl Com {
        unsafe fn method(&self, index: usize) -> usize {
            let table = *(self.0 as *const *const usize);
            *table.add(index)
        }
        unsafe fn query(&self, iid: &Guid) -> Option<Com> {
            let qi = method!(self, 0, fn(*const Guid, *mut Raw) -> i32);
            let mut out: Raw = std::ptr::null_mut();
            (qi(self.0, iid, &mut out) >= 0 && !out.is_null()).then(|| Com(out))
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { method!(self, 2, fn() -> u32)(self.0) };
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// IShellItem の、ファイルシステム上の道筋。
    unsafe fn item_path(item: &Com) -> Option<PathBuf> {
        let get = method!(item, 5, fn(u32, *mut *mut u16) -> i32);
        let mut p: *mut u16 = std::ptr::null_mut();
        if get(item.0, SIGDN_FILESYSPATH, &mut p) < 0 || p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = std::ffi::OsString::from_wide(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as Raw);
        Some(PathBuf::from(s))
    }

    /// IShellItemArray の道筋の一覧。
    unsafe fn array_paths(arr: &Com) -> Vec<PathBuf> {
        let count = method!(arr, 7, fn(*mut u32) -> i32);
        let at = method!(arr, 8, fn(u32, *mut Raw) -> i32);
        let mut n = 0u32;
        if count(arr.0, &mut n) < 0 {
            return Vec::new();
        }
        (0..n)
            .filter_map(|i| {
                let mut it: Raw = std::ptr::null_mut();
                (at(arr.0, i, &mut it) >= 0 && !it.is_null()).then(|| Com(it)).and_then(|it| item_path(&it))
            })
            .collect()
    }

    thread_local! {
        /// 開いている画面（ボタンの処理から閉じるため）と、ボタンで選んだ物。
        static DIALOG: RefCell<Raw> = const { RefCell::new(std::ptr::null_mut()) };
        static PICKED: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
    }

    // ---- 画面の知らせを受ける COM オブジェクト（IFileDialogEvents と IFileDialogControlEvents）。静的に 1 つだけ置き、数えない ----

    #[repr(C)]
    struct Events {
        file: *const Table<10>,
        control: *const Table<7>,
    }
    unsafe impl Sync for Events {}

    #[repr(C)]
    struct Table<const N: usize>([*const (); N]);
    unsafe impl<const N: usize> Sync for Table<N> {}

    unsafe extern "system" fn qi(this: Raw, iid: *const Guid, out: *mut Raw) -> i32 {
        let iid = &*iid;
        let base = &EVENTS as *const Events as Raw;
        if *iid == IID_UNKNOWN || *iid == IID_FILE_DIALOG_EVENTS {
            *out = base;
        } else if *iid == IID_FILE_DIALOG_CONTROL_EVENTS {
            *out = (base as *mut u8).add(std::mem::size_of::<usize>()) as Raw;
        } else {
            *out = std::ptr::null_mut();
            return E_NOINTERFACE;
        }
        let _ = this;
        0
    }
    unsafe extern "system" fn qi2(this: Raw, iid: *const Guid, out: *mut Raw) -> i32 {
        qi(this, iid, out)
    }
    unsafe extern "system" fn add_ref(_: Raw) -> u32 {
        1
    }
    unsafe extern "system" fn ok1(_: Raw, _: Raw) -> i32 {
        0
    }
    unsafe extern "system" fn ok2(_: Raw, _: Raw, _: Raw) -> i32 {
        0
    }
    unsafe extern "system" fn share(_: Raw, _: Raw, _: Raw, _: *mut u32) -> i32 {
        E_NOTIMPL
    }
    unsafe extern "system" fn overwrite(_: Raw, _: Raw, _: Raw, _: *mut u32) -> i32 {
        E_NOTIMPL
    }
    unsafe extern "system" fn item_selected(_: Raw, _: Raw, _: u32, _: u32) -> i32 {
        0
    }
    unsafe extern "system" fn check_toggled(_: Raw, _: Raw, _: u32, _: i32) -> i32 {
        0
    }
    unsafe extern "system" fn activating(_: Raw, _: Raw, _: u32) -> i32 {
        0
    }
    /// 「フォルダを開く」: 選んでいる物（ファイルとフォルダ）、無ければ今いるフォルダを覚えて閉じる。
    unsafe extern "system" fn button_clicked(_: Raw, _: Raw, id: u32) -> i32 {
        if id != BUTTON_ID {
            return 0;
        }
        let dlg = Com(DIALOG.with(|d| *d.borrow()));
        let mut picked = Vec::new();
        let mut arr: Raw = std::ptr::null_mut();
        if method!(dlg, 28, fn(*mut Raw) -> i32)(dlg.0, &mut arr) >= 0 && !arr.is_null() {
            picked = array_paths(&Com(arr));
        }
        if picked.is_empty() {
            let mut f: Raw = std::ptr::null_mut();
            if method!(dlg, 13, fn(*mut Raw) -> i32)(dlg.0, &mut f) >= 0 && !f.is_null() {
                picked.extend(item_path(&Com(f)));
            }
        }
        if !picked.is_empty() {
            PICKED.with(|p| *p.borrow_mut() = Some(picked));
            method!(dlg, 23, fn(i32) -> i32)(dlg.0, 1); // Close（S_FALSE で Show を終える）
        }
        std::mem::forget(dlg); // 借りているだけ（Release しない）
        0
    }

    static FILE_VTBL: Table<10> = Table([
        qi as *const (),
        add_ref as *const (),
        add_ref as *const (),
        ok1 as *const (),       // OnFileOk（閉じてよい）
        ok2 as *const (),       // OnFolderChanging
        ok1 as *const (),       // OnFolderChange
        ok1 as *const (),       // OnSelectionChange
        share as *const (),
        ok1 as *const (), // OnTypeChange
        overwrite as *const (),
    ]);
    static CONTROL_VTBL: Table<7> = Table([qi2 as *const (), add_ref as *const (), add_ref as *const (), item_selected as *const (), button_clicked as *const (), check_toggled as *const (), activating as *const ()]);
    static EVENTS: Events = Events { file: &FILE_VTBL, control: &CONTROL_VTBL };

    /// 開く画面を出す。選んだファイル（複数可）か、ボタンで選んだフォルダ。キャンセルや失敗は空、画面が使えなければ None。
    pub fn open(owner: Raw, start: Option<&Path>, title: &str, button: &str) -> Option<Vec<PathBuf>> {
        unsafe {
            let init = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let result = show(owner, start, title, button);
            if init >= 0 {
                CoUninitialize();
            }
            result
        }
    }

    unsafe fn show(owner: Raw, start: Option<&Path>, title: &str, button: &str) -> Option<Vec<PathBuf>> {
        let mut raw: Raw = std::ptr::null_mut();
        if CoCreateInstance(&CLSID_FILE_OPEN_DIALOG, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_FILE_OPEN_DIALOG, &mut raw) < 0 || raw.is_null() {
            return None;
        }
        let dlg = Com(raw);
        let mut opts = 0u32;
        method!(dlg, 10, fn(*mut u32) -> i32)(dlg.0, &mut opts);
        method!(dlg, 9, fn(u32) -> i32)(dlg.0, opts | FOS_ALLOWMULTISELECT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST);
        let t = wide(title);
        method!(dlg, 17, fn(*const u16) -> i32)(dlg.0, t.as_ptr());
        if let Some(dir) = start {
            let w = wide(&dir.to_string_lossy());
            let mut item: Raw = std::ptr::null_mut();
            if SHCreateItemFromParsingName(w.as_ptr(), std::ptr::null_mut(), &IID_SHELL_ITEM, &mut item) >= 0 && !item.is_null() {
                let item = Com(item);
                method!(dlg, 12, fn(Raw) -> i32)(dlg.0, item.0);
            }
        }
        if let Some(c) = dlg.query(&IID_FILE_DIALOG_CUSTOMIZE) {
            let b = wide(button);
            method!(c, 5, fn(u32, *const u16) -> i32)(c.0, BUTTON_ID, b.as_ptr());
        }
        let mut cookie = 0u32;
        let advised = method!(dlg, 7, fn(Raw, *mut u32) -> i32)(dlg.0, &EVENTS as *const Events as Raw, &mut cookie) >= 0;
        DIALOG.with(|d| *d.borrow_mut() = dlg.0);
        PICKED.with(|p| *p.borrow_mut() = None);
        let hr = method!(dlg, 3, fn(Raw) -> i32)(dlg.0, owner);
        DIALOG.with(|d| *d.borrow_mut() = std::ptr::null_mut());
        if advised {
            method!(dlg, 8, fn(u32) -> i32)(dlg.0, cookie);
        }
        if let Some(p) = PICKED.with(|p| p.borrow_mut().take()) {
            return Some(p);
        }
        if hr < 0 {
            return Some(Vec::new()); // キャンセル
        }
        let mut arr: Raw = std::ptr::null_mut();
        if method!(dlg, 27, fn(*mut Raw) -> i32)(dlg.0, &mut arr) < 0 || arr.is_null() {
            return Some(Vec::new());
        }
        Some(array_paths(&Com(arr)))
    }
}
