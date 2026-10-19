import { ytmAddAccount, ytmForgetAccount, ytmSwitchAccount, ytmUseIdentity, type YtmIdentity, type YtmSavedAccount } from '../lib/engine'
import { menuFromEvent, type MenuGroups } from '../lib/menu'
import { notify } from '../lib/notify'
import { useYtmAccounts } from '../lib/ytmAccount'
import { summary } from '../lib/ytmChoiceWords'
import { openYtmChoices, useYtmChoices } from '../lib/ytmChoices'
import { askSignOut } from './askSignOut'
import { confirmAsk } from './Confirm'
import { RemoteArt } from './RemoteArt'

/*
  Who you are on YouTube Music, and switching: the channels of the sign-in in use (your own and
  any brand accounts), and the other Google accounts kept on this computer. In the title bar's
  corner as your picture, and in Settings, Online. Each linked playlist stays with the account
  it was linked on, and waits while another is in use. The picture's menu also says what Venuta
  does with the account in use, and opens your choices.
*/

const label = (a: { name: string | null; email: string | null }) => a.name ?? a.email ?? 'Google account'

const switchTo = (a: YtmSavedAccount) =>
  void ytmSwitchAccount(a.id)
    .then(() => notify(`Switched to ${label(a)} on YouTube Music.`))
    .catch((e) => notify(String(e)))

const add = () => void ytmAddAccount().catch((e) => notify(String(e)))

const forget = async (a: YtmSavedAccount) => {
  const ok = await confirmAsk({
    title: `Forget ${label(a)} on this computer?`,
    text: 'Its sign-in goes from Venuta. Nothing changes on the account; you can add it again any time.',
    confirm: 'Forget',
  })
  if (ok) await ytmForgetAccount(a.id).catch((e) => notify(String(e)))
}

/** The switcher as a menu: what is on, channels, other accounts, add, sign out. */
function menuOf(saved: YtmSavedAccount[], channels: YtmIdentity[], on: string | null): MenuGroups {
  const groups: MenuGroups = []
  if (on)
    groups.push([
      { label: on, disabled: true, run: () => {} },
      { label: 'Your YouTube Music choices…', run: openYtmChoices },
    ])
  if (channels.length > 1)
    groups.push(
      channels.map((c) => ({
        label: `${c.current ? '✓ ' : ''}${c.name}${c.handle ? ` (${c.handle})` : ''}`,
        disabled: c.current,
        run: () =>
          void ytmUseIdentity(c.id)
            .then(() => notify(`Using ${c.name} on YouTube Music.`))
            .catch((e) => notify(String(e))),
      })),
    )
  const others = saved.filter((a) => !a.current)
  groups.push([...others.map((a) => ({ label: `Switch to ${label(a)}`, run: () => switchTo(a) })), { label: 'Add another Google account…', run: add }])
  groups.push([{ label: 'Sign out of YouTube Music', run: () => void askSignOut() }])
  return groups
}

/** Your picture in the title bar's corner, signed in; the switcher behind it. */
export function YtmProfileButton() {
  const { account, saved, channels } = useYtmAccounts()
  const mine = useYtmChoices()
  if (!account) return null
  const on = mine ? summary(mine.choices) : null
  const using = channels.find((c) => c.current)
  const name = using?.name ?? account.name ?? 'YouTube Music'
  const picture = using?.picture ?? account.picture
  return (
    <button
      onClick={(e) => menuFromEvent(e, menuOf(saved, channels, on))}
      onContextMenu={(e) => menuFromEvent(e, menuOf(saved, channels, on))}
      aria-label={`YouTube Music: ${name}. Switch account`}
      title={`YouTube Music: ${name}${account.email ? ` (${account.email})` : ''}. Switch account or channel`}
      className="mr-1 grid h-8 w-8 place-items-center rounded-full transition-colors hover:bg-ink/10"
    >
      <span className="grid h-[22px] w-[22px] place-items-center overflow-hidden rounded-full bg-ink/[0.12] text-[11px] font-semibold text-ink/70">
        {picture ? <RemoteArt url={picture} size={22} fallback={name.slice(0, 1)} imgClassName="h-full w-full object-cover" /> : name.slice(0, 1)}
      </span>
    </button>
  )
}

/** Settings, Online: channels and kept accounts as rows. */
export function YtmAccountsSection() {
  const { account, saved, channels } = useYtmAccounts()
  const others = saved.filter((a) => !a.current)
  if (!account) {
    if (!others.length) return null
    return (
      <div className="mt-3 border-t border-edge pt-3">
        <div className="text-[13.5px] font-medium">Kept on this computer</div>
        <AccountRows accounts={others} />
      </div>
    )
  }
  return (
    <div className="mt-3 border-t border-edge pt-3">
      {channels.length > 1 && (
        <>
          <div className="text-[13.5px] font-medium">Channel</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">Your own, or a brand account you manage. Library, likes and playlists are that channel’s.</div>
          <ul className="mt-2 grid gap-1">
            {channels.map((c) => (
              <li key={c.id} className="flex items-center gap-3 rounded-xl px-2 py-1.5">
                <Avatar url={c.picture} name={c.name} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px]">{c.name}</div>
                  {c.handle && <div className="truncate text-[12px] text-ink/50">{c.handle}</div>}
                </div>
                {c.current ? (
                  <span className="text-[12px] text-ink/55">In use</span>
                ) : (
                  <button
                    onClick={() =>
                      void ytmUseIdentity(c.id)
                        .then(() => notify(`Using ${c.name} on YouTube Music.`))
                        .catch((e) => notify(String(e)))
                    }
                    className="btn btn-sm"
                  >
                    Use
                  </button>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
      <div className={channels.length > 1 ? 'mt-3' : ''}>
        <div className="flex items-center gap-4">
          <div className="min-w-[200px] flex-1">
            <div className="text-[13.5px] font-medium">Other Google accounts</div>
            <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">Kept here so you can switch without signing in again. A linked playlist syncs only with the account it was linked on.</div>
          </div>
          <button onClick={add} className="btn btn-sm">
            Add another
          </button>
        </div>
        {others.length > 0 && <AccountRows accounts={others} />}
      </div>
    </div>
  )
}

function AccountRows({ accounts }: { accounts: YtmSavedAccount[] }) {
  return (
    <ul className="mt-2 grid gap-1">
      {accounts.map((a) => (
        <li key={a.id} className="flex items-center gap-3 rounded-xl px-2 py-1.5">
          <Avatar url={a.picture} name={label(a)} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px]">{label(a)}</div>
            {a.email && a.name && <div className="truncate text-[12px] text-ink/50">{a.email}</div>}
          </div>
          <button onClick={() => switchTo(a)} className="btn btn-sm">
            Switch
          </button>
          <button onClick={() => void forget(a)} className="btn-ghost btn-sm">
            Forget
          </button>
        </li>
      ))}
    </ul>
  )
}

function Avatar({ url, name }: { url: string | null; name: string }) {
  return (
    <span className="grid h-8 w-8 shrink-0 place-items-center overflow-hidden rounded-full bg-ink/[0.08] text-[13px] font-semibold text-ink/60">
      {url ? <RemoteArt url={url} size={32} fallback={name.slice(0, 1)} imgClassName="h-full w-full object-cover" /> : name.slice(0, 1)}
    </span>
  )
}
