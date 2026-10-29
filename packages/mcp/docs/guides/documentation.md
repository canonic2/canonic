# Bundle your guides as documentation tools

Agents often need your product's own guides: when a request is refused, when
they are unsure of a workflow, or when they are working outside a project. The
`docs` module packages your Markdown guides into your executable at build time
and publishes three tools that list, search and read them offline. The guides
always match the installed version, and the tools never read the user's files.

## Add the bundle to your server

Your package needs `canonic-mcp` twice: as an ordinary dependency, and as a
build dependency with the `docs-build` feature, which is the only part that
reads files.

```toml
[dependencies]
canonic-mcp = { path = "../mcp" }

[build-dependencies]
canonic-mcp = { path = "../mcp", features = ["docs-build"] }
```

Put your guides in `docs/` beside `Cargo.toml`, then bundle them from
`build.rs`:

```rust,ignore
fn main() -> Result<(), canonic_mcp::docs::build::BuildError> {
    canonic_mcp::docs::build::embed()?;
    Ok(())
}
```

`embed` validates the folder, writes the bundle to Cargo's output folder and
asks Cargo to rebuild when any guide changes. It returns the selected files in
bundle order, in case your build script also fingerprints its inputs.

`embed` prints `cargo:rerun-if-changed` for `docs/`, and once a build script
prints that line, Cargo reruns it only when a listed path changes rather than
any file in your package. If your build script does other work, print
`cargo:rerun-if-changed` for that work's inputs too.

Embed the bundle in a `static` and publish the tools with your own prefix and
product name. This server answers only documentation calls:

```rust,ignore
use canonic_mcp::{
    Catalog, Client, RpcError, ServerInfo,
    docs::{Bundle, Documentation},
    stdio::{self, Context, Handler, Outcome, Request},
};
use std::{io, sync::{Arc, LazyLock}};

static GUIDES: Bundle = canonic_mcp::include_docs!();
static DOCS: LazyLock<Documentation> =
    LazyLock::new(|| Documentation::new(&GUIDES, "acme", "Acme", env!("CARGO_PKG_VERSION")));

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", env!("CARGO_PKG_VERSION")).with_instructions(
            "For usage help, or when a request is refused, use acme_docs_list, \
             acme_docs_search and acme_docs_read.",
        ))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        match DOCS.call(&call.name, &call.arguments, &|| context.check()) {
            Some(outcome) => Outcome::Tool(Documentation::result(outcome)),
            None => unreachable!("only documentation tools are published"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Arc::new(Catalog::new(DOCS.tools())?);
    let stdin = io::BufReader::new(io::stdin());
    stdio::serve(Arc::new(Acme), catalog, stdin, io::stdout(), &stdio::Options::default())?;
    Ok(())
}
```

In a server with other tools, add `DOCS.tools()` to your catalog and route
calls where `DOCS.handles(&call.name)` is true. To keep guides available while
other work is busy, admit documentation calls on a lane that long-running work
does not occupy.

## What gets published

The build helper selects every file ending in `.md` under `docs/`, in nested
folders too, and orders the pages by path. A page's path is relative to
`docs/` with `/` separators, such as `guides/setup.md`. Its title is the text
after `# ` on its first line that starts with `# `.

- Files and folders whose names start with `.` are skipped.
- Other files are ignored.
- The build fails, naming the relative path, for a symbolic link anywhere in
  the folder, a page or name that is not UTF-8, a name containing `\` or a
  control character, a folder with no pages, or a size bound below.

Pages are stored byte for byte: line endings, a missing final newline and
non-ASCII text are preserved. Publish usage guides only; internal plans,
repository instructions and machine-specific values do not belong in `docs/`.

| Build bound | Value |
| --- | --- |
| Pages | 1 to 1024 |
| Page path | 512 bytes |
| Page | 1 MiB |
| Line, excluding its line break | 64 KiB |
| All pages together | 16 MiB |

## The three tools

With the prefix `acme`, the tools are `acme_docs_list`, `acme_docs_search` and
`acme_docs_read`. They are annotated read-only, idempotent and closed-world,
and their schemas refuse unknown arguments. Every result includes `version`,
the version you passed to `Documentation::new`.

| Tool | Arguments | Returns |
| --- | --- | --- |
| `list` | `offset` (default 0), `limit` (1–100, default 50) | `documents`: `path`, `title` and `total_lines` for each page; `total`; `next_offset` |
| `search` | `query` (required), `path`, `offset`, `limit` as for list | `matches`: `path`, one-based `line` and the line's `text`; `total`; `next_offset` |
| `read` | `path` (required), `start_line` (default 1), `limit` (1–400 lines, default 100) | `path`, `title`, `start_line`, `total_lines`, `content` and `next_start_line` |

Search is a literal, case-insensitive substring match within one line. `.*`
and `[` match themselves, and spaces around the query are kept. A query must be
1–1024 bytes, not only whitespace and without line breaks. With `path`, only
that page is searched.

`read` returns the selected lines exactly as stored, line breaks included, so
joining the `content` of successive reads reproduces the page. Reading past the
end returns empty `content`.

`limit` is a maximum. A result also stops before its text passes 512 KiB,
counted as escaped in JSON, where a control character takes six bytes, so
always continue from `next_offset` or `next_start_line` until it is `null`
rather than assuming a full page.

## Failures

`Documentation::call` returns a `DocsError`, and `Documentation::result` turns
it into a failed tool result whose text tells the model how to recover. Map
the error kinds yourself if your server has its own result format.

| Error | When |
| --- | --- |
| `InvalidArguments` | A missing, mistyped, out-of-range, blank or multi-line argument, or an unknown one |
| `NotFound` | A path that is not exactly one returned by list; the message names your list tool |
| `Cancelled` | The interruption check failed: the request was cancelled or reached its deadline |
| `Unavailable` | The embedded bundle is corrupt, which only a broken build produces |

Paths are compared exactly. `../README.md`, `/etc/passwd` or a differently cased
name is simply not found; nothing is resolved against a filesystem.

## Cost and lifetime

Building the tools, listing your catalog and calling other tools never touch
the bundle. The first documentation call reads the page index; the first
search or read decompresses the text; the first search also prepares a
lowercased copy for matching. Each step happens at most once per process, even
when calls arrive together, and later calls reuse it. A new process, such as a
new server session, prepares again on its first documentation call.

`call` is synchronous and polls your interruption check between pages and
every few hundred lines, returning `Cancelled` without partial results. A
preparation step already under way finishes first and is kept for the next
call.

With an asynchronous runtime, `call` never yields, so a timeout around its
future or dropping its task cannot stop it. Run it on a blocking thread and
give it a check that watches the deadline and cancellation itself:

```rust
use canonic_mcp::Interrupted;
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Instant};

fn check(deadline: Instant, cancelled: Arc<AtomicBool>) -> impl Fn() -> Result<(), Interrupted> {
    move || {
        if Instant::now() >= deadline || cancelled.load(Ordering::Relaxed) {
            Err(Interrupted)
        } else {
            Ok(())
        }
    }
}
```

Set `cancelled` when the client cancels the request; with Tokio, pass
`check(deadline, cancelled)` into `tokio::task::spawn_blocking` along with the
`call`.

The same `call` works without a session, so a command-line interface can
offer the same list, search and read results.
