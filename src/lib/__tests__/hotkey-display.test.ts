import { describe, expect, it } from 'vitest'
import { formatHotkeyForDisplay } from '../hotkey-display'

const t = (key: string) =>
  ({ 'settings.rightOptionKey': 'Right Option', 'settings.rightAltKey': 'Right Alt' })[key] ?? key

describe('formatHotkeyForDisplay', () => {
  it('shows Right Option on macOS', () => {
    expect(formatHotkeyForDisplay('RightAlt', t, true)).toBe('Right Option')
    expect(formatHotkeyForDisplay('RightAlt+Space', t, true)).toBe('Right Option+Space')
  })

  it('shows Right Alt elsewhere and leaves other keys untouched', () => {
    expect(formatHotkeyForDisplay('RightAlt', t, false)).toBe('Right Alt')
    expect(formatHotkeyForDisplay('Fn+Space', t, true)).toBe('Fn+Space')
    expect(formatHotkeyForDisplay('', t, true)).toBe('')
  })
})
