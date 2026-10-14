<script lang="ts">
  import type { Key, KeyboardLayout } from '$lib/keyboard/types'

  interface Props {
    layout: KeyboardLayout
    selectedKeyId?: string | null
    onselect?: (id: string) => void
  }
  let { layout, selectedKeyId = null, onselect }: Props = $props()

  const KIND_NAMES: Record<Key['kind'], string> = {
    text: '文字',
    flick: 'フリック',
    toggle: 'トグル',
    action: '操作',
    shortcut: 'ショートカット',
    layout: 'レイアウト切替',
    spacer: '空白'
  }

  function mainLabel(k: Key): string {
    if (k.label) return k.label
    switch (k.kind) {
      case 'text':
        return k.text
      case 'flick':
        return k.center.text
      case 'toggle':
        return k.cycle[0] ?? ''
      case 'action':
        return k.action
      case 'shortcut':
        return 'SC'
      case 'layout':
        return k.target
      default:
        return ''
    }
  }

  function describe(k: Key): string {
    const base = `${mainLabel(k) || k.id}（${KIND_NAMES[k.kind]}）`
    if (k.kind === 'flick') {
      const dirs = (['up', 'down', 'left', 'right'] as const)
        .filter((d) => k[d])
        .map((d) => `${{ up: '上', down: '下', left: '左', right: '右' }[d]} ${k[d]?.text}`)
      return dirs.length ? `${base} ${dirs.join('、')}` : base
    }
    return base
  }
</script>

<div class="phone" role="group" aria-label="キーボードのプレビュー（キーを押すと選択）">
  <div class="candbar" aria-hidden="true">
    <span>候補バー</span><span class="dim">きょう　今日　京</span>
  </div>
  <div class="kb">
    {#each layout.rows as row, ri (ri)}
      <div class="row" style:height="{52 * (row.height ?? 1)}px">
        {#each row.keys as key, ki (ki)}
          {#if key.kind === 'spacer'}
            <button
              type="button"
              class="key spacer"
              class:selected={key.id === selectedKeyId}
              style:flex-grow={key.width ?? 1}
              aria-label={`空白（幅 ${key.width ?? 1}）`}
              aria-pressed={key.id === selectedKeyId}
              onclick={() => onselect?.(key.id)}
            ></button>
          {:else}
            <button
              type="button"
              class="key {key.style ?? 'normal'}"
              class:selected={key.id === selectedKeyId}
              style:flex-grow={key.width ?? 1}
              aria-label={describe(key)}
              aria-pressed={key.id === selectedKeyId}
              onclick={() => onselect?.(key.id)}
            >
              {#if key.kind === 'flick'}
                <span class="fl fl-u" aria-hidden="true">{key.up?.text ?? ''}</span>
                <span class="fl fl-d" aria-hidden="true">{key.down?.text ?? ''}</span>
                <span class="fl fl-l" aria-hidden="true">{key.left?.text ?? ''}</span>
                <span class="fl fl-r" aria-hidden="true">{key.right?.text ?? ''}</span>
                <span class="main" aria-hidden="true">{key.label ?? key.center.text}</span>
              {:else}
                <span class="main" class:small={mainLabel(key).length > 3} aria-hidden="true">{mainLabel(key)}</span>
              {/if}
              {#if key.longPress?.length}<span class="lp" aria-hidden="true"></span>{/if}
            </button>
          {/if}
        {/each}
      </div>
    {/each}
  </div>
</div>

<style>
  .phone {
    width: 100%;
    max-width: 430px;
    margin: 0 auto;
    border-radius: 16px;
    overflow: hidden;
    border: 1px solid var(--line);
    background: var(--kb-bg);
    container-type: inline-size;
  }
  .candbar {
    display: flex;
    gap: 12px;
    align-items: center;
    height: 40px;
    padding: 0 12px;
    font-size: 0.8125rem;
    color: var(--muted);
    border-bottom: 1px solid var(--line);
  }
  .dim {
    color: var(--fg);
  }
  .kb {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 4px 10px;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .key {
    position: relative;
    flex-basis: 0;
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: var(--key-bg);
    color: var(--fg);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.3);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1.125rem;
    overflow: hidden;
  }
  .key.function {
    background: var(--key-fn-bg);
  }
  .key.accent {
    background: var(--key-accent-bg);
    color: var(--key-accent-fg);
  }
  .key.spacer {
    background: transparent;
    box-shadow: none;
    outline: 1px dashed var(--muted);
    outline-offset: -1px;
  }
  .key.selected {
    outline: 3px solid var(--focus);
    outline-offset: -1px;
    z-index: 1;
  }
  .key:focus-visible {
    outline: 3px solid var(--focus);
    outline-offset: 1px;
    z-index: 2;
  }
  .main {
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: clip;
  }
  .main.small {
    font-size: 0.8125rem;
  }
  .fl {
    position: absolute;
    font-size: 0.625rem;
    color: var(--muted);
    line-height: 1;
  }
  .key.accent .fl {
    color: inherit;
    opacity: 0.8;
  }
  .fl-u {
    top: 3px;
    left: 50%;
    transform: translateX(-50%);
  }
  .fl-d {
    bottom: 3px;
    left: 50%;
    transform: translateX(-50%);
  }
  .fl-l {
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
  }
  .fl-r {
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
  }
  .lp {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.55;
  }
</style>
