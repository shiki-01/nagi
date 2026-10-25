// 検証メッセージのパス（`rows[0].keys[1].states.pressed.opacity` など）から、エディタで開く場所を決める（純粋関数）。

import { LOOK_STATES, TOOLBAR_ROW } from './types'
import type { KeyboardLayout, StateName } from './types'

export interface JumpTarget {
  /** 開くインスペクタのタブ。 */
  tab: 'basic' | 'look'
  scope: 'layout' | 'row' | 'key'
  keyId?: string
  row?: number
  state: 'normal' | StateName
}

const isState = (v: unknown): v is StateName => (LOOK_STATES as readonly unknown[]).includes(v)

/** パスの示す場所。エディタに対応する場所が無ければ undefined。 */
export function jumpTarget(layout: KeyboardLayout, path: string): JumpTarget | undefined {
  let m = /^toolbar\.keys\[(\d+)\](.*)$/.exec(path)
  if (m) {
    const key = layout.toolbar?.keys[Number(m[1])]
    if (!key) return undefined
    const states = /^\.states\.([A-Za-z]+)/.exec(m[2])
    if (states && isState(states[1])) return { tab: 'look', scope: 'key', keyId: key.id, state: states[1] }
    if (/^\.(look|states)(\.|$)/.test(m[2])) return { tab: 'look', scope: 'key', keyId: key.id, state: 'normal' }
    return { tab: 'basic', scope: 'key', keyId: key.id, state: 'normal' }
  }
  if (/^toolbar\.look(\.|$)/.test(path)) return layout.toolbar ? { tab: 'look', scope: 'row', row: TOOLBAR_ROW, state: 'normal' } : undefined
  m = /^rows\[(\d+)\]\.keys\[(\d+)\](.*)$/.exec(path)
  if (m) {
    const key = layout.rows[Number(m[1])]?.keys[Number(m[2])]
    if (!key) return undefined
    const states = /^\.states\.([A-Za-z]+)/.exec(m[3])
    if (states && isState(states[1])) return { tab: 'look', scope: 'key', keyId: key.id, state: states[1] }
    if (/^\.(look|states)(\.|$)/.test(m[3])) return { tab: 'look', scope: 'key', keyId: key.id, state: 'normal' }
    return { tab: 'basic', scope: 'key', keyId: key.id, state: 'normal' }
  }
  m = /^rows\[(\d+)\]\.look(\.|$)/.exec(path)
  if (m) return layout.rows[Number(m[1])] ? { tab: 'look', scope: 'row', row: Number(m[1]), state: 'normal' } : undefined
  m = /^rules\[(\d+)\]/.exec(path)
  if (m) {
    const when: unknown = (layout.rules?.[Number(m[1])] as { when?: unknown } | undefined)?.when
    return { tab: 'look', scope: 'layout', state: isState(when) ? when : 'normal' }
  }
  if (/^(look|roles|metrics|background|assets)(\.|\[|$)/.test(path)) return { tab: 'look', scope: 'layout', state: 'normal' }
  return undefined
}
