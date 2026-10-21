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
rs3 reads no dotenv (`.env`) file unless one exists at `/`, and the
environment wins over such a file anyway.

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

A crash (for example heap exhaustion, which has no limit yet) restarts rs3
after 2 s with an empty store. Repeated crashes hit the same start limit.

## License

rs3 is released under the MIT license; see `LICENSE`.
