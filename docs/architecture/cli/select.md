# `cli/select.rs`

`cli/select.rs` — which saved connection an invocation means, and whether it may have it.
**One entry point, and it gates**: `select` resolves the id-or-name *and* applies
`Connection::cli_access`, rather than handing a connection back for a caller to remember to
check — the run guard's rule, for the run guard's reason, which is why there is no `find` here
that returns an ungated connection. Both spellings resolve, because a human types the name
(case-insensitively) and a script wants the id's stability. **`#<id>` is the id and nothing
else**; a bare number still matches a name or an id. Without it a numeric name colliding with
another connection's id made that id unreachable — id 5 named "6" and id 6 named "5" left
neither id with a spelling that worked, and the refusal's "use the id instead" pointed back at
the ambiguous string. `#5` never matches a name, not even one literally spelled `#5`
(`a_hash_prefixed_id_names_only_the_id`), and `--connection`'s help names the form.
**Ambiguity is judged over every saved connection, including the ones the CLI cannot use**, so that turning CLI access on for a
second connection later cannot silently repoint an existing script at it
(`ambiguity_counts_connections_the_cli_cannot_use`); two matches are refused rather than
tie-broken, the pair being as likely to be staging and production as duplicates, and the
refusal lists every candidate as `'#4': backup`, the `#<id>` being what it tells the reader to
use instead. `NoConnection`
keeps `Unknown` and `NotExposed` apart because the two sentences send the reader to different
places — "you have not enabled this one" is an instruction, "no such connection" sends them
hunting for a typo they will not find — and `run.rs` gives them different exit codes for the
same reason. `listed` is what `list` prints; `list --all` is what shows the rest, with their
status, so a human can see *why* an expected connection is unreachable.
