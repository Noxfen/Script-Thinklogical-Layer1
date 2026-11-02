<script lang="ts">
  import { app } from '../lib/store.svelte';

  let {
    value = $bindable(),
    id,
    label,
    allowZero = false,
    zeroLabel = 'Nessuna',
    disabled = false,
    max,
  }: {
    value: number;
    id?: string;
    label?: string;
    allowZero?: boolean;
    zeroLabel?: string;
    disabled?: boolean;
    max?: number;
  } = $props();

  const n = $derived(max ?? app.portCount);
  const ports = $derived(Array.from({ length: n }, (_, i) => i + 1));
</script>

<select class="select num" {id} aria-label={label} bind:value {disabled}>
  {#if allowZero}<option value={0}>{zeroLabel}</option>{/if}
  {#if value > n}<option value={value}>{value} (fuori dalla matrice)</option>{/if}
  {#each ports as p (p)}
    {@const l = app.labels.get(p)}
    <option value={p}>{p}{l?.name ? ` – ${l.name}` : ''}{l?.locked ? ' (bloccata)' : ''}</option>
  {/each}
</select>
