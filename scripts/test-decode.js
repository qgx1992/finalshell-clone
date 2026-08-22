// 临时验证脚本:对比 TerminalPanel 的查表法 decodeBase64 与标准 base64
// 用法: node scripts/test-decode.js
const B64_TABLE = new Int8Array(128).fill(-1);
for (let i = 0; i < 26; i++) {
  B64_TABLE[65 + i] = i; // A-Z
  B64_TABLE[97 + i] = 26 + i; // a-z
}
for (let i = 0; i < 10; i++) {
  B64_TABLE[48 + i] = 52 + i; // 0-9
}
B64_TABLE[43] = 62; // +
B64_TABLE[47] = 63; // /

function decodeBase64(input) {
  const len = input.length;
  const out = new Uint8Array((len * 3) >> 2);
  let o = 0;
  let i = 0;
  while (i < len) {
    let acc = 0;
    let n = 0;
    while (i < len && n < 4) {
      const code = input.charCodeAt(i++);
      if (code === 61) break;
      const v = code < 128 ? B64_TABLE[code] : -1;
      if (v < 0) continue;
      acc = (acc << 6) | v;
      n++;
    }
    let remaining = n * 6;
    while (remaining >= 8) {
      remaining -= 8;
      out[o++] = (acc >> remaining) & 0xff;
    }
  }
  return out.subarray(0, o);
}

let fail = 0;
let total = 0;

function check(bytes, label) {
  total++;
  const b64 = Buffer.from(bytes).toString("base64");
  const dec = Buffer.from(decodeBase64(b64));
  if (!dec.equals(Buffer.from(bytes))) {
    fail++;
    console.log(`FAIL [${label}] len=${bytes.length}`);
    console.log("  expect:", Buffer.from(bytes).subarray(0, 32).toString("hex"));
    console.log("  actual:", dec.subarray(0, 32).toString("hex"));
  }
}

// 1. 随机二进制
for (let t = 0; t < 5000; t++) {
  const n = Math.floor(Math.random() * 6000);
  const bytes = Buffer.alloc(n);
  for (let i = 0; i < n; i++) bytes[i] = Math.floor(Math.random() * 256);
  check(bytes, `random-${n}`);
}

// 2. 中文 UTF-8
const zh = Buffer.from(
  "你好世界，这是中文测试。FinalShell Clone 终端。乱码排查专用字符串:编码UTF-8正常显示",
  "utf8"
);
check(zh, "chinese");

// 3. 边界长度(1..8, 全 A + 全 0xff)
for (const n of [1, 2, 3, 4, 5, 6, 7, 8, 32, 33, 34, 35, 36]) {
  check(Buffer.alloc(n, 0x41), `A-${n}`);
  check(Buffer.alloc(n, 0xff), `FF-${n}`);
}

// 4. 模拟后端 flush:32KB 混合(中文+ascii+二进制)
{
  const big = Buffer.alloc(32 * 1024);
  for (let i = 0; i < big.length; i++) {
    const r = Math.random();
    if (r < 0.5) big[i] = 0x41 + (i % 26);
    else if (r < 0.75) big[i] = 0xe4 + (i % 3); // 中文高字节区
    else big[i] = Math.floor(Math.random() * 256);
  }
  check(big, "32k-mixed");
}

console.log(`\n${total} cases, ${fail} failures`);
process.exit(fail === 0 ? 0 : 1);
