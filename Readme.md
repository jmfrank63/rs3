# rs3

rs3 is a small Rust key/value store. An actix-web HTTP server keeps string
key/value pairs in an in-memory map, and `PATCH /key/{key}` runs the stored
value as JavaScript in an embedded V8 (deno_core), whose ops call back into the
store. It started as a 2021 Birkbeck MSc Computer Science project and now
builds on Rust 1.99.1 / edition 2024.

The original 2021 MSc version, including its multi-container deployment setup,
is preserved at git tag `msc`. `Vagrantfile`, `aws/` and `experiments/` belong
to that 2021 setup and are not maintained for the current version.

## Requirements

- **Rust 1.99.1**, pinned in `rust-toolchain.toml` together with `rustfmt` and
  `clippy`. With rustup installed, the first `cargo` command in the repo
  installs the pinned toolchain automatically. The crate uses edition 2024 and
  declares `rust-version = "1.99"`.
- **Linux with glibc**, on x86_64 or aarch64.
- **Network on the first build.** The `v8` crate's build script downloads the
  V8 prebuilt static library for your target from the denoland/rusty_v8
  GitHub releases (about 36–37 MiB compressed). It needs network access plus
  `curl` or `python3`. The library is downloaded once per profile (debug and
  release), so the first build of each profile is slow.
- **Offline or mirrored builds:** set `RUSTY_V8_ARCHIVE` (a path or URL to the
  `.a.gz` file) or `RUSTY_V8_MIRROR` (a base URL that replaces the GitHub
  releases URL).
- **Disk:** `target/` needs several GB: 2.5–4 GB for a debug build with tests,
  about 1 GB for a release build.

## Quick start

```sh
git clone https://github.com/jmfrank63/rs3
cd rs3
cargo build --release --locked
env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run --release --locked
```

The server prints this line once it is listening:

```text
Starting Http server at host address: 127.0.0.1, with port: 5000!
```

In a second terminal:

```sh
curl -s http://127.0.0.1:5000/
cargo test --locked
```

The curl command prints `"Welcome to rs3"`.

`SERVER.HOST` is not a valid shell variable name, so a bare
`SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run` is not an assignment: bash
tries to run it as a command and reports `command not found` (exit 127).
`env` passes the dotted names to the program instead.

Alternatively, copy `env.example` (`SERVER.HOST=localhost`,
`SERVER.PORT=5000`) to a dotenv file named `.env` in the repo root and run
`cargo run --release --locked` without `env`.

## Configuration

rs3 reads exactly two settings, `SERVER.HOST` and `SERVER.PORT`. The names are
case-insensitive (`SERVER.HOST`, `server.host` and `Server.Port` all work), but
the dot is required: underscore names such as `SERVER_HOST` and `SERVER_PORT`
are not read. Every other variable is ignored.

- **Sources:** the process environment, and a dotenv file (`.env`) in the
  working directory or the nearest parent directory that has one. The
  environment wins over the file. A missing file is ignored.
- **No defaults:** both settings are required.
- **A broken dotenv file stops startup.** A line that does not parse, a file
  that starts with a UTF-8 byte order mark (BOM) or an unreadable file gives a
  line that starts with `error: invalid configuration: .env: ` and exit 1.
  This rule is taken from `src/config.rs` and its tests
  (`bom_prefixed_dotenv_is_error`), not from a live run.

Every startup failure is a single `error:` line on stderr followed by exit 1.
These are the lines the release binary prints (with port 5000 standing in for
the port used):

| Input | stderr line (exit 1) |
|-------|----------------------|
| nothing set | `error: invalid configuration: missing configuration field "server"` |
| only `SERVER_HOST` / `SERVER_PORT` (underscores) | `error: invalid configuration: missing configuration field "server"` |
| only `SERVER.HOST` | `error: invalid configuration: missing configuration field "server.port"` |
| only `SERVER.PORT` | `error: invalid configuration: missing configuration field "server.host"` |
| `SERVER.PORT=abc` | ``error: invalid configuration: invalid type: string "abc", expected an integer for key `server.port` in the environment`` |
| `SERVER.PORT=70000` | ``error: invalid configuration: invalid type: 64-bit unsigned integer `70000`, expected an unsigned 16 bit integer for key `server.port` `` |
| `SERVER.PORT=` (empty) | ``error: invalid configuration: invalid type: string "", expected an integer for key `server.port` in the environment`` |
| `SERVER.HOST=` (empty) | `error: cannot bind :5000: failed to lookup address information: Name or service not known` |
| port already in use | `error: cannot bind 127.0.0.1:5000: Address already in use (os error 98)` |
| port 80 as an unprivileged user | `error: cannot bind 127.0.0.1:80: Permission denied (os error 13)` |

The port 80 line assumes the Linux default
`net.ipv4.ip_unprivileged_port_start=1024`.

The banner `Starting Http server at host address: <host>, with port: <port>!`
is printed only after the port is bound, so it means the server is up.
SIGTERM shuts the server down gracefully with exit 0.

The JavaScript limits are not configuration; see [Limits](#limits).

## HTTP API

Every success body is a JSON string (the legacy envelope kept from 2021) with
`content-type: application/json`. The examples run one after the other on a
fresh server:

```sh
curl -s -X POST http://127.0.0.1:5000/insert -d '{"greeting":"hello","m":"6*20"}'
curl -s http://127.0.0.1:5000/key/greeting
curl -s http://127.0.0.1:5000/list
curl -s -X PATCH http://127.0.0.1:5000/key/m
curl -s -X DELETE http://127.0.0.1:5000/key/greeting
```

| Method and path | Request body | Status | Response body (exact) |
|-----------------|--------------|--------|-----------------------|
| `GET /` | — | 200 | `"Welcome to rs3"` |
| `GET /ip` | — | 200 | `"{'IP' : '127.0.0.1'}"` (the configured host) |
| `POST /insert` | `{"greeting":"hello","m":"6*20"}` | 200 | `"{\"greeting\":\"hello\",\"m\":\"6*20\"}"` |
| `GET /key/greeting` | — | 200 | `"hello"` |
| `GET /list` | — | 200 | `"[{\"greeting\" : \"hello\"},{\"m\" : \"6*20\"}]"` |
| `GET /list` (empty store) | — | 200 | `"[]"` |
| `PATCH /key/m` | — | 200 | `"120"` (the value `6*20` run as JavaScript) |
| `DELETE /key/greeting` | — | 200 | `"hello"` (the removed value) |

`GET /list` sorts the entries by key in byte order, so uppercase keys come
before lowercase ones, and JSON-escapes keys and values inside the legacy
`{\"key\" : \"value\"}` envelope. After inserting `{"q":"say \"hi\"\nnext"}`,
the list above reads:

```text
"[{\"greeting\" : \"hello\"},{\"m\" : \"6*20\"},{\"q\" : \"say \\\"hi\\\"\\nnext\"}]"
```

`POST /insert` takes a JSON object whose values are all strings and inserts
every pair. The answer echoes the pairs in request-body order, while
`GET /list` is sorted. Re-inserting an existing key replaces its value, `{}`
inserts nothing, and a rejected body inserts nothing. Continuing on a store
that holds `m`:

| Request | Status | Response body (exact) |
|---------|--------|-----------------------|
| `POST /insert` `{"b":"1","a":"2","B":"3"}` | 200 | `"{\"b\":\"1\",\"a\":\"2\",\"B\":\"3\"}"` |
| `GET /list` | 200 | `"[{\"B\" : \"3\"},{\"a\" : \"2\"},{\"b\" : \"1\"},{\"m\" : \"6*20\"}]"` |
| `POST /insert` `{"a":"9"}`, then `GET /key/a` | 200 | `"9"` |
| `POST /insert` `{}` | 200 | `"{}"` |

Two request rules are unchanged since 2021: any Content-Type is accepted, and
a body over 262144 bytes (256 KiB) gets 413 with the plain-text body
`payload reached size limit` (exactly 262144 bytes is accepted). The body must
be UTF-8; a charset parameter is ignored.

### Errors

Errors from rs3 have `content-type: application/json` and the body
`{"error":"<code>","message":"<detail>"}`:

| Status | `error` | Cause | Response body (exact) |
|--------|---------|-------|-----------------------|
| 400 | `malformed_json` | `POST /insert` with `{"a"` | `{"error":"malformed_json","message":"malformed JSON: EOF while parsing an object at line 1 column 4"}` |
| 400 | `malformed_json` | `POST /insert` with an empty body | `{"error":"malformed_json","message":"malformed JSON: EOF while parsing a value at line 1 column 0"}` |
| 404 | `not_found` | `GET`, `DELETE` or `PATCH /key/greeting` when the key does not exist (also on an empty store) | `{"error":"not_found","message":"key not found: greeting"}` |
| 422 | `invalid_input` | `POST /insert` with `[1]` | `{"error":"invalid_input","message":"invalid input: insert body must be a JSON object"}` |
| 422 | `invalid_input` | `POST /insert` with `{"a":1}` | `{"error":"invalid_input","message":"invalid input: value for key a must be a string"}` |
| 422 | `js_error` | PATCH of `throw new Error("boom")` | `{"error":"js_error","message":"Uncaught Error: boom"}` |
| 422 | `js_error` | PATCH of `let a = ;` | `{"error":"js_error","message":"Uncaught SyntaxError: Unexpected token ';'"}` |
| 422 | `js_depth_limit` | PATCH of `me` = `Deno.core.ops.op_rs3_patch("me")` | `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}` |
| 504 | `js_timeout` | PATCH of `while(true){}` (answered after about 5 s) | `{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}` |
| 500 | `internal` | a fault in the JS host (taken from the code; it cannot be triggered on demand) | `{"error":"internal","message":"internal error"}` |

Two answers come from actix-web itself, not from rs3:

| Status | Cause | Response |
|--------|-------|----------|
| 413 | request body over 262144 bytes | `payload reached size limit`, `content-type: text/plain; charset=utf-8` |
| 404 | unknown route or method (`GET /nope`, `POST /`) | empty body |

What a PATCH script can do, and the limits that produce 422 `js_depth_limit`
and 504 `js_timeout`, are described in the JavaScript ops and Limits sections.

## JavaScript ops

`PATCH /key/{key}` runs the value stored under `key` as a JavaScript script on
deno_core 0.412 (an embedded V8). The script runs on a dedicated JS worker
thread, never on an HTTP worker. It reaches the store through five ops on
`Deno.core.ops`, for example `Deno.core.ops.op_rs3_get("k")`. There is no
bootstrap call.

| Op | Argument | Returns |
|----|----------|---------|
| `Deno.core.ops.op_rs3_list()` | none | the inner `GET /list` text, such as `[{"k" : "v"}]`, sorted by key; `[]` on an empty store |
| `Deno.core.ops.op_rs3_get(key)` | the key | its value |
| `Deno.core.ops.op_rs3_insert(json)` | a JSON object with string values, as a string (use `JSON.stringify(obj)`) | the compact echo, such as `{"k":"v"}` |
| `Deno.core.ops.op_rs3_delete(key)` | the key | the value it held |
| `Deno.core.ops.op_rs3_patch(key)` | the key | `String(result)` of that key's script, run in a fresh nested runtime |

Op errors are ordinary JavaScript exceptions that a script can catch. A
missing key throws an `Error` with the message `key not found: <key>`; bad
input (malformed or wrongly shaped insert JSON, an object or symbol where a
string is expected) throws a `TypeError`. For example, the script
`try { Deno.core.ops.op_rs3_get("nope") } catch (e) { e.message }` answers
`"key not found: nope"`, and an uncaught op error answers 422 `js_error`.

### Migrating 2021 scripts

The 2021 version ran on a pre-release deno_core with a bootstrap call and
`Deno.core.opSync`. Neither exists any more:

| 2021 | Now |
|------|-----|
| `Deno.core.ops();` | delete the statement |
| `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` |
| `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` |
| `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` |
| `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` |
| `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` |

A stored 2021 script now fails at the first legacy call. PATCH of
`Deno.core.ops();` and of `Deno.core.opSync("rs3_list", "")` answers:

```text
422 {"error":"js_error","message":"Uncaught TypeError: Deno.core.ops is not a function"}
422 {"error":"js_error","message":"Uncaught TypeError: Deno.core.opSync is not a function"}
```

### Worked example

`requests/fac.txt` stores the factorial script from `requests/fac.js` under
the key `fac`; the script reads its input with
`Deno.core.ops.op_rs3_get("num")`. From the repo root, against a running
server:

```sh
curl -s -X POST http://127.0.0.1:5000/insert -d '{"num":"5"}'
curl -s -X POST http://127.0.0.1:5000/insert -d @requests/fac.txt
curl -s -X PATCH http://127.0.0.1:5000/key/fac
```

The three commands print:

```text
"{\"num\":\"5\"}"
"{\"fac\":\"function factorial(n) { if (n === 0) { return 1; }return n * factorial(n - 1); }let num = Number(Deno.core.ops.op_rs3_get(\\\"num\\\").toString());factorial(num);\"}"
"120"
```

### Results

A successful PATCH answers 200 with the JSON string of `String(result)`,
where `result` is the value of the script's last expression statement:

| Script | Response body |
|--------|---------------|
| `6*20` | `"120"` |
| `let a=1;` | `"undefined"` |
| `({a:1})` | `"[object Object]"` |
| `Promise.resolve(1)` | `"[object Promise]"` |
| `0.1+0.2` | `"0.30000000000000004"` |

A Promise is not awaited, so it answers `"[object Promise]"`. Return a plain
value, or `JSON.stringify(..)` an object yourself.

### Output

`Deno.core.print(text)` writes `text` unchanged to the server's stdout, so a
script can print lines that look like rs3's own output.
`Deno.core.print(text, true)` writes an escaped line prefixed with
`js stderr: ` to stderr; `Deno.core.print("x\ny", true)` gives
`js stderr: "x\ny"`. Scripts are untrusted, and so is everything they print.

The full op contract (argument coercion, nested PATCH, error classes, stored
scripts after upgrading) is in [docs/js-ops.md](docs/js-ops.md).

## Limits

- **Nesting depth limit: 4**, counting the PATCHed key, so a script may nest
  `op_rs3_patch` three times. A chain of 4 scripts answers normally (`k1`
  patches `k2`, which patches `k3`, which patches `k4` = `"leaf"`, answers
  `"leaf"`); the call that would start a 5th is refused, and the PATCH answers
  422 `js_depth_limit` with the key chain:
  `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: j1 -> j2 -> j3 -> j4 -> j5"}`.
  Sibling `op_rs3_patch` calls at the same level are not counted: a script
  that calls `op_rs3_patch` ten times in a loop and ends with `"done"` answers
  `"done"`.
- **Execution timeout: 5000 ms**, counted from the moment a JS worker picks
  the request up (time waiting in the queue does not count). The script,
  nested scripts included, is terminated and the PATCH answers 504
  `js_timeout`. If the stall is inside the host rather than in JavaScript, a
  backstop answers the same 504 2 s later.
- Both are hard stops, even when the script catches the error. Both are set
  in code (`JsSettings`) and cannot be configured at runtime.
- Neither stop rolls back writes. A script that inserts `w2` with
  `op_rs3_insert` and then loops forever answers 504, and `GET /key/w2`
  afterwards answers `"kept"`.
- Other routes keep being answered while a script runs. Other PATCH requests
  run while a JS worker is free (there is one per available CPU); otherwise
  they wait in the queue.
- **There is no heap limit yet** (planned for v2). A script that allocates
  without bound can exhaust V8's heap and abort the whole process.
- The store lives in memory, so every restart (including systemd's restart
  after a crash) starts with an empty store.

## Behaviour changes

Compared with the 2021 version (git tag `msc`):

- **`/list` is sorted and escaped.** Entries are sorted by key in byte order,
  and keys and values are JSON-escaped; the `" : "` spacing is kept. The `,]`
  quirk is gone (2021 listed entries in hash order, unescaped, and stripped
  every `,]`, which corrupted values containing it).
- **A missing key answers 404.** `GET`, `DELETE` and `PATCH /key/{key}` give
  404 `not_found` JSON, also on an empty store (2021 answered `"[]"` on an
  empty store and panicked otherwise).
- **`POST /insert` validates.** A body that is not JSON answers 400
  `malformed_json`, and one that is not an object of strings answers 422
  `invalid_input`; nothing is inserted (2021 panicked, possibly after
  inserting some pairs).
- **The ops are renamed** `rs3_*` → `op_rs3_*` and called as
  `Deno.core.ops.op_rs3_*(...)`, with no `Deno.core.ops()` bootstrap and no
  `opSync`. A missing key throws a catchable `Error`, and bad input a
  `TypeError` (2021 crashed the server).
- **PATCH errors have status codes:** 422 `js_error`, 422 `js_depth_limit`,
  504 `js_timeout` or 500 `internal` (2021 panicked on a script error).
- **The depth limit and the timeout are new** (see Limits).
- **Configuration is strict.** A malformed or BOM-prefixed dotenv file stops
  startup, and only `SERVER.*` is read (2021 ignored dotenv errors and merged
  every environment variable).
- **Startup reports errors cleanly.** The banner prints only after a
  successful bind, and a startup failure is one `error:` line with exit 1
  (2021 printed the banner before binding and panicked on errors).

## Deployment

This section runs rs3 on one Linux server with systemd, using the two example
files in `deploy/`: the unit `deploy/rs3.service` and its environment file
`deploy/rs3.conf`.

### Build

```sh
cargo build --release --locked
```

This produces `target/release/rs3`. The binary dynamically links only libc,
libm and libgcc_s; V8 is linked in statically. Build on the server, or on a
machine with the same or an older glibc. The release binary built on aarch64
(glibc 2.39) requires `GLIBC_2.34` at most; the x86_64 floor has not been
measured.

### Install

```sh
sudo install -m 0755 target/release/rs3 /usr/local/bin/rs3
sudo install -D -m 0644 deploy/rs3.conf /etc/rs3/rs3.conf
sudo install -m 0644 deploy/rs3.service /etc/systemd/system/rs3.service
sudo systemctl daemon-reload
sudo systemctl enable --now rs3
```

### Configuration under systemd

rs3 reads only `SERVER.HOST` and `SERVER.PORT`, but systemd ignores variable
names that contain a dot. So `/etc/rs3/rs3.conf` holds `RS3_HOST` and
`RS3_PORT`, and the unit's `ExecStart` passes them to rs3 through env(1) as
`SERVER.HOST` and `SERVER.PORT`. Edit them there, then run
`sudo systemctl restart rs3`. Under the unit the working directory is `/`, so
rs3 reads no dotenv (`.env`) file unless one exists at `/`. The environment
wins over the values in such a file, but a malformed or unreadable `/.env`
still stops startup.

### Service user and sandbox

`DynamicUser=yes` gives rs3 a transient unprivileged `rs3` user (an existing
static `rs3` user is reused), so no useradd step is needed. The service has no
capabilities, so the port must be 1024 or above. For a lower port, set
`CapabilityBoundingSet=` and `AmbientCapabilities=` to `CAP_NET_BIND_SERVICE`.

The sandbox directives are listed in the unit. Never add the memory
write-execute ban (`MemoryDenyWriteExecute=yes`): V8's JIT needs writable and
executable memory, and rs3 crashes at the first PATCH. `@pkey` is in the
syscall filter because V8 uses memory protection keys on x86_64 CPUs that
have them.

`MemoryMax=1G` caps the whole service, because V8 has no heap limit yet (see
Limits) and each JS worker runs its own isolates. If rs3 grows past the cap,
the kernel kills rs3 inside its own cgroup instead of other services on the
host, and rs3 restarts with an empty store. Size the value to the server.

### Check

```sh
systemctl status rs3
curl -s http://127.0.0.1:5000/
journalctl -u rs3
```

The curl command prints `"Welcome to rs3"`.

### Update

Rebuild, then re-install the binary with the same command as above:

```sh
sudo install -m 0755 target/release/rs3 /usr/local/bin/rs3
sudo systemctl restart rs3
```

`install` replaces the file, so it works while rs3 is running; `cp` fails with
"Text file busy". A restart empties the in-memory store.

### Security note

rs3 has no authentication: anyone who can reach the port can store data and
run JavaScript. Keep the loopback default (`RS3_HOST=127.0.0.1`), or put
something you control in front of it, such as a reverse proxy with
authentication and TLS. TLS is not built in.

Loopback is not private. Every local user and process can still reach rs3
on `127.0.0.1`, even behind a reverse proxy, and so can web pages open in a
browser on the same machine: `POST /insert` accepts any Content-Type, and rs3
does not check the `Host` header. Run rs3 only on a host where you trust all
of them. The unit has commented-out `IPAddressDeny=`/`IPAddressAllow=` lines
that keep it loopback-only even if `RS3_HOST` changes; a reverse proxy on the
same host still connects over loopback.

### Troubleshooting

The unit uses `Type=exec`, so `systemctl enable --now rs3` reports success as
soon as `/usr/bin/env` has started, even when rs3 then exits on bad
configuration or a busy port. The check is `systemctl status rs3` plus the
curl from the Check step.

On a startup failure rs3 writes a single `error:` line, tagged `rs3`, and
exits 1. `journalctl -u rs3` shows it. For example, an empty `RS3_PORT=`
gives:

```text
rs3[<pid>]: error: invalid configuration: invalid type: string "", expected an integer for key `server.port` in the environment
```

If `/etc/rs3/rs3.conf` is missing, systemd itself logs:

```text
Failed to load environment files: No such file or directory
```

After 5 failed starts within 60 s the unit stops retrying, logs the line
below and stays failed:

```text
Start request repeated too quickly.
```

Fix the file, then flush the restart counter and start rs3 again:

```sh
sudo systemctl reset-failed rs3
sudo systemctl restart rs3
```

The systemd.unit documentation (systemd 255) says: "systemctl reset-failed
will cause the restart rate counter for a service to be flushed".

A crash (for example heap exhaustion, which has no limit yet, or an OOM kill
at `MemoryMax=`) restarts rs3 after 2 s with an empty store. Repeated crashes
hit the same start limit. Because any client can trigger such a crash, a
client that crashes rs3 about six times within 60 s takes it down until an
operator runs the two commands above.

## Development

These are the checks CI runs, in the order a contributor runs them locally
(`scripts/check-workflow.py` runs inside `scripts/ci-guards.sh` as guard (g)):

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
bash scripts/ci-guards.sh
bash scripts/ci-smoke.sh
```

`scripts/ci-smoke.sh` needs the release build first: it starts
`target/release/rs3` on a free loopback port and drives it with curl.

Also run the guard self-test locally; CI does not run it:

```sh
bash scripts/test-ci-guards.sh
```

CI is a single job in `.github/workflows/ci.yml` on GitHub's `ubuntu-26.04`
x86_64 runner. It runs the repo guards, `cargo fmt --check`, clippy, the
tests, the release build and the release smoke. The release smoke exercises
the protected V8 platform, which matters on x86_64 CPUs with memory
protection keys. The guards reject lint suppression, ignored tests, legacy op
syntax and a second v8 in the dependency graph. `scripts/check-workflow.py`
pins the workflow to its canonical lines, so change both together.

## License

rs3 is released under the MIT license; see `LICENSE`.
