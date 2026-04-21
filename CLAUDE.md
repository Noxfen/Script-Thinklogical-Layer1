# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

A single-file Python/Flask web application for controlling a **Thinklogical TLX** audio/video matrix switch via its ASCII TCP protocol. The UI is in Italian and the entire application lives in `tlx_control.py`.

## Running

```bash
pip install flask
python tlx_control.py
# Open http://localhost:5000
```

No build step, no tests, no package config files.

## Architecture

Everything is in `tlx_control.py`. Key sections:

**Configuration** (top of file)
- `MATRIX_IP = "192.168.13.15"`, `MATRIX_PORT = 17567` — hardcoded TLX device address
- `MAX_PORT = 24` — physical port count
- `PRESETS_FILE = "tlx_presets.json"` — persisted preset connections

**TLX ASCII protocol** (lines ~39–96)
- `send_command(cmd)` — opens a TCP socket, sends one ASCII command, reads response, closes
- `execute_bidir_connection(port_a, port_b)` — calls DI (disconnect) + CI (connect input) + CO (connect output) to wire two ports bidirectionally
- Protocol commands: `CI####O####`, `DI####`, `SI####`, `DI9999` (disconnect all)

**Preset persistence** — `load_presets()` / `save_presets()` read/write `tlx_presets.json` as a list of `{name, portA, portB}` objects

**Event log** — in-memory circular list (max 50 entries), polled by the UI every 3 seconds via `/api/log`

**HTML/CSS/JS template** — embedded as a Python string inside the file (dark theme, Italian labels, no external dependencies)

**Flask routes** — all prefixed `/api/`: `presets`, `add_preset`, `delete_preset`, `execute_preset`, `connect_bidir`, `disconnect_all`, `disconnect_bidir`, `status`, `log`, `clear_log`

## Things to know

- The web server binds to `0.0.0.0:5000` (all interfaces), `debug=False`
- Default presets (4 codec-to-tablet pairings) are created on first run if `tlx_presets.json` is absent
- `/api/status` queries all 24 ports sequentially with `SI####` — can be slow if the device is unreachable
- `send_command` uses a fresh TCP connection per command (stateless); socket timeout is 3 seconds
- All user-visible text, comments, and error messages are in Italian
