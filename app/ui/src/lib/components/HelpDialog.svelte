<script lang="ts">
  // Help and About (toolbar ? button, F1, the macOS menu): what the app is, how to cite it, and the user guide.
  // The guide is the repository's USER_GUIDE.md, bundled at build time, so the app and the file never disagree.
  import guide from '../../../../../USER_GUIDE.md?raw';
  import { renderMarkdown } from '../markdown';
  import Icon from './Icon.svelte';

  interface Props {
    open: boolean;
    version: string | null;
    onClose: () => void;
  }
  let { open, version, onClose }: Props = $props();

  const html = renderMarkdown(guide.replace(/^﻿?# [^\r\n]*(\r?\n)+/, '')); // the dialog's own header names it
  let dlg: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) dlg.showModal();
    else if (!open && dlg.open) dlg.close();
  });
</script>

<!-- Esc: the state closes the dialog (oncancel), so a quick Esc then F1 can never find the state still "open" while
     the native close event is pending. onclose stays as a backstop. -->
<dialog bind:this={dlg} class="help" aria-label="Help and About" data-testid="help-dialog"
  oncancel={(e) => {
    e.preventDefault();
    onClose();
  }}
  onclose={() => {
    // the close event arrives a task later; if Help was reopened meanwhile (Esc then F1), it must not shut it again
    if (open && !dlg?.open) onClose();
  }}>
  <header>
    <div>
      <h2>SPYDER Bone{version ? ` ${version}` : ''}</h2>
      <p class="muted">Scores near-infrared scans of bone, one scan at a time, to help decide where destructive sampling
        (radiocarbon, ZooMS, isotopes) is worth it.</p>
    </div>
    <button class="iconbtn" onclick={onClose} aria-label="Close help" title="Close (Esc)"><Icon name="close" size={13} /></button>
  </header>
  <section class="about">
    <p><b>Cite</b> the calibration data behind the collagen models: Ryder, C. et al. 2026. Refining near-infrared spectroscopy
      for collagen quantification in archaeological bone. <i>Journal of Archaeological Science</i> 185:106448.
      doi:10.1016/j.jas.2025.106448</p>
    <p><b>Licences:</b> application code MIT; bundled model, transfer, reference and parameter files CC-BY-4.0.</p>
  </section>
  <article class="guide">
    {@html html}
  </article>
</dialog>

<style>
  .help {
    width: min(860px, calc(100vw - 48px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--panel);
    color: var(--ink);
    box-shadow: var(--shadow);
    overflow: auto;
  }
  .help::backdrop {
    background: rgba(0, 0, 0, 0.45);
  }
  header {
    position: sticky;
    top: 0;
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    padding: 16px 22px 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }
  header h2 {
    margin: 0 0 4px;
    font-size: 17px;
  }
  header p {
    margin: 0;
  }
  .about {
    padding: 10px 22px;
    border-bottom: 1px solid var(--line-soft);
    font-size: 13px;
    color: var(--ink-2);
  }
  .about p {
    margin: 6px 0;
  }
  .guide {
    padding: 4px 22px 24px;
    font-size: 13.5px;
    line-height: 1.55;
  }
  .guide :global(h2) {
    font-size: 15.5px;
    margin: 22px 0 8px;
  }
  .guide :global(h3) {
    font-size: 14px;
    margin: 16px 0 6px;
    color: var(--ink-2);
  }
  .guide :global(hr) {
    display: none;
  }
  .guide :global(table) {
    border-collapse: collapse;
    margin: 8px 0;
  }
  .guide :global(th),
  .guide :global(td) {
    border: 1px solid var(--line);
    padding: 5px 8px;
    text-align: left;
    vertical-align: top;
  }
  .guide :global(code),
  .guide :global(pre) {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 12.5px;
  }
  .guide :global(pre) {
    background: var(--panel-2);
    border: 1px solid var(--line-soft);
    border-radius: 8px;
    padding: 10px 12px;
    overflow: auto;
  }
  .guide :global(blockquote) {
    margin: 8px 0;
    padding-left: 12px;
    border-left: 3px solid var(--line);
    color: var(--ink-2);
  }
  .guide :global(ul) {
    padding-left: 20px;
  }
</style>
