# CHANGELOG — TLX Control

## v2.0 — 2026-11-02

Riscrittura in Rust (Tauri 2 + Svelte 5). Eseguibile unico per Windows e macOS.

**Stessa logica dello script**
- Preset A↔B con la sequenza originale `DI a, DI b, CI a O b, CI b O a`; i preset di
  `tlx_presets.json` si importano da Impostazioni; i 4 preset storici sono già presenti.
- Collegamento manuale A↔B, scollegamento di una porta, «Scollega tutto» (con conferma digitata).

**Correzioni rispetto allo script**
- Una sessione TCP persistente al posto di una connessione per comando; un solo comando in volo.
- Le risposte si leggono fino al terminatore di riga; `ERnnnn` è un errore (lo script lo
  considerava riuscito perché guardava solo il socket).
- Ci si ferma al primo errore: un `DI` fallito non viene più seguito dai `CI` (rischio tastiera
  su due PC).
- Dopo ogni commutazione lo stato viene riletto (`XGET`) e confrontato con l'atteso.
- Lo stato si legge con un solo `XGET` (formato a gruppi) invece di 24 `SI` in 24 connessioni;
  il numero di porte arriva da `XMAXPORTS`, non è più fisso a 24.
- «Scollega porta» può staccare ingresso, uscita o entrambi (default: entrambi).
- «Scollega tutto» usa `DO` sulle sole uscite collegate e non bloccate, a blocchi di 50, invece
  di `DI9999`: le uscite bloccate dall'app restano accese.

**Nuovo**
- Più matrici (profili TCP, RS-232 o Demo) con selezione in alto e connessione automatica.
- Modalità «Postazione KVM» (non esclusiva sul PC) e «Solo video».
- Postazioni (pannello destinazione → sorgente con TAKE), griglia degli incroci con modifiche in
  attesa, etichette e blocco delle porte, annulla ultima operazione.
- Diagnostica: allarmi, LOS e potenza ottica SFP (firmware Rev. M), informazioni di modello.
- Console ASCII con protezione dei comandi pericolosi (`XRESET`, `XTEST*`, `XPUT`, forme `9999`).
- Log con esportazione CSV; ricezione facoltativa dello stato su UDP 17564.
- Simulatore TLX integrato (profilo Demo) e da riga di comando (`tlx-sim`).

## v1 — 2026-04

Script Python/Flask `tlx_control.py` (ora in `legacy/`).
