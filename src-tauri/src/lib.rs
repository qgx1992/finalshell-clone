mod error;
mod monitor;
mod rdp;
mod rdp_consent;
mod security;
mod ssh;
mod store;

#[cfg(test)]
mod typegen;

use security::credentials::SecretCache;
use ssh::manager::{
    close_session_channel, connect_server, disconnect, exec_command, forget_host_key,
    list_sessions, resize_terminal, trusted_host_fingerprint, write_terminal,
};
use ssh::archive::{sftp_archive_preview, sftp_create_archive};
use ssh::sftp::{
    list_dir, sftp_delete, sftp_download, sftp_mkdir, sftp_pwd, sftp_read_file, sftp_rename,
    sftp_upload, sftp_write_file,
};
use ssh::SshManager;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

/// 初始化文件日志。日志写入应用日志目录,用于排查线上问题;不记录任何凭据。
fn init_logging(app: &tauri::AppHandle) {
    use tracing_subscriber::EnvFilter;

    let dir = app
        .path()
        .app_log_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("finalshell-x-logs"));
    let _ = std::fs::create_dir_all(&dir);
    let file_appender = tracing_appender::rolling::daily(&dir, "finalshell-x.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // guard 存活期间才会持续写盘,释放即停止
    std::mem::forget(guard);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("finalshell_x_lib=info,warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .try_init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            init_logging(app.handle());
            // 清理上次运行遗留的临时 .rdp 文件
            rdp::cleanup_temp_files();
            // 旧版本明文凭据迁移:写入系统凭据库后清空 JSON 字段
            let store = store::Store::new(app.handle());
            match store.migrate_plaintext_secrets() {
                Ok(0) => {}
                Ok(n) => tracing::info!(count = n, "migrated plaintext secrets to OS keyring"),
                Err(e) => tracing::error!(error = %e, "failed to migrate plaintext secrets"),
            }
            Ok(())
        })
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
        .manage(SecretCache::default())
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
            // 主机信任管理
            trusted_host_fingerprint,
            forget_host_key,
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
            // 压缩包
            sftp_archive_preview,
            sftp_create_archive,
            // 监控
            monitor::monitor_poll,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
