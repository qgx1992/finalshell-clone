//! 远程 Windows 支持:生成临时 .rdp 文件并调用系统自带的 mstsc.exe 拉起 RDP 会话
//! (FinalShell 同款体验:Windows 远程桌面由系统客户端接管,应用只负责编排)

use crate::store::ServerConfig;
use std::fs;
use std::process::Command;

/// 拉起 RDP:写入临时 .rdp 文件后调用 mstsc.exe(系统自带)
#[tauri::command]
pub fn connect_rdp(server: ServerConfig) -> Result<(), String> {
    let addr = format!("{}:{}", server.host, server.port);
    // 最小化 .rdp 内容:full address + username;密码由 mstsc 登录框输入
    // (明文密码无法安全写入 .rdp,mstsc 也不接受明文密码字段)
    let rdp_content = format!(
        "full address:s:{}\nusername:s:{}\nscreen mode id:i:1\nprompt for credentials:i:1\nauthentication level:i:0\n",
        addr, server.username
    );
    let rdp_path = std::env::temp_dir().join(format!("finalshell-x-{}.rdp", server.id));
    fs::write(&rdp_path, rdp_content).map_err(|e| format!("写入 .rdp 失败: {e}"))?;

    Command::new("mstsc.exe")
        .arg(&rdp_path)
        .spawn()
        .map_err(|e| format!("启动 mstsc 失败: {e}"))?;

    Ok(())
}
