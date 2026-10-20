<script lang="ts">
  // Details column: the standard suite's readings (CONS3 and its three components, then right under them how this
  // verdict was made; the published Ryder 2045 model, the additional estimates, and on any stream that is not the native
  // standard-resolution scan the 1545 nm model read as measured), every note that did not make the two under the verdict, and the checks (their standing caveats one click
  // away). The bands, the evidence level and the ZooMS band check live in the band close-ups (Matt, 2026-10-18).
  import FlagMark from './FlagMark.svelte';
  import Icon from './Icon.svelte';
  import { MODEL_NAME, formatModel, formatReading, keepRanges, ordinal, timeLong, trackPos } from '../display';
  import { checkRows, modelAnnotations, verdictNumbers } from '../details';
  import { isLabSpec, scanInstrument } from '../instrument';
  import { noteIcon, pickNotes } from '../notes';
  import { LOWRANGE_EXPLAIN, noteText } from '../strings';
  import { toneForReading, verdictMeta, type Tone } from '../verdict';
  import type { ModelKey, ScanResult } from '../types';

  interface Props {
    scan: ScanResult;
    rank: number | null;
    total: number;
  }
  let { scan, rank, total }: Props = $props();

  const numbers = $derived(verdictNumbers(scan));
  const details = $derived(pickNotes(scan.notes, scan.notesShown).details);
  const verdictTone = $derived(verdictMeta(scan.verdict).tone);
  const muted = $derived(scan.verdict === 'not_bone');

  // Named by where each model's weight sits; the window it reads is shown underneath (DECISIONS 76). Two names are long
  // by nature: the panel shows a short name and moves the detail to the line underneath (Matt, 2026-10-18: no lonely
  // words); the readable export keeps the full names (MODEL_NAME).
  const NAMES: Record<ModelKey, string> = { ...MODEL_NAME, f05: 'N–H set', lowrange: 'Low-range estimate' };
  const WINDOWS: Partial<Record<ModelKey, string>> = {
    wc2045: 'reads 2030–2060 nm',
    wc1500: 'reads 1500–1550 nm',
    f05: '2175 + 2045 nm, OH‑corrected; reads 2030–2060, 2150–2200 and 1500–1550 nm', // OH-corrected with a non-breaking hyphen
    ryder2045: 'Ryder et al. 2026; reads 2030–2060 nm',
    s1r2: 'reads 1500–1550 nm (first derivative)',
    lowrange: 'the narrow 1545 nm band; reads 1533–1557 nm',
    ch1728: 'reads 1716–1740 nm',
  };

  const checks = $derived(checkRows(scan).filter((c) => !c.standing));
  /** The caveats that read the same on every scan: behind an expander (DECISIONS 125). */
  const caveats = $derived(checkRows(scan).filter((c) => c.standing));

  const tone = (k: ModelKey, v: number | null): Tone => (k === 'cons3' ? verdictTone : toneForReading(v));
</script>

{#snippet track(v: number | null, t: Tone, opts: { primary?: boolean; neutral?: boolean; plain?: boolean; cons?: number | null })}
  {@const ty = opts.primary ? 8 : 9.5}
  {@const th = opts.primary ? 6 : 3}
  <div class="trk" class:primary={opts.primary}>
    <svg viewBox="0 0 100 22" preserveAspectRatio="none" aria-hidden="true">
      {#if opts.plain}
        <!-- an estimate that never sets the verdict (or a target that is not bone): a plain track, no verdict zones -->
        <rect x="0" y={ty} width="100" height={th} style="fill: var(--ink-4); fill-opacity: 0.35" />
      {:else}
        <!-- verdict zones (Unlikely / Borderline / Good) with a thin surface gap between them -->
        <rect x="0" y={ty} width={trackPos(0.5) - 0.4} height={th} style="fill: var(--v-low); fill-opacity: var(--zone-op)" />
        <rect x={trackPos(0.5) + 0.4} y={ty} width={trackPos(3) - trackPos(0.5) - 0.8} height={th} style="fill: var(--v-mid); fill-opacity: var(--zone-op)" />
        <rect x={trackPos(3) + 0.4} y={ty} width={100 - trackPos(3) - 0.4} height={th} style="fill: var(--v-good); fill-opacity: var(--zone-op)" />
      {/if}
      {#if opts.primary && v != null && !opts.plain}
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
        {#if opts.neutral || opts.plain}
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
      <span class="name">{NAMES[k]}{#if opts.sub ?? WINDOWS[k]}<small>{keepRanges(opts.sub ?? WINDOWS[k] ?? '')}</small>{/if}</span>
      <span class="v">{k === 'cons3' ? formatReading(r.value, true) : formatModel(r.value)}</span>
      {@render track(r.value, tone(k, r.value), { ...opts, plain: opts.neutral || muted, cons: opts.primary ? null : (scan.models.cons3?.value ?? null) })}
      {#each modelAnnotations(r) as a (a.kind)}
        <!-- grey text, never a colour (PLAN Step 6) -->
        <span class="mnote" data-annot={a.kind}>{keepRanges(a.text)}</span>
      {/each}
      {#if opts.note}<span class="mnote">{keepRanges(opts.note)}</span>{/if}
    </div>
  {/if}
{/snippet}

<aside class="details" aria-label="Scan details">
  <section>
    <!-- the instrument line went: the status bar says it (DECISIONS 125) -->
    <h3><span class="mono file">{scan.file}</span><span class="muted r">{timeLong(scan.acquiredAt)}</span></h3>
    {#if rank}<div class="rankline"><span class="rank">{rank}</span>{ordinal(rank)} most promising of {total} scans in this folder</div>{/if}
  </section>

  {#if scan.models.cons3}
    <section>
      <h3>Collagen models<span class="muted r">standard suite</span></h3>
      {#if muted}
        <!-- a target that is not bone: the readings stay readable, plainly labelled (DECISIONS 125), never dimmed -->
        <div class="nointerp" data-testid="not-interpretable"><b>Not interpretable for this target.</b> The spectrum does not look like bone, so
          these readings mean little here.</div>
      {/if}
      <div class="axisrow">
        <span style="left:0">0</span><span style="left:{trackPos(0.5)}%">0.5</span><span style="left:{trackPos(1)}%">1</span>
        <span style="left:{trackPos(3)}%">3</span><span style="left:{trackPos(6)}%">6</span><span class="pct">%</span>
      </div>
      <div class="models">
        {@render mrow('cons3', {
          primary: true,
          note: 'Median of the three below (the tick on each track). Starting estimate; band evidence can change the verdict. Typical error about ± 1 point below 3%.',
        })}
        <div class="inputs">
          {@render mrow('wc2045', {})}
          {@render mrow('wc1500', {})}
          {@render mrow('f05', {})}
        </div>
        <!-- right under the consensus group: the rule applied and this scan's own numbers (DECISIONS 123, 125) -->
        <details class="how">
          <summary>How this verdict was made</summary>
          {#each numbers as row (row.title)}
            <div class="hrow" data-testid="verdict-number">
              <div class="ht">{row.title}</div>
              {#if row.items}
                <dl class="kv">
                  {#each row.items as it (it.label)}<dt>{it.label}</dt><dd>{it.value}</dd>{/each}
                </dl>
                {#if row.note}<div class="hn">{keepRanges(row.note)}</div>{/if}
              {:else}
                <div class="hd">{keepRanges(row.d)}</div>
              {/if}
            </div>
          {/each}
          <div class="hrow">
            <div class="ht">This result</div>
            <div class="hn mono">{scan.profileId}{scan.ruleStep ? ` · rule ${scan.ruleStep}` : ''}{scan.transfer
                ? ` · ${scan.transfer.id}@${scan.transfer.version}`
                : ''}{isLabSpec(scan.instrumentClass) ? '' : ` · ${scanInstrument(scan)}`} · {scan.engineVersion}{scan.scoreMs != null ? ` · analysed in ${scan.scoreMs.toFixed(1)} ms` : ''}</div>
          </div>
          <details class="rules">
            <summary>How the rules work</summary>
            <div class="hrow"><div class="ht">Verdict input</div><div class="hd">Consensus of three (default)</div></div>
            <div class="hrow">
              <div class="ht">Screening lines</div>
              <div class="hd">{keepRanges(
                  'One verdict for radiocarbon, isotopes and ZooMS: Unlikely below 0.5, Good from 3 (practical screening lines that build in model and extraction error, not what the analysis needs). ZooMS often works below these lines: the ZooMS band check sits with the collagen bands.',
                )}</div>
            </div>
            <div class="hrow">
              <div class="ht">Evidence rule</div>
              <div class="hd">Flat bands lower the verdict. Strong, full-pattern bands can raise it on clean scans. Contaminant and heat signs are flags; they never change the verdict.</div>
            </div>
          </details>
        </details>
        <div class="mdiv">Published model</div>
        {@render mrow('ryder2045', { note: 'Shown for reference; not part of the verdict.' })}
        {#if scan.models.lowrange || scan.models.ch1728}
          <div class="mdiv">Additional estimates · not used for the verdict</div>
          {@render mrow('lowrange', { neutral: true, note: LOWRANGE_EXPLAIN })}
          {@render mrow('ch1728', {
            neutral: true,
            note: 'The C–H band at 1728 nm. Lipids, waxes and many consolidants raise it; the app compares it with the consensus for the C–H note.',
          })}
        {/if}
        {#if scan.models.s1r2}
          <div class="mdiv">Second opinion · read as measured</div>
          {@render mrow('s1r2', { neutral: true, note: 'Reads the scan as measured, with no correction. Not part of the verdict.' })}
        {/if}
      </div>
    </section>
  {/if}

  {#if details.length}
    <section>
      <h3>Also noted</h3>
      {#each details as n (n.key)}
        <div class="dnote" data-note={n.key}><Icon name={noteIcon(n.key)} size={14} color="var(--accent)" /><span>{keepRanges(noteText(n, scan))}</span></div>
      {/each}
    </section>
  {/if}

  <section>
    <h3>Checks</h3>
    <div class="checks">
      {#each checks as c (c.title)}
        <div class="check">
          {#if c.icon === 'flag'}<FlagMark size={15} />{:else}<Icon name={c.icon} size={15} color={c.icon === 'ok' ? 'var(--ink-4)' : 'var(--accent)'} />{/if}
          <b class:quiet={!c.notice}>{c.title}</b><span class="d">{keepRanges(c.d)}</span>
        </div>
      {/each}
    </div>
    {#if caveats.length}
      <details class="caveats">
        <summary>Limits of the checks</summary>
        <div class="checks">
          {#each caveats as c (c.title)}
            <div class="check">
              <Icon name="info" size={15} color="var(--ink-4)" />
              <b class="quiet">{c.title}</b><span class="d">{keepRanges(c.d)}</span>
            </div>
          {/each}
        </div>
      </details>
    {/if}
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
  /* a target that is not bone: a plain label over readings that stay readable */
  .nointerp {
    font-size: var(--fs-sm);
    color: var(--ink-2);
    padding: 7px 10px;
    border-radius: 8px;
    background: var(--panel-2);
    box-shadow: inset 3px 0 0 var(--ink-4), inset 0 0 0 1px var(--line);
  }
  .nointerp b {
    color: var(--ink);
    font-weight: 650;
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
  /* the selected scan's whole name (the list and the verdict card may shorten it): wraps anywhere, never clips */
  h3 .file {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  h3 .r {
    flex: none;
  }
  .rankline {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: var(--fs-sm);
    color: var(--ink-2);
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
  /* "How this verdict was made", under the consensus group: labelled values that scan, one per line */
  .how {
    margin: 2px 0 6px;
    padding: 6px 8px 4px;
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px var(--line-soft);
  }
  .how > summary,
  .rules > summary,
  .caveats > summary {
    cursor: pointer;
    font-size: var(--fs-sm);
    font-weight: 650;
    color: var(--ink-2);
  }
  .how[open] > summary {
    margin-bottom: 6px;
  }
  .hrow {
    padding: 4px 0 5px;
    border-top: 1px dashed var(--line-soft);
    font-size: var(--fs-sm);
  }
  .ht {
    font-weight: 600;
    color: var(--ink-2);
    margin-bottom: 2px;
  }
  .hd {
    color: var(--ink-3);
    font-size: var(--fs-xs);
  }
  .hn {
    color: var(--ink-3);
    font-size: var(--fs-xs);
    margin-top: 2px;
    overflow-wrap: anywhere;
  }
  .kv {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 1px 10px;
    margin: 0;
    font-size: var(--fs-xs);
  }
  .kv dt {
    color: var(--ink-3);
  }
  .kv dd {
    margin: 0;
    color: var(--ink);
    font-weight: 600;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .rules {
    margin-top: 4px;
    padding-top: 4px;
    border-top: 1px dashed var(--line-soft);
  }
  .rules > summary {
    font-weight: 600;
    font-size: var(--fs-xs);
    color: var(--ink-3);
  }
  .rules[open] > summary {
    margin-bottom: 2px;
  }
  .caveats {
    margin-top: 2px;
  }
  .caveats[open] > summary {
    margin-bottom: 8px;
  }
</style>
