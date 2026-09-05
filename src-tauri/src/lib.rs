mod monitor;
mod rdp;
mod ssh;
mod store;

use ssh::manager::{close_session_channel, connect_server, disconnect, exec_command, list_sessions, resize_terminal, write_terminal};
use ssh::sftp::{list_dir, sftp_delete, sftp_download, sftp_mkdir, sftp_pwd, sftp_read_file, sftp_rename, sftp_upload, sftp_write_file};
use ssh::SshManager;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // 关闭窗口前弹确认框:防止误点 × 丢失所有会话
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    app.dialog()
                        .message("确定要退出 FinalShell-X 吗？\n所有连接会话将断开。")
                        .title("确认退出")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                        .buttons(MessageDialogButtons::OkCancelCustom("退出".into(), "取消".into()))
                        .show(move |confirmed| {
                            if confirmed {
                                app.exit(0);
                            }
                        });
                });
            }
        })
        .manage(SshManager::new())
        .invoke_handler(tauri::generate_handler![
            // 配置存储
            store::list_servers,
            store::save_server,
            store::delete_server,
            store::get_settings,
            store::save_settings,
            // SSH 会话
            connect_server,
            disconnect,
            write_terminal,
            resize_terminal,
            exec_command,
            list_sessions,
            close_session_channel,
            // 远程 Windows (RDP)
            rdp::connect_rdp,
            // SFTP
            list_dir,
            sftp_mkdir,
            sftp_delete,
            sftp_rename,
            sftp_upload,
            sftp_download,
            sftp_read_file,
            sftp_write_file,
            sftp_pwd,
            // 监控
            monitor::monitor_poll,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
