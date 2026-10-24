<script lang="ts">
  // アイコン名の入力（sf:SF Symbols 名 / iconify:prefix:name / 接頭辞なし）。書式は validateIcon と同じ（look-resolve の iconFormatError）。
  // iconify: はブラウザから Iconify API を直接読んでプレビューする（書き出す JSON には ID のみ）。KeyForm の icon 欄と同じ扱い。
  import { iconFormatError } from '$lib/keyboard/look-resolve'
  import { ICONIFY_ICON_PATTERN, LOOK_LIMITS } from '$lib/keyboard/types'

  interface Props {
    value: string | undefined
    label: string
    disabled?: boolean
    onchange: (value: string) => void
  }
  let { value, label, disabled = false, onchange }: Props = $props()

  const error = $derived(value ? iconFormatError(value) : undefined)
  const iconify = $derived.by(() => {
    const m = value ? ICONIFY_ICON_PATTERN.exec(value) : null
    return m ? { url: `https://api.iconify.design/${m[1]}/${m[2]}.svg` } : null
  })
  let failed = $state<string | null>(null)
  let bad = $state<string | null>(null)
</script>

<input
  aria-label={label}
  {disabled}
  value={value ?? ''}
  maxlength={LOOK_LIMITS.iconMax}
  spellcheck="false"
  placeholder="iconify:mdi:home"
  aria-invalid={bad || error ? 'true' : undefined}
  oninput={(e) => {
    const v = e.currentTarget.value
    const err = v === '' ? '1 文字以上で入力してください' : iconFormatError(v)
    bad = err ?? null
    if (!err) onchange(v)
  }}
  onblur={(e) => {
    bad = null
    e.currentTarget.value = value ?? ''
  }}
/>
{#if bad ?? error}<p class="err" role="alert">{bad ?? error}</p>{/if}
{#if iconify}
  <p class="prev">
    {#if failed === iconify.url}
      取得できません。アプリでも取得に失敗した場合は label が表示されます
    {:else}
      {#key iconify.url}<img src={iconify.url} alt={value} height="24" onerror={() => (failed = iconify.url)} />{/key}
      プレビュー（Iconify）。JSON には ID のみ入ります
    {/if}
  </p>
{/if}

<style>
  input {
    width: 100%;
    min-height: 28px;
    box-sizing: border-box;
  }
  .err {
    margin: 2px 0 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  .prev {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  input[aria-invalid='true'] {
    border-color: var(--danger);
  }
</style>
