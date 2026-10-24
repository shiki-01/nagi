// テスト用: リポジトリ内のテキストファイルを読む（Master CSS の Vite プラグインが app.css の `?raw` 読み込みを処理しようとして
// 失敗するため、Node で直接読む）。Node の型定義（@types/node）は入れていないため、型検査は対象外にする。
// @ts-nocheck

import { readFileSync } from 'node:fs'

/** @param {string} url `new URL(相対パス, import.meta.url).href` */
export function readText(url) {
  return readFileSync(new URL(url), 'utf8')
}
