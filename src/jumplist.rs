// タスクバーのアイコンを右クリックしたときのメニュー（Windows のジャンプリスト）。
// 「最近使ったファイル」の一覧と、設定ファイルを開く項目を出す。Windows 以外では何もしない。
//
// - 「新しいウィンドウ」の項目は足さない。Windows が、アプリ名の項目（起動）を自動で出すため。
// - COM は `windows` クレートを足さず、この機能に要る分だけ直接呼ぶ（platform.rs と同じ方針で、依存を増やさない）。
// - AppUserModelID は設定しない。明示すると、すでにタスクバーへピン留めした NeoNote と別のアプリ扱いになり、
//   アイコンが 2 つ並んでしまうため（既定の ID は実行ファイルの場所から決まる）。
// - 一覧の組み立ては別スレッドで行う（シェルの呼び出しで UI を止めない）。

use std::path::{Path, PathBuf};

/// 一覧に出す「最近使ったファイル」の最大数。
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_RECENT: usize = 10;

/// 項目を選んだときに `neonote.exe` へ渡す引数。パスは 1 つの引数になるよう引用符で囲み、
/// 先頭が `-` のファイル名でもパスとして読まれるよう `--` を付ける。
#[cfg_attr(not(windows), allow(dead_code))]
fn link_args(path: &Path) -> String {
    format!("-- {}", quote_arg(&path.to_string_lossy()))
}

/// Windows のコマンドラインの規則で 1 つの引数にする。末尾の `\` は、閉じ引用符の前では 2 重にしないと
/// 引用符を打ち消してしまう（フォルダのパスが `\` で終わることがある）。
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_arg(s: &str) -> String {
    let trailing = s.chars().rev().take_while(|c| *c == '\\').count();
    format!("\"{}{}\"", s, "\\".repeat(trailing))
}

/// ジャンプリストを、いまの「最近開いたファイル」と設定ファイルに合わせて作り直す。
/// 失敗してもエディタは止めず、ログに出すだけ。
pub fn update(recent: &[PathBuf], config: Option<&Path>) {
    #[cfg(windows)]
    imp::update(recent.to_vec(), config.map(Path::to_path_buf));
    #[cfg(not(windows))]
    let _ = (recent, config);
}

#[cfg(windows)]
mod imp {
    use super::{link_args, MAX_RECENT};
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    type Raw = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    #[repr(C)]
    struct PropertyKey {
        fmtid: Guid,
        pid: u32,
    }

    /// PROPVARIANT。ポインタ 2 つ分の領域に、`LPWSTR`（VT_LPWSTR）だけを入れて使う（x86 で 16 バイト、x64 で 24 バイト）。
    #[repr(C)]
    struct PropVariant {
        vt: u16,
        reserved: [u16; 3],
        data: [usize; 2],
    }

    const VT_LPWSTR: u16 = 31;
    const CLSCTX_INPROC_SERVER: u32 = 1;
    const COINIT_APARTMENTTHREADED: u32 = 2;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
    const E_FAIL: i32 = 0x8000_4005u32 as i32;

    const CLSID_DESTINATION_LIST: Guid = Guid { d1: 0x77F10CF0, d2: 0x3DB5, d3: 0x4966, d4: [0xB5, 0x20, 0xB7, 0xC5, 0x4F, 0xD3, 0x5E, 0xD6] };
    const IID_CUSTOM_DESTINATION_LIST: Guid = Guid { d1: 0x6332DEBF, d2: 0x87B5, d3: 0x4670, d4: [0x90, 0xC0, 0x5E, 0x57, 0xB4, 0x08, 0xA4, 0x9E] };
    const CLSID_OBJECT_COLLECTION: Guid = Guid { d1: 0x2D3468C1, d2: 0x36A7, d3: 0x43B6, d4: [0xAC, 0x24, 0xD3, 0xF0, 0x2F, 0xD9, 0x60, 0x7A] };
    const IID_OBJECT_COLLECTION: Guid = Guid { d1: 0x5632B1A4, d2: 0xE38A, d3: 0x400A, d4: [0x92, 0x8A, 0xD4, 0xCD, 0x63, 0x23, 0x02, 0x95] };
    const IID_OBJECT_ARRAY: Guid = Guid { d1: 0x92CA9DCD, d2: 0x5622, d3: 0x4BBA, d4: [0xA8, 0x05, 0x5E, 0x9F, 0x54, 0x1B, 0xD8, 0xC9] };
    const CLSID_SHELL_LINK: Guid = Guid { d1: 0x0002_1401, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    const IID_SHELL_LINK_W: Guid = Guid { d1: 0x0002_14F9, d2: 0, d3: 0, d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    const IID_PROPERTY_STORE: Guid = Guid { d1: 0x886D8EEB, d2: 0x8CF2, d3: 0x4446, d4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99] };
    /// PKEY_Title（項目に出す名前）
    const PKEY_TITLE: PropertyKey = PropertyKey {
        fmtid: Guid { d1: 0xF29F85E0, d2: 0x4FF9, d3: 0x1068, d4: [0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9] },
        pid: 2,
    };

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: Raw, flags: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(clsid: *const Guid, outer: Raw, context: u32, iid: *const Guid, out: *mut Raw) -> i32;
    }

    /// COM オブジェクト 1 つ。捨てるときに Release を呼ぶ。
    struct Com(Raw);

    impl Com {
        /// 関数表の `index` 番目の関数のアドレス（0 = QueryInterface、1 = AddRef、2 = Release）。
        unsafe fn method(&self, index: usize) -> usize {
            let table = *(self.0 as *const *const usize);
            *table.add(index)
        }
    }

    /// `method!(オブジェクト, 番号, fn(引数の型…) -> 戻り値の型)` で、関数表の関数を呼べる形にする（第 1 引数の `this` は自動）。
    macro_rules! method {
        ($com:expr, $index:expr, fn($($arg:ty),*) -> $ret:ty) => {
            std::mem::transmute::<usize, unsafe extern "system" fn(Raw $(, $arg)*) -> $ret>($com.method($index))
        };
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let release = method!(self, 2, fn() -> u32);
                    release(self.0);
                }
            }
        }
    }

    fn check(hr: i32) -> Result<(), i32> {
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    unsafe fn create(clsid: &Guid, iid: &Guid) -> Result<Com, i32> {
        let mut out: Raw = std::ptr::null_mut();
        check(CoCreateInstance(clsid, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, iid, &mut out))?;
        Ok(Com(out))
    }

    unsafe fn query(com: &Com, iid: &Guid) -> Result<Com, i32> {
        let query_interface = method!(com, 0, fn(*const Guid, *mut Raw) -> i32);
        let mut out: Raw = std::ptr::null_mut();
        check(query_interface(com.0, iid, &mut out))?;
        Ok(Com(out))
    }

    /// ジャンプリストの項目 1 つ（`IShellLinkW`）。`exe` を `args` つきで起動する。`title` が一覧に出る名前、`tip` がツールチップ。
    unsafe fn make_link(exe: &[u16], args: &str, title: &str, tip: &str) -> Result<Com, i32> {
        let link = create(&CLSID_SHELL_LINK, &IID_SHELL_LINK_W)?;
        let set_description = method!(link, 7, fn(*const u16) -> i32);
        let set_arguments = method!(link, 11, fn(*const u16) -> i32);
        let set_icon_location = method!(link, 17, fn(*const u16, i32) -> i32);
        let set_path = method!(link, 20, fn(*const u16) -> i32);
        let (args, tip) = (wide(args), wide(tip));
        check(set_path(link.0, exe.as_ptr()))?;
        check(set_arguments(link.0, args.as_ptr()))?;
        check(set_description(link.0, tip.as_ptr()))?;
        check(set_icon_location(link.0, exe.as_ptr(), 0))?; // 実行ファイルに埋め込んだアイコン

        // 名前は IPropertyStore の PKEY_Title で付ける（無いと項目が出ない）
        let store = query(&link, &IID_PROPERTY_STORE)?;
        let set_value = method!(store, 6, fn(*const PropertyKey, *const PropVariant) -> i32);
        let commit = method!(store, 7, fn() -> i32);
        let title = wide(title);
        let value = PropVariant { vt: VT_LPWSTR, reserved: [0; 3], data: [title.as_ptr() as usize, 0] };
        check(set_value(store.0, &PKEY_TITLE, &value))?;
        check(commit(store.0))?;
        Ok(link)
    }

    /// `IObjectCollection` に項目を足す。
    unsafe fn add(items: &Com, link: &Com) -> Result<(), i32> {
        let add_object = method!(items, 5, fn(Raw) -> i32);
        check(add_object(items.0, link.0))
    }

    /// ユーザーが「この一覧から削除」した項目の引数。これを入れ直すと CommitList が失敗するので、飛ばすために集める。
    unsafe fn removed_arguments(removed: &Com) -> Vec<String> {
        let mut out = Vec::new();
        if removed.0.is_null() {
            return out;
        }
        let get_count = method!(removed, 3, fn(*mut u32) -> i32);
        let get_at = method!(removed, 4, fn(u32, *const Guid, *mut Raw) -> i32);
        let mut count = 0u32;
        if get_count(removed.0, &mut count) < 0 {
            return out;
        }
        for i in 0..count.min(200) {
            let mut raw: Raw = std::ptr::null_mut();
            if get_at(removed.0, i, &IID_SHELL_LINK_W, &mut raw) < 0 {
                continue; // IShellLink 以外（他の仕組みが足した項目）は対象外
            }
            let link = Com(raw);
            let get_arguments = method!(link, 10, fn(*mut u16, i32) -> i32);
            let mut buf = [0u16; 2048];
            if get_arguments(link.0, buf.as_mut_ptr(), buf.len() as i32) >= 0 {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                out.push(String::from_utf16_lossy(&buf[..len]));
            }
        }
        out
    }

    /// 一覧の中身を作って確定する（BeginList の後）。
    unsafe fn fill(list: &Com, exe: &[u16], cap: usize, recent: &[PathBuf], config: Option<&Path>, removed: &[String]) -> Result<(), i32> {
        // 最近使ったファイル
        let items = create(&CLSID_OBJECT_COLLECTION, &IID_OBJECT_COLLECTION)?;
        let mut shown = 0;
        for path in recent {
            if shown >= cap {
                break;
            }
            let args = link_args(path);
            if removed.contains(&args) || !path.exists() {
                continue;
            }
            let title = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            let link = make_link(exe, &args, &title, &path.display().to_string())?;
            add(&items, &link)?;
            shown += 1;
        }
        if shown > 0 {
            let array = query(&items, &IID_OBJECT_ARRAY)?;
            let append_category = method!(list, 5, fn(*const u16, Raw) -> i32);
            let name = wide(crate::lang::tr("最近使ったファイル", "Recent files"));
            check(append_category(list.0, name.as_ptr(), array.0))?;
        }

        // タスク: 設定ファイルを開く（まだ作られていないときは出さない）
        if let Some(config) = config.filter(|c| c.is_file()) {
            let tasks = create(&CLSID_OBJECT_COLLECTION, &IID_OBJECT_COLLECTION)?;
            let link = make_link(exe, &link_args(config), crate::lang::tr("設定ファイルを開く", "Open settings file"), &config.display().to_string())?;
            add(&tasks, &link)?;
            let array = query(&tasks, &IID_OBJECT_ARRAY)?;
            let add_user_tasks = method!(list, 7, fn(Raw) -> i32);
            check(add_user_tasks(list.0, array.0))?;
        }

        let commit_list = method!(list, 8, fn() -> i32);
        check(commit_list(list.0))
    }

    unsafe fn build(recent: &[PathBuf], config: Option<&Path>) -> Result<(), i32> {
        let exe_path = std::env::current_exe().map_err(|_| E_FAIL)?;
        let exe: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let list = create(&CLSID_DESTINATION_LIST, &IID_CUSTOM_DESTINATION_LIST)?;
        let begin_list = method!(list, 4, fn(*mut u32, *const Guid, *mut Raw) -> i32);
        let mut slots = 0u32;
        let mut removed_raw: Raw = std::ptr::null_mut();
        check(begin_list(list.0, &mut slots, &IID_OBJECT_ARRAY, &mut removed_raw))?;
        let removed = Com(removed_raw);
        let removed_args = removed_arguments(&removed);
        let cap = (slots as usize).clamp(1, MAX_RECENT);
        let result = fill(&list, &exe, cap, recent, config, &removed_args);
        if result.is_err() {
            let abort_list = method!(list, 11, fn() -> i32);
            abort_list(list.0);
        }
        result
    }

    /// 更新の世代。続けて何度も呼ばれたとき、古い呼び出しは飛ばして最後の 1 回だけを反映する。
    static LATEST: AtomicU64 = AtomicU64::new(0);
    static BUSY: Mutex<()> = Mutex::new(());

    pub fn update(recent: Vec<PathBuf>, config: Option<PathBuf>) {
        let generation = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = std::thread::Builder::new().name("jumplist".into()).spawn(move || {
            let _guard = BUSY.lock().unwrap_or_else(|e| e.into_inner());
            if LATEST.load(Ordering::SeqCst) != generation {
                return; // もっと新しい更新が待っている
            }
            let result = unsafe {
                let init = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
                if init < 0 && init != RPC_E_CHANGED_MODE {
                    Err(init)
                } else {
                    let r = build(&recent, config.as_deref());
                    if init >= 0 {
                        CoUninitialize();
                    }
                    r
                }
            };
            if let Err(hr) = result {
                crate::log::warn("JumpList", &format!("タスクバーのメニューを更新できませんでした: HRESULT 0x{hr:08X}"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_becomes_one_quoted_argument_after_double_dash() {
        assert_eq!(link_args(Path::new("C:\\my docs\\a.txt")), "-- \"C:\\my docs\\a.txt\"");
        assert_eq!(link_args(Path::new("D:\\-weird.txt")), "-- \"D:\\-weird.txt\"");
    }

    #[test]
    fn trailing_backslash_is_doubled_so_it_cannot_eat_the_closing_quote() {
        assert_eq!(quote_arg("C:\\dir\\"), "\"C:\\dir\\\\\"");
        assert_eq!(quote_arg("C:\\dir\\\\"), "\"C:\\dir\\\\\\\\\"");
        assert_eq!(quote_arg("plain"), "\"plain\"");
    }
}
