//! 远程 Windows 支持:生成临时 .rdp 文件并调用系统自带的 mstsc.exe 拉起 RDP 会话
//! (FinalShell 同款体验:Windows 远程桌面由系统客户端接管,应用只负责编排)

use crate::store::ServerConfig;
use std::fs;
use std::process::Command;

/// 把分辨率配置翻译成 .rdp 的显示相关字段
/// 注意: screen mode id:i:1 = 窗口模式, i:2 = 全屏(微软文档: https://learn.microsoft.com/windows-server/administration/windows-commands/mstsc)
fn rdp_display(res: &str) -> String {
    // 全屏:让 mstsc 铺满本地屏幕(screen mode 2)
    if res == "fullscreen" || res.is_empty() {
        return "screen mode id:i:2\n".into();
    }
    // 自适应窗口:窗口模式 + smart sizing,按窗口大小缩放
    if res == "auto" {
        return "screen mode id:i:1\nsmart sizing:i:1\n".into();
    }
    // 固定分辨率,如 "1920x1080":窗口模式 + 固定宽高
    if let Some((w, h)) = res.split_once('x') {
        if let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
            return format!(
                "screen mode id:i:1\ndesktopwidth:i:{w}\ndesktopheight:i:{h}\n"
            );
        }
    }
    // 无法解析则回退全屏
    "screen mode id:i:2\n".into()
}

/// 拉起 RDP:写入临时 .rdp 文件后调用 mstsc.exe(系统自带)
#[tauri::command]
pub fn connect_rdp(server: ServerConfig) -> Result<(), String> {
    let addr = format!("{}:{}", server.host, server.port);
    // 最小化 .rdp 内容:full address + username + 显示配置;密码由 mstsc 登录框输入
    // (明文密码无法安全写入 .rdp,mstsc 也不接受明文密码字段)
    let rdp_content = format!(
        "full address:s:{}\nusername:s:{}\n{}prompt for credentials:i:1\nauthentication level:i:0\n",
        addr,
        server.username,
        rdp_display(&server.rdp_resolution),
    );
    let rdp_path = std::env::temp_dir().join(format!("finalshell-x-{}.rdp", server.id));
    fs::write(&rdp_path, rdp_content).map_err(|e| format!("写入 .rdp 失败: {e}"))?;

    Command::new("mstsc.exe")
        .arg(&rdp_path)
        .spawn()
        .map_err(|e| format!("启动 mstsc 失败: {e}"))?;

    Ok(())
}
