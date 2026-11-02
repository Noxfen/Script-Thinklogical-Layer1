<script lang="ts">
  // Dialog di conferma / richiesta testo. Accessibile: focus trap, Esc chiude, focus ripristinato.
  import { tick } from 'svelte';
  import { app } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  let box = $state<HTMLDivElement | null>(null);
  let text = $state('');
  let word = $state('');
  let opener: HTMLElement | null = null;

  const d = $derived(app.dialog);
  const wordOk = $derived(!d?.requireWord || word.trim().toUpperCase() === d.requireWord.toUpperCase());
  const textOk = $derived(!d?.input || text.trim().length > 0);
  const canConfirm = $derived(wordOk && textOk);

  $effect(() => {
    if (d) {
      opener = document.activeElement as HTMLElement | null;
      text = d.input?.value ?? '';
      word = '';
      void tick().then(() => {
        const first =
          box?.querySelector<HTMLElement>('input') ??
          box?.querySelector<HTMLElement>(d.danger ? '[data-cancel]' : '[data-confirm]');
        first?.focus();
        if (first instanceof HTMLInputElement) first.select();
      });
    }
  });

  function close(v: string | boolean | null) {
    const back = opener;
    app.closeDialog(v);
    opener = null;
    void tick().then(() => back?.focus?.());
  }

  function confirm() {
    if (!d || !canConfirm) return;
    close(d.input ? text.trim() : true);
  }

  function onKey(e: KeyboardEvent) {
    if (!d) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close(d.input ? null : false);
      return;
    }
    if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'INPUT') {
      e.preventDefault();
      confirm();
      return;
    }
    if (e.key === 'Tab' && box) {
      const f = [...box.querySelectorAll<HTMLElement>('button, input, select, textarea, [tabindex]:not([tabindex="-1"])')].filter(
        (el) => !(el as HTMLButtonElement).disabled,
      );
      if (!f.length) return;
      const first = f[0];
      const last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

{#if d}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onkeydown={onKey} onmousedown={(e) => e.target === e.currentTarget && close(d.input ? null : false)}>
    <div
      class="dialog"
      class:danger={d.danger}
      role="dialog"
      aria-modal="true"
      aria-labelledby="dlg-title"
      aria-describedby="dlg-msg"
      bind:this={box}
    >
      <div class="head">
        {#if d.danger}<Icon name="alert" size={22} stroke={2} />{/if}
        <h2 id="dlg-title">{d.title}</h2>
      </div>
      <p id="dlg-msg" class="msg">{d.message}</p>

      {#if d.input}
        <label class="field">
          <span class="label">{d.input.label}</span>
          <input class="input" bind:value={text} placeholder={d.input.placeholder ?? ''} maxlength="120" />
        </label>
      {/if}

      {#if d.requireWord}
        <label class="field">
          <span class="label">Per confermare scrivi <strong class="word">{d.requireWord}</strong></span>
          <input class="input" bind:value={word} autocomplete="off" spellcheck="false" aria-invalid={!wordOk} />
        </label>
      {/if}

      <div class="actions">
        <button class="btn" data-cancel onclick={() => close(d.input ? null : false)}>{d.cancelLabel ?? 'Annulla'}</button>
        <button class="btn" class:danger={d.danger} class:primary={!d.danger} data-confirm disabled={!canConfirm} onclick={confirm}>
          {d.confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: fade var(--t) ease-out;
  }
  .dialog {
    width: min(480px, 100%);
    background: var(--ui-raised);
    border: 1px solid var(--ui-divider);
    border-radius: var(--r-lg);
    box-shadow: var(--ui-shadow);
    padding: 22px 24px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .dialog.danger {
    border-top: 4px solid var(--tt-error);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .head :global(svg) {
    color: var(--ui-danger-fg);
  }
  h2 {
    font-size: 18px;
  }
  .msg {
    color: var(--ui-text-muted);
  }
  .word {
    font-family: var(--mono);
    letter-spacing: 0.04em;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 4px;
  }
  .actions .btn {
    min-height: 40px;
    min-width: 110px;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
