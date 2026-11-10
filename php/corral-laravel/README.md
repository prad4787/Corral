# corral/laravel

Dev-only Laravel package that streams queries, HTTP client calls, log
messages, rendered views, cache operations and queue jobs to the Corral app.

It is inert unless Corral is running: Corral's PHP prepend file sets
`CORRAL_EVENTS_SOCKET` only while its socket exists. Events are buffered
per request and sent once at the end, over a unix socket only your user can
open.

Install into a project with `corral watch enable` (adds this package as a
`--dev` dependency from Corral's local copy) and remove it with
`corral watch disable`.

Tuning (environment variables):

- `CORRAL_WATCH=queries,http,logs` keeps only those watchers
  (default: `queries,http,logs,views,cache,jobs`)
- `CORRAL_MIN_QUERY_MS=50` only reports queries slower than 50 ms

License: MIT.
