// ビルド済みサイト（npm run preview）のスクリーンショットを shots/ に保存する。
// 使い方: PW_CHROMIUM=/path/to/chrome npm run shots   （BASE_URL で URL を変更可）
import { chromium } from 'playwright-core'
import { mkdirSync } from 'node:fs'

const base = process.env.BASE_URL ?? 'http://localhost:4173'
mkdirSync('shots', { recursive: true })
const browser = await chromium.launch({ executablePath: process.env.PW_CHROMIUM, args: ['--no-sandbox'] })
for (const scheme of ['light', 'dark']) {
  for (const [name, w, h] of [['mobile', 390, 844], ['desktop', 1280, 900]]) {
    const ctx = await browser.newContext({ viewport: { width: w, height: h }, colorScheme: scheme })
    const page = await ctx.newPage()
    const errors = []
    page.on('pageerror', (e) => errors.push(e.message))
    page.on('console', (m) => m.type() === 'error' && errors.push(m.text()))
    for (const [path, label] of [['/', 'top'], ['/privacy', 'privacy'], ['/custom-keyboard', 'editor']]) {
      await page.goto(base + path, { waitUntil: 'networkidle' })
      if (label === 'editor') await page.locator('[data-key-id]').first().click()
      await page.screenshot({ path: `shots/${label}-${name}-${scheme}.png`, fullPage: true })
    }
    console.log(name, scheme, 'errors:', errors.length ? errors : 'none')
    await ctx.close()
  }
}
await browser.close()
