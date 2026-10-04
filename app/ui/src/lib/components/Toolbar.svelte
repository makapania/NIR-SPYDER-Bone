<script lang="ts">
  // One verdict for every analysis (DECISIONS 80 amended): no analysis picker. The verdict serves radiocarbon,
  // isotopes and ZooMS alike; a ZooMS line under the verdict marks scans that look better for ZooMS.
  import Icon from './Icon.svelte';
  import { watchPill, watchTitle } from '../live';
  import type { InstrumentClass, SessionInfo, WatchStatus } from '../types';

  interface Props {
    session: SessionInfo | null;
    theme: 'dark' | 'light';
    cvd: boolean;
    onCvd: () => void;
    onClass: (c: InstrumentClass) => void;
    onOpenFiles: () => void;
    onOpenFolder: () => void;
    /** Live folder watching status (null: not watching). */
    watch: WatchStatus | null;
    onWatch: () => void;
    onPause: () => void;
    onResume: () => void;
    onStop: () => void;
    onTheme: () => void;
    /** "Export CSV…" (the session's scans). */
    onExport: () => void;
    /** Nothing to export yet. */
    exportDisabled?: boolean;
    /** Help and About (F1). */
    onHelp: () => void;
  }
  let {
    session,
    theme,
    cvd,
    watch,
    onCvd,
    onClass,
    onOpenFiles,
    onOpenFolder,
    onWatch,
    onPause,
    onResume,
    onStop,
    onTheme,
    onExport,
    exportDisabled = false,
    onHelp,
  }: Props = $props();

  const pill = $derived(watchPill(watch));

  const classTitle = $derived(
    !session
      ? ''
      : session.classSource === 'serial_preset'
        ? `Preset from serial ${session.serial} (a known ${session.serialClass === 'hires' ? 'high-res' : 'standard'} unit). Flip it if that is wrong; the folder remembers your choice.`
        : session.classSource === 'header_preset'
          ? `Preset from the files' detector settings (they look like a ${session.instrumentClass === 'hires' ? 'high-res' : 'standard'} unit). Only a hint: flip it if that is wrong; the folder remembers your choice.`
        : session.classSource === 'default'
          ? 'Standard until you set it. Flip it if this unit is high-res; the folder remembers your choice.'
          : 'Set by you for this folder. Flipping it recomputes every scan.',
  );
</script>

<header class="titlebar">
  <div class="brand">
    <svg width="24" height="24" viewBox="0 0 22 22" aria-hidden="true">
      <rect x="1" y="1" width="20" height="20" rx="6" fill="var(--ink)" />
      <path d="M4 13.5 C6 13.5 6.6 8 8.2 8 S10 15 11.6 15 S13.2 6 15 6 S16.8 12 18 12" fill="none" stroke="var(--panel)"
        stroke-width="1.6" stroke-linecap="round" />
    </svg>
    <span class="lbl-brand">SPYDER Bone</span>
  </div>

  <div class="group">
    <button class="btn" onclick={onOpenFiles} title="Open .asd files (Ctrl+O)"><Icon name="file" size={14} />Open files</button>
    <button class="btn" onclick={onOpenFolder} title="Open a folder of .asd files (Ctrl+Shift+O)"><Icon name="folder" size={14} />Open folder</button>
    <div class="watchctl" data-state={watch?.state ?? 'idle'}>
      <button class="live {pill.tone}" onclick={onWatch} title={watchTitle(watch)} data-testid="watch-pill">
        <span class="dot"></span><b>{pill.verb}</b>
        {#if pill.path}<span class="path mono">{pill.path}</span>{/if}
        {#if watch && watch.mode === 'poll' && watch.state !== 'folder_missing'}<span class="poll">every {Math.round(watch.intervalMs / 1000)} s</span>{/if}
      </button>
      {#if watch && watch.state !== 'stopped'}
        {#if watch.state === 'paused'}
          <button class="iconbtn sm" onclick={onResume} aria-label="Resume watching" title="Resume watching (scans saved meanwhile are picked up)"><Icon name="play" size={13} /></button>
        {:else}
          <button class="iconbtn sm" onclick={onPause} aria-label="Pause watching" title="Pause: new scans wait until you resume"><Icon name="pause" size={13} /></button>
        {/if}
        <button class="iconbtn sm" onclick={onStop} aria-label="Stop watching" title="Stop watching this folder (the scans stay in the list)"><Icon name="stop" size={12} /></button>
      {/if}
    </div>
  </div>

  <div class="group" title={classTitle}>
    <span class="eyebrow">Instrument</span>
    <div class="seg" role="group" aria-label="Instrument class">
      <button aria-pressed={session?.instrumentClass === 'standard'} onclick={() => onClass('standard')}>Standard</button>
      <button aria-pressed={session?.instrumentClass === 'hires'} onclick={() => onClass('hires')}>High-res</button>
    </div>
  </div>

  <button class="btn" onclick={onExport} disabled={exportDisabled} data-testid="export-csv"
    aria-label="Export CSV"
    title="Export every scan in the list as a CSV (UTF-8): verdicts, rule steps, notes (the ZooMS line included), unrounded readings, band states, the ZooMS band check and the other checks."><Icon
      name="export" size={14} /><span class="lbl-export">Export CSV…</span></button>

  <div class="spacer"></div>

  <button class="chip cvd" aria-pressed={cvd} onclick={onCvd} aria-label="Colour-blind safe"
    title="Colour-blind safe: swap the chart colours for a palette that stays distinct under colour-vision deficiency. Glyph shapes carry every meaning either way."><Icon name="eye" size={14} /><span class="lbl-cvd">Colour-blind safe</span></button>
  <button class="iconbtn" onclick={onTheme} aria-label="Switch to {theme === 'dark' ? 'light' : 'dark'} theme"
    title="Light / dark"><Icon name="theme" size={17} /></button>
  <button class="iconbtn" onclick={onHelp} aria-label="Help and About" title="Help and About (F1)" data-testid="help-btn"><Icon
      name="help" size={17} /></button>
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: 18px;
    padding-inline: 16px;
    border-bottom: 1px solid var(--line);
    background: var(--panel-2);
    min-width: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    font-weight: 650;
    font-size: 16.5px;
    letter-spacing: 0.005em;
    white-space: nowrap;
  }
  .cvd[aria-pressed='true'] {
    color: var(--accent);
    border-color: var(--accent);
  }
  .group {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .live {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    border: 0;
    cursor: pointer;
    padding: 5px 12px 5px 10px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--ink-2);
    font-size: var(--fs-sm);
  }
  .live b {
    color: var(--ink);
    font-weight: 600;
    white-space: nowrap;
    flex: none;
  }
  .watchctl {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
  .live.idle {
    background: none;
    box-shadow: inset 0 0 0 1px var(--line);
    flex: none;
  }
  .titlebar :global(.btn),
  .titlebar .seg,
  .titlebar .chip,
  .titlebar .iconbtn {
    flex: none;
  }
  .live.idle .dot,
  .live.paused .dot {
    background: var(--ink-3);
  }
  .live.idle .dot::after,
  .live.paused .dot::after,
  .live.missing .dot::after {
    display: none;
  }
  .live.missing .dot {
    background: none;
    box-shadow: inset 0 0 0 1.5px var(--ink-3);
  }
  .live.paused,
  .live.missing {
    background: var(--line-soft);
  }
  .poll {
    font-size: var(--fs-xs);
    color: var(--ink-3);
    white-space: nowrap;
  }
  .iconbtn.sm {
    width: 26px;
    height: 26px;
  }
  .live .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 190px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    position: relative;
    flex: none;
  }
  .dot::after {
    content: '';
    position: absolute;
    inset: -4px;
    border-radius: 50%;
    border: 1.5px solid var(--accent);
    opacity: 0;
    animation: ping 2.4s var(--ease) infinite;
  }
  @keyframes ping {
    0% {
      transform: scale(0.4);
      opacity: 0.7;
    }
    80%,
    100% {
      transform: scale(1.4);
      opacity: 0;
    }
  }
  .eyebrow {
    white-space: nowrap;
  }
  /* Laptops first (Matt, 2026-10-03): as the window narrows, labels give way in this order (eyebrows, the
     colour-blind label, the poll interval, the app name, the export label) so no button ever clips its text.
     Checked from the 1100 px minimum window up. Without the analysis picker (DECISIONS 80 amended) the labels
     stay longer; about 230 px stays free for a watched folder's path. */
  @media (max-width: 1390px) {
    .eyebrow {
      display: none;
    }
  }
  @media (max-width: 1300px) {
    .lbl-cvd {
      display: none;
    }
  }
  @media (max-width: 1180px) {
    .titlebar {
      gap: 12px;
    }
    .live .path {
      max-width: 120px;
    }
    .poll {
      display: none;
    }
  }
  @media (max-width: 1140px) {
    .lbl-brand {
      display: none;
    }
    .lbl-export {
      display: none;
    }
    .live .path {
      max-width: 90px;
    }
  }
</style>
