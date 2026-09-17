import { useEffect } from 'react'
import { motion } from 'framer-motion'
import { BookCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { getCorrectionRules, getDictionary, undoLearnedCorrection } from '../../lib/tauri'

const NOTICE_DURATION_MS = 6000

export function CapsuleLearned() {
  const { t } = useTranslation()
  const learned = useAppStore((s) => s.learnedCorrection)
  const setLearnedCorrection = useAppStore((s) => s.setLearnedCorrection)

  useEffect(() => {
    const timer = setTimeout(() => setLearnedCorrection(null), NOTICE_DURATION_MS)
    return () => clearTimeout(timer)
  }, [learned, setLearnedCorrection])

  if (!learned) return null

  const handleUndo = (event: React.MouseEvent) => {
    event.stopPropagation()
    setLearnedCorrection(null)
    void undoLearnedCorrection(learned.pattern, learned.replacement, learned.addedWord)
      .then(() => Promise.all([getDictionary(), getCorrectionRules()]))
      .then(([dictionary, rules]) => {
        const store = useAppStore.getState()
        store.setDictionary(dictionary)
        store.setCorrectionRules(rules)
      })
      .catch((error) => console.error('Failed to undo learned correction:', error))
  }

  return (
    <motion.div
      className="relative z-10 flex h-9 items-center gap-2 pl-3 pr-1.5"
      initial={{ opacity: 0, x: -4 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      <BookCheck size={13} className="flex-shrink-0 text-white" />
      <p className="flex-1 truncate text-[11px] text-white">
        {t('capsule.learnedCorrection', {
          pattern: learned.pattern,
          replacement: learned.replacement,
        })}
      </p>
      <button
        type="button"
        onPointerDown={(event) => event.stopPropagation()}
        onPointerUp={(event) => event.stopPropagation()}
        onClick={handleUndo}
        className="flex-shrink-0 cursor-pointer rounded-full border-none bg-white/20 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-white/30"
      >
        {t('capsule.undoLearned')}
      </button>
    </motion.div>
  )
}
