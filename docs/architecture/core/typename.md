# `core/typename.rs`

`typename.rs` — **taking a declared type apart**, and the only place that knows where a type
string's parentheses are. `split` → `TypeText{head, args, tail}` (all borrowed and trimmed; the
closing paren is the **last** one, because an `ENUM` member may contain one), `base` →
`int(11) unsigned` → `int unsigned` (the parenthesised part dropped, the words after it kept,
which is what puts `timestamp(3) with time zone` on the same match arm as its unparameterised
spelling), `args`, `leading_word` and `is_unsigned`. Three parts of the app read a declared type
and each had its own splitter: `ddl::split_type` (are these two types the same),
`celledit::base_type`/`type_args` (which editor does this column get) and `import::classify`
(what kind of value goes in it) — the first two line-for-line copies that had already drifted on
an unclosed paren, the third splitting on `['(', ' ']` and taking the first word, which is a
fourth answer rather than a simplification. The *questions* stay different — `leading_word`
exists because `classify` matches a keyword list and wants `timestamp` where `ddl` wants
`timestamp without time zone` — but where the parentheses are is answered once.
`is_unsigned` asks the **base**, not the raw text: a `contains("unsigned")` called
`enum('unsigned','signed')` an unsigned column, harmless only because the flag is read on the
integer arm alone. Nothing here understands what is *inside* the parens — that is
`celledit::value_list`, which goes through `sql::skip_noncode` because a member may contain a
quote, a comma or a `)`.
