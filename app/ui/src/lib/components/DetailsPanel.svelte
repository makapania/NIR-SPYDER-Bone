<script lang="ts">
  // Details column: the standard suite's readings (CONS3 and its three components, the published
  // Ryder 2045 model, and on high-res the transfer-free 1545 nm model (first derivative)), the evidence of organics
  // with the ZooMS band check (supporting evidence; DECISIONS 80 amended), the checks, and every note that did not
  // make the two under the verdict.
  import EvidenceGlyph from './EvidenceGlyph.svelte';
  import FlagMark from './FlagMark.svelte';
  import Icon from './Icon.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import { bandDef } from '../bands';
  import { LEVEL_WORD, MODEL_NAME, formatModel, formatReading, ordinal, timeLong, trackPos, zoomsHeadline } from '../display';
  import { checkRows, modelAnnotations } from '../details';
  import { noteIcon, pickNotes } from '../notes';
  import { noteText } from '../strings';
  import { toneForReading, verdictMeta, type Tone } from '../verdict';
  import type { ModelKey, ScanResult, SpectrumViews, ReferenceSpectrum } from '../types';

  interface Props {
    scan: ScanResult;
    rank: number | null;
    total: number;
    views: SpectrumViews | null;
    refs: ReferenceSpectrum[];
    onHighlight: (k: number) => void;
  }
  let { scan, rank, total, views, refs, onHighlight }: Props = $props();

  const hires = $derived(scan.instrumentClass === 'hires');
  const details = $derived(pickNotes(scan.notes, scan.notesShown).details);
  const verdictTone = $derived(verdictMeta(scan.verdict).tone);
  /** The ZooMS band check's call (the band patterns with the 1545 nm vote). */
  const zcall = $derived(scan.zooms.verdict ?? 'cant_tell');
  const muted = $derived(scan.verdict === 'not_bone');

  // Named by where each model's weight sits; the window it reads is shown underneath (DECISIONS 76).
  const NAMES = MODEL_NAME;
  const WINDOWS: Partial<Record<ModelKey, string>> = {
    wc2045: 'reads 2030–2060 nm',
    wc1500: 'reads 1500–1550 nm',
    f05: 'reads 2030–2060, 2150–2200 (and 1500–1550) nm',
    ryder2045: 'Ryder et al. 2026; reads 2030–2060 nm',
    s1r2: 'reads 1500–1550 nm (first derivative)',
  };

  const EVWORDS: Record<string, string> = {
    none: 'No protein signal detected. Significant collagen is very unlikely at this spot.',
    trace: 'Faint organic signal. Some organic matter is present, but not much protein.',
    clear: 'Protein signal present. See the collagen models for how much.',
    strong: 'Strong protein signal. See the collagen models for how much.',
    cant_tell: 'Too noisy to tell at this spot. A lighter or cleaner area, or more averages, may scan better.',
  };

  const ref10 = $derived(refs.find((r) => r.label === '10%'));
  const ref3 = $derived(refs.find((r) => r.label === '3%'));
  function spark(nm: number): { scan: string; ref: string; x0: number; x1: number } | null {
    if (!views || !ref10 || !ref3) return null;
    // the 1545 band is read on the OH-corrected 1500–1550 nm window, which is all that view holds
    const ohc = nm === 1545;
    const key = ohc ? 'D2_31_ohc' : 'D2_31';
    if (ohc && !views.views.D2_31_ohc) return null;
    const i0 = (ohc ? 1500 : nm - 40) - views.startNm;
    const i1 = (ohc ? 1550 : nm + 40) - views.startNm;
    let lo = Infinity;
    let hi = -Infinity;
    for (let i = i0; i <= i1; i++) {
      const v = ref10.spectra.views[key][i];
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
    const span = hi - lo || 1;
    lo -= span * 0.15;
    hi += span * 0.15;
    const X = (i: number) => (((i - i0) / (i1 - i0)) * 100).toFixed(1);
    const Y = (v: number) => (2 + ((hi - Math.max(lo, Math.min(hi, v))) / (hi - lo)) * 22).toFixed(1);
    const path = (d: Float32Array) => {
      let p = '';
      for (let i = i0; i <= i1; i++) p += (p ? 'L' : 'M') + X(i) + ' ' + Y(d[i]);
      return p;
    };
    return { scan: path(views.views[key]), ref: path(ref3.spectra.views[key]), x0: +X(nm - 2 - views.startNm), x1: +X(nm + 2 - views.startNm) };
  }

  const checks = $derived(checkRows(scan));

  const tone = (k: ModelKey, v: number | null): Tone => (k === 'cons3' ? verdictTone : toneForReading(v));
</script>

{#snippet track(v: number | null, t: Tone, opts: { primary?: boolean; neutral?: boolean; cons?: number | null })}
  {@const ty = opts.primary ? 8 : 9.5}
  {@const th = opts.primary ? 6 : 3}
  <div class="trk" class:primary={opts.primary}>
    <svg viewBox="0 0 100 22" preserveAspectRatio="none" aria-hidden="true">
      <!-- verdict zones (Unlikely / Borderline / Good) with a thin surface gap between them -->
      <rect x="0" y={ty} width={trackPos(0.5) - 0.4} height={th} style="fill: var(--v-low); fill-opacity: var(--zone-op)" />
      <rect x={trackPos(0.5) + 0.4} y={ty} width={trackPos(3) - trackPos(0.5) - 0.8} height={th} style="fill: var(--v-mid); fill-opacity: var(--zone-op)" />
      <rect x={trackPos(3) + 0.4} y={ty} width={100 - trackPos(3) - 0.4} height={th} style="fill: var(--v-good); fill-opacity: var(--zone-op)" />
      {#if opts.primary && v != null}
        <rect x={trackPos(v - 1)} y="4" width={trackPos(v + 1) - trackPos(v - 1)} height="14" rx="7" fill="var(--v-{t})" opacity="0.22" />
      {/if}
      {#if opts.cons != null}
        <line x1={trackPos(opts.cons)} x2={trackPos(opts.cons)} y1="4" y2="18" stroke="var(--ink-3)" stroke-width="1.5" vector-effect="non-scaling-stroke">
          <title>Consensus of three</title>
        </line>
      {/if}
    </svg>
    {#if v != null}
      <span class="dot" style="left: {trackPos(v)}%">
        {#if opts.neutral}
          <svg width="12" height="12" aria-hidden="true"><circle cx="6" cy="6" r="4.6" fill="var(--panel)" stroke="var(--ink-2)" stroke-width="1.6" /></svg>
        {:else}
          <svg width={opts.primary ? 15 : 12} height={opts.primary ? 15 : 12} aria-hidden="true">
            <circle cx={opts.primary ? 7.5 : 6} cy={opts.primary ? 7.5 : 6} r={opts.primary ? 6.2 : 4.8} fill="var(--v-{t})" stroke="var(--panel)" stroke-width="1.5" />
          </svg>
        {/if}
      </span>
      {#if v >= 7}<span class="over">›</span>{/if}
    {/if}
  </div>
{/snippet}

{#snippet mrow(k: ModelKey, opts: { primary?: boolean; neutral?: boolean; note?: string; sub?: string })}
  {@const r = scan.models[k]}
  {#if r}
    <div class="mrow" class:primary={opts.primary} data-model={k}>
      <span class="name">{NAMES[k]}{#if opts.sub ?? WINDOWS[k]}<small>{opts.sub ?? WINDOWS[k]}</small>{/if}</span>
      <span class="v">{k === 'cons3' ? formatReading(r.value, true) : formatModel(r.value)}</span>
      {@render track(r.value, tone(k, r.value), { ...opts, cons: opts.primary ? null : (scan.models.cons3?.value ?? null) })}
      {#each modelAnnotations(r) as a (a.kind)}
        <!-- grey text, never a colour (PLAN Step 6) -->
        <span class="mnote" data-annot={a.kind}>{a.text}</span>
      {/each}
      {#if opts.note}<span class="mnote">{opts.note}</span>{/if}
    </div>
  {/if}
{/snippet}

<aside class="details" aria-label="Scan details">
  <section>
    <h3><span class="mono">{scan.file}</span><span class="muted r">{timeLong(scan.acquiredAt)}</span></h3>
    <div class="muted line">
      {hires ? 'LabSpec 4 High-res' : 'LabSpec 4 Standard'} · {scan.classSource === 'user'
        ? 'set by you'
        : scan.classSource === 'default'
          ? 'Standard by default'
          : scan.classSource === 'header_preset'
            ? 'preset from the detector settings'
            : `preset from serial ${scan.serial}`}{hires
        ? ` · transferred to standard resolution (blur + gain${scan.transfer ? `, v${scan.transfer.version}` : ''}, provisional)`
        : ''}
    </div>
    {#if rank}<div class="rankline"><span class="rank">{rank}</span>{ordinal(rank)} most promising of {total} scans in this folder</div>{/if}
  </section>

  {#if scan.models.cons3}
    <section class:dim={muted}>
      <h3>Collagen models<span class="muted r">standard suite</span></h3>
      <div class="axisrow">
        <span style="left:0">0</span><span style="left:{trackPos(0.5)}%">0.5</span><span style="left:{trackPos(1)}%">1</span>
        <span style="left:{trackPos(3)}%">3</span><span style="left:{trackPos(6)}%">6</span><span class="pct">%</span>
      </div>
      <div class="models">
        {@render mrow('cons3', {
          primary: true,
          note: 'Median of the three below (the tick on each track). Sets the verdict. Typical error about ± 1 point below 3%.',
        })}
        <div class="inputs">
          {@render mrow('wc2045', {})}
          {@render mrow('wc1500', {})}
          {@render mrow('f05', {})}
        </div>
        <div class="mdiv">Published model</div>
        {@render mrow('ryder2045', { note: 'Shown for reference; not part of the verdict.' })}
        {#if hires && scan.models.s1r2}
          <div class="mdiv">Second opinion · high-res scans only</div>
          {@render mrow('s1r2', { neutral: true, note: 'Reads the scan as measured, with no transfer. Not part of the verdict.' })}
        {/if}
      </div>
      {#if muted}<p class="muted small">The target does not look like bone, so these readings mean little.</p>{/if}
    </section>
  {/if}

  {#if details.length}
    <section>
      <h3>Also noted</h3>
      {#each details as n (n.key)}
        <div class="dnote" data-note={n.key}><Icon name={noteIcon(n.key)} size={14} color="var(--accent)" /><span>{noteText(n)}</span></div>
      {/each}
    </section>
  {/if}

  {#if scan.evidence.bands.length}
    <section>
      <h3>Evidence of organics<span class="muted r">from the spectrum, no model</span></h3>
      <div class="evsum">
        <EvidenceGlyph state={scan.evidence.level} size={24} />
        <div>
          <b>{LEVEL_WORD[scan.evidence.level]}</b>{#if scan.evidence.s != null}<span class="muted"> · score {scan.evidence.s.toFixed(1)}</span>{/if}
          <div class="evs">{EVWORDS[scan.evidence.level]}</div>
        </div>
      </div>
      <div class="evgrid">
        {#each scan.evidence.bands as b, k (b.id)}
          {@const sp = spark(b.nm)}
          <div class="ecell" class:lx={b.state === 'cant_tell'} role="group" aria-label="{bandDef(b.id).label} {b.nm} nm"
            title={b.evidenceState === 'faint' ? 'Lit, but below this band\'s faint threshold: the organic-evidence level counts it as flat.' : undefined}
            onmouseenter={() => onHighlight(k)} onmouseleave={() => onHighlight(-1)}>
            <span class="nm">{bandDef(b.id).label} {b.nm}{b.id === 'nh2044' ? '*' : b.id === 'nh1545' ? '†' : ''}</span>
            <EvidenceGlyph state={b.state} size={13} />
            {#if sp}
              <svg class="sp" viewBox="0 0 100 26" preserveAspectRatio="none" aria-hidden="true">
                <rect x={sp.x0} y="0" width={sp.x1 - sp.x0} height="26" fill="var(--band-soft)" />
                <path d={sp.ref} fill="none" stroke="var(--r3)" stroke-width="1" stroke-dasharray="3 2" vector-effect="non-scaling-stroke" />
                <path d={sp.scan} fill="none" stroke="var(--s1)" stroke-width="1.6" vector-effect="non-scaling-stroke" />
              </svg>
            {/if}
            <span class="lv">{b.state === 'cant_tell' ? "Can't tell" : b.state[0].toUpperCase() + b.state.slice(1)}</span>
          </div>
        {/each}
      </div>
      <p class="muted small">−d²A/dλ² ±40 nm around each band, shaded where it is read; dashed: 3% reference. *2044 sits on the OH/water band shoulder and can only add evidence. †1545 is read on the OH-corrected 1500–1550 nm window, where collagen shows as a dip. The score orders scans below about 1%; it is not a percentage.</p>
      <div class="zcheck" data-testid="zooms-check">
        <VerdictGlyph verdict={zcall} size={16} />
        <div>
          <b>ZooMS band check: {verdictMeta(zcall).short}</b><span class="muted"> · {zoomsHeadline(scan.zooms, scan.evidence.bands)}</span>
          <div class="evs">The six collagen band patterns, with the 1545 nm band as a vote. Supporting evidence only: it never changes the verdict. Where it calls a scan better than the verdict, a ZooMS line shows under the verdict.</div>
        </div>
      </div>
    </section>
  {/if}

  <section>
    <h3>Checks</h3>
    <div class="checks">
      {#each checks as c (c.title)}
        <div class="check">
          {#if c.icon === 'flag'}<FlagMark size={15} />{:else}<Icon name={c.icon} size={15} color={c.icon === 'ok' ? 'var(--ink-4)' : 'var(--accent)'} />{/if}
          <b class:quiet={!c.notice}>{c.title}</b><span class="d">{c.d}</span>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <details class="adv">
      <summary>Advanced</summary>
      <div class="advrow"><span>Verdict input</span><span class="muted">Consensus of three (default)</span></div>
      <div class="advrow"><span>Screening lines</span><span class="muted">One verdict for radiocarbon, isotopes and ZooMS: Unlikely below 0.5, Good from 3 (practical screening lines that build in model and extraction error, not what the analysis needs). ZooMS often works below these lines, so the ZooMS band check adds a ZooMS line where the band patterns look better than the verdict.</span></div>
      <div class="advrow"><span>Evidence rule</span><span class="muted">Flat bands lower the verdict. Strong, full-pattern bands can raise it on clean scans. Contaminant and heat signs are flags; they never change the verdict.</span></div>
      <div class="advrow"><span>Display smoothing</span><span class="muted">Chart only; the models keep their own settings.</span></div>
      <div class="advrow"><span>This result</span><span class="muted mono small">{scan.profileId}{scan.ruleStep ? ` · rule ${scan.ruleStep}` : ''}{scan.transfer
            ? ` · ${scan.transfer.id}@${scan.transfer.version}`
            : ''} · {scan.engineVersion}{scan.scoreMs != null ? ` · analysed in ${scan.scoreMs.toFixed(1)} ms` : ''}</span></div>
    </details>
  </section>
</aside>

<style>
  .details {
    border-left: 1px solid var(--line);
    background: var(--panel);
    overflow: auto;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  section {
    padding: 15px 16px 14px;
    border-bottom: 1px solid var(--line-soft);
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  section.dim .models {
    opacity: 0.55;
  }
  h3 {
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 650;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  h3 .r {
    margin-left: auto;
    font-weight: 400;
    font-size: var(--fs-sm);
  }
  .line {
    font-size: var(--fs-sm);
  }
  .rankline {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .small {
    font-size: var(--fs-xs);
    margin: 0;
  }
  .models {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mrow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 10px;
    padding: 7px 8px;
    border-radius: 8px;
    align-items: center;
  }
  .mrow.primary {
    background: var(--panel-2);
    box-shadow: inset 0 0 0 1px var(--line);
    padding: 9px 9px 8px;
  }
  .mrow.primary .name {
    font-size: var(--fs-md);
  }
  .mrow.primary .v {
    font-size: var(--fs-lg);
  }
  /* the consensus's three inputs hang off it: a rule in the gutter, indented lighter names. The
     tracks are NOT indented, so every track stays on the shared 0–6% axis. */
  .inputs {
    position: relative;
    display: flex;
    flex-direction: column;
    margin-bottom: 2px;
  }
  .inputs::before {
    content: '';
    position: absolute;
    left: 2px;
    top: -2px;
    bottom: 12px;
    border-left: 1.5px solid var(--line);
  }
  .inputs .mrow {
    padding: 5px 8px 4px;
  }
  .inputs .name {
    font-weight: 500;
    color: var(--ink-2);
    padding-left: 8px;
  }
  .inputs .v {
    font-weight: 600;
    font-size: var(--fs-sm);
  }
  .trk.primary > svg {
    height: 22px;
  }
  .name {
    font-size: var(--fs-sm);
    font-weight: 600;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name small {
    font-weight: 400;
    color: var(--ink-3);
    font-size: var(--fs-xs);
  }
  .v {
    text-align: right;
    font-weight: 650;
    font-size: var(--fs-md);
    white-space: nowrap;
  }
  .mnote {
    grid-column: 1 / -1;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .trk {
    position: relative;
    grid-column: 1 / -1;
  }
  .trk > svg {
    width: 100%;
    height: 22px;
    display: block;
  }
  .dot {
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
    line-height: 0;
  }
  /* block, so the inline-SVG baseline gap does not lift the dot off its track */
  .dot svg {
    display: block;
  }
  .over {
    position: absolute;
    right: -8px;
    top: 50%;
    transform: translateY(-55%);
    font-size: 12px;
    color: var(--ink-3);
  }
  .axisrow {
    position: relative;
    height: 14px;
    font-size: 10px;
    color: var(--ink-3);
    margin: 0 8px;
  }
  .axisrow span {
    position: absolute;
    transform: translateX(-50%);
  }
  .axisrow span:first-child {
    transform: none;
  }
  .axisrow .pct {
    right: 0;
    transform: none;
  }
  .mdiv {
    font-size: var(--fs-xs);
    color: var(--ink-3);
    padding: 8px 8px 0;
    border-top: 1px dashed var(--line);
    margin-top: 4px;
  }
  .dnote {
    display: grid;
    grid-template-columns: 14px 1fr;
    gap: 8px;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    line-height: 1.4;
  }
  .evsum {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .evsum b {
    font-size: var(--fs-lg);
    font-weight: 650;
  }
  .evs {
    font-size: var(--fs-sm);
    color: var(--ink-2);
  }
  .zcheck {
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 9px;
    align-items: start;
    font-size: var(--fs-sm);
    padding-top: 8px;
    border-top: 1px dashed var(--line);
  }
  .zcheck b {
    font-weight: 650;
  }
  .zcheck .evs {
    font-size: var(--fs-xs);
    color: var(--ink-3);
    margin-top: 2px;
  }
  .evgrid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px;
  }
  .ecell {
    border: 1px solid var(--line-soft);
    border-radius: 8px;
    padding: 6px 7px 5px;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px 4px;
    align-items: center;
  }
  .ecell:hover {
    border-color: var(--ink-4);
  }
  .ecell.lx {
    border-style: dashed;
  }
  .nm {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--ink-2);
  }
  .sp {
    grid-column: 1 / -1;
    width: 100%;
    height: 26px;
    display: block;
  }
  .lv {
    grid-column: 1 / -1;
    font-size: var(--fs-xs);
    color: var(--ink-2);
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .check {
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 2px 9px;
    font-size: var(--fs-sm);
    align-items: start;
  }
  .check b {
    font-weight: 600;
    color: var(--ink);
  }
  .check b.quiet {
    color: var(--ink-2);
    font-weight: 500;
  }
  .check .d {
    grid-column: 2;
    color: var(--ink-3);
    font-size: var(--fs-xs);
  }
  .adv summary {
    cursor: pointer;
    font-weight: 650;
  }
  .adv[open] summary {
    margin-bottom: 8px;
  }
  .advrow {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 8px;
    font-size: var(--fs-sm);
    padding: 4px 0;
    color: var(--ink-2);
  }
  .advrow .muted {
    font-size: var(--fs-xs);
  }
</style>
