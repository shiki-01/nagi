<script lang="ts">
  // JSON などのコード例。右上のボタンでコピーできる（クリップボードが使えない環境ではボタンを出さない）。
  let { code, caption }: { code: string; caption?: string } = $props()

  let copied = $state(false)
  let canCopy = $state(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  $effect(() => {
    canCopy = typeof navigator !== 'undefined' && !!navigator.clipboard
    return () => clearTimeout(timer)
  })

  async function copy() {
    try {
      await navigator.clipboard.writeText(code)
      copied = true
      clearTimeout(timer)
      timer = setTimeout(() => (copied = false), 1800)
    } catch {
      copied = false
    }
  }
</script>

<figure class="example">
  {#if caption}<figcaption>{caption}</figcaption>{/if}
  <div class="box">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre tabindex="0"><code>{code}</code></pre>
    {#if canCopy}
      <button type="button" onclick={copy} aria-label="この例をコピー">{copied ? 'コピーしました' : 'コピー'}</button>
    {/if}
  </div>
  <span class="sr" role="status">{copied ? 'コピーしました' : ''}</span>
</figure>

<style>
  .example {
    margin: 14px 0 22px;
  }
  figcaption {
    color: var(--muted);
    font-size: 0.8125rem;
    margin-bottom: 4px;
  }
  .box {
    position: relative;
  }
  pre {
    margin: 0;
    padding: 14px 16px;
    padding-right: 88px;
    overflow-x: auto;
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    line-height: 1.6;
    tab-size: 2;
  }
  pre code {
    background: none;
    padding: 0;
    font-size: inherit;
    white-space: pre;
  }
  button {
    position: absolute;
    top: 8px;
    right: 8px;
    padding: 4px 10px;
    font: inherit;
    font-size: 0.75rem;
    color: var(--fg);
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    cursor: pointer;
  }
  button:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
