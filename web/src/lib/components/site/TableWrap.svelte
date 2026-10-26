<script lang="ts">
  import type { Snippet } from 'svelte'
  // 横にあふれる表を、キーボードでもスクロールできるようにフォーカス可能にして包む。
  let { label, children }: { label: string; children: Snippet } = $props()
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="table-wrap" role="region" aria-label={label} tabindex="0">
  {@render children()}
</div>

<style>
  .table-wrap {
    overflow-x: auto;
    margin: 12px 0 16px;
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .table-wrap:focus-visible {
    outline: 3px solid var(--focus);
  }
  .table-wrap :global(table) {
    border-collapse: collapse;
    width: 100%;
    min-width: 640px;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .table-wrap :global(th),
  .table-wrap :global(td) {
    text-align: left;
    vertical-align: top;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
    overflow-wrap: break-word;
  }
  .table-wrap :global(tbody tr:last-child > *) {
    border-bottom: none;
  }
  .table-wrap :global(thead th) {
    background: var(--surface-2);
    font-weight: 600;
    white-space: nowrap;
  }
  .table-wrap :global(tbody th) {
    font-weight: 400;
    white-space: nowrap;
  }
</style>
