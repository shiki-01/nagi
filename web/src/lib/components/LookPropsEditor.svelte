<script lang="ts">
  // look のプロパティ一覧（グループごとに折りたたみ）。書き込み先 locs と、継承される値の計算に使う ctx を受け取る。
  import type { InheritContext, LookLoc } from '$lib/keyboard/look-ops'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import LookField from './LookField.svelte'

  interface Props {
    store: EditorStore
    locs: readonly LookLoc[]
    ctx?: InheritContext
    /** label / icon（states / rules の中だけで有効）を出すか。 */
    allowContent?: boolean
    /** 指定したプロパティだけを出す（background 用）。 */
    only?: readonly string[]
  }
  let { store, locs, ctx, allowContent = false, only }: Props = $props()

  const WEIGHTS: Record<string, string> = {
    ultraLight: 'ultraLight（極細）',
    thin: 'thin（細）',
    light: 'light（やや細）',
    regular: 'regular（標準）',
    medium: 'medium（やや太）',
    semibold: 'semibold（中太）',
    bold: 'bold（太）',
    heavy: 'heavy（極太）',
    black: 'black（最太）'
  }
  const DESIGNS: Record<string, string> = {
    system: 'system（標準）',
    rounded: 'rounded（丸み）',
    monospaced: 'monospaced（等幅）',
    serif: 'serif（セリフ）'
  }
  const FITS: Record<string, string> = {
    fill: 'fill（全体を覆い、はみ出しを切る）',
    fit: 'fit（全体を収める）',
    tile: 'tile（元の大きさで敷き詰める）'
  }

  interface Item {
    prop: string
    label: string
    hint?: string
    options?: Record<string, string>
  }
  const GROUPS: { title: string; items: Item[] }[] = [
    {
      title: '文字・アイコン',
      items: [
        { prop: 'label', label: '表示する文字 (label)', hint: 'この状態の間だけ表示を差し替えます。label だけ指定すると、その間はキーのアイコンを出しません' },
        { prop: 'icon', label: '表示するアイコン (icon)', hint: 'sf: / iconify: / 接頭辞なし。iconify: は取り込み時にアプリが取得して保存します' },
        { prop: 'fontSize', label: '文字の大きさ' },
        { prop: 'fontWeight', label: '文字の太さ', options: WEIGHTS },
        {
          prop: 'fontDesign',
          label: '書体の系統',
          options: DESIGNS,
          hint: '日本語はいずれもシステムの日本語フォントになり、rounded で丸ゴシック系になるとは限りません'
        },
        { prop: 'color', label: '文字・アイコンの色' }
      ]
    },
    {
      title: '背景',
      items: [
        { prop: 'fill', label: '背景色' },
        { prop: 'fillOpacity', label: '背景の不透明度', hint: '背景色と画像だけに掛かります（文字・枠線・影には掛かりません）。押下中は組み込みで 0.6 です' },
        { prop: 'fillImage', label: '背景画像' },
        { prop: 'fillImageFit', label: '画像の当て方', options: FITS },
        { prop: 'fillImageOpacity', label: '画像の不透明度' }
      ]
    },
    {
      title: '形・枠線',
      items: [
        { prop: 'cornerRadius', label: '角丸' },
        { prop: 'borderWidth', label: '枠線の太さ' },
        { prop: 'borderColor', label: '枠線の色' }
      ]
    },
    {
      title: '影',
      items: [
        { prop: 'shadowColor', label: '影の色' },
        { prop: 'shadowRadius', label: '影のぼかし', hint: '0 のとき影は描きません' },
        { prop: 'shadowX', label: '影の横オフセット' },
        { prop: 'shadowY', label: '影の縦オフセット' },
        { prop: 'shadowOpacity', label: '影の不透明度' }
      ]
    },
    {
      title: '表示の変形',
      items: [
        { prop: 'opacity', label: 'キー全体の不透明度' },
        { prop: 'scale', label: '拡大率', hint: '当たり判定は変わりません（見た目だけ）' },
        { prop: 'offsetX', label: '表示位置の横ずらし', hint: '当たり判定は変わりません' },
        { prop: 'offsetY', label: '表示位置の縦ずらし', hint: '当たり判定は変わりません' },
        { prop: 'inset', label: '描画範囲を縮める', hint: 'キー同士の見かけの間隔を広げます。当たり判定はそのままです' },
        { prop: 'padding', label: '文字の内側余白' }
      ]
    },
    {
      title: '動き',
      items: [{ prop: 'animationMs', label: '切り替えの長さ', hint: 'この指定へ切り替わる（または戻る）ときの補間。前後の大きいほうが使われます' }]
    }
  ]

  const groups = $derived(
    GROUPS.map((g) => ({
      title: g.title,
      items: g.items.filter((i) => (only ? only.includes(i.prop) : allowContent || (i.prop !== 'label' && i.prop !== 'icon')))
    })).filter((g) => g.items.length > 0)
  )
</script>

{#each groups as g, gi (g.title)}
  <details class="grp" open={gi < 2 || !!only}>
    <summary>{g.title}</summary>
    {#each g.items as item (item.prop)}
      <LookField {store} prop={item.prop} label={item.label} hint={item.hint} {locs} {ctx} optionLabels={item.options} />
    {/each}
  </details>
{/each}

<style>
  .grp {
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 2px 10px 6px;
    margin-bottom: 8px;
  }
  summary {
    cursor: pointer;
    padding: 6px 0;
    font-weight: 600;
    font-size: 0.875rem;
  }
</style>
