; NSIS 钩子:把 WebView2Loader.dll 复制进安装目录
; (GNU 工具链下 exe 动态链接该 DLL,tauri-bundler 默认漏打包)
; 注意:路径必须与构建时项目所在位置一致(tauri-bundler 不提供 PROJECTDIR 变量,
; 只能写死构建机绝对路径;项目路径含空格时必须加引号)。
!macro NSIS_HOOK_PREINSTALL
  File "D:\A-my project\finalshell-clone\src-tauri\target\release\WebView2Loader.dll"
!macroend
