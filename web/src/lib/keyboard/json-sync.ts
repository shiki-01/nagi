// JSON を直接編集したときの取り込み（純粋関数）。構造が読める（描画できる）形なら反映し、スキーマ違反は
// 通常どおり store.errors / store.warnings に出る（途中の編集を拒まない）。構文エラーなどは反映せず理由を返す。

import { looksLikeLayout } from './store.svelte'
import type { KeyboardLayout } from './types'

export type DraftResult = { ok: true; layout: KeyboardLayout } | { ok: false; error: string }

export function parseDraft(text: string): DraftResult {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch (e) {
    return { ok: false, error: `JSON の構文エラー: ${(e as Error).message}` }
  }
  if (!looksLikeLayout(data)) return { ok: false, error: 'レイアウトとして読めません（id・name・rows と、各キーの id・kind が必要です）' }
  return { ok: true, layout: data }
}
