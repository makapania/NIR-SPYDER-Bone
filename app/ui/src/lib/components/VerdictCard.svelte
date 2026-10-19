<script lang="ts">
  // The verdict: glyph + word, the reading (display policy), then the compact items (CardItems): every fired flag and strong
  // warning and the card's notes (notes.ts precedence) as short labels with their full text one interaction away, so the card
  // stays short however many fire (Matt, 2026-10-18) and never pushes the spectrum down. The verdict's sentence and the
  // low-range explanation are one interaction away too (tooltip on hover and keyboard focus; click or tap opens them in
  // place); the sentence stays in view on a card with no flag or warning. One verdict for every analysis; flags and notes
  // never change it.
  import CardItems from './CardItems.svelte';
  import VerdictGlyph from './VerdictGlyph.svelte';
  import { cardItems } from '../card';
  import { formatReading, timeLong, keepRanges } from '../display';
  import { isPositive } from '../notes';
  import { LOWRANGE_EXPLAIN, NOTE_TEXT, NOT_BONE_TEXT, UNUSABLE_TEXT } from '../strings';
  import { verdictMeta, verdictSentence } from '../verdict';
  import type { ScanResult } from '../types';

  let { scan, isNewest }: { scan: ScanResult; isNewest: boolean } = $props();

  const uid = $props.id();
  const meta = $derived(verdictMeta(scan.verdict));
  const m = $derived(scan.models.cons3?.value ?? null);
  // the low-range estimate (A2): a quiet, uncoloured line under the reading; it never sets the verdict
  const lr = $derived(scan.models.lowrange?.value ?? null);
  const showLr = $derived(lr != null && Number.isFinite(lr) && scan.verdict !== 'rescan' && scan.verdict !== 'not_bone');
  const items = $derived(cardItems(scan));
  const ruleNote = $derived(items.notes.some((n) => n.note && !isPositive(n.note) && ['plus_d', 'flat_bands', 'lift_good', 'lift_borderline'].includes(n.note)));
  const sentence = $derived(
    ruleNote || scan.verdict === 'rescan' || scan.verdict === 'not_bone'
      ? ''
      : scan.verdictRule === 'no_model_reading'
        ? NOTE_TEXT.no_model_reading({})
        : verdictSentence(scan.verdict),
  );
  /** The sentence in view: on a card with no flag or warning (with one, "looks ample" would read against it; it stays one
   *  interaction away on the verdict word). */
  const sentenceShown = $derived(sentence !== '' && items.alerts.length === 0);
  let openWord = $state(false);
  let openLr = $state(false);
  let quiet = $state<'' | 'word' | 'lr'>('');
  const sig = $derived(scan.scanId);
  $effect(() => {
    void sig;
    openWord = false;
    openLr = false;
  });
  function esc(e: KeyboardEvent, which: 'word' | 'lr') {
    if (e.key !== 'Escape') return;
    quiet = which;
    if (which === 'word') openWord = false;
    else openLr = false;
  }
</script>

<div class="verdict" style="--tint: var(--v-{meta.tone}-tint)">
  <div class="newline">
    {#if isNewest}<span class="tag new">New scan</span>{:else}<span class="tag">Selected</span>{/if}
    <span class="mono file">{scan.file}</span><span>· {timeLong(scan.acquiredAt)}</span>
  </div>

  <div class="vrow">
    <span class="glyphbig"><VerdictGlyph verdict={scan.verdict} size={52} /></span>
    <div class="vtext">
      {#if sentence && !sentenceShown}
        <!-- the sentence, one interaction away when a flag or warning holds the card -->
        <div class="tipwrap">
          <button type="button" class="vword asbtn" class:long={meta.word.length > 16} style="color: var(--v-{meta.tone}-ink)" data-testid="verdict-word"
            aria-expanded={openWord} aria-controls="{uid}-sx" aria-describedby="{uid}-st"
            onclick={() => ((openWord = !openWord), (quiet = 'word'))} onkeydown={(e) => esc(e, 'word')} onmouseleave={() => (quiet = '')} onblur={() => (quiet = '')}>{meta.word}</button>
          <span class="tip" class:off={quiet === 'word' || openWord} role="tooltip" id="{uid}-st">{sentence}</span>
        </div>
      {:else}
        <div class="vword" class:long={meta.word.length > 16} style="color: var(--v-{meta.tone}-ink)" data-testid="verdict-word">{meta.word}</div>
      {/if}
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
        <div class="vmodel" title="The median of three collagen models; each is in the right-hand column">consensus of three models</div>
      {/if}
    </div>
  </div>

  {#if sentence && !sentenceShown}
    <div class="vnote" id="{uid}-sx" hidden={!openWord}>{sentence}</div>
  {/if}

  <!-- under the reading at the card's full width, not indented beside the glyph (DECISIONS 123); one line, the explanation
       one interaction away -->
  {#if showLr}
    <div class="lowrange tipwrap">
      <button type="button" class="asbtn lrbtn" aria-expanded={openLr} aria-controls="{uid}-lx" aria-describedby="{uid}-lt"
        onclick={() => ((openLr = !openLr), (quiet = 'lr'))} onkeydown={(e) => esc(e, 'lr')} onmouseleave={() => (quiet = '')} onblur={() => (quiet = '')}>
        <span data-testid="lowrange">Low-range {formatReading(lr, true)} · not used for the verdict</span><span class="info" aria-hidden="true">i</span>
      </button>
      <span class="tip" class:off={quiet === 'lr' || openLr} role="tooltip" id="{uid}-lt">{keepRanges(LOWRANGE_EXPLAIN)}</span>
      <div class="lrnote" id="{uid}-lx" data-testid="lowrange-note" hidden={!openLr}>{keepRanges(LOWRANGE_EXPLAIN)}</div>
    </div>
  {/if}

  {#if sentenceShown}<div class="vnote">{sentence}</div>{/if}

  <CardItems alerts={items.alerts} notes={items.notes} />
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
  .lowrange {
    font-size: var(--fs-sm);
    color: var(--ink-3);
    line-height: 1.4;
  }
  .lrnote {
    font-size: var(--fs-xs);
  }
  .vnote {
    font-size: var(--fs-md);
    color: var(--ink-2);
    max-width: 44ch;
  }
  /* text that opens in place: drawn as plain text, a button for the keyboard, touch and screen readers */
  .asbtn {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    margin: 0;
    text-align: left;
    cursor: pointer;
  }
  .asbtn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 4px;
  }
  button.vword {
    font-family: var(--font-display);
    font-size: var(--fs-hero);
    line-height: 1.05;
    font-weight: 650;
    letter-spacing: -0.015em;
    text-decoration: underline dotted 1.5px;
    text-decoration-color: color-mix(in srgb, currentColor 35%, transparent);
    text-underline-offset: 5px;
  }
  button.vword.long {
    font-size: 25px;
  }
  .lrbtn {
    display: inline;
    color: var(--ink-3);
    font-size: var(--fs-sm);
    line-height: 1.4;
  }
  .info {
    display: inline-block;
    vertical-align: -2px;
    margin-left: 6px;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px currentColor;
    font: italic 700 9.5px/13px Georgia, serif;
    text-align: center;
    opacity: 0.8;
  }
  .lrnote {
    margin-top: 3px;
    font-size: var(--fs-xs);
  }
  .lrnote[hidden],
  .vnote[hidden] {
    display: none;
  }
  .tipwrap {
    position: relative;
  }
  .tip {
    display: none;
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    width: max-content;
    max-width: 250px;
    padding: 7px 9px;
    border-radius: 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    box-shadow: var(--shadow);
    color: var(--ink-2);
    font: 400 var(--fs-xs) / 1.45 var(--font-ui);
    letter-spacing: 0;
    pointer-events: none;
    text-wrap: pretty;
  }
  .asbtn:hover + .tip,
  .asbtn:focus-visible + .tip {
    display: block;
  }
  .tip.off {
    display: none !important;
  }
  /* short windows (150% scaling, 1366×768): tighter type so the chart stays in view */
  @media (max-height: 800px) {
    .verdict {
      padding: 12px 16px 11px;
      gap: 7px;
    }
    .vword,
    button.vword {
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
</style>
