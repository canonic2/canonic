# Browsers

Testrun provides isolated Chrome, Firefox and WebKit sessions on **macOS 15 x64**.
On **Windows x64**, the installed CLI and stdio MCP can launch managed Chrome
for Testing, Firefox and WebKit. Linux browser launch is currently unavailable.
For a headed launch, start the Testrun manager in the active Windows console
session. From an SSH service session, use a headless managed launch. Use
`testrun browser doctor --engine webkit` to check the available modes and
preparation state for WebKit. For the first launch and a page check, follow
[Getting started](getting-started.md#try-a-browser).
It prepares private pinned browser builds automatically, with Node and Playwright
for Firefox/WebKit.
You do not need a global Node, npm or Playwright installation.
[Browser investigations](diagnostic-browser.md) retain selected settled browser
results and native query output through the shared diagnostic evidence workflow.

The Firefox/WebKit surface supports contexts, pages, retained evaluation handles,
callback responses, structured evaluation, page reads, locators, input, navigation,
waits, screenshots, bounded diagnostics, read-only JPEG previews, H.264 recordings and registered
file transfers. Browser audio and the Chromium engine selection are unavailable. External
viewer hosts can acquire input for an exact owned page through the shared
[control handoff](viewer-control.md). The supported host and media limits are
described below.

Chrome has a native Rust CDP lifecycle implementation on the same host. It uses
the private Chrome for Testing 156.0.8078.47 build and supports context/page
creation, listing, selection and close, navigation, bounded page reads, frame
entry, native input, locator actions, dialogs, registered file/directory transfers,
downloads, typed page events and ordered batches, with status, cancellation and
disconnect. Screenshots, semantic/image comparisons, annotations, JPEG previews,
H.264 recording and viewer control use the shared contracts below. Chrome also
exports PDFs in headless mode. Chrome does not require Node or Playwright.
Structured evaluation, handles and bounded diagnostic history are available on
Chrome, Firefox and WebKit. Chrome uses document-bound native CDP object groups;
its evaluation and network inspection do not require Node or Playwright.

The first connection to private Chrome for Testing acquires a 205 MB archive.
An ordinary launch uses a fresh private profile and an inherited DevTools
pipe. Chrome accepts explicit
`viewport` and `deviceScaleFactor` creation options, with optional `acceptDownloads`
and `hasTouch`. The CLI launch defaults to 1280×720 at scale 1 and exposes
`--downloads`/`--touch`; MCP context options select other geometry. Other context
options are refused before preparation. Chrome can run beside Firefox/WebKit
browsers and mobile devices; use the returned `browser:` target when several
browsers are connected. Chrome disconnect
requests normal exit, retains cleanup across timeouts, and removes its profile
after witnessed process release. Select a retained profile or attach explicitly
when a fixture needs to outlive the connection.
Firefox/WebKit disconnect closes the browser through its Playwright driver; when
every witnessed browser process has exited but the driver is still running two
seconds later, and a fresh check finds no other live child of it, Testrun ends
the driver process it owns. If the browser processes could not be inspected
before closing, the driver is never ended and the disconnect reports
`CLEANUP_PENDING`.
On macOS, disposable Chrome profiles use a test keychain and avoid login
keychain prompts.

An owned Chrome launch holds new pages, cross-process frames and dedicated workers
until its target setup completes. Status reports pending pauses and acknowledged
resumes in `targetStartup`. If startup setup fails or attachment events are lost,
disconnect the owned browser before reconnecting; Testrun retains the paused
ownership rather than repeating uncertain setup or silently running that target.

## Select a profile, installed channel or running browser

These modes are available on macOS 15 x64. On Windows x64, managed Chrome for
Testing supports `--disposable-profile` in either available launch mode.
Windows installed-channel launches require the active console session, even
with `--mode headless`.

```sh
testrun connect --engine firefox --profile acme-qa
testrun connect --engine firefox --borrowed-profile /private/tmp/acme-profile
testrun connect --engine chrome --channel chrome --mode headed
testrun connect --engine chrome --attach ws://127.0.0.1:9222/devtools/browser/acme
```

- `--profile` creates or reopens a private named profile. `--borrowed-profile`
  selects an existing private directory with its original
  `.testrun-profile-v1.json` compatibility record, for example a closed
  fixture copied from a retained profile. The engine, channel and browser
  build must match the record. Disconnect stops the process Testrun launched
  and preserves the data. Use disconnect to release the persistent context.
- `--channel chrome` selects Google Chrome Stable and `--channel msedge`
  selects Microsoft Edge Stable. Both use the native Chrome backend and
  default to a new private profile. `browser doctor --engine chrome` reports
  installed build metadata and whether it meets the pinned CDP version floor.
  After startup, `browser_status` reports the observed browser and protocol
  identity.
- `--attach` selects a running Chrome CDP browser or Firefox/WebKit
  Playwright server. Use a direct WebSocket endpoint. Attachment creates a new isolated
  context by default. Chrome can adopt an existing isolated context via
  `--adopt-context` with its exact CDP context ID. Testrun selects its existing
  page only when there is exactly one. Disconnect releases the connection and
  its own isolated contexts. The server, adopted context and unrelated tabs
  remain running. Use a launch for host download directories and Chrome's
  origin-bound client-certificate tunnel.

To authenticate an attachment, provision the files on the Testrun manager
host. Use the private `attachments/` subdirectory of the configured
`TESTRUN_BROWSER_ROOT` or of the default Intel macOS browser cache at
`~/.local/share/testrun/browser-bundles/v1-darwin-x64-macos15`. Give the
directory mode `0700` and files mode `0600`. Place each file directly in that
directory.

- `--attachment-credential FILE` selects a JSON record such as
  `{"origin":"wss://browser.example.com","token":"Acme-token"}`.
  Include the port in the origin when the endpoint uses a non-default port.
- `--attachment-ca FILE` selects a private PEM CA for this TLS connection.

File paths may be absolute or relative to the calling directory. Testrun
reports TLS verification, authorization and an absent endpoint separately as
`ATTACHMENT_TLS_FAILED`, `ATTACHMENT_AUTHORIZATION_FAILED` and
`ATTACHMENT_ENDPOINT_NOT_FOUND`.

In MCP, use `connect` or `browser_connect` with `engine` and one of
`profile: {kind: "disposable"}`,
`profile: {kind: "retained", name: "acme-qa"}`,
`profile: {kind: "borrowed", path: "/private/tmp/acme-profile"}`, or
`attachment: {endpoint, credentialPath?, caPath?, contextId?}`. The optional
`channel` field selects a branded launch and can accompany a profile. A direct
endpoint can be supplied by a hosted browser service. Provisioning, provider
account management, browser auto-discovery and URL-embedded credentials are
outside the connection contract.

### Test an unpacked Chrome extension

Use the regular context of a Chrome for Testing profile owned by Testrun.
For a fresh profile, use `testrun browser connect --engine chrome --disposable-profile`
or pass `profile: {kind: "disposable"}` to MCP. Testrun removes that
profile at disconnect. For a named retained profile, use `--profile NAME`
or pass its retained profile descriptor to MCP.
Register the extension files as fixtures, then use `directory_register` to
assemble `manifest.json` and its assets into one directory. Pass that directory
reference to `browser_extension` with `action.kind` set to `install`. Keep the
returned `extensionId` for later actions.

| `action.kind` | Fields | Result |
| --- | --- | --- |
| `list` | `cursor?`, `limit?` | Installation state for this connection's extensions and `nextCursor` |
| `reload` | `extensionId` | Reload from the verified source directory |
| `trigger_action` | `extensionId`, `page` | Default action dispatched to the named page's tab |
| `uninstall` | `extensionId` | Installation removed and extension workers released |

Use `browser send` with `operation:"browser_extension"` and a nested `action`.
With shared `act`, use `action:"browser_extension"` and the nested action name
in `input.action`. Verify the extension's page effect separately from the
dispatch receipt. Chrome activates the selected tab before running its
default action. Start each new connection by registering and installing the
fixture again. Extension IDs belong to the connection that installed them.
Uninstall before disconnect to verify removal from a retained profile.
Disconnect reports any remaining connection-owned IDs in
`cleanup.extensionCleanupUnverified`.

### Test a progressive web app

In an owned Chrome for Testing profile, navigate a page to the standard web
app. Send `browser_pwa` with `action.kind:"install"` and that `page`. The page
must be served over HTTPS or loopback HTTP and have a valid manifest. Keep the
returned `appId`.

- `list` checks installation status for this connection's apps. Use optional
  `cursor` and `limit` with the returned `nextCursor`.
- `launch` takes `appId` and an optional in-scope `url`. The result supplies the
  app page reference for normal browser actions.
- `uninstall` takes `appId` and removes the installation and its launched tabs.

With shared `act`, use `action:"browser_pwa"` and the nested action name in
`input.action`. After an interrupted mutation, check `list` for pending work
and use `uninstall` or disconnect to release the owned installation.
Disconnect makes a bounded attempt to remove apps and reports any PWA IDs
whose OS cleanup could not be verified in
`cleanup.pwaCleanupUnverified`.

Chrome accepts the shared application-control operations below through
`browser send` and shared browser `act`. Use the complete page reference
from status, then poll the returned operation ID as described under
[shared commands](#use-the-shared-commands). For example, after replacing the
UUID placeholders with the returned identities:

```sh
testrun browser send '{"operation":"page_navigate","page":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"navigation":{"kind":"goto","url":"https://example.com","waitUntil":"load"}}'
testrun browser send '{"operation":"page_read","page":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"read":"text","locator":{"kind":"role","role":"heading"}}'
```

Reads support the locator kinds and read modes below, including open shadow DOM
and explicitly entered cross-origin frames. A single-value read refuses multiple
matches. ARIA and form/state reads mask password and autocomplete credential
values, including password fields with overridden roles, without changing input
values. Navigation and content replacement make old frame references stale;
read again or re-enter the frame to obtain its current reference. A root geometry
read issues a short-lived coordinate-input observation. Navigation reports actual observed
HTTP response metadata and uses `null` when there was no new observed response,
including fragment changes. `responseMetadataComplete: false` reports dropped
diagnostic events. `networkidle` uses Chrome's lifecycle observation, not a proof
that application work finished. Load waits follow client redirects to the current
main document. Chrome method failures include the CDP code and a backend message
bounded to 2048 characters in `backendFailure` (`partial.backendFailure` for a
failed batch step); the message can contain application data.
Standalone condition waits keep status, cancellation and ordinary operations
responsive. A batch keeps its steps in order and can be inspected with an
independent `page_read` while it waits. Chrome geometry reads stay in ordinary
order because they produce an input observation. A locator wait that times
out after observing zero matches reports `locatorMatched: false` and suggests
checking the locator and frame. Dialogs and chooser responses use a reserved
lane. Visible/hidden conditions account for iframe ancestors, including
cross-site frames.

### Snapshot references, readable content and computed styles

`page_read` with `read: "snapshot"` returns a compact list of controls, roles and
iframe elements. Each entry includes a locator with `kind: "element"`, its exact
frame/document reference and an element UUID. Pass that locator to a later read
or action. Detached, evicted, foreign or replaced-document identities are refused;
references are bounded and cannot be stored as selectors for a future journey.
If the page exceeds the snapshot budget, `result.value` retains the controls
that fit. When a page read reports `narrowingRequired`, its `hint` explains how
to retry with a `locator` for a smaller region, dialog or container.

Use `read: "readable"` for the rendered article/main text, or `page_read_url`
with an explicit HTTP(S) URL for a bounded HTML read through the owned context's
network credentials. Direct URL reads do not navigate the page or execute fetched
scripts. They return HTTP status and inert extracted content, capped at 256 KiB
of fetched HTML and 8 KiB of output. Redirect handling follows the backend loader.
`read: "styles"` returns common computed CSS properties; `page_styles` accepts
up to 64 explicit property names and `maxBytes` from 256 to 8192.

On Chrome, `page_css` inspects the native cascade. Open a DevTools attachment
with `developer_open`, then pass its `developer`, the same `page`, and a
`locator` (including snapshot element references); an optional `frame` selects
an owned child document. Rules include inline/inherited declarations, custom
properties, source positions and layer/media/container/scope/supports wrappers.
Declaration `state` is Chrome's `Active`, `Overloaded` or `null` when the model
does not determine a state. Results retain native cascade order.

Use `offset` and `limit` (default 10, maximum 50) to page through rules. This is
a live observation rather than a frozen snapshot. Output is capped at 64 KiB;
large declaration blocks or wrapper lists set `truncated:true`. `nextOffset`
and `hasMore` describe remaining rules. Source URLs omit credentials and query
strings; CSS declarations themselves are private application text. Closing an
attachment during inspection returns `DEVELOPER_BUSY`; close it after the query
settles. Firefox/WebKit retain computed-property inspection through `page_styles`.

`page_clipboard` takes `page`, an exact `origin` such as `https://example.com`,
and `action: {kind: "read"}` or `{kind: "write", text: "Acme"}`. This accesses
the **system text clipboard**, capped at 16 KiB, through the current owned page.
Origin mismatch and insecure contexts are refused before clipboard access.
Chrome grants the requested clipboard permission only to that origin in its
owned context. WebKit reads require the origin's `clipboard-read` permission;
Firefox clipboard reads are unsupported by the pinned backend. Writes on
Firefox/WebKit require browser permission or current user activation and return
`CLIPBOARD_PERMISSION_REQUIRED` when refused. No host clipboard command is used.

### Registered directories and reusable journeys

`directory_register` accepts a basename and up to 16 entries, each with a
relative `path` and a registered `fixture` reference. Paths cannot escape the
private directory; depth is at most eight and combined bytes at most 512 KiB.
Pass the returned directory reference alone to `page_upload` or `chooser_resolve`
for a directory input. Testrun verifies source hashes and the private directory
tree before attachment. Directory uploads on Firefox or WebKit require a
Testrun-owned launch. Attached Playwright sessions accept ordinary file
fixtures as streamed payloads.

Store a journey as the same typed action list you submit to shared `act`.
Resolve fresh page/frame/snapshot references for each run. Batches execute at most
eight steps in order, stop on failure/cancellation, and preserve completed results.
An action's optional postcondition observes the application after dispatch; a failed
postcondition retains `dispatchCompleted: true` and never replays the write.
`force` and scripted `dispatch` remain explicit modes in the result.

## Start a browser

```sh
testrun browser list
testrun browser doctor --engine firefox
testrun connect --engine firefox --mode headless
```

List and doctor never download or launch. Connect immediately returns `targetId`
and `preparationOperationId`. Run `testrun browser status --target BROWSER_TARGET`
with that target ID to follow preparation and startup. Wait for `browser.phase`
to be `ready` before submitting page operations. Use `--mode headed` for a
separate visible window. Select `--engine webkit` for WebKit.
Each connect starts another browser, so Firefox and WebKit, or several of
either, can run side by side; see [MCP and cleanup](#mcp-and-cleanup).

Run `testrun browser doctor --engine firefox` or choose `webkit` to see the
missing archive IDs and their total `downloadBytes` for your host. Node and
the driver are shared. Cache reuse checks installed contents without network
access. Intel macOS defaults to
`~/.local/share/testrun/browser-bundles/v1-darwin-x64-macos15/`. Windows x64 uses
`%LOCALAPPDATA%\Testrun\browser-bundles\v1-win32-x64\`.
The advanced host variable `TESTRUN_BROWSER_ROOT` selects an absolute private
cache root, including for isolated development fixtures. No project config or
agent-provided executable, download URL or profile path is accepted.

## Use the shared commands

`connect`, `act`, `observe`, `status` and `disconnect` cover mobile and browser
targets. Browser work is asynchronous: submission returns before completion.
After connect, copy its `targetId`, inspect status until ready, and use that
explicit browser target for shared commands. `browser status` without a target
lists the connected browsers, including after a lost connect reply.
Each browser connect allocates another browser; inspect before retrying it.

```sh
testrun act '{"action":"page_list"}' --target browser:CONNECTION-UUID
testrun status --target browser:CONNECTION-UUID --action-id ACTION-UUID --wait-ms 30000
testrun observe --target browser:CONNECTION-UUID --page '{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"}'
testrun cancel --target browser:CONNECTION-UUID --action-id ACTION-UUID
testrun disconnect --target browser:CONNECTION-UUID
```

Replace the UUID placeholders with complete returned identities. Browser
connections default to headless in the CLI and MCP. Choose `headed` when the
user needs to interact with a visible browser window. MCP `connect` and
`browser_connect` accept `engine`, optional `mode` and optional `context`.
The CLI uses `--engine`, `--mode` and `--context JSON`. Browser
`act` takes one JSON action or a JSON array in CLI; MCP takes an `actions` array
of 1..8 actions. Each uses `action`, for example:

```json
{"targetId":"browser:CONNECTION-UUID","actions":[{"action":"page_action","page":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"locator":{"kind":"role","role":"button","name":"Save"},"input":{"action":"click"}}],"timeoutMs":30000}
```

Use `input.action` for `page_action`, `context_clock` and `context_credentials`, and `actions[].action`
inside `page_input`. Locators, conditions, navigation and serialized values retain
their typed `kind` fields. Browser coordinates remain **CSS viewport pixels**
with a fresh input observation, while mobile points remain display fractions.
No coordinate conversion, engine fallback or mobile action emulation is inferred.

Browser `act` accepts optional `--action-id UUID` and `--timeout-ms` (MCP
`actionId`/`timeoutMs`, default 30,000, range 1..900,000). Reuse the ID with the
same actions to retrieve the retained operation; changed actions conflict.
`status` returns the complete `operation` and its polling `actionId`. Inspect
`operation.settled`, `outcome`, `code` and `result`. A successful submission or
expired status wait does not establish that an action completed.

Browser batches execute in order and stop on failure, cancellation or timeout.
They keep ordered `results`, `completedActions` and, on failure, `failedIndex`,
the failing action's `partial` and owned input cleanup. Completed work is never
rolled back or replayed. There is no implicit screenshot or settling: include a
`page_screenshot`, `page_read` or `page_wait` action, or call `observe` and poll
its result. Mobile-only `profile`, `settle`, `observeAfterEach` and CLI
`--final-only` do not apply to browser batches.

For a responsive callback, resource close/stop or emergency `input_release`
while a batch is running, use the individual `browser_send` request below.
Those requests have reserved capacity; a batch waits for the mutation lane.
`input_release` is excluded from batches. Cancel requests do not undo dispatched
effects. Inspect partial results and the actual page before deciding on new work.

The `browser_*` tools and CLI commands remain supported with their own exact
request/operation/result fields. They also provide the advanced typed surface
described in the rest of this guide.

## Context creation options

Supply context creation options through shared CLI
`connect --engine ENGINE --context JSON`, MCP `browser_connect.context`, or
`context_create.options` (including CLI `browser send`). The legacy
`browser connect` command has no `--context` flag. Omitting the whole options
object uses a 1280×720
viewport, device scale factor 1, downloads denied and touch disabled. A supplied
object requires `viewport` and `deviceScaleFactor` unless it names a `device`;
optional fields are:

- `device`: a device name from discovery's `settings.devices`, such as
  `"iPhone 15"` or `"Pixel 7"`, from the pinned Playwright descriptor list. It
  supplies `userAgent`, `viewport`, `screen`, `deviceScaleFactor`, `isMobile`
  and `hasTouch`; any of those you also pass overrides the descriptor. Unknown
  names refuse with `UNKNOWN_DEVICE`. A mobile descriptor on Firefox refuses
  like `isMobile: true`.
- `screen`: width and height in CSS pixels, each 1..8192, as for `viewport`.
- `hasTouch`, `acceptDownloads` and `isMobile`: booleans. `isMobile: true` is
  available on Chrome and WebKit; Firefox refuses it before allocating a context.
- `userAgent`: 1..512 characters; `locale` and `timezoneId`: 1..128 characters.
  Control characters are refused. The browser validates locale/timezone values;
  native Chrome reports an unknown timezone as `INVALID_TIMEZONE` when the
  context's first page opens, as the pinned SDK does. On native Chrome a custom
  user agent also sets matching user-agent client hints (platform, version,
  mobile) derived as Playwright does.
- `geolocation`: `{latitude, longitude, accuracy}` set from creation. Pages read
  it only with the `geolocation` permission granted.
- `permissions`: permission names granted for every origin from creation.
  Discovery's `settings.permissionNames` lists the selected engine's names;
  others refuse with `PERMISSION_UNSUPPORTED` before allocation.
- `serviceWorkers`: `allow` (default) or `block`, which makes
  `navigator.serviceWorker.register` resolve without registering anything.
- `extraHTTPHeaders`: name/value pairs, up to 64 headers and 16 KiB combined.
  These defaults apply to requests in the owned context, including future pages.
  Native Chrome also applies these headers to dedicated workers, merged with
  their page's headers, and to existing and future context-owned service
  workers, which never receive page-specific headers.
- `httpCredentials`: a credential object or ordered list on all three engines.
  Each entry requires `username` and `password`, with optional
  `origin` and `send` (`unauthorized` by default, or `always` for associated HTTP
  requests). Native Chrome answers challenges for owned pages, child frames,
  their dedicated workers and live context-owned service workers. As in the
  pinned SDK, shared workers get neither credentials nor context headers on
  native Chrome. The bounds and
  origin-selection rules below also apply here.
- `javaScriptEnabled`, `bypassCSP`, `ignoreHTTPSErrors` and `offline`: explicit
  native booleans. Native defaults apply when omitted: JavaScript enabled,
  CSP and HTTPS checks enabled, online. On native Chrome these apply to pages,
  child frames and popups; `offline` and the user agent also apply to
  context-owned service workers, and dedicated workers follow their page's
  network.
- `colorScheme`: `dark`, `light`, `no-preference` or `no-override`;
  `reducedMotion`: `reduce`, `no-preference` or `no-override`;
  `forcedColors`: `active`, `none` or `no-override`;
  `contrast`: `more`, `no-preference` or `no-override`. Without them, pages
  report a light scheme, no reduced motion, no forced colors and no contrast
  preference, as in Playwright.
- `baseURL`: an absolute HTTP(S) URL, up to 2048 ASCII characters, without
  credentials or fragment; any other value refuses with `INVALID_BASE_URL`.
  It is available on every engine, including native Chrome. See
  [Relative URLs](#relative-urls).
- `recordHar`: `{content, mode, urlFilter, path, omitContent}` records the
  context's HTTP(S) traffic from creation, like an `har_capture_start` issued at
  once. As in Playwright, `content` defaults to `attach` for a `.zip` path and
  `embed` otherwise; `omitContent:true` selects `omit` when `content` is
  omitted. `mode` defaults to `full`, and `urlFilter` resolves against `baseURL`.
  `context_close` publishes the private archive and returns it as `har`.
  Optional `path` also writes your own file, as for `har_capture_start`; a
  relative path is relative to the directory you run the command from.
  Invalid filters refuse before the context is created. A `path` that can't
  be used refuses after creation with `recordHarPending` and the new context,
  so you can close it.
  Available on every engine.

These options apply at context creation. They do not change an existing context
or silently reload its pages. Device scale factor is finite and in 0.5..4.
Unknown fields and unsupported engine options fail explicitly.

Use `page_set_extra_http_headers` with `{page, headers}` to replace one owned
page's header list. Names merge case-insensitively with context defaults; page
values win, including empty values. For an empty custom header, pinned Firefox
omits that header on the wire; Chrome and WebKit send the empty value. The
context value is suppressed in each case. `headers:[]` removes page overrides and
reveals current context defaults. The page list survives navigation and applies
to child frames and to requests from the page's dedicated workers, as in
Playwright; sibling pages and associated HTTP clients retain context defaults. Each supplied list has the bounds above. Receipts report counts only.
Possibly applied failures report uncertainty without replay; explicitly
reconfigure or close the owned page. Broader worker and rollback parity remains
under investigation.

### Relative URLs

In a context created with `baseURL`, Testrun resolves relative URLs the way URLs
resolve in a browser (`new URL(value, baseURL)`), for:

- `page_navigate` `goto` URLs;
- exact URLs and globs in `network_route`, `page_route_set`/`context_route_set`,
  `page_har_set`/`context_har_set` and `har_capture_start` `urlFilter`;
- `web_socket_route_add` URLs and exact/glob socket matchers, using the base
  with `ws:` or `wss:` in place of `http:` or `https:`;
- `diagnostics_wait` condition `url` values, using the `ws:`/`wss:` base for
  `web_socket` conditions;
- `page_wait` `url` values and `url_match` exact/glob matchers;
- browser-associated `http_context_create` clients that set no `baseUrl` of their own.

For example, with `"baseURL":"https://example.com/app/"`, `"/api/*"` matches
`https://example.com/api/item` and `"data/**"` matches URLs under
`https://example.com/app/data/`. Globs starting with `*` and all regexes are used
unchanged. Glob wildcard segments keep their spelling; only literal URL parts are
normalized. Typed route removal compares resolved matchers, so a relative
matcher and its absolute equivalent select the same registration.

Without `baseURL`, relative values keep their existing refusals, such as
`INVALID_NAVIGATION_URL` and `ROUTE_PATTERN_INVALID`. HTTP header defaults apply
to absolute requests with or without a base URL.

## Submit and inspect operations

```sh
testrun browser send '{"operation":"context_create"}'
testrun browser status --operation-id RETURNED-UUID --wait-ms 30000
testrun browser send '{"operation":"page_list"}'
```

An operation returns its ID immediately. Status returns its outcome and result;
wait expiration leaves the operation owned by Testrun. Reusing `--operation-id`
with the same request retrieves the same operation; different arguments are refused.
`browser cancel --operation-id UUID` prevents queued work or records cancellation
requested after dispatch. Dispatched effects are never replayed or undone.

Requests include `context_create/list/select/close`, `page_create/list/select/close`,
`handle_create/dispose` and `callback_resolve`. Use the exact resource reference
from a prior result. Closing a selected page/context clears selection. Document
navigation invalidates its handles. Evaluation expressions can change page state;
only run expressions authorized for the task.

## Read and control a page

Submit these requests through `browser send` or MCP `browser_send`.
Copy complete page/frame references from results; references include the
connection, resource ID and generation.

| Request | Behavior |
| --- | --- |
| `page_navigate` | `goto`, `set_content`, `reload`, `back`, `forward`; explicit load milestone |
| `page_read` | Bounded `text`, `dom`, `aria`, `form`, `count`, `state`, `geometry`, `messages`; use the returned locator hint when the reply is too large |
| `page_action` | Click, double click, hover, fill, clear, focus, blur, check, select, press, type, drag, dispatch |
| `page_wait` | Attached, detached, visible, hidden, enabled, disabled, editable, checked, text, value, count, exact `url`, `url_match` |
| `frame_enter` | Resolve an iframe element into an owned frame reference |
| `page_screenshot` | PNG/JPEG viewport, full page, element or clip capture |
| `page_resize` | Explicit CSS viewport width/height, with preview source reconfiguration |
| `page_input` | Native CSS viewport mouse, keyboard and touch actions with a fresh observation |
| `input_release` | Fence older raw input and balance owned held keys/buttons |

Chrome locator actions and coordinate input target the requested page without
bringing the browser window to the front.

Use an ISO date such as `2026-01-15` with `fill` on an HTML date input. Chrome
sets the value directly and emits the normal `input` and `change` events.

Locators use a `kind`: `css`/`xpath` with `selector`, `role` with `role` and
optional `name`, `text`/`label`/`placeholder`/`alt`/`title` with `text`, or
`test_id` with `value`. `within`, `filter` and `nth` compose scopes. CSS and
semantic selectors reach open shadow roots; XPath follows its native rules.
Actions require one match. Ambiguous locators and stale document references
are refused. Inspect the published request schema for each variant's fields.
A click that removes its own target can complete and let the batch continue.
Use a postcondition when you need to verify the effect. A batch stopped by
`AMBIGUOUS_LOCATOR` returns the earlier results and the zero-based `failedIndex`.
Narrow that step before continuing from it.

Set `capture.outputPath` on `page_screenshot` to save the image directly
in an existing evidence directory. For example, use
`capture: {format: "png", outputPath: "/path/to/run-evidence/checkout.png"}`.
The image is written as a new private file. The response reports its actual
location in `attachment.path`. Relative paths resolve from the caller's
working directory. Use an absolute approved path with Shield over MCP.
If the image was published and temporary-file cleanup or the directory flush
failed, `CAPTURE_IO` includes the attachment and `durabilityConfirmed` in
the operation result. The durability flag reports whether the directory flush
completed. `cleanupCode: "CLEANUP_FAILED"` means a temporary file may remain.
A batch reports these fields in the failed step's `partial` result.

`page_wait` with `{"kind":"url","value":...}` waits for the frame's exact URL.
`{"kind":"url_match","matcher":...}` waits until the URL matches an exact, glob,
regex or URL pattern matcher, as in `network_route`, for example
`{"kind":"url_match","matcher":{"kind":"glob","pattern":"**/checkout?step=*"}}`.
Matching is done by Testrun, not page script, and applies only to HTTP(S) URLs.
In a context with `baseURL`, both forms accept relative URLs.
Once the URL matches, `url_match` also waits for the frame's document to reach
`waitUntil`: `load` (the default, after the load event), `domcontentloaded` or
`commit` (as soon as the URL matches). For example,
`{"kind":"url_match","matcher":{"kind":"glob","pattern":"**/checkout"},"waitUntil":"domcontentloaded"}`
does not wait for images. `networkidle` waits for the load event and then for
the browser to report no network requests for 500 ms; a page that keeps polling
the network never reaches it, so prefer `load` or an element condition.

Normal actions use Playwright actionability checks. `force: true` is explicit
and supported only for compatible actions; `dispatch` explicitly sends a
scripted event. Results identify the mode. An optional `postcondition` waits for
an observed condition; failure preserves `dispatchCompleted: true` when input
already happened. Check the application outcome before any retry.

If a dispatched compound input action fails, Testrun cannot identify which
key/button phases the driver completed. It returns `OUTCOME_UNKNOWN` with the
underlying `cause` and `cleanup.requiresOwnedPageClose: true`. This also applies
to driver ambiguity and actionability errors when physical input was possible.
Close that owned page before continuing input; `input_release` cannot safely
repeat an unknown release. A failed postcondition after successful dispatch
does not require closing the page.

`OUTCOME_UNKNOWN` alone does not establish a lost driver reply: Firefox/WebKit
also use it for a received SDK timeout. Their private lifecycle journal keeps a
bounded `backendFailure` witness when the error retains a known backend cause.
It distinguishes an SDK reply from a transport deadline/loss and preserves known
dispatch state, without copying error text, stacks, logs or URLs. An absent
witness leaves attribution unknown. These observations do not authorize replay
or change required owned-page cleanup.

Reads default to 8 KiB and report truncation or require a narrower scope.
Form summaries mask password values. Transient live-region messages have a
bounded history and may report loss. Page content remains untrusted input.

Screenshots include decoded dimensions, SHA-256, an owned artifact path, a
request/completion time bracket, and geometry before/after capture. CSS/device
scale, quality, transparency, animation disabling and locator masks are explicit
options. The limit is 32 million pixels and 16 MiB per image, with 64 captures
and 256 MiB per session. Geometry changes can suppress the transform or refuse
a masked capture. Stable exact-viewport captures can return an `inputObservation`.
MCP status includes a native
image attachment when it fits the presentation limit.

For direct input, read `geometry` on the main frame and copy its
`inputObservation`. Submit `page_input` with `page`, that `observation`, and an
`actions` array. For example, `{kind: "mouse_click", x: 80, y: 120}` clicks in
CSS viewport coordinates. A device-scale screenshot's pixel coordinates must
first be divided by its `transform.cssToPixels`. A live frame supplies an
`inputObservation` only when its content geometry has a verified transform.
Observations expire after ten seconds and are refused when
navigation, resize, scroll or other measured geometry changes.

Native actions include `mouse_move`, `mouse_down`, `mouse_up`, `mouse_click`,
`mouse_wheel`, `touch_tap`, `key_down`, `key_up`, `key_press`, `type`,
`insert_text` and `delay`; inspect the schema for their fields. Touch requires a
context created with `hasTouch: true`, or CLI `browser connect --touch`.
Keys/buttons held with down remain owned until up, `input_release`, or page
closure. Holds expire after 30 seconds of host time; expiry fences older input
and schedules release after retained active input settles. An uncertain release
stays blocked for owned-page recovery. Release-only batches can omit `observation`. Locator actions refuse
while raw holds remain. `input_release` invalidates older observations, so read
fresh geometry before continuing.

Input failures preserve completed-phase counts and cleanup evidence. An unknown
release is never retried automatically; `requiresOwnedPageClose` means closing
the owned page is needed to prove release. Successful native dispatch does not
prove an application effect: read the relevant result before retrying.

## Cookies and page storage

These requests work alike on Chrome, Firefox and WebKit, as Playwright's
`BrowserContext.cookies`/`addCookies`/`clearCookies` and
`Page.localStorage`/`Page.sessionStorage` do. Reads return secret values, so
results carry `secretBearing:true`.

```text
context_cookies_read: {context: CONTEXT_REF, urls: "https://example.com/"}
context_cookies_add: {context: CONTEXT_REF, cookies: [{name: "acme", value: "1", url: "https://example.com"}]}
context_cookies_clear: {context: CONTEXT_REF, options: {name: {regex: "^session"}}}
page_web_storage_read: {page: PAGE_REF, kind: "local"}
page_web_storage_read: {page: PAGE_REF, kind: "session", name: "acme"}
page_web_storage_update: {page: PAGE_REF, kind: "local", update: {action: "set", name: "acme", value: "1"}}
```

`context_cookies_read` returns a page of cookies in name, domain, path and
partition order; omit `urls` to read every cookie. Follow `nextCursor` or
`nextRequest` with the same `urls`. Each added cookie needs either `url` or
both `domain` and `path`. `context_cookies_clear` without options clears every
cookie; `name`, `domain` and `path` each take an exact string or `{regex, flags}`,
and only matching cookies expire.

`page_web_storage_read` reads the page's main frame: one item with `name`
(`found:false` when absent), or a page of items in name order. Update actions are
`set` (with `name` and `value`), `remove` (with `name`) and `clear`. Names are at
most 4 KiB and values at most 512 KiB.

## Storage state

Create a context with state already in place by passing `storageState` to
`context_create`, or in `browser_connect`'s `context`: inline state or a file path
relative to the CLI or MCP client's working directory. It is applied before the
context's first page and is not retained afterwards. Firefox and WebKit take
Playwright storage state. Chrome takes its own format or Playwright state, which
it converts.

`storage_export` captures an owned context's cookies and local storage.
`options.indexedDB:true` adds IndexedDB and `options.opfs:true` adds OPFS
(refused on WebKit). `options.path` also writes the state to a file relative to
the caller, creating owner-only directories and replacing an existing file. The
result is a snapshot reference with counts, hash and a private artifact; values
are omitted. `options.credentials:true` adds the context's
[virtual passkeys](#test-passkeys-with-a-virtual-authenticator), private keys
included, to the private snapshot. Session storage is never included.

Chrome snapshots use the versioned `testrun-chrome-storage-v1` format, which names
the Chrome build, the included storage types and each cookie's partition key.
A Chrome snapshot with credentials uses `testrun-chrome-storage-v2`, which adds
them; Testrun reads both.
Firefox and WebKit use Playwright storage state. `storage_register` imports an
explicit absolute `path` to a private regular state file and copies it
privately. On Chrome, a Playwright file is converted and the result reports
`converted:true`. Each connection retains up to 16 snapshots of at most 1 MiB.
References cannot cross connections, and `browser_status` with `includeDetails: true` lists them under
`storage`.

```text
storage_export: {context: CONTEXT_REF, options: {indexedDB: true, path: "state.json"}}
storage_register: {path: "/private/Acme/state.json"}
storage_apply: {context: CONTEXT_REF, snapshot: SNAPSHOT_REF, mode: "replace"}
storage_forget: {snapshot: SNAPSHOT_REF}
```

Submit these as shared `act` actions with `action`, or individual
`browser_send` requests with `operation`. `storage_apply` requires a `mode`:

| Mode | Engines | Effect |
| --- | --- | --- |
| `replace` | All | As Playwright `setStorageState`: clears the cache and cookies, clears every origin the context visited and every snapshot origin, then restores the snapshot |
| `merge` | Chrome | Adds or overwrites the snapshot's cookies, local-storage keys, IndexedDB databases (each replaced by name) and OPFS files; nothing else changes. A database another page holds open is left unchanged and fails that origin |

On Chrome, the result reports each step: the cache, the cookies, and each origin's
status (`restored`, `cleared` or `merged`). Applying invalidates older input
observations and does not reload the app or prove account identity. If a restore
fails after something was sent, it settles as `OUTCOME_UNKNOWN` with the same
progress, steps that may have changed marked `unknown` and unsent steps
`pending`. Inspect or close the context before new work; Testrun never repeats
it. On Chrome, an unknown outcome also ends the browser session, as every
uncertain Chrome write does; disconnect and connect again.

A snapshot with virtual credentials applies only with `installAuthenticator:true`,
because it installs the virtual authenticator in place of real ones; otherwise
it refuses with `AUTHENTICATOR_INSTALL_REQUIRED`. `replace` with a snapshot that
has no credentials uninstalls a virtual authenticator, as Playwright does.
Creation `storageState` cannot carry credentials
(`VIRTUAL_CREDENTIALS_REQUIRE_APPLY`): create the context, then apply the
snapshot.

`storage_forget` removes only the owned snapshot file and reference. It refuses
to delete a changed artifact and does not clear the application's state. Snapshot
files use private evidence retention until explicit forget; disconnect does not
delete them. A publication failure retains its allocation with `published:false`
in `browser_status` with `includeDetails: true` for cleanup or recovery.

## Cache Storage on Chrome

`context_cache_storage_read` lists an origin's caches, a page of one cache's
entries with `cacheName`, or one entry's response with `cacheName` and `url`
(`bodyBase64`, at most 1 MiB). Caches with more than 1000 entries refuse an
entry listing; read their entries one at a time by `url`. `context_cache_storage_delete` deletes one cache, or one entry
with `url`. Both need a live page in the context; Firefox and WebKit refuse them.

```text
context_cache_storage_read: {context: CONTEXT_REF, origin: "https://example.com", cacheName: "acme-v1"}
context_cache_storage_delete: {context: CONTEXT_REF, origin: "https://example.com", cacheName: "acme-v1"}
```

## Wait for page events

`events_read` returns metadata-only history for an owned context. Save its
`latestSequence` before triggering an action, then submit `events_wait` with that
context, that sequence as `after` and 1..8 conditions. The wait also sees events that
arrived between the trigger and wait submission. For example:

```json
{"operation":"events_wait","context":{"connectionId":"CONNECTION-UUID","id":"CONTEXT-UUID","generation":"GENERATION-UUID"},"after":12,"conditions":[{"event":"dialog"},{"event":"page_created"}]}
```

Events are `page_created` (including popups), `page_closed`, `page_crashed`,
`dialog`, `file_chooser`, `download`, `frame_navigated`, `frame_attached` and
`frame_detached` (child frames; the resource is the frame), `load` and
`dom_content_loaded` (the page's main frame, as in Playwright), `worker_created`
(a dedicated worker of the page; the resource is the worker) and, on Chrome,
`service_worker_created`. A service worker belongs to the context, so its event
has `page: null` and only conditions without a `page` filter match it.
Conditions optionally filter an exact `page` reference. Each witness returns
its page and resource reference. A witness describes an event; the resource may already have closed or
expired, so subsequent operations still check liveness. A download event marks
allocation; inspect `download_status` for verified bytes and completion.

History retains 512 events per connection across contexts. `events_read` accepts
optional opaque `cursor` and `limit` (default 50, range 1..100). Follow
`nextCursor` until null, keeping the same context. Pages stop at 256 KiB of
serialized events. A cursor older than retained history returns
`invalid_cursor` and asks you to restart without a cursor; it does not prove
that an event never happened. `nextRequest` carries the exact continuation.
Waits use the operation's `timeoutMs`, remain cancellable and leave other reads
and callback responses available. They return the first match and
`conditionIndex`. Expiry keeps the last observed cursor without repeating the
trigger. History excludes URLs, dialog text, filenames and page content.

Conditions are alternatives: history order selects the first witness, then
condition order breaks a tie. A page close or crash ends that page's remaining
alternatives. A matching terminal witness succeeds; otherwise, when no
alternative remains eligible, the wait fails with `PAGE_CLOSED` or
`PAGE_CRASHED`. Conditions for another live page or without a page filter stay
eligible. A retained terminal witness can still match after page removal.
Allocation, close and crash acknowledgements share one retained witness per
page/event identity, including native close replies that precede destroy events.

Pending waits publish `waiting:true` and `nextCursor`. Cancellation, expiry
(`WAIT_EXPIRED`) and terminal-page failures retain `after`, `nextCursor` and
`nextAction:"events_read"`. Resume from that cursor without repeating the
trigger. Native waits consume new witnesses from their latest progress cursor;
they do not rematch already-consumed events on each poll. An unconsumed history
gap still fails with `EVENT_HISTORY_EXPIRED`. These are metadata-history waits.
Playwright's `on`/`once`/`off` listeners and `waitForEvent` predicates run caller
code, which a JSON request cannot carry: read the history or wait on typed
conditions instead, and filter the returned witnesses yourself.

## Wait for diagnostic or network events

Start diagnostics for an owned context and capture `latestSequence` before the
action that should cause an event. Submit `diagnostics_wait` with that `context`,
that sequence as `after`, and 1..8 typed `conditions`. It returns the first matching
event and `conditionIndex` without sending input. For example:

```json
{"event":"response","method":"POST","status":409,"url":"https://example.com/api"}
```

Events are `console`, `page_error`, `request`, `response`, `request_finished`,
`request_failed` or `web_socket`. Conditions may select an exact owned `page`,
`requestId` or `webSocketId`.
Network methods are bounded HTTP tokens; `status` applies to responses.
URL matching uses the exact redacted URL from diagnostic history, excluding
credentials, query and fragment. Ordinary wait results omit private message text.

The explicit cursor prevents old events from satisfying a new wait and permits
the wait to be submitted after the triggering action. Evicted history fails with
`DIAGNOSTIC_CURSOR_EXPIRED`; it cannot establish that an event was absent. Status,
independent reads and cancellation remain responsive. The operation's
`timeoutMs` bounds waiting; expiry retains `matched:false` and the observed cursor.
Popup/dialog/download event barriers remain separate work.

## Read-only live preview

External hosts acquire input through the separate shared `control_*` tools and
private authenticated channel described in [viewer control](viewer-control.md).
That channel is bound to an exact page and current viewport observation. Frame
access alone does not acquire input control.

Submit `{operation: "preview_start", page: REFERENCE}` to start a page-bound
JPEG source. Optional `options` accepts `quality` (1–100, default 75) and `fps`
(1–30, default 20). These are requested ceilings, not verified throughput or
quality guarantees. The result returns a `preview` reference and private viewer
metadata. Use `preview_status` and `preview_stop` with that reference; repeated
stop is idempotent. Only one active preview is admitted per page and eight per
browser connection.

A native host integration obtains a single-use grant through the private
broker and connects to `mediaUrl`. Browser origins are refused. Browser viewers
receive the existing binary frame packets with JPEG content and must acknowledge
each frame using `{type: "frame_ack", sequence: "SEQUENCE"}`. One frame may be
in flight; later queued frames are replaced by the latest frame. Private frame
bytes are not returned through ordinary CLI/MCP status or written as screenshots.

Input, reads and dedicated screenshots can continue during preview. The preview
is fixed to its page; selecting a different page does not switch its source.
Navigation clears the cached document and advances source/viewport identities.
`page_resize` takes `{page, viewport: {width, height}}` (each 1–8192); source
reconfiguration supports at most eight million pixels. A navigation/resize
on Firefox/WebKit reports `previewRecovery` separately from the completed page
mutation; native Chrome reports asynchronous recovery through `preview_status`.
Source recovery failure does not authorize repeating that mutation.

Status distinguishes actual encoded dimensions, independently measured CSS
geometry, driver-reported viewport, source timestamps and host arrival age.
Source timestamp calibration and performance acceptance remain unverified.
Verified frames include a CSS-to-pixel transform and a ten-second input observation.
The pinned Firefox source can add one pixel of right/bottom JPEG padding at odd
viewport sizes; the transform identifies that padding, which is outside CSS input
bounds. Unmapped, zoomed or pointer-locked frames omit coordinate authority.
An owner-connection heartbeat does not make an old static frame fresh. Browser
audio and masks on live frames are unavailable. Firefox headed
capture can mask extreme bottom corners with the native window shape; metadata
reports this visibility limit without claiming a measured content rectangle.

Use `page_snapshot` to retain a bounded semantic ARIA snapshot. If the
snapshot is too large, `SNAPSHOT_BOUNDS` returns a preview and guidance to
narrow the request with `locator`. `capture_compare`
takes two `captureId` UUIDs from this connection; paths and foreign captures are
refused. Image comparison reports pixels whose maximum RGBA channel difference
exceeds `threshold` (0–255), a changed ratio and a private PNG diff. `maxChangedRatio`
(0–1, default 0) controls `matches`. Snapshot comparison reports added/removed
lines and their ratio to the total lines, with at most 512 lines per side.
Comparison attempts share the capture budget and reserve up to 16 MiB each.

`page_annotate` accepts up to 32 locator/label `markers`; an empty list clears them.
Highlights follow their elements until replaced or the document ends. Labels are
plain text, and highlights do not redact content. `page_pdf` exports a bounded
private PDF on headless Chrome with explicit paper size, margins, page ranges,
backgrounds, header/footer templates, tagging and outline options. Inspect its
schema for bounds; Firefox/WebKit and headed Chrome refuse PDF export.

## Record a page

Submit `{operation: "recording_start", page: REFERENCE}`. Optional `options`
accepts `maxDurationMs` (100–600000, default 60000). Inspect or stop the returned
`recording` reference with `recording_status` or `recording_stop`. Stop is
idempotent; a caller timeout leaves finalization owned and available for retry.
There is one active recording per page, eight per connection, and 32 retained
recording records. No caller-selected output path is accepted.
Status is `initializing` until the encoder accepts a frame, then `recording`,
and `finalizing` after stop or the duration bound. For a time-critical journey,
read `recording_status` until it reports `recording` before sending input.

Recording consumes the same JPEG source independently of the viewer. Closing or
stopping a preview preserves active recording; a preview can attach again to that
source. A recording started without a preview creates no viewer. When its duration
ends, it finalizes automatically and releases a source with no other consumers.
Preview source options are reused; conflicting preview options are refused.

Testrun transcodes the JPEG feed into H.264 MP4 without audio. macOS uses
AVAssetWriter. Windows uses Media Foundation and requires
the Windows media platform and H.264 components. Check recording availability
with `testrun browser doctor --engine chrome`, or pass the engine you use. The
initial even-sized canvas stays fixed and fits resized content with black
letterboxing. Timing uses host frame
arrival from recording admission, and a declared final duplicate frame extends
static content to the requested stop time. Encoder startup does not move that
time origin. The admission frame stays owned through source reconfiguration;
source interruptions are tracked even when a latest-frame replacement hides a gap.
Source epochs, gaps and dropped frames make the result `partial`. Page/source
loss also attempts decodable partial finalization. A failed encoder is reported
as `failed`; an unfinished file is not published as complete evidence.

Completed and partial results contain a private artifact path, SHA-256, byte
count, manifest, geometry, timing/drop statistics and full-track decode evidence.
Publication requires the decoded frame count to equal admitted encoder frames.
Recordings stop at 256 MiB of observed in-progress data; this is a monitored bound,
not a hard disk quota. Live masks, calibrated capture timing and media performance
acceptance remain unavailable.

## Evaluate and inspect values

`evaluate` takes `scope` (a page, frame, dedicated worker or retained handle reference),
`expression`, optional `isFunction`, `argument` and `returnHandle`. Arguments
default to JavaScript `undefined`; explicit `{kind:"json",value:null}` remains
null. `returnHandle` defaults to false. For example, submit:

```json
{"operation":"evaluate","scope":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"expression":"value => ({title: document.title, label: value})","isFunction":true,"argument":{"kind":"json","value":"Acme"}}
```

Replace all reference fields with a complete reference returned by Testrun.
The result's `value` uses a tagged representation: `json` for ordinary scalar
values, `undefined`, special `number` values (`nan`, `positive_infinity`,
`negative_infinity`, `negative_zero`), `bigint`, `date`, `url`, `regexp`,
`error` and `typed_array`. Date arguments use a valid UTC timestamp in
`YYYY-MM-DDTHH:MM:SS.sssZ` form. Arrays use `{kind: "array", id, items}`; objects
use `{kind: "object", id, entries: [{key, value}]}`. A `{kind: "reference", id}`
points to a previously introduced array/object in the same value, including
itself. This preserves shared and cyclic references. `json` arguments can
contain ordinary JSON arrays/objects; their integers must be JavaScript-safe.

Raw `ArrayBuffer` results follow the pinned SDK's public object projection:
each occurrence becomes a separate empty `object`, without bytes or own
properties. This also applies to binding arguments and `handle_read`. Use a
`typed_array` result, for example `buffer => new Uint8Array(buffer)` on a retained
buffer handle, to read bytes. Generic evaluation of a detached buffer, including
one nested in an object, returns `undefined`; explicitly reading a detached
handle fails. Supplying `kind:"array_buffer"` is unsupported. Caller-side buffer
properties can be supplied as ordinary JSON, as with the SDK's input projection;
typed binding replies likewise use the projected ordinary object.

Set `isFunction:true` for function source, including regular/async method forms
such as `acme(value) { return value; }`. Normalization compiles the source without
invoking it; the caller function runs once and its promise is awaited.
`isFunction:false` evaluates an expression without calling a resulting function.
Use parentheses around an object literal when it should be an expression.
When `scope` is a handle, the function receives its held value first, followed
by `argument`. With a page or frame scope it receives only `argument`.
`returnHandle:true` retains primitive values as well as objects and elements.
The pinned Firefox driver refuses a raw BigInt result and the pinned WebKit
driver retains it as `undefined`. Typed BigInt arguments remain supported.
Bound evaluation may return another independent handle. Neither disposal of a
parent handle nor later mutation of the original variable replaces that held
reference. Nodes and primitive handles can also be arguments in another live
frame of the same owned page. Ordinary objects and functions require their
original execution world; cross-page and worker/frame mixing are refused with
`FOREIGN_HANDLE`. Native Chrome refuses node adoption between renderer sessions
with `HANDLE_ADOPTION_UNAVAILABLE`. A caller `AbortSignal` is unavailable; cancel
the operation instead, remembering that cancellation does not prove the
expression stopped. Playwright's `exposeFunction` and `exposeBinding` correspond
to [application callbacks](#receive-application-callbacks).

Adoption does not move a DOM node: its owner document and connected/detached
state remain unchanged. Duplicate references in one argument preserve identity.
The source handle must be live when arguments are prepared; stale handles never
invoke the caller. A result retained in the destination frame has that frame's
lifetime, so disposing the original source handle or subsequently replacing its
document does not dispose the acknowledged destination result. Native temporary
source/target objects have independent cleanup ownership; `argumentCleanupPending`
reports retained cleanup after a failure or unknown caller outcome. Cleanup never
repeats the expression. Close the owning context to release an uncertain group.

Use `selector_evaluate` for immediate `$eval`/`$$eval` selection in a page, frame
or retained element handle. It takes `scope` and `options` with `locator`,
`expression`, optional `isFunction`, typed `argument`, `all` and `strict`.
Single selection uses the first match by default; `strict:true` refuses multiple
matches. Missing single matches report `ELEMENT_NOT_FOUND` without invoking the
expression. `all:true` passes every match, including an empty array, and cannot
be combined with strict mode. Selection is immediate unless an attachment wait
is explicitly requested.

```json
{"operation":"selector_evaluate","scope":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"options":{"locator":{"kind":"css","selector":".acme-item"},"expression":"elements => elements.map(element => element.textContent)","isFunction":true,"all":true}}
```

The expression runs in the document's main world and awaits promises. A function
receives the selected element/array followed by the typed argument. An omitted
typed argument becomes JavaScript `undefined`; explicit JSON null stays null. An element
handle limits selection to its descendants; ordinary object handles and workers
are refused as selection roots. Arguments use the same node/primitive adoption
rules as `evaluate`. Snapshot `element`
locators, including nested ones, report `SELECTOR_REFERENCE_UNAVAILABLE`; use a
retained element handle as the scope or another locator form. Results use the
same bounded value representation as `evaluate`. Set `returnHandle:true` to
retain the result instead, including DOM elements, arrays and cyclic objects;
the existing handle read/property/evaluate/dispose operations apply. A returned
element can be the root of a later selector evaluation.

Set `waitForAttachment:true` for strict single-element selection before invoking
caller code once. Strictness defaults to true for waits; explicit `strict:false`
and `all:true` are refused. `waitTimeoutMs` accepts 1–300000 ms, defaults to 30000,
and is capped by the enclosing operation deadline. Supplying it without an
attachment wait is refused. Selection polls the current document at intervals
up to 50 ms. Missing selection expires as `SELECTOR_WAIT_EXPIRED`; ambiguity
reports `AMBIGUOUS_LOCATOR`, and a detached relative root reports
`SELECTOR_ROOT_DETACHED`. These settled selection failures do not invoke the
expression. The wait ends before caller evaluation starts; promises returned by
caller code retain the enclosing operation deadline.

Navigation/disposal fences the document and returned handles. Waiting does not
reattach to a replacement document. Transport loss or cancellation after dispatch
can leave an uncertain evaluation; it is never replayed. Caller AbortSignal and
function exposure are unavailable. These bounded forms do not establish every
Locator SDK option or cross-navigation behavior.

With `evaluate`, set `returnHandle: true` to retain its result. Pass it as
`{kind: "handle", handle: REFERENCE}` in another evaluation argument, inspect
its value with `handle_read`, or release it with `handle_dispose`. Arguments
use the same execution-world and cross-frame adoption rules above. Navigation
and disposal invalidate the original handle; an independently retained destination
result keeps its own frame lifetime.

`handle_owner_frame` and `handle_content_frame` take a retained element `handle`
and return a nullable `frame`, its nullable `page`, and the original `sourcePage`.
Owner-frame lookup includes Document nodes and follows DOM adoption into another
page in the same browser context. Content-frame lookup returns a frame only for
an iframe/frame element known to the originating page; ordinary elements and
Document nodes return null. A detached element can still have an owner document.
Non-element handles refuse with `INVALID_SCOPE`. Source-document replacement
fences an in-flight result; the operation never replays into a new document.
Owner lookup follows the SDK's document-property access and reports
`potentiallyMutating:true`; content lookup reports false. Native owner lookup
awaits the helper result and supplies the SDK evaluator's user gesture. Native Chrome uses
private temporary object groups, bounded to 32 retained relationship requests.
An unanswered call retains its group until attachment loss. A settled query can
return `relationshipCleanupPending:true` when group release remains pending;
housekeeping retries only the safe release. Neither query allocates a public
handle.

`frame_element` takes an owned child `frame` and returns a retained embedding
element `handle`, its parent `frame`, `page` and original `childFrame`. Main
frames refuse with `INVALID_SCOPE`. The handle belongs to the parent document:
replacing or removing the child does not retire it; parent-document replacement
does. Child or parent replacement during retrieval fences publication. Native
Chrome resolves the node in the parent realm and transfers one private group to
the public handle only after the lookup settles. Disposal releases that group;
unknown creation retains capacity until its native realm/session ends. It shares
the 256 retained/pending handle limit and 32 pending relationship-group bound.
The query does not invoke application getters.

A frame reference names one document: after its frame navigates, the old
reference refuses with `STALE_REFERENCE`. To keep a Playwright-style `Frame`
across navigation, submit `frame_retain` with an owned `frame`. It returns a
`frame` owner reference (`frame_owner` in status), the current `documentFrame`
and its `page`; retaining the same frame again returns the same owner. Use the
owner as the `scope` of `evaluate` and `selector_evaluate`, and as
`frame_element`'s `frame`: each request runs in the frame's
current document when it is prepared. A request already in flight keeps its
document fence and fails with `DOCUMENT_CHANGED` rather than moving to the new
document. Handles still belong to their document. The owner ends when the frame
detaches or its page or context closes. `frame_owner_release` with the owner as
`frame` ends it early; it does not close the frame (`nativeScopeClosed:false`).
Owners share the connection's 256-resource limit.

To check that page objects can be collected, submit
`{operation:"page_request_gc", page:PAGE}`. It asks the page's JavaScript engine
for a full garbage collection and returns `{page, requested:true}` once the
browser has run it; a following evaluation can check a `WeakRef` or
`FinalizationRegistry` result. It is the pinned SDK's `page.requestGC()`
(native Chrome uses `HeapProfiler.collectGarbage`). Status capabilities list it
under `evaluation.requestGC`.

Native Chrome also applies its existing safe-release queue high-water mark to
new retained results. With 256 releases queued, embedding lookup and retained
evaluation/property/selector requests refuse with `EXECUTION_CLEANUP_PENDING`
before their native creation or selector preparation. Existing handles remain
readable and disposable. Housekeeping retries releases; new creation becomes
available when cleanup makes room. Already admitted resources keep their owned
cleanup and are never dropped to make the queue appear empty.

Closing the exact owning page/context ends an unanswered relationship or
embedding-element wait locally, with `scopeClosed:true` and unknown native
completion preserved. It does not replay the query or claim its original
native reply arrived. Only the current operation's active private group can
supply this scope; unrelated calls retain their uncertainty fence.

An initial native `about:blank` popup can keep its exact live execution realm
through the first commit when its accessible opener has a witnessed live opaque
world in the same context. Document and world references rotate; earlier
references remain stale. The opener proof is consumed by that first navigation
event. Other commits and native destruction retain their existing fences.

Native Chrome's 256-handle limit includes retained references and pending
creations. `browser_status` with `includeDetails: true` exposes `handleCapacity` counts: `limit`,
`retained`, `pending` and `uncertain`. A creation with unknown native completion
retains its slot and reports `handleAllocationPending:true` in its error receipt.
Unknown reservations survive document/reference rotation and broad context-clear
notifications. Matching native context destruction, inspector-session detachment
or verified owned-browser teardown ends them. Receipts retain their settlement-time
observation; current capacity reports subsequent cleanup. Confirmed failure and
preparation failures before caller dispatch return the reserved slot. Ordinary
value evaluation remains available when handle capacity is full.
The native status schema declares all four fields; the limit is 256 and each
count is an integer from 0 to 256. Unrecognized counter fields remain refused.

An unknown native evaluation can leave the session ready only when that exact
operation still owns a bounded handle reservation or private argument/relationship
group, its native scope remains live and the connection is healthy. The original
receipt stays unknown; a later reply does not deliver a new result or authorize
replay. Another operation's owner cannot exempt a new timeout. Unowned unknown
calls and native connection loss retain the session fence and require disconnect.

`handle_properties` enumerates own enumerable string keys with optional
`query: {offset, limit}` (offset 0–10000; limit 1–100, default 32), an approximately
8 KiB result bound and `hasMore`/`nextOffset`. Enumeration is not a stable snapshot
across calls. `handle_property` takes `handle` and `name` (up to 1024 UTF-8 bytes)
and enumeration refuses keys beyond that bound. `offsetLimitReached: true` with
`nextOffset: null` reports when continuation would exceed the offset bound.
`handle_property` returns
an independently retained property handle. Disposing the parent
handle preserves that property handle; document navigation invalidates both.
Property lookup can invoke getters, and enumeration can invoke proxy traps, so
these operations are reported as potentially mutating.
They use ordinary operation capacity and control authority, but can dispatch
while another caller evaluation or input is waiting. Property-only/evaluation
batches keep their order; a batch mixed with other ordinary work keeps ordinary
ordering. A property lookup preserves a Promise or thenable as the property
handle; it does not await that value or invoke its `then` getter. Enumeration
returns names, rather than the SDK's map of property value handles.
Browser results and binding arguments copy own enumerable properties from ordinary
objects, including class instances and Map/Set objects. Map/Set entries are not
included; convert them explicitly or evaluate through a retained handle. Shared
object identity is preserved. Ordinary functions and symbols project to
`undefined`, including retained reads and nested properties. Throwing property
getters are omitted. An enumerable `toJSON` method projects to an empty object;
an otherwise empty object can use a nonenumerable or inherited `toJSON` result.
Throwing `toJSON` methods are ignored. These reads can execute application code
and keep the existing result and graph bounds. Function descriptions from reserved
SDK bridges cannot be supplied as callbacks. Prototype-changing `__proto__`
argument keys are refused.

Special browser objects and unconverted cross-realm builtins still require a
retained handle or explicit page-side conversion. This projection does not
register host callbacks or provide general structured-clone support.

Arguments and results each have a 1 MiB encoded bound and structural limits.
Evaluation can mutate the page or make network requests. A completed evaluation
whose result is refused still reports `dispatchCompleted: true`; inspect the
application before retrying. An expired wait or cancelled caller does not prove
the expression stopped. Arbitrary native host functions cannot be registered.

`worker_list` takes `page` and returns dedicated worker references.
`context_worker_list` takes `context`; Chrome also returns shared/service workers
owned by that context, with no invented creating-page association. Select an
application worker by its disclosed URL and scope; driver-reported relative or
payload URLs are redacted as non-network URLs. Engine component workers can
also appear. An available execution context does not prove that the application
script has finished starting. Evaluate the returned references with `evaluate`;
worker handles and page/frame handles cannot be mixed. Worker results identify
`worker` and have `frame: null`.

Chrome fences worker handles on actual worker/world loss. Shared/service-worker
handles survive a creating page closing while that worker remains live. Its
active context diagnostics include worker console/errors. Manually attached new
workers label setup history `new-worker-observed-during-subscription`. Console
and errors collected before a newly paused target's startup resume is acknowledged
use `owned-target-startup-pending`, including init-script messages from pages and
frames. Existing-target initial replay stays excluded. Firefox/WebKit's pinned driver exposes
dedicated workers through these operations. Their console events in active
context diagnostics identify the exact owned worker; text requires explicit
opt-in. Shared/service-worker inspection is unavailable there. Dedicated worker
references and handles survive unrelated frame document replacement. Actual
worker close/disposal invalidates them; a replacement worker at the same URL has
its own reference. Listing cannot revive a closed worker while native disposal
is pending. Frame document handles still expire when their document changes.

### Receive application callbacks

Use a binding when application JavaScript needs a reply from the controlling
agent. Submit `binding_add` with `scope` and a `name`, such as `acmeBridge`.
The returned `bindingId` owns that registration. All engines accept page or
context scopes. Page registrations cover that page's child frames, including
cross-process frames. Context registrations also cover future pages and popups.
Names cannot overlap between a context registration and a page in that context.
Bindings are available to registered init scripts before application startup.

Application code can then call `await globalThis.acmeBridge('Acme', 41)`.
The call creates a `binding` callback in `browser_status` with
`includeDetails: true`. Status omits argument values. Submit `binding_read` with
its `callback` reference to get the structured `arguments` and source `frame`.
Then call `callback_resolve` with either
`response: {kind: "binding_resolve", value: 42}` or
`response: {kind: "binding_reject"}`. Resolution resumes the application's
promise, including when an evaluation is waiting for it. Each callback can
settle once; document replacement invalidates callbacks from the old document.

For a typed success value, use `binding_resolve_value` with the same `value`
format as evaluation arguments. For example,
`{kind:"binding_resolve_value",value:{kind:"bigint",value:"9007199254740993"}}`
returns a JavaScript BigInt. `binding_reject_value` throws its typed value into
the application's promise, including `undefined`, special numbers, Errors and
cyclic graphs. Typed replies have the evaluation codec's 1 MiB bound and refuse
live handles and function values. The original `binding_resolve` accepts JSON;
`binding_reject` supplies the generic rejection used for expiry and closure.
Binding Error values prefix a stack missing its `name: message` header, matching
the pinned SDK; a stack already carrying that header is preserved.

Registrations persist across navigation. `binding_list` takes `scope` and shows
registration metadata. `binding_remove` takes `scope` and `bindingId`; resolve
outstanding callbacks first if it returns `BINDING_CALLS_PENDING`. Removal clears
the registered function without undoing application effects. A concurrent removal
returns `BINDING_REMOVAL_PENDING`; inspect the original operation before retrying.
Scope closure releases its registrations.
On Chrome, a known cleanup failure leaves future installation stopped and allows
cleanup retry. An uncertain target command returns `BINDING_CLEANUP_PENDING`;
inspect the original operation or close the owning scope before retrying cleanup.

Each connection allows 64 registrations. Names use 1–128 UTF-8 bytes, contain no
control characters and cannot begin with `__testrun`. Calls accept at most 64
arguments within a 1 MiB encoded read bound. Callbacks expire after 30 seconds and
reject their promises. A lost reply can leave registration or removal uncertain;
retain its `bindingId` and inspect the operation rather than repeating the add.

### Add script and style tags

`page_add_script_tag` and `page_add_style_tag` add a `<script>` or `<style>`
to the current document of a page or frame `scope`, as Playwright's
`addScriptTag`/`addStyleTag` do, and return its element `handle`. Pass one of:

- `url`: a script or stylesheet address; the request waits until it loads and
  fails if it does not.
- `path`: a regular UTF-8 file of at most 512 KiB, relative to the working
  directory of the CLI or MCP client that sends the request. Its text is sent as
  content with a `sourceURL` comment naming the path. A `path` replaces
  `content`.
- `content`: inline text, at most 512 KiB of UTF-8.

When the page's Content Security Policy blocks the tag, the request fails, as
the SDK rejects; the blocked element stays in the document. Chrome's error names
the violated directive (`Content-Security-Policy blocked the tag: …`); Firefox
and WebKit fail with `BROWSER_METHOD_FAILED`.

`page_add_script_tag` also takes an optional `type`, such as `module`. Unlike
startup code below, the tag affects only the current document. These refusals
happen before anything reaches the browser:

| Code | Cause |
| --- | --- |
| `INVALID_ARGUMENT` | No `url`, `path` or `content` |
| `TAG_PATH_INVALID` | An empty, over-long or NUL-containing `path` |
| `TAG_FILE_IO` | The file cannot be opened or read |
| `TAG_FILE_NOT_REGULAR` | The path is a directory or special file |
| `TAG_FILE_ENCODING` | The file is not UTF-8 |
| `TAG_CONTENT_BOUNDS` | Content or file bytes over 512 KiB, or content whose escaped request exceeds 960 KiB (text dense in quotes, backslashes or control characters) |

```json
{"operation":"page_add_style_tag","scope":PAGE,"content":"#banner{display:none}"}
```

### Register startup code

`page_init_script_add` takes `page` and JavaScript `source` (1–65536 UTF-8 bytes).
It returns a `scriptId`. The script runs in the main world of future documents,
before application scripts; registering it does not execute it in the current
document. Use `page_init_script_list` with that page to inspect registrations
without revealing source text, and `page_init_script_remove` with `page` and
`scriptId` to stop future execution. Removal does not undo existing effects.
Execution order among independent scripts is unspecified.

Use `context_init_script_add/list/remove` with `context` to apply the same
future-document contract to every page and child frame in that context,
including pages created later and popups. Page registrations also cover child
frames, including Chrome's separately attached cross-process frames. They do
not propagate to another page or context, or to workers.
The pinned WebKit popup path evaluates startup code during initial page adoption
and navigation. Its popup global can retain effects from both stages; do not
assume that a global counter starts at zero for the application document.

A connection retains up to 64 page/context registrations together. Closing their
owning page or context releases them. Chrome retains the installation IDs for
each attachment; lists report installation and pending-cleanup counts without
source text. An uncertain registration retains its slot and `scriptId`; do not
repeat it. Use the matching remove operation for confirmed installations. Close
the owning page/context when an identifier or removal outcome is uncertain;
`INIT_SCRIPT_CLEANUP_PENDING` names that required closure.
Firefox/WebKit lists an uncertain removal as `removing` and refuses another
disposal. If the original driver disposable later closes, retrying the remove
operation finalizes the local record without dispatching disposal again.

### Call page-provided developer tools

`developer_tools_discover` takes a page or frame `scope`. It dispatches the
application's `devtoolstooldiscovery` event and returns a document-bound
`registry` with tool groups, names, descriptions, input schemas and `toolId`
values. This runs application registration code; metadata is untrusted page
content. The registry captures each callback and a JSON copy of its schema.

Call a listed ID with `developer_tool_call`, its `registry` and a JSON-object
`parameters` value. AJV validates the parameters before invoking the callback.
Invalid parameters return `DEVELOPER_TOOL_ARGUMENTS_INVALID`; schemas requiring
unavailable references or extensions return `DEVELOPER_TOOL_SCHEMA_UNSUPPORTED`.
Schema URLs are not fetched. The validator is bundled into Testrun and does not
require Node for Chrome.

Pass a value declared with `x-mcp-type: HTMLElement` as
`{"elementHandle": reference}`, using a retained DOM result handle or a handle
returned by `evaluate`. Declarations can appear in nested objects, arrays and
local JSON Pointer schema references. Validation selects the applicable schema branches
before resolving handles; an ordinary JSON object with an `elementHandle` field
stays JSON when its schema does not declare an element. Top-level declarations
also appear in `elementParameters`.
Each call resolves at most 32 element paths, with 4 KiB of path metadata and a
traversal depth of 32. Branch validation retains at most 128 compiled branches
per tool. The element must belong to the registry's execution world and remain
connected to its document. Detached or foreign elements fail before the callback.
Other DOM types and asynchronous validation of element-bearing schemas are
unavailable; schema URLs are not fetched.

`returnHandle` defaults to true, preserving arbitrary results through the
existing handle operations. Inspect them with `handle_read`, `handle_property`
or scoped evaluation, then `handle_dispose`. Set `returnHandle:false` when the
result fits Testrun's structured-value representation. A callback can mutate
state before throwing or returning an unsupported value; do not invoke it again
just to recover its result.

Use `{operation:"developer_tool_result_read",handle:HANDLE}` for a private
projection of a DOM-rich result. It accepts an owned page/frame result handle,
including one returned by `evaluate`. The response contains structured `value`
and an `annotations` array. Elements become markers with an annotation `index`;
each annotation supplies its first graph `path` and a frame-bound `locator`.
Use the returned `page`, `frame` and annotation `locator` with `page_snapshot`,
`page_read` or page actions. The read does not invoke the original tool callback.

Projection preserves cycles and typed values, visits at most 4096 graph entries
to depth 32, and annotates at most 32 connected elements in the handle's document.
The encoded value has a 32 KiB bound and the final response a 64 KiB bound.
Detached or foreign elements are refused. Ordinary accessor properties become
markers without calling their getters; proxy traps can still execute application
code, so reads report `potentiallyMutating:true`. Results remain live and can
change between reads. Element locators follow the same document lifetime and
bounded weak-reference registry as snapshot locators.
`projectionCleanupPending` reports retained cleanup for native object transfers;
Testrun retries that cleanup without repeating the tool callback.

Chrome debugging WebMCP tools use the separate native interface below.

Use `developer_tools_release` to clear the registry and release its remote
handle. Repeated completed release preserves its witness. Document replacement
or frame/page closure fences the old registry and its result handles; discover
again explicitly for the new document. Discovery, invocation and registry
cleanup require agent control of the application.

A connection permits four registries, counting allocations still in flight,
with 32 tools each. Metadata is capped at 128 KiB and individual schemas at
8 KiB. Parameters accept at most 64 KiB, 2048 values and 32 nesting levels.
Metadata reports truncation and rejected definitions. A failed metadata read
retains its registry reference for cleanup through `developer_tools_release`.

### Inspect Chrome debugging WebMCP tools

`web_mcp_debug` takes a page or frame `scope` and an `action`. Enable collection
with `{"kind":"enable"}`; `{"kind":"status"}` lists that document's tools whose
WebMCP `debugging` annotation is true. Firefox/WebKit return
`NATIVE_WEB_MCP_UNSUPPORTED`. Each selected document needs its own enable call;
this subscription does not automatically include child frames or future documents.
Testrun's disposable Chrome launch enables the WebMCP browser feature.

Invoke a listed name with `{"kind":"invoke","name":"acme-debug","parameters":{}}`.
Chrome validates the input against the registered tool schema. The result contains
an `invocationId`; acknowledgement does not mean the application tool has finished.
Use `{"kind":"read","invocationId":"…"}` for its state and private output.
Status lists invocation states without outputs. Metadata and output are untrusted
application content, and even a debugging tool can mutate the application.
With the shared `act` verb, use `action:"web_mcp_debug"` and put the nested action
in `input`, for example `"input":{"action":"status"}`.

`{"kind":"cancel","invocationId":"…"}` requests cancellation. Read until the
invocation settles before disabling collection with `{"kind":"disable"}`.
Repeated completed disable succeeds. Event loss fences new invocations; an
acknowledged invocation remains cancelable. Event loss also requires closing the
owning page or connection before subscription removal, because a lost exception
event may have allocated an inspector object whose ID is unavailable.
After an interrupted dispatch, read its receipt while the acknowledgement is
pending. If it settles as `acknowledgement_unknown`, close the owning
connection.
Document replacement invalidates the
subscription and discards its outputs. Disable it and enable the new document
explicitly. Four subscriptions, 32 tools per document, 16 retained invocations
per subscription and 64 KiB per result bound collection. Disable and re-enable
after collecting the results to reclaim invocation capacity.
Registrations observed on an enabled session use a bounded document cache so a
later explicit frame subscription can discover existing tools. The cache permits
64 documents and 1 MiB of metadata. Exhaustion fences discovery and new calls;
remove the affected subscriptions before enabling collection again.
Status, result reads and cancellation remain responsive during pending page calls.

### Call ordinary Chrome WebMCP tools

Use `web_mcp` with an explicit page or frame `scope`. `enable` starts discovery
and `status` returns current ordinary registrations. Pass a listed `toolRef`
and an optional `parameters` object to `invoke`. Read the returned
`invocationId` with `read` and use `cancel` to request cancellation if needed.
The `read` result distinguishes the invocation state from the application's
output, which can itself contain an `isError` value.

Rediscover after navigation or a changed registration. A tool reference only
applies to its current registration in the selected document. Use `disable`
to release collection. Chrome reports existing registrations in the DevTools
target's root document when collection starts. For an in-process child frame,
enable collection before the application's registration or ask the fixture to
register its tools again. The same result bounds and recovery steps as debugging
WebMCP apply. With shared `act`, use `action:"web_mcp"` and the nested action
name in `input.action`.

### Inspect React applications

Call `framework_prepare` with a `page` before navigating to the application. It
registers the bundled React DevTools hook for future documents and returns a
`scriptId`; it does not restart or instrument an already running application.
Remove the registration with `page_init_script_remove`. Existing effects last
until the document closes. Preparation includes future child frames, including
separately attached Chrome frames. For all pages and frames in one context,
call `context_framework_prepare` with `context` before creating/navigating the
application pages. Remove that returned `scriptId` with
`context_init_script_remove`. Registries opened afterward still belong to an
exact page/frame document.

After React starts, call `framework_open` with a page or frame `scope`. The
returned `registry` and renderer list belong to that document. Use
`framework_query` with that registry and a `query`:

- `tree`: component IDs, names, keys and parents, with `offset` and `limit`.
- `inspect`: a component `id`, including the renderer's props/hooks/state data.
- `suspense`: boundary state, environment and reported blocker/owner details.
  `onlyDynamic` filters each bounded page to suspended boundaries or those with
  reported blockers. Follow `nextOffset` while `hasMore` is true, including after
  an empty filtered page.
- `renders_start`, `renders_status`, `renders_stop`: the selected React
  renderer's native DevTools profile. `durationMs` defaults to 30000 and cannot
  exceed 60000; expiry stops collection and retains the report for retrieval.

For `renders_start`, `recordChangeDescriptions` defaults to true and
`recordTimeline` defaults to false. Explicit false values are preserved. The
first controls per-commit component change descriptions; the second requests
native React timeline measures when the renderer and build support them.
Start, status and stop reports include the applied `options`. These two options
are refused on other query kinds. Existing result bounds and truncation apply.

Set `rendererId` when selecting among renderers. The default prefers a mounted
DOM renderer over a React Flight renderer. Profiling one renderer through two
Testrun registries is refused; coordinate other DevTools profilers separately.
Repeated `renders_stop` reads the retained report without collecting again.
If collection stops but reading its data fails, status reports `phase:"stopped"`,
`recording:false` and `cleanupPending:true`. Retry `renders_stop` to read the
same stopped profile without stopping again. Its owner blocks another registry
from overwriting that renderer's data until retrieval or explicit release.
If start throws after it may have changed the renderer, status reports
`phase:"start-uncertain"` and `recording:null`; release that registry to clean up
before starting another profile. Failed stops retain their owner for explicit
retry and cancel the automatic expiry attempt.
Reports have a 64 KiB bound and disclose truncation; inspection limits nested
values and preserves accessor markers instead of calling application getters.
Renderer support and development/profiling-build requirements still apply.

Use `framework_release` to stop any active profile and release the registry.
Release discards an unread profile; it does not collect data just to dispose it.
An acknowledged stop is not repeated when releasing a pending data read.
Generic `handle_dispose` refuses a framework registry so it cannot discard an
active profiler's cleanup owner, including while disposal uses the responsive
lane. Use `framework_release` for owned cleanup; repeating a completed release
returns its receipt without invoking the registry again.
Release retains failures for retry and preserves its completed witness. It
does not remove the hook's init-script registration. Navigation and document
closure invalidate old registries and component IDs. Framework operations run
application-side code and require current agent control.

## Read browser diagnostics

Browser lists and connected-browser status use `cursor`/`limit` (50/100) and
nullable `nextCursor`. Status pages only inspect the selected connections.
Offline pages contain last-known records with `live:false`, bound to that
saved snapshot; they do not prove cleanup. See [pagination](pagination.md).

Start collection before the journey with
`{operation: "diagnostics_start", context: REFERENCE}`. Read it with
`{operation: "diagnostics_read", context: REFERENCE, query: {limit: 1}}` for a
single event, then add `query.cursor` for the next page.
Use `nextCursor` for the next read, or submit `nextRequest`. Omit `cursor` to start at the oldest available
record. Optional filters are `page`, `worker`, `kind`, `levels`, `resourceTypes`,
`requestId` and `webSocketId`; `limit` defaults to
50 and accepts 1–100. Pages stop at 7,000 bytes of serialized events. `nextCursor`
is null at the current end. Filters must remain unchanged between pages.

For example, `query:{kind:"console",levels:["error","warning"]}` selects console
levels, and `query:{resourceTypes:["fetch","xhr"]}` selects network resource
types. These selectors match reported metadata without case sensitivity; an empty
array selects all. Each array accepts at most 32 ASCII names of up to 64 letters,
digits, underscores or hyphens. A `worker` filter takes an exact live worker
reference from the same context. It never infers a worker from its URL or name.

To inspect one record, use `query:{sequence:SEQUENCE}` with the sequence returned
in history. Omit `cursor` for this form. Scope and other filters still apply;
`DIAGNOSTIC_EVENT_NOT_FOUND` means no matching record exists, while an evicted
sequence returns `invalid_cursor`. Exact lookup never substitutes a
later record and uses the same private-text/stack controls as paged reads.

Reads also accept an exact reported `method`, an HTTP `status` (100–599) and an
exact redacted network/location `url`. Method matching preserves case. To select
the last observed POST/409 response, use
`query:{page:PAGE,kind:"response",method:"POST",status:409,latest:true}`.
This returns at most one record and advances `nextCursor` to the checked window
end, including when nothing matches. HTTP observations require separate
application-outcome verification.

Use inclusive `through:END_SEQUENCE` to close a step's observed range and follow
opaque cursors with unchanged filters. Omitted `through` uses the latest
sequence. Forward pages and `latest` respect that end; a future or evicted
cursor returns `invalid_cursor`. Exact `sequence` lookup is separate from
`through` and latest selection. These windows describe observations, without
attributing causality to an action that happened nearby.
The result reports its `observedRange`; explicit cursor bounds also constrain
correlation candidates.

Kinds are `console`, `page_error`, `request`, `response`, `request_finished`,
`request_failed` and `web_socket`. Network records share a request UUID; response records
include HTTP status. Lifecycle correlation uses that exact identity. Requests first seen after
their initial event are marked `initialEventMissing`.

Add `query:{sequence:SEQUENCE,correlation:{windowMs:5000,limit:4}}` to inspect
request candidates for one console or page-error record. It also works on paged
reads. The observation window accepts 1–60000 ms and includes records before or
after the selected event; candidate limits accept 1–4. Candidates stay within
the owned context and known page/worker scope. Failed requests and HTTP 4xx/5xx
responses are grouped by request UUID, preferring matching redacted URLs.
`confidence:"candidate-only"` never assigns an exact request, even when only one
candidate remains. `ambiguous`, `candidateCount` and `truncated` preserve multiple
possibilities. Candidate URLs clip at 256 UTF-8 bytes and report `urlTruncated`.
`historyLossObserved` identifies earlier loss or eviction in this bounded history.
Correlation details contain no bodies, headers or console text.

Chrome also collects browser-generated network console entries through its
native log channel. Initial log replay is excluded for existing targets. Such
records identify `source:"network"`; a retained native request ID and exact
original URL can supply `requestIdentity` and `confidence:"source-identity"`.
Otherwise they use the same unproven candidate view. `causalProof:false` applies
to both forms: matching a request does not prove why the application failed.

To label a known fixture failure without hiding it, add `query.ignoreRules`,
for example `[{kind:"response",requestId:REQUEST_ID,status:409,reason:"Expected fixture conflict"}]`.
Each of at most four rules requires a kind, a reason of 1–256 UTF-8 bytes
without control characters, and an exact request ID, WebSocket ID or redacted URL.
Optional page, worker and status fields narrow its match. Matching events keep
their original kind and failure state and gain `ignored:true` plus rule reasons
and scopes. Rules apply only to that read; resupply them in subsequent reports.
They do not change stored history, event waits or correlation candidates.

Default reads contain metadata. Set `includeText: true` to include bounded
console/error/failure messages, which may contain private application data.
Set `includeStack:true` independently to read bounded private stacks. Chrome
console/errors return CDP call frames; Firefox/WebKit errors return engine stack
text, and their console events have no stack. `stackAvailable`, `stackIncluded`
and the stack's `truncated` flag identify what was supplied. Function names and
raw stack text can reveal private application data even with `includeText:false`.
Stacks are limited to 2048 encoded bytes; Chrome retains at most 16 frames and
four async-parent levels without fetching source text or remote parent stacks.
Headers, cookies and request/response bodies are omitted;
Metadata URLs omit credentials, query strings and fragments. URL paths can still contain
private application data. Console argument objects are released by default.

Diagnostic history covers the pinned SDK's `Page.consoleMessages()`,
`pageErrors()` and `requests()`, with these differences. History is append-only
and read by cursor: there is no `clearConsoleMessages` or `clearPageErrors`;
start a new read from `latestSequence` instead. Events are records to read or
wait for, not listener callbacks run inside Testrun. Callers' source locations
come from the stacks above rather than automatic caller-stack discovery. Raw
Chrome DevTools Protocol sessions (`CDPSession`) are not exposed; use the named
native investigation operations.

To inspect live console values, start diagnostics, then send
`{operation:"diagnostics_arguments_set", context:REFERENCE, enabled:true}`.
Subsequent console records include `data.arguments.groupId` when retention
succeeds. `diagnostics_arguments_list` takes `context` and lists metadata without
reading private values. Retention holds at most 32 groups per connection and
16 arguments per message; metadata identifies truncation or unavailable values.

Read one argument with
`{operation:"diagnostics_arguments_read", context:REFERENCE, groupId:UUID,
query:{index:0, path:["customer","name"]}}`. The result uses structured
evaluation values, bounded to 32 KiB. Omit `path` to read the argument itself.
Add `properties:{offset:0,limit:32}` to enumerate own enumerable string names;
the limit accepts 1–100. Paths accept at most 16 keys and 8192 total key bytes.
Values remain live and can change after logging. Path reads and serialization
can execute application getters or proxy traps, and report
`potentiallyMutating:true`; failed reads are not automatically replayed.

Use `diagnostics_arguments_remove` with `context` and `groupId` to release a
group, or set `enabled:false` to release that context's groups. Failed cleanup
keeps its owner and capacity for retry. `diagnostics_stop` preserves retained
groups and their opt-in setting. Document or worker loss expires affected
groups. Chrome uses the exact execution world; Firefox/WebKit conservatively
expire page groups when any owned frame's document changes. An uncertain Chrome
read loses the connection's authority; disconnect it to finish owned cleanup.

History holds up to 8,192 records and 16 MiB per browser connection across contexts;
request lookup retains up to 4,096 identities. Chrome's upstream event queue also
holds up to 8,192 events and 16 MiB, so bursts can be collected before a read.
Status reports counts and cursors, not message content. Reads report evicted records and forgotten
request identities; an expired cursor or sequence returns `invalid_cursor`.
An oversized requested record returns bounded matching fields with
`detailsTruncatedForRead:true` and advances the cursor. This read projection does
not change the record's retained history or imply source-event loss.
`diagnostics_stop` stops collection for that context and preserves bounded history.
Chrome transport loss while no diagnostic context is active does not increment
the capture's `dropped` count or expire stopped history. During active collection,
source loss fences earlier cursors and clears request correlation; reads and
status report that loss. Retention limits still apply to long or large captures.
Closing a context ends its subscription. WebSocket observation is reported as
`web_socket` events with `webSocketId`, phase, direction, opcode, payload encoding
and encoded byte count. Filter reads or waits with `webSocketId`. Payload prefixes
follow `includeText`; `textTruncated` marks incomplete text or base64. These
events observe traffic and do not intercept it. Forgotten socket identities are
reported separately. HAR callback replay is described below; native Chrome
tracing is described under investigation evidence.

`web_socket_route_add` takes a Page or Context `scope` and one exact ASCII `ws://`
or `wss://` URL including a literal query, without fragment, credentials or wildcard. It intercepts
new page/frame sockets in current and future documents. Existing sockets and
worker sockets are outside this scope, as in the Playwright SDK: a worker's
sockets connect normally. A matching Page route takes precedence
over its Context route. The result includes `routeId` and `bindingId`.

For a typed matcher, use `web_socket_route_add_matcher` with `scope` and
`matcher: {kind:"glob",pattern:"wss://example.com/**/socket?token=*"}` or
`{kind:"exact",url:"wss://example.com/socket?token=Acme"}`. Globs use the same
ASCII star/brace/escape rules as HTTP interception, with an absolute WS(S) prefix
or leading `**`. Query marks are literal; `*` also excludes slashes inside query
values. Exact URLs normalize host case and default ports. Unmatched sockets
connect normally without a binding callback. A connection supports up to 32
routes, including multiple routes in one scope. The newest matching Page route
wins; if none matches, the newest matching Context route wins. Listing returns
newest-first order. Selection uses registration order even when init scripts run
in a different order in a future document.

For regular expressions, pass a matcher such as
`{kind:"regex",pattern:"^wss://example[.]com/socket[?]token=[a-z]+$",flags:"i"}`.
It matches the canonical WS(S) URL, including its query. Any JavaScript
`RegExp` source and flags work, including lookaround, backreferences, Unicode
property escapes and the `v` flag; `g` and `y` start every test at the URL
start. Omitting `i` is case-sensitive. Patterns have a 2048-byte limit and
invalid syntax refuses with `WEBSOCKET_REGEX_INVALID` before registration.

Testrun runs a pattern on a bounded linear automaton when it can (at most 512
states and 16 KiB of tables); otherwise the page tests it with the browser's
own `RegExp`, as the Playwright SDK tests it in its driver. That native test
has no time limit, so a catastrophically backtracking pattern can stall the
page's `new WebSocket()` call. Canonical URLs longer than 8192 bytes refuse
socket construction with `WEBSOCKET_URL_BOUNDS` while a regex policy is
consulted.

URL patterns work as for HTTP routes, for example
`{kind:"url_pattern",hostname:"*.example.com",pathname:"/socket",search:"token=:value"}`.
The page parses each new socket's URL and tests every component against its
own compiled automaton; components you omit are not checked. All of a
pattern's components share the 16 KiB automaton limit, and a pattern Testrun
cannot compile refuses with `WEBSOCKET_REGEX_UNSUPPORTED`. Function predicates
are unavailable, as for HTTP routes.

Each live socket keeps the route selected when it was created. Adding a newer
route affects future sockets. Removing a route closes only its selected sockets
and preserves sibling routes and their sockets; future sockets can select an
older matching sibling. There is no callback-time handoff to another route.
If setup or removal is unfinished in the same scope, another registration
refuses with `WEBSOCKET_ROUTE_NOT_READY`. Inspect `web_socket_route_list` and
remove the retained route by its original `routeId` to finish cleanup.

Routes behave like the Playwright SDK by default (`sdkDefaults` omitted or
true); see SDK default behavior below. With `sdkDefaults:false`, routes use
command mode as described next.
Socket creation and messages arrive as managed binding callbacks. Use
`binding_read` to read the private event argument (`type`, `id`, `url`/`protocols`
for creation, or `data: {data, isBase64}` for messages). Resolve with
`response: {kind: "binding_resolve", value: [...]}`, where the array contains
at most eight commands. Each command takes `type`: `connect` opens the original
server, `ensureOpened` opens a mock socket, `sendToPage`/`sendToServer` takes
`data: {data, isBase64}`, and `closePage`/`closeServer` takes optional `code` and
`reason`. The command always addresses the socket that produced that callback.
For example, reply to creation with `[{"type":"ensureOpened"}]` to mock it,
then reply to a page message with
`[{"type":"sendToPage","data":{"data":"Acme reply","isBase64":false}}]`.
An empty array drops a message. To forward subsequent messages in one direction,
reply with `{type:"setForwarding",from:"page",enabled:true}` or
`{type:"setForwarding",from:"server",enabled:true}`. Both directions default to
false; `enabled:false` restores callbacks for that direction. Page-to-server
forwarding requires `connect`, and messages retain the 512 KiB and connecting-queue
bounds. Forwarded messages do not create binding callbacks. For a relay, reply to creation with both forwarding
commands followed by `{type:"connect"}`. Every command addresses that callback's
socket, and the whole reply is validated before any command changes it.

Creation's private `protocols` array preserves the constructor's offered order.
Intercepted sockets accept at most 16 unique, nonempty HTTP protocol tokens of
128 ASCII bytes each. Invalid or duplicate tokens throw `SyntaxError` before a
socket or callback is allocated; count/type/length bounds throw `RangeError`.
The offers are copied at construction, so changing the application's array
cannot change a later relay handshake. `connect` sends those offers unchanged
and the server's selected protocol becomes `socket.protocol`. With
`ensureOpened`, the pinned mock chooses the first offer, or an empty string when
none were offered. Testrun does not provide a command to override that selection.

`closePage` accepts standard codes 1000–1003 and 1007–1014, or application codes
3000–4999. For example, reply to unsupported protocol offers with
`[{type:"closePage",code:1002,reason:"Unsupported protocol"}]`.
`closeServer` uses the browser's native close API and accepts only 1000 or
3000–4999. Both commands bound the UTF-8 reason to 123 bytes. Closing a mock
without a server attachment releases the socket owner and stops later commands
in the same reply; it cannot subsequently connect to a server.

Omitting a close code preserves omission. A `closePage` command without a code
produces the pinned mock's close event with code 0. A `closeServer` command
without a code and with an empty reason sends no status in the native close
frame; an empty peer acknowledgement is reported as 1005. Specify `code:1000`
when you intend that
code. A nonempty reason without a server close code uses 1000 so the reason can
be encoded in the close frame. Codes 0 and 1005 cannot be supplied as explicit
close-command codes.

`connect` is single-use for a socket, including after its server closes. The
helper validates connection/send state for the whole reply before dispatching
its first command. A duplicate connection or a send to a closed endpoint makes
the reply invalid and closes the owned socket without publishing earlier
messages. A `closeServer` command immediately fences server-bound sends while
retaining ownership until the native close event. It leaves the page side
available for `sendToPage` and later `closePage`; duplicate server-close commands
do not repeat the native close. `web_socket_route_send` reports
`WEBSOCKET_SERVER_NOT_CONNECTED` before connection and `WEBSOCKET_SOCKET_CLOSED`
while server close is pending or acknowledged.

Close forwarding is independent. Reply with
`{type:"setCloseForwarding",from:"page",enabled:true}` and/or
`{type:"setCloseForwarding",from:"server",enabled:true}` to forward closure
without a binding callback in that direction; both default to false.
`enabled:false` restores close callbacks. Forwarding preserves the close code,
reason and clean/unclean status. A page's `close()` enters `CLOSING` until the
server acknowledgement closes the page side; enable both directions for an
automatic handshake. With manual handling, reply to `onClosePage` with
`closeServer` and to `onCloseServer` with `closePage`. Testrun keeps the socket
owned until the page closure is acknowledged and the server has closed.
In command mode, acknowledging the page closure does not create another close callback.

SDK default behavior applies unless a route was added with `sdkDefaults:false`.
Creation still produces one binding callback. Reply with `[{type:"connect"}]` to relay;
an empty array or `null` without a connection automatically opens a mock. Without
message handlers, connected sockets forward both directions; an unconnected
mock drops page messages. Without close handlers, closure forwards independently
in both directions. A page-side close acknowledgement also invokes an installed
page close handler, preserving its own code and reason.
Automatic forwarding to an already closing/closed endpoint drops the message
and preserves the surviving endpoint. Explicit send requests still report
closed-endpoint refusal.

Use `web_socket_route_handlers` to install, replace or remove handlers at any
time, including while a callback or evaluation is pending. Supply the same
`scope`, `routeId`, private `socketId` and live `frame` used for unsolicited sends,
plus a nonempty `handlers` object. Its keys are `pageMessage`, `serverMessage`,
`pageClose` and `serverClose`. Each value is a caller-owned UUID identifying that
handler, or `null` to remove it; omitted keys keep their current handler. For
example, `handlers:{pageMessage:"00000000-0000-0000-0000-000000000001"}` intercepts
page messages. Private `binding_read` results include the event's `listenerId`.
Replacement affects later arrivals; an already delivered callback keeps its
original identity. Removing a handler restores default forwarding for subsequent
events. A close handler owns propagation, so returning `null` leaves the opposite
endpoint alone. SDK mode uses this operation for handler ownership; forwarding
flag commands are refused rather than competing with it. Command-mode routes
refuse handler updates with `WEBSOCKET_SDK_MODE_REQUIRED`.

Shared `act` batches containing only socket sends, handler updates and endpoint
closes run in order while ordinary input or evaluation is held. They use normal
operation capacity and controller authority; mixed batches retain ordinary
ordering. A batch stops at its first failure and never replays completed steps.

Use `web_socket_route_close` outside a callback to close either endpoint. Supply
the same exact socket/frame/route ownership, `to:"page"` or `"server"`, and
optional `options:{code:3001,reason:"Acme close"}`. Omitted options preserve the
close defaults above; present code/reason values cannot be `null`. The request
uses the endpoint-specific code and UTF-8 reason bounds. Its `closeRequested`
receipt acknowledges dispatch; close events establish endpoint completion.
It works in both route modes. Handler updates and close requests refuse foreign
frames, another route's socket, unfinished removal and expired documents, and
never replay uncertain mutations. Socket bounds, worker restrictions and the
asynchronous JSON callback workflow still apply; these options do not claim full
SDK event or callback parity.

For a direct close, omitting `options` and supplying `{}` have the same effect.
A page close without a code produces close-event code 0 in the pinned mock;
closing the server without code or reason sends no wire status, reported as
1005 on acknowledgement. A nonempty reason without a code uses server status
1000 so the reason reaches the peer on every engine. The Playwright SDK behaves
differently per engine here: Chrome refuses its native call, Firefox drops the
reason and WebKit sends a frame the peer rejects. An explicit empty reason sends
an empty close in Testrun, while the SDK's Chrome call refuses. Testrun keeps its
consistent behavior.

Removing `pageClose` or `serverClose` restores default propagation for later
arrivals in that direction. An already arrived callback keeps its original
handler identity and ownership through resolution, even after removal. Removing
that handler does not replay the earlier close through the restored default.

Use `web_socket_route_send` outside a callback reply. Supply the route's `scope`
and `routeId`, the `frame` returned by `binding_read`, the private event's `id` as
`socketId`, `to: "page"` or `"server"`, and
`message: {data: "Acme push", isBase64: false}`. Evaluation results also identify
their frame. Sending to the server requires an earlier `connect` command. The send
uses the same 512 KiB and queue bounds, can run while evaluation is waiting, and
refuses foreign frames, another route's socket and expired documents. Its result
acknowledges dispatch without echoing the payload. An uncertain write is not
replayed.

Messages allow up to 512 KiB of decoded data and 768 KiB of JSON-escaped text.
An application `send` beyond these bounds throws `RangeError`, and a larger
message from the server closes both sides with code 3009. At most 32 messages
and 2 MiB wait for a connecting server; callback queues are bounded.
Rejected, expired or invalid replies close both sides. The helper admits at most
256 intercepted sockets over a document realm's lifetime. `web_socket_route_list`
reports registration state without payloads. `web_socket_route_remove` takes
`scope` and `routeId`, removes future installation, closes owned live sockets and
releases the binding. Pending callbacks or uncertain setup can retain cleanup;
use the original route ID to retry, or close the owning scope.

When a socket's document navigates, its frame detaches, or its page closes or
crashes, the socket's state ends with that document. Its close handlers receive
no callbacks, unlike the Playwright SDK, which reports both endpoints closed;
later send, close and handler requests for it refuse with
`WEBSOCKET_SOCKET_EXPIRED`. `web_socket_route_list` and `web_socket_route_remove`
keep working for the route.

Chrome collects from owned page/frame/worker attachments through native CDP.
Redirect response and completion records preserve the predecessor's UUID; the
next request receives a new UUID. `worker` identifies an indexed worker when
available; service-worker traffic coverage is not complete. A CDP source gap
invalidates earlier explicit cursors and forgets request correlations, so a
wait cannot claim complete history across dropped events. Chrome stops local
collection without disabling the Network domain used by navigation. Console
and exception remote objects in the inspector's console group are released on
the control lane. Engine console history keeps its own bounded retention.
Console text follows the pinned backend's output: the Firefox diagnostic path
reports a `console.count()` label without its numeric count. Testrun does not
invent the missing number.
If `subscriptionPending` is true, setup or stop has not completed; retry the
explicit start/stop request or close the context. A partial subscription is not
complete event coverage.

### Intercept one request URL

`page_route_set` takes `page` and `url`: an exact ASCII HTTP(S) URL, including
its query, without fragment, credentials or wildcard characters. Each call
replaces that page's previous pattern. Pass `url: null` to disable interception. All engines
include child frames, including Chrome's separately attached cross-process frames.
Use `context_route_set` with `context` and the same options to cover existing and
future pages, popups and frames in that context. A matching page policy takes
precedence over its context policy. Each scope has one pattern; disabling one
scope preserves the other. Native Chrome context manual policies also cover
service-worker requests; page policies do not. Firefox/WebKit retain the pinned
SDK's absence of service-worker route callbacks.

Chrome worker callbacks identify `worker` with `page: null` and keep the context
policy owner after the creating page closes. Discover the live service worker
with `context_worker_list`. Context handler replacement, removal and fallback
keep their normal ordering; pending worker callbacks block policy edits and
context credential changes. Clear with `behavior: "wait"` to let the original
callback settle. Worker detachment or context closure retires its callback
authority; later replies refuse rather than dispatching to another attachment.
Native discovery labels this scope `context-manual-and-har-routes` and reports
`serviceWorkerHar: true`. Context HAR policies cover existing and future service
workers: archive hits reply on the worker's original attachment, abort misses
fail the request, and fallback misses continue to the next context handler or
the network. Ordered `network_route.add_har` handlers keep newest-first order
with manual handlers after the creating page closes. Logical filter nonmatches
continue privately. Page HAR policies grant no worker authority. Removing a
context policy preserves archive ownership until its native teardown succeeds;
pending callbacks keep their existing edit guards. Native service-worker
requests also inherit context headers and credentials after creating-page
closure. Context updates/removal apply to existing and future workers; explicit
manual authentication callbacks keep precedence over automatic credentials.
Other inherited HTTP settings and HAR capture have separate coverage limits.

Callback, route-policy and page/context-close shared `act` batches can release an
in-flight evaluation. They preserve action order, normal operation capacity and
controller ownership. Batches containing ordinary actions keep ordinary ordering. Native policy edits
can inspect a held callback and refuse it; explicit clear-wait serves its reply
before removing the policy. Acknowledged page/context closure removes retired
route callbacks before returning its receipt.

If an acknowledged page or context close ends an awaiting caller evaluation, its
operation settles with `STALE_REFERENCE`, outcome `unknown`, `scopeClosed: true`
and `callerEffectsUnknown: true`. Caller effects may already have occurred.
Page-close evidence includes its exact `page`; context-owned service/shared
workers and sibling pages remain usable. After acknowledged closure, the transport returns active command
capacity and retains only exact unconfirmed reply identities. A late reply
still must match its original request ID, session and method's result schema;
it cannot deliver a closed allocation or retain caller payload. The request is
never replayed. Page retirement selects only its page/frame/dedicated-worker
attachments; context retirement includes context-owned workers and its private
HTTP-state collector. Other live contexts remain usable. An unacknowledged close
leaves the original calls owned and active.

Native transport keeps 56 ordinary slots and eight reserved control slots.
Up to 4096 unreplied closed-scope identities are retained separately. If a close
would exceed that retirement budget, `NATIVE_RETIREMENT_CAPACITY` refuses before
native disposal; disconnect the owned browser before closing more affected
pages or contexts. Closing a scope with no affected calls still works. Unknown IDs,
conflicting sessions, malformed results and duplicate replies remain protocol
failures. A known original browser error may omit its session. Other attachment-
lifetime variants and full pinned close options/events remain separate contracts.

Use `network_route` with a Page or Context `scope` and
`action: {kind:"add",url:URL}` to register ordered handlers. It returns a
`handlerId`. One scope supports up to 32 exact-URL or glob handlers;
the connection supports 256 handlers. Newest handlers run first, followed by
matching context handlers after the page chain. Each callback identifies its
`handlerId`. `route_fallback` advances the chain; terminal continue, abort or
fulfill skips the remaining handlers. URL overrides do not change matching, and
handoffs preserve the original request and expiry.

Add `times` to `add` or `add_matcher` to limit that registration's invocations
(0–4294967295). Omitted or null is unlimited; zero, like one, permits one call,
following the pinned SDK. Matching a request does not consume a use until that
handler is selected. Terminal replies skip older candidates without charging
them, and fallback skips a limited candidate already exhausted by another
request. Callback expiry or an invalid reply does not refund an invocation.
Listings expose `remainingUses` (null for unlimited) and `exhausted`.
Repeated driver route events preserve the existing selected handler, budget and
original callback expiry.
`network_route` with `action:{kind:"list"}` remains available while evaluation
or another application call is waiting on a callback.

An exhausted handler stops matching new requests immediately. Once its scope's
pending requests and fallback dependencies settle, Testrun removes it through
the acknowledged pattern-update path, reclaiming registration capacity. A
successful callback reply can include `handlerCleanupPending` if that update
fails; its original reply remains resolved. Inspect the scope and follow the
reported `network_route` clear action, or close the owned scope. Automatic
cleanup does not repeat an uncertain pattern update.

For URL globs, use `action: {kind:"add_matcher",matcher:{kind:"glob",
pattern:"**/api/{orders,items}?version=*"}}`. Patterns accept 1–2048 ASCII bytes,
begin with `http://`, `https://` or `**`, and select HTTP(S) requests only.
`*` excludes `/`, including inside a query; `**` crosses path boundaries.
Braces select nonnested alternatives, backslash escapes a literal, and `?` is
literal. An absolute `/**/` segment also matches zero directories.
Absolute patterns normalize literal URL segments, host case and default ports;
patterns beginning with `**` retain their supplied spelling. Glob-bearing
segments preserve their operators and query case. Context-relative patterns
and implicit base-URL expansion remain outside this interface.
The native transport privately continues candidates that fail the precise
matcher; these do not create client callbacks. `routeContinuationsPending`
counts those requests until their replies settle.
For regular expressions, use `matcher: {kind:"regex",pattern:"/api/(?:orders|items)/[0-9]+$",flags:"i"}`.
The pattern is a 0–2048-byte ASCII JavaScript `RegExp` source with the same
meaning it has in JavaScript. That covers groups (including named groups and
`(?i:...)`-style modifiers), alternation, character classes (including `[^]`),
greedy and lazy quantifiers, anchors, word boundaries, and character escapes
of ASCII characters. Without the `u` flag, JavaScript's legacy rules apply:
unmatched braces and unknown escapes such as `\a` are literal characters.
An empty source matches every eligible URL; the empty class `[]` never matches,
including inside alternatives or quantifiers. Legacy octal escapes follow
JavaScript's digit-consumption rules when they denote ASCII characters.
Outside a class, a nonzero numeric escape is accepted only when its complete
decimal number exceeds the total capture count, including captures later in
the source. For example, `\101` denotes `A` when it is not capture 101;
8/9 legacy escapes remain literal digits. These legacy forms remain invalid
under `u`. Without `u`, `\k` is a literal `k` only when the complete source
has no named captures; a named capture also makes it invalid inside a class.
Flags are any of `d`, `g`, `i`, `m`, `s`, `u`, `v` and `y`, each at most once and
in any order, except `u` with `v`. As in Playwright, every URL is tested from its
start: `d` and `g` have no effect, and `y` makes the pattern match only at the
start of the URL.

Lookahead, lookbehind, backreferences, `\p{...}` properties, non-ASCII source
text, duplicate group names and the `v` flag run on the same pinned JavaScript
RegExp engine Playwright uses. Each such pattern compiles once; testing one URL
has a 50 ms budget and a memory bound, and a test that exceeds either does not
match. Syntax JavaScript rejects refuses with `ROUTE_REGEX_INVALID`.
WebSocket routes test these patterns in the page instead, as described for
`web_socket_route_add_matcher`.

For a [URLPattern](https://developer.mozilla.org/docs/Web/API/URL_Pattern_API),
pass its component patterns, for example
`matcher: {kind:"url_pattern",hostname:"*.example.com",pathname:"/api/:id(\\d+)"}`.
The components are `protocol`, `username`, `password`, `hostname`, `port`,
`pathname`, `search` and `hash`; an omitted component matches anything, a
protocol's trailing `:`, a search's leading `?` and a hash's leading `#` are
ignored, and matching is case-insensitive, as in Playwright. Patterns use
URLPattern syntax: `:name` segments, `*` wildcards, `{...}` groups, the `?`,
`*` and `+` modifiers and `(regexp)` groups, whose regexps follow the regex rules
above. Each component is ASCII up to 1024 bytes and is matched as written, not
normalized through a URL parser first. Malformed patterns refuse with
`ROUTE_URL_PATTERN_INVALID`. URL patterns are available for HTTP routes, HAR
filters, `url_match` waits and WebSocket routes.

Playwright's function predicates have no equivalent: requests are JSON, so
they cannot carry code to run against each URL. Use exact, glob, regex or
URLPattern matchers instead.

Matching searches the original serialized ASCII HTTP(S) URL, including its
query, without URL-normalizing the regex text. Regexes run on a bounded Rust
automaton when they can, and on the pinned RegExp engine otherwise, while both
transports admit broad HTTP(S) candidates and privately continue nonmatches.
This also preserves original matching after fallback overrides, finite
invocation limits and typed removal. Regex filters do not grant authentication
authority.
Globs do not enable authentication callbacks. An enabled page/context auth policy
still owns challenges throughout that scope, independently of its URL filter.
If a broad glob filter remains after auth is disabled, late challenges receive
browser-default handling without a credential callback.

To answer from a HAR archive at a chosen position in the chain, add
`action: {kind:"add_har",archiveId:ID,notFound:"fallback"}` with optional
`urlFilter` (a matcher; omitted means every HTTP(S) request) and `times`. It
works like Playwright's `routeFromHAR`:

- Handlers run newest first, so a HAR handler takes precedence over callback
  handlers added before it and yields to those added after it.
- When a request's chain reaches it, Testrun answers from the archive itself,
  without showing you a callback.
- On an archive miss, `notFound` decides: `abort` (default), `continue` to the
  network, or `fallback` to the next handler, which may be a callback handler or
  the Context's handlers.

`list` reports each handler's `kind` (`callback` or `har`) with its `archiveId`
and `notFound`. An archive referenced by a HAR handler cannot be removed. Remove
the handler with `remove` or `clear` first. `page_har_set` and
`context_har_set` remain scope-wide alternatives that cannot share a scope with
handlers.

Use `action: {kind:"list"}`, `{kind:"remove",handlerId:ID}` or `{kind:"clear"}`
on the same scope to manage handlers. `action: {kind:"remove_matcher",matcher:MATCHER}`
removes every registration of that typed matcher in the selected scope; no match
is a no-op. Supply the original matcher spelling. Selection compares the dialect
and supplied text, so an exact URL and a glob with identical text remain independent.
`clear` removes all HTTP routing in the selected scope, including standalone
`page_route_set`/`context_route_set` and scope-wide `page_har_set`/`context_har_set`
policies. Other scopes remain active. HAR archives remain registered and can be
removed once no remaining policy uses them. Failed teardown retains the policy
and archive ownership for explicit cleanup retry or owned-scope closure.
Adding handlers and a `clear` without `behavior` refuse while that scope owns
pending request replies (`ROUTE_CALLBACKS_PENDING`). To clear a scope as soon as
its paused requests are answered, as Playwright's
`unrouteAll({behavior: "wait"})` does, send `{kind:"clear",behavior:"wait"}`: the
operation waits until you resolve each paused callback, then clears, and refuses
with `ROUTE_CALLBACKS_PENDING` if callbacks are still pending at its timeout.
`{kind:"clear",behavior:"default"}` or `"ignoreErrors"`, like Playwright's
`unrouteAll` without waiting, clears at once: requests it had paused continue to
the network, and a later reply to their callbacks refuses. `remove` and
`remove_matcher` behave like Playwright's `unroute`: removing the scope's last
handler continues its paused requests the same way, while with other handlers
left, paused requests stay paused and skip the removed one.
Adding a distinct matcher or removing its last handler updates
the installed pattern union. Setup retains handler identities until interception
is acknowledged; `setupPending` identifies incomplete pattern updates, and clearing
the scope releases them. A standalone `page_route_set` or `context_route_set` policy
must be disabled before adding handlers; replacing or disabling a scope through
those operations retires its registered handler IDs. Handler listings redact
queries from URLs and identify each handler's `matcherKind`.

A matching request pauses and exposes a `kind: "route"` callback in browser
status (native Chrome's `callbacks`, Firefox/WebKit's `resources`). `policyScope`
identifies the page or context whose policy owns that request. Resolve its
reference through `callback_resolve`. A popup's first navigation on Firefox/WebKit
can pause before its page reference exists; that callback remains context-owned.
Do not infer its page from the URL. Reply with
`response: {kind:"route_continue"}`,
`{kind:"route_abort"}`, or `{kind:"route_fulfill",status:200,body:"Acme",contentType:"text/plain"}`.
`route_abort` takes an optional `errorCode`, as in Playwright: `aborted`,
`accessdenied`, `addressunreachable`, `blockedbyclient`, `blockedbyresponse`,
`connectionaborted`, `connectionclosed`, `connectionfailed`, `connectionrefused`,
`connectionreset`, `internetdisconnected`, `namenotresolved`, `timedout` or
`failed` (the default). Each engine reports its own error text; WebKit reports
every code the same way.
`route_fulfill` follows Playwright's `route.fulfill`: `status` defaults to 200,
`headers` is a list of `{name,value}`, `json` sends a JSON body instead of `body`
(with `application/json` unless `contentType` is set), and `path` serves a host
file of up to 1 MiB, typed by its extension with Playwright's table. A relative
`path` is relative to the directory you run the command from.
Fulfillment accepts status 200–599, except that the pinned WebKit driver cannot
fulfill 3xx responses, including 304. WebKit refuses those replies with
`ROUTE_STATUS_UNSUPPORTED` before consuming the callback; it can still be
continued or aborted. Text bodies accept up to 65536 UTF-8 bytes. The response
lane remains available while the originating navigation or evaluation waits.
Each callback resolves once; a stale reference cannot affect a later request.
As in Playwright, Page routes see dedicated workers' requests, while
shared-worker requests are not routed. A worker that ends while its request is
paused does not invalidate the callback; continuing it succeeds.
Unanswered callbacks expire after 30 seconds and are aborted. An uncertain reply
is not replayed.

Reply with `{kind:"route_fallback"}` to yield a Page callback to the matching
Context policy, including automatic Context HAR replay. This consumes the Page
callback and issues a fresh Context callback; poll status for its reference when
the Context policy is manual. The paused request keeps its original expiry.
The source Page policy remains owned until the Context reply finishes, so
replacing/removing either policy can return `ROUTE_CALLBACKS_PENDING` during
handoff. With no later matching policy, fallback continues to the network.
`route_continue` always continues directly. To retain request changes through
the handoff, use `{kind:"route_fallback_with", overrides:{method:"POST",
bodyBase64:"QWNtZQ=="}}`. It accepts the same URL/method/header/body bounds as
`route_continue_with`. Matching uses the original request URL; Context HAR
replay and final continuation use the changed request. A later explicit override
replaces the corresponding field; omitted fields remain. No backend continuation
occurs until a terminal reply. Multiple handlers within one scope are unavailable.
If target-policy setup or replacement races with fallback, `ROUTE_POLICY_PENDING`
or `ROUTE_POLICY_CHANGED` leaves the original callback available. Retry after the
policy operation settles, within the original callback deadline.

Use `route_fulfill_binary` with `status`, `bodyBase64` and an optional `headers`
array of `{name,value}` pairs for arbitrary response bytes. Inline decoded bodies
have a 64 KiB limit. For larger bodies, use `route_fulfill_evidence` with `status`,
`networkEvidenceId` and optional `headers`: pass a body `evidenceId` returned by
`network_read`. That path supports up to 1 MiB, verifies the owned artifact before
replying, and does not copy its contents into CLI/MCP arguments. Removing an
evidence record or changing its file makes fulfillment fail before the callback
is consumed. At most 64 provided headers share an 8 KiB budget. Header
names/values and base64 are validated before the callback is consumed. Duplicate
headers, including separate cookies, retain their order. Content-length is
computed from the body; a conflicting provided value is rejected, except for
HEAD/304 representation metadata. HEAD sends no payload, 204 omits content-length,
and 204/205/304 refuse nonempty bodies. Transfer-encoding is removed because
fulfillment supplies a complete body rather than transfer frames.

### Fetch a paused request and fulfill from the response

To inspect or modify the real response before the page sees it, send
`route_fetch` with the paused `callback` and an `httpContext` created by
`http_context_create`. Testrun fetches the paused request's URL, method,
headers and body through that HTTP client and returns the same result as
`http_fetch`, with the response body stored as HTTP evidence:

```json
{"operation":"route_fetch","callback":ROUTE-REFERENCE,"httpContext":HTTP-CONTEXT-REFERENCE,"maxRedirects":0,"timeoutMs":10000}
```

The callback stays paused. Cookie, Host, Content-Length and connection
headers from the page request are not copied: cookies come from the HTTP
client's jar, so create it with `browserContext` to send and receive the
page's cookies (Firefox requires `allowUnknownCookiePartitions:true`, as for
any browser-associated client). `maxRedirects` (0–20) and `timeoutMs`
(0–120000; zero leaves only the enclosing operation deadline) bound the fetch,
and `maxRetries` (0–20, default 0) retries it after
a connection reset. The response body is read up to 1 MiB.

To fetch something other than the page's request, pass `overrides` with any of
`url` (any HTTP or HTTPS URL), `method`, `headers` (replacing the whole list)
and `bodyBase64`, as in
`"overrides":{"method":"POST","headers":[{"name":"content-type","value":"application/json"}],"bodyBase64":"e30="}`.
Overrides apply to this fetch only: the paused request is unchanged, and a later
`route_continue` sends the original. Cookie and connection headers in
`overrides.headers` are dropped like the page's. Invalid overrides refuse before
anything is sent.

Then reply with
`{"kind":"route_fulfill_http_evidence","httpEvidenceId":"EVIDENCE-UUID","headers":[{"name":"content-type","value":"text/plain"}]}`.
`status` defaults to the fetched status and can be changed. Response headers
are not copied automatically, because HTTP evidence can redact them; pass the
headers the page needs. Fulfillment copies the body into a network evidence
record of the page's browser context, listed by `network_evidence_list`; remove
it with `network_evidence_remove` when it is no longer needed. To change the
body, fulfill with `route_fulfill_binary` or `route_fulfill` instead. The fetch
counts as one server request, and the page request does not reach the network.

Resolve paused requests before replacing or disabling Chrome's pattern;
otherwise it returns `ROUTE_CALLBACKS_PENDING`. Existing headers/body values are
not exposed by native callback status. Reply with `route_continue_with` and an
`overrides` object to change `url`, `method`, `headers` or `bodyBase64` before
continuing once. URLs must retain the original HTTP/HTTPS protocol. Header values
retain the existing route bounds; browser-controlled headers are preserved and
cannot be replaced. Binary request bodies are limited to 64 KiB. Validation
failures leave the callback available. Context routes and layered handlers remain
outside these operations.

Import a private HAR 1.2 file with `har_register` and its absolute `path`.
On Unix, its source directory must be owned and private (0700), and the file
must be owned and private (0600). Relative `_file` attachments and ZIP members
are copied into an immutable replay snapshot; traversal and attachment links
are refused. Later changes to source files do not change registered replies.
The limits are four archives, 256 entries per archive, 32 MiB input, 16 MiB of
request/response payloads, and 1 MiB per body. Headers retain the route limits.
`har_list` returns archive and private artifact metadata. If publication fails,
the failure retains `archiveId`; use `har_finalize` to publish retained bytes
without rereading the source. `har_remove` verifies and removes the owned
snapshot while preserving imported source files.

After configuring `page_route_set` or `context_route_set`, resolve a paused callback with
`{kind:"route_har",archiveId:"ARCHIVE_ID"}`. Matching uses the exact private
request URL/method, POST bytes and matching headers; multipart boundary values
are normalized. Decoded HAR payloads receive fresh framing headers; recorded
content encodings and transfer lengths are excluded, while HEAD/304 metadata
lengths are preserved. Archive redirects follow their method rules for non-navigation
requests. The receipt's `har` field reports the match and redirect count.
Missing entries abort by default. `notFound:"continue"` explicitly permits a
missing request to reach the network. `notFound:"fallback"` passes a missing
request to the next handler, exactly as a `route_fallback` reply would: a later
same-scope handler, then the Context's handler or HAR policy, then the network.
The receipt reports `fallback:true` and the `nextCallback` when another handler
takes the request. Unknown archives, unavailable request/body
bytes and unsupported response framing fail before consuming the callback.
Manual navigation redirects return `HAR_NAVIGATION_REDIRECT_REQUIRED`, also
leaving the callback available for another reply.

Use `page_har_set` with `page` and `archiveId` to reply automatically, including
navigation redirects and concurrent subrequests. Disable manual interception
first. Missing entries abort unless `notFound:"continue"` is explicit. All engines
include child frames, including native Chrome cross-process frames.
Use `context_har_set` with `context` for current and future pages, popups and
frames. A matching page policy takes precedence; manual routing and HAR replay
are exclusive within one scope. `notFound:"continue"` continues to the network
without consulting another policy. `notFound:"fallback"` on a Page policy lets
the Context's handlers or HAR policy handle the missing request. On a Context
policy no later handler exists, so the request reaches the network. Service-worker interception remains
unavailable. `har_list` reports each rule's scope, phase, reply count and failures. An archive
in use cannot be removed. Stop with `page_har_set` and `archiveId:null`; uncertain
setup or stop retains ownership until cleanup completes or the owning scope closes.
The pinned Firefox backend can fail to disable interception after a replayed
navigation redirect. The stop error reports `cleanupPending` and keeps the rule
in `stopping`; close that owned page to release it. The archive remains in use
until the page is closed.

Both replay operations accept an optional `urlFilter` with the same exact, glob,
regex or URL pattern shape as `network_route`. Omitted or null selects all
HTTP(S) requests in scope. Filters match the complete original URL, including its
query, before archive lookup. A Page filter nonmatch allows a matching Context
policy to handle the request; a nonmatch in both scopes reaches the network.
For a selected request missing from its archive, `notFound` still applies: abort
by default, continue directly to the network, or fall back to the next handler.
Only `fallback` lets a missing Page entry reach the Context archive. The filter compiles once before
interception setup. `har_list` exposes `urlFilterActive` without the pattern.
Disable a rule with `archiveId:null` and omit `urlFilter`; supplying a non-null
filter without an archive refuses with `HAR_FILTER_REQUIRES_ARCHIVE`.

Start a separate private HTTP collection with `har_capture_start`, `context` and
optional `content`:

- `omit` (default): records request and response metadata; bodies carry
  `content-omitted-by-policy`.
- `embed`: stores bodies inline as base64 in the published `.har`.
- `attach`: publishes a `.zip` containing `har.har`. Each distinct nonempty body
  becomes one member named by its SHA-256 digest and a type-based extension, and
  is referenced by `content._file` or `postData._file`. Empty bodies stay inline.
  `har_register` accepts this ZIP unchanged.

Optional `mode` selects the HAR fields:

- `minimal` (default): the request and response fields replay needs.
- `full`: adds `log.pages` and each entry's `pageref`, request and response
  `httpVersion`, `cookies` parsed from `Cookie` and `Set-Cookie` headers, the
  request `queryString`, `redirectURL`, `content.mimeType`, sizes, `timings`
  and `time`, `serverIPAddress`/`_serverPort` and `_securityDetails` for HTTPS.
  Values the browser does not report keep HAR's unknown value (`-1`, or
  `HTTP/1.1` for the version). Firefox and WebKit report request and response
  header and body sizes; Chrome reports `_transferSize`. Each page's `title` and
  `pageTimings` (`onContentLoad`, `onLoad`, in epoch milliseconds) are read from
  its current document when capture stops. A page that has closed or can't be
  read keeps `title: ""`, `-1` timings and `_testrunTitleUnavailable`. Firefox and WebKit look up
  the server address, TLS details, version and sizes for each recorded response
  when capture stops, so a full-mode stop takes longer than a minimal one.

Native Chrome context captures also retain service-worker requests before or
after worker creation, including after the creating page closes. Both modes
identify those entries with `_serviceWorkerRef: "worker@<worker UUID>"` and omit
`pageref`; the identifier records provenance and grants no live dispatch
authority. Page-selected captures exclude worker requests. Context replay,
headers and automatic credentials keep their existing owners while capture is
active. Dedicated/shared-worker attribution and broader capture variants have
separate coverage limits.

In `full` mode, each WebSocket the capture selects becomes an entry with
`_resourceType: "websocket"`. The URL filter matches socket URLs with `ws:`
read as `http:` and `wss:` as `https:`. The entry carries the 101 handshake
status and `_webSocketMessages`: each message's `type` (`send` or `receive`),
`time` in epoch milliseconds, `opcode` and `data` (text, or Base64 for binary).
Messages are recorded only when `content` is `embed` or `attach`. With `attach`,
each socket's messages are a separate `<id>.jsonl` file (one message object per
line) named by the entry's `response.content._file`, beside the response body
files, and the entry has no `_webSocketMessages`; a socket that sent and
received nothing has no file. `har_register` reads those files back, so an
attached archive replays and re-exports like an embedded one. Each socket keeps at most 1000 messages,
and further ones are counted in `_testrunDroppedMessages`. Socket entries survive
re-registration but are never replayed.

To also receive the archive as your own file, set `path` to a new absolute
`.har` or `.zip` file in a private directory: owned by you, with no access for
others. Testrun refuses an existing file (`HAR_EXPORT_EXISTS`), other paths
(`HAR_EXPORT_PATH_INVALID`) or shared directories before capture starts.
`har_capture_stop` still publishes the private archive, then writes your file
with mode `0600` on Unix or an owner-only ACL on Windows. It reports the file as
`export`:

- A `.zip` path receives the attached ZIP.
- A `.har` path with `content: "attach"` receives the HAR plus one file per
  body, beside it or in an existing private `resourcesDir`. `har_register` can
  re-import such a HAR only when its bodies are beside it.
- Other `.har` paths receive one self-contained file.

Removing the archive never deletes your file.

Status and stop results report the selected `content` and `mode`. Closing a
context with an active capture stops and publishes it first, collecting bodies
and full-mode details while the context is still open. The `context_close`
result includes the archive as `har`. `har_capture_status` reports
entry/completion counts and explicit source gaps. Diagnostic subscriptions remain
independent: stopping diagnostics does not stop capture, and stopping capture
preserves an active diagnostic subscription. After the journey, `har_capture_stop`
freezes the collection and returns an `archiveId` usable for replay, plus private
artifact metadata. Captures and imports share the four-archive limit.
Optional `urlFilter` selects requests using the `network_route` matcher shape:
`{"kind":"exact","url":"https://example.com/api?item=one"}`,
`{"kind":"glob","pattern":"**/api?item=*"}`, or
`{"kind":"regex","pattern":"/api\\?item=one$","flags":"i"}`.
Omitted or null captures all observed HTTP(S) requests. Matching uses the complete
URL including its query, and regexes follow the same JavaScript `RegExp` rules as
HTTP route handlers. Unsupported syntax refuses before capture starts. Excluded
requests consume no capture entry or payload budget and remain available to
independent diagnostics. Status exposes `urlFilterActive`, without the pattern.
Capture and replay filters are configured independently.
Optional `page` limits capture to requests from one page of that context,
including its child frames. Requests from other pages, workers and service
workers are not recorded. A page from another context refuses with
`FOREIGN_PAGE`. The collection continues after the selected page closes, records
nothing further and still publishes at `har_capture_stop`. Status reports the
selected `page`, or null for the whole context. Firefox and WebKit attribute
requests to pages as their drivers do, which can include dedicated-worker requests.
Each entry's `_testrunRequestId` refers to the collecting session's network
diagnostic identity for exact correlation.
Native attachment setup is asynchronous; traffic before an attachment is enabled
falls outside capture coverage. Create pages before starting capture when their
startup traffic matters.

Capture artifacts preserve private URLs and headers. Bodies are bounded and
explicitly included; omitted, expired, compressed-size-unknown or otherwise
unavailable bodies carry reasons instead of empty replacements. Native request
text retention is limited to 64 KiB; response bodies keep the archive's 1 MiB
per-body bound. Entry and aggregate limits remain visible as dropped counts.
Observation timestamps are labelled `observer-clock`; backend timing metadata is
preserved where supplied. These archives prioritize replay and do not synthesize
missing HAR timing fields. A failed stop retains its capture state for retry or
owned-session disconnect. Publication failure retains `archiveId` for
`har_finalize`; it does not repeat network requests. Owned shutdown publishes the
retained prefix without fetching additional bodies.

To re-record an archive, supply its ID as `replacesArchiveId` when starting
capture; this is Playwright's `routeFromHAR` with `update`, so `content`
defaults to `attach` and `mode` to `minimal`, its `updateContent` and
`updateMode`. Without `replacesArchiveId`, `content` defaults to `omit`. Stop
replay rules using that archive first. The capture records fresh traffic; its
stop receipt returns a new `archiveId` and `replacesArchiveId`.
Select the new ID for subsequent replay. The prior archive remains immutable and
cannot be removed until capture stops. The replacement contains the new
collection, and the previous version stays available for rollback.

On Chrome, set `authenticationChallenges: true` alongside the route URL to
receive HTTP authentication challenges in that page target. These callbacks
have `details.phase: "authentication"`; their summary includes the challenge
origin and scheme and omits credentials. Reply with
`{kind:"auth_credentials",username:"Acme",password:"example-password"}`,
`{kind:"auth_cancel"}`, or `{kind:"auth_default"}` to let Chrome use its default
handling. Ordinary `route_continue` replies cannot resolve an authentication
challenge. Challenges expire after 30 seconds and cancel authentication.
Supplying credentials can cause the browser to retry the challenged request;
Testrun does not repeat an uncertain credential reply. This native option applies
to the page target's challenges, independently of the request URL pattern.
Firefox/WebKit reject this option with `AUTH_CHALLENGE_UNSUPPORTED`.

### Inspect one HTTP request

Use `{operation:"network_read", context:REFERENCE, requestId:UUID}` with the
exact UUID from diagnostic history. Add `options:{headers:true}` for bounded
header details. Authorization, cookies and unknown/custom header values are
masked; a small metadata allowlist remains visible. Chrome reports the
completeness of its Network-event header source.

Add `options:{responseDetails:true}` to inspect private `responseDetails`:
`serverAddr` (IP address and port), `securityDetails` (TLS protocol, subject,
issuer and certificate validity) and `timing`. No request is repeated and no body
is read. Connection facts can be null, including TLS details for ordinary HTTP.
Certificate strings are bounded to 2048 bytes per field; rejected fields appear
in `securityFieldsOmitted`. Other certificate internals are excluded.
`timing.startTime` is Unix epoch milliseconds; other timing fields are offsets in
milliseconds. Certificate validity uses Unix epoch seconds. Missing timing uses
the pinned SDK's `0` start and `-1` offsets; `timingObservedFields` distinguishes
reported fields from those defaults. `responseEnd` remains `-1` until completion
is observed. Cached requests can have unknown connection timings. The response
availability and source are explicit. These are browser request observations;
they do not describe a host `http_fetch` client.
WebKit exposes connection facts after request completion. An earlier read keeps
timing available and reports `connectionDetailsPending:true`; read again after
the request finishes. An unreported optional certificate field stays absent.
`responseDetails` also reports `httpVersion`, `statusText` and the pinned SDK's
`sizes` (`requestBodySize`, `requestHeadersSize`, `responseBodySize`,
`responseHeadersSize` and Chrome's `transferSize`). Sizes are null with
`sizesPending:true` until the request finishes; read again after completion.
Firefox and WebKit also report `httpVersion` only after completion.

Every read reports `method`, the lowercase `resourceType` (such as `fetch` or
`document`) and the response `status`, null before a response arrives. It also
reports the request's relations: `navigation` (a navigation request),
`frame` (the owning frame reference, or null when it is gone or the request
belongs to a worker), `serviceWorker`, `fromServiceWorker`, `statusText`, and the
redirect chain as `redirectedFrom` and `redirectedTo`. Each is a request UUID
that `network_read` accepts, or null. A redirect's earlier hop stays readable
with its own status, status text and response headers.

For exact header evidence, add `options:{rawHeaders:true}`. The result then has
`rawRequestHeaders` and `rawResponseHeaders`, each
`{entries:[{name,value}], truncated, private:true}` with up to 256 entries and
64 KiB. They are unmasked: cookies, authorization and custom values appear
as sent, so keep them out of shared reports. Repeated headers stay separate
entries. Firefox and WebKit keep the driver's wire order; Chrome reports headers
in name order. `rawHeadersIncluded` confirms the option. Masked `headers:true`
stays the default view.

For private body evidence, set `options:{body:"request"}` or `body:"response"`.
`maxBytes` defaults to 65536 and accepts up to 1048576. Bytes are saved as an
unredacted private `decoded-http-payload` artifact; the result contains its path,
size and digest. Response inspection requires completion and never repeats the
request. Chrome uses its bounded CDP response cache (262144 bytes per resource);
evicted or unavailable payloads return `BODY_UNAVAILABLE`. The unchanged
Firefox/WebKit driver materializes whole responses, so Testrun requires a known,
uncompressed Content-Length before asking for a response body. Unknown size and
compressed responses return `BODY_SIZE_UNAVAILABLE` or
`BODY_DECOMPRESSION_BOUND_UNAVAILABLE`.

Each connection retains up to 32 body artifacts. `network_evidence_list` lists
them; `network_evidence_remove` takes `evidenceId` and refuses changed files.
Closing the browser preserves captured evidence on disk. Diagnostic metadata
and ordinary status never include the body bytes.

### Make HTTP requests

Use `http_context_create` for API requests with their own cookies and network
settings. These clients run in Rust on the host across all three browser engines.
By default they use an isolated cookie jar. Add `browserContext` with an owned
context reference to exchange its browser cookies under the selection rules below. Both kinds use
host HTTP semantics without browser
CORS enforcement. They do not execute scripts in returned HTML.

Create a client with an optional base URL:

```json
{"operation":"http_context_create","options":{"baseUrl":"https://example.com/","timeoutMs":30000,"maxRedirects":10}}
```

Omitting `maxRedirects` uses 20. An explicit zero returns the initial redirect
response without following it; a positive exhausted budget returns
`HTTP_REDIRECT_LIMIT` with response metadata. Empty or absent `Location` does
not trigger another request. At an exhausted positive budget, a redirect status
still reports the limit even if its final `Location` is absent.

301/302 convert POST to GET; 303 converts methods other than GET/HEAD to GET.
Those rewrites discard the entity and its Content-Encoding, Content-Language,
Content-Length, Content-Location and Content-Type headers. GET/HEAD on 303 and
methods on 307/308 preserve their entity settings. Explicit empty bodies remain
distinct from no entity. Redirects strip credential headers on every hop and
all explicit headers across origins; this does not change the original client.

For a browser-associated client, include `browserContext` in those options.
Each redirect hop reads current browser cookies; response cookies, including
HTTP-only cookies, are written back to that same context. This exchange is not
an atomic snapshot with application cookie changes.

Chrome and WebKit exclude partitioned cookies by default. To include one exact
namespace, add `cookiePartition:{topLevelSite:"https://example.com"}` to the HTTP
client options. This includes unpartitioned cookies and cookies with that exact
canonical HTTP(S) site key. Chrome also accepts `hasCrossSiteAncestor`; its
omitted value defaults to `true`. WebKit requires this field to be omitted
because its cookie channel exposes only the site key. Opaque partition keys
are excluded. Secure partitioned response cookies from HTTPS are written into
the explicitly selected namespace.

Firefox's pinned cookie channel omits partition labels. Associated HTTP creation
there returns `HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE` unless options include
`allowUnknownCookiePartitions:true`. This explicitly accepts its unlabeled
context view, which can include cookies from different top-level partitions.
The option cannot select a partition; `cookiePartition` is unsupported on that
channel. Creation and listing report `cookiePartitionIdentityAvailable` and
`unknownCookiePartitionsAllowed` without disclosing the selected site key.

Associated clients on every engine inherit the browser context's `baseURL` when
they set no `baseUrl`. Associated clients use the configured browser `userAgent`,
or the owned engine's default when none is configured. Firefox/WebKit clients
also inherit the context's `ignoreHTTPSErrors` policy. HTTP `userAgent` and `ignoreHttpsErrors` options
override those defaults; an explicit `false` retains TLS verification. Request
headers override the HTTP user-agent default. Listing reports effective TLS
verification and whether a user agent is configured without exposing its value.
An omitted, null or empty HTTP `userAgent` option selects that default; a literal
empty `User-Agent` request header is sent unchanged.
Omitted or null HTTP `headers` follows the associated context's current
`extraHTTPHeaders`. An explicit list replaces those defaults; `headers:[]`
suppresses inheritance. Request headers then override defaults by case-insensitive
name, including explicit empty values. Defaults apply to each request origin,
with or without `baseUrl`; `baseUrl` resolves relative request URLs.
Each fetch snapshots its defaults before dispatch. A context header change the
browser rejects restores the previous headers, as the pinned SDK does: native
Chrome reapplies them to each attachment, and inherited HTTP clients use them
again. If the outcome stays uncertain (a lost reply or a failed reapplication),
inherited HTTP creation/fetch refuses with `HTTP_CONTEXT_HEADERS_UNCERTAIN`
until an explicit configuration is acknowledged.
Explicit HTTP header overrides keep their own defaults during that uncertainty.
Proxy settings inherit the resolved browser-context default unless explicitly
overridden. Omitted or null `clientCertificates` inherits the owned context's
TLS identities; `clientCertificates:[]` disables that inheritance. Isolated clients
verify TLS unless overridden and use the pinned Playwright host/language user
agent on the initial macOS host. Its OS version is read once without starting
Node; engine defaults come from native browser metadata, never page JavaScript.
Native Chrome still refuses browser-creation userAgent/ignoreHTTPSErrors;
configure its HTTP TLS policy explicitly.

Set HTTP `httpCredentials` to one `{username, password, origin?, send?}` object
or an ordered list. The first entry whose lowercased `origin` equals the URL's
origin wins; an omitted or empty origin matches every origin. The configured
origin is compared literally after lowercasing: a trailing slash, explicit
default port or path does not equal the URL's canonical origin. Lists accept up
to 32 entries, each field up to 4096 UTF-8 bytes and all fields up to 64 KiB.
Omitted/null HTTP credentials follow the associated browser context's
current acknowledged credentials; an explicit object/list replaces them and
`[]` disables inheritance. Existing Firefox cookie-partition acceptance is still
required. Each request snapshots its credentials before allocation or HTTP
dispatch and keeps them through challenges, redirects and explicit retries.
Later changes affect subsequent requests.

Use `context_configure` with `setting:{kind:"http_credentials",credentials:...}`
to replace an owned browser context's credentials. The `credentials`
field is required: provide an object/list, null or `[]` to clear.
An uncertain setter result fences inherited client
creation/fetch with `HTTP_CONTEXT_CREDENTIALS_UNCERTAIN` until an explicit
configuration is acknowledged; existing explicit HTTP credential overrides
remain independent. Admission refusals preserve prior defaults; SDK setters
also restore them after a known undispatched result.
Setter receipts contain counts and ownership facts, not secrets. Context close
retires the shared defaults; a late acknowledgement cannot revive them.

Native Chrome shares interception with manual and HAR routes. Explicit
authentication callbacks take precedence over configured credentials. Pending
route callbacks refuse credential changes with `ROUTE_CALLBACKS_PENDING`.
Before applying a change, Chrome finishes already-queued private automatic
replies in that context using their original credential selection and the
remaining operation deadline. A manual callback blocks the edit immediately;
an uncertain automatic reply also remains owned and blocks changes without
replaying credentials. Defaults change only after this pending work is settled.
Automatic requests/challenges stay private: an ordinary request continues,
the first matching challenge receives credentials, and a repeated challenge on
that native request is cancelled. Browser challenge handling selects by request
origin regardless of challenge scheme/source; `send` controls only HTTP clients.
Completed network requests release their attempt record. Tracking is bounded to
512 live records, with no eviction that could permit another credential reply.
An observation gap cancels automatic challenges and refuses native credential
reconfiguration with `AUTHENTICATION_OBSERVATION_LOST`; replace the connection.
Unknown interception acknowledgements can require closing the affected page or
context before recovery. Live service-worker requests use their context and
original attachment, without page authority. Credential removal stops automatic
challenge replies after acknowledgement; it does not delete Chromium's HTTP
authentication cache. A later request can still send a cached Authorization
value. Dedicated/shared-worker challenge handling remains outside this contract.

The default `send:"unauthorized"` sends credentials only after an acknowledged
401 whose trimmed `WWW-Authenticate` starts with case-sensitive `Basic`, and
only if Authorization is absent or empty. It retries that request once with
the same method/body, includes acknowledged challenge cookies, and uses the
original deadline. An uncertain browser cookie update prevents the retry.
`send:"always"` replaces the initial Authorization header before dispatch.
Redirects remove credentials under the existing header rules; a subsequent
challenge selects credentials for that hop's origin. A failed authentication
does not keep retrying. Replies and failures report `authenticationRetries`
separately from connection-reset `retryCount`; `replayed:true` reports either
kind of replay. Ordinary receipts and debug formatting omit credential values;
submitted requests remain private operation evidence.

An isolated HTTP client's `options.storageState` accepts a pinned APIRequest
state object with optional `cookies` and `origins`, or the path of a regular
JSON file, as written by `storageState({path})`. A relative path resolves
against the working directory of the CLI or MCP client that sent the request.
Links are followed and file permissions are not checked. State is copied before
client allocation; changing the file afterward does not change the client.
Input is limited to 1 MiB, with at most 256 live cookies and 64 KiB retained
cookie data. Cookie names/values that could inject a header refuse. File reads
share the four-reader limit with selected uploads and respect cancellation and
the original deadline. Directories and other special files refuse with
`HTTP_STORAGE_STATE_FILE_NOT_REGULAR`; unreadable or missing files with
`HTTP_STORAGE_STATE_FILE_IO`.

`http_context_storage_state` takes `httpContext` and optional
`options:{indexedDB:true,opfs:true,path:"state/auth.json"}`. With `path`, the
state is also written to that file: a relative path resolves against the
sending client's working directory, missing directories are created
owner-only, and an existing file is replaced. The file is readable only by its
owner because it holds credentials. The receipt's `file` reports the path and
size. If writing fails, the private evidence stays published: the error's
`evidenceId` can be read or saved with `diagnostics_export`. For an isolated client, it captures current cookies and the client's
initial origin records as private evidence. Local-storage records are preserved;
IndexedDB is empty unless requested, and OPFS is omitted unless requested.
These origin records are retained data and are not executed or read from a
browser. Clearing cookies preserves them. The response contains counts and an
artifact receipt, not cookie or origin values. Read the selected evidence with
`http_evidence_read`, or use its private attachment for state larger than that
read's 64 KiB result limit. `http_evidence_remove` removes it explicitly.

For browser-associated clients, export reads the exact live
browser context's cookies and visited-origin storage. Local storage is included;
`indexedDB:true` adds real database schemas, indexes and records. Chrome and Firefox also
supports `opfs:true` for origin-private file bytes. WebKit refuses that flag with
`OPFS_UNSUPPORTED`. Unrequested IndexedDB/OPFS fields follow the browser's native
omission behavior; session storage and virtual authenticator credentials are
excluded. Existing Firefox cookie-partition opt-in is still required when the
client is created. A client or browser context closed before the state read
settles cannot publish its late result.

Native Chrome tracks visited HTTP(S) origins for the context, including child
frames and pages that have closed. Collection first tries current pages in
attachment order, then visits remaining origins in their original order through
one hidden private page. That page has its own blank-document interception,
bypasses service workers and does not enter application page lists, activity,
startup scripts, bindings or route callbacks. State uses the pinned Playwright
storage codec in an isolated world. Exports are limited to 1 MiB. A context is
bounded to 1024 visited origins and 2048 bytes per normalized origin. Private-page
allocation and each blank-response fulfillment use up to five seconds within
the remaining operation deadline. History loss or overflow refuses export
instead of returning partial state; replace that browser context after overflow
or reconnect after observation loss. Opaque cookie partition identity refuses
with `HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE`.

A hidden-page allocation with an uncertain outcome requires browser disconnect;
creation is never replayed. Cancellation, session closing or an expired operation
prevents a new hidden allocation. Page cleanup waits for verified destruction
within the original deadline. Complete state is staged before page cleanup. If
cleanup remains pending, `http_evidence_finalize` retries cleanup and publication
of those same bytes. Concurrent associated collection refuses with
`HTTP_STORAGE_STATE_PENDING`. An incomplete collection never publishes evidence.

If state publication fails, the operation result retains `captureId` and
`stateRetained:true`. After resolving the I/O cause, call
`http_evidence_finalize` with that `captureId`. It publishes the same captured
bytes even after cookies change or the HTTP client or source browser context is removed; it does not
capture state again or repeat an HTTP request. `http_evidence_list` exposes
`pendingArtifacts` for this recovery. Unfinished publication keeps disconnect
cleanup pending until it can finish. State evidence cannot be used as an HTTP
route-fulfillment body. Caller-selected export filenames and browser-associated
state initialization are unavailable. Associated initialization refuses with
`HTTP_BROWSER_STORAGE_STATE_UNAVAILABLE`.

Associated responses identify `browserCookieUpdate.state` as `none`,
`acknowledged` or `unknown`. An uncertain update stops redirect following and
sets `redirectStoppedForCookieUpdate:true`; the HTTP response remains available
and the request is not replayed. `http_context_cookies_clear` clears the whole
associated browser context's cookie jar. Removing the HTTP client preserves
browser cookies. Closing the browser context prevents new associated requests;
already dispatched work retains its response and reports cookie-update failures.

Keep the returned `httpContext` reference. Submit a request with it; replace all
reference fields below with that complete returned reference:

```json
{"operation":"http_fetch","httpContext":{"connectionId":"CONNECTION-UUID","id":"HTTP-CONTEXT-UUID","generation":"GENERATION-UUID"},"request":{"url":"/api","method":"POST","body":{"kind":"json","value":{"name":"Acme"}},"readBody":true,"maxBytes":65536}}
```

To copy a browser request into a host HTTP client, start network diagnostics
for its context. Private request retention starts automatically before the
subscription receives events. Request diagnostic events report
`requestInputAvailable` and their `requestId`. Submit
`http_fetch_request` with `httpContext`, that ID as `sourceRequestId`, and optional
`options` using the ordinary HTTP request fields. Omitted fields inherit the
retained request inputs; omitted, null or empty `method` inherits their method. Omitted
or null headers inherit its provisional headers; `headers:[]` replaces them.
An explicit empty body replaces the source body. Binary initializer bytes are
copied directly. All ordinary HTTP transport, cookie, deadline and evidence
rules still apply.

For any present HTTP body, including an empty body, its serialized byte length
replaces a copied or explicitly supplied Content-Length header, as in the pinned
SDK. Body replacement cannot retain the initializer's old length.

Text bodies follow the pinned SDK's caller-header rule. The first caller header
named Content-Type (case insensitive) selects JSON string handling only when
its value is exactly `application/json`. Valid JSON text is sent byte for byte,
including whitespace and duplicate keys. Other text is JSON-quoted; an empty
string becomes `""`. Quoting must fit the existing body byte bound before HTTP
allocation. Binary/base64 bodies bypass this string rule. Context default
headers affect the outgoing request but do not select JSON string handling.
If caller headers repeat Content-Type with different casing, the first selects
encoding and the last supplies the outgoing value.
Provided raw text and base64 bodies default to `application/octet-stream` when
Content-Type is absent, including empty bodies. An explicit empty header value
is preserved. An omitted body does not add a content-type default.

Retention requires active network diagnostics. Set `http_request_inputs_set`
with `context` and `enabled:false` to opt out before or during diagnostics;
that choice survives stop/start in the same context. Set `enabled:true` to
resume retention for subsequent requests. A new context starts with the default.
Copies survive source page/context closure and disabling capture. Release one
with `http_request_input_remove` and its `sourceRequestId`; already dispatched
requests keep their own copy. Connection cleanup releases all copies. Capture
refuses without eviction at 512 inputs or 32 MiB total; each body is limited to
1 MiB, headers to 64/16 KiB and URLs to 4096 bytes. Routine diagnostic records
contain availability and refusal codes rather than retained headers/body bytes.
Valid route continuation/fallback overrides update the retained request's URL,
method, caller headers and binary body cumulatively. Header replacement retains
caller intent, including Cookie headers that browser dispatch may filter; copied
duplicate names are joined as in the pinned SDK. An empty header list or body
replaces the previous value. Already dispatched HTTP copies keep their earlier
snapshot. A source that has not yet reached diagnostics reports
`HTTP_REQUEST_INPUT_NOT_READY` before consuming its callback; wait for its request
event before retrying. An update exceeding retention bounds makes future copies
unavailable rather than returning stale values, without evicting other inputs.

Request-body reads use retained bytes when present and report
`requestBodyFidelity:"binary-request-initializer"` or
`"binary-caller-route-override"`. Releasing a retained input while capture remains
enabled makes its later body read unavailable. Without a retained initializer,
the older native CDP text read reports
`requestBodyFidelity:"cdp-text-utf8-not-lossless"`; it cannot preserve arbitrary
binary request bodies. Uncaptured SDK body reads report `"sdk-wire-initializer"`.
Two differences from the SDK are deliberate. Retention is bounded: the SDK keeps
Request objects until their context closes, while Testrun refuses new copies at
the limits above instead of growing without bound. Native Chrome can return exact
bytes only for bodies it retained; the CDP text fallback is reported, not hidden.
This bounded interface does not establish full SDK Request parity.

HTTP request URLs use WHATWG normalization, including outer whitespace and
path/query spaces. Fragments remain in private response details but are omitted
from the network request. URL username/password values are percent-decoded as
UTF-8 and sent as Basic authorization; explicit Authorization takes precedence,
and configured always-send credentials override it. A Basic challenge can select
configured credentials when no nonempty explicit Authorization was supplied.
Redirects select URL credentials for the new hop. Routine metadata redacts URL
credentials, query and fragment. Malformed percent-encoded credentials refuse
before dispatch. Request header values use Node's Latin-1 byte encoding; values
above U+00FF and forbidden controls refuse with `HTTP_HEADER_INVALID` before any
origin request. This also applies to HTTP-client defaults and user-agent values.

`method` defaults to `GET` and is uppercased before dispatch. Request `params`
accepts an encoded query string that replaces the existing query, or ordered
`[{name:"label",value:"Acme"}]` pairs that append to it. Empty strings and arrays
leave the query unchanged. Pairs preserve duplicate names and use form percent
encoding. Queries accept up to 128 pairs within 4096 UTF-8 input bytes; the final
URL also has a 4096-byte bound. Body text is UTF-8; use
`body: {kind:"base64",data:"AAEC/w=="}` for binary input. Use
`body: {kind:"json",value:VALUE}` for JSON or
`body: {kind:"form",fields:[{name:"label",value:"Acme"}]}` for a URL-encoded
form. Form fields are strings; repeated names preserve their order. JSON and
forms default to `application/json` and `application/x-www-form-urlencoded`
respectively; explicit HTTP-client or request content types take precedence.
Forms allow 128 fields with names up to 1024 UTF-8 bytes and values up to 16384
bytes. Every request body has a 1 MiB encoded bound checked before dispatch,
including JSON escaping and form percent encoding.
For multipart uploads, use `body: {kind:"multipart",fields:[
{kind:"text",name:"label",value:"Acme"},
{kind:"file",name:"upload",filename:"Acme.bin",mimeType:"application/octet-stream",data:"AAEC/w=="}]}`.
File `data` is Base64. Parts preserve order and duplicate names. Multipart bodies
accept 128 parts within the same 1 MiB total encoded bound, including framing;
names and filenames accept 1024 UTF-8 bytes and MIME types accept 256 ASCII
bytes without controls. Testrun supplies the matching boundary unless an explicit
content type overrides it; an override must describe the actual entity.
To upload a selected local file, use a part such as
`{kind:"file_path",name:"upload",path:"/private/tmp/Acme/report.json"}`.
`path` is an absolute path on the Testrun host. Only regular files are accepted;
symlinks, devices and pipes refuse. `filename` optionally overrides the basename.
The host reads each selected file once into the bounded request payload before
starting HTTP. Cancellation or expiry during preparation prevents HTTP dispatch; an in-progress
read keeps its reader slot until it returns or observes cancellation.
At most four readers run at once, and the complete multipart entity
must still fit 1 MiB. Arbitrary live application streams are unavailable.
For either file form, omitted or empty `mimeType` uses the pinned filename table,
falling back to `application/octet-stream`. Explicit MIME values take precedence;
file contents are not sniffed. Body retries reuse the prepared bytes.
Responses include status, redacted URL, redirect count and masked
header metadata. HTTP 4xx/5xx responses return their status as data. A successful
HTTP response does not establish that an application mutation completed.
Set `failOnStatusCode:true` on the HTTP client or request to return `HTTP_STATUS`
for statuses outside 200–399. Request values override the client default. The
failure includes masked response metadata and omits response body bytes. Status
handling does not repeat requests. Leave it false to inspect error bodies as
private evidence.

`readBody` defaults to false. When true, `maxBytes` defaults to 65536 and accepts
1–1048576. The result's `body` names a private `http-entity-body` artifact with
size and digest. Transfer framing is removed. Single gzip (including x-gzip),
deflate and Brotli content codings are decoded; `maxBytes` bounds decoded bytes
and encoded input has a separate 1 MiB limit. `bodyDecoded`, `bodyBytes`,
`encodedBodyBytes` and `contentEncoding` describe the finalized bytes. Original
response headers still describe the server's encoded representation. Unknown or
stacked codings retain their bytes and report `contentEncoding: "unrecognized"`.
Malformed compressed data returns `HTTP_BODY_DECODE` with response metadata;
neither decoding failures nor body limits admit retries. Empty encoded responses
are valid; gzip permits a missing final trailer, matching the pinned request
reader's flush behavior. Default requests send `Accept: */*` and advertise
`Accept-Encoding: gzip,deflate,br` on every hop. Client headers override these
defaults and request headers override client headers case-insensitively,
including explicit empty values. Redirects preserve those overrides; built-in
defaults fill headers that remain omitted.
Ordinary status and evidence
listings omit body bytes. `bodyComplete` describes body retrieval and
`bodyIncluded` indicates a finalized artifact. `HTTP_BODY_BOUNDS` can occur after
the request has reached the server; inspect its response metadata before
repeating a mutating call.

Use `http_evidence_read` with the body's `evidenceId` and `format: "text"`,
`"json"`, `"json_value"` or `"base64"` for an explicit private result. Text replaces invalid
UTF-8. JSON decodes those same replacement characters before parsing and uses
ECMAScript IEEE-754 numbers, including rounding integers beyond exact range.
`json_value` preserves every parsed value as a flat graph: `root` indexes
`nodes`; string nodes contain UTF-16 `units`, object keys are arrays of those
code units, and arrays contain child
`items` indexes, and objects contain ordered `{key,value}` entries. Object
entries follow ECMAScript own-property order, with duplicate names retaining
only their final value. Number values use finite JSON numbers or the strings
`positive_infinity`, `negative_infinity` and `negative_zero`. This represents
lone surrogate units and overflow without losing data through JSON transport.
Plain `json` refuses non-finite numbers or lone surrogates with
`HTTP_RESPONSE_JSON` and a private recovery hint to read the same evidence as
`json_value`. Invalid JSON syntax also refuses. `maxBytes` bounds the entire
encoded result (default and maximum 65536); oversized results refuse without
truncation. Read the artifact directly for larger bodies. Reads verify the
retained file's size and digest and continue to work after HTTP-client removal,
until `http_evidence_remove` deletes it. Neither reads nor parse errors send
HTTP. Response `ok` is true only for statuses 200–299.

Use `http_response_read` with a successful fetch's `requestId` for an explicit
private result containing the exact final URL (including its query), status,
`ok` and header array. The HTTP/1.x observer preserves original header casing,
cross-name order and duplicate fields, including interleaved `Set-Cookie`
values. `headerNames:"original"`, `headerOrder:"wire"` and
`wireMetadataAvailable:true` identify observed fields. Header values and reason
phrases use the pinned Node client's ISO-8859-1 byte-to-string conversion;
non-ASCII header values also provide exact `valueBase64` bytes. `statusText`
contains the wire reason phrase, including a known empty string, with
`statusTextAvailable:true`. It is never replaced with a canonical phrase.

Set `includeHeadersObject:true` to additionally read `headersObject` and
`headerObjectKeys`. The object lowercases names and joins repeated values with
`, `, except `set-cookie`, which uses a newline. The pinned SDK's ordinary-object
assignment does not create a `__proto__` string property; the original header
array still retains that field. `headerObjectKeys` preserves ECMAScript own-key
order, including ascending integer names. Values use ISO-8859-1 conversion of
the retained bytes. This projection is opt-in and shares the entire result's
existing byte/structural limits. When wire metadata is unavailable, its
cross-name order remains limited by the parsed fallback's reported order.

Observation retains at most 16384 bytes per head and skips up to eight
informational responses. It stops at the final header terminator without copying
body bytes. The observed status and header values must match the parsed response.
If observation is unavailable, malformed, over budget or mismatched,
`wireMetadataAvailable:false` and `wireMetadataFailure` explain the gap. That
fallback exposes parsed lowercase names and per-name value order, with UTF-8
replacement and exact Base64 for non-UTF-8 values; `statusText` is null and
`statusTextAvailable` is false. Redirects and explicit retries reset observation.
Connection pooling, cookie handling and TLS policy keep their existing behavior.

These details are retained without `readBody`, survive client and body-evidence
removal, and are limited to the latest 128 successful responses per browser
connection. `maxBytes` bounds the entire result to 1–65536 bytes, defaulting to
65536; oversized results refuse without truncation. `http_response_remove`
takes `requestId` and releases only these private details, leaving body evidence
and completion receipts intact. Removal is idempotent. A read after removal or
eviction returns `UNKNOWN_HTTP_RESPONSE_DETAILS`; it never sends HTTP. Pending,
discarded and failed fetches have no private response details. Routine response
metadata continues to redact header values and URL queries.

The private result also contains `serverAddr` (socket peer IP and port),
`securityDetails` (TLS protocol, certificate issuer/subject common names and
validity times in epoch seconds) and final-hop `timing`. Plain HTTP has null
security details. Unavailable certificate metadata is null with an explicit
`securityDetailsFailure`; the certificate projection has a 16 KiB bound.
Timing `startTime` is epoch milliseconds; the other fields are milliseconds
relative to that start, or -1 when unavailable. Reused sockets retain their
peer/certificate facts and report no new DNS, TCP or handshake phases.
`readBody:true` observes `responseEnd` after body collection. Without body
collection, that phase is -1 unless the response has no entity body.

Successful CONNECT/SOCKS tunnels expose the connected proxy peer and endpoint
TLS details. Proxy DNS/TCP setup is not a destination phase; an endpoint TLS
handshake can still provide `connectEnd` and `requestStart`. Synthetic CONNECT
refusals have null peer/security fields and unavailable connection phases,
while preserving their actual response status, headers and body.

Creation options also accept `headers`, an explicit `proxy` URL or settings object and
`ignoreHttpsErrors` (default false). TLS uses Rustls WebPKI roots. A null or
omitted proxy inherits the selected browser context's proxy, when present; host
environment proxies are never inherited. Context-wide
headers apply across request origins, including absolute requests outside a
configured `baseUrl`. Per-request headers override them by case-insensitive
name. Each header collection accepts 64 entries within
16 KiB total. Responses have a 16 KiB header limit.

`clientCertificates` accepts up to 16 entries with `origin` and either PEM
`cert`/`key` or a PKCS#12 `pfx` archive, all base64 encoded. An optional
`passphrase` decrypts the private key or archive. Passphrases accept at most 4096
bytes and cannot contain NUL. Input bytes total at most 1 MiB; each normalized
certificate chain and key must also fit that bound. The leaf certificate must
match the private key. Invalid material refuses before context allocation.
Host HTTP key decryption uses the shared four-reader preparation pool outside
browser-state locks. Deadline expiry prevents allocation; a worker keeps its
permit until it exits. An unchanged prepared identity avoids another decryption.
Origins use
the pinned URL-origin normalization, including default ports and discarded paths.
Repeated normalized origins form a selection group. The last identity of each
key type takes precedence; PKCS#12 entries are applied after PEM entries,
preserving order within each format. The client selects a key supporting the
server's signature algorithms and, like the pinned explicit SDK configuration,
does not filter identities by certificate-authority hints. The 16-entry total
bound still applies across all origins. Each redirect selects its destination's
group again, and TLS-policy overrides use separate connections.
Testrun validates certificate material when the client or context is created and
refuses before allocation. The SDK checks the same entry shapes then, but
parses HTTP-client material only on the first request to a matching origin, and
accepts a passphrase-only entry that presents no identity; Testrun refuses both
earlier with `HTTP_CLIENT_CERTIFICATE_*` codes. Origins are not scheme-checked;
a non-HTTPS origin never matches a TLS connection.
An identity does not disable host HTTP server verification or become an HTTPS
proxy's client identity. Routine status and error receipts omit private material.
Chrome, Firefox and WebKit context creation accept the same `clientCertificates`
option; associated host HTTP clients inherit it unless explicitly overridden.
Native Chrome uses an owned private SOCKS/TLS relay for configured origins. It
verifies the selected upstream independently and preserves the browser's offered
ALPN protocols. The original upstream proxy remains the associated HTTP default;
destination identities do not authenticate the HTTPS proxy. Context and connection
cleanup join all private relay sockets, including stalled negotiations; uncertain
native allocation retains its relay until owned cleanup.
As in the pinned SDK, certificate contexts use an internal browser TLS exception
for the proxy. Nonmatching origins remain byte relays under that context's
browser TLS policy and present no configured identity. Plain sibling contexts
retain normal TLS refusal. Public Chrome `ignoreHTTPSErrors` and `userAgent`
creation options remain unavailable. This bounded implementation does not
provide full SDK certificate parity. Pinned WebKit on this Mac does not
present browser-page client certificates to `localhost`; its associated host
HTTP client still inherits the configured identities.

Use `certPath` with `keyPath`, or `pfxPath`, to select certificate files instead
of inline bytes. Relative paths use the CLI caller's or MCP server's working
directory, including shared `connect` and `act` calls; they are bound before
the detached manager receives the request. Inline bytes take precedence over
the corresponding path, including explicitly empty bytes. Selected regular-file
symlinks are followed. Each path has a 4096-byte limit, and the existing one-MiB
aggregate material limit applies to inline and file bytes together. File loading
uses the same four-reader limit and operation deadline as material preparation.
Selected files become immutable inline snapshots before context allocation;
replacement or deletion does not change existing clients or inherited browser
context identities. Missing, non-regular, changing or oversized files refuse
without allocating a context. Routine failures omit selected paths and secrets.
Testrun refuses invalid material before allocation; the pinned SDK can defer
some material errors until the first fetch. Unrestricted file types, exact SDK
error staging, native Chrome browser certificates and full certificate parity
remain open.

The proxy object accepts `server`, `bypass`, `username` and `password`; `server`
is required. These fields each have a 4096-byte limit. HTTP and HTTPS proxies use
CONNECT for HTTP and HTTPS destinations. A missing scheme defaults to HTTP.
Nonempty object credentials replace credentials in the server URL; an empty
object username leaves URL credentials intact. Percent-encoded credentials decode
as UTF-8 and travel only in the private CONNECT Basic header. They do not become
origin headers or response metadata. Explicit caller origin headers remain explicit.
Each proxied request owns its tunnel, without idle tunnel reuse. HTTPS proxy
certificates are always verified, independently of endpoint `ignoreHttpsErrors`.
Host HTTP trusts only the bundled Mozilla (WebPKI) roots for proxies and
destinations; it reads neither the system trust store nor extra-CA settings such
as Node's `NODE_EXTRA_CA_CERTS`, so a proxy signed by a private CA is refused.
A proxy omitting its port uses 80 for `http` and 443 for `https`. A proxy that
closes before replying, sends an empty status line or a header without a colon
fails the request once, without an origin request or retry.

A non-200 CONNECT response can become the HTTP result, including a 2xx status
other than 200. Origin headers/body and destination TLS do not travel over that
refused connection. Status, redirect and `failOnStatusCode` options still apply;
`http_response_read` exposes its original private headers and status text.
This path requires a complete validated head within 16 KiB and 20 headers, and
at most 32 KiB already buffered with that head. It never reads additional bytes
from the refusing proxy. With `readBody:true`, an incomplete buffered body fails
without complete body evidence or an automatic replay. Malformed heads, TLS
failures and other transport errors remain failures. A refused HEAD request with
buffered body bytes remains a protocol failure, matching the pinned client.
These are bounded contracts;
full pinned proxy error-response semantics remain unavailable.

`bypass` is a comma-separated list of case-sensitive hostname suffixes, evaluated
again for each redirect destination. A leading dot is optional; a suffix must
match the whole hostname or a dot boundary. Whitespace is trimmed using JavaScript
rules. Wildcards and port qualifiers have no special meaning.

HTTP clients also accept `socks`, `socks4`, `socks4a`, `socks5` and `socks5h`
proxy URLs, either directly or in `server`. SOCKS4/5 aliases delegate destination
name resolution, as the pinned request helper does; IPv4 and IPv6 framing remain
distinct. Credentials come from percent-decoded URL userinfo. Separate object
`username`/`password` fields are ignored for SOCKS. SOCKS4 sends only its user ID;
SOCKS5 negotiates no-authentication or user/password authentication. Decoded URI
credential fields are limited to 255 UTF-8 bytes, and SOCKS4 user IDs cannot
contain NUL. Destination names are limited to 255 bytes. The default proxy port
is 1080, including an explicit SOCKS port of zero.

SOCKS negotiation shares the original request deadline and observes cancellation
while waiting for a peer reply. Once cancellation is recognized, that handshake
cannot later send origin headers or body. HTTPS destinations still start their
own TLS handshake and retain omission versus explicit `ignoreHttpsErrors:false`.
Browser creation separately accepts `socks4` and `socks5`; other browser SOCKS
aliases and wider SDK protocol error/bounds semantics remain unavailable.
An associated HTTP client can select its own explicit SOCKS proxy.

Browser context creation accepts `proxy:{server,bypass,username,password}` through
`connect --context`, MCP connect's `context`, and `context_create.options`.
It uses the same field budgets, normalizes the server to scheme/host/port and
discards URL userinfo, path, query and fragment. Credentials must be explicit
fields. Browser bypass behavior follows the selected engine; native Chrome forces
loopback through the proxy unless an explicit loopback bypass token is present.
Associated HTTP clients inherit these normalized settings; an explicit client
proxy replaces them. HTTP bypass still uses the suffix matcher described above.

Browser SOCKS4/5 discards URI userinfo and refuses nonempty object `username` or
`password` with `HTTP_PROXY_AUTH_UNSUPPORTED`. Empty object credentials are
accepted. This differs from the HTTP client's URI-authentication behavior.
Native Chrome SOCKS5 launch uses the pinned resolver rule that delegates names
while excluding the proxy endpoint; context proxy settings retain their own
CDP fields. The fixed WebKit backend treats `socks4://` as an HTTP page proxy:
creation succeeds, but navigation fails against a SOCKS server. WebKit SOCKS5
also bypasses loopback page requests; its associated HTTP client uses its
own proxy selection. These engine differences do not authorize an HTTP request
after its source browser context closes.

Browser launch also accepts a top-level `proxy` in MCP `connect` and
`browser_connect`. Both CLI launch forms accept the object through `--proxy`:

```sh
testrun connect --engine firefox --proxy '{"server":"http://127.0.0.1:8080","username":"Acme","password":"example"}'
```

Omitting a context proxy uses that browser's launch default for the initial and
later contexts. An explicit context proxy replaces the whole default object;
its omitted credentials or bypass entries do not inherit individual launch
fields. Associated HTTP clients inherit the resolved context proxy, with their
own explicit proxy override taking precedence. Create another context for a
different context proxy; connect again for a different browser default.
Verified browser closure releases its retained private launch defaults.

Initial defaults for the selected HTTP(S) context or launch proxy follow the pinned
engine: Chrome replaces context origin HTTP
credentials with a nonempty proxy username and its password (omission means an
empty password); Firefox preserves origin credentials. WebKit replaces origin
credentials and merges a `Proxy-Authorization` context header, which also reaches
direct/bypassed requests and associated HTTP origins. WebKit requires an explicit
password for a nonempty proxy username (`HTTP_PROXY_PASSWORD_REQUIRED`); an empty
password is accepted. Header budgets include the injected header. Changing an
associated client's proxy preserves its inherited origin credentials and headers.
Closing the source context fences its associated clients before network access.
Native credential edits refused with `ROUTE_CALLBACKS_PENDING` include bounded
counts of manual callbacks and private automatic continuations, their responding,
deferred-retry and expired states, and normalized failure-code counts. These
details contain no callback authority, request URL or credentials. The refusal
preserves defaults and never retries an uncertain automatic reply.
HTTP preparation keeps an already-started native observation tick alive until
that tick finishes. Preparation completion or caller cancellation
does not drop that reply; cancellation and the original HTTP admission deadline
are checked again afterward, before client allocation or request dispatch.
Already-ready preparation starts no housekeeping work.
Other browser SOCKS aliases, wider SDK bounds and proxy error semantics remain
unavailable. These contracts do not establish full SDK proxy parity.

Both creation options and individual requests accept `timeoutMs`,
`maxRedirects` and `ignoreHttpsErrors`; request values override the client defaults.
An explicit `ignoreHttpsErrors:false` enforces verification even when the client
default disables it. Opposite TLS policies use independent connection pools;
an override applies through redirects and leaves subsequent requests unchanged.
Response metadata reports `tlsVerification` and `tlsPolicyOverridden`.
`timeoutMs` accepts 0–120000. A positive value bounds the whole HTTP request,
including redirects and body retrieval. Zero disables that HTTP-local timeout;
the enclosing Testrun operation deadline and cancellation still apply. A request
can set zero to override a positive context timeout, or a positive timeout to
override a zero context default.
`maxRedirects` accepts 0–20; zero returns redirects without following them.
Exhausting a positive limit returns `HTTP_REDIRECT_LIMIT`. Redirects preserve or
change the method/body according to HTTP status. Authorization is retained for
same-origin hops and removed when the origin changes. Other explicit headers,
including Proxy-Authorization, remain forwarded, matching the pinned APIRequest
client. Every hop replaces Host with the destination host/port and removes the
explicit Cookie header so the selected cookie jar supplies destination cookies.

`http_context_cookies_add` takes `httpContext` and `cookies`, with at most 256
input cookies and 64 KiB of encoded input. Each cookie requires `name`, `value`
and either an absolute URL with a host or both `domain` and `path`. A URL derives
the cookie domain, directory path and secure flag. Optional `expires`, `httpOnly`,
`secure`, `sameSite` and partition fields use the pinned cookie shape. Omitted
attributes in an isolated jar default to session expiry, false, false and Lax.
Associated additions preserve omitted attributes for the browser's defaults.
Adding replaces an existing name/domain/path tuple; past expiry deletes it.
Invalid input shapes, expiry values and unsafe cookie header values refuse before
mutation. An isolated jar capacity failure leaves the previous jar intact.
Expired names keep their insertion position until cookie iteration or filtered
removal empties their group. Isolated retention includes at most 256 cookie/name
groups and 64 KiB of cookie data and empty-name history. A cookie read prunes
expired entries and empty groups before subsequent additions.
Browser-associated additions update the owned browser context's cookie jar.

`http_context_cookies_read` takes `httpContext` and optional `urls` (one absolute
URL or at most 128 URLs). Omission or an empty list reads all cookies. Matching
uses the pinned read rules: domain suffix, path prefix, and HTTPS or local-host
selection for secure cookies. These differ from outgoing Cookie header rules.
The explicit result contains cookie values and is marked `secretBearing`; results
over 64 KiB refuse without truncation. This bounded administration contract does
not establish full SDK parity.

`http_context_cookies_clear` accepts optional `options` with `name`, `domain` and
`path` criteria. Each is an exact string or `{"regex":"pattern","flags":"i"}`.
All supplied criteria must match. Missing options or `{}` clears the whole jar;
null criteria and unknown fields refuse. RegExp flags follow ECMAScript, and
each cookie starts matching with lastIndex zero. Cookie filters support
lookaround, backreferences, Unicode properties and Unicode-set patterns; routing
matchers retain their documented grammar. No-match filters leave cookies intact.
Associated filtering expires only matching cookies in the live browser context.

Filter preparation and matching share the original operation deadline and
cancellation owner. Syntax failures, cancellation or deadline expiry before
mutation leave the jar unchanged. Patterns are at most 2048 bytes with at most
254 capturing groups, flags at most eight bytes and exact criteria at most
64 KiB. At most 4096 candidate cookies and
1 MiB of attribute/filter input are matched on four bounded workers, each with
32 MiB of evaluator memory and a bounded stack. The evaluator has no host I/O
bindings. Inputs are cookie attributes and filter data; caller script execution
is not part of this operation. The installation retains its ECMAScript license
notice with the other browser notices.

`http_context_list` shows client metadata, `activeRequests` and isolated-jar
cookie counts. Requests on one client can run concurrently within the existing
ordinary-operation and 32-pending-response limits. Each request owns its wire
headers, timing, TLS connection facts and cancellation state. Responses settle
independently, and response cookies update the shared jar as they arrive.
The client retains one idle transport lane after concurrent work finishes.
Associated clients identify their browser context; their cookie counts are null.
Each isolated client retains
up to 256 cookies and 64 KiB of Set-Cookie header data, pruning expiry and
evicting the oldest update first. Set-Cookie entries beyond 4096 bytes are dropped; listings report
evictions and drops. The SDK's jar and bodies are unbounded; Testrun's cookie,
header and body limits are deliberate and refuse or report explicitly. Use `http_context_cookies_clear` to clear that jar and
`http_context_remove` to release an idle client. Both take `httpContext` and
refuse with `HTTP_CONTEXT_BUSY` while a request is running. Clearing an isolated
jar leaves browser cookies unchanged; clearing an associated jar changes that
browser context. Client removal preserves browser cookies.

Use `http_context_dispose` with `httpContext` and optional
`options:{"reason":"Acme request context closed"}` to close a client's public
request/body lifetime. An isolated client cancels its exact owned requests and
releases idle connections. An associated client lets requests already dispatched
finish; they can still return response metadata, while later body reads and new
calls fail as disposed. Disposing an associated client leaves its browser
context and cookies intact. Closing the parent browser context or disconnecting
cancels its remaining owned HTTP work. An acknowledged browser context close
disposes associated HTTP clients before retiring the context reference, even
when its lifecycle event arrives later. New calls then report
`HTTP_CONTEXT_DISPOSED`. Actual worker leases and pending response
finalization still govern cleanup; disposal does not assert that they exited.

`http_context_list` reports `disposed` separately from active work. A disposed
client retains its slot until `http_context_remove` releases it while idle.
`http_context_read` takes `httpContext`, optional `includeReason` (default false)
and `maxBytes` (1–65536, default 65536). Reasons appear only in an explicit
private `includeReason:true` read. `initialReason` retains the first disposal's
reason; `fetchReason` follows repeated disposal, including clearing it when the
reason is omitted. Reasons are bounded to 64 KiB of UTF-8; the entire encoded
read must fit `maxBytes`, without truncation.

For responses fetched with `readBody:true`, `http_response_body_read` takes
`requestId`, `format` (`base64`, `text`, `json` or `json_value`) and optional `maxBytes`
(1–65536, default 65536). It reads through the public response lifetime.
`http_response_dispose` takes `requestId` and ends that response's body access;
repeated calls on an open client are harmless. Client disposal also ends body
access for completed and later-arriving responses. Body reads then fail with
`HTTP_RESPONSE_DISPOSED`; disposing a response through a closed client fails
with `HTTP_CONTEXT_DISPOSED`. Private metadata from `http_response_read` and
explicitly captured `http_evidence_read` bytes remain independently owned until
their own removal. Disposing a client or response never deletes that evidence.

Requests can mutate remote state. `HTTP_REQUEST_OUTCOME_UNKNOWN` means a request
may have reached its server. Request `maxRetries` defaults to zero and accepts
0–20; an explicit positive value permits another complete attempt only after a
connection reset, within the original deadline and cancellation owner. A retry
can repeat a mutation that reached the server. Results and failures report
`retryCount` and `replayed`; HTTP statuses, body bounds and other transport
failures do not trigger retries. The browser remains available for inspection.
Caller wait expiry does not abandon the owned request.
Use `http_evidence_list` to find retained responses by `requestId`.
Deadline expiry returns `HTTP_RESPONSE_PENDING` with the retained `requestId`;
native cancellation also retains that ID. Cookie acknowledgements already in
flight can finish after the caller stops waiting.
Cancellation interrupts only that request's owned HTTP sockets and leaves its
siblings running. A completed retained request cannot close a later request
that reuses its transport lane. Active worker ownership remains visible until
the worker exits; cancellation and cleanup receipts retain pending work honestly.
`http_response_finalize` retries a retained response's local artifact write
without issuing HTTP; `http_response_discard` releases a completed pending
response. An active request returns `HTTP_RESPONSE_PENDING` to either operation.
`http_evidence_remove` takes `evidenceId` and refuses changed files.

A connection owns at most 16 HTTP clients, 32 pending responses and 32 HTTP body
artifacts. Completion witnesses retain the latest 128 request results; captured
body witnesses remain with their artifacts. Response bytes survive client
removal and failed file writes until finalization or explicit discard. Browser
disconnect attempts finalization and reports pending cleanup if it cannot write
retained bytes. Captured files remain on disk after disconnect.

## Native Chrome investigation evidence

`developer_open` takes an owned `page` and an optional native DevTools `panel`.
Use the returned `developer` reference with `developer_status` and
`developer_close`. A frontend that was already open is borrowed; closing the
Testrun attachment preserves that frontend. Chrome uses its own bundled modules
through the existing Rust CDP connection. Firefox/WebKit refuse these native
developer operations with `NATIVE_DEVELOPER_UNSUPPORTED`.
There is one attachment per page and at most four per connection. Closing an
attachment still initializing returns `DEVELOPER_BUSY`; its allocation remains
owned until it settles. Repeated completed closure preserves its witness.

`page_audit` takes `page` and optional `options` with `mode: "navigation"` or
`"snapshot"`, `device: "desktop"` or `"mobile"`, `includeNodes` and `limit`
(1–100, default 20). Chrome's bundled Lighthouse audits accessibility, SEO,
best practices and agentic browsing. Its accessibility category runs axe-core
over the whole page; there are no separate axe rule-tag or element-scope
options. Navigation mode reloads the application;
snapshot mode audits the current page. Results include category scores and a
bounded failure summary. Full private JSON and standalone HTML reports contain
unredacted application data. An audit requires its own frontend, restores page
geometry, and refuses an existing frontend with `AUDIT_FRONTEND_CONFLICT`.
Runtime audit errors preserve any finalized report metadata.

`profile_start` takes `page` and optional `options` with `cpu` (default true),
`javascriptCoverage`, `cssCoverage`, and `samplingIntervalUs` (100–100000,
default 1000). Run the application, then use `profile_stop` with that page.
`profile_status` reports collection/finalization state. Private artifacts use
the actual `v8-cpu-profile` (`.cpuprofile`), `v8-precise-javascript-coverage` and
`cdp-css-rule-usage` JSON formats by default. This native format is cumulative
across navigation in the selected page target and omits source texts; CSS includes
bounded stylesheet identity metadata. These are engine coverage ranges, not assertion results.
Each result has an 8 MiB bound; finalization retains pending values for retry.

To find where the CPU time went, read the saved profile with
`{operation:"profile_query", evidenceId:UUID, query:{kind:"functions"}}`, using
the `v8-cpu-profile` report's `evidenceId`. Each item is one function
(`functionName`, the URL without query or fragment, and zero-based `lineNumber`
and `columnNumber`) with `selfMs`, `totalMs`, their percentages of the sampled
time, and `selfSamples`. `sortBy` is `"self"` (default) or `"total"`. Total time
counts a recursive function once per call stack, as the DevTools bottom-up view
does. Pages use `offset` and `limit` (1–100, default 50) and return
`nextOffset`, null on the last page. `timing:"sample-timeline"` means times come
from the profile's sample timestamps; `"hit-count-estimate"` spreads the profile
duration over hit counts when the timeline is absent. A report in another format
is refused with `PROFILE_EVIDENCE_FORMAT`. The summary is private: function names
and code locations come from the page.

With JavaScript coverage enabled, `javascriptCallCount` and `javascriptDetailed`
select precise call counts and block-level ranges (both default true).
`reportAnonymousScripts:false` excludes entries with an empty URL; true includes
them, and omission preserves the native raw result. These options require
`javascriptCoverage:true`.

For Playwright entry shapes in an `{entries}` report envelope, set
`coverageFormat:"playwright"`. JavaScript
reports contain `{entries:[{scriptId,url,functions,source?}]}` with the native
function/range counts. CSS reports contain `{entries:[{url,text,ranges:[{start,end}]}]}`
with disjoint used spans, excluding one-code-unit spans as the pinned SDK does.
Offsets remain UTF-16 code units; they are not UTF-8 byte positions. Reports use
`playwright-javascript-coverage` and `playwright-css-coverage` format labels.
This format defaults `resetOnNavigation` and `includeSourceTexts` to true and
`reportAnonymousScripts` to false. Explicit booleans override those defaults.

`resetOnNavigation:true` forgets script/stylesheet identities on the target's
execution-context-clear event. False retains the earlier identity/source catalog.
Returned counters still follow the native engine's lifetime: V8 can omit a
collected pre-navigation script, and retained CSS sources can have empty ranges.
Neither setting drains counters merely to reset a window. `includeSourceTexts:true`
retains private source texts as scripts/stylesheets appear, so a later navigation
cannot substitute a newer source for an old identity. Both options also work with
the native format; native CSS adds a `sources` map keyed by stylesheet ID.
Source collection retains at most 512 identities and 4 MiB of JSON-encoded texts
per navigation window (or per profile without resets), with four concurrent
reads and at most 32 pending reads. A failed read, lost identity or exhausted
bound marks coverage incomplete; `coverageSourceComplete` reports that fact.
Missing JavaScript text remains an entry without `source`; SDK CSS omits sheets
whose requested text is unavailable. If the whole coverage result exceeds 8 MiB,
Testrun publishes an incomplete `native-coverage-capture-status-json` report with
the omitted result's byte count. Delta takes leave collectors running; final stop
can release them without retrying the oversized result. No counter read is
repeated, and the status report does not contain coverage data.
Coverage's debugger ownership conflicts
with `page_debugger` enable and reports `COVERAGE_DEBUGGER_CONFLICT`; stop the
current owner first. CPU profiling remains independent of the coverage format.

Use `profile_take_coverage` with `page` to drain a JavaScript/CSS coverage window
while the collectors and CPU profiler keep running. Each enabled coverage type
produces its own private delta artifact; the next take begins another window.
Final coverage reports contain the window since the last take. CPU sampling
continues until `profile_stop`. Delta artifacts are independent of
the final profile reports and use `v8-precise-javascript-coverage-delta` or
`cdp-css-rule-usage-delta`; Playwright-shaped reports use the corresponding
`playwright-javascript-coverage-delta` and `playwright-css-coverage-delta` labels.
Release them with `developer_evidence_remove` when no
longer needed. The shared 32-artifact limit is reserved before counter reads.

A failed local write retains the drained bytes. Retry `profile_take_coverage`
to finish that capture, or use `developer_evidence_finalize` with its `captureId`;
neither repeats native reads. `profile_status.coverageCaptureId` identifies the
pending capture. An uncertain drain instead refuses further takes with
`PROFILE_COVERAGE_UNCERTAIN`; stop the profile. Partial artifacts remain explicit,
and stop does not replay the uncertain JavaScript take. Source texts and URLs are
private report content, including query strings; ordinary status omits them.

`trace_start` takes `context` and optional `options` with `screenshots` and
`cpuSampling` (both false by default). Chrome tracing covers the owned browser
process; the context identifies the selection and does not filter other owned
pages out of the trace. Use `trace_status` and `trace_stop` without a page.
The private Chrome trace-event JSON artifact is bounded to 32 MiB, with explicit
source-loss/truncation metadata. Chrome DevTools Performance and Perfetto read
complete traces. This format is separate from Playwright Trace Viewer archives.
With a live page in the selected context, tracing retains that page's verified
tab attachment for frame metadata and keeps trace control and stream reads on
the same session. `trace_status.metadataPage` identifies the metadata anchor.
Native `maxDurationMs` selects 1000–300000 ms measured from the acknowledged
capture start; omission keeps manual stopping. The actor attempts finalization
at expiry while idle, during navigation waits and during ordinary pending RPCs.
This is a capture deadline, with a separate bounded finalization attempt. A late
stream remains `finalization_pending`; inspect status and use `trace_stop` to
finish the original capture. Automatic finalization does not retry failed reads
or repeat `Tracing.end`.

Native `reload:{ignoreCache:true}` reloads the metadata page after trace start
and waits for load. `reload:{}` uses `ignoreCache:false`; omitting `reload`
does not navigate. Cache bypass applies to that reload without changing the
browser's global cache setting. A live page is required before capture allocation.
A failed reload reports `traceStarted:true` and leaves the capture owned for
status/stop; it does not repeat navigation. Firefox/WebKit refuse `reload`.

Native `options.page` selects an exact owned page in `context` for trace metadata
and optional reload. It does not change the selected page or restrict the
process-wide trace. Omission uses the selected page in that context, then its
first page. A stale page or a page from another context is refused before capture.
Native trace capture requires a Testrun-owned Chrome launch. Attached Chrome
sessions refuse `trace_start` before any setup or reload.

Native `autoStop:true` requests finalization five seconds after trace start and
any requested reload finish. `autoStop:false` keeps manual stopping. Omission
preserves the raw trace duration defaults above. Do not combine `autoStop` with
`maxDurationMs`; that reports `TRACE_DURATION_CONFLICT`. A failed reload leaves
the acknowledged capture owned without arming this five-second timer; stop it
explicitly. Firefox/WebKit refuse `page` and `autoStop`.

For the native performance lifecycle, pass `options.performance:{}`. Its
`reload` and `autoStop` booleans both default to true. Reload saves the exact
owned page URL, navigates to `about:blank` and waits for load before recording,
then navigates back and waits for load. The five-second finalization window
starts afterward. `performance:{reload:false,autoStop:false}` keeps the current
document and records until explicit stop. Selection remains unchanged. A failed
reset reports `traceStarted:false`; a failed return navigation reports
`traceStarted:true` and keeps capture owned without arming the timer. Neither
failure replays navigation. The preset cannot be combined with raw `reload`,
`autoStop` or `maxDurationMs` (`TRACE_PERFORMANCE_OPTIONS_CONFLICT`). Firefox and
WebKit refuse it before capture. Raw options retain their defaults.

The preset includes the pinned DevTools default categories, JavaScript CPU
sampling and screenshots, including source rundown data. These private traces
can contain application sources and pixels. The existing 32 MiB artifact bound
and 16 MiB recording buffer still apply; loss or truncation prevents a complete
capture claim. Set `performance.ignoreCache:true` to bypass the selected page's
HTTP cache during the return navigation. It defaults false and has no effect
with `reload:false`. Caching is restored after success or failure. If restoration
fails, status reports `cacheRestorePending:true` and the capture stays owned;
use `trace_stop` to retry restoration and finish the capture without replaying
navigation. Automatic analysis and the full conditional pageId workflow remain
separate contracts.

For a caller copy of a complete native trace, set `trace_start.options.filePath`
or `trace_stop.filePath`. The stop path overrides the start path. Relative paths
resolve against the invoking CLI or MCP process's working directory. Dot segments
are normalized before existing ancestor aliases are resolved. The last extension
is replaced with `.gz` when the resolved input ends in exact lowercase `.gz`,
otherwise `.json`: `Acme.txt` becomes `Acme.json`, `Acme.backup.GZ` becomes
`Acme.backup.json`, and `.gz` becomes `.gz.gz`. JSON preserves the private evidence
bytes; gzip compresses those same bytes. The `export` receipt includes the actual
canonical destination, byte count, SHA-256 and original evidence ID.

Use a directory owned by the current user with no group/other write permission,
or a sticky directory owned by the current user or root. Existing readable
parents such as mode 0755 and sticky shared parents such as 01777 are supported;
output files retain mode 0600 and existing parent permissions stay unchanged.
Non-sticky parents writable by other users refuse because they could replace
the publication temporary. Missing parent directories below an accepted ancestor
are created with mode 0700 only when complete, verified evidence is published;
start and destination validation create nothing. URL inputs, control characters,
inputs over 4096 UTF-8 bytes and existing destination files refuse.
Publication never overwrites a file.
This bounded path contract does not implement MCP negotiated-root authorization
or all pinned filename semantics.
Firefox/WebKit refuse these native export options before capture or stop.

An invalid stop destination leaves recording active. A destination that becomes
unavailable after start can fail after capture completes: the error and status
retain the complete `report` and an `exportPending` reason. Retry `trace_stop`
with a new `filePath`, or resolve the destination problem and retry without one.
This reads the retained immutable evidence and never recollects the trace.

For stop-time native analysis, set `trace_stop.analysisPage: PAGE_REF` to the
trace's exact metadata page. Capture and any export finish before Testrun reuses
that page's live owned DevTools frontend or opens one. The result names the
frontend in `analysisFrontend`; close it explicitly with `developer_close`
after releasing its model. Analysis uses ordinary operation capacity.
An existing unowned frontend refuses after discovery, before opening, switching
or attaching to it. Explicit developer-session operations retain their own
attachment and cleanup behavior.

Alternatively, `analysisDeveloper: DEVTOOLS_REF` selects an already owned
frontend for that same page. A borrowed frontend or another page refuses before
recording ends. The two analysis options are mutually exclusive; omission or
null for both requests the raw stop. Chrome can lose trace data while a
performance preset records with a DevTools frontend open. When Chrome reports
that loss (`nativeCompletion.dataLossOccurred:true`), analysis refuses with
`TRACE_INCOMPLETE` and the result keeps the incomplete report and its
`sourceLossReasons`. To analyze reliably, record without an open frontend and
let `analysisPage` open one after capture.
After successful capture and any export, the result includes `analysis` with the
same evidence ID, an analysis ID and the bounded native summary. Use that ID with
`performance_query`, and release it with `performance_release`.

Analysis failure preserves the complete report and records `analysisPending`,
including any retained model's cleanup handle. Release a retained model before
retrying `trace_stop` with the selected analysis option. The retry loads the same verified
evidence without another trace end/read or capture allocation. Raw stops preserve
the pending-analysis annotation; they do not request another parse. A successful
analysis receipt replays after model/frontend release and does not reopen them.
Frontend-allocation errors also retain complete evidence and frontend cleanup
handles. An uncertain creation without a known frontend refuses another automatic
allocation; inspect retained owners and use explicit frontend recovery or browser
cleanup. Its automatic-allocation reservation survives later captures until
owned browser cleanup. No retry repeats trace collection.
An `autoStop` timer finalizes the raw capture; it does not analyze it. To
analyze a timed capture, call `trace_stop` with `analysisPage` or
`analysisDeveloper` after `trace_status` reports it complete. Analysis results
are structured JSON, not formatted text.
Keep the trace evidence until any requested export and analysis finish.
Incomplete captures remain explicit
errors and are not exported. An automatic stop uses the start path. Starting a
new capture replaces the latest stop/export receipt; older evidence keeps its
own explicit retention lifecycle.

Native capture status and terminal reports expose `nativeCompletion.received`
and its `dataLossOccurred` flag. The flag
is null before a valid native completion arrives; false means Chrome explicitly
reported no loss. `sourceLossReasons` independently records observed native
data loss, an event gap, metadata-session loss, stream-read failure or finalization
after browser exit. These observations remain in retained reports and repeated
stop receipts. A false native flag does not make an interrupted capture complete.
Trace category exclusions are forwarded separately from included categories;
the raw and performance preset both exclude unselected default categories.
Native status retains the latest terminal receipt with `active:false`, and
`trace_stop` returns that same result, including an incomplete-capture error,
until another capture is admitted. Report metadata records `stopReason` as
`requested`, `duration`, `auto-stop`, or `owned-close`. Repeated stop performs no further
browser commands or artifact writes, unless an export is pending or a new export
destination is explicitly requested. Repeating an already successful destination
returns its latest receipt without writing again. Evidence removal remains explicit.
At most four such tab attachments are retained; tab loss and connection teardown
release their ownership. A context without a live page can record raw process
events, but those events may lack the frame metadata required for native insights.

Firefox/WebKit use context tracing through the same `trace_start/status/stop`
operations. They export real private `playwright-trace-zip` archives for
Playwright Trace Viewer. Options include `screenshots` for screencast frames,
`snapshots` for DOM/network snapshots, and `maxDurationMs` (1000–300000,
default 30000). Screenshots and snapshots default to false. `cpuSampling` is
unsupported there. Native Chrome refuses snapshot archive options and
chunk/group operations before collection; its trace format remains Chrome JSON.

`snapshots:true` selects DOM/network snapshots. For separate selection, pass
`snapshots:{dom:true,aria:true,screen:true}`; omitted object fields are false.
ARIA snapshots and per-action screen images are separate from the `screenshots`
screencast timeline option. These captures include private page content.
Optional `name` selects an intermediate-file prefix, and `title` labels the first
chunk in Trace Viewer. Prefixes accept 1–128 ASCII letters, digits, underscores
or hyphens; output artifacts keep their owned private paths. Native Chrome
refuses these Playwright naming options before collection.

`live:true` selects Playwright live recording, including script response bodies
when DOM/network snapshots are enabled. It defaults to false. Testrun publishes
immutable completed chunks; it does not expose a live-reader endpoint.

To include source-file attachments, set `sources:true` and select `sourceFiles`
as absolute regular-file paths. Up to 16 distinct paths are accepted, with
256 KiB per file and 2 MiB total. Relative paths, final-component symlinks,
non-regular files, missing files and detected changes while reading refuse
before tracing starts. Selected bytes are snapshotted at start and reused in
every chunk, even if the original is later changed or deleted. `sources:true`
requires a nonempty selection; sourceFiles require sources:true. Source selection
defaults off. Automatic caller-stack/source discovery is unavailable.

Attachments use Trace Viewer's `src/<SHA-1-of-supplied-path><extension>` naming.
The digest identifies a viewer entry; artifact integrity uses the existing
SHA-256 checks. Routine status/report metadata gives the selected count without
file paths or source bytes. Private archives include the selected bytes and any
explicit source locations. Native Chrome refuses sources/live archive options.

Starting a Playwright trace opens its first chunk. Use `trace_group_start` with
`name`, then `trace_group_end` to label related calls (up to 32 nested groups).
An optional `location:{file,line,column}` records a group source location in the
private archive. File accepts 1–1024 UTF-8 bytes without control characters;
line/column are optional nonnegative 32-bit signed integers. A location does not
authorize reading that file: select it separately in sourceFiles to attach its
contents. SDK disposable objects are not exposed.
`trace_chunk_stop` exports that chunk and closes outstanding groups while keeping
the context trace configured. `trace_chunk_start` opens another, with optional
`name` and `title`; the title defaults to "Testrun investigation". Stop the active
chunk first. Chunk prefixes use the same safe-name rules as trace start. Titles
and group labels accept 1–256 UTF-8 bytes without control characters. One connection
owns at most one trace and four chunks per capture.
`trace_stop` exports the active chunk, stops collection and returns the retained
report metadata; repeated stop returns the same result.

Each ZIP has a 32 MiB read/export bound. The private source directory has a
64 MiB/4096-entry budget checked between commands and at 250 ms intervals.
Duration expiry requests finalization; an in-flight driver command must settle
before its trace can stop. These are collection checks rather than filesystem
quotas. A source-budget failure discards the unfinished chunk and emits an
incomplete capture-status JSON artifact. Caller timeouts return
`TRACE_COMMAND_PENDING`; inspect `trace_status` instead of repeating setup or a
chunk command. `setupSettled` witnesses initial setup; `pendingCommand.id` later
appears as `lastCommand.id` with its outcome and exported report metadata.
Context closure and disconnect retain finalization ownership.
An uncertain stop keeps `cleanupPending` and requires owned disconnect.
Archives contain private application/network data and explicitly selected source
content. They do not establish Playwright Test assertion coverage.

On Firefox/WebKit, `context_debugger` controls automation execution for an owned
`context`. Debugging is API-only: headed browsers do not open Playwright's
Inspector window when execution pauses. Submit `action:{kind:"pause"}` to request a pause before the next
pausable driver call, then poll `action:{kind:"status"}` until `paused:true`.
Arming a pause does not stop JavaScript or prove a call has paused. Status reports
`stateKnown:false` and `paused:null` before observation or after source loss.
`pauseId` identifies the current observed pause. Use
`action:{kind:"next",pauseId:PAUSE_ID}` to resume it and pause at the next
automation call, or `{kind:"resume",pauseId:PAUSE_ID}` to continue. These controls
remain responsive while a page action is waiting. A Testrun action can contain
several caller driver calls; stepping follows those calls rather than JavaScript
lines. Host actionability and condition probes, and typed `page_read`
observations, use internal SDK metadata: they neither consume pause/next/run-to
requests nor enter automation call collection or SDK trace action records. Caller `evaluate` and
`selector_evaluate` calls remain visible to the automation debugger.

On Firefox/WebKit, caller page/frame/handle evaluation and selector evaluation
can run while an ordinary input is paused or awaiting application readiness.
They retain caller control admission, document/handle lifetime checks and normal
operation capacity. They do not consume the reserved callback/control slots.
An `act` batch containing only evaluations runs independently of held input,
with its own actions still ordered and stopping on failure. Mixed action batches
retain ordinary serialization. Evaluation does not resume the held input or
release its reservation. This contract does not establish native Chrome debugger
concurrency or full debugger parity.

Caller page/frame/handle evaluation, selector evaluation and `handle_read` can
also complete while an ordinary native Chrome action waits for application
readiness. Native caller execution shares the 32-slot ordinary admission budget;
an active independent call retains its slot until settlement, and mixed batches
remain on the ordinary lane. Handle disposal uses the reserved response lane,
so owned handles can be released during that wait even when normal capacity is
full. Queued execution rechecks cancellation, deadlines and caller control before
dispatch. These calls do not complete or release the original action; make its
application condition ready or cancel it explicitly. Firefox/WebKit handle reads
also run independently of held input. Wider native debugger, resource-bound and
execution/subscription contracts remain under investigation.

Acknowledged Pause/Next requests dispatch the real caller SDK action before
an additional host actionability preflight. With call collection enabled, the
waiting action is visible as running; the SDK waits for readiness and then
pauses before input. With collection disabled, the SDK pauses before readiness.
A matching
`run_to` location also pauses before that wait, including when collection is
enabled. Resume then lets the same strict, non-forced SDK action perform its
readiness checks; a hidden element still waits. Input reservations remain owned
through the pause, and a failed or unknown input reply retains the existing
uncertainty fence. Enabling collection does not make a hidden or disabled
element actionable, nor does a reservation prove a physical press.

With acknowledged call collection enabled, ordinary page actions and nonmatching
`run_to` actions also expose their running caller before SDK readiness. They
continue without a pause unless a pending control matches them. The same strict,
non-forced SDK action owns readiness and execution; other calls retain their
host actionability preflight. Unknown or pending controls retain their guards.

A shared-act batch containing only `context_debugger` actions can reach a held
caller independently. Its actions retain their order, ordinary admission
capacity and exact pause/controller checks. It does not borrow reserved control
slots. Batches that also contain ordinary actions keep ordinary ordering.

Ordinary status omits action titles and source details. Explicit
`action:{kind:"status",includeDetails:true}` discloses private automation title
and location metadata, each clipped to 256 Unicode characters. It is not a
JavaScript stack. Each new pause has a fresh ID; old IDs refuse without dispatch.
An unknown control reply retains `pendingControlId` and `lastControl`; inspect
them and close the owned context or disconnect instead of repeating a resume.
Context closure retires paused input and preserves sibling contexts; an
unreceived input reply keeps its unknown outcome and remains owned by the
connection. The driver releases a retained SDK automation pause only after its
whole context has closed, allowing the original SDK call to settle without
continuing input in a live page or worker. A stale debugger reference alone is
not evidence of that release. `automationDebuggerCleanup` retains lost cleanup
replies until disconnect. Source loss
invalidates pause identities. Native Chrome uses `page_debugger` for its separate
JavaScript debugger and refuses `context_debugger`.

On Firefox/WebKit, annotate an individual `page_action.action` with its caller's
source location:

```json
{"kind":"at_source","source":{"file":"Acme-journey.rs","line":30},"action":{"kind":"click"}}
```

Through the shared `act` tool, that page action uses
`input:{action:"at_source",source:{file:"Acme-journey.rs",line:30},input:{action:"click"}}`.

From an observed pause, submit
`action:{kind:"run_to",pauseId:PAUSE_ID,location:{file:"Acme-journey.rs",line:30}}`.
This resumes the paused call and arms a pause before a future annotated action
whose filename contains that file fragment and whose supplied line/column match
exactly. Other locations
continue. Poll for a fresh `pauseId`; acknowledgement alone does not mean the
target was reached. Locations are caller-supplied metadata. The filename accepts
up to 1 KiB of UTF-8; line and column are optional nonnegative 32-bit integers.
Nested annotations refuse. Native Chrome reports `CALL_SOURCE_UNSUPPORTED` for
these SDK annotations.

To collect automation-call diagnostics on Firefox/WebKit, submit
`action:{kind:"enable"}` once. Collection lasts until the context closes.
Enabling requires the current browser-control lease; reads remain available
after that lease is released.
Read `action:{kind:"calls",offset:0,limit:16}` for latest call IDs and statuses;
add `includeDetails:true` for private titles, driver-supplied source locations,
errors and incremental log entries. These are automation diagnostics, not a
JavaScript stack. Retention is limited to 32 calls and eight log entries per call,
with strings clipped to 256 Unicode characters. Results report eviction and
truncation. Calls are ordered by their latest update; pagination can change as
calls progress and does not provide a complete history. Source loss clears this
cache and fences reads. An unknown enable reply remains owned; inspect status
and close the context instead of repeating enable.

`developer_evidence_list` lists audit, profile, trace and heap artifacts.
It also reports reserved capacity and pending writes. Use
`developer_evidence_finalize` with a pending `captureId` to retry writing retained
report bytes without rerunning the audit. Owned shutdown attempts finalization;
an I/O failure keeps cleanup pending. Interrupted traces preserve an explicit
`chrome-trace-event-json-prefix`, or a capture-status JSON artifact when no trace
bytes were received. These incomplete artifacts identify their actual format.
`developer_evidence_remove` takes `evidenceId`, verifies the recorded file and
digest before deletion, and refuses changed files. A connection retains up to
32 developer artifacts and 128 MiB in total. Each Lighthouse report is bounded
to 16 MiB. Closing the owned browser preserves evidence on disk.

`page_debugger` takes `page` and an `action`. Start with
`{kind:"enable",exceptions:"none"}` (`uncaught` and `all` are also supported).
`{kind:"status",includeFrames:true}` returns bounded stack locations and the
current `pauseId`; ordinary status omits function names. Use `{kind:"pause"}`
to request a pause. Resume with `{kind:"resume",pauseId:UUID}`, or step with
`{kind:"step",pauseId:UUID,step:"into"}` (`over`/`out` are also supported).
Each pause has a fresh identity; an old identity cannot resume a later pause.
Status, evaluation and resume remain responsive while an application evaluation
is stopped at a breakpoint. `{kind:"evaluate",pauseId:UUID,frame:0,expression:"local"}`
returns the structured evaluation format and may mutate the paused application.
`{kind:"disable"}` releases the debugger and resumes execution.

Status also returns `sessionId` and, with frames, scope indices. Read a scope with
`{kind:"variables",pauseId:UUID,frame:0,scope:0,offset:0,limit:20}`: bounded own
descriptors, skipped getters and private primitive values, without evaluating an
expression. Temporary `{kind:"logpoint_set",url:URL,line:0,marker:"Acme:entered"}`
uses a literal JSON console marker and returns an owned removal ID. Both retain
pause/controller fencing as appropriate; same-page profiler/debugger ownership
is exclusive. See [interactive debugging](diagnostic-interactive.md) for bounds,
execution effects, cleanup and diagnostic build/runtime-check reports.

Set an exact URL breakpoint with `{kind:"breakpoint_set",url:URL,line:0,column:0}`;
line/column are zero based and an optional `condition` is JavaScript. Remove the
returned UUID with `{kind:"breakpoint_remove",breakpointId:UUID}`. Native debugger
IDs stay private. The API retains at most 128 breakpoints and 100 stack frames
per page, with bounded inline stack output. These controls operate Chrome's
JavaScript engine, separately from Playwright's automation debugger.
Through the shared `act` tool, wrap a debugger action as
`{action:"page_debugger",page:REFERENCE,input:{action:"status",includeFrames:true}}`.
Direct `browser_send` requests use the `action: {kind: ...}` form above.

### Inspect a heap snapshot

Chrome's `heap_capture` takes `page` and optional `maxBytes` (16 MiB by default,
1024–33554432). Capture can trigger garbage collection and pause application
execution. The result contains a private `v8-heap-snapshot-json` artifact, not
inline heap strings. Snapshots can contain credentials and application data.
Loss, interruption or a byte limit preserves a distinctly labelled
`v8-heap-snapshot-json-prefix`; incomplete artifacts cannot be loaded for analysis.
Failed local writes use `developer_evidence_finalize` without another capture.

Open `developer_open` for the same page, then use `heap_load` with its `developer`
reference and the capture's `evidenceId`. It verifies the file and digest and loads
Chrome's bundled DevTools heap worker. Keep the returned `snapshotId`. A connection
retains at most two loaded models; models end with their DevTools attachment.

For an existing snapshot file, `heap_import` takes `developer`, an absolute `path`
and optional `maxBytes` with the capture bounds above. The input must be an
uncompressed UTF-8 file, owned by the current user, with private file and parent
directory permissions. Declared node, edge and allocation-trace counts must match
their numeric arrays; JSON nesting is limited to 128 levels. Testrun copies the
bounded bytes into private evidence;
later source edits do not change that copy. Use the returned report's `evidenceId`
with `heap_load`. Import records the input; the bundled worker validates its
snapshot structure during loading. Imported evidence belongs to the selected
developer attachment for analysis and does not claim that attachment captured it.

Imported models report `identity.verified: false`. They cannot be compared with
captured models. Comparing imports requires the same explicit `identityGroup`
UUID on each `heap_import`: this is your assertion that their object IDs share
identity, and comparison results remain labelled unverified. Without that
assertion, comparison returns `HEAP_INCOMPARABLE`. Removing imported evidence is
separate from model release; failed local publication uses
`developer_evidence_finalize` with its retained `captureId`.

`heap_query` takes `developer`, `snapshotId` and `query`. Query kinds are `summary`,
`objects`, `object`, `edges`, `retainers`, `retaining_paths`, `dominators`, `classes`,
`class_nodes`, `duplicate_strings` and `contexts`. `objects` accepts `className`, `propertyName`,
`nodeType` and `isDetached` filters, inclusive `retainedSize`/`selfSize` byte ranges
such as `{"min":1024,"max":4096}`, and `sortBy` (`retainedSize`, `selfSize` or `id`).
Omit either range endpoint for an open range. Matching and sorting use the bundled
heap model. `edges` accepts `sortBy` (`retainedSize`, `selfSize` or `name`),
`minRetainedSize` in bytes, and `excludePrimitives` (default true). Edge sorting
defaults to retained size; this native provider supports a lower size bound.
`contexts` accepts `scopeInfoNodeId`, an inclusive `retainedSize`
range, `offset` and `limit`. It ranks matching contexts by
`deadFieldsRetainedSizeSum` before pagination and groups the selected contexts
under their original `scopes`. This score is a retention heuristic, not an
estimate of reclaimable memory; other analysis metadata describes the source
snapshot rather than just the selected page.
`classes` sorts by descending retained size and reports `objectCount` and
`totalSelfSize` across the filtered classes before pagination. It returns a
numeric `classId` for each selected row. Use
`{"kind":"class_nodes","classId":1,"offset":0,"limit":10}` to inspect its
instances within that same `snapshotId`. Selectors preserve the complete internal
class key even when display text is truncated. A model retains at most 100000
selectors and 8 MiB of class-key bytes; releasing the model ends those selectors.
Both `classes` and `class_nodes` accept `filterName`:
`objectsRetainedByDetachedDomNodes`, `objectsRetainedByConsole`,
`objectsRetainedByEventHandlers`, `objectsRetainedByContexts`,
`sharedNativeContext`, `noNativeContext` or
`attributedToSpecificNativeContext`. The last requires `objectId`, the snapshot
ID of the native context to attribute objects to. Testrun resolves that ID in the
selected model before applying the native filter. Other filters do not require it.
Object/edge/retainer/path/dominator queries take the numeric
`nodeId` from an object result. Paginated queries accept `offset` (0–100000) and
`limit` (1–100, default 50). For example:

```json
{"operation":"heap_query","developer":{"connectionId":"CONNECTION-UUID","id":"DEVELOPER-UUID","generation":"GENERATION-UUID"},"snapshotId":"SNAPSHOT-UUID","query":{"kind":"objects","className":"AcmeRecord","limit":10}}
```

Query results expose private heap content explicitly. They are bounded to 64 KiB,
with bounded strings, nesting and item counts and a `truncated` indicator. The
byte budget also counts JSON escapes and UTF-8 text. A truncated result may omit
graph branches or page items. Use a page's `nextOffset` to continue without skipping
omitted rows, or request smaller pages or search limits for more detail. Context
pages expose each context's global `rank` and resume at the first missing rank.
The operation's `maxNameLength` controls displayed names from 1–1024 Unicode characters
(default 100); node types and numeric identities remain intact. Retaining-path
queries accept `maxDepth` (1–32), `maxNodes` (1–5000) and
`maxSiblings` (1–100). Omitted limits use the bundled model's defaults.
These are DevTools'
native snapshot graph and retention calculations. Captured objects describe that
snapshot's time, not the application's current state.

HeapProfiler stays enabled after capture to preserve V8 object identities across
snapshots. Use `heap_stop` with `page` when finished; it refuses while that capture
is running. A connection retains at most four page profilers. Browser status lists
the retained instrumentation. Stopping/restarting a profiler, a native profile
reset, event loss or a renderer attachment change starts a new identity epoch.
Heap status reports `inspectionCleanupPending` for retained temporary inspector
groups. Cleanup retries settled calls; uncertain calls remain owned until the
frontend attachment ends.

`heap_compare` takes two loaded snapshots under the same `developer`:
`baseSnapshotId` and `snapshotId`, plus optional `offset`/`limit`. It reports native
class additions/removals and size changes, with a `classIndex` for each row.
Summary totals cover all changed classes before pagination. `maxNameLength`
also applies to comparisons, with the same 100-character default and 1024 maximum.
Pass that index in `heap_compare` to page its added/deleted object IDs and
self sizes, sorted by descending self size. Added IDs belong to `snapshotId`;
deleted IDs belong to `baseSnapshotId`. The result labels both source models.
Use an index only with the same ordered snapshot pair. Different identity epochs return
`HEAP_INCOMPARABLE`; a numeric object-ID match across resets is not evidence of
the same object. `heap_release` takes `developer` and
`snapshotId`, terminates that model's worker and preserves its evidence artifact.
Removing an artifact is separate from releasing a loaded model.

### Analyze a recorded trace

Start `trace_start` before the journey, then stop it with `trace_stop`. Use the
complete report's `evidenceId` with `performance_load` and an owned `developer`
reference. Loading verifies the artifact's digest and parses its existing bytes
with Chrome's bundled trace model. It does not run the journey again. Incomplete
traces and borrowed DevTools frontends are refused; parsing uses state belonging
to the owned frontend.
`PERFORMANCE_TRACE_METADATA_UNAVAILABLE` means the artifact lacks frame metadata;
start tracing with a live page before recording the journey for native analysis.

Keep the returned `analysisId`. Its summary includes redacted navigation URLs,
trace bounds and `insightSetId` values. The trace covers the owned browser process;
the frontend page and the recording's context do not filter other owned pages out.
Select a set with `navigationObserved:true` when inspecting a recorded navigation;
the native `NO_NAVIGATION` set can report the same URL.

`performance_query` takes `developer`, `analysisId` and `query`:

- `{kind:"summary"}` lists the trace's insight sets.
- `{kind:"timings",offset:0,limit:50}` lists native page-load timings by frame and
  navigation, with their units, classification and estimation flag.
- `{kind:"vitals",insightSetId:ID}` returns observed LCP, INP and CLS where the
  native model supplies them. Missing observations have `available:false` and
  `value:null`. INP uses the model's high-percentile interaction; the longest
  interaction is reported separately. These are the values observed during the
  trace window only, not page-lifetime or field (CrUX) metrics.
- `{kind:"insights",insightSetId:ID,offset:0,limit:50}` lists native findings,
  warnings and estimated metric savings. Add `insight:NAME` from that list to
  inspect one finding's bounded private details.

These metrics cover the recorded interval. They are neither CrUX field data nor
measurements of the page's entire lifetime. Timing values identify microseconds
or milliseconds; CLS is a score. Insight savings are estimates from the native
model, not verified improvements to the application.

A connection retains one parsed trace, capped at 32 MiB and 250,000 events.
Queries return at most 64 KiB, with bounded nesting, strings, item counts and
`truncated` metadata. Pagination accepts `offset` 0–100000 and `limit` 1–100.
`performance_release` takes `developer` and `analysisId`, releases the parsed
model and keeps its artifact. Closing the frontend also releases the model;
uncertain cleanup remains owned until frontend or browser teardown completes.

## Use microphone and camera fixtures in Chrome

Select a WAV for the microphone and a Y4M or MJPEG file for the camera when you
launch Chrome. Either fixture can be used on its own. Testrun identifies the
camera format from the file signature.

```sh
testrun browser connect --engine chrome --microphone-fixture fixtures/dictation.wav --camera-fixture fixtures/camera.y4m
```

The top-level `testrun connect` command accepts the same flags. With MCP, pass
`{"engine":"chrome","mediaFixtures":{"microphonePath":"fixtures/dictation.wav","cameraPath":"fixtures/camera.y4m"}}`
to `browser_connect` or `connect`. Relative paths resolve from the CLI or MCP
server's working directory.

Testrun copies each selected regular file into the private Chrome profile. The
limit is **256 MiB per file**. Testrun configures Chrome's looping fake-capture
devices for the whole launch, including newly opened pages. With one fixture
selected, Chrome supplies a built-in fake device for the other input. Testrun
checks file signatures before launch. Verify capture in your test to confirm
Chrome can decode and replay the selected files.

Grant `microphone` or `camera` for the page's origin as usual. On macOS, Testrun
configures the audio service to run inside Chrome for access to the private WAV.
For speech tests that need the original waveform, turn off echo cancellation,
noise suppression and automatic gain control in the app's capture constraints.

## Configure the browser environment

Use `context_configure` with `context` and `setting`, for example:

```json
{"operation":"context_configure","context":{"connectionId":"CONNECTION-UUID","id":"CONTEXT-UUID","generation":"GENERATION-UUID"},"setting":{"kind":"offline","offline":true}}
```

Replace the reference fields with values returned by Testrun. Other setting kinds are
`geolocation` with `{latitude, longitude, accuracy}` or null,
`permissions_grant` with an explicit HTTP(S) `origin` and `permissions`, and
`permissions_clear` to clear all context permission overrides, including
creation-time `permissions`. Origins cannot include
credentials, wildcards, queries, fragments or application paths. Discovery
lists the selected engine's supported permission names. Repeated grants for the
same origin add to its earlier grants. Grants affect browser
permissions; they do not grant OS access or provide hardware. All of these
settings work on Chrome, Firefox and WebKit; each setting also applies to pages
and frames opened later in the context.
`extra_http_headers` with `headers:[{name:"X-Acme",value:"example"}]` replaces
the context's header defaults on every engine; `headers:[]` resets the list.
It affects current/future targets and associated HTTP clients that inherit
headers. Receipts report header counts. A failed change can leave browser
configuration uncertain; explicitly configure the desired list again or close
the context. Inherited HTTP requests refuse while that uncertainty remains.
All three engines accept `http_credentials` with a required `credentials`
object/list or null/empty-list reset. It updates the context and the defaults
followed by associated HTTP clients; explicit HTTP credentials retain their own
defaults. Requests already admitted retain their credential snapshot. Counts
are reported without secret values; uncertain updates require acknowledged
explicit recovery or owned-context closure.
WebKit can retain an existing document's permission-query decision after clearing
overrides. Explicitly reload that page or close its owned context when you need
the permission baseline restored; Testrun does not navigate it automatically.

Use `page_emulate_media` with `page` and `options` to set `media` (screen/print),
`colorScheme`, `reducedMotion`, `forcedColors` and `contrast`. At least one option
is required. Omitted fields preserve existing overrides; `no-override` turns
off that feature's emulation for the page, including any value from the
context's creation options. Results report both requested settings and observed
media-query values. Combinations can affect the effective scheme, so use the
observed values. On Firefox and WebKit, active preview/recording sources
reconfigure and the result reports preview recovery separately; native Chrome
reports `previewRecovery: {required: false}` because its sources continue. If
native Chrome cannot apply a change, it restores the previous media and reports
`rolledBack`. Read fresh geometry before further native input after changing
media.

On Chrome, `page_emulate_throttling` slows one page and its frames:

```json
{"operation":"page_emulate_throttling","page":{"connectionId":"CONNECTION-UUID","id":"PAGE-UUID","generation":"GENERATION-UUID"},"options":{"network":"slow-3g","cpuRate":4}}
```

`cpuRate` is a CPU slowdown factor from 1 to 20, where 1 clears it. `network` is
`slow-3g`, `fast-3g`, `slow-4g`, `fast-4g`, `offline` or `none`, which clears
it; the presets match Chrome DevTools. Omitted fields keep their value, and the
result reports the `effective` settings. A context's `offline` setting still
applies on top of a page preset. Firefox and WebKit refuse the operation with
`THROTTLING_UNSUPPORTED`.

Settings belong to the owned context and disappear when it closes. To continue
in the same context, explicitly restore known settings, such as `offline:
false`, null geolocation or media `no-override`. A configuration acknowledgment
does not prove an application effect. If an outcome is unknown, inspect the
application or close the owned context before deciding whether to retry.

## Control page time

Use `context_clock` with a context reference and an `action`:

```json
{"kind":"set_fixed_time","timeMs":1700000000000}
```

Actions are `install` (optional `timeMs`), `set_fixed_time`, `set_system_time`,
`pause_at` (required `timeMs`), `run_for`, `fast_forward` (required `durationMs`)
and `resume`. Time is Unix-epoch milliseconds. `run_for` accepts up to 60000 ms
and executes due timers; `fast_forward` accepts up to seven days and fires missed
timers at most once. Fixed time changes Date while timers continue. Pause-at
advances to an absolute time and pauses the clock.

The clock affects pages, frames and future documents in that context. Other
contexts and workers keep their clocks. Host deadlines, input observation expiry,
preview frame age and recording duration stay on real time. Preview resize can
continue while page animation frames are paused.

`resume` resumes the virtual clock; close the owned context to fully restore
native timing. Clock calls can run application callbacks and mutate the page.
Inspect unknown outcomes before retrying.

On Firefox/WebKit, Testrun corrects the pinned SDK's future-document bootstrap:
replaying a paused clock cancels its pending native wakeup, and the first
`performance.timeOrigin` read initializes the clock history. A new document's
timers stay paused until a clock action advances or resumes them.

Chrome reports `implementation:"testrun-clock-v1"` and
`playwrightClockEquivalent:false`. It controls the main-world Date, timeout,
interval, animation-frame and idle-callback APIs in the selected context's pages,
frames and future documents. Install before navigating to your application:
timers already scheduled through native APIs, and cached native functions, keep
their native behavior. Workers, other contexts and isolated worlds are unaffected.

Chrome's animation-frame callbacks use synthetic 16 ms ticks. They do not advance
native paint, CSS animations, Web Animations or media playback. The helper also
replaces `performance` and `Event.timeStamp`: `performance.now()` follows virtual
ticks, entry reads return empty lists, marks/measures are synthetic and the other
performance facade methods are stubs. Use native diagnostics for real timing.
Explicit-argument Date construction, `Intl`, `Temporal` and `AbortSignal` stay
native.

Chrome repeating timers use a minimum delay of 1 ms. Future documents restore
paused ticks, fixed Date and time origin before their first script runs. Documents
created during an update keep a virtual clock and can initially see its prior
state. Wait for the clock action to complete before starting dependent navigation.

Each document has its own controller. Updates run sequentially across documents;
the result lists `completedWorlds`, `controlledApis` and `excluded` scope. Future
documents reconstruct the context's time history without replaying callbacks from
older documents. Chrome retains up to 256 clock actions per context; close it and
create a new context at capacity. A callback error or incomplete Chrome clock
update returns progress and blocks further clock updates with `CLOCK_PENDING`.
Close the owned context to recover. An uncertain transport write may require
disconnecting the session under its ordinary recovery rules.
After a settled clock failure, later page/frame attachments in that context use native
timing. Other contexts and owned-context close remain available. Existing
documents can retain partial clock effects until the context closes.

## Test passkeys with a virtual authenticator

Use `context_credentials` to test WebAuthn registration and passkey sign-in
without a physical authenticator. Install the authenticator before the page
first uses `navigator.credentials`, then let the page register a passkey or
seed one:

```text
context_credentials: {context: CONTEXT_REF, input: {action: "install"}}
context_credentials: {context: CONTEXT_REF, input: {action: "create", rpId: "app.example.com"}}
context_credentials: {context: CONTEXT_REF, input: {action: "list", rpId: "app.example.com"}}
context_credentials: {context: CONTEXT_REF, input: {action: "delete", id: "CREDENTIAL_ID"}}
```

`create` generates a resident ES256 passkey for `rpId`, the site's lowercase
domain; optional `id` and `userHandle` are unpadded base64url and `signCount`
sets the starting counter. A credential created before `install` is held but
invisible to pages until installation. Results show `id`, `rpId`, `userHandle`,
`publicKey` and `signCount`, never private keys. `list` returns pages in ID
order with `nextCursor`. Each context holds up to 256 credentials, including
those the page registers, and closing it ends the authenticator. It does not
remove accounts on the server.

To reuse a passkey in a later run, export it with
`storage_export {options: {credentials: true}}`, keep the private file, and
restore it with `storage_register` and `storage_apply {installAuthenticator: true}`.
Private keys cannot be passed to `create`.

On Chrome, each page has its own authenticator that Testrun keeps in step with
the context: a passkey a page registers reaches other open pages of the context
at the next `context_credentials` call, and new pages receive every passkey
when they open. The authenticator uses CTAP 2.1 with automatic user
verification; Testrun does not emulate biometrics, hardware keys, FedCM,
browser autofill or digital-credential wallets.

## Sign in with a credential broker

`auth_sign_in` signs a page in with credentials that a credential broker
holds, so passwords never appear in tool arguments, results or transcripts.
Testrun does not include a broker or store credentials. A broker is a program
you run that serves the `testrun-broker-v1` protocol on `<name>.sock` in
Testrun's private broker directory, `/tmp/testrun-qa-<uid>/brokers` (where
`<uid>` is your numeric user ID). The directory must be yours with no group or
other access. `auth_broker_list` shows the directory, the brokers it finds and
the account labels each offers.

Describe the application's sign-in screen once as a journey:

```json
{"origin":"https://app.example.com",
 "username":{"kind":"label","text":"Email"},
 "password":{"kind":"label","text":"Password"},
 "submit":{"kind":"role","role":"button","name":"Sign in"},
 "signedIn":{"kind":"role","role":"navigation","name":"Main"},
 "signedOut":{"kind":"role","role":"heading","name":"Sign in to Acme"},
 "failure":{"kind":"role","role":"alert"},
 "identity":{"kind":"test_id","value":"account-name"}}
```

Then sign in, or check the current state without changing anything:

```text
auth_sign_in: {page: PAGE_REF, broker: "acme-vault", account: "staff", journey: JOURNEY, expectedIdentity: "Staff user"}
auth_status: {page: PAGE_REF, journey: JOURNEY, expectedIdentity: "Staff user"}
```

`auth_sign_in` asks the broker only for an account of the journey's `origin`,
and enters it only if the page's main document still has that origin and has
not navigated meanwhile (`DOCUMENT_CHANGED`). It returns `action:"none"` when
the page already shows `signedIn` with the expected identity, and refuses with
`IDENTITY_MISMATCH` when another account is signed in; sign out in the app
first. `expectedIdentity` needs the journey's `identity` locator, or the call
refuses with `JOURNEY_IDENTITY_REQUIRED` before contacting the broker. If the
identity text repeats an entered value, such as the username, the result
reports `textWithheld:true` instead of the text. It waits up to 30 seconds for the outcome: `signed_in`, `SIGN_IN_REFUSED`
with the page's message (withheld if it repeats an entered value), or the
journey's optional `otp` field, which is filled when the broker supplies a code
and otherwise returns `MFA_REQUIRED`. The code is entered only while the page
still has the journey's origin; if sign-in redirected the page elsewhere, it
refuses with `ORIGIN_MISMATCH` and `submitted:true`. Testrun never resubmits.
After an uncertain outcome, use `auth_status`.

Stop any trace, HAR capture or request callback in the context first: sign-in
refuses with `SECRET_POLICY_UNENFORCEABLE` while they could record the values.
Network diagnostics can stay on. Requests the context sends while values are
entered are marked `requestBodyWithheld`, and reading their request body with
`network_read` refuses with `BODY_WITHHELD`; their other metadata stays
readable. Values are still visible to anything that reads the page itself, so
don't evaluate script against the form or read its fields during sign-in.
`SESSION_NOT_STARTED` means the broker is not running; start it yourself.
`BROKER_UNTRUSTED` names the directory or socket check that failed.

`auth_status` reports `signed_in`, `signed_out` (the journey's `signedOut`
element is visible), `unknown` or `other_origin`, with the identity text when
signed in.

## Upload and download files

Register synthetic, blank-template or cleared seed bytes before attaching them.
The CLI reads only the explicitly selected regular file, up to 4 MiB:

```sh
testrun browser fixture-register ./Acme.csv --target browser:CONNECTION-UUID \
  --mime-type text/csv --provenance synthetic_generated \
  --cleared-by 'Acme QA' --clearance-method 'Generated test data'
```

When you have a local path, run the CLI command above. To register bytes via
MCP, use `fixture_register` with `fixture: {name, mimeType, dataBase64,
provenance, cleared: {by, method}}`. Encode up to 4 MiB of cleared bytes in
standard base64. Send large fixtures one per call.
`fixture_list` returns references and `fixture_verify` rechecks bytes. Testrun
privately stores the metadata and SHA-256, then re-hashes each selected fixture
before upload.
`page_upload` takes `page`, optional `frame`, `locator` and `fixtures` references.
An empty list clears the input. At most 16 files and 4 MiB total can be attached
at once, with 64 registered records per session.

File choosers are intercepted before page use. Status with `includeDetails: true` exposes pending chooser
references; `chooser_resolve` supplies `fixtures`, with an empty list clearing
the input. Responses are single use and expire after 30 seconds. Successful
attachment does not establish that the application uploaded bytes to its server;
results explicitly report `serverUploadVerified: false`.

Downloads require `browser connect --downloads`, or MCP context option
`acceptDownloads: true`; the default is false. Use `download_list`,
`download_status`, `download_cancel` and `download_delete` with returned references.
Status exposes pending, complete, cancellation and failure states. Completed
artifacts carry actual byte count and SHA-256. Browser filenames are hints;
Testrun chooses private output names. Publication is limited to 8 MiB and 32
download records per session; deleted records make room for new downloads. A
download beyond that limit, or one the browser writes outside the session's own
folder, is cancelled and listed under `transfers.refused` in status, as is a
file chooser from another page's frame; the session continues. Incoming size is monitored, but this is not a hard
disk quota: the browser can write more before cancellation is observed. Failure
does not publish incomplete bytes as a complete artifact. Delete removes the
owned browser artifact and published file; it can be repeated.

## MCP and cleanup

The MCP tools are `browser_list`, `browser_doctor`, `browser_connect`,
`browser_status`, `browser_send`, `browser_cancel` and `browser_disconnect`.
`browser_send` takes `{request: REQUEST, operationId?: UUID, timeoutMs?: MS}`.
A session keeps the latest 256 operations for status and repeated
`operationId` submissions; when that history is full, the oldest finished
operation is forgotten first. Only 256 unfinished operations make a new one fail
with `OPERATION_CAPACITY`. The `browser.operations` entries are summaries with
`result: null`. Get the full result at `operation.result` by passing
`operationId` to `browser_status` or `actionId` to shared `status`. This returns
the requested operation and its target ID. Omit the operation ID to inspect
browser state. The default snapshot includes up to three operation summaries.
Pending operations appear first in submission order, followed by settled
operations from newest to oldest by submission. With a target ID, use `limit`
from 1 to 16 to change the number of summaries, or use `includeDetails: true`
to see resources, transfers, diagnostics and all operation summaries.
`operationsTotal` gives the total retained operations when `detailsOmitted`
is true. With `waitMs` and no operation ID, status waits for preparation and then
the oldest pending operation on the selected browser. If several browsers are
connected, name one with `targetId`. The snapshot reports `waitExpired` and, if
it waited on an operation, `waitedOperationId`. A live reply includes the manager's
`testrunVersion`. If a status poll reports `WAIT_EXPIRED` with `settled: false`,
the operation is still queued or running. Poll the same ID to check it again.
The CLI uses `browser status --include-details` or `status --include-details`
for browser details, and `browser status --operation-id` or `status --action-id`
for an operation result.
Connect takes
`{engine: 'chrome' | 'firefox' | 'webkit', mode?: 'headless' | 'headed', context?: OPTIONS}`.

Connect returns `preparationOperationId` and its `preparationOperation` outcome.
Pass that ID to `status`/`cancel` as `actionId` (shared `cancel` also requires
`targetId`), or to
`browser_status`/`browser_cancel` as `operationId`. A pending cancellation keeps
the installation owned until it settles, then prevents launch. A completed
preparation keeps its original result; cancel the separate startup operation or
disconnect to release a launched browser. A status wait expiry does not cancel
preparation. These IDs also identify their browser when several are connected.

Browsers live in the user's background Testrun manager beside the connected
mobile devices, and every CLI command and MCP server sees the same browsers.
Each `browser_connect` starts another browser and returns its `targetId`,
`browser:<connectionId>`. Other browser tools take that `targetId` (CLI
`--target`); they may omit it while one browser is connected, or when a page or
context reference or an `operationId` in the request belongs to one.
`browser_status` without a target or operation ID returns a page of
connections under `browsers`, with `nextCursor` for more results. `limit`
bounds the number of browsers per page. Each compact row has up to three
operation summaries.
With no manager running, it returns
`live: false` with a last-known record that does not prove resource cleanup.

A browser stays open until `browser_disconnect` names it, or `disconnect` names
its `browser:` target; `browser_disconnect` without a target closes the only
connected browser. Plain `disconnect` without a target resolves among mobile
devices and never closes a browser. A client exiting, task end and idle time
leave browsers open. Status does not change any lifetime.

Chrome can drop diagnostic events during a burst of network activity. Status
reports the loss and affected evidence may be incomplete; target attachment and
startup continue because their events are retained separately from eviction.
Critical-event overflow still loses the connection. A failed Chrome method
includes `backendFailure` with its numeric CDP `code`, `source: "cdp-reply"`
and `dispatch: "replied"` in the operation result. For a batch, inspect the
failed step's `partial`. Remote messages, stacks and data stay private.

```sh
testrun browser disconnect --target browser:CONNECTION-ID
```

Closing a browser waits for its preparation, driver exit and observed browser
processes. Once the manager accepts a disconnect, it continues cleanup even if
the client exits. If cleanup is unfinished, `browser_disconnect` returns
`CLEANUP_PENDING` and the browser stays connected while the manager retries.
The error is marked `retryable`, and you can run the same disconnect to retry
immediately. If cleanup reached the
browser session, the error includes `details.operationId` and
`details.operationCode`. For Firefox and WebKit,
`details` can name the unfinished stage (`unfinishedStage`, with per-stage `cleanupStages` timings in
milliseconds). `browserCloseOutcome` reports `ACKNOWLEDGED` or the close
attempt's error code, without remote messages or stacks; null means that stage
was not reached. `driverShutdown.lifecycle` contains up to 64 observed driver
lifecycle markers, including browser-process closure and temporary-directory
cleanup. It includes elapsed times and explicit omission/partial-line flags,
without PIDs, paths, URLs or browser output. These observations help locate a
stall; process-release counts remain separate evidence. A successful
Firefox/WebKit disconnect includes the completed report in `cleanup`, with
`complete:true` and no unfinished stage. Include these fields when reporting
cleanup that keeps failing. If release completes but the journal write fails,
the receipt includes `cleanup.journalFailure`. An already absent target has no
new cleanup report.
The private lifecycle journal rolls at 4,096 entries or 8 MiB per connection.
Oldest entries age out as new entries are written.
Successful closes refresh the durable browser list. If that record cannot be
written, the verified close is still returned with `statusRecordError` alongside.
`preCloseInspection` reports whether process witnesses were refreshed before
browser closure. If closure proceeds without a completed inspection, ownership
remains unknown. An interrupted read can recover before closure starts.
`driverShutdown.processWait` adds up to 32 birth-fenced samples while waiting for
driver exit, with driver-alive and witnessed-descendant counts, elapsed session
times and visible clipping. Failed observations report null counts. These
samples supplement the final release checks and do not grant signal authority.
`driverShutdown.lifecycle.ownedChildren` observes up to four anonymous SDK child
owners and stream descriptors 0–4. It separates native child exit from the later
child close event, with first observation, exit and close stream snapshots plus
individual stream end/finish/close observations. `observingNativeState`,
`nativeExitState` and `nativeCloseState` retain status-known/handle-present flags
and bounded close-callback counts at those observations. `latestNativeState`
and `latestNativeStreams` preserve changed state sampled once per second, with
its receipt time in `latestNativeStateElapsedMs`. The sampling timer is
unreferenced and cleared on native close. A known exit status does not invent
an observed exit event or an exit timestamp. Actual exit codes, signal names,
PIDs and stream payloads are omitted. Null times or flags mean no observation.
Times mark receipt of owned stderr observations; flags were read at the callback
or sampling point. Capacity, unavailable sources and invalid
records remain explicit. These fields preserve cleanup evidence across retries;
they do not authorize signals or change the original shutdown deadline.
Disconnecting a browser target that is not connected reports
`closed: true`. Unrelated profiles and processes are preserved.

If preparation fails, inspect status and
`browser-preparation-error-CONNECTION-ID.json` under Testrun's private data root
(`TESTRUN_DATA_ROOT`, or `/tmp/testrun-qa-UID` by default) for the concrete
cause. Fix host network, proxy, permissions or disk space, disconnect, then
reconnect. A corrupt installation
returns `BUNDLE_CORRUPT`; disconnect the browsers using that exact build before
replacing its private cache directory. Cache pruning and persistent profiles have
no public command in this slice. With no manager running, status reports durable
records as last-known state and never treats them as proof of live resources or
successful cleanup.
