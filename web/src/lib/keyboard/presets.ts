// config/keyboard/presets/*.json を取り込む。別担当が並行して作成中のため、
// 無い間は sampleLayout() 1 つにフォールバックする。不正なプリセットは捨てて理由を返す。

import { sampleLayout } from './ops'
import { describePath, errorsOf, validateLayout } from './validate'
import type { KeyboardLayout } from './types'

export interface PresetLoadResult {
  presets: KeyboardLayout[]
  /** 読めなかったプリセットの説明。 */
  problems: string[]
  /** true なら presets はフォールバックのサンプル。 */
  fallback: boolean
}

export function loadPresets(modules: Record<string, unknown>): PresetLoadResult {
  const entries = Object.entries(modules).sort(([a], [b]) => a.localeCompare(b))
  const raw = entries.map(([path, mod]) => {
    const data = mod && typeof mod === 'object' && 'default' in mod ? (mod as { default: unknown }).default : mod
    return { path, data }
  })
  const ids = raw.flatMap((r) => (r.data && typeof (r.data as { id?: unknown }).id === 'string' ? [(r.data as { id: string }).id] : []))
  const presets: KeyboardLayout[] = []
  const problems: string[] = []
  for (const { path, data } of raw) {
    const name = path.split('/').pop() ?? path
    const errors = errorsOf(validateLayout(data, { knownLayoutIds: ids }))
    if (errors.length) {
      const e = errors[0]
      problems.push(`${name}: ${describePath(e.path)}: ${e.message}${errors.length > 1 ? `（ほか ${errors.length - 1} 件）` : ''}`)
    } else presets.push(data as KeyboardLayout)
  }
  if (presets.length === 0) return { presets: [sampleLayout()], problems, fallback: true }
  return { presets, problems, fallback: false }
}

// リポジトリ直下の config/keyboard/presets/*.json（web/ から見て ../config）。ファイルが無ければ空のオブジェクト。
const modules = import.meta.glob('../../../../config/keyboard/presets/*.json', { eager: true })

export const presetResult: PresetLoadResult = loadPresets(modules)
