<script lang="ts">
  import { describePath, errorsOf, warningsOf } from '$lib/keyboard/validate'
  import type { ValidationError } from '$lib/keyboard/validate'

  interface Props {
    /** error と warning が混ざっていてよい（severity で分けて表示する。省略は error）。 */
    errors: ValidationError[]
    title?: string
    warningTitle?: string
    /** パスの示す場所を開く（キーの選択、詳細設定の該当タブなど）。 */
    onjump?: (path: string) => void
    /** onjump で開ける場所か。省略時はキーを指すパスだけ。 */
    canjump?: (path: string) => boolean
  }
  let { errors, title = '検証エラー', warningTitle = '警告（無視される値）', onjump, canjump = (p) => /^rows\[\d+\]\.keys\[\d+\]/.test(p) }: Props = $props()

  const errs = $derived(errorsOf(errors))
  const warns = $derived(warningsOf(errors))
</script>

{#snippet list(items: ValidationError[])}
  <ul>
    {#each items.slice(0, 50) as e, i (i)}
      <li>
        <span class="where">{describePath(e.path)}</span>
        <span>{e.message}</span>
        {#if onjump && canjump(e.path)}
          <button type="button" class="btn small" onclick={() => onjump(e.path)}>その場所を開く</button>
        {/if}
      </li>
    {/each}
  </ul>
  {#if items.length > 50}<p>ほか {items.length - 50} 件</p>{/if}
{/snippet}

{#if errs.length}
  <div class="errors" role="alert">
    <p class="ttl">{title}（{errs.length} 件）</p>
    {@render list(errs)}
  </div>
{/if}
{#if warns.length}
  <div class="warnings" role="status">
    <p class="ttl">{warningTitle}（{warns.length} 件）</p>
    <p class="note">警告があっても書き出せます。アプリは該当するプロパティだけを無視し、下位の値で表示します。</p>
    {@render list(warns)}
  </div>
{/if}

<style>
  .errors,
  .warnings {
    border: 1px solid var(--danger);
    background: var(--danger-soft);
    border-radius: 10px;
    padding: 8px 12px;
    font-size: 0.875rem;
  }
  .warnings {
    margin-top: 8px;
    border-color: #b7791f;
    background: color-mix(in srgb, #f6c453 22%, var(--surface));
  }
  .ttl {
    margin: 0 0 4px;
    font-weight: 700;
    color: var(--danger);
  }
  .warnings .ttl {
    color: var(--fg);
  }
  .note {
    margin: 0 0 4px;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  ul {
    margin: 0;
    padding-left: 1.2em;
  }
  li {
    margin: 4px 0;
  }
  .where {
    font-weight: 600;
    margin-right: 6px;
  }
  .where::after {
    content: ':';
  }
</style>
