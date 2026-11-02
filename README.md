# TLX Control

Programma per comandare le matrici **Thinklogical TLX** (TLX12 … TLX1280): preset di
collegamento, postazioni KVM, griglia degli incroci, diagnostica. Sostituisce lo script
Python/Flask `tlx_control.py` (ora in `legacy/`) con la stessa logica, in un eseguibile unico
per **Windows** e **macOS**.

Versione **2.0** — TT Group SRL.

## Per l'operatore

1. Doppio clic su **TLX Control** (Windows: `TLX Control.exe` o il programma installato; Mac: l'app).
2. Si apre la finestra e il programma **si collega da solo** all'ultima matrice usata
   (in alto a sinistra si sceglie quale matrice usare).
3. Schermata **Preset**: un clic sul preset e il collegamento è fatto. Il programma rilegge la
   matrice e dice se il collegamento è davvero avvenuto.
4. **+ Nuovo preset** oppure **Salva situazione attuale** per creare preset nuovi: restano salvati.
5. Chiudendo la finestra si chiude tutto (la connessione alla matrice viene chiusa in modo ordinato).

Non serve installare Python né aprire il terminale. Su Windows serve WebView2, già presente su
Windows 10/11 aggiornati (l'installer lo scarica se manca).

**Modalità tecnico** (Impostazioni): mostra anche la griglia degli incroci, le etichette delle
porte, la diagnostica ottica (LOS, potenza SFP) e la console dei comandi ASCII.

## Come parla con la matrice

| canale | uso | note |
|---|---|---|
| **TCP 17567** | comandi (API ASCII) | una sessione persistente, un comando alla volta, all'IP **virtuale** della matrice (default `192.168.13.15`) |
| **RS-232** | comandi, in alternativa alla rete | porta «RS232» del controller attivo (non CONSOLE), **9600 8N1, XON/XOFF**, cavo dritto |
| **UDP 17564** | stato degli incroci trasmesso dalla matrice ogni 4 s | facoltativo (Impostazioni → profilo). Su Windows al primo uso compare l'avviso del firewall: consentire su rete privata |

Senza UDP lo stato si tiene aggiornato da solo: ogni 10 s il programma chiede `XLASTEVENT` e, se
qualcosa è cambiato, rilegge tutto con `XGET`.

### Tipi di collegamento

| nel programma | comandi | quando |
|---|---|---|
| **Bidirezionale (come prima)** | `DI a`, `DI b`, `CI a O b`, `CI b O a` | la logica dello script Python: coppia esclusiva (codec ↔ tablet). Stacca a e b da ogni altra uscita |
| **Postazione KVM** | `DI rx`, `CI tx O rx`, `CI rx O tx` | PC (TX) → postazione (RX) con tastiera; gli altri monitor che guardano lo stesso PC restano collegati |
| **Solo video** | `CI a O b` | un monitor in sola visione |

In Impostazioni si può scegliere se inviare questi passi come comandi separati (come lo script)
o come un unico **Macro** `M…`. In entrambi i casi ci si ferma al primo errore e poi si rilegge
lo stato.

## Per lo sviluppatore

```
crates/tlx-core   protocollo ASCII (parser, comandi, XGET/Macro), trasporti TCP e RS-232,
                  sessione con coda e heartbeat, ricevitore UDP 17564, simulatore TLX
crates/tlx-sim    simulatore da riga di comando (per provare senza matrice)
src-tauri         applicazione desktop (Tauri 2): comandi IPC, configurazione, preset
ui                interfaccia Svelte 5 (TypeScript, Vite), stile TT Group
docs/IPC.md       contratto fra interfaccia e backend
legacy/           lo script Python originale
```

Requisiti: Rust stabile **≥ 1.90** (Tauri 2.12 non compila con meno: `rustup update stable`),
Node 22.

```bash
npm ci --prefix ui
cargo test -p tlx-core -p tlx-sim          # test del protocollo e del simulatore
./ui/node_modules/.bin/tauri dev           # app in sviluppo
./ui/node_modules/.bin/tauri build         # installer (NSIS su Windows, .dmg/.app su Mac)
cargo run -p tlx-sim -- --ports 24         # matrice finta su 0.0.0.0:17567
```

Per provare l'interfaccia senza backend: `npm --prefix ui run dev` e aprire
`http://localhost:1420` (backend simulato in JavaScript; `?ports=160` per una matrice grande).

Gli eseguibili già compilati per Windows stanno in `release/v2.0/` (cartella fuori da git),
con le note di consegna in `release/v2.0/NOTE.md`.

**Build automatiche** (il file esiste ma non è ancora nel repo: serve `gh auth refresh -h
github.com -s workflow` e poi un commit, perché il token non ha lo scope `workflow`):
`.github/workflows/build.yml` compila Windows (x64) e macOS (universale)
a ogni push; gli installer si scaricano dagli *Artifacts* della run. Con un tag `v2.0` crea una
release in bozza.

La configurazione (profili, preset, etichette) sta in:
- Windows: `%APPDATA%\it.tt-group.tlx-control\config.json`
- macOS: `~/Library/Application Support/it.tt-group.tlx-control/config.json`

Fonti del protocollo: manuale *TLX Matrix Switch ASCII API V5 Rev M* e documento interno
*API TL — sviluppo modulo TT-AVC* v2.0 (cartella progetto Thinklogical).
