import { describe, expect, it } from 'vitest'
import constantsSource from '../constants.ts?raw'

describe('release version wiring', () => {
  it('lets frontend builds read the release tag version from Vite env', () => {
    expect(constantsSource).toContain('import.meta.env.VITE_APP_VERSION')
  })
})
