#!/usr/bin/env python3
"""通过 WebView2 CDP 驱动 FinalShell Clone UI 做端到端测试"""
import json
import sys
import time
import urllib.request
import websocket

DEBUG_PORT = 9222

def get_page_ws():
    for _ in range(30):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{DEBUG_PORT}/json", timeout=2) as r:
                pages = json.loads(r.read().decode())
            for p in pages:
                if p.get("type") == "page" and "finalshell" in (p.get("url") or "").lower() or p.get("type") == "page":
                    if p.get("url") and "1420" in p["url"]:
                        return p["webSocketDebuggerUrl"]
        except Exception:
            pass
        time.sleep(0.5)
    raise RuntimeError("找不到 WebView2 调试页面")

class CDP:
    def __init__(self, ws_url):
        self.ws = websocket.create_connection(ws_url, timeout=10)
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
        res = self.send("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True})
        v = res.get("result", {}).get("value")
        if res.get("exceptionDetails"):
            raise RuntimeError(f"JS error: {res['exceptionDetails']}")
        return v
    def wait_for(self, expr, timeout=15, desc=""):
        deadline = time.time() + timeout
        while time.time() < deadline:
            v = self.eval_js(expr)
            if v:
                return v
            time.sleep(0.3)
        raise RuntimeError(f"等待超时: {desc or expr}")

def main():
    print("[1] 获取调试页面...")
    ws_url = get_page_ws()
    cdp = CDP(ws_url)
    cdp.send("Runtime.enable")
    print("[2] 页面已连接,检查 UI 渲染...")
    title = cdp.eval_js("document.querySelector('.logo-text')?.textContent")
    print(f"    应用标题: {title}")
    cdp.wait_for("document.querySelector('.btn-primary') !== null", desc="新建连接按钮")

    print("[3] 打开新建连接对话框...")
    cdp.eval_js("document.querySelector('.header-actions .btn-primary').click()")
    cdp.wait_for("document.querySelector('.modal') !== null", desc="对话框")

    print("[4] 填写测试服务器配置...")
    # 名称
    cdp.eval_js("""
    (() => {
      const inputs = document.querySelectorAll('.modal input');
      const set = (el, v) => {
        const proto = Object.getPrototypeOf(el);
        const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
        setter.call(el, v);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      };
      // 名称(第1个input)、主机(第3个)、端口(第4个)、用户名(第5个)、密码(第6个)
      const all = [...document.querySelectorAll('.modal input, .modal textarea')];
      set(all[0], 'Demo 测试机');
      set(all[2], '127.0.0.1');
      set(all[3], '2222');
      set(all[4], 'test');
      set(all[5], 'test123');
      return all.length;
    })()
    """)
    time.sleep(0.3)
    print("[5] 点击保存...")
    cdp.eval_js("[...document.querySelectorAll('.modal-footer .btn-primary')].at(-1).click()")
    cdp.wait_for("document.querySelector('.modal') === null", desc="对话框关闭")
    cdp.wait_for("document.querySelectorAll('.sidebar-server').length > 0", desc="服务器出现在列表")
    print("    服务器列表:", cdp.eval_js("[...document.querySelectorAll('.sv-name')].map(e=>e.textContent)"))

    print("[6] 双击连接服务器...")
    cdp.eval_js("document.querySelector('.sidebar-server').dispatchEvent(new MouseEvent('dblclick', {bubbles: true}))")
    # 连接是单击。用 click
    cdp.eval_js("document.querySelector('.sidebar-server').click()")
    cdp.wait_for("document.querySelector('.tab') !== null", desc="标签页打开")
    print("    标签页:", cdp.eval_js("[...document.querySelectorAll('.tab-title')].map(e=>e.textContent)"))

    print("[7] 等待终端连接成功...")
    cdp.wait_for("[...document.querySelectorAll('.tab')].some(t => t.textContent.includes('🟢')) || document.querySelector('.xterm') !== null", timeout=20, desc="终端就绪")
    # 终端输出出现(banner)
    cdp.wait_for("document.querySelector('.xterm') !== null", desc="xterm 渲染")

    print("[8] 向终端输入命令 ls ...")
    # 通过 CDP 发送键盘事件到 xterm? 直接调用内部 API 更稳:模拟按键
    # xterm 渲染在 canvas,CDP Input.dispatchKeyEvent 是物理按键,发给 webview
    # 简单方案:通过 xterm 的 onData?不行。用 CDP Input.dispatchKeyEvent 模拟真实键盘
    # 先获取终端尺寸
    dims = cdp.eval_js("({cols: document.querySelector('.xterm-rows')?.childElementCount, hasTerm: !!document.querySelector('.xterm')})")
    print("    终端状态:", dims)
    # 发按键:l s Enter
    for key in ["l", "s", "Enter"]:
        cdp.send("Input.dispatchKeyEvent", {"type": "keyDown", "key": key})
        cdp.send("Input.dispatchKeyEvent", {"type": "keyUp", "key": key})
        time.sleep(0.1)
    time.sleep(1.5)
    # 检查 SFTP 面板出现(连接成功后 sessionId 就绪,SftpPanel 渲染)
    print("[9] 检查 SFTP 面板...")
    try:
        cdp.wait_for("document.querySelector('.sftp-panel') !== null || document.querySelector('.sftp-collapsed') !== null", timeout=10, desc="SFTP 面板")
        files = cdp.eval_js("[...document.querySelectorAll('.sftp-row .sftp-name')].map(e=>e.textContent).slice(0,8)")
        print("    SFTP 文件列表:", files)
    except Exception as e:
        print("    SFTP 面板未出现:", e)

    print("[10] 检查监控面板...")
    try:
        cdp.wait_for("document.querySelector('.monitor-panel') !== null", timeout=12, desc="监控面板")
        info = cdp.eval_js("document.querySelector('.monitor-info')?.textContent")
        print("    监控信息:", info)
    except Exception as e:
        print("    监控面板未出现:", e)

    print("[11] 打开快捷命令面板...")
    cdp.eval_js("[...document.querySelectorAll('.header-actions .btn')].find(b=>b.textContent.includes('命令')).click()")
    time.sleep(0.5)
    qcp = cdp.eval_js("document.querySelector('.qcp-list')?.textContent?.slice(0,100)")
    print("    快捷命令:", qcp)

    print("=== E2E 测试完成 ===")
    cdp.ws.close()

if __name__ == "__main__":
    main()
