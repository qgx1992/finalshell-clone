#!/usr/bin/env python3
"""通过 WebView2 CDP 驱动 FinalShell Clone UI 做端到端测试。

用法::

    python e2e_test.py                 # 冒烟(默认)：只验证 UI 渲染 / 对话框 / 快捷命令，不需要服务器
    python e2e_test.py --full          # 完整：还需要 127.0.0.1:2222 上有 test/test123 的 SSH 测试机
    python e2e_test.py --port 9222     # 指定 WebView2 调试端口(默认 9222)
    python e2e_test.py --url-match 1420  # 只在页面 URL 含该子串时认为命中(默认不限制)

环境变量::

    FINA_E2E_SSH=1        等价于 --full
    FINA_E2E_PORT=9222    等价于 --port
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
                          必须在启动应用前设置，否则拿不到调试端口

退出码：0 表示通过，1 表示失败（失败步骤会打印出来，便于 CI 定位）。
"""

import argparse
import json
import os
import sys
import time
import traceback
import urllib.request

import websocket

# Windows 控制台默认不是 UTF-8，中文日志会变乱码
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

DEFAULT_PORT = 9222

# 本机 CDP 端点必须直连：如果环境里配了 HTTP_PROXY / HTTPS_PROXY，urllib 默认会把
# 127.0.0.1 也丢给代理，结果就是"连接超时"而不是"拒绝连接"，很难查。
_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _http_json(url, timeout=2):
    with _OPENER.open(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


def get_page_ws(port, url_match=None, timeout=30.0):
    """等待 WebView2 暴露调试目标，返回页面的 webSocketDebuggerUrl。"""
    deadline = time.time() + timeout
    last_err = None
    while time.time() < deadline:
        try:
            targets = _http_json(f"http://127.0.0.1:{port}/json")
            pages = [t for t in targets if t.get("type") == "page"]
            for page in pages:
                url = page.get("url") or ""
                if url_match and url_match not in url:
                    continue
                if page.get("webSocketDebuggerUrl"):
                    return page["webSocketDebuggerUrl"]
        except Exception as e:  # 端口还没起来
            last_err = e
        time.sleep(0.5)
    hint = f"（最后一次错误: {last_err}）" if last_err else ""
    raise RuntimeError(
        f"找不到 WebView2 调试页面，端口 {port}{hint}。"
        "请确认启动应用前设置了 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=%d" % port
    )


class CDP:
    def __init__(self, ws_url):
        # suppress_origin: Chromium 的 DevTools 端点会拒掉带 Origin 的 WebSocket 握手
        # (403 Forbidden)，不带 Origin 头才允许，因此必须显式关掉。
        self.ws = websocket.create_connection(ws_url, timeout=10, suppress_origin=True)
        self.id = 0

    def send(self, method, params=None):
        self.id += 1
        mid = self.id
        self.ws.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"CDP error {method}: {msg['error']}")
                return msg.get("result", {})

    def eval_js(self, expr):
        res = self.send(
            "Runtime.evaluate",
            {"expression": expr, "returnByValue": True, "awaitPromise": True},
        )
        if res.get("exceptionDetails"):
            raise RuntimeError(f"JS error: {res['exceptionDetails']}")
        return res.get("result", {}).get("value")

    def wait_for(self, expr, timeout=15, desc=""):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.eval_js(expr):
                return True
            time.sleep(0.3)
        raise RuntimeError(f"等待超时: {desc or expr}")

    def close(self):
        try:
            self.ws.close()
        except Exception:
            pass


def run_smoke(cdp):
    """不需要 SSH 服务器的部分：UI 渲染 + 新建连接对话框 + 快捷命令面板。"""
    print("[1] 检查 UI 渲染...")
    title = cdp.eval_js("document.querySelector('.logo-text')?.textContent")
    print(f"    应用标题: {title}")
    cdp.wait_for("document.querySelector('.btn-primary') !== null", desc="新建连接按钮")
    cdp.wait_for("document.querySelector('.sidebar') !== null", desc="侧边栏")

    print("[2] 打开新建连接对话框...")
    cdp.eval_js("document.querySelector('.header-actions .btn-primary').click()")
    cdp.wait_for("document.querySelector('.modal') !== null", desc="对话框")
    inputs = cdp.eval_js("document.querySelectorAll('.modal input').length")
    print(f"    对话框输入框数量: {inputs}")
    cdp.eval_js(
        "[...document.querySelectorAll('.modal-footer .btn')]"
        ".find(b => !b.classList.contains('btn-primary')).click()"
    )
    cdp.wait_for("document.querySelector('.modal') === null", desc="对话框关闭")

    print("[3] 打开快捷命令面板...")
    cdp.eval_js(
        "[...document.querySelectorAll('.header-actions .btn')]"
        ".find(b => b.textContent.includes('命令')).click()"
    )
    cdp.wait_for("document.querySelector('.qcp-list') !== null", desc="快捷命令面板")
    qcp = cdp.eval_js("document.querySelector('.qcp-list')?.textContent?.slice(0, 80)")
    print(f"    快捷命令: {qcp}")


def run_full(cdp):
    """需要 127.0.0.1:2222 上有 test/test123 SSH 测试机的部分。"""
    print("[4] 填写测试服务器配置...")
    cdp.eval_js(
        """
    (() => {
      const set = (el, v) => {
        const proto = Object.getPrototypeOf(el);
        const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
        setter.call(el, v);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      };
      document.querySelector('.header-actions .btn-primary').click();
      const all = [...document.querySelectorAll('.modal input, .modal textarea')];
      set(all[0], 'Demo 测试机');
      set(all[2], '127.0.0.1');
      set(all[3], '2222');
      set(all[4], 'test');
      set(all[5], 'test123');
      return all.length;
    })()
    """
    )
    time.sleep(0.3)
    print("[5] 保存并双击连接...")
    cdp.eval_js("[...document.querySelectorAll('.modal-footer .btn-primary')].at(-1).click()")
    cdp.wait_for("document.querySelector('.modal') === null", desc="对话框关闭")
    cdp.wait_for("document.querySelectorAll('.sidebar-server').length > 0", desc="服务器出现在列表")
    cdp.eval_js("document.querySelector('.sidebar-server').click()")
    cdp.wait_for("document.querySelector('.tab') !== null", desc="标签页打开")
    cdp.wait_for("document.querySelector('.xterm') !== null", timeout=20, desc="终端渲染")

    print("[6] 输入命令 ls ...")
    for key in ["l", "s", "Enter"]:
        cdp.send("Input.dispatchKeyEvent", {"type": "keyDown", "key": key})
        cdp.send("Input.dispatchKeyEvent", {"type": "keyUp", "key": key})
        time.sleep(0.1)
    time.sleep(1.5)

    print("[7] 检查 SFTP 面板...")
    cdp.wait_for(
        "document.querySelector('.sftp-panel') !== null || document.querySelector('.sftp-collapsed') !== null",
        timeout=15,
        desc="SFTP 面板",
    )
    files = cdp.eval_js(
        "[...document.querySelectorAll('.sftp-row .sftp-name')].map(e => e.textContent).slice(0, 8)"
    )
    print(f"    SFTP 文件列表: {files}")

    print("[8] 检查监控面板...")
    cdp.wait_for("document.querySelector('.monitor-panel') !== null", timeout=20, desc="监控面板")
    info = cdp.eval_js("document.querySelector('.monitor-info')?.textContent")
    print(f"    监控信息: {info}")


def main(argv=None):
    ap = argparse.ArgumentParser(description="FinalShell Clone 端到端测试(WebView2 CDP)")
    ap.add_argument("--full", action="store_true", help="连真实 SSH 服务器跑完整流程")
    ap.add_argument("--port", type=int, default=int(os.environ.get("FINA_E2E_PORT", DEFAULT_PORT)))
    ap.add_argument("--url-match", default=None, help="页面 URL 需包含的子串(可选)")
    args = ap.parse_args(argv)

    full = args.full or os.environ.get("FINA_E2E_SSH") == "1"
    cdp = None
    try:
        print(f"[0] 连接 WebView2 调试端口 {args.port} ...")
        cdp = CDP(get_page_ws(args.port, args.url_match))
        cdp.send("Runtime.enable")
        run_smoke(cdp)
        if full:
            run_full(cdp)
        else:
            print("[跳] 服务器相关步骤未执行（加 --full 或 FINA_E2E_SSH=1 启用）")
        print("=== E2E 测试通过 ===")
        return 0
    except Exception as e:
        print(f"=== E2E 测试失败: {e} ===", file=sys.stderr)
        traceback.print_exc()
        return 1
    finally:
        if cdp:
            cdp.close()


if __name__ == "__main__":
    sys.exit(main())
