# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

**TLX Control** (v2.0, TT Group SRL): a desktop app for **Windows and macOS** that controls **Thinklogical TLX** fibre matrix switches (TLX12 … TLX1280) through the ASCII API, over **TCP 17567** or **RS-232**. It offers connection presets, KVM stations, a crosspoint grid and diagnostics. The UI is in Italian.

It is a rewrite of the old single-file Python/Flask script (`tlx_control.py`, now in `legacy/`, kept only for reference and for importing its `tlx_presets.json`). Stack: Rust workspace + **Tauri 2** + **Svelte 5** (TypeScript, Vite). Nothing of the Flask app is used any more.

## Build, run, test

Requirements: stable Rust **1.90 or newer** (Tauri 2.12 refuses to build on less — `rustup update stable`), Node 22. Run everything from the repo root.

```bash
npm ci --prefix ui                                  # UI dependencies (also installs the tauri CLI)
cargo test -p tlx-core -p tlx-sim                   # protocol, session, client, simulator tests (what CI runs)
cargo clippy -p tlx-core -p tlx-sim -- -D warnings  # lint (what CI runs)
npm --prefix ui run check                           # svelte-check
npm --prefix ui run build                           # UI production build into ui/dist
./ui/node_modules/.bin/tauri dev                    # app in development (starts the Vite dev server on :1420)
./ui/node_modules/.bin/tauri build                  # installers: NSIS on Windows, .dmg/.app on macOS
cargo run -p tlx-sim -- --ports 24 --listen 0.0.0.0:17567   # fake matrix
```

- Run the `tauri` CLI **from the repo root**, as above. `npm --prefix ui run tauri -- build` does NOT work: npm runs the script with `ui/` as the working directory and the CLI then fails with «Couldn't recognize the current folder as a Tauri project» because `tauri.conf.json` lives in `src-tauri/`.
- `tlx-sim` options (see `crates/tlx-sim/src/main.rs`): `--ports <N>` (default 24), `--listen <IP:PORT>` (default `0.0.0.0:17567`), `--bcast <IP:PORT>` (UDP 17564 status broadcast; off if absent), `--bcast-ms <MS>` (default 4000), `--delay-ms <MS>` (artificial delay before each reply), `--no-verbose` (replies without the `#...` comment), `--no-rev-m` (firmware without `XLOS`/`XRXPWR`/`XTXPWR`, they answer `ER0001`).
- The app also has a built-in **Demo** profile that starts the same simulator in-process (`TransportKind::Demo`), so neither a matrix nor `tlx-sim` is needed to try the app.
- UI without the backend: `npm --prefix ui run dev`, open `http://localhost:1420`. The backend is then `ui/src/lib/mock.ts` (in-memory matrix; `?ports=160` for a big one).
- `src-tauri` (package `tlx-control`) has its own unit tests in `plan.rs` and `config.rs`. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` are both clean (71 tests: 47 `tlx-core` unit, 7 `tlx-core` integration against the simulator, 3 `tlx-sim` CLI, 14 `src-tauri`), and CI runs both.
- `.github/workflows/build.yml`: job `test` (Linux: the cargo test and clippy above, `npm run check`, `npm run build`), then job `bundle` (Windows x64, macOS universal). A `v*` tag creates a draft release.
- `cargo fmt --check` is part of CI. `rustfmt.toml` sets `max_width = 100` and `use_small_heuristics = "Max"` to match the hand-written style; without it rustfmt defaults to 80 columns and reformats every file it touches.
- `HANDOFF/`, `release/` and `.claude/` are git-ignored. `HANDOFF/wip/` is a stale copy of the source tree left over from the rewrite: do not edit it, and exclude it from searches. `release/` is where the built executables are handed over.

## Architecture

Cargo workspace (`Cargo.toml`): `crates/tlx-core`, `crates/tlx-sim`, `src-tauri`; plus `ui/` (npm project, not a Cargo member).

**`crates/tlx-core`** — protocol, transports, session, client, simulator. No Tauri dependency.
- `src/lib.rs` — module list and re-exports (`MatrixClient`, `ClientConfig`, `Routing`, `MatrixInfo`, `Response`, ...).
- `src/protocol/` — pure parsers and builders, no I/O:
  - `response.rs` — response line parser (`R` + 4 chars + `OK[data]` or `ERnnnn`, then `#comment`);
  - `command.rs` — client-side command builders (`ci`, `di`, `do_`, `macro_`, ...) that enforce the TT rules, plus `parse_command` (matrix-side decoder, used by the simulator and for local state updates);
  - `danger.rs` — `is_dangerous()`;
  - `info.rs` — parsers for `XVERSION`, `XTYPE`, `XMAXPORTS`, `XMAXCARDS`, `XINSTALLED`, `XPORTCONFIG`, `XOPTIONS`, `XALARM`, `XLOS`, `XRXPWR`/`XTXPWR`, `XLASTEVENT`;
  - `routing.rs` — crosspoint state (`Routing`, `XGET` parsing);
  - `bcast.rs` — UDP 17564 datagram encode/parse and `BcastAssembler` (the TLX1280 sends two blocks).
- `src/transport.rs` — line-based channel over TCP or serial; accumulates bytes up to the line terminator.
- `src/session.rs` — one command channel, one command in flight, timeouts, handshake. After a timeout or I/O error the session is broken and must be reopened.
- `src/client.rs` — `MatrixClient`: cloneable handle to an actor task that owns the session and queues commands; heartbeat (`XLASTEVENT`), reconnection, UDP listener, events.
- `src/events.rs` — event types emitted by the client (connection state, info, routing, log).
- `src/sim.rs` — in-process TLX simulator (TCP API, optional UDP broadcast); used by Demo mode and tests.
- `src/util.rs` — internal helpers (time, truncation, date formatting).

**`crates/tlx-sim`** — `src/main.rs` only: command-line wrapper around `tlx_core::sim`.

**`src-tauri`** — the desktop app (lib `tlx_control_lib`, binary `tlx-control`; also `tauri.conf.json` and `capabilities/default.json`).
- `src/main.rs` — entry point, calls `tlx_control_lib::run()`.
- `src/lib.rs` — Tauri builder: single-instance and dialog plugins, `App` creation, auto-connect to the active profile, the `invoke_handler` list, orderly shutdown on exit.
- `src/ipc.rs` — IPC DTOs (camelCase) and the `#[tauri::command]` functions.
- `src/state.rs` — `App`: config, connection to the matrix (including the Demo simulator), UI caches, log, undo, execution of switching plans.
- `src/plan.rs` — pure planning: from an intent (link, disconnect, preset, scene, grid changes) to the ASCII commands and the expected state; also plan simulation, verification and safety warnings. Unit-tested.
- `src/config.rs` — persistent config (profiles, labels, presets, settings).

**`ui/`** — Svelte 5 (runes). `src/App.svelte`, `src/main.ts`; `src/lib/` (`api.ts` typed `invoke` wrappers, `types.ts`, `store.svelte.ts` global state, `util.ts`, `mock.ts` browser-only backend); `src/components/` (top bar, sidebar, dialogs, toasts, banners) and `src/components/screens/` (Presets, Preset editor, Stations, Matrix grid, Ports, Diagnostics, Console, Log, Settings). Presets, Stations and Settings are always visible; Matrix, Ports, Diagnostics, Console and Log are behind the «Modalità tecnico» switch (`settings.expertMode`, gated in `Sidebar.svelte`).

**IPC contract**: `docs/IPC.md` is the reference. JSON names are camelCase; the UI calls `invoke` commands and listens to `tlx://*` events (`tlx://conn`, `tlx://info`, `tlx://routing`, `tlx://diag`, `tlx://log`). `ui/src/lib/types.ts` is the source of truth on the UI side and must stay aligned with `src-tauri/src/ipc.rs`. A new or changed command touches `ipc.rs`, the handler list in `src-tauri/src/lib.rs`, `ui/src/lib/api.ts`, `ui/src/lib/types.ts`, `ui/src/lib/mock.ts` and `docs/IPC.md`.

**Configuration** is stored in `<app_config_dir>/config.json` (Tauri's `app_config_dir`; identifier `it.tt-group.tlx-control`): `%APPDATA%\it.tt-group.tlx-control\` on Windows, `~/Library/Application Support/it.tt-group.tlx-control/` on macOS.

## Things to know

- **The `Rnnnn` length field of a response is NOT reliable** (the manuals show `R0000` everywhere, and `XGET` on a big matrix exceeds 9999 bytes). Delimit on the line terminator, skip the 4 characters after `R`, accept `LF` and `CR LF`. Never compare with a literal `R0003OK` (verbose mode appends `#comment`); success is `R` + 4 chars + `OK`. Data is what lies between `OK` and the first `#`.
- **Connections are one-way.** A KVM station is two connections (`CI tx O rx` and `CI rx O tx`). The `bidir` link mode is exclusive and does `DI a, DI b, CI a O b, CI b O a`; `kvm` does not free the TX, so other RX keep watching it; `oneway` is video only. See `docs/IPC.md`.
- **"Command accepted" is not "state reached".** After every switching operation the app rereads the matrix with `XGET` and compares the whole state with the expected one; the check is mandatory (`verifyAfterSwitch` in the settings is ignored). `ok` means no error and verified; `verified: null` means unknown outcome.
- A switching operation starts from a fresh `XGET`, simulates the plan, refuses to touch locked ports, stops at the first `ER` or timeout (a failed `DI` must never be followed by the `CI`: two PCs could end up sharing a keyboard), then rereads the state.
- **Dangerous commands** (`XRESET`, `XTEST*`, `XPUT`, `XHELP`, every form with `9999`, the trailing `;` of the Macro) are recognised by `protocol::is_dangerous`, rejected by `send_raw` unless `force`, and confirmed by the operator in the UI. The builders in `protocol/command.rs` reject `9999` except through the explicit `di_all`/`do_all`. "Scollega tutto" sends `DO` on each connected, unlocked output (chunks of 50), not `DI9999`.
- One command in flight at a time. A command that times out is never resent; the session is then broken and the client reconnects (backoff 1, 2, 5, 10 s, then 10 s, forever). While disconnected, commands fail immediately.
- **UDP 17564 (status broadcast) is off by default** (`udpListen: false` in the profile): on Windows the first listen triggers the firewall prompt. Without UDP the client sends `XLASTEVENT` every `heartbeatS` (default 10) and rereads with `XGET` when it changes.
- Ports are integers 1..N, `0` = no port; N comes from `XMAXPORTS`, never hard-code 24. Times in the contract are epoch milliseconds.
- All user-visible text, comments and error messages are in Italian (command errors are `Result<T, String>` with an Italian string ready to display). This file is the exception: English, like the other CLAUDE.md files.

## Authoritative protocol sources

Internal spec (read it before touching protocol, session or plan code):
`D:\Claude\Projects\Thinklogical\API TL sviluppo modulo TT-AVC\Corrente\API_TL_sviluppo_modulo_TT-AVC_v2.0.md`
- §2 transports, §3 syntax and responses, §4 semantics, §5 sequences, §6.1 UDP 17564, §10 models, §13.1 errors in the manufacturer manuals, §14 requirements, App. B test fixtures.

Per-device behaviour (multi-fibre extenders, Quad Rx, OSD, Macro):
`D:\Claude\Projects\Thinklogical\API TL implementazione per dispositivo\Corrente\API_TL_implementazione_per_dispositivo_v1.0.md`

Manufacturer manual: *TLX Matrix Switch ASCII API V5 Rev M*. Doc comments in `crates/tlx-core` cite the spec sections (`spec §x`).
