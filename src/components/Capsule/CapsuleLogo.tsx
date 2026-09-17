/** WavePen mark: sound bars flowing into a handwritten stroke (monochrome app icon). */
export function CapsuleLogo({ size = 22, className = '' }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={className}
      aria-hidden="true"
    >
      <g transform="translate(-4.08 -3.6) scale(0.24)">
        <rect x="22" y="58" width="7" height="14" rx="3.5" fill="currentColor" opacity="0.75" />
        <rect x="34" y="46" width="7" height="38" rx="3.5" fill="currentColor" opacity="0.75" />
        <rect x="46" y="38" width="7" height="54" rx="3.5" fill="currentColor" opacity="0.75" />
        <rect x="58" y="50" width="7" height="30" rx="3.5" fill="currentColor" opacity="0.75" />
        <path
          d="M70 65 C78 40 86 40 90 60 C94 80 100 86 106 70 C109 62 110 56 112 52"
          stroke="currentColor"
          strokeWidth="7"
          strokeLinecap="round"
        />
      </g>
    </svg>
  )
}
