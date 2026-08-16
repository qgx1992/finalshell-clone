mod monitor;
mod ssh;
mod store;

use ssh::manager::{close_session_channel, connect_server, disconnect, exec_command, list_sessions, resize_terminal, write_terminal};
use ssh::sftp::{list_dir, sftp_delete, sftp_download, sftp_mkdir, sftp_pwd, sftp_read_file, sftp_rename, sftp_upload, sftp_write_file};
use ssh::SshManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
