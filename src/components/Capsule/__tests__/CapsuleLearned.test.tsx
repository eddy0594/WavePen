import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CapsuleLearned } from '../CapsuleLearned'
import { useAppStore } from '../../../stores/appStore'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      key === 'capsule.learnedCorrection'
        ? `Learned ${values?.pattern} → ${values?.replacement}`
        : key === 'capsule.undoLearned'
          ? 'Undo'
          : key,
  }),
}))

const tauri = vi.hoisted(() => ({
  undoLearnedCorrection: vi.fn().mockResolvedValue(undefined),
  getDictionary: vi.fn().mockResolvedValue([]),
  getCorrectionRules: vi.fn().mockResolvedValue([]),
}))
vi.mock('../../../lib/tauri', () => tauri)

afterEach(() => {
  cleanup()
  useAppStore.getState().setLearnedCorrection(null)
})

describe('CapsuleLearned', () => {
  it('shows the learned correction and undoes it including the added word', async () => {
    useAppStore
      .getState()
      .setLearnedCorrection({ pattern: '論識', replacement: '潤飾', addedWord: true })
    render(<CapsuleLearned />)

    expect(screen.getByText('Learned 論識 → 潤飾')).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }))

    expect(useAppStore.getState().learnedCorrection).toBeNull()
    await waitFor(() =>
      expect(tauri.undoLearnedCorrection).toHaveBeenCalledWith('論識', '潤飾', true),
    )
  })
})
