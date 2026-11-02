#!/usr/bin/env python3
"""
TLX Matrix Control - Interfaccia web per commutazione porte Thinklogical TLX
tramite API ASCII (TCP 17567).

Requisiti:
    pip install flask

Uso:
    python tlx_control.py
    Poi apri il browser su http://localhost:5000
"""

import socket
import json
import os
from flask import Flask, render_template_string, request, jsonify
from datetime import datetime

# ====== CONFIGURAZIONE ======
MATRIX_IP = "192.168.13.15"
MATRIX_PORT = 17567
MAX_PORT = 24
TIMEOUT = 3  # secondi
PRESETS_FILE = "tlx_presets.json"
# ============================

app = Flask(__name__)
event_log = []


def log_event(message):
    timestamp = datetime.now().strftime("%H:%M:%S")
    event_log.insert(0, f"[{timestamp}] {message}")
    if len(event_log) > 50:
        event_log.pop()


def send_command(cmd):
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.settimeout(TIMEOUT)
            s.connect((MATRIX_IP, MATRIX_PORT))
            s.sendall((cmd + "\n").encode("ascii"))
            response = s.recv(1024).decode("ascii", errors="replace").strip()
            log_event(f"→ {cmd}   ←  {response}")
            return {"ok": True, "command": cmd, "response": response}
    except socket.timeout:
        log_event(f"ERRORE TIMEOUT su comando: {cmd}")
        return {"ok": False, "command": cmd, "error": "Timeout"}
    except Exception as e:
        log_event(f"ERRORE: {cmd} - {str(e)}")
        return {"ok": False, "command": cmd, "error": str(e)}


def format_port(port_num):
    return f"{int(port_num):04d}"


def load_presets():
    if not os.path.exists(PRESETS_FILE):
        default = [
            {"name": "Codec 1 ↔ Tablet A", "portA": 1, "portB": 5},
            {"name": "Codec 1 ↔ Tablet B", "portA": 1, "portB": 8},
            {"name": "Codec 2 ↔ Tablet A", "portA": 4, "portB": 5},
            {"name": "Codec 2 ↔ Tablet B", "portA": 4, "portB": 8},
        ]
        save_presets(default)
        return default
    try:
        with open(PRESETS_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as e:
        log_event(f"Errore lettura preset: {e}")
        return []


def save_presets(presets):
    try:
        with open(PRESETS_FILE, "w", encoding="utf-8") as f:
            json.dump(presets, f, indent=2, ensure_ascii=False)
        return True
    except Exception as e:
        log_event(f"Errore salvataggio preset: {e}")
        return False


def execute_bidir_connection(port_a, port_b):
    pa = format_port(port_a)
    pb = format_port(port_b)
    log_event(f"--- Collegamento bidirezionale porta {int(pa)} ↔ porta {int(pb)} ---")
    send_command(f"DI{pa}")
    send_command(f"DI{pb}")
    r1 = send_command(f"CI{pa}O{pb}")
    r2 = send_command(f"CI{pb}O{pa}")
    return {"ok": r1["ok"] and r2["ok"], "r1": r1, "r2": r2}


HTML_TEMPLATE = """
<!DOCTYPE html>
<html lang="it">
<head>
    <meta charset="UTF-8">
    <title>TLX Matrix Control</title>
    <style>
        * { box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #1a1a2e; color: #e4e4e7;
            margin: 0; padding: 20px; min-height: 100vh;
        }
        .container { max-width: 1000px; margin: 0 auto; }
        h1 { color: #06b6d4; border-bottom: 2px solid #06b6d4; padding-bottom: 10px; }
        .info-bar {
            background: #16213e; padding: 12px 20px; border-radius: 8px;
            margin-bottom: 20px; display: flex; justify-content: space-between; font-size: 14px;
        }
        .info-bar .label { color: #94a3b8; }
        .info-bar .value { color: #06b6d4; font-weight: bold; }
        .card {
            background: #16213e; padding: 20px; border-radius: 8px;
            margin-bottom: 20px; box-shadow: 0 2px 8px rgba(0,0,0,0.3);
        }
        .card h2 { margin-top: 0; color: #f59e0b; font-size: 18px; }
        label { display: inline-block; min-width: 80px; margin-right: 10px; }
        select, input {
            background: #0f172a; color: #e4e4e7; border: 1px solid #334155;
            padding: 8px 12px; border-radius: 4px; font-size: 15px; margin-right: 10px;
        }
        input[type="text"] { min-width: 200px; }
        select:focus, input:focus { outline: none; border-color: #06b6d4; }
        button {
            background: #06b6d4; color: white; border: none;
            padding: 10px 18px; border-radius: 4px; font-size: 14px;
            cursor: pointer; margin: 4px; font-weight: 500; transition: background 0.2s;
        }
        button:hover { background: #0891b2; }
        button.danger { background: #dc2626; }
        button.danger:hover { background: #b91c1c; }
        button.success { background: #16a34a; }
        button.success:hover { background: #15803d; }
        button.warning { background: #ea580c; }
        button.warning:hover { background: #c2410c; }
        button.small { padding: 5px 10px; font-size: 12px; }
        .preset-item {
            display: flex; align-items: center; background: #0f172a;
            padding: 10px 15px; border-radius: 6px; margin-bottom: 8px; gap: 10px;
        }
        .preset-item .preset-name { flex: 1; font-weight: 500; }
        .preset-item .preset-ports { color: #94a3b8; font-size: 13px; min-width: 120px; }
        .log {
            background: #0a0a1e; border-radius: 4px; padding: 15px;
            font-family: 'Courier New', monospace; font-size: 13px;
            max-height: 300px; overflow-y: auto; white-space: pre-wrap; line-height: 1.6;
        }
        .row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
        #status-display {
            font-family: 'Courier New', monospace; background: #0a0a1e;
            padding: 15px; border-radius: 4px; min-height: 60px; white-space: pre-wrap;
        }
        .add-preset-form { background: #0f172a; padding: 15px; border-radius: 6px; margin-top: 10px; }
    </style>
</head>
<body>
<div class="container">
    <h1>🔌 TLX Matrix Control</h1>
    
    <div class="info-bar">
        <span><span class="label">Matrice:</span> <span class="value">{{ matrix_ip }}:{{ matrix_port }}</span></span>
        <span><span class="label">Porte disponibili:</span> <span class="value">1-{{ max_port }}</span></span>
    </div>

    <div class="card">
        <h2>⭐ Preset Personalizzati</h2>
        <div id="presets-list">Caricamento...</div>
        
        <div class="add-preset-form">
            <strong style="color:#94a3b8; font-size:13px;">AGGIUNGI NUOVO PRESET:</strong><br><br>
            <div class="row">
                <label>Nome:</label>
                <input type="text" id="newPresetName" placeholder="es. Codec 1 → Tablet">
                <label>Porta A:</label>
                <select id="newPresetA">
                    {% for i in range(1, max_port+1) %}<option value="{{ i }}">{{ i }}</option>{% endfor %}
                </select>
                <label>Porta B:</label>
                <select id="newPresetB">
                    {% for i in range(1, max_port+1) %}<option value="{{ i }}" {% if i == 5 %}selected{% endif %}>{{ i }}</option>{% endfor %}
                </select>
                <button class="success" onclick="addPreset()">➕ Salva preset</button>
            </div>
        </div>
    </div>

    <div class="card">
        <h2>🔗 Collegamento Manuale</h2>
        <div class="row">
            <label>Porta A:</label>
            <select id="portA">
                {% for i in range(1, max_port+1) %}<option value="{{ i }}">Porta {{ i }}</option>{% endfor %}
            </select>
            <label>Porta B:</label>
            <select id="portB">
                {% for i in range(1, max_port+1) %}<option value="{{ i }}" {% if i == 5 %}selected{% endif %}>Porta {{ i }}</option>{% endfor %}
            </select>
            <button class="success" onclick="connectBidirectional()">▶ Collega A ↔ B</button>
        </div>
    </div>

    <div class="card">
        <h2>❌ Disconnessioni</h2>
        <div class="row">
            <button class="danger" onclick="disconnectAll()">⛔ Disconnetti TUTTE le porte (DI9999)</button>
            <label style="margin-left:20px;">Porta singola:</label>
            <select id="portSingle">
                {% for i in range(1, max_port+1) %}<option value="{{ i }}">Porta {{ i }}</option>{% endfor %}
            </select>
            <button class="warning" onclick="disconnectSingle()">Disconnetti porta</button>
        </div>
    </div>

    <div class="card">
        <h2>📊 Stato Matrice</h2>
        <div class="row"><button onclick="refreshStatus()">🔄 Aggiorna stato</button></div>
        <div id="status-display" style="margin-top:15px;">Premi "Aggiorna stato" per visualizzare.</div>
    </div>

    <div class="card">
        <h2>📜 Log Eventi</h2>
        <div class="row">
            <button onclick="refreshLog()">🔄 Ricarica log</button>
            <button onclick="clearLog()">🗑 Svuota log</button>
        </div>
        <div class="log" id="log" style="margin-top:15px;">{{ log }}</div>
    </div>
</div>

<script>
async function apiCall(endpoint, data, method='POST') {
    try {
        const options = { method: method, headers: {'Content-Type': 'application/json'} };
        if (method !== 'GET') options.body = JSON.stringify(data || {});
        const response = await fetch(endpoint, options);
        return await response.json();
    } catch(e) { return {ok: false, error: e.message}; }
}

async function loadPresets() {
    const r = await apiCall('/api/presets', null, 'GET');
    const listDiv = document.getElementById('presets-list');
    if (!r.presets || r.presets.length === 0) {
        listDiv.innerHTML = '<p style="color:#94a3b8; font-style:italic;">Nessun preset salvato.</p>';
        return;
    }
    listDiv.innerHTML = '';
    r.presets.forEach((p, idx) => {
        const item = document.createElement('div');
        item.className = 'preset-item';
        item.innerHTML = `
            <button class="success" onclick="executePreset(${idx})">▶</button>
            <span class="preset-name">${escapeHtml(p.name)}</span>
            <span class="preset-ports">P${p.portA} ↔ P${p.portB}</span>
            <button class="danger small" onclick="deletePreset(${idx})">🗑 Elimina</button>
        `;
        listDiv.appendChild(item);
    });
}

function escapeHtml(s) {
    return s.replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
}

async function executePreset(idx) {
    await apiCall('/api/execute_preset', {index: idx});
    refreshLog();
}

async function addPreset() {
    const name = document.getElementById('newPresetName').value.trim();
    const portA = parseInt(document.getElementById('newPresetA').value);
    const portB = parseInt(document.getElementById('newPresetB').value);
    if (!name) { alert('Inserisci un nome per il preset!'); return; }
    if (portA === portB) { alert('Le due porte devono essere diverse!'); return; }
    await apiCall('/api/add_preset', {name: name, portA: portA, portB: portB});
    document.getElementById('newPresetName').value = '';
    loadPresets();
}

async function deletePreset(idx) {
    if (!confirm('Eliminare questo preset?')) return;
    await apiCall('/api/delete_preset', {index: idx});
    loadPresets();
}

async function connectBidirectional() {
    const portA = document.getElementById('portA').value;
    const portB = document.getElementById('portB').value;
    if (portA === portB) { alert('Le due porte devono essere diverse!'); return; }
    await apiCall('/api/connect_bidir', {portA: portA, portB: portB});
    refreshLog();
}

async function disconnectAll() {
    if (!confirm('Disconnettere TUTTE le porte?')) return;
    await apiCall('/api/disconnect_all');
    refreshLog();
}

async function disconnectSingle() {
    const port = document.getElementById('portSingle').value;
    await apiCall('/api/disconnect_bidir', {port: port});
    refreshLog();
}

async function refreshStatus() {
    const display = document.getElementById('status-display');
    display.textContent = 'Caricamento...';
    const r = await apiCall('/api/status');
    if (r.ok) {
        let out = 'CONNESSIONI ATTIVE:\\n' + '─'.repeat(50) + '\\n';
        if (r.connections && r.connections.length > 0) {
            r.connections.forEach(c => { out += `  Porta ${c.input} → Porta ${c.output}\\n`; });
        } else { out += '  (nessuna connessione attiva)\\n'; }
        display.textContent = out;
    } else { display.textContent = 'Errore: ' + (r.error || 'sconosciuto'); }
}

async function refreshLog() {
    const r = await fetch('/api/log').then(x=>x.json());
    document.getElementById('log').textContent = r.log;
}

async function clearLog() {
    await apiCall('/api/clear_log');
    refreshLog();
}

loadPresets();
setInterval(refreshLog, 3000);
</script>
</body>
</html>
"""


@app.route("/")
def index():
    return render_template_string(
        HTML_TEMPLATE,
        matrix_ip=MATRIX_IP,
        matrix_port=MATRIX_PORT,
        max_port=MAX_PORT,
        log="\n".join(event_log) if event_log else "(log vuoto)",
    )


@app.route("/api/presets", methods=["GET"])
def api_get_presets():
    return jsonify({"presets": load_presets()})


@app.route("/api/add_preset", methods=["POST"])
def api_add_preset():
    data = request.get_json()
    presets = load_presets()
    presets.append({
        "name": data["name"],
        "portA": int(data["portA"]),
        "portB": int(data["portB"]),
    })
    save_presets(presets)
    log_event(f"Preset aggiunto: {data['name']} (P{data['portA']}↔P{data['portB']})")
    return jsonify({"ok": True})


@app.route("/api/delete_preset", methods=["POST"])
def api_delete_preset():
    data = request.get_json()
    presets = load_presets()
    idx = int(data["index"])
    if 0 <= idx < len(presets):
        removed = presets.pop(idx)
        save_presets(presets)
        log_event(f"Preset eliminato: {removed['name']}")
    return jsonify({"ok": True})


@app.route("/api/execute_preset", methods=["POST"])
def api_execute_preset():
    data = request.get_json()
    presets = load_presets()
    idx = int(data["index"])
    if 0 <= idx < len(presets):
        p = presets[idx]
        log_event(f"⭐ Esecuzione preset: {p['name']}")
        result = execute_bidir_connection(p["portA"], p["portB"])
        return jsonify(result)
    return jsonify({"ok": False, "error": "Preset non trovato"})


@app.route("/api/connect_bidir", methods=["POST"])
def api_connect_bidir():
    data = request.get_json()
    result = execute_bidir_connection(data["portA"], data["portB"])
    return jsonify(result)


@app.route("/api/disconnect_all", methods=["POST"])
def api_disconnect_all():
    log_event("--- Disconnessione di TUTTE le porte ---")
    r = send_command("DI9999")
    return jsonify(r)


@app.route("/api/disconnect_bidir", methods=["POST"])
def api_disconnect_bidir():
    data = request.get_json()
    port = format_port(data["port"])
    log_event(f"--- Disconnessione porta {int(port)} ---")
    r = send_command(f"DI{port}")
    return jsonify(r)


@app.route("/api/status", methods=["POST"])
def api_status():
    connections = []
    for p in range(1, MAX_PORT + 1):
        port_str = format_port(p)
        r = send_command(f"SI{port_str}")
        if r["ok"] and "OK" in r["response"]:
            resp = r["response"]
            if "I" in resp and "O" in resp:
                try:
                    parts = resp.split("I", 1)[1]
                    in_port = int(parts.split("O")[0])
                    out_parts = parts.split("O")[1:]
                    for o in out_parts:
                        out_port = int(o[:4]) if len(o) >= 4 else 0
                        if out_port != 0:
                            connections.append({"input": in_port, "output": out_port})
                except (ValueError, IndexError):
                    pass
    return jsonify({"ok": True, "connections": connections})


@app.route("/api/log", methods=["GET"])
def api_log():
    return jsonify({"log": "\n".join(event_log) if event_log else "(log vuoto)"})


@app.route("/api/clear_log", methods=["POST"])
def api_clear_log():
    event_log.clear()
    log_event("Log cancellato.")
    return jsonify({"ok": True})


if __name__ == "__main__":
    print("=" * 60)
    print("  TLX Matrix Control")
    print(f"  Matrice: {MATRIX_IP}:{MATRIX_PORT}")
    print(f"  Interfaccia web: http://localhost:5000")
    print(f"  Preset salvati in: {os.path.abspath(PRESETS_FILE)}")
    print("=" * 60)
    log_event(f"Server avviato - matrice target: {MATRIX_IP}:{MATRIX_PORT}")
    app.run(host="0.0.0.0", port=5000, debug=False)
