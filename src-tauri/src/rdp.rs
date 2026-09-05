//! 远程 Windows 支持:生成临时 .rdp 文件并调用系统自带的 mstsc.exe 拉起 RDP 会话
//! (FinalShell 同款体验:Windows 远程桌面由系统客户端接管,应用只负责编排)

use crate::store::ServerConfig;
use std::fs;
use std::process::Command;

use crate::rdp_consent;

/// 窗口边框 + 标题栏的估算尺寸(96dpi 下 Win10/11 典型值)。
/// mstsc 通常会自行把客户区调整为会话分辨率,这里只保证 winposstr 的矩形不小于会话。
const CHROME_W: u32 = 16;
const CHROME_H: u32 = 39;

/// 显示模式
#[derive(Debug, PartialEq)]
pub enum Display {
    /// 全屏(铺满本地屏幕)
    FullScreen,
    /// 窗口 + 随窗口缩放
    Auto,
    /// 固定分辨率窗口
    Fixed(u32, u32),
}

/// 解析配置里的分辨率字符串: "fullscreen" | "auto" | "1920x1080"
pub fn parse_resolution(res: &str) -> Display {
    match res {
        "" | "fullscreen" => Display::FullScreen,
        "auto" => Display::Auto,
        other => other
            .split_once('x')
            .and_then(|(w, h)| {
                let w = w.trim().parse::<u32>().ok()?;
                let h = h.trim().parse::<u32>().ok()?;
                if w == 0 || h == 0 {
                    return None;
                }
                Some(Display::Fixed(w, h))
            })
            // 无法解析则回退全屏
            .unwrap_or(Display::FullScreen),
    }
}

/// 显示相关的 .rdp 字段。
///
/// 关键点(踩过的坑):
/// - `screen mode id`:**1 = 窗口模式,2 = 全屏**(不是反过来)。
/// - `dynamic resolution` 默认为 1,会在窗口大小变化时重新协商会话分辨率 ——
///   固定分辨率必须显式设为 0,否则用户一拖动窗口分辨率就被改掉。
/// - `winposstr:s:0,ShowCmd,Left,Top,Right,Bottom`,ShowCmd=1 普通窗口 / 3 最大化。
///   不给 winposstr 时 mstsc 会沿用 Default.rdp 的状态(常常是最大化),表现为"贴顶";
///   而从最大化状态往下拖会还原成很小的窗口 —— 所以固定分辨率时显式给出居中的普通窗口。
fn rdp_display(d: &Display, work_area: Option<(i32, i32, u32, u32)>) -> String {
    match *d {
        Display::FullScreen => "screen mode id:i:2\nsmart sizing:i:0\ndynamic resolution:i:1\n".into(),
        Display::Auto => "screen mode id:i:1\nsmart sizing:i:1\ndynamic resolution:i:1\n".into(),
        Display::Fixed(w, h) => {
            let mut s = format!(
                "screen mode id:i:1\ndesktopwidth:i:{w}\ndesktopheight:i:{h}\nsmart sizing:i:0\ndynamic resolution:i:0\n"
            );
            let (wa_x, wa_y, wa_w, wa_h) = work_area.unwrap_or((0, 0, w + CHROME_W, h + CHROME_H));
            let win_w = w + CHROME_W;
            let win_h = h + CHROME_H;
            // 居中的普通窗口;屏幕装不下时贴工作区左上角,至少保证标题栏可见可拖动
            let left = (wa_x + ((wa_w.saturating_sub(win_w)) / 2) as i32).max(wa_x);
            let top = (wa_y + ((wa_h.saturating_sub(win_h)) / 2) as i32).max(wa_y);
            s.push_str(&format!(
                "winposstr:s:0,1,{left},{top},{},{}\n",
                left + win_w as i32,
                top + win_h as i32
            ));
            s
        }
    }
}

/// mstsc 命令行的窗口尺寸开关(与 .rdp 文件里的字段双保险)
/// 微软文档: mstsc.exe [<connectionfile>] ... [/w:<width> /h:<height>]
fn size_args(d: &Display) -> Vec<String> {
    match *d {
        Display::Fixed(w, h) => vec![format!("/w:{w}"), format!("/h:{h}")],
        _ => Vec::new(),
    }
}

/// 拼出完整的 .rdp 内容(可单测)
fn build_rdp(server: &ServerConfig, work_area: Option<(i32, i32, u32, u32)>) -> String {
    // 最小化 .rdp 内容:full address + username + 显示/重定向配置;密码由 mstsc 登录框输入
    // (明文密码无法安全写入 .rdp,mstsc 也不接受明文密码字段)
    // redirectclipboard 显式写出,不依赖客户端默认值
    format!(
        "full address:s:{}:{}\nusername:s:{}\nuse multimon:i:0\n{}redirectclipboard:i:{}\nprompt for credentials:i:1\nauthentication level:i:0\n",
        server.host,
        server.port,
        server.username,
        rdp_display(&parse_resolution(&server.rdp_resolution), work_area),
        if server.rdp_clipboard { 1 } else { 0 },
    )
}

/// 拉起 RDP:写入临时 .rdp 文件后调用 mstsc.exe(系统自带)
#[tauri::command]
pub fn connect_rdp(app: tauri::AppHandle, server: ServerConfig) -> Result<(), String> {
    let display = parse_resolution(&server.rdp_resolution);
    // 主显示器工作区(排除任务栏),用于把固定分辨率窗口居中
    let work_area = app.primary_monitor().ok().flatten().map(|m| {
        let wa = m.work_area();
        (wa.position.x, wa.position.y, wa.size.width, wa.size.height)
    });
    let rdp_path = std::env::temp_dir().join(format!("finalshell-x-{}.rdp", server.id));
    fs::write(&rdp_path, build_rdp(&server, work_area))
        .map_err(|e| format!("写入 .rdp 失败: {e}"))?;

    let child = Command::new("mstsc.exe")
        .arg(&rdp_path)
        .args(size_args(&display))
        .spawn()
        .map_err(|e| format!("启动 mstsc 失败: {e}"))?;

    // Windows 11 的资源确认弹窗里「剪贴板」默认不勾选,用户直接点连接就等于不共享;
    // 开了共享剪贴板时帮用户把那个复选框勾上(不代替用户点「连接」)
    if server.rdp_clipboard {
        rdp_consent::allow_clipboard_async(child.id());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_rdp, parse_resolution, rdp_display, size_args, Display};
    use crate::store::ServerConfig;

    const WA: Option<(i32, i32, u32, u32)> = Some((0, 0, 2560, 1392));

    fn render(res: &str) -> String {
        rdp_display(&parse_resolution(res), WA)
    }

    #[test]
    fn parses_resolution_values() {
        assert_eq!(parse_resolution("fullscreen"), Display::FullScreen);
        assert_eq!(parse_resolution(""), Display::FullScreen);
        assert_eq!(parse_resolution("auto"), Display::Auto);
        assert_eq!(parse_resolution("1920x1080"), Display::Fixed(1920, 1080));
        // 非法输入回退全屏
        assert_eq!(parse_resolution("garbage"), Display::FullScreen);
        assert_eq!(parse_resolution("0x0"), Display::FullScreen);
    }

    #[test]
    fn fullscreen_uses_screen_mode_2() {
        let s = render("fullscreen");
        assert!(s.contains("screen mode id:i:2"));
        assert!(!s.contains("winposstr"));
    }

    #[test]
    fn auto_is_windowed_with_smart_sizing() {
        let s = render("auto");
        assert!(s.contains("screen mode id:i:1"));
        assert!(s.contains("smart sizing:i:1"));
        assert!(s.contains("dynamic resolution:i:1"));
    }

    #[test]
    fn fixed_resolution_is_static_windowed_and_centered() {
        let s = render("1920x1080");
        assert!(s.contains("screen mode id:i:1"));
        assert!(s.contains("desktopwidth:i:1920"));
        assert!(s.contains("desktopheight:i:1080"));
        // 拖动/缩放窗口不再改变会话分辨率
        assert!(s.contains("dynamic resolution:i:0"));
        assert!(s.contains("smart sizing:i:0"));
        // ShowCmd=1 普通窗口(不最大化),且在 2560x1392 工作区内居中
        let left = (2560 - (1920 + 16)) / 2;
        let top = (1392 - (1080 + 39)) / 2;
        assert!(
            s.contains(&format!(
                "winposstr:s:0,1,{left},{top},{},{}",
                left + 1936,
                top + 1119
            )),
            "winposstr 不符: {s}"
        );
    }

    #[test]
    fn oversized_resolution_clamps_to_work_area_origin() {
        // 会话比屏幕还大:窗口贴工作区左上角,保证标题栏可见
        assert!(render("3840x2160").contains("winposstr:s:0,1,0,0,"));
    }

    #[test]
    fn only_fixed_resolution_passes_size_switches() {
        assert_eq!(size_args(&Display::Fixed(1600, 900)), vec!["/w:1600", "/h:900"]);
        assert!(size_args(&Display::FullScreen).is_empty());
        assert!(size_args(&Display::Auto).is_empty());
    }

    #[test]
    fn clipboard_is_shared_by_default() {
        // ServerConfig::default() 代表新建/旧配置补默认值后的状态
        let mut s = ServerConfig::default();
        s.host = "10.0.0.9".into();
        s.port = 3389;
        s.username = "Administrator".into();
        assert!(s.rdp_clipboard, "剪贴板共享应默认开启");

        let out = build_rdp(&s, None);
        assert!(out.contains("redirectclipboard:i:1"), "{out}");
        assert!(out.contains("full address:s:10.0.0.9:3389"), "{out}");
        assert!(out.contains("username:s:Administrator"), "{out}");

        s.rdp_clipboard = false;
        assert!(build_rdp(&s, None).contains("redirectclipboard:i:0"));
    }
}
