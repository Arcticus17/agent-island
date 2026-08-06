#!/usr/bin/env python3
"""Agent Island 公网中继服务（自建原型）。

运行：
    python relay-server.py [port]

默认端口 8787。Agent Island 的“远程”面板填入
http://<本机IP>:8787 即可开始推送状态。
"""

import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

STATE_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "relay-state.json")
DEVICES = {}


def load_state():
    global DEVICES
    try:
        with open(STATE_FILE, "r", encoding="utf-8") as f:
            DEVICES = json.load(f)
    except Exception:
        DEVICES = {}


def save_state():
    try:
        with open(STATE_FILE, "w", encoding="utf-8") as f:
            json.dump(DEVICES, f, ensure_ascii=False, indent=2)
    except Exception:
        pass


MOBILE_HTML = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="#121214">
<link rel="manifest" href="/manifest.webmanifest">
<title>Agent Island Mobile</title>
<style>
body{font-family:"Segoe UI","Microsoft YaHei",sans-serif;background:#121214;color:#f2f2f5;margin:0;padding:16px}
h1{font-size:18px;margin:0 0 4px}
#updated{color:#8e8e93;font-size:11px;margin-bottom:14px}
.card{background:#1c1c1f;border:1px solid #2a2a2e;border-radius:12px;padding:12px 14px;margin-bottom:10px}
.row{display:flex;align-items:center;gap:10px}
.dot{width:10px;height:10px;border-radius:50%;flex-shrink:0}
.green{background:#30d158}.yellow{background:#ffd60a}.red{background:#ff453a}.gray{background:#636366}
.name{font-weight:600}.status{color:#9a9aa0;font-size:12px;margin-left:auto}
.cwd{color:#9a9aa0;font-size:12px;margin-top:6px;word-break:break-all}
.out{color:#c8c8cc;font-size:12px;margin-top:6px;white-space:pre-wrap;max-height:64px;overflow:hidden}
</style>
</head>
<body>
<h1>Agent Island</h1>
<div id="updated">连接中...</div>
<div id="list"></div>
<script>
const token=new URLSearchParams(location.search).get('token')||'';
async function load(){
  try{
    const r=await fetch('/api/device/'+location.pathname.split('/').pop()+'/status',{headers:{'X-Token':token}});
    const data=await r.json();
    const colors={working:'green',running:'green',done:'green',idle:'yellow',waiting:'yellow',high_load:'yellow',stopped:'red',error:'red'};
    const statusText={working:'工作中',running:'工作中',done:'已完成',idle:'等待中',waiting:'等待确认',high_load:'高负载',stopped:'已停止',error:'报错'};
    document.getElementById('updated').textContent='更新于 '+new Date(data.updated||Date.now()).toLocaleTimeString();
    const agents=data.agents||[];
    document.getElementById('list').innerHTML=agents.map(a=>{
      const c=colors[a.status]||'gray';
      return '<div class="card"><div class="row"><span class="dot '+c+'"></span><span class="name"></span><span class="status"></span></div><div class="cwd"></div><div class="out"></div></div>';
    }).join('');
    document.querySelectorAll('.card').forEach((n,i)=>{
      const a=agents[i]; if(!a)return;
      n.querySelector('.name').textContent=a.name;
      n.querySelector('.status').textContent=statusText[a.status]||a.status||'-';
      n.querySelector('.cwd').textContent=a.cwd||'-';
      n.querySelector('.out').textContent=(a.recent_output||[]).slice(-3).join('\\n')||'暂无日志';
    });
  }catch(e){
    document.getElementById('list').textContent='无法连接，请检查设备是否在线';
  }
}
load();
setInterval(load,3000);
</script>
</body>
</html>"""

MANIFEST = """{
  "name": "Agent Island Mobile",
  "short_name": "Agent Island",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#121214",
  "theme_color": "#121214"
}"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _send(self, code, body, content_type="application/json; charset=utf-8"):
        if isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Device-Id, X-Token")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.end_headers()

    def _device_id(self):
        return self.headers.get("X-Device-Id", "")

    def _token(self):
        return self.headers.get("X-Token", "")

    def do_POST(self):
        if self.path.rstrip("/") != "/api/push":
            self._send(404, '{"error":"not found"}')
            return
        device = self._device_id()
        token = self._token()
        if not device or not token:
            self._send(400, '{"error":"missing device/token"}')
            return
        length = int(self.headers.get("Content-Length", "0"))
        try:
            agents = json.loads(self.rfile.read(length).decode("utf-8"))
        except Exception:
            self._send(400, '{"error":"bad json"}')
            return
        DEVICES[device] = {
            "token": token,
            "agents": agents,
            "updated": time.time() * 1000,
        }
        save_state()
        self._send(200, '{"ok":true}')

    def do_GET(self):
        path = self.path.split("?")[0].rstrip("/")
        if path == "/api/health":
            self._send(200, '{"ok":true}')
            return
        if path == "/manifest.webmanifest":
            self._send(200, MANIFEST, "application/manifest+json; charset=utf-8")
            return
        parts = path.split("/")
        if path.startswith("/api/device/") and path.endswith("/status"):
            device = parts[3] if len(parts) >= 5 else ""
            entry = DEVICES.get(device)
            if not entry or entry.get("token") != self._token():
                self._send(403, '{"error":"forbidden"}')
                return
            self._send(200, json.dumps({"agents": entry.get("agents", []), "updated": entry.get("updated", 0)}))
            return
        if len(parts) == 3 and parts[1] == "device":
            self._send(200, MOBILE_HTML, "text/html; charset=utf-8")
            return
        self._send(404, '{"error":"not found"}')


def main():
    load_state()
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8787
    server = ThreadingHTTPServer(("0.0.0.0", port), Handler)
    print(f"relay server listening on http://0.0.0.0:{port}")
    server.serve_forever()


if __name__ == "__main__":
    main()
