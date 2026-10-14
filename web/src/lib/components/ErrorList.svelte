<script lang="ts">
  import { describePath } from '$lib/keyboard/validate'
  import type { ValidationError } from '$lib/keyboard/validate'

  interface Props {
    errors: ValidationError[]
    title?: string
    /** パスがキーを指しているとき、そのキーを選ぶ。 */
    onjump?: (path: string) => void
  }
  let { errors, title = '検証エラー', onjump }: Props = $props()
  const jumpable = (p: string) => /^rows\[\d+\]\.keys\[\d+\]/.test(p)
</script>

{#if errors.length}
  <div class="errors" role="alert">
    <p class="ttl">{title}（{errors.length} 件）</p>
    <ul>
      {#each errors.slice(0, 50) as e, i (i)}
        <li>
          <span class="where">{describePath(e.path)}</span>
          <span>{e.message}</span>
          {#if onjump && jumpable(e.path)}
            <button type="button" class="btn small" onclick={() => onjump(e.path)}>そのキーを選択</button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if errors.length > 50}<p>ほか {errors.length - 50} 件</p>{/if}
  </div>
{/if}

<style>
  .errors {
    border: 1px solid var(--danger);
    background: var(--danger-soft);
    border-radius: 10px;
    padding: 8px 12px;
    font-size: 0.875rem;
  }
  .ttl {
    margin: 0 0 4px;
    font-weight: 700;
    color: var(--danger);
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
