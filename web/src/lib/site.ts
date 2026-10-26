/** サイト共通の定数。sitemap・OGP・canonical で使う。 */
export const SITE_URL = 'https://nagi.shiki-01.com'
export const SITE_NAME = 'Nagi'
export const REPO_URL = 'https://github.com/shiki-01/nagi'
export const ISSUES_URL = `${REPO_URL}/issues`
/** sitemap.xml に載せる公開ページ（404 は含めない）。 */
export const PAGES = ['/', '/support', '/privacy', '/custom-keyboard', '/reference'] as const
