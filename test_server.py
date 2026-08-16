#!/usr/bin/env python3
"""FinalShell Clone 端到端测试服务器
- 监听 127.0.0.1:2222
- 账号 test / 密码 test123
- 迷你 Linux shell 模拟(支持 pwd/cd/ls/cat/echo 等 + 模拟 /proc 监控数据)
- SFTP server 基于本地临时目录
"""
import os
import socket
import sys
import threading
import time
import paramiko
from paramiko import ServerInterface, SFTPServerInterface, SFTPAttributes, SFTPHandle, SFTP_OK
from paramiko import AUTH_SUCCESSFUL, AUTH_FAILED, OPEN_SUCCEEDED, OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
import errno

HOST = "127.0.0.1"
PORT = 2222
USER = "test"
PASS = "test123"
SFTP_ROOT = os.path.join(os.environ.get("TEMP", "/tmp"), "fstest_sftp")

HOST_KEY = os.path.join(os.environ.get("TEMP", "/tmp"), "fstest_hostkey")

os.makedirs(SFTP_ROOT, exist_ok=True)
# 准备模拟文件树
os.makedirs(os.path.join(SFTP_ROOT, "etc"), exist_ok=True)
os.makedirs(os.path.join(SFTP_ROOT, "home", "test"), exist_ok=True)
os.makedirs(os.path.join(SFTP_ROOT, "var", "log"), exist_ok=True)
os.makedirs(os.path.join(SFTP_ROOT, "opt", "apps"), exist_ok=True)
with open(os.path.join(SFTP_ROOT, "etc", "os-release"), "w", encoding="utf-8") as f:
    f.write('NAME="TestOS"\nVERSION="1.0 (demo)"\nID=testos\n')
with open(os.path.join(SFTP_ROOT, "etc", "hostname"), "w", encoding="utf-8") as f:
    f.write("finalshell-demo\n")
with open(os.path.join(SFTP_ROOT, "etc", "motd"), "w", encoding="utf-8") as f:
    f.write("Welcome to FinalShell Clone demo server!\n")
with open(os.path.join(SFTP_ROOT, "home", "test", "hello.txt"), "w", encoding="utf-8") as f:
    f.write("Hello from the demo server!\n")
with open(os.path.join(SFTP_ROOT, "home", "test", "app.log"), "w", encoding="utf-8") as f:
    f.write("\n".join(f"2026-08-12 10:{i:02d}:00 INFO request ok" for i in range(60)))
with open(os.path.join(SFTP_ROOT, "opt", "apps", "deploy.sh"), "w", encoding="utf-8") as f:
    f.write("#!/bin/bash\n# demo deploy script\necho deploying...\n")

# ---------------- 模拟 /proc 数据 ----------------
def gen_proc_stat():
    # 模拟 4 核 CPU,总 idle 每次采样递增(产生平滑的 CPU 使用曲线)
    t = time.time()
    user = 80000 + int(t * 10) % 5000
    nice = 0
    system = 30000 + int(t * 5) % 2000
    idle = 900000 + int(t * 120) % 20000
    iowait = 1000
    irq = 200
    softirq = 800
    steal = 0
    line = f"cpu  {user} {nice} {system} {idle} {iowait} {irq} {softirq} {steal} 0 0"
    out = [line]
    for i in range(4):
        u = user // 4 + i * 100
        s = system // 4 + i * 50
        idl = idle // 4
        out.append(f"cpu{i}  {u} {nice} {s} {idl} {iowait//4} {irq//4} {softirq//4} 0 0 0")
    return "\n".join(out) + "\n"

def gen_proc_meminfo():
    total_kb = 16 * 1024 * 1024  # 16GB
    used_kb = int(total_kb * (0.42 + 0.05 * ((time.time() // 10) % 5)))
    avail = total_kb - used_kb
    swap_total = 4 * 1024 * 1024
    swap_free = swap_total - 300 * 1024
    return (
        f"MemTotal:      {total_kb} kB\n"
        f"MemFree:       {avail - 500000} kB\n"
        f"MemAvailable:  {avail} kB\n"
        f"Buffers:        210000 kB\n"
        f"Cached:        3200000 kB\n"
        f"SwapTotal:     {swap_total} kB\n"
        f"SwapFree:      {swap_free} kB\n"
    )

def gen_proc_net_dev():
    t = time.time()
    rx = int(t * 100000) % 500000000
    tx = int(t * 60000) % 200000000
    return (
        "Inter-|   Receive                                                |  Transmit\n"
        " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n"
        f"  eth0: {rx}  1000000    0    0    0     0          0         0 {tx}   600000    0    0    0     0       0          0\n"
        f"    lo:  120000   20000    0    0    0     0          0         0   120000   20000    0    0    0     0       0          0\n"
    )

def gen_proc_loadavg():
    return f"{0.35 + (time.time() % 3) / 10:.2f} {0.60:.2f} {0.55:.2f} 2/345 12345\n"

def gen_proc_uptime():
    return f"{123456.78} {98765.43}\n"

def gen_df():
    return (
        "Filesystem     1024-blocks      Used Available Capacity Mounted on\n"
        "overlay         20480000   6123456  14356544      30% /\n"
        "tmpfs            10240000       1024  10238976       1% /dev/shm\n"
        "tmpfs              512000       5120    506880       2% /run\n"
        "/dev/vda2       51392000  12345678  39046322      25% /data\n"
    )

# ---------------- 迷你 shell ----------------
class MiniShell:
    """模拟一个 Linux shell,支持常用命令 + 虚拟 /proc"""
    def __init__(self, root):
        self.root = root
        self.cwd = "/"
        self.running = True
        self.history = []

    def real(self, path):
        if path == "/":
            return self.root
        return os.path.normpath(os.path.join(self.root, path.lstrip("/")))

    def handle(self, line):
        line = line.strip()
        if not line:
            return ""
        self.history.append(line)
        if line in ("exit", "logout"):
            self.running = False
            return "logout\n"
        parts = line.split()
        cmd = parts[0]
        args = parts[1:]
        out = []
        try:
            if cmd == "pwd":
                out.append(self.cwd + "\n")
            elif cmd == "cd":
                target = args[0] if args else "/"
                if target == "..":
                    self.cwd = "/" if self.cwd == "/" else self.cwd.rsplit("/", 1)[0] or "/"
                elif target.startswith("/"):
                    r = self.real(target)
                    if os.path.isdir(r):
                        self.cwd = target
                    else:
                        out.append(f"cd: no such file or directory: {target}\n")
                else:
                    r = os.path.join(self.real(self.cwd), target)
                    if os.path.isdir(r):
                        self.cwd = os.path.normpath(self.cwd + "/" + target)
                    else:
                        out.append(f"cd: no such file or directory: {target}\n")
            elif cmd == "ls":
                path = args[-1] if args and not args[-1].startswith("-") else "."
                r = self.real(os.path.join(self.cwd, path) if not path.startswith("/") else path)
                try:
                    entries = sorted(os.listdir(r))
                    out.append("  ".join(entries) + "\n")
                except OSError:
                    out.append(f"ls: cannot access '{path}': No such file or directory\n")
            elif cmd in ("cat", "head"):
                for p in args:
                    if p.startswith("/proc/"):
                        out.append(self.proc_read(p))
                        continue
                    full = p if p.startswith("/") else os.path.join(self.cwd, p)
                    r = self.real(full)
                    try:
                        with open(r, "r", encoding="utf-8", errors="replace") as f:
                            data = f.read()
                        out.append(data)
                    except OSError:
                        out.append(f"cat: {p}: No such file or directory\n")
            elif cmd == "echo":
                text = " ".join(args)
                text = text.replace("'", "").replace('"', "")
                out.append(text + "\n")
            elif cmd == "uname":
                out.append("Linux finalshell-demo 5.15.0-91-generic #101-Ubuntu SMP x86_64 GNU/Linux\n")
            elif cmd == "hostname":
                out.append("finalshell-demo\n")
            elif cmd == "nproc":
                out.append("4\n")
            elif cmd == "uptime":
                out.append(" 10:00:00 up 1 day,  2:30,  2 users,  load average: 0.35, 0.60, 0.55\n")
            elif cmd == "df":
                out.append(gen_df())
            elif cmd == "free":
                out.append("              total        used        free      shared  buff/cache   available\n")
                out.append("Mem:       16777216     7046400     2800000      120000     6930816     9730816\n")
                out.append("Swap:       4194304      307200     3887104\n")
            elif cmd == "ss":
                out.append("State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n")
                out.append("LISTEN 0      128          0.0.0.0:80        0.0.0.0:*    users:((nginx))\n")
                out.append("LISTEN 0      128          0.0.0.0:22        0.0.0.0:*    users:((sshd))\n")
                out.append("LISTEN 0      511          0.0.0.0:443       0.0.0.0:*    users:((nginx))\n")
            elif cmd == "top":
                out.append("top - 10:00:00 up 1 day,  2 users,  load average: 0.35, 0.60, 0.55\n")
                out.append("Tasks: 345 total,   1 running, 344 sleeping,   0 stopped,   0 zombie\n")
                out.append("%Cpu(s): 12.5 us,  4.2 sy,  0.0 ni, 82.3 id,  0.8 wa\n")
                out.append("MiB Mem :  16384.0 total,   2734.4 free,   6883.2 used\n")
                out.append("  PID USER      PR  NI    VIRT    RES    SHR S  %CPU  %MEM     TIME+ COMMAND\n")
                out.append(" 1234 root      20   0  1200.0m 512.0m 128.0m S   8.0   3.1  10:00.00 node\n")
                out.append("  567 test      20   0   400.0m 180.0m  64.0m S   3.0   1.1   2:00.00 python\n")
            elif cmd == "du":
                out.append("120M\t./logs\n48M\t./apps\n2.4M\t./config\n")
            elif cmd == "help":
                out.append("可用命令: pwd, cd, ls, cat, head, echo, uname, hostname, nproc, uptime, df, free, ss, top, du, help, exit\n")
            else:
                out.append(f"bash: {cmd}: command not found (demo shell)\n")
        except Exception as e:
            out.append(f"error: {e}\n")
        return "".join(out)

    def handle_pipe(self, line):
        """支持 | 管道(简化实现)"""
        if "|" not in line:
            return self.handle(line)
        segs = [s.strip() for s in line.split("|") if s.strip()]
        if not segs:
            return ""
        out = self.handle(segs[0])
        for seg in segs[1:]:
            parts = seg.split()
            if not parts:
                continue
            op = parts[0]
            if op == "head":
                n = 10
                for p in parts[1:]:
                    if p.lstrip("-").isdigit():
                        n = int(p.lstrip("-"))
                lines = out.splitlines()
                out = "\n".join(lines[:n]) + ("\n" if lines else "")
            elif op == "grep":
                opts = [p for p in parts[1:] if p.startswith("-")]
                rest = [p for p in parts[1:] if not p.startswith("-")]
                pat = rest[0] if rest else ""
                if pat.startswith("^"):
                    pat = pat[1:]
                if len(rest) > 1 and rest[1] == "/proc/cpuinfo":
                    lines = self.proc_read("/proc/cpuinfo").splitlines()
                else:
                    lines = out.splitlines()
                matched = [l for l in lines if pat in l]
                if "-c" in opts:
                    out = f"{len(matched)}\n"
                else:
                    out = "\n".join(matched) + "\n"
            elif op == "wc":
                out = f"{len(out.splitlines())} {len(out.split())} {len(out)}\n"
            elif op == "sort":
                out = "\n".join(sorted(out.splitlines())) + "\n"
            else:
                out = self.handle(" ".join(parts))
        return out

    def handle_line(self, line):
        """支持 ; 和 || 分隔多条命令"""
        results = []
        for part in line.split(";"):
            for sub in part.split("||"):
                sub = sub.strip()
                if not sub:
                    continue
                results.append(self.handle_pipe(sub))
        return "".join(results)

    def proc_read(self, path):
        """返回模拟的 /proc 虚拟文件内容"""
        name = path.rstrip("/").split("/")[-1]
        if name == "stat":
            return gen_proc_stat()
        if name == "meminfo":
            return gen_proc_meminfo()
        if name == "dev":
            return gen_proc_net_dev()
        if name == "loadavg":
            return gen_proc_loadavg()
        if name == "uptime":
            return gen_proc_uptime()
        if name == "cpuinfo":
            return "processor\t: 0\nmodel name\t: Test CPU\ncpu cores\t: 4\n"
        return f"cat: {path}: No such file or directory\n"

    def run(self, chan):
        chan.send("Last login: Fri Aug 12 09:00:00 2026 from 127.0.0.1\n")
        chan.send(f"test@finalshell-demo:~$ ")
        buf = b""
        while self.running:
            if not chan.recv_ready():
                time.sleep(0.02)
                continue
            data = chan.recv(1024)
            if not data:
                break
            for b in data:
                if b == 3:  # Ctrl+C
                    chan.send("^C\n")
                    chan.send(f"test@finalshell-demo:{self.cwd}$ ")
                    buf = b""
                elif b in (13, 10):  # Enter (CR 或 LF)
                    line = buf.decode("utf-8", "replace").strip("\r\n")
                    print(f"[demo] shell input: {line!r}", flush=True)
                    chan.send("\r\n")
                    resp = self.handle_line(line)
                    if resp:
                        chan.send(resp)
                    if self.running:
                        chan.send(f"test@finalshell-demo:{self.cwd}$ ")
                    buf = b""
                elif b == 127 or b == 8:  # Backspace
                    if buf:
                        buf = buf[:-1]
                        chan.send("\b \b")
                else:
                    buf += bytes([b])
                    chan.send(bytes([b]))
        chan.close()

# ---------------- SFTP ----------------
def sftp_map(path):
    if path in ("/", "."):
        return SFTP_ROOT
    if path.startswith("/"):
        return os.path.normpath(os.path.join(SFTP_ROOT, path.lstrip("/")))
    return os.path.normpath(os.path.join(SFTP_ROOT, path))

class MiniSFTPHandle(SFTPHandle):
    def write(self, offset, data):
        print(f"[sftp] handle.write offset={offset} len={len(data)}", flush=True)
        return super().write(offset, data)
    def read(self, offset, length):
        print(f"[sftp] handle.read offset={offset} len={length}", flush=True)
        return super().read(offset, length)
    def stat(self):
        try:
            return SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError:
            return None
    def chattr(self, attr):
        return SFTP_OK

class MiniSFTP(SFTPServerInterface):
    def __init__(self, server, *args, **kwargs):
        super().__init__(server, *args, **kwargs)

    def list_folder(self, path):
        real = sftp_map(path)
        try:
            out = []
            for name in os.listdir(real):
                full = os.path.join(real, name)
                a = SFTPAttributes.from_stat(os.stat(full))
                a.filename = name
                out.append(a)
            return out
        except OSError:
            return paramiko.SFTP_NO_SUCH_FILE
    def stat(self, path):
        try:
            return SFTPAttributes.from_stat(os.stat(sftp_map(path)))
        except OSError:
            return None
    def lstat(self, path):
        try:
            return SFTPAttributes.from_stat(os.lstat(sftp_map(path)))
        except OSError:
            return None
    def open(self, path, flags, attr):
        print(f"[sftp] open {path} flags={flags} (O_WRONLY={os.O_WRONLY} O_CREAT={os.O_CREAT} O_TRUNC={os.O_TRUNC})", flush=True)
        real = sftp_map(path)
        mode = 0
        if flags & os.O_RDWR:
            mode = os.O_RDWR
        elif flags & os.O_WRONLY:
            mode = os.O_WRONLY
        else:
            mode = os.O_RDONLY
        if flags & os.O_CREAT:
            mode |= os.O_CREAT
        if flags & os.O_TRUNC:
            mode |= os.O_TRUNC
        if flags & os.O_APPEND:
            mode |= os.O_APPEND
        try:
            fd = os.open(real, mode | os.O_BINARY, 0o644)
            if mode & os.O_RDWR:
                f = os.fdopen(fd, "r+b", buffering=0)
            elif mode & os.O_WRONLY:
                f = os.fdopen(fd, "wb" if (mode & os.O_TRUNC) else "ab", buffering=0)
            else:
                f = os.fdopen(fd, "rb", buffering=0)
        except OSError as e:
            print(f"[sftp] open FAILED: {e}", flush=True)
            return paramiko.SFTP_NO_SUCH_FILE
        h = MiniSFTPHandle(flags)
        h.filename = real
        h.readfile = f
        h.writefile = f
        return h
    def remove(self, path):
        try:
            os.remove(sftp_map(path))
            return SFTP_OK
        except OSError:
            return paramiko.SFTP_NO_SUCH_FILE
    def rename(self, oldpath, newpath):
        try:
            os.rename(sftp_map(oldpath), sftp_map(newpath))
            return SFTP_OK
        except OSError:
            return paramiko.SFTP_FAILURE
    def mkdir(self, path, attr):
        try:
            os.mkdir(sftp_map(path))
            return SFTP_OK
        except OSError:
            return paramiko.SFTP_FAILURE
    def rmdir(self, path):
        try:
            os.rmdir(sftp_map(path))
            return SFTP_OK
        except OSError:
            return paramiko.SFTP_FAILURE
    def canonicalize(self, path):
        return "/"

class TestServer(ServerInterface):
    def __init__(self):
        self.sftp_channels = set()
        self.shell_channels = set()
        self.exec_channels = {}
    def check_auth_password(self, username, password):
        if username == USER and password == PASS:
            return AUTH_SUCCESSFUL
        return AUTH_FAILED
    def check_auth_publickey(self, username, key):
        return AUTH_FAILED
    def check_channel_request(self, kind, chanid):
        if kind in ("session",):
            return OPEN_SUCCEEDED
        return OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_shell_request(self, channel):
        self.shell_channels.add(channel.chanid)
        return True
    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        return True
    def check_channel_exec_request(self, channel, command):
        print(f"[demo] EXEC REQUEST chan={channel.chanid}: {command[:60]!r}", flush=True)
        self.exec_channels[channel.chanid] = command.decode("utf-8", "replace")
        return True
    def check_channel_subsystem_request(self, channel, name):
        print(f"[demo] SUBSYSTEM REQUEST chan={channel.chanid} name={name}", flush=True)
        if name == "sftp":
            self.sftp_channels.add(channel.chanid)
            return True
        return False
    def get_allowed_auths(self, username):
        return "password,publickey"

def main():
    if not os.path.exists(HOST_KEY):
        key = paramiko.RSAKey.generate(2048)
        key.write_private_key_file(HOST_KEY)
    else:
        key = paramiko.RSAKey(filename=HOST_KEY)
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((HOST, PORT))
    sock.listen(5)
    print(f"[demo] SSH server listening on {HOST}:{PORT} (user={USER} pass={PASS})", flush=True)
    print(f"[demo] SFTP root: {SFTP_ROOT}", flush=True)
    while True:
        conn, addr = sock.accept()
        t = paramiko.Transport(conn)
        t.add_server_key(key)
        server = TestServer()
        try:
            t.start_server(server=server)
        except Exception as e:
            print(f"[demo] handshake failed: {e}", flush=True)
            continue
        threading.Thread(target=serve_transport, args=(t, server), daemon=True).start()

def serve_transport(t, server):
    """一个连接可能打开多个 channel(shell + exec + sftp),循环 accept 全部"""
    while True:
        chan = t.accept(60)
        if chan is None:
            break
        threading.Thread(
            target=handle_chan, args=(chan, server), daemon=True
        ).start()

def handle_chan(chan, server):
    # 等待请求类型确定(exec/sftp/shell 存在竞态,需要轮询)
    print(f"[demo] handle_chan start chan={chan.chanid}", flush=True)
    mode = None
    deadline = time.time() + 8
    while time.time() < deadline:
        if chan.chanid in server.sftp_channels:
            mode = "sftp"
            print(f"[demo] chan={chan.chanid} -> sftp", flush=True)
            break
        if chan.chanid in server.exec_channels:
            mode = "exec"
            print(f"[demo] chan={chan.chanid} -> exec", flush=True)
            break
        if chan.chanid in server.shell_channels:
            mode = "shell"
            print(f"[demo] chan={chan.chanid} -> shell", flush=True)
            break
        time.sleep(0.02)
    if mode is None:
        mode = "shell"
    try:
        if mode == "sftp":
            sftp = paramiko.SFTPServer(chan, "sftp", server, MiniSFTP)
            sftp.start_subsystem("sftp", chan.get_transport(), chan)
            sftp.finish_subsystem()
        elif mode == "exec":
            cmd = server.exec_channels.get(chan.chanid, "")
            print(f"[demo] exec cmd: {cmd[:60]!r}", flush=True)
            shell = MiniShell(SFTP_ROOT)
            out = shell.handle_line(cmd)
            print(f"[demo] exec out len={len(out)}", flush=True)
            chan.send(out)
            print(f"[demo] exec sent", flush=True)
            try:
                chan.send_exit_status(0)
            except Exception:
                pass
            chan.close()
        else:
            shell = MiniShell(SFTP_ROOT)
            shell.run(chan)
    except Exception as e:
        import traceback
        traceback.print_exc()
        print(f"[demo] chan error: {e}", flush=True)
    finally:
        try:
            chan.close()
        except Exception:
            pass

if __name__ == "__main__":
    main()
