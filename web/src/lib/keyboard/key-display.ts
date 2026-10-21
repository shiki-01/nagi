// キーの表示用ラベルと読み上げ用の説明（KeyboardPreview / パレットで共有）。

import type { Key, KeyKind } from './types'

export const KIND_NAMES: Record<KeyKind, string> = {
  text: '文字',
  flick: 'フリック',
  toggle: 'トグル',
  action: '操作',
  shortcut: 'ショートカット',
  layout: 'レイアウト切替',
  spacer: '空白'
}

export function mainLabel(k: Key): string {
  if (k.label) return k.label
  switch (k.kind) {
    case 'text':
      return k.text
    case 'flick':
      return k.center?.text ?? k.kana ?? ''
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

export function describeKey(k: Key): string {
  if (k.kind === 'spacer') return `空白（幅 ${k.width ?? 1}）`
  const base = `${mainLabel(k) || k.id}（${KIND_NAMES[k.kind]}）`
  if (k.kind === 'flick') {
    const names = { up: '上', down: '下', left: '左', right: '右' }
    const dirs = (['up', 'down', 'left', 'right'] as const).filter((d) => k[d]).map((d) => `${names[d]} ${k[d]?.text}`)
    return dirs.length ? `${base} ${dirs.join('、')}` : base
  }
  return base
}
