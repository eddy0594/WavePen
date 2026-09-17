import type { PolishChineseScript } from '../stores/appStore'

/** Maps a BCP 47 system locale (e.g. `zh-Hant-TW`, `en-US`) to a supported UI language. */
export function detectUiLanguage(locale: string | undefined): string {
  const normalized = (locale ?? '').toLowerCase()
  if (normalized.startsWith('zh')) {
    const isSimplified =
      normalized.includes('hans') || normalized.endsWith('-cn') || normalized.endsWith('-sg')
    return isSimplified ? 'zh' : 'zh-TW'
  }
  return 'en'
}

export function systemUiLanguage(): string {
  return detectUiLanguage(typeof navigator !== 'undefined' ? navigator.language : undefined)
}

/**
 * Chinese output script implied by picking a UI language, or `null` when the
 * choice says nothing about Chinese (e.g. English) and the setting should stay.
 */
export function chineseScriptForUiLanguage(language: string): PolishChineseScript | null {
  if (language === 'zh-TW') return 'traditional'
  if (language === 'zh') return 'simplified'
  return null
}
