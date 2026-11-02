<script lang="ts">
  // Schermata «Console» (solo modalità tecnico): invio di comandi ASCII grezzi alla matrice.
  // Nessuna prop: tutto lo stato arriva dallo store `app`.
  //
  // Stato disponibile:
  //   app.consoleEntries   → ConsoleEntry[] (id, ts, command, result: CmdResult | null, error)
  //   app.consoleRecall    → string[] comandi già inviati, dal più recente (frecce su/giù)
  //   app.connected, app.expert, app.settings?.confirmDangerous
  // Azioni:
  //   app.sendRaw(cmd, force) → api.sendRaw; registra la riga nella cronologia e ritorna CmdResult
  //   app.clearConsole()      → svuota la cronologia (solo lato UI)
  //   app.confirm({...})      → conferma dei comandi pericolosi prima di usare force = true
  // La conferma spetta a questa schermata: `isDangerousCommand(cmd)` dice quando chiederla
  // (XRESET, XTEST*, XPUT, XHELP e le forme 9999 che cambiano la routing).
  import { app, type ConsoleEntry } from '../../lib/store.svelte';
  import { fmtTime, isDangerousCommand } from '../../lib/util';
  import Icon from '../Icon.svelte';

  let command = $state('');
  const dangerous = $derived(isDangerousCommand(command));

  /** Macro con più comandi in sequenza: il backend non lo rifiuta, la UI chiede conferma comunque. */
  const macroRisk = $derived(
    !dangerous && /^M/.test(command.trim().toUpperCase()) && command.includes(';'),
  );
  /** true quando serve una conferma esplicita prima dell'invio. */
  const needsConfirm = $derived(dangerous || macroRisk);

  let field = $state<HTMLInputElement | null>(null);
  let transcript = $state<HTMLDivElement | null>(null);
  let busy = $state(false);
  let spacesRemoved = $state(false);
  let helpOpen = $state(false);

  // richiamo dalla cronologia: -1 = sto scrivendo, 0 = comando più recente
  let recall = $state(-1);
  let draft = '';

  const empty = $derived(app.consoleEntries.length === 0);
  const canSend = $derived(app.connected && !busy && command.trim().length > 0);

  /** La matrice vuole i comandi in maiuscolo e senza spazi. */
  function norm(s: string): string {
    return s.toUpperCase().replace(/\s+/g, '');
  }

  function onInput(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const raw = el.value;
    const n = norm(raw);
    recall = -1;
    spacesRemoved = /\s/.test(raw);
    if (n !== raw) {
      // mantiene il cursore dov'era, tolti gli spazi che lo precedevano
      const pos = el.selectionStart ?? n.length;
      const cut = (raw.slice(0, pos).match(/\s/g) ?? []).length;
      el.value = n;
      const p = Math.max(0, Math.min(n.length, pos - cut));
      el.setSelectionRange(p, p);
    }
    command = n;
  }

  /** Scorre `app.consoleRecall`: su = più vecchio, giù = più recente e poi il testo in scrittura. */
  function step(dir: -1 | 1) {
    const hist = app.consoleRecall;
    if (hist.length === 0) return;
    if (dir === 1) {
      if (recall === -1) draft = command;
      const i = Math.min(hist.length - 1, recall + 1);
      recall = i;
      command = hist[i];
    } else if (recall <= 0) {
      recall = -1;
      command = draft;
    } else {
      recall -= 1;
      command = hist[recall];
    }
    spacesRemoved = false;
    const end = command.length;
    queueMicrotask(() => field?.setSelectionRange(end, end));
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      step(1);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      step(-1);
    } else if (e.key === 'Escape' && command !== '') {
      e.preventDefault();
      command = '';
      recall = -1;
      spacesRemoved = false;
    }
  }

  /** Cosa fa, in concreto, un comando pericoloso: testo mostrato prima e dentro la conferma. */
  function dangerInfo(cmd: string): string {
    const c = norm(cmd);
    if (c.startsWith('XRESET'))
      return 'riavvia la matrice: tutti i collegamenti cadono e le postazioni restano senza immagine per alcuni secondi.';
    if (c.startsWith('XTEST'))
      return 'mette la matrice in modalità di prova: le uscite mostrano il segnale di test al posto delle sorgenti reali.';
    if (c.startsWith('XPUT'))
      return 'riscrive la configurazione interna della matrice: cambia impostazioni di fondo del dispositivo.';
    if (c.startsWith('XHELP'))
      return 'fa stampare alla matrice tutto il testo di aiuto: la risposta è lunghissima e la sessione può restare occupata.';
    if (/^DI0*9999/.test(c)) return 'stacca TUTTI gli ingressi: spegne TUTTE le uscite della matrice.';
    if (/^DO0*9999/.test(c)) return 'stacca TUTTE le uscite: spegne TUTTE le uscite della matrice.';
    if (/^CI\d{1,4}O0*9999/.test(c))
      return 'manda lo stesso ingresso a TUTTE le uscite: ogni monitor della matrice mostrerà quella sorgente.';
    if (/^SI0*9999/.test(c))
      return 'interroga TUTTE le porte in una sola risposta: non commuta nulla, ma la risposta è molto lunga.';
    if (/^M/.test(c))
      return 'è un Macro: esegue più commutazioni in un colpo solo e può riconfigurare l’intera matrice.';
    return 'agisce su TUTTE le porte della matrice (forma 9999): può spegnere o riconfigurare tutte le uscite in una volta.';
  }

  async function send() {
    const cmd = norm(command);
    if (!cmd || !app.connected || busy) return;

    // Conferma della UI: sempre richiesta per i comandi pericolosi, anche quando
    // `confirmDangerous` è false e quindi il backend non chiede nulla.
    if (needsConfirm) {
      const ok = await app.confirm({
        title: `Comando pericoloso: ${cmd}`,
        message:
          `Il comando ${cmd} ${dangerInfo(cmd)} ` +
          `Verrà inviato alla matrice ${app.activeProfile?.name ?? '—'} e non può essere annullato automaticamente.`,
        confirmLabel: 'Invia il comando',
        cancelLabel: 'Annulla',
        danger: true,
        requireWord: cmd,
      });
      if (!ok) {
        field?.focus();
        return;
      }
    }

    busy = true;
    try {
      // force = true solo per i comandi che il backend rifiuterebbe (isDangerousCommand).
      await app.sendRaw(cmd, dangerous);
      command = '';
      recall = -1;
      draft = '';
      spacesRemoved = false;
    } finally {
      busy = false;
      field?.focus();
    }
  }

  /** riuscito | errore della matrice (ER…) | errore di trasporto o timeout */
  function kindOf(e: ConsoleEntry): 'ok' | 'matrix' | 'transport' {
    if (e.error !== null || e.result === null) return 'transport';
    if (e.result.errorCode !== null || !e.result.ok) return 'matrix';
    return 'ok';
  }

  function reuse(cmd: string) {
    command = cmd;
    recall = -1;
    spacesRemoved = false;
    field?.focus();
  }

  // l'ultima risposta resta visibile senza scorrere a mano
  $effect(() => {
    const n = app.consoleEntries.length;
    const box = transcript;
    if (n && box) box.scrollTop = box.scrollHeight;
  });

  const HELP: { cmd: string; what: string }[] = [
    { cmd: 'XVERSION', what: 'versione dell’API ASCII del firmware' },
    { cmd: 'XTYPE', what: 'modello della matrice' },
    { cmd: 'XMAXPORTS', what: 'numero di porte disponibili' },
    { cmd: 'XGET', what: 'stato di tutti gli incroci (chi alimenta ogni uscita)' },
    { cmd: 'SIxxxx', what: 'elenca le uscite alimentate dall’ingresso xxxx' },
    { cmd: 'CIxxxxOyyyy', what: 'collega l’ingresso xxxx all’uscita yyyy' },
    { cmd: 'DIxxxx', what: 'stacca l’ingresso xxxx da tutte le sue uscite' },
    { cmd: 'DOyyyy', what: 'stacca l’uscita yyyy: quel monitor resta senza immagine' },
    { cmd: 'XALARM', what: 'allarmi attivi sul dispositivo' },
  ];
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Console</h1>
      <p>
        Invio diretto dei comandi ASCII della matrice {app.activeProfile?.name ?? '—'}. Le risposte
        grezze restano qui sotto: usala solo se sai cosa stai mandando.
      </p>
    </div>
    <div class="row">
      <span class="badge neutral">
        <Icon name="terminal" size={13} stroke={2} />{app.consoleEntries.length} comandi
      </span>
      <button class="btn" disabled={app.consoleEntries.length === 0} onclick={() => app.clearConsole()}>
        <Icon name="trash" size={16} /> Svuota
      </button>
    </div>
  </div>

  {#if !app.connected}
    <div class="callout warn">
      <Icon name="alert" />
      <div>Matrice non connessa: i comandi non partono.</div>
    </div>
  {/if}

  <!-- ------------------------------------------------------------- aiuto breve -->
  <details class="card helpbox" bind:open={helpOpen}>
    <summary>
      <Icon name={helpOpen ? 'chevronDown' : 'chevronRight'} size={16} />
      <span>Comandi utili</span>
      <span class="muted small">porte a 4 cifre, da 0001 a {app.portCount}</span>
    </summary>
    <dl class="helplist">
      {#each HELP as h (h.cmd)}
        <dt class="mono">{h.cmd}</dt>
        <dd>{h.what}</dd>
      {/each}
    </dl>
    <p class="muted small foot">
      Freccia su e freccia giù richiamano i comandi già inviati. Il comando viene messo in maiuscolo e
      gli spazi vengono rimossi.
    </p>
  </details>

  <!-- ------------------------------------------------------------- campo comando -->
  <form class="card entry" class:risk={needsConfirm} onsubmit={(e) => (e.preventDefault(), send())}>
    <div class="field">
      <label class="label" for="cmd-field">Comando ASCII</label>
      <div class="row nowrap">
        <input
          id="cmd-field"
          class="input mono cmd"
          class:warnfield={needsConfirm}
          bind:this={field}
          value={command}
          oninput={onInput}
          onkeydown={onKey}
          disabled={!app.connected}
          placeholder="CI0001O0005"
          autocomplete="off"
          autocapitalize="characters"
          spellcheck="false"
          maxlength="120"
          aria-describedby="cmd-help"
          aria-invalid={needsConfirm}
        />
        <button
          class="btn"
          class:danger={needsConfirm}
          class:primary={!needsConfirm}
          type="submit"
          disabled={!canSend}
        >
          <Icon name={busy ? 'loader' : 'arrowRight'} size={16} /> {busy ? 'Invio…' : 'Invia'}
        </button>
      </div>
      <span class="help" id="cmd-help">
        {#if !app.connected}
          Il campo è disattivato perché la matrice non è connessa: collegala prima di inviare comandi.
        {:else if spacesRemoved}
          Gli spazi non sono ammessi e sono stati rimossi.
        {:else}
          Invio per spedire. Freccia su o giù per richiamare i comandi già inviati ({app.consoleRecall
            .length}).
        {/if}
      </span>
    </div>

    {#if needsConfirm}
      <div class="callout error" role="alert">
        <Icon name="alert" />
        <div>
          <strong>Comando pericoloso.</strong>
          <span class="mono">{norm(command)}</span>
          {dangerInfo(command)} Prima dell’invio viene chiesta una conferma in cui va riscritto il
          comando.
          {#if app.settings?.confirmDangerous === false}
            <span class="muted"
              >La conferma del dispositivo è disattivata nelle impostazioni: resta solo questa.</span
            >
          {/if}
        </div>
      </div>
    {/if}
  </form>

  <!-- ------------------------------------------------------------- trascrizione -->
  <h2 class="section-title">Trascrizione</h2>
  <!-- tabindex: la trascrizione si scorre anche da tastiera (è un riquadro con scorrimento) -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="card flush box"
    bind:this={transcript}
    tabindex="0"
    role="log"
    aria-label="Trascrizione della console"
  >
    {#if empty}
      <div class="empty">
        <p><strong>Nessun comando inviato.</strong></p>
        <p class="small wrap">
          Qui finiscono i comandi che scrivi qui sopra e la risposta grezza della matrice, con ora,
          dati, codice d’errore e tempo di risposta. Serve per provare un comando o per capire perché
          la matrice risponde con un errore.
        </p>
      </div>
    {:else}
      {#each app.consoleEntries as e (e.id)}
        {@const k = kindOf(e)}
        <article class="line {k}">
          <div class="meta row">
            <span class="num small muted">{fmtTime(e.ts, true)}</span>
            {#if k === 'ok'}
              <span class="badge ok"><Icon name="checkCircle" size={13} stroke={2} />Riuscito</span>
            {:else if k === 'matrix'}
              <span class="badge error">
                <Icon name="errorCircle" size={13} stroke={2} />Errore della matrice{e.result
                  ?.errorCode
                  ? ` ${e.result.errorCode}`
                  : ''}
              </span>
            {:else}
              <span class="badge warn">
                <Icon name="plugOff" size={13} stroke={2} />Nessuna risposta
              </span>
            {/if}
            {#if e.result}
              <span class="num small muted" title="Tempo di risposta">{e.result.latencyMs} ms</span>
            {/if}
            <button class="btn sm ghost again" disabled={!app.connected || busy} onclick={() => reuse(e.command)}>
              Riprendi
            </button>
          </div>
          <pre class="sent mono"><span class="prompt" aria-hidden="true">&gt;</span> {e.command}</pre>
          {#if e.error}
            <pre class="resp mono bad">{e.error}</pre>
          {:else if e.result}
            <pre class="resp mono" class:bad={k === 'matrix'}>{e.result.response ??
                e.result.error ??
                '(nessuna risposta)'}</pre>
            {#if e.result.data}
              <div class="kv"><span class="k">Dati</span><pre class="mono val">{e.result.data}</pre></div>
            {/if}
            {#if e.result.errorCode}
              <div class="kv">
                <span class="k">Codice d’errore</span><span class="mono val">{e.result.errorCode}</span>
              </div>
            {/if}
            {#if e.result.comment}
              <div class="kv">
                <span class="k" title="Testo del firmware, in inglese">Commento del firmware</span>
                <span class="mono val">{e.result.comment}</span>
              </div>
            {/if}
          {/if}
        </article>
      {/each}
    {/if}
  </div>
</div>

<style>
  /* ----------------------------------------------------------- aiuto breve */
  details.helpbox {
    padding: 0;
    margin-bottom: 14px;
  }
  details.helpbox summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    cursor: pointer;
    font-weight: 700;
    font-size: 13.5px;
    list-style: none;
    min-height: 40px;
  }
  details.helpbox summary::-webkit-details-marker {
    display: none;
  }
  details.helpbox summary:hover {
    background: var(--ui-hover);
  }
  details.helpbox summary :global(svg) {
    color: var(--ui-text-muted);
    flex: none;
  }
  .helplist {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 14px;
    margin: 0;
    padding: 2px 16px 10px;
    font-size: 13px;
  }
  .helplist dt {
    font-weight: 700;
  }
  .helplist dd {
    margin: 0;
    color: var(--ui-text-muted);
  }
  .foot {
    padding: 0 16px 12px;
  }

  /* ----------------------------------------------------------- campo comando */
  .entry {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .entry.risk {
    border-color: var(--ui-danger-fg);
    box-shadow: inset 3px 0 0 var(--ui-danger-fg);
  }
  .row.nowrap {
    flex-wrap: nowrap;
  }
  .cmd {
    letter-spacing: 0.06em;
    font-size: 15px;
    min-height: 40px;
  }
  .cmd.warnfield {
    border-color: var(--ui-danger-fg);
    background: var(--ui-danger-bg);
  }
  .entry .btn[type='submit'] {
    min-height: 40px;
    min-width: 120px;
  }

  /* ----------------------------------------------------------- trascrizione */
  .box {
    max-height: 48vh;
    overflow: auto;
  }
  .line {
    padding: 8px 14px 10px;
    border-bottom: 1px solid var(--ui-divider);
    border-left: 3px solid transparent;
  }
  .line:last-child {
    border-bottom: 0;
  }
  .line.ok {
    border-left-color: var(--ui-success-fg);
  }
  .line.matrix {
    border-left-color: var(--ui-danger-fg);
    background: var(--ui-danger-bg);
  }
  .line.transport {
    border-left-color: var(--ui-warning-fg);
    background: var(--ui-warning-bg);
  }
  .meta {
    gap: 8px;
    margin-bottom: 4px;
  }
  .again {
    margin-left: auto;
    color: var(--ui-text-muted);
    font-size: 12.5px;
  }
  pre.sent,
  pre.resp {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: var(--mono);
    font-size: 13px;
  }
  pre.sent {
    font-weight: 700;
  }
  .prompt {
    color: var(--ui-primary);
    font-weight: 700;
  }
  pre.resp {
    color: var(--ui-text-muted);
  }
  pre.resp.bad {
    color: var(--ui-danger-fg);
  }
  .kv {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin-top: 2px;
    font-size: 12.5px;
  }
  .kv .k {
    color: var(--ui-text-muted);
    font-weight: 700;
    flex: none;
  }
  .kv .val {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-size: 12.5px;
  }
  .empty .wrap {
    margin: 6px auto 0;
    max-width: 64ch;
  }
</style>
