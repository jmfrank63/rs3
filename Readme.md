# rs3

rs3 is a small Rust key/value store. An actix-web HTTP server keeps string
key/value pairs in an in-memory map, and `PATCH /key/{key}` runs the stored
value as JavaScript in an embedded V8 (deno_core), whose ops call back into the
store. It started as a 2021 Birkbeck MSc Computer Science project and now
builds on Rust 1.99.1 / edition 2024.

The original 2021 MSc version, including its multi-container deployment setup,
is preserved at git tag `msc`. `Vagrantfile`, `aws/` and `experiments/` belong
to that 2021 setup and are not maintained for the current version.

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
