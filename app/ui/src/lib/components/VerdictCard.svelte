<script lang="ts">
  // The verdict: glyph + word, the reading (display policy), FLAG chips (prominent, own colour; they
  // never change the verdict), the sentence and at most two notes (notes.ts precedence). One verdict for
  // every analysis; the ZooMS line (DECISIONS 80 amended) is a note with its own accent, after the rule's note. The
  // protein-band level sits in the "Collagen bands" header beside it (design pass v2: it saves a row
  // here, and the per-band glyphs already sit on the bands in the close-ups).
  import FlagMark from './FlagMark.svelte';
  import Icon from './Icon.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import { formatReading, timeLong } from '../display';
  import { isFaint, isPositive, isZoomsLine, noteIcon, pickNotes } from '../notes';
  import { NOTE_TEXT, NOT_BONE_TEXT, UNUSABLE_TEXT, flagLabel, flagText, noteText } from '../strings';
  import { verdictMeta, verdictSentence } from '../verdict';
  import type { ScanResult } from '../types';

  let { scan, isNewest }: { scan: ScanResult; isNewest: boolean } = $props();

  const meta = $derived(verdictMeta(scan.verdict));
  const m = $derived(scan.models.cons3?.value ?? null);
  const picked = $derived(pickNotes(scan.notes, scan.notesShown));
  const flags = $derived(scan.signs.filter((s) => s.fired));
  const ruleNote = $derived(picked.headline.some((n) => !isPositive(n.key) && ['plus_d', 'flat_bands', 'lift_good', 'lift_borderline'].includes(n.key)));
  const sentence = $derived(
    ruleNote || scan.verdict === 'rescan' || scan.verdict === 'not_bone'
      ? ''
      : scan.verdictRule === 'no_model_reading'
        ? NOTE_TEXT.no_model_reading({})
        : verdictSentence(scan.verdict),
  );
</script>

<div class="verdict" style="--tint: var(--v-{meta.tone}-tint)">
  <div class="newline">
    {#if isNewest}<span class="tag new">New scan</span>{:else}<span class="tag">Selected</span>{/if}
    <span class="mono file">{scan.file}</span><span>· {timeLong(scan.acquiredAt)}</span>
  </div>

  <div class="vrow">
    <span class="glyphbig"><VerdictGlyph verdict={scan.verdict} size={52} /></span>
    <div class="vtext">
      <div class="vword" class:long={meta.word.length > 16} style="color: var(--v-{meta.tone}-ink)" data-testid="verdict-word">{meta.word}</div>
      {#if scan.verdict === 'rescan'}
        <div class="vsub">{UNUSABLE_TEXT[scan.unusableReason ?? 'low_signal']}</div>
        {#if scan.unusableDetail}<div class="vmodel">{scan.unusableDetail}</div>{/if}
      {:else if scan.verdict === 'not_bone'}
        <div class="vsub">{NOT_BONE_TEXT}</div>
      {:else if scan.verdictRule === 'flat_bands'}
        <div class="vsub">No protein signal at this spot</div>
        <div class="vmodel">Collagen models: {formatReading(m)}</div>
      {:else}
        <div class="vnum"><b>{formatReading(m)}</b> collagen</div>
        <div class="vmodel">consensus of three collagen models</div>
      {/if}
    </div>
  </div>

  {#each flags as f (f.id)}
    <div class="flagbox" data-testid="flag">
      <span class="flagchip"><FlagMark size={15} on />{flagLabel(f.id)}</span>
      <p>{flagText(f.id, f.heatKind)}</p>
    </div>
  {/each}

  {#if sentence}<div class="vnote">{sentence}</div>{/if}

  {#each picked.headline as n (n.key)}
    <div class="note" class:positive={isPositive(n.key)} class:zooms={isZoomsLine(n.key)} class:faint={isFaint(n.key)} data-note={n.key}>
      <Icon name={noteIcon(n.key)} size={16} color={isFaint(n.key) ? 'var(--ink-3)' : 'var(--accent)'} />
      <div>{noteText(n)}</div>
    </div>
  {/each}

</div>

<style>
  .verdict {
    padding: 15px 20px 13px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    border-right: 1px solid var(--line-soft);
    border-radius: var(--r-lg) 0 0 var(--r-lg);
    background: linear-gradient(160deg, var(--tint), transparent 72%);
    min-width: 0;
  }
  .newline {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-sm);
    color: var(--ink-3);
    white-space: nowrap;
    min-width: 0;
  }
  .file {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tag {
    flex: none;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    padding: 1px 8px;
    border-radius: 999px;
    color: var(--ink-3);
    background: var(--line-soft);
  }
  .tag.new {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .vrow {
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .glyphbig {
    animation: fillIn 0.6s var(--ease);
  }
  @keyframes fillIn {
    from {
      transform: scale(0.85);
      opacity: 0.2;
    }
    to {
      transform: none;
      opacity: 1;
    }
  }
  .vtext {
    min-width: 0;
  }
  .vword {
    font-family: var(--font-display);
    font-size: var(--fs-hero);
    line-height: 1.05;
    font-weight: 650;
    letter-spacing: -0.015em;
  }
  .vword.long {
    font-size: 25px;
  }
  .vnum {
    font-size: var(--fs-lg);
    color: var(--ink-2);
    margin-top: 6px;
    white-space: nowrap;
  }
  .vnum b {
    color: var(--ink);
    font-weight: 650;
    font-size: 25px;
    letter-spacing: -0.01em;
  }
  .vsub {
    font-family: var(--font-display);
    font-size: 17.5px;
    font-weight: 600;
    color: var(--ink);
    margin-top: 6px;
    line-height: 1.25;
  }
  .vmodel {
    font-size: var(--fs-sm);
    color: var(--ink-3);
  }
  .vnote {
    font-size: var(--fs-md);
    color: var(--ink-2);
    max-width: 44ch;
  }
  /* chip floats at the start of the text, so a flag costs two or three lines, not four */
  .flagbox {
    display: flow-root;
    padding: 9px 12px 9px 13px;
    border-radius: 10px;
    background: var(--flag-tint);
    box-shadow: inset 3px 0 0 var(--flag), inset 0 0 0 1px var(--flag-edge);
  }
  .flagchip {
    float: left;
    margin: 0 9px 1px 0;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 3px 12px 3px 9px;
    border-radius: 999px;
    background: var(--flag);
    color: var(--flag-on);
    font-weight: 700;
    font-size: var(--fs-md);
    letter-spacing: 0.01em;
  }
  .flagbox p {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--ink);
    line-height: 1.45;
    padding-top: 2px;
  }
  .note {
    border-radius: 9px;
    background: var(--accent-soft);
    padding: 8px 11px;
    font-size: var(--fs-sm);
    color: var(--ink-2);
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 9px;
    line-height: 1.4;
  }
  /* short windows (150% scaling, 1366×768): tighter type so the chart stays in view */
  @media (max-height: 800px) {
    .verdict {
      padding: 12px 16px 11px;
      gap: 7px;
    }
    .vword {
      font-size: 26px;
    }
    .vnum b {
      font-size: 22px;
    }
    .vnum {
      margin-top: 3px;
    }
    .vnote {
      font-size: var(--fs-sm);
    }
  }
  .note.positive {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--ink-4);
    color: var(--ink);
  }
  /* the ZooMS line: the point of DECISIONS 80 (amended), so it reads at a glance; an accent edge, not a verdict colour */
  .note.zooms {
    color: var(--ink);
    font-weight: 550;
    box-shadow: inset 3px 0 0 var(--accent);
  }
  /* the faint protein sign: weak evidence, so quiet (no fill, muted text) */
  .note.faint {
    background: transparent;
    color: var(--ink-3);
    box-shadow: inset 0 0 0 1px var(--line);
  }
</style>
