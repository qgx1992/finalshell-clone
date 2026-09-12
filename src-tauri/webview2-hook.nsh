; 本文件由 scripts/build-msvc.mjs 在打包前自动生成,请勿手工编辑。
; MSVC 构建静态链接 WebView2,通常无需侧载 DLL;
; 仅当 GNU 构建产物中存在 WebView2Loader.dll 时才一并打包。
!macro NSIS_HOOK_PREINSTALL
  ; 未找到 WebView2Loader.dll,跳过(MSVC 静态链接场景属正常)
!macroend
