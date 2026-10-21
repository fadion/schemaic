# `schemaic-ui`

`schemaic-ui` — the Floem UI. The central `Ui` struct (threaded everywhere) is split per-domain:
`Copy` signal bundles (`TabsUi`/`SchemaUi`/`ConnUi`/`AiUi`/`TermUi`/`LayoutUi`/`OverlayUi`) +
`Rc<…Actions>` callback bundles — so `ui.run` is `ui.tab_actions.run`, `ui.db_nodes` is
`ui.schema.db_nodes`, the tabs signal is `ui.tabs_ui.tabs`. Modules:
