//! Windows 11 的「远程桌面连接安全警告」对话框里,剪贴板 / 打印机等本地资源
//! **默认是未勾选的**(不管 .rdp 里 redirectclipboard 写没写、也不管 LocalDevices
//! 里有没有信任记录),用户直接点「连接」就等于拒绝共享剪贴板。
//!
//! 这里在拉起 mstsc 之后起一个后台线程,只做一件事:找到**我们自己拉起的那个
//! mstsc 进程**的资源确认对话框,把「剪贴板」那一项勾上。不代替用户点「连接」,
//! 也不动其它复选框(打印机 / 智能卡 / 不再询问)。
//!
//! 找不到对话框(用户已配置为不提示 / 系统语言不在下表 / mstsc 版本变化)时静默放弃,
//! 用户仍可手动勾选。

use std::time::{Duration, Instant};

/// 各语言下「剪贴板」复选框的文案(小写比较)
const CLIPBOARD_LABELS: &[&str] = &[
    "剪贴板",              // zh-CN
    "剪貼板",              // zh-TW/HK
    "剪貼簿",              // zh-TW(另一种译法)
    "clipboard",           // en
    "klembord",            // nl
    "zwischenablage",      // de
    "presse-papiers",      // fr
    "portapapeles",        // es
    "área de transferência", // pt
    "area de transferencia", // pt(无重音)
    "appunti",             // it
    "urklipp",             // sv
    "schowek",             // pl
    "クリップボード",       // ja
    "클립보드",             // ko
    "буфер обмена",        // ru
];

/// 判断某个复选框文案是否是「剪贴板」(去掉助记符 & 与首尾空白后比较)
pub fn is_clipboard_label(text: &str) -> bool {
    let cleaned: String = text.replace('&', "").trim().to_lowercase();
    if cleaned.is_empty() {
        return false;
    }
    CLIPBOARD_LABELS.iter().any(|l| cleaned.contains(l))
}

#[cfg(windows)]
mod win {
    use super::{is_clipboard_label, Duration, Instant};

    type Hwnd = isize;
    const BM_GETCHECK: u32 = 0x00F0;
    const BM_CLICK: u32 = 0x00F5;
    /// 对话框窗口类
    const DIALOG_CLASS: &str = "#32770";

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, l: isize) -> i32;
        fn EnumChildWindows(parent: Hwnd, cb: extern "system" fn(Hwnd, isize) -> i32, l: isize) -> i32;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn GetClassNameW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
        fn GetWindowTextW(h: Hwnd, buf: *mut u16, n: i32) -> i32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn SendMessageW(h: Hwnd, msg: u32, w: usize, l: isize) -> isize;
    }

    fn read_w(f: impl Fn(*mut u16, i32) -> i32) -> String {
        let mut buf = [0u16; 256];
        let n = f(buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    fn class_of(h: Hwnd) -> String {
        read_w(|p, n| unsafe { GetClassNameW(h, p, n) })
    }

    fn text_of(h: Hwnd) -> String {
        read_w(|p, n| unsafe { GetWindowTextW(h, p, n) })
    }

    fn pid_of(h: Hwnd) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        pid
    }

    /// EnumWindows 回调:收集属于目标进程的可见对话框
    struct Scan {
        pid: u32,
        found: Vec<Hwnd>,
    }

    extern "system" fn on_window(h: Hwnd, l: isize) -> i32 {
        let scan = unsafe { &mut *(l as *mut Scan) };
        if unsafe { IsWindowVisible(h) } != 0 && pid_of(h) == scan.pid && class_of(h) == DIALOG_CLASS {
            scan.found.push(h);
        }
        1
    }

    extern "system" fn on_child(h: Hwnd, l: isize) -> i32 {
        let out = unsafe { &mut *(l as *mut Vec<Hwnd>) };
        if class_of(h) == "Button" && is_clipboard_label(&text_of(h)) {
            out.push(h);
            return 0; // 找到即停止枚举
        }
        1
    }

    /// 勾上对话框里的「剪贴板」;返回是否找到该复选框
    fn tick_clipboard(dialog: Hwnd) -> bool {
        let mut hits: Vec<Hwnd> = Vec::new();
        unsafe { EnumChildWindows(dialog, on_child, &mut hits as *mut _ as isize) };
        match hits.first() {
            Some(&cb) => {
                // 已勾选就不要再点(BM_CLICK 是切换语义,会把它取消掉)
                if unsafe { SendMessageW(cb, BM_GETCHECK, 0, 0) } == 0 {
                    unsafe { SendMessageW(cb, BM_CLICK, 0, 0) };
                }
                true
            }
            None => false,
        }
    }

    /// 轮询等待目标进程的资源确认对话框并勾选剪贴板
    pub fn allow_clipboard_blocking(pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut scan = Scan { pid, found: Vec::new() };
            unsafe { EnumWindows(on_window, &mut scan as *mut _ as isize) };
            for dlg in scan.found {
                if tick_clipboard(dlg) {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }
}

/// 后台勾选「剪贴板」(仅 Windows;其它平台是空实现)
pub fn allow_clipboard_async(pid: u32) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        // mstsc 弹窗通常 1~5 秒内出现;给 20 秒余量(网络慢时凭据框可能先出现)
        win::allow_clipboard_blocking(pid, Duration::from_secs(20));
    });
    #[cfg(not(windows))]
    let _ = pid;
}

#[cfg(test)]
mod tests {
    use super::is_clipboard_label;

    #[test]
    fn matches_localized_clipboard_labels() {
        assert!(is_clipboard_label("剪贴板(&L)"));
        assert!(is_clipboard_label("剪貼簿"));
        assert!(is_clipboard_label("&Clipboard"));
        assert!(is_clipboard_label("Clipboard"));
        assert!(is_clipboard_label("クリップボード"));
        assert!(is_clipboard_label("Zwischenablage"));
    }

    #[test]
    fn ignores_other_controls() {
        assert!(!is_clipboard_label(""));
        assert!(!is_clipboard_label("打印机(&T)"));
        assert!(!is_clipboard_label("连接(&N)"));
        assert!(!is_clipboard_label("取消(&C)"));
        assert!(!is_clipboard_label("智能卡或 Windows Hello 企业版(&D)"));
        assert!(!is_clipboard_label("Printers"));
    }
}
