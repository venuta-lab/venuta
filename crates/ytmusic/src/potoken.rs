//! Proof-of-origin ("PO") tokens, made the way YouTube's own web page makes them.
//!
//! YouTube's web player proves it is a real browser before it is answered as one: the page runs
//! Google's BotGuard program (a small VM in JavaScript), sends what the program says to Google's
//! attestation service, and gets back an *integrity token*. Fed back into the program, that token
//! turns into a *minter*, and the minter makes one short token per thing it vouches for (the
//! "content binding"): the video id for `/player`, the visitor data or the account's data sync id
//! for the streams. Signed in, YouTube Music's `/player` answers UNPLAYABLE without one, and with
//! no player answer there is no address to tell a play to.
//!
//! The steps, as LuanRT/BgUtils and bgutil-ytdlp-pot-provider document them:
//!
//! 1. The challenge: the signed-out www.youtube.com home page carries it inline
//!    (`window.ytAtN({'R': …})`, the same answer `/att/get` gives), with the page's own
//!    `ytcfg` next to it. The program reads that config (`yt.config_`), so the two go together.
//!    `/att/get` itself is the fallback for a page that doesn't carry one.
//! 2. The interpreter script named by the challenge (`//www.google.com/js/th/….js`).
//! 3. BotGuard runs the program and takes its snapshot: [`BotGuardVm::start`]. JavaScript, so it
//!    runs where the app has a browser engine (this crate knows nothing of webviews).
//! 4. `GenerateIT` with the snapshot: the integrity token, good for its stated lifetime.
//! 5. The minter from the integrity token ([`BotGuardVm::ready_minter`]), then one token per
//!    binding ([`BotGuardVm::mint`]), in a few milliseconds each.
//!
//! The integrity token and its minter are kept until shortly before the token runs out; a
//! failure to make them is not tried again for [`RETRY_AFTER`], so a broken check costs one
//! attempt, not one per song. Every request here is signed out: no cookie, no account.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

/// The key YouTube's page sends with every attestation request (the same for years).
pub const REQUEST_KEY: &str = "O43z0dpjhgX20SCx4KAo";
/// The attestation service's public API key, as the page sends it.
const API_KEY: &str = "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw";
/// What asks for the home page and the attestation: a desktop Edge, as the webview is.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
/// Where the interpreter scripts live; nothing else is ever run.
const INTERPRETER_HOST: &str = "//www.google.com";
const INTERPRETER_DIR: &str = "/js/th/";
/// A failure to make the minter is not tried again before this.
pub const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
/// The integrity token is let go this long before YouTube says it runs out, at least.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
/// What an integrity token is kept for when YouTube doesn't say.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(6 * 60 * 60);
/// The shortest time an integrity token is kept, however little YouTube says it has left.
const MIN_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// The whole of making a minter (page, script, program, attestation).
const MAKE_TIMEOUT: Duration = Duration::from_secs(45);
/// One token from a ready minter.
const MINT_TIMEOUT: Duration = Duration::from_secs(10);
/// The signature timestamp read off the page is used this long before the page is asked again
/// (YouTube's player changes every few days).
const STS_KEEP: Duration = Duration::from_secs(6 * 60 * 60);

/// Why no token could be made, in words safe for the playback log: never an id or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotError(pub String);

impl PotError {
    fn new(why: &str) -> Self {
        PotError(why.to_owned())
    }

    /// `why`, with what the browser engine said (cut short, letters and punctuation only).
    fn with_detail(why: &str, detail: &str) -> Self {
        let detail = tidy(detail);
        if detail.is_empty() {
            PotError::new(why)
        } else {
            PotError(format!("{why} ({detail})"))
        }
    }
}

impl std::fmt::Display for PotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PotError {}

/// A message from the browser engine, made safe to log: at most 80 characters of letters, digits,
/// spaces and plain punctuation. Anything that could carry a token or an id is long and dense, so
/// it is cut off with the rest.
fn tidy(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || " .,:;-_()'".contains(*c))
        .take(80)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// What BotGuard needs to run once: the challenge, and the page config it reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Challenge {
    /// The interpreter script's address as the challenge gives it (`//www.google.com/js/th/…`).
    pub interpreter_url: String,
    pub interpreter_hash: String,
    /// The VM program (base64 text).
    pub program: String,
    /// The global the interpreter defines (`window[global_name].a(...)` runs the program).
    pub global_name: String,
    /// The page's `ytcfg`, set as `yt.config_` before the program runs. None from `/att/get`
    /// when no page config could be had.
    pub page_config: Option<Value>,
}

/// A future from the browser engine side. Its error is the engine's own message.
pub type VmFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Where BotGuard's JavaScript runs. One session at a time: `start` begins a new one, dropping
/// any before it, and the minter it readies is the one `mint` uses.
pub trait BotGuardVm: Send + Sync {
    /// Run the interpreter `script`, then the challenge's program (with its page config in
    /// place), and take the snapshot: the text `GenerateIT` is sent.
    fn start<'a>(&'a self, script: &'a str, challenge: &'a Challenge) -> VmFuture<'a, String>;
    /// Turn the integrity token (base64, as `GenerateIT` gave it) into this session's minter.
    fn ready_minter<'a>(&'a self, integrity_token: &'a str) -> VmFuture<'a, ()>;
    /// One token for `binding`, web-safe base64.
    fn mint<'a>(&'a self, binding: &'a str) -> VmFuture<'a, String>;
}

/// The attestation answer: the integrity token, and how long it is good for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IntegrityToken {
    pub token: String,
    /// `estimatedTtlSecs`.
    pub ttl: Option<Duration>,
    /// `mintRefreshThreshold`: how long before the end a new one should be made.
    pub refresh: Option<Duration>,
}

impl IntegrityToken {
    /// `GenerateIT`'s answer: `[integrityToken, estimatedTtlSecs, mintRefreshThreshold,
    /// websafeFallbackToken]`. An error answer (no token first) is refused.
    pub(crate) fn from_answer(v: &Value) -> Result<Self, PotError> {
        let token = v
            .get(0)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| match v.get(0).and_then(Value::as_i64) {
                // An error answer: its code, and its message when it is short words.
                Some(code) => PotError::with_detail(
                    "the attestation gave no integrity token",
                    &format!(
                        "code {code}{}",
                        v.get(1)
                            .and_then(Value::as_str)
                            .map(|m| format!(": {m}"))
                            .unwrap_or_default()
                    ),
                ),
                None => PotError::new("the attestation gave no integrity token"),
            })?;
        let secs = |i: usize| {
            v.get(i)
                .and_then(Value::as_f64)
                .filter(|s| s.is_finite() && *s > 0.0)
                .map(Duration::from_secs_f64)
        };
        Ok(Self {
            token: token.to_owned(),
            ttl: secs(1),
            refresh: secs(2),
        })
    }

    /// How long the minter made from it is kept: its lifetime less the refresh threshold (at
    /// least [`REFRESH_MARGIN`]), never less than [`MIN_LIFETIME`].
    pub(crate) fn keep_for(&self) -> Duration {
        let ttl = self.ttl.unwrap_or(DEFAULT_LIFETIME);
        let margin = self.refresh.unwrap_or_default().max(REFRESH_MARGIN);
        ttl.saturating_sub(margin).max(MIN_LIFETIME)
    }
}

/// `GenerateIT`'s body: the request key and BotGuard's snapshot.
pub(crate) fn generate_it_body(snapshot: &str) -> String {
    serde_json::json!([REQUEST_KEY, snapshot]).to_string()
}

/// The `/att/get` body, as the web page sends it signed out.
pub(crate) fn att_get_body(client_version: &str) -> String {
    serde_json::json!({
        "context": { "client": { "clientName": "WEB", "clientVersion": client_version } },
        "engagementType": "ENGAGEMENT_TYPE_UNBOUND",
    })
    .to_string()
}

/// The web client version `/att/get` is sent as when the page doesn't say.
const ATT_CLIENT_VERSION: &str = "2.20261019.01.00";

/// A JavaScript string literal's body, unescaped (`\xHH`, `\uHHHH`, `\/`, `\\`, `\'` …).
/// None when an escape is broken.
pub(crate) fn unescape_js(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    let hex = |chars: &mut std::str::Chars<'_>, n: usize| -> Option<u32> {
        let digits: String = chars.by_ref().take(n).collect();
        (digits.len() == n)
            .then(|| u32::from_str_radix(&digits, 16).ok())
            .flatten()
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'x' => out.push(char::from_u32(hex(&mut chars, 2)?)?),
            'u' => {
                let unit = hex(&mut chars, 4)?;
                if (0xD800..0xDC00).contains(&unit) {
                    // A surrogate pair: the low half follows as its own `\uDCxx`.
                    if chars.next()? != '\\' || chars.next()? != 'u' {
                        return None;
                    }
                    let low = hex(&mut chars, 4)?;
                    let code = 0x10000 + ((unit - 0xD800) << 10) + (low.checked_sub(0xDC00)?);
                    out.push(char::from_u32(code)?);
                } else {
                    out.push(char::from_u32(unit)?);
                }
            }
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '0' => out.push('\0'),
            other => out.push(other),
        }
    }
    Some(out)
}

/// The body of the single-quoted string literal `s` starts with (after its opening quote is
/// skipped), up to its closing quote. None when it never closes.
fn single_quoted(s: &str) -> Option<&str> {
    let body = s.strip_prefix('\'')?;
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '\'' => return Some(&body[..i]),
            _ => {}
        }
    }
    None
}

/// The page's first `ytcfg.set({...})` object: the one with `EVENT_ID` and the client version.
pub(crate) fn page_config(html: &str) -> Option<Value> {
    let start = html.find("ytcfg.set({")? + "ytcfg.set(".len();
    let mut values = serde_json::Deserializer::from_str(&html[start..]).into_iter::<Value>();
    values.next()?.ok().filter(Value::is_object)
}

/// The current player's signature timestamp (`STS` in the page's config). The web clients'
/// `/player` answers UNPLAYABLE ("Video unavailable") without a current one, signed in or out.
pub(crate) fn page_sts(html: &str) -> Option<i32> {
    page_config(html)?
        .get("STS")?
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .filter(|n| *n > 0)
}

/// The challenge the signed-out home page carries inline (`window.ytAtN({'R': '…'})`), with the
/// page's own config.
pub(crate) fn challenge_from_home(html: &str) -> Result<Challenge, PotError> {
    let none = || PotError::new("the YouTube page carried no security check");
    let call = html.find("window.ytAtN({").ok_or_else(none)?;
    let rest = &html[call..];
    let r = rest.find("'R':").ok_or_else(none)?;
    let literal = single_quoted(rest[r + 4..].trim_start()).ok_or_else(none)?;
    let text = unescape_js(literal).ok_or_else(none)?;
    let answer: Value = serde_json::from_str(&text).map_err(|_| none())?;
    challenge_from_attestation(&answer, page_config(html))
}

/// The challenge in an attestation answer (`{bgChallenge: {...}}`, from the page or `/att/get`).
pub(crate) fn challenge_from_attestation(
    answer: &Value,
    page_config: Option<Value>,
) -> Result<Challenge, PotError> {
    let bad = || PotError::new("the security check YouTube sent couldn't be read");
    let bg = answer.get("bgChallenge").ok_or_else(bad)?;
    let text = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let interpreter_url =
        text(bg.pointer("/interpreterUrl/privateDoNotAccessOrElseTrustedResourceUrlWrappedValue"))
            .ok_or_else(bad)?;
    if interpreter_path(&interpreter_url).is_none() {
        return Err(PotError::new(
            "the security check named a script from somewhere unexpected",
        ));
    }
    let global_name = text(bg.get("globalName")).ok_or_else(bad)?;
    if !global_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return Err(bad());
    }
    Ok(Challenge {
        interpreter_url,
        interpreter_hash: text(bg.get("interpreterHash")).unwrap_or_default(),
        program: text(bg.get("program")).ok_or_else(bad)?,
        global_name,
        page_config,
    })
}

/// The interpreter's path on www.google.com (`/js/th/<name>.js`), when the address is one of
/// Google's interpreter scripts and nothing else.
pub(crate) fn interpreter_path(url: &str) -> Option<&str> {
    let path = url
        .strip_prefix("https:")
        .unwrap_or(url)
        .strip_prefix(INTERPRETER_HOST)?;
    let name = path.strip_prefix(INTERPRETER_DIR)?.strip_suffix(".js")?;
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some(path)
}

/// A minted token looks like one: web-safe base64, and long enough to hold a signature.
pub(crate) fn looks_like_token(s: &str) -> bool {
    s.len() >= 32
        && s.len() <= 4096
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=')
}

/// Where each request goes (tests point them at a local server).
#[derive(Debug, Clone)]
pub(crate) struct Urls {
    pub home: String,
    pub att_get: String,
    pub generate_it: String,
    /// What the interpreter's path is put after (`https://www.google.com`).
    pub interpreter_origin: String,
}

impl Default for Urls {
    fn default() -> Self {
        Self {
            home: "https://www.youtube.com/".into(),
            att_get: "https://www.youtube.com/youtubei/v1/att/get?prettyPrint=false".into(),
            generate_it: "https://jnn-pa.googleapis.com/$rpc/google.internal.waa.v1.Waa/GenerateIT"
                .into(),
            interpreter_origin: "https://www.google.com".into(),
        }
    }
}

/// The minter as it stands.
#[derive(Debug, Default)]
enum Minter {
    #[default]
    None,
    /// Ready, good until then.
    Ready(Instant),
    /// Making one failed; not tried again before then, and why.
    Failed(Instant, PotError),
}

/// What a token asked for now needs.
#[derive(Debug, PartialEq)]
enum Step {
    /// The minter is ready: mint with it.
    Use,
    /// Making one failed a short while ago: the same answer, nothing asked.
    Refuse(PotError),
    /// None, or it ran out, or the wait after a failure is over: make one.
    Make,
}

impl Minter {
    fn next_step(&self, now: Instant) -> Step {
        match self {
            Minter::Ready(until) if now < *until => Step::Use,
            Minter::Failed(until, why) if now < *until => Step::Refuse(why.clone()),
            _ => Step::Make,
        }
    }
}

/// Makes PO tokens: the integrity token and its minter kept, one token per binding.
pub struct PoTokens {
    http: reqwest::Client,
    urls: Urls,
    vm: RwLock<Option<Arc<dyn BotGuardVm>>>,
    minter: tokio::sync::Mutex<Minter>,
    /// When the page was last read for the signature timestamp, and what it said (None: the
    /// page couldn't be had or didn't say).
    sts: std::sync::Mutex<Option<(Instant, Option<i32>)>>,
}

impl Default for PoTokens {
    fn default() -> Self {
        Self::with_urls(Urls::default())
    }
}

impl PoTokens {
    pub(crate) fn with_urls(urls: Urls) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            urls,
            vm: RwLock::new(None),
            minter: tokio::sync::Mutex::new(Minter::None),
            sts: std::sync::Mutex::new(None),
        }
    }

    /// Where BotGuard runs, from now on. A minter made elsewhere is let go.
    pub fn install(&self, vm: Arc<dyn BotGuardVm>) {
        *self.vm.write().unwrap_or_else(|e| e.into_inner()) = Some(vm);
        if let Ok(mut m) = self.minter.try_lock() {
            *m = Minter::None;
        }
    }

    /// Something to run BotGuard in is installed.
    pub fn can_mint(&self) -> bool {
        self.vm_now().is_some()
    }

    fn vm_now(&self) -> Option<Arc<dyn BotGuardVm>> {
        self.vm.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Let the minter go (a token it made was refused): the next token starts afresh.
    pub async fn forget(&self) {
        *self.minter.lock().await = Minter::None;
    }

    /// One token for `binding` (a video id for `/player`; the visitor data, or the data sync id
    /// signed in, for the streams). Makes the minter first when there is none or it has run out.
    pub async fn mint(&self, binding: &str) -> Result<String, PotError> {
        let vm = self
            .vm_now()
            .ok_or_else(|| PotError::new("the security check can't run here"))?;
        let mut minter = self.minter.lock().await;
        match minter.next_step(Instant::now()) {
            Step::Use => {}
            Step::Refuse(why) => return Err(why),
            Step::Make => {
                *minter = Minter::None;
                let made = tokio::time::timeout(MAKE_TIMEOUT, self.make_minter(vm.as_ref()))
                    .await
                    .unwrap_or_else(|_| Err(PotError::new("the security check took too long")));
                match made {
                    Ok(keep) => *minter = Minter::Ready(Instant::now() + keep),
                    Err(e) => {
                        *minter = Minter::Failed(Instant::now() + RETRY_AFTER, e.clone());
                        return Err(e);
                    }
                }
            }
        }
        let token = tokio::time::timeout(MINT_TIMEOUT, vm.mint(binding))
            .await
            .unwrap_or_else(|_| Err("timed out".into()));
        match token {
            Ok(t) if looks_like_token(&t) => Ok(t),
            Ok(_) => {
                *minter = Minter::None;
                Err(PotError::new("the security check made an unusable token"))
            }
            Err(e) => {
                *minter = Minter::None;
                Err(PotError::with_detail("couldn't make a token", &e))
            }
        }
    }

    /// Steps 1 to 5: a ready minter in the VM, and how long to keep it.
    async fn make_minter(&self, vm: &dyn BotGuardVm) -> Result<Duration, PotError> {
        let challenge = self.challenge().await?;
        let script = self.interpreter(&challenge).await?;
        let snapshot = vm
            .start(&script, &challenge)
            .await
            .map_err(|e| PotError::with_detail("BotGuard didn't run", &e))?;
        if snapshot.trim().is_empty() {
            return Err(PotError::new("BotGuard gave no answer"));
        }
        let it = self.integrity_token(&snapshot).await?;
        vm.ready_minter(&it.token)
            .await
            .map_err(|e| PotError::with_detail("BotGuard made no minter", &e))?;
        Ok(it.keep_for())
    }

    /// The current player's signature timestamp, for the web clients' `/player` (see
    /// [`page_sts`]): off the page the last challenge came with, or the page asked for on its own
    /// when that is older than [`STS_KEEP`]. None when the page can't be had, and then it isn't
    /// asked again before [`RETRY_AFTER`].
    pub async fn signature_timestamp(&self) -> Option<i32> {
        let kept = *self.sts.lock().unwrap_or_else(|e| e.into_inner());
        match kept {
            Some((at, Some(sts))) if at.elapsed() < STS_KEEP => return Some(sts),
            Some((at, None)) if at.elapsed() < RETRY_AFTER => return None,
            _ => {}
        }
        self.home_page().await;
        let kept = *self.sts.lock().unwrap_or_else(|e| e.into_inner());
        kept.and_then(|(_, sts)| sts)
    }

    /// The signed-out home page, what it says of the signature timestamp kept.
    async fn home_page(&self) -> Option<String> {
        let page = async {
            self.http
                .get(&self.urls.home)
                .header("accept-language", "en-US,en;q=0.9")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .ok()?
                .text()
                .await
                .ok()
        }
        .await;
        let sts = page.as_deref().and_then(page_sts);
        *self.sts.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), sts));
        page
    }

    /// The challenge: the home page's own, else `/att/get`'s (with the page's config if any).
    async fn challenge(&self) -> Result<Challenge, PotError> {
        let page = self.home_page().await;
        if let Some(html) = &page {
            if let Ok(c) = challenge_from_home(html) {
                return Ok(c);
            }
        }
        let config = page.as_deref().and_then(page_config);
        let version = config
            .as_ref()
            .and_then(|c| c.get("INNERTUBE_CLIENT_VERSION"))
            .and_then(Value::as_str)
            .unwrap_or(ATT_CLIENT_VERSION)
            .to_owned();
        let answer: Value = self
            .http
            .post(&self.urls.att_get)
            .header("content-type", "application/json")
            .body(att_get_body(&version))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| PotError::new("couldn't reach YouTube for the security check"))?
            .json()
            .await
            .map_err(|_| PotError::new("the security check YouTube sent couldn't be read"))?;
        challenge_from_attestation(&answer, config)
    }

    /// The interpreter script, from Google only.
    async fn interpreter(&self, challenge: &Challenge) -> Result<String, PotError> {
        let path = interpreter_path(&challenge.interpreter_url).ok_or_else(|| {
            PotError::new("the security check named a script from somewhere unexpected")
        })?;
        let script = self
            .http
            .get(format!("{}{path}", self.urls.interpreter_origin))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| PotError::new("couldn't fetch the security check's script"))?
            .text()
            .await
            .map_err(|_| PotError::new("couldn't fetch the security check's script"))?;
        if script.trim().is_empty() {
            return Err(PotError::new("the security check's script was empty"));
        }
        Ok(script)
    }

    /// Step 4: the snapshot for an integrity token.
    async fn integrity_token(&self, snapshot: &str) -> Result<IntegrityToken, PotError> {
        let answer: Value = self
            .http
            .post(&self.urls.generate_it)
            .header("content-type", "application/json+protobuf")
            .header("x-goog-api-key", API_KEY)
            .header("x-user-agent", "grpc-web-javascript/0.1")
            .body(generate_it_body(snapshot))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| PotError::new("the attestation refused BotGuard's answer"))?
            .json()
            .await
            .map_err(|_| PotError::new("the attestation's answer couldn't be read"))?;
        IntegrityToken::from_answer(&answer)
    }
}

/// The process's token maker: one integrity token and minter for the whole app.
pub fn po_tokens() -> &'static PoTokens {
    static TOKENS: OnceLock<PoTokens> = OnceLock::new();
    TOKENS.get_or_init(PoTokens::default)
}

#[cfg(test)]
mod tests;
