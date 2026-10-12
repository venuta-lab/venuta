import { motion } from 'motion/react'
import type { Section } from '../components/Rail'

const COPY: Record<string, string> = {
  review:
    'Proposed fixes will wait here for your approval, album by album, with every change shown before it happens.',
  incoming:
    'Drop new albums here and they come out tagged, with art, lyrics and levelled volume, filed in the right place.',
  playlists: 'Your playlists, rebuilt from Apple Music and kept in sync as files move.',
  cards: 'Your SD cards, each kept as a mirror of the library. Insert a card and it syncs.',
}

export function SoonScreen({ section }: { section: Section }) {
  const Icon = section.icon
  return (
    <div className="grid h-full place-items-center">
      <motion.div
        initial={{ scale: 0.96, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: 'spring', stiffness: 260, damping: 26, delay: 0.05 }}
        className="glass max-w-[460px] rounded-[28px] px-10 py-12 text-center"
      >
        <div
          className="mx-auto grid h-14 w-14 place-items-center rounded-2xl"
          style={{ background: 'color-mix(in oklab, var(--accent) 30%, transparent)' }}
        >
          <Icon />
        </div>
        <h1 className="mt-6 text-[26px] font-semibold tracking-[-0.02em]">{section.label}</h1>
        <p className="mt-3 text-[14.5px] leading-relaxed text-ink/65">{COPY[section.id]}</p>
        <p className="mt-6 text-[12px] text-ink/40">Arrives in a later milestone.</p>
      </motion.div>
    </div>
  )
}
