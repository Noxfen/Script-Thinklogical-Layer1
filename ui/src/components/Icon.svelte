<script lang="ts" module>
  // Icone lineari 24×24 disegnate a mano (tratto = currentColor).
  const P: Record<string, string> = {
    presets: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
    station: 'M3 5h18v11H3zM8 20h8M12 16v4',
    matrix: 'M4 4v16M9 4v16M14 4v16M19 4v16M4 7h16M4 12h16M4 17h16',
    ports: 'M5 7h3v10H5zM10.5 7h3v10h-3zM16 7h3v10h-3zM3 4h18M3 20h18',
    pulse: 'M3 12h4l2.5-6 4 12 2.5-6H21',
    terminal: 'M4 5h16v14H4zM7.5 9.5l3 2.5-3 2.5M12.5 15h4',
    list: 'M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01',
    settings:
      'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1',
    link: 'M9.5 14.5l5-5M10 6.5l1.5-1.5a4 4 0 0 1 5.7 5.7L15.5 12.5M14 17.5l-1.5 1.5a4 4 0 0 1-5.7-5.7l1.7-1.8',
    unlink: 'M10 6.5l1.5-1.5a4 4 0 0 1 5.7 5.7L15.5 12.5M14 17.5l-1.5 1.5a4 4 0 0 1-5.7-5.7l1.7-1.8M4 4l16 16',
    undo: 'M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3',
    refresh: 'M20 11a8 8 0 0 0-14.6-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.6 4.5L20 16M20 20v-4h-4',
    power: 'M12 3v9M6.3 6.5a8 8 0 1 0 11.4 0',
    check: 'M5 12.5l4.5 4.5L19 7.5',
    checkCircle: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12.5l3 3 5-6',
    alert: 'M12 3.5L2.5 20h19zM12 10v4.5M12 17.5h.01',
    errorCircle: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.5h.01',
    lock: 'M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3',
    unlock: 'M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 6.8-1.2',
    plus: 'M12 5v14M5 12h14',
    trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
    edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
    up: 'M12 19V5M6 11l6-6 6 6',
    down: 'M12 5v14M6 13l6 6 6-6',
    save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6',
    search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
    download: 'M12 4v11M7 10.5l5 5 5-5M4 20h16',
    upload: 'M12 20V9M7 13.5l5-5 5 5M4 4h16',
    pause: 'M8 5v14M16 5v14',
    play: 'M7 4.5v15l12-7.5z',
    x: 'M6 6l12 12M18 6L6 18',
    chevronDown: 'M6 9l6 6 6-6',
    chevronRight: 'M9 6l6 6-6 6',
    dot: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z',
    keyboard: 'M3 7h18v10H3zM6.5 10h.01M9.5 10h.01M12.5 10h.01M15.5 10h.01M8 14h8',
    plug: 'M9 3v5M15 3v5M6.5 8h11v3a5.5 5.5 0 0 1-11 0zM12 16.5V21',
    plugOff: 'M9 3v5M15 3v5M6.5 8h11v3a5.5 5.5 0 0 1-11 0zM12 16.5V21M3 3l18 18',
    loader: 'M12 3a9 9 0 1 0 9 9',
    sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M4.9 19.1l1.8-1.8M17.3 6.7l1.8-1.8',
    moon: 'M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z',
    monitorAuto: 'M3 5h18v11H3zM8 20h8M12 16v4M12 5v11',
    arrowRight: 'M4 12h15M14 7l5 5-5 5',
    arrowsLR: 'M4 9h15M15 5l4 4-4 4M20 15H5M9 11l-4 4 4 4',
    video: 'M3 6h12v12H3zM15 10l6-3v10l-6-3z',
    tool: 'M14.5 4.5a4.5 4.5 0 0 0-5.7 5.7L3.5 15.5 8.5 20.5l5.3-5.3a4.5 4.5 0 0 0 5.7-5.7l-2.8 2.8-3.2-.8-.8-3.2z',
    copy: 'M9 9h11v11H9zM5 15V4h11',
    grip: 'M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01',
  };
  export type IconName = keyof typeof P;
</script>

<script lang="ts">
  let {
    name,
    size = 18,
    stroke = 1.75,
    spin = false,
    label,
  }: { name: string; size?: number; stroke?: number; spin?: boolean; label?: string } = $props();
</script>

<svg
  class="icon"
  class:spin
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  <path d={P[name] ?? P.dot} />
</svg>

<style>
  .icon {
    flex: none;
    display: inline-block;
    vertical-align: middle;
  }
  .spin {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
