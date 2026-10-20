# rs3 JavaScript store ops

`PATCH /key/{key}` runs the value stored under `key` as a JavaScript script on
deno_core 0.412 (an embedded V8). Scripts reach the store through five ops on
`Deno.core.ops`. Each op's JavaScript name is the name of its Rust function, so
the five ops are `op_rs3_list`, `op_rs3_get`, `op_rs3_insert`, `op_rs3_delete`
and `op_rs3_patch`.

Every op takes and returns strings. Errors are ordinary JavaScript exceptions
that a script can catch with `try`/`catch`. After a nested host fault, every op
throws `Error: internal error` for the rest of the PATCH (see Errors).

## Migrating from the legacy API

rs3 used to run on a 2021 pre-release of deno_core, where scripts first called
a bootstrap function and then reached the store through `Deno.core.opSync`.
Neither exists any more. Rewrite old scripts with this table:

| Old call | New call | Argument | Returns | Throws when |
|----------|----------|----------|---------|-------------|
| `Deno.core.ops();` | delete the statement | none | none | calling it now throws `TypeError: Deno.core.ops is not a function`, because `Deno.core.ops` is a plain object |
| `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` | none; extra arguments are ignored | the inner `GET /list` text, such as `[{"k" : "v"}]`, sorted by key in byte order; `[]` when the store is empty | only after a nested host fault (see Errors) |
| `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` | the key, coerced to a string | the value | the key is missing: `Error` with message `key not found: <key>` (legacy: the server panicked); after a nested host fault: `Error: internal error` (see Errors) |
| `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` | a JSON object whose values are all strings, passed as a string | the compact echo, such as `{"k":"v"}` | malformed JSON: `TypeError` whose message starts with `malformed JSON:`; wrong shape: `TypeError` whose message starts with `invalid input:`; nothing is written on error; after a nested host fault: `Error: internal error` (see Errors) |
| `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` | the key | the value the key held | the key is missing: `Error` with message `key not found: <key>`; after a nested host fault: `Error: internal error` (see Errors) |
| `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` | the key | `String(completion value)` of that key's script, run in a fresh nested runtime | the key is missing; the nested script fails, rethrown as `Error` with the nested message minus its leading `Uncaught `; the nested runtime hits a host fault, thrown as `Error: internal error`; the whole PATCH then answers 500 even if the script catches it, and every later op throws the same error |

The legacy `Deno.core.opSync` function no longer exists either, and calling it
throws a `TypeError`. The old example script also called an op named `op_list`
that was never registered. Use `Deno.core.ops.op_rs3_list()` instead.

No compatibility shim, alias or wrapper for the legacy calls is provided. This
table is the migration path.

## Errors

- **Missing key.** `op_rs3_get`, `op_rs3_delete` and `op_rs3_patch` throw a
  plain JavaScript `Error` when the key does not exist. Its message is
  `key not found: ` followed by the key, for example `key not found: nope`.
- **Bad insert input.** `op_rs3_insert` checks the whole body before writing
  anything. Malformed JSON throws a `TypeError` whose message starts with
  `malformed JSON:`. Valid JSON of the wrong shape (not an object, or a value
  that is not a string) throws a `TypeError` whose message starts with
  `invalid input:`, for example
  `invalid input: value for key y must be a string`. Nothing from a rejected
  body is written.
- **Object or symbol arguments.** Passing an object or a symbol where an op
  expects a string throws
  `TypeError: op argument must be a string or another primitive value`. The
  object's own `toString` is never called. To insert an object, pass
  `JSON.stringify(obj)`.
- **All of these can be caught.** For example, the script
  `try { Deno.core.ops.op_rs3_get("nope") } catch (e) { e.message }`
  completes with `key not found: nope`. The caught value is a real `Error`:
  `e instanceof Error` is true, and `e.name` is `Error` or `TypeError`.
- **Why there is no NotFound class.** Bare deno_core registers only the
  built-in classes `Error`, `RangeError`, `ReferenceError`, `SyntaxError`,
  `TypeError` and `URIError`. An op error with any other class name, such as a
  custom `NotFound`, would reach the script as `undefined` instead of an error
  object. That is why a missing key is a plain `Error`.
- **Uncaught errors fail the script.** If the script does not catch an op
  error, the script fails and PATCH answers 422 with the body
  `{"error":"js_error","message":"Uncaught Error: key not found: nope"}`.
- **Nested errors.** When the script run by `op_rs3_patch` fails, the caller
  sees an `Error` carrying the nested message minus its leading `Uncaught `.
  A nested `throw new RangeError("inner boom")` arrives as an `Error` with the
  message `RangeError: inner boom`. A nested missing key arrives as
  `key not found: <key>`.
- **Nested host faults.** If the nested runtime itself fails, for example
  because it cannot be created, that is a server fault, not a script error.
  The caller sees an `Error` with the fixed message `internal error` and no
  host details. Catching it does not help: once any nested level has hit a
  host fault, the whole PATCH answers 500 (see PATCH results). From then on,
  every op the script calls (`op_rs3_list`, `op_rs3_get`, `op_rs3_insert`,
  `op_rs3_delete` and `op_rs3_patch`) throws the same `Error: internal error`
  without touching the store, so the script cannot make further writes after
  the request has failed. There are no transactions: store writes the script
  made before the fault stay.

## PATCH results

`PATCH /key/{key}` answers as follows.

- **Success: 200.** The body is the JSON string of `String(completion value)`,
  where the completion value is the value of the script's last expression
  statement:

  | Script ends with | Body |
  |------------------|------|
  | `6*20` | `"120"` |
  | `let a=1;` | `"undefined"` |
  | `({a:1})` | `"[object Object]"` |
  | `Promise.resolve(1)` | `"[object Promise]"` |
  | `null` | `"null"` |
  | `Symbol('x')` | `"Symbol(x)"` |
  | an empty script | `"undefined"` |

  Turning the completion value into a string can run the script's own code,
  such as a `toString`, `valueOf` or `Symbol.toPrimitive` method. If that code
  throws, or the value cannot be converted at all (for example
  `Object.create(null)`), the script fails with a 422 as described below.

  Promises are not awaited and their `.then` callbacks never run. Return a
  plain value, or `JSON.stringify(..)` an object yourself. Awaiting promises is
  planned for v2 (APIV-02).
- **Script error: 422.** A thrown exception, a syntax error, an uncaught op
  error or an exception while converting the completion value to a string
  gives `{"error":"js_error","message":"Uncaught Error: boom"}`. The
  message is the JavaScript exception message. Other examples:
  `Uncaught SyntaxError: Unexpected token ';'`,
  `Uncaught ReferenceError: nope is not defined`, and `Uncaught plain` for
  `throw "plain"`. The stack trace is written to the server's stderr and is
  never returned in a response.
- **Host fault: 500.** If the JS executor is unavailable or a runtime cannot be
  created, the answer is `{"error":"internal","message":"internal error"}`.
  This includes a nested runtime started by `op_rs3_patch` at any depth, even
  when the script caught the error the op threw. Writes made before the fault
  stay; ops called after it are refused. The details go to the server log
  only.
- **Missing key: 404.** PATCH of a key that does not exist answers
  `{"error":"not_found","message":"key not found: <key>"}` without running
  anything.

## Runtime model

- Every PATCH runs its script in a fresh runtime on a dedicated `rs3-js`
  thread, never on an HTTP worker. The runtime is dropped when the script
  finishes.
- Because each script gets its own runtime, top-level `let`, `const`,
  `function` and `class` declarations in two scripts never collide, even when
  one script runs the other through `op_rs3_patch`. Nothing but the store is
  shared between PATCH requests, and nothing survives from one run of a script
  to the next except what it wrote to the store.
- The store is shared. A value written by one script is immediately visible to
  every other script and to the HTTP routes.
- `op_rs3_list()` output is sorted by key in byte order (uppercase before
  lowercase), exactly like `GET /list`, and uses the same text format.

## Arguments and encoding

- String arguments are coerced like JavaScript `String(..)`.
  `Deno.core.ops.op_rs3_get()` with no argument reads the key `undefined`, and
  `op_rs3_get(5)` reads the key `5`. `null` reads `null` and `true` reads
  `true`. Objects and symbols throw a `TypeError` (see Errors). Extra arguments
  are ignored.
- `op_rs3_list()` on an empty store returns `[]`.
- `op_rs3_insert("{}")` returns `{}` and writes nothing.
- Keys and values are exact UTF-8 strings, stored and returned byte for byte
  with no Unicode normalisation. Non-ASCII keys work as script names (a key
  such as `ключ/🔑` can be PATCHed), and error messages keep non-ASCII text,
  for example `key not found: ключ`.

## Sandbox and known limits

- Scripts run in bare deno_core. `fetch`, `require` and `setTimeout` are
  undefined, and there is no file or network access.
- `Deno.core.print(text)` still works. It writes `text` unchanged to the
  server's stdout, not to the HTTP response, so anything a script prints ends
  up in the server's output. `Deno.core.print(text, true)` writes to the
  server's stderr instead. There rs3 escapes the text and writes one line per
  call, prefixed with `js stderr: ` (for example `js stderr: "a\nb"`), so a
  script cannot fake server log lines. Treat everything a script prints, on
  either stream, as untrusted. Error messages and stack traces of failed
  scripts are logged to stderr in the same escaped form.
- `Deno.core.ops.op_panic` is disabled and throws `Error: op is disabled`,
  because a panic inside an op would abort the whole server. Uncaught, it
  gives a 422 with the message `Uncaught Error: op is disabled`.
- **Unbounded nesting until Phase 5.** There is no limit yet on how deep
  `op_rs3_patch` calls can nest. A script that patches itself, directly or
  through a cycle of keys, does not crash the server, but it ends with an
  error only after about 235 (debug build) or 778 (release build) nested
  levels. That costs about 800 MB of memory and 2.7 s of one JS worker per
  request. When the error is a JavaScript exception, the answer is a 422 whose
  message is very long because every level adds its own prefix. Nested
  runtimes share one thread's stack, so deep nesting, or calling
  `op_rs3_patch` from inside deep recursion, can also leave too little stack
  to create the next runtime. That is a host fault and answers 500.
- **Non-terminating scripts until Phase 5.** A script that never finishes,
  such as `while(true){}`, occupies one JS worker indefinitely. Once every
  worker is busy, further PATCH requests queue behind them and wait. The other
  routes keep working, because HTTP workers never run JavaScript.
- Phase 5 adds a nesting depth limit (answered with 422) and an execution
  timeout (answered with 504). Until then, do not store self-patching or
  non-terminating scripts.

## Worked example

The factorial sample in `requests/fac.js` reads its input with
`Deno.core.ops.op_rs3_get("num")`:

```js
function factorial(n) {
    if (n === 0) {
        return 1;
    }
    return n * factorial(n - 1);
}

let num = Number(Deno.core.ops.op_rs3_get("num").toString());
factorial(num);
```

To run it against a server on localhost:5000:

1. Insert the input: `POST /insert` with the body `{"num":"5"}`.
2. Insert the script: `POST /insert` with the contents of `requests/fac.txt`,
   which holds the same script as a JSON string under the key `fac`.
3. Run it: `PATCH /key/fac` answers 200 with the body `"120"`.

`requests/rs3_http_localhost_5000.http` walks through this and the other
samples (sum, store and the binary tree) as REST-client requests.

## Stored scripts after upgrading

The store lives in memory and starts empty on every boot, so nothing written
by an old rs3 version survives the upgrade. After upgrading, migrate your
scripts with the table above and insert them again.
