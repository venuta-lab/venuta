/*
  The "Copy for Venuta" bookmark. Saved once in your own browser; clicked on a music website where
  you're signed in (Spotify, Apple Music, YouTube Music, TIDAL, JioSaavn, Amazon Music, Deezer…),
  it scrolls the playlist (or your liked songs) so all of it loads, reads each song as the page
  shows it, and copies the list. Venuta reads it when you paste it in Import & export. The page
  sends nothing anywhere; nothing is installed.

  What it copies: a first line `#VENUTA-PLAYLIST:<name>|<site>` (engine/src/links.rs reads it),
  then one song a line: title, artists, album, length, separated by tabs.
*/

/** The bookmark's script, as readable source (run by `copySongs`). Exported for testing. */
export const BOOKMARK_SOURCE = `(async (copy) => {
  const host = location.hostname;
  const site =
    /spotify/.test(host) ? 'Spotify' :
    /music\\.apple/.test(host) ? 'Apple Music' :
    /music\\.youtube/.test(host) ? 'YouTube Music' :
    /tidal/.test(host) ? 'TIDAL' :
    /jiosaavn/.test(host) ? 'JioSaavn' :
    /amazon/.test(host) ? 'Amazon Music' :
    /deezer/.test(host) ? 'Deezer' :
    /soundcloud/.test(host) ? 'SoundCloud' : host;
  const text = (e) => (e ? e.textContent || '' : '').replace(/\\s+/g, ' ').trim();
  // The first element with text for the first selector that has one (a picture link has none).
  const first = (row, sels) => { for (const s of sels) for (const e of row.querySelectorAll(s)) if (text(e)) return e; return null; };
  const ROWS = [
    '[data-testid="tracklist-row"]',
    '[data-testid="track-list-item"]',
    'ytmusic-responsive-list-item-renderer',
    '[data-track-id]', '[data-test="tracklist-row"]',
    'music-image-row', 'music-text-row',
    'li:has(a[href*="/song/"])',
    '[role="row"]',
  ];
  const TITLE = [
    '[data-testid="internal-track-link"]', '[data-testid="track-title"]',
    '.title-column .title', 'yt-formatted-string.title',
    '[data-test="table-cell-title"]', 'a[href*="/song/"]', 'a[href*="/track/"]', 'a[href*="watch?v="]',
  ];
  const ARTIST = 'a[href*="/artist/"], a[href*="/artists/"], a[href*="channel/"], [data-testid="track-title-by-line"] a';
  const ALBUM = ['[data-testid="track-column-tertiary"] a', 'a[href*="browse/MPRE"]', 'a[href*="/albums/"]', 'a[href*="/album/"]:not([href*="?i="])'];
  const songOf = (row) => {
    let title = text(first(row, TITLE));
    if (!title) {
      const attr = row.getAttribute('primary-text');
      title = attr || '';
    }
    if (!title) return null;
    const artists = [...new Set([...row.querySelectorAll(ARTIST)].map(text).filter(Boolean))].join(', ') || row.getAttribute('secondary-text-1') || '';
    const album = text(first(row, ALBUM)) || row.getAttribute('secondary-text-2') || '';
    let length = row.getAttribute('duration') || '';
    if (!length) for (const e of row.querySelectorAll('*')) { if (!e.children.length && /^\\d{1,2}:\\d{2}(:\\d{2})?$/.test(text(e))) { length = text(e); break; } }
    return [title, artists, album, length].map((x) => x.replace(/\\t/g, ' '));
  };
  // The playlist's rows: the innermost matches, in the page's longest list (not a sidebar's).
  const rows = () => {
    for (const s of ROWS) {
      const all = [...document.querySelectorAll(s)];
      if (!all.length) continue;
      const inner = all.filter((r) => !r.querySelector(s));
      const groups = new Map();
      const listOf = (r) => r.parentElement.closest('[role="grid"], [role="rowgroup"], [role="list"], ol, ul, tbody, ytmusic-playlist-shelf-renderer, ytmusic-shelf-renderer') || r.parentElement;
      for (const r of inner) groups.set(listOf(r), [...(groups.get(listOf(r)) || []), r]);
      return [...groups.values()].sort((x, y) => y.length - x.length)[0];
    }
    return [];
  };
  const songs = new Map();
  const read = () => rows().forEach((row, i) => {
    const s = songOf(row);
    if (!s) return;
    const at = row.closest('[aria-rowindex]')?.getAttribute('aria-rowindex') || row.getAttribute('data-row') || row.getAttribute('aria-rowindex');
    songs.set(at ? 'n' + at.padStart(6, '0') : s[0] + '|' + s[1], s);
  });
  // Scroll whatever holds the list so a long one loads all of it.
  const firstRow = rows()[0];
  let box = firstRow;
  while (box && !(box.scrollHeight > box.clientHeight + 40 && /auto|scroll/.test(getComputedStyle(box).overflowY))) box = box.parentElement;
  box = box || document.scrollingElement;
  const start = box.scrollTop;
  box.scrollTop = 0;
  await new Promise((r) => setTimeout(r, 400));
  read();
  let last = -1, still = 0;
  while (still < 4 && songs.size < 20000) {
    box.scrollTop += Math.max(200, box.clientHeight * 0.7);
    await new Promise((r) => setTimeout(r, 300));
    read();
    if (box.scrollTop === last) still++; else still = 0;
    last = box.scrollTop;
  }
  box.scrollTop = start;
  // The playlist's own title, not the sidebar's (Spotify's 'Your Library' is an h1 too).
  const name = text(document.querySelector('[data-testid="entityTitle"]')) || text(document.querySelector('main h1')) || document.title.split(/ [-|·–] /)[0].trim() || 'Copied songs';
  const keys = [...songs.keys()].sort((a, b) => (a[0] === 'n' && b[0] === 'n' ? (a < b ? -1 : 1) : 0));
  const list = '#VENUTA-PLAYLIST:' + name.replace(/[|\\n]/g, ' ') + '|' + site + '\\n' + keys.map((k) => songs.get(k).join('\\t')).join('\\n') + '\\n';
  const say = (words) => {
    const d = document.createElement('div');
    d.textContent = words;
    d.style.cssText = 'position:fixed;z-index:2147483647;left:50%;bottom:32px;transform:translateX(-50%);background:#1d1b20;color:#fff;font:600 14px system-ui,sans-serif;padding:12px 18px;border-radius:12px;box-shadow:0 10px 30px rgba(0,0,0,.35)';
    document.body.appendChild(d);
    setTimeout(() => d.remove(), 4000);
  };
  if (!copy) return list;
  if (!songs.size) { say('Venuta found no songs on this page. Open a playlist or your liked songs first.'); return; }
  try { await navigator.clipboard.writeText(list); } catch {
    const t = document.createElement('textarea'); t.value = list; document.body.appendChild(t); t.select(); document.execCommand('copy'); t.remove();
  }
  say('Copied ' + songs.size + ' songs for Venuta. Paste them in Import & export.');
})(true)`

/** The bookmark itself: what you save in your browser's bookmarks bar. */
export const BOOKMARK_URL = 'javascript:' + encodeURIComponent(BOOKMARK_SOURCE)
