// キーボードレイアウト JSON を schema 1 から 2 へ移行する（一回限り。docs/keyboard-style-spec.md §12）。
//
//   node scripts/migrate-layout-v2.mjs <ファイルまたはディレクトリ>...
//
// ディレクトリは直下の *.json を対象にする。`schema: 1` のものだけを書き換え、`schema: 2` などは触らない（冪等）。
// 変更点: `schema` を 2 に、キー（と longPress の項目）の `style` を `role` に改名する。
// 手書きの整形（1 行にまとめたキーなど）を保つため、JSON を作り直さず該当の文字列だけを置換する。
// 置換後に、構造としての期待値と一致するか確かめ、食い違えば書き換えない。

// Node の型定義（@types/node）は入れていないため、svelte-check（checkJs）の型検査は対象外にする
// @ts-nocheck

import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** @param {unknown} v @returns {v is Record<string, unknown>} */
const isObj = (v) => typeof v === 'object' && v !== null && !Array.isArray(v)

/** @param {Record<string, unknown>} key */
function renameKey(key) {
  /** @type {Record<string, unknown>} */
  const out = {}
  for (const [k, v] of Object.entries(key)) {
    if (k === 'style') out.role = v
    else if (k === 'longPress' && Array.isArray(v)) out[k] = v.map((p) => (isObj(p) ? renameKey(p) : p))
    else out[k] = v
  }
  return out
}

/** 構造としての移行結果（検算用）。 @param {Record<string, unknown>} layout */
function migrateValue(layout) {
  const rows = Array.isArray(layout.rows)
    ? layout.rows.map((/** @type {unknown} */ row) => {
        if (!isObj(row) || !Array.isArray(row.keys)) return row
        return { ...row, keys: row.keys.map((/** @type {unknown} */ k) => (isObj(k) ? renameKey(k) : k)) }
      })
    : layout.rows
  return { ...layout, schema: 2, rows }
}

/**
 * レイアウト JSON の文字列を移行する。
 * @param {string} text
 * @returns {{ status: 'migrated' | 'skipped' | 'error', text: string, reason?: string }}
 */
export function migrateText(text) {
  /** @type {unknown} */
  let data
  try {
    data = JSON.parse(text)
  } catch (e) {
    return { status: 'error', text, reason: `JSON として読めません: ${/** @type {Error} */ (e).message}` }
  }
  if (!isObj(data) || data.schema !== 1) return { status: 'skipped', text, reason: 'schema が 1 ではない' }
  const out = text
    .replace(/("schema"\s*:\s*)1\b/, '$12')
    .replace(/"style"(\s*:\s*"(?:normal|function|accent)")/g, '"role"$1')
  /** @type {unknown} */
  let parsed
  try {
    parsed = JSON.parse(out)
  } catch {
    return { status: 'error', text, reason: '置換後の JSON が壊れました' }
  }
  if (JSON.stringify(parsed) !== JSON.stringify(migrateValue(data))) {
    return { status: 'error', text, reason: '想定外の形のため自動で移行できません（style の値が normal / function / accent 以外など）' }
  }
  return { status: 'migrated', text: out }
}

/** @param {string} path @returns {string[]} */
function expand(path) {
  if (!statSync(path).isDirectory()) return [path]
  return readdirSync(path)
    .filter((n) => n.endsWith('.json'))
    .sort()
    .map((n) => join(path, n))
}

function main() {
  const args = process.argv.slice(2)
  if (args.length === 0) {
    console.error('使い方: node scripts/migrate-layout-v2.mjs <ファイルまたはディレクトリ>...')
    process.exit(2)
  }
  let failed = false
  for (const file of args.flatMap(expand)) {
    const result = migrateText(readFileSync(file, 'utf8'))
    if (result.status === 'migrated') {
      writeFileSync(file, result.text)
      console.log(`移行: ${file}`)
    } else if (result.status === 'error') {
      failed = true
      console.error(`失敗: ${file}: ${result.reason}`)
    } else {
      console.log(`そのまま: ${file}（${result.reason}）`)
    }
  }
  if (failed) process.exit(1)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
