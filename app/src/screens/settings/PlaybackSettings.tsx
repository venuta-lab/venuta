import { useEffect, useState } from 'react'
import { Segmented, Switch } from '../../components/ui'
import { OutputPicker } from '../../components/player/OutputPicker'
import { pauseWhenHeadphonesGo, setPauseWhenHeadphonesGo, setSound, usePlayer } from '../../lib/player'
import { NeverOffered, StreamQualityRow, WhatPlaysRows } from './ListeningSettings'
import { Group, PageHead, Row } from './parts'

/*
  Settings > Playback: where the sound goes, how it sounds, what plays next and how online songs
  come. Where the sound goes is also in the quality badge beside the song, and the sound tools in
  Now Playing's Sound: these are the same settings.
*/

/** The speakers, headphones or DAC Venuta plays on, whether it has them to itself
 *  (bit-perfect), and each output's delay. */
function Output() {
  const s = usePlayer()
  return (
    <div className="border-b border-edge py-4 last:border-b-0">
      <div className="text-[14px] font-medium">Play through</div>
      <div className="mt-0.5 max-w-[68ch] text-[12.5px] leading-relaxed text-ink/55">
        The speakers, headphones or DAC Venuta plays on, by kind. A chosen device that goes away (unplugged, switched off, a monitor asleep) hands over to Windows' default, and the music comes back to it when it returns; headphones and Bluetooth pause instead, unless you turn that off below. Also in the quality badge beside the song.
      </div>
      <div className="mt-3 max-w-[560px]">
        <OutputPicker signal={s.signal} roomy />
      </div>
    </div>
  )
}

/** Headphones or Bluetooth going away pause the music, so it never suddenly plays out loud. */
function PauseWhenHeadphonesGo() {
  const [on, setOn] = useState<boolean | null>(null)
  useEffect(() => {
    void pauseWhenHeadphonesGo().then(setOn)
  }, [])
  if (on === null) return null
  return (
    <Row
      title="Pause when headphones or Bluetooth disconnect"
      hint="When the headphones, headset or Bluetooth device the music is on goes away, the music pauses where it was instead of carrying on through the speakers. Wired or USB speakers going away still hand over to Windows' default."
    >
      <Switch
        label="Pause when headphones or Bluetooth disconnect"
        on={on}
        onChange={(v) => {
          setOn(v)
          void setPauseWhenHeadphonesGo(v)
        }}
      />
    </Row>
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
        <PauseWhenHeadphonesGo />
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
