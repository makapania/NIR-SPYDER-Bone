<script lang="ts">
  // Small line icons for notes and toolbar actions.
  let { name, size = 16, color = 'currentColor' }: { name: string; size?: number; color?: string } = $props();
  const P: Record<string, string> = {
    up: 'M8 13V3M4 7l4-4 4 4',
    down: 'M8 3v10M4 9l4 4 4-4',
    info: 'M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8zM8 7.2v4M8 4.9v.1',
    flask: 'M6 2h4M6.5 2v4L3 13a1 1 0 0 0 .9 1.4h8.2A1 1 0 0 0 13 13L9.5 6V2M4.6 10.5h6.8',
    layers: 'M8 2 2 5l6 3 6-3-6-3zM2 8l6 3 6-3M2 11l6 3 6-3',
    noise: 'M1.5 9.5l1.6-3 1.6 5 1.6-7 1.6 8 1.6-6 1.6 4 1.6-3 1.6 2',
    check: 'M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8zM5.4 8.2l1.8 1.8 3.4-3.6',
    // peptide peaks (a mass spectrum): the ZooMS line
    zooms: 'M1.5 13.8h13M3.5 13.8V9.5M6.2 13.8V4.5M8.9 13.8V7.6M11.6 13.8V2.6',
    file: 'M4 1.8h5l3 3v9.4H4zM9 1.8v3h3',
    folder: 'M1.8 4.2h4.4l1.4 1.6h6.6v7.4H1.8z',
    eye: 'M1.5 8s2.4-4.6 6.5-4.6S14.5 8 14.5 8s-2.4 4.6-6.5 4.6S1.5 8 1.5 8zM8 6.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z',
    theme: 'M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zM8 2v12',
    export: 'M8 2v8M4.5 6.5 8 10l3.5-3.5M3 13h10',
    ok: 'M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8zM5.4 8.2l1.8 1.8 3.4-3.6',
    pause: 'M5.5 3.5v9M10.5 3.5v9',
    play: 'M5 3.2v9.6L12.6 8z',
    stop: 'M4 4h8v8H4z',
    close: 'M4 4l8 8M12 4l-8 8',
    help: 'M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8zM6.2 6.3a1.9 1.9 0 1 1 2.6 1.8c-.5.2-.8.6-.8 1.1v.5M8 11.4v.1',
  };
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke={color} stroke-width="1.5" stroke-linecap="round"
  stroke-linejoin="round" aria-hidden="true" class="icon">
  <path d={P[name] ?? P.info} />
  {#if name === 'theme'}<path d="M8 2a6 6 0 0 1 0 12z" fill={color} stroke="none" />{/if}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
