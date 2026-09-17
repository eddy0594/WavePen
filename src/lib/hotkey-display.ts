type Translate = (key: string) => string

/** Replaces native key tokens (e.g. `RightAlt`) with the user-facing key name. */
export function formatHotkeyForDisplay(hotkey: string, t: Translate, isMac: boolean): string {
  if (!hotkey) return hotkey
  return hotkey
    .split('+')
    .map((part) => {
      if (part === 'RightAlt') return t(isMac ? 'settings.rightOptionKey' : 'settings.rightAltKey')
      return part
    })
    .join('+')
}

function isMacNavigator(): boolean {
  return typeof navigator !== 'undefined' && navigator.platform.toUpperCase().includes('MAC')
}

/** {@link formatHotkeyForDisplay} for the platform the app is running on. */
export function formatHotkeyForPlatform(hotkey: string, t: Translate): string {
  return formatHotkeyForDisplay(hotkey, t, isMacNavigator())
}
