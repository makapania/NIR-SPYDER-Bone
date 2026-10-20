<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import BandCloseUps from './lib/components/BandCloseUps.svelte';
  import DetailsPanel from './lib/components/DetailsPanel.svelte';
  import HelpDialog from './lib/components/HelpDialog.svelte';
  import { instrumentName, statusInstrument } from './lib/instrument';
  import ScanList from './lib/components/ScanList.svelte';
  import SpectraCard, { type Mode } from './lib/components/SpectraCard.svelte';
  import SpectrumPlot, { type PlotSeries } from './lib/components/SpectrumPlot.svelte';
  import Toolbar from './lib/components/Toolbar.svelte';
  import VerdictCard from './lib/components/VerdictCard.svelte';
  import { api } from './lib/api';
  import { classifyDrop } from './lib/drop';
  import { readableCsv } from './lib/readable';
  import { arrivalMessage, folderName, incompleteMessage, watchStatusLine } from './lib/live';
  import { isUnscored, orderScans, promisingRanks, type SortMode } from './lib/sort';
  import { timeLong } from './lib/display';
  import { LatestOnly, chartIdentity, chartKey, shownViews, type HeldViews } from './lib/views';
  import { clampListWidth, columnDefaults, listWidthBounds, parseListWidth } from './lib/layout';
  import type {
    Analysis,
    AppInfo,
    DisplayInfo,
    InstrumentClass,
    IpcSelfTest,
    ReferenceSpectrum,
    ScanResult,
    SessionInfo,
    SpectrumViews,
    WatchArrival,
    WatchStatus,
  } from './lib/types';

  // ---- persisted per-viewer preferences (theme only; dark is the default) ----
  const THEME_KEY = 'spyder.theme';
  function loadTheme(): 'dark' | 'light' {
    try {
      return localStorage.getItem(THEME_KEY) === 'light' ? 'light' : 'dark';
    } catch {
      return 'dark';
    }
  }

  const D2_KEY = 'spyder.d2';
  /** 2nd-derivative orientation: bands up (the default) or the conventional bands down. */
  function loadBandsDown(): boolean {
    try {
      return localStorage.getItem(D2_KEY) === 'down';
    } catch {
      return false;
    }
  }

  // the scans list's width, when the viewer has widened it (null: the window's default)
  const LIST_KEY = 'spyder.listWidth';
  function loadListWidth(): number | null {
    try {
      return parseListWidth(localStorage.getItem(LIST_KEY));
    } catch {
      return null;
    }
  }
  function saveListWidth(w: number | null) {
    try {
      if (w == null) localStorage.removeItem(LIST_KEY);
      else localStorage.setItem(LIST_KEY, String(w));
    } catch {
      /* not remembered; fine */
    }
  }

  const CVD_KEY = 'spyder.cvd';
  function loadCvd(): boolean {
    try {
      return localStorage.getItem(CVD_KEY) === 'on';
    } catch {
      return false;
    }
  }

  // Screenshot / test hooks: #light, #promising, #scan=Spectrum00039, #asmeasured, #bandsdown
  const hash = typeof location !== 'undefined' ? location.hash.replace('#', '').split('&') : [];
  const has = (k: string) => hash.includes(k);
  const hashScan = hash.find((h) => h.startsWith('scan='))?.slice(5);

  let theme = $state<'dark' | 'light'>(has('light') ? 'light' : has('dark') ? 'dark' : loadTheme());
  let cvd = $state(has('cvd') || loadCvd());
  // One verdict for every analysis (DECISIONS 80 amended): the radiocarbon / isotopes profile, with the ZooMS line.
  const analysis: Analysis = 'radiocarbon';
  let session = $state<SessionInfo | null>(null);
  let scans = $state<ScanResult[]>([]);
  let selectedId = $state<string | null>(null);
  let newestId = $state<string | null>(null);
  let arrivedId = $state<string | null>(null);
  let sortMode = $state<SortMode>(has('promising') ? 'promising' : 'newest');
  let organicOnly = $state(false);
  let mode = $state<Mode>('D2');
  let smoothing = $state(31);
  let bandsDown = $state(has('bandsdown') || loadBandsDown());
  let lens = $state<'ohc' | 'raw'>(has('asmeasured') ? 'raw' : 'ohc');
  /** Main plot of a corrected scan: transferred (what the models read) or as measured. */
  let stream = $state<'std' | 'meas'>('std');
  let display = $state<DisplayInfo | null>(null);
  let refsOn = $state<Record<string, boolean>>({ '0%': true, '1%': true, '3%': true, '6%': true, '10%': true });
  /** The selected scan's display arrays and what they were fetched for (see lib/views.ts). */
  let heldViews = $state<HeldViews<SpectrumViews> | null>(null);
  let refs = $state<ReferenceSpectrum[]>([]);
  let refs31 = $state<ReferenceSpectrum[]>([]);
  let message = $state('');
  let highlight = $state(-1);
  let listWidth = $state<number | null>(loadListWidth());
  let viewport = $state(typeof window !== 'undefined' ? window.innerWidth : 1440);
  /** The list's width as drawn: the viewer's choice kept within this window's bounds, else the window's default. */
  const listShown = $derived(listWidth == null ? columnDefaults(viewport).list : clampListWidth(listWidth, viewport));
  const listBounds = $derived(listWidthBounds(viewport));
  function setListWidth(w: number) {
    listWidth = clampListWidth(w, viewport);
    saveListWidth(listWidth);
  }
  function resetListWidth() {
    listWidth = null;
    saveListWidth(null);
  }
  let info = $state<AppInfo | null>(null);
  let ipc = $state<IpcSelfTest | null>(null);
  // live folder watching
  let watch = $state<WatchStatus | null>(null);
  /** True once a real folder replaced the example folder (stays true after Stop: its scans stay listed). */
  let live = $state(false);
  /** Live arrivals not clicked yet: tagged NEW. */
  let unseen = $state<ReadonlySet<string>>(new Set());

  const themeKey = $derived(`slate-${theme}-${cvd ? 'cvd' : 'vivid'}`);
  const ordered = $derived(orderScans(scans, sortMode, organicOnly));
  const ranks = $derived(promisingRanks(scans));
  const selected = $derived(scans.find((s) => s.scanId === selectedId) ?? null);
  /** Session generation (in the scan id) + input bytes + class: the arrays of anything else are never shown. */
  const viewIdentity = $derived(selected && !isUnscored(selected) ? chartIdentity(selected) : null);
  const views = $derived(shownViews(heldViews, viewIdentity));
  const newIds = $derived<ReadonlySet<string>>(live ? unseen : new Set(newestId ? [newestId] : []));
  const scored = $derived(scans.filter((s) => !isUnscored(s)).length);

  // ---- performance (PLAN section 6: < 100 ms from an accepted file to verdict and plot) ----
  interface PerfRecord {
    file: string;
    /** spyder-core analysis in Rust. */
    scoreMs: number;
    /** From the watcher accepting the file (its snapshot) to the plot's first draw, wall clock. */
    acceptedToPlotMs: number;
    /** From the arrival event reaching the UI to the plot's first draw. */
    eventToPlotMs: number;
    /** The main plot's own build-and-draw time. */
    drawMs: number;
  }
  const perfLog: PerfRecord[] = [];
  let perfWait: { id: string; file: string; t0: number; receivedMs: number; scoreMs: number } | null = null;
  function onMainDrawn(ms: number) {
    const w = perfWait;
    if (!w || selectedId !== w.id || views?.id !== w.id) return;
    perfWait = null;
    perfLog.push({
      file: w.file,
      scoreMs: w.scoreMs,
      acceptedToPlotMs: Date.now() - w.receivedMs,
      eventToPlotMs: performance.now() - w.t0,
      drawMs: ms,
    });
  }

  $effect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.cvd = cvd ? 'on' : 'off';
    try {
      localStorage.setItem(THEME_KEY, theme);
      localStorage.setItem(CVD_KEY, cvd ? 'on' : 'off');
      localStorage.setItem(D2_KEY, bandsDown ? 'down' : 'up');
    } catch {
      /* not persisted; fine */
    }
  });

  async function reload(keepSelection = true) {
    // the session too: an arrival can preset the switch from a known serial (DECISIONS 50)
    [scans, session] = await Promise.all([api.listScans(analysis), api.session()]);
    const newest = orderScans(scans, 'newest')[0];
    newestId = newest?.scanId ?? null;
    if (!keepSelection || !scans.some((s) => s.scanId === selectedId)) {
      const byHash = hashScan ? scans.find((s) => s.file.startsWith(hashScan)) : null;
      selectedId = (byHash ?? newest)?.scanId ?? null;
    }
  }

  // views for the selected scan (binary path), keyed by identity + smoothing; only the latest request's
  // response is used, and it is dropped if the scan on screen changed meanwhile
  const viewRequests = new LatestOnly();
  $effect(() => {
    const identity = viewIdentity;
    const id = selectedId;
    const w = smoothing;
    // any change invalidates the requests still in flight, even when nothing new is needed
    const ticket = viewRequests.begin();
    if (!identity || !id) return;
    const key = chartKey(identity, w);
    if (untrack(() => heldViews?.key) === key) return;
    api
      .scanViews(id, w)
      .then((v) => {
        if (viewRequests.isCurrent(ticket) && chartIdentity(selected) === identity) heldViews = { identity, key, views: v };
      })
      .catch(() => {
        // never leave another scan's (or class's) arrays behind
        if (viewRequests.isCurrent(ticket) && heldViews?.identity !== identity) heldViews = null;
      });
  });
  $effect(() => {
    const w = smoothing;
    void api.references(w).then((r) => (refs = r));
  });

  async function setClass(c: InstrumentClass) {
    session = await api.setInstrumentClass(c);
    // the arrays depend on the class (the transfer): the new class changes the scan's chart identity, so the
    // selected scan's arrays are fetched again and the old class's arrays are never shown beside the new readings
    await reload();
  }
  /** A new session replaced the list (files or a folder opened): no watch, nothing NEW. */
  async function afterOpen(r: { message: string; opened?: boolean; superseded?: boolean } | null) {
    // superseded: a later Open or Watch replaced this one before it finished; that one owns the screen
    if (!r || r.superseded) return;
    message = r.message;
    if (!r.opened) return;
    watch = null;
    live = false;
    unseen = new Set();
    heldViews = null;
    session = await api.session();
    await reload(false);
  }
  async function openFiles() {
    try {
      await afterOpen(await api.openFiles());
    } catch (e) {
      message = `Could not open those files: ${e}`;
    }
  }
  async function openFolder() {
    try {
      await afterOpen(await api.openFolder());
    } catch (e) {
      message = `Could not open that folder: ${e}`;
    }
  }
  /** Files or a folder dragged onto the window open like Open files / Open folder. */
  async function onDrop(paths: string[]) {
    const d = classifyDrop(paths);
    if (d.kind === 'none') {
      message = d.message;
      return;
    }
    try {
      await afterOpen(await api.openPaths(d.kind === 'files' ? d.paths : [d.path], d.kind === 'folder'));
    } catch (e) {
      message = `Could not open ${d.kind === 'folder' ? 'that' : 'those files'}: ${e}`;
    }
  }
  let dropHover = $state(false);
  let helpOpen = $state(false);
  /** The readable export of every listed file (session order), worded as on screen. */
  function readableText(): string {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, '0');
    const at = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
    return readableCsv(scans, { version: info?.version ?? '', exportedAt: at });
  }
  async function exportCsv() {
    try {
      const r = await api.exportCsv(analysis, readableText());
      if (r) message = r.message;
    } catch (e) {
      message = `Could not export: ${e}`;
    }
  }
  function select(id: string) {
    selectedId = id;
    if (unseen.has(id)) {
      const u = new Set(unseen);
      u.delete(id);
      unseen = u;
    }
  }

  // ---- live folder watching ----
  let reloading = false;
  let dirty = false;
  let followTo: string | null = null;
  /** Rebuilds the list now; arrivals during a rebuild are coalesced into one more rebuild (no fixed delay, so
   *  a single arrival reaches the screen at once). */
  async function scheduleReload() {
    if (reloading) {
      dirty = true;
      return;
    }
    reloading = true;
    try {
      do {
        dirty = false;
        const to = followTo;
        followTo = null;
        await reload();
        if (to && scans.some((s) => s.scanId === to)) {
          selectedId = to;
          arrivedId = to;
          setTimeout(() => (arrivedId = arrivedId === to ? null : arrivedId), 900);
        }
      } while (dirty);
    } finally {
      reloading = false;
    }
  }

  async function onArrival(a: WatchArrival) {
    // Follow new arrivals only while the newest scan (or nothing) is selected; never pull the user away.
    const follow =
      followTo !== null || selectedId === null || selectedId === newestId || !scans.some((s) => s.scanId === selectedId);
    const t0 = performance.now();
    const id = await api.scoreArrival(a);
    if (!a.existing) unseen = new Set([...unseen, id]);
    if (follow) followTo = id;
    if (follow && !a.existing && a.scoreMs != null)
      perfWait = { id, file: a.file, t0, receivedMs: a.receivedMs, scoreMs: a.scoreMs };
    const m = arrivalMessage(a);
    if (m) message = m;
    scheduleReload();
  }

  function onWatchStatus(st: WatchStatus) {
    // A stopped watch of another folder (the one just replaced) must not clear the new one.
    if (watch && st.folder !== watch.folder) return;
    watch = st.state === 'stopped' ? null : st;
  }

  async function startLive(st: WatchStatus) {
    watch = st;
    live = true;
    unseen = new Set();
    heldViews = null;
    session = await api.session();
    await reload(false);
  }

  async function watchFolder(path?: string, includeExisting: boolean | null = null) {
    try {
      const r = path ? await api.watchPath(path, includeExisting) : await api.watchFolder();
      if (!r) return;
      message = r.message;
      if (r.status) await startLive(r.status);
    } catch (e) {
      message = `Could not watch that folder: ${e}`;
    }
  }

  // ---- 50 overlaid spectra (PLAN section 6: under 50 ms) ----
  let bench = $state<{ series: PlotSeries[]; done: (ms: number) => void } | null>(null);
  async function benchOverlay(n = 50): Promise<{ n: number; ms: number }> {
    const pool: Float32Array[] = [];
    for (const s of scans) {
      if (pool.length >= n) break;
      if (isUnscored(s) || s.verdict === 'rescan') continue;
      try {
        pool.push((await api.scanViews(s.scanId, smoothing)).views.D2);
      } catch {
        /* skip */
      }
    }
    for (const r of refs) if (pool.length < n) pool.push(r.spectra.views.D2);
    const base = pool.length;
    for (let i = 0; pool.length < n && base > 0; i++) pool.push(pool[i % base]);
    const colours = ['--r0', '--r1', '--r3', '--r6', '--r10'];
    return new Promise((resolve) => {
      bench = {
        series: pool.slice(0, n).map((d, i) => ({ label: `s${i}`, data: d, color: colours[i % 5], width: 1.2 })),
        done: (ms) => {
          bench = null;
          resolve({ n: Math.min(n, pool.length), ms });
        },
      };
    });
  }

  // Test hooks: the same actions as the buttons, without native dialogs (end-to-end runs drive the real app
  // through these; they open nothing a user could not open).
  if (typeof window !== 'undefined') {
    (window as unknown as { __spyderTest: unknown }).__spyderTest = {
      openFolder: async (path: string) => afterOpen(await api.openPaths([path], true)),
      openFiles: async (paths: string[]) => afterOpen(await api.openPaths(paths, false)),
      watch: (path: string, includeExisting: boolean) => watchFolder(path, includeExisting),
      stopWatch: () => stopWatch(),
      setClass: (c: InstrumentClass) => setClass(c),
      setLens: (l: 'ohc' | 'raw') => (lens = l),
      setStream: (st: 'std' | 'meas') => (stream = st),
      setMode: (m: Mode) => (mode = m),
      setSort: (m: SortMode) => (sortMode = m),
      selectFile: (file: string) => {
        const sc = scans.find((x) => x.file === file);
        if (sc) select(sc.scanId);
        return !!sc;
      },
      selectIndex: (i: number) => {
        const sc = ordered[i];
        if (sc) select(sc.scanId);
        return sc?.file ?? null;
      },
      exportTo: (path: string) => api.exportCsvTo(path, analysis, readableText()),
      bench: (n = 50) => benchOverlay(n),
      perf: () => perfLog.slice(),
      state: () => ({
        session,
        count: scans.length,
        verdicts: scans.reduce<Record<string, number>>((m, x) => ((m[x.scanKind && x.scanKind !== 'sample' ? x.scanKind : x.verdict] = (m[x.scanKind && x.scanKind !== 'sample' ? x.scanKind : x.verdict] ?? 0) + 1), m), {}),
        selected: selected?.file ?? null,
        verdict: selected?.verdict ?? null,
        message,
        coreError: info?.coreError ?? null,
      }),
    };
  }
  async function pauseWatch() {
    watch = (await api.watchPause()) ?? watch;
    if (watch) message = `Paused. Scans saved in ${folderName(watch.folder)} meanwhile are picked up when you resume.`;
  }
  async function resumeWatch() {
    watch = (await api.watchResume()) ?? watch;
    message = '';
  }
  async function stopWatch() {
    const f = watch?.folder;
    await api.watchStop();
    watch = null;
    if (f) message = `Stopped watching ${f}. Its scans stay in the list.`;
  }

  async function simulate() {
    const id = await api.simulateScan();
    await reload();
    selectedId = id;
    arrivedId = id;
    setTimeout(() => (arrivedId = null), 900);
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (e.key === 'F1') {
      e.preventDefault();
      helpOpen = true;
      return;
    }
    if (helpOpen) return; // the Help dialog is modal: no scan navigation or Open behind it
    if (t.closest('select, input, textarea')) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') {
      e.preventDefault();
      void (e.shiftKey ? openFolder() : openFiles());
      return;
    }
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      const i = ordered.findIndex((s) => s.scanId === selectedId);
      const j = i + (e.key === 'ArrowDown' ? 1 : -1);
      if (j >= 0 && j < ordered.length) select(ordered[j].scanId);
    }
  }

  let unMenu: (() => void) | null = null;
  let unWatch: (() => void) | null = null;
  let unDrop: (() => void) | null = null;
  onMount(async () => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.cvd = cvd ? 'on' : 'off';
    session = has('standard') ? await api.setInstrumentClass('standard') : await api.session();
    display = await api.displayInfo();
    refs31 = await api.references(31);
    await reload(false);
    info = await api.appInfo();
    ipc = await api.ipcSelfTest();
    unMenu = await api.onMenu((id) => {
      if (id === 'help') helpOpen = true;
      else void (id === 'watch_folder' ? watchFolder() : id === 'open_folder' ? openFolder() : openFiles());
    });
    unDrop = await api.onFileDrop({ hover: (over) => (dropHover = over), drop: (paths) => void onDrop(paths) });
    // Listen first, then resume the folder watched when the app last closed (zero setup on relaunch).
    unWatch = await api.onWatch({
      arrival: (a) => void onArrival(a),
      status: onWatchStatus,
      incomplete: (i) => (message = incompleteMessage(i)),
    });
    try {
      const st = await api.watchResumeLast();
      if (st) {
        await startLive(st);
        message = `Watching ${st.folder} again, as when the app last closed.`;
      }
    } catch (e) {
      message = `Could not resume watching: ${e}`;
    }
  });
  onDestroy(() => {
    unMenu?.();
    unWatch?.();
    unDrop?.();
  });
</script>

<svelte:window onkeydown={onKey} bind:innerWidth={viewport} />

<div class="app">
  <Toolbar version={info?.version ?? null} {session} {theme} {cvd} {watch} onCvd={() => (cvd = !cvd)} onClass={setClass} onOpenFiles={openFiles}
    onOpenFolder={openFolder} onWatch={() => watchFolder()} onPause={pauseWatch} onResume={resumeWatch} onStop={stopWatch}
    onTheme={() => (theme = theme === 'dark' ? 'light' : 'dark')} onExport={exportCsv} exportDisabled={scored === 0}
    onHelp={() => (helpOpen = true)} />

  <div class="body" style={listWidth == null ? undefined : `--list-w: ${listShown}px`}>
    <ScanList scans={ordered} total={scans.length} {selectedId} {newIds} {arrivedId} {sortMode} {organicOnly}
      onSelect={select} onSort={(m) => (sortMode = m)} onOrganic={() => (organicOnly = !organicOnly)}
      width={listShown} widthMin={listBounds.min} widthMax={listBounds.max} onWidth={setListWidth} onWidthReset={resetListWidth} />

    <main class="main">
      {#if info?.coreError}
        <div class="coreerror card" role="alert" data-testid="core-error">
          <b>No verdict model available:</b> {info.coreError}
          <span class="muted">Files are listed but not scored. Reinstalling the app restores the bundled models. A file in your plug-in
            folder{info.userPlugins ? ` (${info.userPlugins})` : ''} replaces a bundled one only under the same id with a higher version;
            one that reuses a bundled file's id and version is not used.</span>
          {#if info.pluginNotes?.length}
            <ul class="muted plugin-notes" data-testid="plugin-notes">{#each info.pluginNotes as n (n)}<li>{n}</li>{/each}</ul>
          {/if}
        </div>
      {/if}
      {#if selected && isUnscored(selected)}
        <section class="card unscored" aria-label="Selected file">
          <div class="mono file">{selected.file} <span class="muted">· {timeLong(selected.acquiredAt)}</span></div>
          {#if selected.scanKind === 'reference'}
            <h2>Reference scan (not scored)</h2>
            <p class="muted">A white-reference save: its sample block is identical to its reference block. It is listed so you can see it
              arrived; there is nothing to score.</p>
          {:else if selected.scanKind === 'unscored'}
            <h2>Not scored</h2>
            <p class="muted">{selected.kindDetail ?? 'No verdict model is available.'}</p>
          {:else}
            <h2>Not readable</h2>
            <p class="muted">This file is not an ASD scan this app can read ({selected.kindDetail ?? 'unknown format'}). It is listed so a
              file never silently disappears.</p>
          {/if}
          <p class="muted mono path">{selected.path}</p>
        </section>
      {:else if selected}
        <section class="card hero" aria-label="Selected scan">
          <VerdictCard scan={selected} isNewest={selected.scanId === newestId} />
          <BandCloseUps scan={selected} {views} refs={refs31} {themeKey} {highlight} onHighlight={(k) => (highlight = k)} {bandsDown} />
        </section>
        <SpectraCard scan={selected} {views} {refs} {refsOn} {mode} {smoothing} {lens} {bandsDown} onBandsDown={(d) => (bandsDown = d)} {stream} ohWindows={display?.ohWindows ?? []}
          {themeKey} {highlight} onMode={(m) => (mode = m)} onSmoothing={(w) => (smoothing = w)} onLens={(l) => (lens = l)}
          onStream={(st) => (stream = st)} onToggleRef={(l) => (refsOn = { ...refsOn, [l]: !refsOn[l] })} {onMainDrawn} />
      {:else}
        <div class="empty card">
          {#if watch}
            <h2>Watching {folderName(watch.folder)}</h2>
            <p class="muted">Scans appear here as they are saved. Every scan (.asd, .sco, .000) gets its own verdict.</p>
          {:else}
            <h2>Open files or a folder</h2>
            <p class="muted">Every scan (.asd, .sco, .000) gets its own verdict. Nothing to set up.</p>
          {/if}
        </div>
      {/if}
    </main>

    {#if selected && !isUnscored(selected)}
      <DetailsPanel scan={selected} rank={sortMode === 'promising' ? (ranks.get(selected.scanId) ?? null) : null} total={scans.length} />
    {/if}
  </div>

  <footer class="statusbar">
    {#if session}
      <span data-testid="instrument-line">{statusInstrument(session, selected ?? null)}</span>
      {#if session.serialClass && session.serialClass !== session.instrumentClass}
        <span class="sep"></span><span class="gentle">Serial {session.serial} is a known {instrumentName(session.serialClass).toLowerCase()} unit</span>
      {/if}
      <span class="sep"></span>
    {/if}
    <span>Verdict from: consensus of three collagen models</span>
    <span class="sep"></span>
    <span>{session?.example
        ? 'Example folder, synthetic spectra'
        : `${scans.length} file${scans.length === 1 ? '' : 's'}${scored !== scans.length ? ` (${scored} scored)` : ''}`}</span>
    {#if watchStatusLine(watch)}<span class="sep"></span><span class="gentle" data-testid="watch-line">{watchStatusLine(watch)}</span>{/if}
    {#if message}<span class="sep"></span><span class="msg" role="status">{message}</span>{/if}
    <span class="spacer"></span>
    {#if ipc}
      <span class="ipc" title="Binary float32 IPC self-test: {ipc.n} values, {ipc.bytes} bytes, {ipc.ms.toFixed(1)} ms">
        {ipc.path === 'tauri-binary' ? 'Binary IPC' : 'Browser preview'} {ipc.ok ? 'ok' : 'FAILED'}{info?.runtime === 'tauri' && !info.coreConnected
          ? ' · no verdict model'
          : ''}
      </span>
      <span class="sep"></span>
    {/if}
    {#if session?.example}<button onclick={simulate}>Simulate a new scan</button>{/if}
  </footer>
</div>

<HelpDialog open={helpOpen} version={info?.version ?? null} pluginNotes={info?.pluginNotes ?? []} userPlugins={info?.userPlugins ?? null}
  onClose={() => (helpOpen = false)} />

{#if dropHover}
  <div class="drophint" aria-hidden="true"><div>Drop scan files (.asd, .sco, .000) or a folder to open them</div></div>
{/if}

{#if bench}
  <div class="bench" aria-hidden="true">
    <SpectrumPlot startNm={350} series={bench.series} xRange={[1100, 2450]} zero {themeKey} ariaLabel="overlay benchmark"
      onDrawn={bench.done} />
  </div>
{/if}

<style>
  .drophint {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.35);
    pointer-events: none;
    z-index: 50;
  }
  .drophint div {
    padding: 18px 26px;
    border: 2px dashed var(--accent);
    border-radius: 12px;
    background: var(--panel);
    color: var(--ink);
    font-weight: 600;
  }
  .app {
    height: 100%;
    display: grid;
    grid-template-rows: 54px minmax(0, 1fr) 30px;
    min-height: 600px; /* = the window's minHeight (tauri.conf.json), so the status bar always shows */
  }
  .coreerror {
    padding: 10px 16px;
    font-size: var(--fs-sm);
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: baseline;
  }
  .bench {
    position: fixed;
    left: 300px;
    top: 120px;
    width: 900px;
    height: 340px;
    background: var(--chart);
    z-index: 50;
  }
  /* --list-w: set when the viewer widens the scans list (lib/layout.ts keeps it within bounds) */
  .body {
    display: grid;
    grid-template-columns: var(--list-w, 272px) minmax(0, 1fr) 344px;
    min-height: 0;
  }
  .main {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    container: maincol / inline-size;
  }
  /* The hero card: the verdict and the collagen bands (BandCloseUps: summary, close-ups, legend). Peak distinctness first
     (Matt, 2026-10-18): the close-ups keep about v0.3's horizontal scale and take the height. Beside the verdict when the
     main column has room for them (a 1280 px window), as v0.3's close-ups were; else they go under it, centred. */
  .hero {
    display: grid;
    grid-template-columns: minmax(290px, 1fr) minmax(0, 1fr);
    grid-template-rows: auto 1fr auto;
    grid-template-areas:
      'verdict sum'
      'verdict foot'
      'plots plots';
  }
  .hero > :global(.verdict) {
    grid-area: verdict;
    border-radius: var(--r-lg) 0 0 0;
  }
  .hero > :global(.bandrow) {
    justify-content: center;
  }
  .hero > :global(.bandfoot) {
    align-self: end;
  }
  /* beside the verdict from a 560 px main column (the 1100 px window): from 720 px (1280 px) at full size, below that as
     narrow as v0.3's close-ups with short titles; under the verdict only when the viewer has widened the scans list on a
     small window. The close-ups' column never goes under their minimum, which follows the text measured in the rendered
     font: a wide font takes a few px from the verdict, down to its own minimum (--verdict-min); where even that is not
     enough (BandCloseUps marks the row .cramped), the close-ups go under the verdict, as on a narrower window */
  @container maincol (min-width: 560px) {
    .hero {
      --verdict-min: 290px;
      grid-template-columns: minmax(var(--verdict-min), 1fr) minmax(min-content, 1fr);
      grid-template-areas:
        'verdict sum'
        'verdict plots'
        'verdict foot';
    }
    .hero > :global(.verdict) {
      border-radius: var(--r-lg) 0 0 var(--r-lg);
    }
    .hero > :global(.bandrow) {
      justify-content: flex-start;
    }
    .hero > :global(.bandfoot) {
      align-self: start;
    }
  }
  @container maincol (min-width: 560px) and (max-width: 719.98px) {
    .hero :global(.bandrow) {
      --narrow: 1;
      gap: 4px;
      padding: 4px 4px 2px;
    }
    .hero :global(.bandrow .cu) {
      min-width: var(--minw-narrow);
      padding: 4px 2px 2px;
    }
    .hero :global(.bandrow .cut .long) {
      display: none;
    }
    .hero :global(.bandrow .cut .short) {
      display: inline;
    }
    .hero :global(.bandrow .brk) {
      width: 8px;
    }
  }
  @container maincol (min-width: 720px) {
    .hero {
      --verdict-min: 300px;
      grid-template-columns: minmax(var(--verdict-min), 1fr) minmax(min-content, 470px);
    }
  }
  /* the close-ups cannot sit beside the verdict at their minimum widths (a wide font on a small window): under it */
  .hero:has(> :global(.bandrow.cramped)) {
    grid-template-columns: minmax(290px, 1fr) minmax(0, 1fr);
    grid-template-areas:
      'verdict sum'
      'verdict foot'
      'plots plots';
  }
  .hero:has(> :global(.bandrow.cramped)) > :global(.verdict) {
    border-radius: var(--r-lg) 0 0 0;
  }
  .hero:has(> :global(.bandrow.cramped)) > :global(.bandrow) {
    justify-content: center;
  }
  .hero:has(> :global(.bandrow.cramped)) > :global(.bandfoot) {
    align-self: end;
  }
  .empty {
    padding: 40px;
    text-align: center;
  }
  .unscored {
    padding: 22px 24px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .unscored h2 {
    margin: 4px 0 0;
  }
  .unscored p {
    margin: 0;
    max-width: 70ch;
  }
  .unscored .path {
    font-size: var(--fs-xs);
    overflow-wrap: anywhere;
  }
  .statusbar {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-inline: 16px;
    font-size: var(--fs-xs);
    color: var(--ink-3);
    border-top: 1px solid var(--line);
    background: var(--panel-2);
    white-space: nowrap;
    overflow: hidden;
  }
  .sep {
    width: 1px;
    height: 12px;
    background: var(--line);
    flex: none;
  }
  .gentle {
    color: var(--ink-2);
  }
  .msg {
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .statusbar button {
    border: 0;
    background: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--fs-xs);
    padding: 0;
  }
  @media (max-width: 1366px) {
    .body {
      grid-template-columns: var(--list-w, 236px) minmax(0, 1fr) 312px;
    }
  }
  /* 150% Windows scaling on a 1920×1080 screen is a 1280×720 CSS viewport */
  @media (max-width: 1300px) {
    .body {
      grid-template-columns: var(--list-w, 216px) minmax(0, 1fr) 290px;
    }
    .main {
      padding: 10px 10px;
      gap: 8px;
    }
  }
</style>
