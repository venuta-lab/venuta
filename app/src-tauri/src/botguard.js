// The page side of YouTube's security check (BotGuard), run in Venuta's hidden window. The app
// fetches everything (the challenge, Google's interpreter script, the attestation) and hands it
// in here; this page only runs the JavaScript, as YouTube's own web page does, and keeps the
// minter it makes. Each call is a job the app polls for by its number (`take`).
(() => {
  if (window.__venutaBg) return;
  const jobs = new Map();
  let session = null;

  const fromBase64 = (s) => {
    const plain = s.replace(/[-_.]/g, (c) => ({ '-': '+', _: '/', '.': '=' })[c]);
    const bin = atob(plain);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  };
  const toWebsafeBase64 = (bytes) => {
    let bin = '';
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_');
  };
  const words = (e) => String((e && e.message) || e || 'failed').slice(0, 80);
  const within = (promise, ms, what) =>
    Promise.race([
      promise,
      new Promise((_, reject) => setTimeout(() => reject(new Error(what)), ms)),
    ]);
  const job = (id, work) => {
    jobs.set(id, { done: false });
    Promise.resolve()
      .then(work)
      .then(
        (value) => jobs.set(id, { done: true, ok: true, value }),
        (e) => jobs.set(id, { done: true, ok: false, value: words(e) }),
      );
  };

  window.__venutaBg = {
    // A finished job's outcome, once; null while it runs.
    take(id) {
      const j = jobs.get(id);
      if (!j || !j.done) return null;
      jobs.delete(id);
      return j;
    },

    // The interpreter, then the program with the page's config in place, then the snapshot.
    start(id, c) {
      job(id, async () => {
        if (session && session.shutdown) {
          try {
            session.shutdown();
          } catch (_) {
            // A session already gone is no matter.
          }
        }
        session = null;
        if (c.config) {
          window.yt = window.yt || {};
          window.yt.config_ = c.config;
        }
        new Function(c.script)();
        const vm = window[c.globalName];
        if (!vm || typeof vm.a !== 'function') throw new Error('BotGuard did not load');
        let started;
        const ready = new Promise((resolve) => (started = resolve));
        const setup = (snapshot, shutdown, passEvent, checkCamera) =>
          started({ snapshot, shutdown, passEvent, checkCamera });
        const quiet = () => {};
        vm.a(c.program, setup, true, undefined, quiet, [[], []], undefined, false, [
          quiet,
          quiet,
          quiet,
          quiet,
          quiet,
        ]);
        const fns = await within(ready, 15000, 'BotGuard did not start');
        if (typeof fns.snapshot !== 'function') throw new Error('BotGuard has no snapshot');
        const signal = [];
        const answer = await within(
          new Promise((resolve) => fns.snapshot(resolve, [undefined, undefined, signal, undefined])),
          15000,
          'BotGuard took no snapshot',
        );
        if (typeof answer !== 'string' || !answer) throw new Error('BotGuard gave no answer');
        session = { shutdown: fns.shutdown, signal, minter: null };
        return answer;
      });
    },

    // The minter, from the integrity token the attestation gave for this session's answer.
    minter(id, integrityToken) {
      job(id, async () => {
        const make = session && session.signal[0];
        if (typeof make !== 'function') throw new Error('BotGuard gave no minter');
        const minter = await within(
          Promise.resolve(make(fromBase64(integrityToken))),
          15000,
          'the minter took too long',
        );
        if (typeof minter !== 'function') throw new Error('BotGuard gave no minter');
        session.minter = minter;
        return true;
      });
    },

    // One token for a binding (a video id, the visitor data or a data sync id).
    mint(id, binding) {
      job(id, async () => {
        if (!session || !session.minter) throw new Error('no minter');
        const bytes = await within(
          Promise.resolve(session.minter(new TextEncoder().encode(binding))),
          8000,
          'mint timed out',
        );
        if (!(bytes instanceof Uint8Array) || !bytes.length) throw new Error('no token');
        return toWebsafeBase64(bytes);
      });
    },
  };
})();
