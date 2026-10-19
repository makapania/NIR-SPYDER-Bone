<script lang="ts">
  // The verdict card's flags, strong warnings and notes in compact form (Matt, 2026-10-18: "room for three banners or
  // more", the rest in tooltips or expanders), so the card stays short however many fire. Each item is a short label; its
  // full text is one interaction away: a tooltip on hover AND keyboard focus (Escape hides it), and a click, tap, Enter or
  // Space that opens it in place (aria-expanded; screen readers also get the text as the button's description). The dark
  // chip is the item: flags (magenta pill) and strong warnings (orange square) as solid chips on the first line(s), the
  // quieter notes as small tinted chips under them. (Slim banners and checklist rows were the other forms tried: ef40ecd.)
  import FlagMark from './FlagMark.svelte';
  import Icon from './Icon.svelte';
  import { keepRanges } from '../display';
  import type { CardItem } from '../card';

  let { alerts, notes }: { alerts: CardItem[]; notes: CardItem[] } = $props();

  const uid = $props.id();
  /** The one item opened in place (null: none). */
  let open = $state<string | null>(null);
  /** The item whose tooltip Escape (or a click) has hidden, until the pointer or focus leaves it. */
  let quiet = $state<string | null>(null);
  // another scan: nothing stays open
  const sig = $derived([...alerts, ...notes].map((i) => i.id + i.body).join('|'));
  $effect(() => {
    void sig;
    open = null;
  });

  const safe = (it: CardItem) => it.id.replace(/[^a-z0-9_-]/gi, '-');
  function toggle(id: string) {
    open = open === id ? null : id;
    quiet = id;
  }
  function onKey(e: KeyboardEvent, id: string) {
    if (e.key === 'Escape') {
      quiet = id;
      if (open === id) open = null;
    }
  }
  const solid = (it: CardItem) => it.tier === 'alert';
  const iconColor = (it: CardItem, onSolid: boolean) =>
    onSolid ? (it.tone === 'flag' ? 'var(--flag-on)' : 'var(--warn-on)') : it.tone === 'faint' ? 'var(--ink-3)' : it.tone === 'flag' ? 'var(--flag)' : it.tone === 'warn' ? 'var(--warn-ink)' : 'var(--accent)';
</script>

{#snippet mark(it: CardItem, onSolid: boolean, size: number)}
  {#if it.icon === 'flag'}<FlagMark {size} on={onSolid} />{:else}<Icon name={it.icon} {size} color={iconColor(it, onSolid)} />{/if}
{/snippet}

{#snippet trigger(it: CardItem)}
  <button type="button" class="trig chip {it.tone}" aria-expanded={open === it.id} aria-controls="{uid}-x-{safe(it)}"
    aria-describedby="{uid}-t-{safe(it)}" onclick={() => toggle(it.id)} onkeydown={(e) => onKey(e, it.id)}
    onmouseleave={() => (quiet = null)} onblur={() => (quiet = null)}>
    {@render mark(it, solid(it), 13)}
    <span class="lbl">{it.label}</span>
    <!-- the caret: this chip opens; it turns over when open -->
    <span class="caret" aria-hidden="true"></span>
  </button>
  <span class="tip" class:off={quiet === it.id || open === it.id} role="tooltip" id="{uid}-t-{safe(it)}">{keepRanges(it.body)}</span>
{/snippet}

{#snippet more(it: CardItem)}
  <div class="more {it.tone}" id="{uid}-x-{safe(it)}" hidden={open !== it.id}>{keepRanges(it.body)}</div>
{/snippet}

{#snippet item(it: CardItem)}
  <div class="item" class:note={it.tier === 'note'} data-note={it.note} data-testid={it.tone === 'flag' ? 'flag' : it.tone === 'warn' ? 'warning' : undefined}>
    {@render trigger(it)}
  </div>
{/snippet}

{#snippet chipGroup(list: CardItem[], small: boolean)}
  {#if list.length}
    <div class="group" class:small>
      {#each list as it (it.id)}{@render item(it)}{/each}
    </div>
    {#each list as it (it.id)}{@render more(it)}{/each}
  {/if}
{/snippet}

<div class="items">
  {@render chipGroup(alerts, false)}
  {@render chipGroup(notes, true)}
</div>

<style>
  .items {
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
  }
  .items:empty {
    display: none;
  }
  .group {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .trig {
    font: inherit;
    border: 0;
    margin: 0;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    text-align: left;
    color: var(--ink);
    background: none;
    min-width: 0;
  }
  .trig:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .lbl {
    min-width: 0;
  }
  /* chips: the dark chip is the whole item */
  /* tight enough that the longest heading ("Collagen estimates disagree strongly") stays on one line with its caret at
     1100 px */
  .chip {
    padding: 3px 7px 3px 6px;
    font-size: var(--fs-sm);
    font-weight: 700;
    line-height: 1.3;
    letter-spacing: -0.01em;
  }
  /* a small chevron in the chip's own colour: down when closed, up when open */
  .caret {
    flex: none;
    width: 5px;
    height: 5px;
    margin: -3px 1px 0 2px;
    border-right: 1.6px solid currentColor;
    border-bottom: 1.6px solid currentColor;
    transform: rotate(45deg);
    opacity: 0.85;
    transition: transform 0.15s var(--ease);
  }
  .trig[aria-expanded='true'] .caret {
    transform: translateY(3px) rotate(-135deg);
  }
  .chip.flag {
    border-radius: 999px;
    background: var(--flag);
    color: var(--flag-on);
  }
  .chip.warn {
    border-radius: 4px;
    background: var(--warn);
    color: var(--warn-on);
  }
  .small .chip {
    padding: 2px 7px 2px 7px;
    letter-spacing: 0;
    border-radius: 7px;
    font-weight: 550;
    background: var(--accent-soft);
    color: var(--ink);
  }
  .small .chip.zooms {
    box-shadow: inset 3px 0 0 var(--accent);
    padding-left: 9px;
  }
  .small .chip.positive {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--ink-4);
  }
  .small .chip.faint {
    background: transparent;
    color: var(--ink-3);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .trig:hover {
    filter: brightness(1.06);
  }
  /* the full text in place */
  .more {
    margin-top: 4px;
    padding: 6px 10px 7px 11px;
    border-radius: 8px;
    font-size: var(--fs-sm);
    line-height: 1.45;
    color: var(--ink);
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .more[hidden] {
    display: none;
  }
  .more.flag {
    background: var(--flag-tint);
    box-shadow: inset 3px 0 0 var(--flag);
  }
  .more.warn {
    background: var(--warn-tint);
    box-shadow: inset 3px 0 0 var(--warn);
  }
  /* the tooltip: hover and keyboard focus (a native title shows on hover only); under the item, the card's width */
  .tip {
    display: none;
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    z-index: 20;
    padding: 7px 9px;
    border-radius: 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    box-shadow: var(--shadow);
    color: var(--ink-2);
    font-size: var(--fs-xs);
    font-weight: 400;
    line-height: 1.45;
    pointer-events: none;
    text-wrap: pretty;
  }
  .trig:hover + .tip,
  .trig:focus-visible + .tip {
    display: block;
  }
  .tip.off {
    display: none !important;
  }
</style>
