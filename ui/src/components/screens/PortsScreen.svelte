<script lang="ts">
  import { app } from '../../lib/store.svelte';
  import type { PortLabel, PortRole } from '../../lib/types';
  import { defaultLabel, fmtDbm, inputOf, matchPort, outputsOf, ROLE_LABELS } from '../../lib/util';
  import Icon from '../Icon.svelte';

  function build(): PortLabel[] {
    return Array.from({ length: app.portCount }, (_, i) => ({ ...(app.labels.get(i + 1) ?? defaultLabel(i + 1)) }));
  }
  let draft = $state<PortLabel[]>(build());
  let original = $state(JSON.stringify(build()));
  let q = $state('');
  let sel = $state<Set<number>>(new Set());
  let bulkRole = $state<PortRole | ''>('');
  let bulkGroup = $state('');

  const dirty = $derived(JSON.stringify(draft) !== original);
  const rows = $derived(draft.filter((l) => matchPort(q, l.port, l)));
  const routing = $derived(app.snapshot.routing);
  const diag = $derived(app.snapshot.diagnostics);
  const showDiag = $derived(!!diag && app.snapshot.info?.losSupported !== false);
  const groups = $derived([...new Set(draft.map((l) => l.group).filter(Boolean))].sort());
  const allSel = $derived(rows.length > 0 && rows.every((r) => sel.has(r.port)));

  // se la matrice cambia dimensione, ricostruisci la bozza (solo se non ci sono modifiche)
  $effect(() => {
    const n = app.portCount;
    if (n !== draft.length && !dirty) {
      draft = build();
      original = JSON.stringify(draft);
    }
  });

  function toggleSel(p: number) {
    const s = new Set(sel);
    if (s.has(p)) s.delete(p);
    else s.add(p);
    sel = s;
  }
  function toggleAll() {
    sel = allSel ? new Set() : new Set(rows.map((r) => r.port));
  }
  function applyRole() {
    if (!bulkRole) return;
    for (const l of draft) if (sel.has(l.port)) l.role = bulkRole as PortRole;
  }
  function applyGroup() {
    for (const l of draft) if (sel.has(l.port)) l.group = bulkGroup.trim();
  }

  function isDefault(l: PortLabel) {
    return !l.name.trim() && l.role === 'other' && !l.group.trim() && !l.locked && !l.notes.trim();
  }

  async function save() {
    if (!app.config || !app.activeProfile) return;
    const keep = draft.filter((l) => !isDefault(l)).map((l) => ({ ...l, name: l.name.trim(), group: l.group.trim(), notes: l.notes.trim() }));
    app.config.labels[app.activeProfile.id] = keep;
    if (await app.saveConfig('Etichette delle porte salvate')) original = JSON.stringify(draft);
  }
  function reset() {
    draft = JSON.parse(original);
  }
</script>

<div class="page wide">
  <div class="page-head">
    <div>
      <h1>Porte</h1>
      <p>Nomi, ruoli e gruppi delle porte di {app.activeProfile?.name}. Il ruolo decide dove compare la porta in «Postazioni».</p>
    </div>
    <div class="row">
      <button class="btn" disabled={!dirty} onclick={reset}>Annulla modifiche</button>
      <button class="btn primary" disabled={!dirty} onclick={save}><Icon name="save" size={16} /> Salva etichette</button>
    </div>
  </div>

  <div class="tools row">
    <div class="search">
      <Icon name="search" size={16} />
      <input class="input" type="search" placeholder="Cerca" bind:value={q} aria-label="Cerca porte" />
    </div>
    <div class="bulk row" class:off={!sel.size}>
      <span class="small"><strong class="num">{sel.size}</strong> selezionate</span>
      <select class="select" bind:value={bulkRole} disabled={!sel.size} aria-label="Ruolo per le porte selezionate">
        <option value="">Ruolo…</option>
        <option value="tx">{ROLE_LABELS.tx}</option>
        <option value="rx">{ROLE_LABELS.rx}</option>
        <option value="other">{ROLE_LABELS.other}</option>
      </select>
      <button class="btn sm" disabled={!sel.size || !bulkRole} onclick={applyRole}>Applica ruolo</button>
      <input class="input" list="gruppi" placeholder="Gruppo…" bind:value={bulkGroup} disabled={!sel.size} aria-label="Gruppo per le porte selezionate" />
      <button class="btn sm" disabled={!sel.size} onclick={applyGroup}>Applica gruppo</button>
    </div>
  </div>
  <datalist id="gruppi">{#each groups as g (g)}<option value={g}></option>{/each}</datalist>

  <div class="card flush tablewrap">
    <table class="table">
      <thead>
        <tr>
          <th><input type="checkbox" checked={allSel} onchange={toggleAll} aria-label="Seleziona tutte" /></th>
          <th class="num">Porta</th>
          <th>Nome</th>
          <th>Ruolo</th>
          <th>Gruppo</th>
          <th title="Una porta bloccata non può essere modificata dalla app">Bloccata</th>
          <th>Note</th>
          <th>Riceve da</th>
          <th>Invia a</th>
          {#if showDiag}
            <th>Segnale</th>
            <th class="num">RX</th>
            <th class="num">TX</th>
          {/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as l (l.port)}
          {@const inp = inputOf(routing, l.port)}
          {@const outs = outputsOf(routing, l.port)}
          {@const k = l.port - 1}
          <tr class:selrow={sel.has(l.port)}>
            <td><input type="checkbox" checked={sel.has(l.port)} onchange={() => toggleSel(l.port)} aria-label={`Seleziona porta ${l.port}`} /></td>
            <td class="num"><strong>{l.port}</strong></td>
            <td><input class="input" bind:value={l.name} placeholder={`Porta ${l.port}`} maxlength="60" aria-label={`Nome porta ${l.port}`} /></td>
            <td>
              <select class="select role" bind:value={l.role} aria-label={`Ruolo porta ${l.port}`}>
                <option value="tx">PC</option>
                <option value="rx">Postazione</option>
                <option value="other">Altro</option>
              </select>
            </td>
            <td><input class="input grp" list="gruppi" bind:value={l.group} aria-label={`Gruppo porta ${l.port}`} /></td>
            <td class="center">
              <label class="lockbox" title={l.locked ? 'Bloccata' : 'Libera'}>
                <input type="checkbox" bind:checked={l.locked} aria-label={`Porta ${l.port} bloccata`} />
                <Icon name={l.locked ? 'lock' : 'unlock'} size={15} />
              </label>
            </td>
            <td><input class="input" bind:value={l.notes} aria-label={`Note porta ${l.port}`} /></td>
            <td class="rt">{#if inp}<span class="num">{inp}</span> <span class="muted">{app.name(inp)}</span>{:else}<span class="muted">—</span>{/if}</td>
            <td class="rt">{#if outs.length}<span class="num">{outs.join(', ')}</span>{:else}<span class="muted">—</span>{/if}</td>
            {#if showDiag && diag}
              <td>
                {#if diag.los[k] === true}<span class="badge error"><Icon name="errorCircle" size={13} stroke={2} />Nessuna luce</span>
                {:else if diag.los[k] === false}<span class="badge ok"><Icon name="checkCircle" size={13} stroke={2} />OK</span>
                {:else}<span class="muted">—</span>{/if}
              </td>
              <td class="num">{fmtDbm(diag.rxUw[k])}</td>
              <td class="num">{fmtDbm(diag.txUw[k])}</td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>

<style>
  .wide {
    max-width: none;
  }
  .tools {
    margin-bottom: 12px;
    justify-content: space-between;
  }
  .search {
    position: relative;
    width: 240px;
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--ui-text-muted);
  }
  .search .input {
    padding-left: 32px;
  }
  .bulk {
    gap: 8px;
    padding: 4px 4px 4px 12px;
    border-radius: var(--r-md);
    background: var(--ui-surface);
  }
  .bulk .select {
    width: 200px;
  }
  .bulk .input {
    width: 160px;
  }
  .bulk.off {
    opacity: 0.7;
  }
  .tablewrap {
    overflow: auto;
  }
  .table td {
    padding: 3px 8px;
  }
  .table .input {
    min-width: 120px;
    border-color: transparent;
    background: transparent;
  }
  .table .input:hover,
  .table .input:focus-visible,
  .table .select:hover {
    border-color: var(--ui-border);
    background: var(--ui-bg);
  }
  .role {
    min-width: 120px;
  }
  .grp {
    min-width: 130px;
  }
  .center {
    text-align: center;
  }
  .lockbox {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
    min-height: 32px;
  }
  .lockbox :global(svg) {
    color: var(--ui-text-muted);
  }
  .rt {
    white-space: nowrap;
    font-size: 13px;
  }
  tr.selrow td {
    background: var(--ui-selected-bg);
  }
</style>
