# @huliho/i18n

Message catalogs and the shared i18n helpers, consumed by every app in
the workspace. The catalogs live in `messages/` as ICU MessageFormat
JSON, one file per locale (`en` is the base, `nl` the second locale);
the inlang project in `project.inlang/` binds them to the Paraglide
compiler that each app runs in its own build.

`relativeTime(locale, at, now)` says how long ago a moment was in the
largest unit that fits ("yesterday", "3 weeks ago") through
`Intl.RelativeTimeFormat`. Under a minute it returns null so the
caller chooses its own words for "now".

`pnpm build` at the repo root compiles the package and then validates
the catalogs: every locale must carry exactly the base locale's keys,
each a non-empty, parsable message. The same step generates the
`en-XA` pseudo catalog from `en`, so accented, widened text reveals
any string that skipped the catalogs. Unit tests run with `pnpm test`.
