import { useEffect, useState } from 'react'
import { Segmented, Switch } from '../../components/ui'
import { outputChoices, setOutput, setSound, usePlayer, type OutputChoices } from '../../lib/player'
import { notify } from '../../lib/notify'
import { NeverOffered, StreamQualityRow, WhatPlaysRows } from './ListeningSettings'
import { Group, PageHead, Row } from './parts'

/*
  Settings > Playback: where the sound goes, how it sounds, what plays next and how online songs
  come. Where the sound goes is also in the quality badge beside the song, and the sound tools in
  Now Playing's Sound: these are the same settings.
*/

/** The speakers or DAC Venuta plays on, and whether it has them to itself (bit-perfect). */
function Output() {
  const [choices, setChoices] = useState<OutputChoices | null>(null)
  useEffect(() => {
    void outputChoices().then(setChoices)
  }, [])
  if (!choices) return null
  const change = (device: string | null, exclusive: boolean) => {
    setChoices({ ...choices, device, exclusive })
    void setOutput(device, exclusive).catch((e) => notify(String(e)))
  }
  return (
    <>
      <Row title="Play through" hint="The speakers, headphones or DAC Venuta plays on. Also in the quality badge beside the song.">
        <select
          value={choices.device ?? ''}
          onChange={(e) => change(e.target.value || null, choices.exclusive)}
          aria-label="Play through"
          className="field h-9 max-w-[320px] rounded-full px-3 text-[13px]"
        >
          <option value="">Windows default</option>
          {choices.devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
            </option>
          ))}
        </select>
      </Row>
      <Row title="Exclusive (bit-perfect)" hint="Each song at its own rate and depth, untouched. Other apps can't play sound meanwhile.">
        <Switch label="Exclusive (bit-perfect)" on={choices.exclusive} onChange={(on) => change(choices.device, on)} />
      </Row>
    </>
  )
}

const CROSSFADES = Array.from({ length: 13 }, (_, i) => i)

/** Volume levelling and crossfade, set once; the equalizer and tone, while you listen. */
function HowItSounds(props: { onOpenSound: () => void }) {
  const s = usePlayer()
  const sound = s.sound
  const eqOn = sound.eq && sound.bands.some((b) => Math.abs(b) > 0.05)
  const toneOn = Math.abs(sound.bass_db) > 0.05 || Math.abs(sound.treble_db) > 0.05
  return (
    <>
      <Row
        title="Volume levelling"
        hint="Evens out loudness: your files by their ReplayGain values, online songs by the catalog's own loudness for each. Per album keeps the differences within an album. Files without values play as they are; Health's quality check measures them."
      >
        <Segmented
          size="sm"
          label="Volume levelling"
          value={sound.leveling}
          onChange={(leveling) => void setSound({ ...sound, leveling })}
          options={[
            { value: 'off', label: 'Off' },
            { value: 'track', label: 'Per song' },
            { value: 'album', label: 'Per album' },
          ]}
        />
      </Row>
      <Row title="Crossfade" hint="Blends the end of one song into the next. Never between songs of the same album, so live albums and mixes stay seamless.">
        <select
          value={sound.crossfade_s}
          onChange={(e) => void setSound({ ...sound, crossfade_s: Number(e.target.value) })}
          aria-label="Crossfade"
          className="field h-9 rounded-full px-3 text-[13px]"
        >
          {CROSSFADES.map((sec) => (
            <option key={sec} value={sec}>
              {sec ? `${sec} ${sec === 1 ? 'second' : 'seconds'}` : 'Off'}
            </option>
          ))}
        </select>
      </Row>
      <Row
        title="Equalizer and tone"
        hint={`${eqOn ? `Equalizer on (${sound.preset})` : 'Equalizer off'}, ${toneOn ? 'tone changed' : 'tone flat'}. Presets, ten bands, bass, treble and the limiter, while you listen. Also the Sound button in the player bar.`}
      >
        <button onClick={props.onOpenSound} className="btn">
          Open Sound
        </button>
      </Row>
    </>
  )
}

export function PlaybackSettings(props: { onOpenSound: () => void }) {
  return (
    <div>
      <PageHead title="Playback" line="Where the sound goes, how it sounds, and what plays next. For your files and online songs alike." />
      <Group title="Where the sound goes">
        <Output />
      </Group>
      <Group title="How it sounds">
        <HowItSounds onOpenSound={props.onOpenSound} />
      </Group>
      <Group title="What plays">
        <WhatPlaysRows />
      </Group>
      <Group title="Online songs">
        <StreamQualityRow />
      </Group>
      <Group title="Never offered">
        <NeverOffered />
      </Group>
    </div>
  )
}
