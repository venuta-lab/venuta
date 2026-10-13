/*
  What a folder screen says when there is nothing to show: the folder has gone, or it has no
  music in it. Each says what happened and offers the way on, never just an empty page.
*/

function Action(props: { onClick: () => void; primary?: boolean; children: React.ReactNode }) {
  return (
    <button onClick={props.onClick} className={`${props.primary ? 'btn-primary' : 'btn'} btn-lg`}>
      {props.children}
    </button>
  )
}

function Shell(props: { title: string; text: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="grid h-full place-items-center px-6">
      <div className="glass max-w-[560px] rounded-[26px] px-9 py-8">
        <h1 className="title break-words text-[28px] leading-tight">{props.title}</h1>
        <div className="mt-3 text-[14.5px] leading-relaxed text-ink/65">{props.text}</div>
        <div className="mt-7 flex flex-wrap gap-2">{props.children}</div>
      </div>
    </div>
  )
}

/** The open folder can't be found: an unplugged drive, or a renamed or moved folder. */
export function MissingFolder(props: {
  path: string
  isLibrary: boolean
  onRetry: () => void
  onChoose: () => void
  onLibrary?: () => void
}) {
  return (
    <Shell
      title={`${props.path} can't be found`}
      text={
        <>
          If it is on a drive that is not plugged in, plug it back in and try again. If the folder was renamed or moved, choose
          it where it is now.
          {props.isLibrary && ' Your answers, history and settings are inside it, so nothing is lost while it is away.'}
        </>
      }
    >
      <Action primary onClick={props.onRetry}>
        Try again
      </Action>
      <Action onClick={props.onChoose}>Choose where it is now</Action>
      {props.onLibrary && <Action onClick={props.onLibrary}>Open your library</Action>}
    </Shell>
  )
}

/** A folder with no music in it: your new, empty library, or the wrong folder chosen. */
export function NoMusic(props: { path: string; isLibrary: boolean; onAdd: () => void; onChoose: () => void }) {
  if (props.isLibrary)
    return (
      <Shell
        title="Your library is empty"
        text="Add music you've bought or downloaded, and it comes in tagged, with covers and lyrics. If you already have a folder of music, bring it in and Venuta builds a clean copy here."
      >
        <Action primary onClick={props.onAdd}>
          Add new music
        </Action>
        <Action onClick={props.onChoose}>Bring in a music folder</Action>
      </Shell>
    )
  return (
    <Shell
      title="No music in this folder"
      text={`${props.path} has no songs Venuta can read (FLAC, MP3, M4A, WAV and others). Choose the folder that holds your albums; folders inside it are read too.`}
    >
      <Action primary onClick={props.onChoose}>
        Choose another folder
      </Action>
    </Shell>
  )
}

/** Before a library is chosen, every place leads back to setting one up. */
export function NoLibrary(props: { onSetup: () => void }) {
  return (
    <Shell
      title="Set up your library first"
      text="Venuta keeps your music and everything it knows about it in one folder, your library. Choose where it lives, then bring in music you have or start empty."
    >
      <Action primary onClick={props.onSetup}>
        Set up your library
      </Action>
    </Shell>
  )
}
