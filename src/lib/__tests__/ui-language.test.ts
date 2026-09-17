import { describe, expect, it } from 'vitest'
import { chineseScriptForUiLanguage, detectUiLanguage } from '../ui-language'

describe('detectUiLanguage', () => {
  it('maps Traditional Chinese system locales to zh-TW', () => {
    expect(detectUiLanguage('zh-TW')).toBe('zh-TW')
    expect(detectUiLanguage('zh-Hant-TW')).toBe('zh-TW')
    expect(detectUiLanguage('zh-HK')).toBe('zh-TW')
  })

  it('maps Simplified Chinese system locales to zh', () => {
    expect(detectUiLanguage('zh-CN')).toBe('zh')
    expect(detectUiLanguage('zh-Hans-CN')).toBe('zh')
  })

  it('falls back to English for everything else', () => {
    expect(detectUiLanguage('en-US')).toBe('en')
    expect(detectUiLanguage('ja-JP')).toBe('en')
    expect(detectUiLanguage(undefined)).toBe('en')
  })
})

describe('chineseScriptForUiLanguage', () => {
  it('derives the output script only from Chinese UI languages', () => {
    expect(chineseScriptForUiLanguage('zh-TW')).toBe('traditional')
    expect(chineseScriptForUiLanguage('zh')).toBe('simplified')
    expect(chineseScriptForUiLanguage('en')).toBeNull()
  })
})
