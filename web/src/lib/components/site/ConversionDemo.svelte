<!--
  トップの変換デモ。読みを 1 文字ずつ入力 → Space で文節に分かれて変換 → 注目文節の候補を出す → 確定、を繰り返す。
  実際のエンジンは動かさず、決め打ちの台本を再生するだけ。動きを減らす設定では変換後の状態を止めて表示する。
-->
<script lang="ts">
  import { onMount } from 'svelte'

  type Source = '学習' | 'LLM' | '辞書' | 'かな'
  type Script = {
    reading: string
    segments: string[]
    focus: number
    candidates: { text: string; source: Source }[]
  }

  const scripts: Script[] = [
    {
      reading: 'ゆうがたのうみはなぎだった',
      segments: ['夕方の', '海は', '凪だった'],
      focus: 2,
      candidates: [
        { text: '凪だった', source: 'LLM' },
        { text: '薙だった', source: '辞書' },
        { text: 'なぎだった', source: 'かな' }
      ]
    },
    {
      reading: 'かいぎはごごからです',
      segments: ['会議は', '午後からです'],
      focus: 0,
      candidates: [
        { text: '会議は', source: '学習' },
        { text: '懐疑は', source: '辞書' },
        { text: '回帰は', source: '辞書' }
      ]
    },
    {
      reading: 'きしゃのきしゃがきしゃした',
      segments: ['貴社の', '記者が', '帰社した'],
      focus: 1,
      candidates: [
        { text: '記者が', source: 'LLM' },
        { text: '汽車が', source: '辞書' },
        { text: '貴社が', source: '辞書' }
      ]
    }
  ]

  type Phase = 'typing' | 'converted' | 'committed'
  let index = $state(0)
  let typed = $state(0)
  let phase = $state<Phase>('converted')
  let key = $state<string | null>(null)

  const s = $derived(scripts[index])

  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let timer: ReturnType<typeof setTimeout>
    let alive = true
    const wait = (ms: number) => new Promise<void>((r) => (timer = setTimeout(r, ms)))
    const press = async (name: string) => {
      key = name
      await wait(380)
      key = null
    }
    ;(async () => {
      // 初期表示は 1 本目の変換後。少し見せてから動き出す
      await wait(2600)
      while (alive) {
        await press('Enter')
        phase = 'committed'
        await wait(1200)
        index = (index + 1) % scripts.length
        typed = 0
        phase = 'typing'
        for (let i = 1; i <= scripts[index].reading.length && alive; i++) {
          typed = i
          await wait(95 + Math.random() * 70)
        }
        await wait(500)
        await press('Space')
        phase = 'converted'
        await wait(3200)
      }
    })()
    return () => {
      alive = false
      clearTimeout(timer)
    }
  })
</script>

<figure class="demo">
  <div class="window" aria-hidden="true">
    <div class="bar">
      <span class="title">メモ</span>
      <span class="mode">あ</span>
    </div>
    <div class="body">
      <p class="line">
        {#if phase === 'typing'}
          <span class="pre live">{s.reading.slice(0, typed)}</span><span class="caret"></span>
        {:else if phase === 'converted'}
          {#each s.segments as seg, i (i)}
            <span class="pre seg" class:focus={i === s.focus}>{seg}</span>
          {/each}
        {:else}
          <span class="done">{s.segments.join('')}</span><span class="caret"></span>
        {/if}
      </p>
      <div class="candidates" class:show={phase === 'converted'}>
        {#each s.candidates as c, i (c.text)}
          <div class="cand" class:selected={i === 0}>
            <span class="n">{i + 1}</span>
            <span class="t">{c.text}</span>
            <span class="src">{c.source}</span>
          </div>
        {/each}
      </div>
    </div>
    <div class="keys">
      <span class="key" class:down={key === 'Space'}>Space</span>
      <span class="key" class:down={key === 'Enter'}>Enter</span>
    </div>
  </div>
  <figcaption>
    変換の様子（イメージ）。読みは文節ごとに区切られ、太い下線の文節が候補の対象です。候補の右には出どころ（学習・LLM・辞書など）が付きます。
  </figcaption>
</figure>

<style>
  .demo {
    margin: 0;
  }
  .window {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    box-shadow: 8px 8px 0 var(--sun);
    overflow: hidden;
    font-size: 1rem;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
    font-size: 0.75rem;
    color: var(--muted);
  }
  .title {
    flex: 1;
  }
  .mode {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    color: var(--fg);
    font-weight: 600;
  }
  .body {
    padding: 22px 20px 16px;
    min-height: 196px;
    box-sizing: border-box;
  }
  .line {
    margin: 0;
    min-height: 2.2em;
    font-size: 1.375rem;
    line-height: 1.6;
    font-family: var(--font-serif);
    letter-spacing: 0.02em;
  }
  .pre {
    text-decoration-line: underline;
    text-decoration-color: var(--fg);
    text-underline-offset: 0.3em;
    text-decoration-thickness: 1px;
  }
  .live {
    text-decoration-style: dotted;
  }
  .seg + .seg {
    margin-left: 0.18em;
  }
  .seg.focus {
    text-decoration-thickness: 3px;
    background: var(--sun-soft);
  }
  .caret {
    display: inline-block;
    width: 2px;
    height: 1.1em;
    margin-left: 1px;
    vertical-align: -0.15em;
    background: var(--accent);
    animation: blink 1.1s steps(1) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  .candidates {
    margin-top: 14px;
    width: min(100%, 260px);
    border: 1px solid var(--line);
    border-radius: 4px;
    background: var(--bg);
    opacity: 0;
    transform: translateY(-4px);
    transition:
      opacity 0.18s,
      transform 0.18s;
  }
  .candidates.show {
    opacity: 1;
    transform: none;
  }
  .cand {
    display: grid;
    grid-template-columns: 1.2em 1fr auto;
    align-items: baseline;
    gap: 10px;
    padding: 5px 10px;
    font-size: 0.9375rem;
  }
  .cand + .cand {
    border-top: 1px solid var(--line);
  }
  .cand.selected {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .n {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    opacity: 0.7;
  }
  .src {
    font-size: 0.6875rem;
    letter-spacing: 0.04em;
    opacity: 0.75;
  }
  .keys {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    padding: 10px 12px;
    border-top: 1px solid var(--line);
  }
  .key {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--muted);
    padding: 3px 10px;
    border: 1px solid var(--line);
    border-bottom-width: 3px;
    border-radius: 4px;
    transition:
      transform 0.08s,
      border-color 0.08s;
  }
  .key.down {
    color: var(--fg);
    border-color: var(--line-strong);
    border-bottom-width: 1px;
    transform: translateY(2px);
  }
  figcaption {
    margin-top: 18px;
    font-size: 0.8125rem;
    color: var(--muted);
    line-height: 1.7;
  }
</style>
