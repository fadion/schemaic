# Schemaic — Albanian catalogue.
#
# Machine-drafted and unreviewed (docs/roadmap.md); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Albanian.

## Settings › Appearance › Language

settings-language = Gjuha
settings-language-system = Sistemi ({ $language })
settings-language-restart = Ndryshimi i gjuhës hyn në fuqi herën tjetër që niset Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Madhësia e shkronjave
settings-term-cursor-style = Stili i kursorit
settings-term-copy-on-select = Kopjo kur përzgjidhet
settings-term-copy-on-select-hint = Kopjoni tekstin e përzgjedhur në kujtesë sapo të përfundojë përzgjedhja.
settings-term-blink = Kursor që pulson
settings-term-blink-hint = Kursori pulson kur terminali është në fokus.

## Settings › AI

settings-ai-agent = CLI e agjentit
settings-ai-disabled = Çaktivizuar
settings-ai-off-hint = Asgjë nuk i dërgohet asnjë agjenti, asnjë CLI agjenti nuk niset dhe nuk ofrohet asnjë veprim AI. Zgjidhni një CLI për të aktivizuar asistentin.
settings-ai-cli-path = Shtegu i { $cli }
settings-ai-cli-path-placeholder = Lëreni bosh për zbulim automatik
settings-ai-detected = Zbuluar automatikisht: { $path }
settings-ai-detect-failed = Zbulimi automatik dështoi. { $cli } nuk u gjet — jepni shtegun e tij më sipër.
settings-ai-cli-path-missing = Skedari nuk ekziston.
settings-ai-model = Modeli
settings-ai-model-placeholder = Lëreni bosh për modelin e paracaktuar të mjedisit të agjentit
settings-ai-model-hint = Çdo id që pranon kjo CLI — një pseudonim ose një version i datuar për ta mbajtur të njëjtin edhe pas përditësimeve.
settings-ai-model-hint-with-chips = Çdo id që pranon kjo CLI — një pseudonim ose një version i datuar për ta mbajtur të njëjtin edhe pas përditësimeve. Butonat janë shkurtore, jo e gjithë lista.
settings-ai-effort = Përpjekja
settings-ai-instructions = Udhëzime të personalizuara
settings-ai-instructions-placeholder = Dialekti, konventat, rregullat e ekipit…
settings-ai-schema-context = Konteksti i skemës
settings-ai-schema-context-hint = Sa nga struktura e bazës së të dhënave shoqëron çdo mesazh. Me “Asnjë” mbahen mënjanë edhe mjetet e skemës, kështu asistenti ju pyet për emrat në vend që ta lexojë vetë katalogun.
settings-ai-data-access = Qasja në të dhëna
settings-ai-data-access-where = Caktohet për çdo lidhje, te cilësimet e lidhjes.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Vetëm skema
ai-data-on-request = Vetëm çfarë bashkëngjis
ai-data-full = Lejoje të lexojë të dhëna
ai-data-schema-only-hint = Vetëm struktura — emrat, tipet dhe përkufizimet. Asnjë rresht nuk largohet nga ky kompjuter dhe bashkëngjitja refuzohet.
ai-data-on-request-hint = Asistenti nuk lexon vetë asnjë të dhënë. Rreshtat që bashkëngjitni nga një rezultat largohen nga ky kompjuter bashkë me atë pyetje.
ai-data-full-hint = Asistenti mund të ekzekutojë vetë kërkesa vetëm për lexim dhe të lexojë rreshta shembull. Çdo gjë që lexon largohet nga ky kompjuter.
settings-ai-gutter = Vijë theksuese te përgjigjet
settings-ai-gutter-hint = Shënoni përgjigjet e asistentit me një vijë me ngjyrë përgjatë skajit të tyre të djathtë. Kur është çaktivizuar, ato kanë të njëjtën hapësirë në të dyja anët.

## Settings › section list

settings-section-general = Të përgjithshme
settings-section-editor = Redaktori
settings-section-formatting = Formatimi i kodit
settings-section-query = Kërkesa
settings-section-terminal = Terminali
settings-section-ai = Asistenti AI
settings-section-appearance = Pamja

## Settings › shared

settings-off = Joaktiv

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } karakter
   *[other] { $n } karaktere
}

## Settings › General

settings-log = Skedari i regjistrit
settings-log-hint = Diagnostikimi, përfshirë rrëzimet, shkruhet në { $path }.
settings-log-hint-none = Në këtë kompjuter nuk u hap dot asnjë skedar regjistri, prandaj nuk po shkruhet asgjë në të.
settings-log-open-folder = Hap dosjen
settings-bundle = Skedari i cilësimeve
settings-bundle-hint = Preferencat dhe fragmentet tuaja në një skedar, për t’i marrë në një kompjuter tjetër. Lidhjet, paraqitja e dritares, shell-i i terminalit dhe fragmentet e lidhura me një lidhje të caktuar mbeten këtu.
settings-bundle-export = Eksporto
settings-bundle-import = Importo
settings-cli = Linja e komandave
settings-cli-install = Instalo
settings-cli-installing = Po instalohet
settings-cli-remove = Hiq
settings-cli-removing = Po hiqet

## Settings › modal

settings-title = Cilësimet
settings-restore-tabs = Rikthe skedat gjatë nisjes
settings-restore-tabs-hint = Rihapni skedat e kërkesave nga seanca juaj e fundit kur niset aplikacioni.

## Settings › Editor

settings-editor-font-size = Madhësia e shkronjave
settings-editor-predictions = Parashikime në rresht
settings-editor-predictions-hint = Shfaqni fjalët e mundshme pasuese pas kursorit — çiftin e një fjale kyçe, çelësin e jashtëm të një join-i, kolonat për grupim ose futje. Tab, ose → në fund të rreshtit, i merr të plota, Ctrl+→ (⌘→ në macOS) fjalë pas fjale, dhe Escape i refuzon.
settings-editor-remembered = Parashikime nga urdhrat e mëparshëm
settings-editor-remembered-hint = Ofroni edhe pjesën tjetër të një rreshti nga një urdhër që keni ekzekutuar, keni shkruar në një skedë tjetër ose keni ruajtur si fragment. Çaktivizojeni që urdhrat e mëparshëm të mos shfaqen në ekran.
settings-editor-aliases = Pseudonime tabelash
settings-editor-aliases-hint = Kur plotësimi shkruan një tabelë pas FROM ose JOIN, shtoni pas saj një pseudonim të shkurtër nga inicialet — orders o, order_items oi — dhe përdoreni pseudonimin në kushtin ON të join-it.
settings-editor-rainbow = Kllapa ylber
settings-editor-rainbow-hint = Ngjyrosni çdo çift kllapash sipas thellësisë së folezimit, që kllapa mbyllëse të lidhet me hapësen e saj me një shikim.
settings-editor-whitespace = Shfaq hapësirat
settings-editor-whitespace-hint = Vizatoni hapësirat dhe tabulimet si shenja të zbehta, që një tabulim të dallohet nga hapësirat.
settings-editor-guides = Udhëzuesit e dhëmbëzimit
settings-editor-guides-hint = Vizatoni një vijë vertikale të zbehtë në çdo nivel dhëmbëzimi.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Shkronjat e fjalëve kyçe
settings-format-case-keep = Siç janë shkruar
settings-format-case-upper = Të mëdha
settings-format-case-lower = Të vogla
settings-format-commas-trailing = Në fund të rreshtit
settings-format-commas-leading = Në fillim të rreshtit
settings-format-gap-one = Një rresht bosh
settings-format-gap-none = Asnjë rresht bosh
settings-format-keyword-case-hint = Si i shkruan “Formato kodin” fjalët kyçe si SELECT dhe FROM. Emrat, vargjet dhe komentet ruajnë shkronjat e tyre.
settings-format-function-case = Shkronjat e funksioneve
settings-format-function-case-hint = Si i shkruan “Formato kodin” funksionet e vetë motorit, si COUNT dhe COALESCE, aty ku thirren. Funksionet tuaja ruajnë shkronjat e tyre.
settings-format-type-case = Shkronjat e tipeve
settings-format-type-case-hint = Si i shkruan “Formato kodin” tipet e të dhënave të vetë motorit, si INT dhe VARCHAR. Tipet që keni krijuar ruajnë shkronjat e tyre.
settings-format-commas = Presjet
settings-format-commas-hint = Ku e vendos “Formato kodin” presjen e një liste që e ndan në disa rreshta. Listat e mbajtura në një rresht, si argumentet e një funksioni, nuk preken, përveç nëse i ndan gjerësia e mbështjelljes.
settings-format-statement-gap = Ndërmjet urdhrave
settings-format-statement-gap-hint = Rreshtat bosh që “Formato kodin” lë ndërmjet dy urdhrave. “Siç janë shkruar” mban një rresht bosh kudo ku ka pasur.
settings-format-wrap = Gjerësia e mbështjelljes
settings-format-wrap-hint = Një listë që “Formato kodin” e mban në një rresht, si argumentet e një funksioni, kalon në një element për rresht sapo rreshti i saj do ta kalonte këtë gjerësi.
settings-format-river = Rreshto djathtas fjalët kyçe të klauzolave
settings-format-river-hint = Rreshtoni SELECT, FROM, WHERE dhe të tjerat sipas skajit të tyre të djathtë, me trupin e çdo klauzole pranë fjalës së saj kyçe, në vend që fjala kyçe të qëndrojë në një rresht më vete.
settings-format-align = Rreshto kolonat
settings-format-align-hint = Rreshtoni AS-të e pseudonimeve në një listë SELECT, = e caktimeve të një UPDATE dhe tipet e kolonave të një tabele.
settings-format-join-on = Kushti i join-it në rresht më vete
settings-format-join-on-hint = Vendosni ON të një join-i, si dhe AND dhe OR të kushtit të tij, në një rresht më vete nën join.

## Settings › Query

settings-query-row-limit = Kufiri i paracaktuar i rreshtave
settings-query-timeout = Koha maksimale e urdhrit
settings-query-timeout-hint = Anuloni një urdhër që zgjat më shumë se kaq. Çaktivizuar si parazgjedhje — një import ose një raport mund të zgjasë me të drejtë një kohë.
timeout-off = Pa kufi kohe
timeout-hours = { $n ->
    [one] { $n } orë
   *[other] { $n } orë
}
timeout-minutes = { $n ->
    [one] { $n } minutë
   *[other] { $n } minuta
}
timeout-seconds = { $n ->
    [one] { $n } sekondë
   *[other] { $n } sekonda
}
settings-query-confirm-writes = Konfirmo para se të ekzekutohen shkrimet
settings-query-confirm-writes-hint = Pyetni para se të ekzekutohet çdo urdhër që ndryshon të dhënat ose skemën.
settings-query-validate = Vërtetim i drejtpërdrejtë në bazën e të dhënave
settings-query-validate-hint = Kontrolloni urdhrin nën kursor kundrejt bazës së të dhënave ndërsa shkruani (një PREPARE që nuk ekzekutohet) për të nxjerrë gabimet e sakta. Shton një shkëmbim me bazën e të dhënave në çdo pauzë.
settings-query-notify = Njofto kur përfundon një ekzekutim në sfond
settings-query-notify-hint = Shfaqni një njoftim në desktop kur një kërkesë, skript, eksport ose dump që zgjati një sekondë a më shumë përfundon ndërsa Schemaic nuk është dritarja aktive. Butoni në shiritin e detyrave theksohet gjithsesi. Njoftimi nuk përmban kurrë SQL-në.
notify-query-finished = Kërkesa përfundoi
notify-query-failed = Kërkesa dështoi
notify-script-finished = Skripti përfundoi
notify-script-failed = Skripti dështoi
notify-export-finished = Eksporti përfundoi
notify-export-failed = Eksporti dështoi
notify-dump-finished = Dump-i përfundoi
notify-dump-failed = Dump-i dështoi
notify-took = Zgjati { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Ruaj vlerat në regjistrin e shkrimeve
settings-query-write-log-hint = Çdo shkrim që ekzekuton Schemaic ose schemaic exec i shtohet write_log.jsonl në dosjen e cilësimeve dhe nuk fshihet kurrë. Vlerat në SQL-në e tij maskohen, përveç nëse kjo është aktive; një fjalëkalim maskohet gjithsesi.

## Settings › Appearance

settings-appearance-ui-theme = Tema e ndërfaqes
settings-appearance-editor-theme = Tema e redaktorit
settings-appearance-scale = Shkalla e ndërfaqes

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Shkurtoret

## Shortcuts modal › group headings

shortcuts-group-global = Të përgjithshme
shortcuts-group-editor = Redaktori
shortcuts-group-results-grid = Rrjeta e rezultateve
shortcuts-group-schema-tree = Pema e skemës
shortcuts-group-ai-panel = Paneli AI
shortcuts-group-terminal = Terminali
shortcuts-group-er-diagram = Diagrami ER

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Gjej kudo
shortcuts-command-palette = Paleta e komandave
shortcuts-new-tab = Skedë e re kërkese
shortcuts-close-tab = Mbyll skedën e kërkesës
shortcuts-next-tab = Skeda pasuese
shortcuts-previous-tab = Skeda e mëparshme
shortcuts-jump-to-tab = Kalo te skeda
shortcuts-reopen-tab = Rihap skedën e fundit të mbyllur
shortcuts-open-file = Hap skedar SQL
shortcuts-save-file = Ruaj skedarin SQL
shortcuts-save-file-as = Ruaj skedarin SQL si
shortcuts-toggle-schema = Shfaq/fshih panelin e skemës
shortcuts-toggle-ai = Shfaq/fshih panelin AI
shortcuts-toggle-terminal = Shfaq/fshih terminalin
shortcuts-run-query = Ekzekuto kërkesën
shortcuts-autocomplete = Plotësim automatik
shortcuts-inline-ai = Redaktim AI në vend
shortcuts-find-in-editor = Gjej në redaktor
shortcuts-find-replace = Gjej dhe zëvendëso
shortcuts-go-to-line = Shko te rreshti
shortcuts-matching-bracket = Kalo te kllapa përkatëse ose te BEGIN/END
shortcuts-line-comment = Komento/çkomento rreshtin
shortcuts-block-comment = Komento/çkomento bllokun
shortcuts-toggle-case = Ndërro shkronjat e mëdha/të vogla
shortcuts-join-lines = Bashko rreshtat
shortcuts-duplicate = Dyfisho rreshtin / përzgjedhjen
shortcuts-cut = Prit rreshtin / përzgjedhjen
shortcuts-format-code = Formato kodin
shortcuts-format-statement = Formato urdhrin
shortcuts-single-line = Bëje në një rresht
shortcuts-move-line = Lëviz rreshtin lart / poshtë
shortcuts-quick-fix = Zbato rregullimin e shpejtë nën kursor
shortcuts-indent = Rrit / zvogëlo dhëmbëzimin
shortcuts-take-prediction = Merr parashikimin në rresht (edhe me → në fund të rreshtit) / fjalën e tij pasuese
shortcuts-find-in-results = Gjej në rezultate
shortcuts-go-to-row = Shko te rreshti
shortcuts-copy = Kopjo
shortcuts-paste-staged = Ngjit si ndryshime në pritje
shortcuts-select-all = Përzgjidh të gjitha
shortcuts-first-last-cell = Qeliza e parë / e fundit
shortcuts-edit-cell = Redakto qelizën
shortcuts-next-cell = Qeliza pasuese / e mëparshme gjatë redaktimit
shortcuts-commit-edits = Konfirmo ndryshimet
shortcuts-mark-deletion = Shëno rreshtin për fshirje
shortcuts-results-toolbar = Shko te shiriti i veglave të rezultateve
shortcuts-along-toolbar = Lëviz përgjatë shiritit të veglave (Esc kthehet te rrjeta)
shortcuts-open-selected = Hap tabelën, kolonën ose objektin e përzgjedhur
shortcuts-context-menu = Menuja e kontekstit për rreshtin e përzgjedhur
shortcuts-send-message = Dërgo mesazhin
shortcuts-new-line = Rresht i ri
shortcuts-recall-prompt = Rikujto mesazhin e mëparshëm / pasues
shortcuts-copy-paste = Kopjo / ngjit
shortcuts-find-in-diagram = Gjej në diagram

## Key names in shortcut labels — a keyboard's own legends, so a language
## whose keyboards print another word uses it (German: Strg, Umschalt, Entf).
## Keys written the same everywhere (letters, arrows, Tab, Esc, F6) and macOS's
## Cmd are not here. Read key by key by `shortcuts::keys_label`.

key-ctrl = Ctrl
key-shift = Shift
key-alt = Alt
key-enter = Enter
key-del = Del
key-home = Home
key-end = End
key-space = Hapësirë

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Shfaq/fshih panelin
palette-toggle-panel-hint = skema · ai · terminali · historiku i kërkesave · aktiviteti i serverit
palette-panel-schema = Skema
palette-panel-ai = AI
palette-panel-terminal = Terminali
palette-panel-history = Historiku i kërkesave
palette-panel-activity = Aktiviteti i serverit
palette-new-tab = Skedë e re
palette-export-settings = Eksporto cilësimet
palette-import-settings = Importo cilësimet
palette-duplicate-tab = Dyfisho skedën
palette-close-tab = Mbyll skedën
palette-reopen-closed-tab = Rihap skedën e mbyllur
palette-reopen-closed-tab-hint = <skeda>
palette-open-file = Hap skedar
palette-open-file-hint = një skript .sql
palette-save-file = Ruaj skedarin
palette-save-file-as = Ruaj skedarin si
palette-next-tab = Skeda pasuese
palette-previous-tab = Skeda e mëparshme
palette-format-code = Formato kodin
palette-format-statement = Formato urdhrin
palette-format-statement-hint = urdhri te kursori
palette-make-single-line = Bëje në një rresht
palette-make-single-line-hint = përzgjedhja ose urdhri, në një rresht
palette-run = Ekzekuto
palette-run-hint = ekzekuto të gjitha urdhrat
palette-go-to-line = Shko te rreshti
palette-go-to-line-hint = <numri>
palette-go-to-statement = Shko te urdhri
palette-search-hint = <kërkimi>
palette-history = Historiku
palette-clear-history = Pastro historikun
palette-clear-history-hint = lidhja aktuale
palette-ask-ai = Pyet AI-në
palette-ask-ai-hint = <mesazhi>
palette-ask-ai-empty = Shkruani një mesazh…
palette-terminal = Terminali
palette-terminal-hint = <komanda>
palette-run-in-terminal = Ekzekuto në terminal
palette-terminal-empty = Shkruani një komandë…
palette-ui-theme = Tema e ndërfaqes
palette-ui-theme-hint = e çelët · e errët
palette-editor-theme = Tema e redaktorit
palette-editor-theme-hint = <tema>
palette-font-size = Madhësia e shkronjave
palette-increase-font-size = Rrit madhësinë e shkronjave
palette-decrease-font-size = Zvogëlo madhësinë e shkronjave
palette-indent-style = Stili i dhëmbëzimit
palette-indent-style-hint = tabulime · hapësira
palette-indent-spaces = Hapësira
palette-indent-tabs = Tabulime
palette-indent-width = Gjerësia e dhëmbëzimit
palette-toggle-word-wrap = Aktivizo/çaktivizo mbështjelljen e fjalëve
palette-toggle-whitespace = Shfaq/fshih hapësirat
palette-toggle-whitespace-hint = shfaq hapësirat dhe tabulimet
palette-toggle-indent-guides = Shfaq/fshih udhëzuesit e dhëmbëzimit
palette-switch-connection = Ndërro lidhjen
palette-switch-connection-hint = <lidhja>
palette-server-variables = Variablat e serverit
palette-server-variables-hint = cilësimet e lidhjes aktive
palette-enter-number = Jepni një numër
palette-nothing-found = Nuk u gjet asgjë

## Command palette › what a result row is

palette-row-snippet = fragment
palette-row-open-tab = skedë e hapur
palette-row-history = historik
palette-row-view-body = trupi i pamjes
palette-row-body = trupi ({ $kind })
palette-row-trigger-on = trigger në { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Hap në CLI
tree-open = Hap
tree-open-in-new-tab = Hap në skedë të re
tree-copy-name = Kopjo emrin
tree-copy-qualified-name = Kopjo emrin e plotë
tree-properties = Vetitë
tree-used-by = Përdoret nga
tree-live-monitor = Monitor i drejtpërdrejtë
tree-er-diagram = Diagrami ER
tree-compare-with = Krahaso me
tree-generate = Gjenero
tree-generate-ddl = Gjenero DDL
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Rifresko
tree-collapse-all = Palos të gjitha
tree-favourite = Shto te të preferuarat
tree-unfavourite = Hiq nga të preferuarat
tree-colour = Ngjyra
tree-colour-none = Asnjë
tree-hide = Fshih
tree-import = Importo
tree-export = Eksporto
tree-back-up = Krijo kopje rezervë
tree-restore-from-file = Rikthe nga skedari
tree-copy-to = Kopjo te
tree-create = Krijo
tree-create-table = Tabelë
tree-create-view = Pamje
tree-create-type = Tip
tree-create-domain = Domen
tree-create-sequence = Sekuencë
tree-create-synonym = Sinonim
tree-create-alias-type = Tip alias
tree-create-xml-schema-collection = Koleksion skemash XML
tree-create-function = Funksion
tree-create-procedure = Procedurë
tree-create-event = Ngjarje
tree-create-extension = Zgjerim
tree-create-database = Bazë të dhënash
tree-create-schema = Skemë
tree-create-kind = Krijo { $kind }
tree-edit-kind = Redakto { $kind }
tree-folder-create-extension = Krijo zgjerim
tree-edit-table = Redakto tabelën
tree-edit-view = Redakto pamjen
tree-edit-column = Redakto kolonën
tree-edit-index = Redakto indeksin
tree-edit-primary-key = Redakto çelësin primar
tree-edit-foreign-key = Redakto çelësin e jashtëm
tree-triggers = Trigger-ët
tree-triggers-count = Trigger-ët ({ $n })
tree-refresh-view = Rifresko pamjen
tree-truncate = Zbraz
tree-truncate-title = Zbraz tabelën
tree-truncate-rows = { $n ->
    [one] Të fshihet rreshti { $rows } në { $table }? Ky veprim nuk mund të zhbëhet.
   *[other] Të fshihen të gjithë { $rows } rreshtat në { $table }? Ky veprim nuk mund të zhbëhet.
}
tree-truncate-every-row = Të fshihet çdo rresht në { $table }? Ky veprim nuk mund të zhbëhet.
tree-drop = Fshi
tree-drop-index = Fshi indeksin
tree-drop-foreign-key = Fshi çelësin e jashtëm
tree-ai-explain = Shpjegim me AI
tree-users = Përdoruesit dhe privilegjet
tree-server-variables = Variablat e serverit
tree-new-database = Krijo bazë të dhënash
tree-show-table-sizes = Shfaq madhësitë e tabelave

## Connection switcher

conn-menu-manage = Menaxho lidhjet

## Shared dialog buttons

dialog-cancel = Anulo
dialog-yes = Po
dialog-no = Jo

## Open-transaction prompt

tx-prompt-title = Transaksion i hapur
tx-prompt-body = { $tab } ka një transaksion të hapur me { $n ->
    [one] 1 urdhër
   *[other] { $n } urdhra
} në të. Mund ta konfirmoni për të mbajtur ndryshimet, ose ta riktheni për t’i hedhur poshtë.
tx-prompt-body-aborted = { $tab } ka një transaksion që u ndërpre nga një urdhër i dështuar, me { $n ->
    [one] 1 urdhër
   *[other] { $n } urdhra
} në të. Ai mund vetëm të rikthehet, gjë që i hedh poshtë ndryshimet.
tx-prompt-rollback = Rikthe
tx-prompt-commit = Konfirmo

## Error modal

error-modal-title = Gabim
error-modal-none = Nuk ka gabim.
error-modal-ai-fix = Rregullim me AI
error-modal-explain = Shpjego

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = kërkesa dështoi: { $error }
db-connection-failed = lidhja dështoi: { $error }
db-query-cancelled = kërkesa u anulua

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Nuk u lidh — SQL-ja fillestare e kësaj lidhjeje u refuzua: { $why }
db-refused-transport-entra = Hyrja me Microsoft Entra kërkon SSL / TLS në nivelin Verify full: token-i i saj hap çdo bazë të dhënash Azure SQL që llogaria mund të arrijë, prandaj serveri duhet të vërtetojë emrin e tij para se t’i jepet një i tillë.
db-refused-transport-aws = Hyrja me AWS IAM kërkon SSL / TLS të paktën në nivelin “Kërkohet”: token-i i saj është një fjalëkalim për këtë server dhe nuk duhet të kalojë rrjetin i pashifruar.
db-refused-transport-gcp = Hyrja me Google Cloud IAM kërkon SSL / TLS në nivelin Verify CA, me CA-në e serverit të instancës si Root CA: token-i i saj hap çdo instancë Cloud SQL që llogaria mund të arrijë.
db-refused-transport = Kjo hyrje kërkon një mënyrë SSL / TLS më të fortë se ajo e lidhjes.
db-refused-backup-in-terminal = Kopja rezervë e një baze të dhënash serveri bëhet me mjetet e vetë serverit, në terminal.
db-refused-commit-aborted = transaksioni ishte ndërprerë tashmë — një urdhër në të dështoi ose u ndal, kështu konfirmimi do ta hidhte poshtë punën në vend që ta ruante. Nuk u konfirmua asgjë.
db-refused-sqlite-no-manual = Lidhjet SQLite nuk e mbështesin ende mënyrën manuale të transaksioneve — urdhrat ekzekutohen dhe konfirmohen sapo dërgohen
db-refused-mssql-read-only-auto = Një lidhje SQL Server vetëm për lexim punon në mënyrën Auto: SQL Server nuk ka transaksion vetëm për lexim ku të mbahet një skedë Manual, dhe çdo urdhër që ekzekuton mënyra Auto rikthehet
db-refused-client-charset = u refuzua: grupi i karaktereve të klientit të seancës ({ $charset }) mund të fshehë një thonjëz brenda një karakteri, kështu kontrolli i urdhrit nuk mund ta lexojë ashtu si do ta lexonte serveri
db-refused-sql-mode = u refuzua: sql_mode i serverit ({ $mode }) i lexon thonjëzat ndryshe nga kontrolli i urdhrit dhe nuk mund të ndryshohej për këtë seancë
db-refused-writing-plan = Planet këtu ekzekutohen në një transaksion vetëm për lexim, dhe MySQL e MariaDB nuk planifikojnë një urdhër që shkruan brenda tij. Planifikoni në vend të kësaj formën SELECT të urdhrit.
db-refused-writing-plan-function = Planet këtu ekzekutohen në një transaksion vetëm për lexim, dhe MySQL e MariaDB nuk planifikojnë një urdhër që shkruan brenda tij. Një funksion që ai thërret shkruan, gjë që një plani këtu nuk i lejohet.
db-refused-windows-build-only = Hyrja me Windows ofrohet vetëm në versionin e Schemaic për Windows.
db-refused-cloud-iam-engine = Hyrja me Cloud IAM është për lidhjet PostgreSQL dhe MySQL.
db-refused-mssql-cancel-needs-kill = SQL Server nuk mund të anulojë urdhrin e një seance tjetër pa e përfunduar seancën — përdorni “Përfundo seancën”.
db-refused-blank-value = { $has_row ->
        [some] Rreshti { $row } ka një vlerë bosh ose të zbrazët për { $column } ({ $sql_type }): SQL Server do ta ruante si 0 ose 1900-01-01 në vend që ta refuzonte. Importoni fushat bosh si NULL, ose plotësojini.
       *[none] Një vlerë bosh ose e zbrazët nuk mund të shkruhet në { $column } ({ $sql_type }): SQL Server do ta ruante si 0 ose 1900-01-01 në vend që ta refuzonte. Vendoseni qelizën në NULL, ose shkruani një vlerë.
    }
db-refused-code-page = { $has_row ->
        [some] Rreshti { $row } ka një vlerë për { $column } ({ $sql_type }) që
       *[none] Një vlerë për { $column } ({ $sql_type })
    } përmban { $has_lost ->
        [some] `{ $character }`, për të cilin faqja e kodit { $code_page } nuk ka karakter
       *[none] një karakter për të cilin faqja e saj e kodit nuk ka karakter
    }: SQL Server do ta ruante si `?` ose si një karakter të ngjashëm në vend që ta refuzonte. Ruajeni kolonën si nvarchar, ose hiqni karakterin.
db-refused-sqlite-no-file = nuk është caktuar asnjë skedar baze të dhënash
db-refused-sqlite-memory = :memory: nuk është skedar baze të dhënash — SQLite e mban një bazë të dhënash në memorie vetëm për aq kohë sa jeton lidhja që e krijoi, dhe ky aplikacion hap një lidhje të re për çdo veprim, kështu çdo shkrim do të raportohej si i ruajtur dhe pastaj do të hidhej poshtë. Jepni shtegun e një skedari.
db-refused-sqlite-uri = { $file }: ky është një URI, jo skedar baze të dhënash — SQLite do ta lexonte pjesën pas `?` si opsione hapjeje, dhe `?mode=memory` hap një bazë të dhënash të përkohshme që pranon çdo shkrim dhe nuk ruan asnjë. Jepni shtegun e vetë skedarit.
db-refused-sqlite-worker = procesi ndihmës i SQLite dështoi: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } nuk është kolonë e kësaj tabele, prandaj nuk ka vlerë të paracaktuar për të shkruar
db-refused-sqlite-generated = { $column } është kolonë e gjeneruar: nuk ka vlerë të paracaktuar ku të rikthehet, dhe SQLite e llogarit nga rreshti
db-refused-sqlite-sizes-too-large = Madhësitë e tabelave lexohen vetëm për një bazë të dhënash deri në { $cap }: SQLite i mat duke lexuar çdo faqe të skedarit, dhe ky është { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = koha mbaroi
db-connect-init-failed = SQL-ja fillestare `{ $statement }` dështoi: { $error }
db-connect-session-retired = lidhja e kësaj skede u mbyll pas një “Ndalo” që serveri nuk e pranoi, dhe nuk u hap dot një e re; transaksioni i saj u rikthye. Kaloni skedën në Auto dhe pastaj përsëri në Manual për të hapur një tjetër.
db-connect-begin-failed = transaksioni nuk mund të nisej: { $error }
db-connect-prepare-session = seanca nuk mund të përgatitej: { $error }
db-connect-read-only-setup = seanca nuk mund të bëhej vetëm për lexim: { $error }
db-connect-guard-session = seanca nuk mund të mbrohej: { $error }
db-connect-plan-setup = plani nuk mund të përgatitej: { $error }
db-connect-no-maintenance-db = nuk arrihet asnjë bazë të dhënash mirëmbajtjeje
db-connect-browser-silent = SQL Server Browser në { $server } nuk dha port për instancën { $instance }: shërbimi Browser nuk po punon, porti UDP 1434 është i bllokuar gjatë rrugës, ose asnjë instancë atje nuk ka atë emër. Nëse instanca dëgjon në një port fiks, vendoseni hostin { $server } pa \{ $instance } dhe portin atë port.
db-connect-browser-without-tcp = SQL Server Browser në { $server } u përgjigj për instancën { $instance }, por nuk dha port TCP: instanca nuk dëgjon në TCP. Aktivizoni TCP/IP për të në SQL Server Configuration Manager dhe riniseni.
db-connect-azure-timed-out = Azure SQL nuk e përfundoi hyrjen brenda { $secs } sekondave. Një bazë të dhënash serverless që është pezulluar i duhet rreth një minutë për t’u rifilluar, dhe kjo përpjekje e ka zgjuar — lidhuni sërish pas pak.
db-connect-azure-unavailable = { $server } Një bazë të dhënash serverless që është pezulluar i duhet rreth një minutë për t’u rifilluar, dhe kjo përpjekje e ka zgjuar — lidhuni sërish pas pak.
db-ssh-connect-no-answer = Lidhja SSH me { $host }: asnjë përgjigje brenda { $secs } s
db-ssh-auth-no-answer = Vërtetimi SSH me { $host }: asnjë përgjigje brenda { $secs } s
db-ssh-auth-error = Gabim vërtetimi SSH: { $error }
db-ssh-auth-failed = Vërtetimi SSH dështoi
db-ssh-key-load = Ngarkimi i çelësit SSH dështoi: { $error }
db-ssh-key-auth = Gabim vërtetimi me çelësin SSH: { $error }
db-ssh-agent = Gabim i agjentit SSH: { $error }
db-ssh-agent-empty = Agjenti SSH nuk ka identitete të ngarkuara (ekzekutoni `ssh-add`)
db-ssh-agent-auth = Gabim vërtetimi i agjentit SSH: { $error }
db-ssh-no-agent = nuk u gjet asnjë agjent SSH (nisni shërbimin OpenSSH Authentication Agent, ose ekzekutoni Pageant)
db-ssh-no-agent-socket = nuk ka agjent SSH ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = Hosti ndërmjetës SSH nuk mundi të arrinte { $host }: { $error }
db-ssh-connect-to = Lidhja SSH me { $host } dështoi: { $error }
db-ssh-connect = Lidhja SSH dështoi: { $error }
db-ssh-no-host = Tuneli SSH nuk ka host
db-ssh-bind = lidhja e tunelit lokal me portin dështoi: { $error }
db-ssh-named-instance = Një instancë me emër nuk mund të arrihet përmes një tuneli SSH: SQL Server Browser përgjigjet me portin e saj përmes UDP, të cilin tuneli nuk e bart. Vendoseni hostin { $server } pa \{ $instance } dhe portin atë ku dëgjon instanca.
db-ssh-host-key-changed = Çelësi SSH i hostit për { $host } ka NDRYSHUAR që kur Schemaic i besoi për herë të parë.
db-ssh-host-key-expected = Pritej  { $key }
db-ssh-host-key-offered = Ofrohet { $key }
db-ssh-host-key-none = (asnjë çelës i regjistruar)
db-ssh-host-key-changed-advice = Kjo mund të nënkuptojë se serveri u rindërtua ose çelësi i tij u ndërrua — ose se diçka po e përgjon lidhjen. Nuk iu dërgua asgjë. Verifikoni gjurmën e re të gishtit me një kanal tjetër (në konsolën e vetë serverit, ose me atë që e administron) para se t’i besoni. Pasi ta keni bërë, hiqni hyrjen e këtij hosti nga { $store } në dosjen e konfigurimit të Schemaic për të regjistruar çelësin e ri.
db-ssh-host-key-store = Schemaic nuk mundi të lexonte regjistrin e tij të çelësave të besuar SSH të hosteve ({ $store }), kështu nuk mund të dallojë nëse çelësi i { $host } ({ $key }) është ai që i keni besuar më parë.
db-ssh-host-key-store-advice = Lidhja u refuzua në vend që çelësit t’i besohej në vend. Riparoni ose fshini { $store } në dosjen e konfigurimit të Schemaic — fshirja e tij do të thotë se çdo hosti SSH do t’i besohet nga e para në lidhjen e tij të ardhshme, prandaj bëjeni këtë vetëm nëse mund t’i verifikoni gjurmët e gishtave.
db-tls-setup = Konfigurimi i TLS dështoi: { $error }
db-tls-client-cert = certifikata e klientit u refuzua: { $error }
db-tls-trust-store = depoja e besimit TLS është e papërdorshme: { $error }
db-tls-bad-name = { $name } nuk është emër që mund ta mbajë një certifikatë: { $error }
db-tls-ca-empty = skedari CA { $path } nuk përmban certifikata
db-tls-ca-unusable = skedari CA { $path } përmban një certifikatë që nuk mund të përdoret si pikë besimi
db-tls-cert-unreadable = skedari i certifikatës { $path } nuk mund të lexohet: { $error }
db-tls-cert-not-pem = { $path } nuk është skedar certifikate PEM: { $error }
db-tls-cert-is-der = { $path } nuk përmban certifikatë PEM — një skedar DER duhet konvertuar më parë (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = skedari i çelësit { $path } nuk mund të lexohet: { $error }
db-tls-key-encrypted = { $path } është një çelës privat i mbrojtur me frazëkalim, të cilin Schemaic nuk mund ta hapë. Deshifrojeni më parë: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } nuk është çelës privat PEM: { $error }
db-sign-in-no-value = { $field ->
        [host] Lidhja nuk ka host me të cilin të hyjë.
       *[user] Lidhja nuk ka përdorues me të cilin të hyjë.
    }
db-sign-in-leading-dash = { $field ->
        [host] Hosti i lidhjes fillon me '-', të cilin CLI e cloud-it do ta lexonte si opsion e jo si host.
       *[user] Përdoruesi i lidhjes fillon me '-', të cilin CLI e cloud-it do ta lexonte si opsion e jo si përdorues.
    }
db-sign-in-control = { $field ->
        [host] Hosti i lidhjes përmban një karakter kontrolli.
       *[user] Përdoruesi i lidhjes përmban një karakter kontrolli.
    }
db-sign-in-shim-aws = AWS CLI e gjetur është një shim .cmd (versioni 1 i instaluar me pip), dhe Schemaic nuk ekzekuton shim përmes cmd.exe. Instaloni versionin 2 të AWS CLI, ose hyni me fjalëkalim.
db-sign-in-shim-gcloud = gcloud i gjetur është një shim .cmd pa Python të përfshirë pranë tij, dhe Schemaic nuk ekzekuton shim përmes cmd.exe. Riinstaloni Google Cloud CLI me instaluesin e saj, ose hyni me fjalëkalim.
db-sign-in-shim-azure = Azure CLI e gjetur është një shim .cmd/.bat pa Python pranë saj, dhe Schemaic nuk ekzekuton shim përmes cmd.exe. Riinstaloni Azure CLI me instaluesin e Microsoft, ose hyni me një login SQL.
db-sign-in-no-token = CLI e cloud-it u përgjigj pa token.
db-sign-in-more-than-token = CLI e cloud-it u përgjigj me më shumë se një token.
db-sign-in-azure-bad-answer = Përgjigjja e Azure CLI nuk ishte token-i që duhej të printonte.
db-sign-in-azure-no-access-token = Azure CLI u përgjigj pa token qasjeje.
db-sign-in-signed-out-aws = AWS CLI nuk ka kredenciale. Ekzekutoni `aws configure` ose `aws sso login`, pastaj lidhuni sërish.
db-sign-in-signed-out-gcloud = Google Cloud CLI nuk është e identifikuar. Ekzekutoni `gcloud auth login`, pastaj lidhuni sërish.
db-sign-in-signed-out-azure = Azure CLI nuk është e identifikuar. Ekzekutoni `az login`, pastaj lidhuni sërish.
db-sign-in-no-token-from = { $cli } nuk mundi të merrte token: { $line }
db-sign-in-no-token-from-silent = { $cli } nuk mundi të merrte token.
db-sign-in-no-token-from-azure-silent = Azure CLI nuk mundi të merrte token: nuk dha asnjë arsye
db-sign-in-not-found = { $cli } nuk u gjet. Instalojeni dhe identifikohuni me të, ose hyni me fjalëkalim.
db-sign-in-not-found-azure = Azure CLI (az) nuk u gjet. Instalojeni dhe ekzekutoni `az login`, ose hyni me një login SQL.
db-sign-in-no-answer = { $cli } nuk u përgjigj brenda { $secs } sekondave.
db-sign-in-not-started = { $cli } nuk mund të nisej: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = eksporti ndaloi së lexuari
db-query-no-rows-to-export = ky urdhër nuk kthen rreshta për t’u eksportuar
db-query-no-server-settings = Ky motor nuk ka cilësime serveri për të shfaqur.
db-query-no-status-list = Ky motor nuk publikon listë statusi — vetëm variablat e tij.
db-query-no-import-columns = Nuk ka kolona ku të importohet
db-query-read-stopped = Leximi ndaloi: { $error }
db-query-batch-continued = SQL Server vazhdon ekzekutimin e një grupi urdhrash pas gabimit të një urdhri, përveç nëse gabimi e përfundon grupin, kështu urdhrat pas atij që dështoi mund të jenë ekzekutuar.
db-query-sqlite-locked = Në SQLite një lexim i gjatë bllokon një shkrim: një eksport ose import i një tabele të tërë mban një bllokim leximi mbi skedarin derisa të përfundojë, dhe asnjë shkrim nuk mund të nisë para kësaj (MySQL dhe PostgreSQL nuk punojnë kështu). Prisni që të përfundojë ose anulojeni, pastaj provoni sërish. Që leximet dhe shkrimet të mbivendosen përgjithmonë në këtë skedar, kalojeni në mënyrën e ditarit WAL — `PRAGMA journal_mode = WAL` — gjë që ndryshon vetë skedarin, prandaj Schemaic nuk do ta bëjë për ju.
db-query-rollback-failed-too = { $error } — dhe edhe rikthimi dështoi: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = urdhri { $n } dështoi: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } urdhër i mëparshëm është zbatuar tashmë dhe nuk mund të rikthehet
   *[other] { $failed } — { $n } urdhra të mëparshëm janë zbatuar tashmë dhe nuk mund të rikthehen
}
ddl-cancelled = plani u anulua
ddl-dangling-foreign-key = plani lë një çelës të jashtëm që nuk tregon asgjë: { $row }
ddl-breaks-view = plani do ta prishte pamjen { $view }, e cila lexon një kolonë që ai e riemërton ose e fshin ({ $why }). Nuk u ndryshua asgjë. Redaktoni ose fshini më parë pamjen.
ddl-no-databases = SQLite nuk ka baza të dhënash për t’u krijuar ose fshirë — atje një bazë të dhënash është një skedar, të cilin Schemaic nuk e krijon dhe nuk e fshin për ju.
ddl-elsewhere-failed = Pamjet në baza të tjera të dhënash që bëjnë select * nga { $table } nuk u rifreskuan ({ $error }), dhe lexojnë e shkruajnë kolonat e gabuara derisa të rifreskohen (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } pamje në një bazë tjetër të dhënash që bën select * nga { $table } nuk mund të rifreskohej, dhe lexon e shkruan kolonat e gabuara derisa të rifreskohet (sp_refreshview): { $views }.
       *[other] { $count } pamje në një bazë tjetër të dhënash që bëjnë select * nga { $table } nuk mund të rifreskoheshin, dhe lexojnë e shkruajnë kolonat e gabuara derisa të rifreskohen (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } bazë të dhënash nuk u kërkua për një pamje që bën select * nga { $table }: { $databases }.
       *[other] { $count } baza të dhënash nuk u kërkuan për një pamje që bën select * nga { $table }: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , dhe { $more } të tjera
    }
ddl-rollback-failed = { $error } — dhe edhe rikthimi dështoi: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = fshirja në { $table } preku { $affected } rreshta (pritej saktësisht 1)
write-one-row-update = përditësimi në { $table } preku { $affected } rreshta (pritej saktësisht 1)
write-one-row-insert = futja në { $table } preku { $affected } rreshta (pritej saktësisht 1)
write-batch-inserted = një grup prej { $rows } rreshtash futi { $wrote }
write-batch-affected = një grup prej { $rows } rreshtash preku { $wrote }
write-row-issue = rreshti { $line }, kolona { $column }: { $problem } ({ $text })
write-row-issue-empty = rreshti { $line }, kolona { $column }: { $problem } (bosh)
write-copy-too-long = { $column } përmban më shumë tekst nga ç’mund të bartë një lexim, kështu kopjimi ndaloi para se ta shkruante
write-copy-too-long-unnamed = një kolonë përmban më shumë tekst nga ç’mund të bartë një lexim, kështu kopjimi ndaloi para se ta shkruante
write-copy-bytes = { $column } përmban bajte që kopjimi nuk mund t’i lexojë ashtu siç janë, kështu ndaloi para se t’i shkruante
write-copy-not-its-form = vlera e lexuar për { $column } nuk është në formën që kërkoi kopjimi, kështu nuk u shkrua asgjë
write-copy-not-its-form-unnamed = vlera e lexuar për një kolonë nuk është në formën që kërkoi kopjimi, kështu nuk u shkrua asgjë
write-commit-cancelled = Konfirmimi u anulua
write-import-cancelled = Importi u anulua
write-rollback-complete = {" "}— të gjitha ndryshimet u rikthyen
write-rollback-incomplete = {" "}— rikthimi NUK i zhbëri: motori i ruajtjes i kësaj tabele nuk është transaksional, kështu rreshtat e shkruar tashmë mbeten. Kontrolloni tabelën para se të provoni sërish.
write-rollback-unknown = {" "}— dhe rikthimi nuk mund të konfirmohej: lidhja dështoi para se të përgjigjej serveri. Rreshtat mund të kenë mbetur ose jo. Kontrolloni tabelën para se të provoni sërish.

## Schema tree › the panel

tree-panel-title = SKEMA
tree-tip-databases = Shfaq ose fshih bazat e të dhënave
tree-tip-options = Opsionet e skemës
tree-search-placeholder = Kërko…
tree-loading = Po ngarkohet
tree-no-tables = Nuk ka tabela
tree-failed-to-connect = Lidhja dështoi
tree-failed-view = Shiko
tree-single-user-unread = Me një përdorues — nuk u lexua
tree-single-user-read = Lexo
tree-count-tables = { $n ->
    [one] { $n } tabelë
   *[other] { $n } tabela
}
tree-count-cols = { $n ->
    [one] { $n } kol.
   *[other] { $n } kol.
}
tree-count-keys = { $n ->
    [one] { $n } çelës
   *[other] { $n } çelësa
}
tree-count-partitions = { $n ->
    [one] { $n } ndarje
   *[other] { $n } ndarje
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = tip
kind-domain = domen
kind-sequence = sekuencë
kind-function = funksion
kind-procedure = procedurë
kind-event = ngjarje
kind-synonym = sinonim
kind-alias-type = tip alias
kind-xml-schema-collection = koleksion skemash XML
kind-extension = zgjerim
kind-group-type = Tipet
kind-group-domain = Domenet
kind-group-sequence = Sekuencat
kind-group-function = Funksionet
kind-group-procedure = Procedurat
kind-group-event = Ngjarjet
kind-group-synonym = Sinonimet
kind-group-alias-type = Tipet alias
kind-group-xml-schema-collection = Koleksionet e skemave XML
kind-group-extension = Zgjerimet

## Query history panel

history-title = HISTORIKU I KËRKESAVE
history-clear-tip = Pastro historikun e kësaj lidhjeje…
history-clear-title = Pastro historikun e kërkesave
history-clear-message = Të fshihen { $n } { $n ->
    [one] kërkesë e regjistruar
   *[other] kërkesa të regjistruara
} për këtë lidhje? Ky veprim nuk mund të zhbëhet.
history-scope-current = Aktuale
history-scope-all = Të gjitha
history-cut-note = Po shfaqen { $shown } më të rejat nga { $matched } — kërkoni për ta ngushtuar.
history-empty = Ende nuk ka kërkesa.
history-no-match = Asnjë kërkesë që përputhet.
history-open-in-new-tab = Hap në skedë të re
history-delete = Fshi
history-failed = Dështoi

## Snippet library panel

snippets-title = BIBLIOTEKA E FRAGMENTEVE
snippets-save-tip = Ruaj SQL-në e redaktorit si fragment
snippets-no-match = Asnjë fragment nuk përputhet.
snippets-band-this-connection = KJO LIDHJE
snippets-band-all-connections = TË GJITHA LIDHJET
snippets-builtin = I integruar
snippets-name-placeholder = Emri i fragmentit
snippets-insert = Fut në redaktor
snippets-open-in-new-tab = Hap në skedë të re
snippets-edit = Redakto
snippets-duplicate = Dyfisho
snippets-delete = Fshi
snippets-show-in = Shfaq te
snippets-scope-this-connection = Kjo lidhje
snippets-scope-engine = Çdo lidhje { $engine }
snippets-scope-all-connections = Të gjitha lidhjet

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = SOT
history-band-this-week = KËTË JAVË
history-band-earlier = MË PARË

## Query tab strip

tabs-tip-unsaved = { $path } — ndryshime të paruajtura
tabs-pin = Fikso
tabs-unpin = Çfikso
tabs-rename = Riemërto
tabs-duplicate = Dyfisho
tabs-close = Mbyll
tabs-open-file = Hap skedar
tabs-save = Ruaj
tabs-save-as = Ruaj si
tabs-reload = Ringarko nga disku
tabs-reopen-last = Rihap skedën e fundit
tabs-close-others = Mbyll skedat e tjera
tabs-close-all = Mbyll të gjitha skedat

## Server Activity panel

activity-title = AKTIVITETI I SERVERIT
activity-refresh-tip = Rifresko tani
activity-auto-refresh-off = Rifreskimi automatik joaktiv
activity-auto-refresh-every = Rifreskim automatik çdo { $secs } s
activity-interval-off = Joaktiv
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } h { $m } min
activity-age-days = { $d } d { $h } h
activity-view-sessions = Seancat
activity-view-locks = Bllokimet
activity-search-sessions = Kërko seanca…
activity-search-locks = Kërko bllokime…
activity-loading = Po ngarkohet…
activity-sqlite-no-sessions = SQLite nuk ka seanca serveri — ekzekutohet brenda këtij procesi, kështu nuk ka asgjë tjetër të lidhur për t’u parë.
activity-sessions-no-match = Asnjë seancë që përputhet.
activity-sessions-none = Nuk ka seanca të tjera në këtë server.
activity-sessions-undrawn = { $n ->
    [one] Edhe { $n } seancë përputhet — ngushtoni kërkimin për ta parë.
   *[other] Edhe { $n } seanca përputhen — ngushtoni kërkimin për t’i parë.
}
activity-locks-no-match = Asnjë bllokim që përputhet.
activity-locks-no-waits = Nuk ka pritje bllokimesh në këtë server.
activity-locks-none = Asnjë seancë tjetër nuk mban bllokim.
activity-locks-contended-only = Ky server rendit vetëm bllokimet që priten, ose që mbajnë peng një pritje — jo çdo bllokim që ka një seancë.
activity-locks-undrawn = { $n ->
    [one] Edhe { $n } bllokim përputhet — ngushtoni kërkimin për ta parë.
   *[other] Edhe { $n } bllokime përputhen — ngushtoni kërkimin për t’i parë.
}
activity-list-capped = lista u kufizua
activity-locks-total = { $n ->
    [one] { $shown } bllokim
   *[other] { $shown } bllokime
}
activity-locks-waiting = { $n ->
    [one] { $shown } në pritje
   *[other] { $shown } në pritje
}
activity-state-blocked = E bllokuar
activity-state-idle-in-tx = Joaktive në transaksion
activity-state-running = Në ekzekutim
activity-state-idle = Joaktive
activity-count-sessions = { $n ->
    [one] { $figure } seancë
   *[other] { $figure } seanca
}
activity-count-running = { $figure } në ekzekutim
activity-count-blocked = { $figure } të bllokuara
activity-count-idle-in-tx = { $figure } joaktive në transaksion
activity-lock-wait = Pritje bllokimi
activity-lock-wait-sentence = Seanca { $waiter } ({ $waiter_who }) { $waited ->
        [none] pret
       *[other] pret prej { $waited }
    } një bllokim të mbajtur nga seanca { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] joaktive në transaksion
        [blocked] vetë e bllokuar
        [running] në ekzekutim
       *[idle] joaktive
    }{ $held ->
        [none] {""}
       *[other] {" "}prej { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}Edhe { $others } seancë tjetër mban një bllokim që i duhet.
       *[other] {" "}Edhe { $others } seanca të tjera mbajnë një bllokim që i duhet.
    }
activity-kill-holder = Përfundo { $id }
activity-terminate-tip = Përfundo seancën { $id }
activity-read-only-tip = Kjo lidhje është shënuar vetëm për lexim.
activity-lock-holds = mban
activity-lock-waits-for = pret
activity-lock-held-by = mbahet nga { $ids }
activity-copy-statement = Kopjo urdhrin
activity-cancel-query = Anulo kërkesën
activity-kill-session = Përfundo seancën
activity-waiting-on = në pritje të një bllokimi të mbajtur nga { $ids }
activity-blocks = { $n ->
        [one] bllokon { $n } seancë
       *[other] bllokon { $n } seanca
    }
activity-read-only-cancel = Kjo lidhje është shënuar vetëm për lexim, prandaj Schemaic nuk anulon kërkesa në të. Hiqni më parë “vetëm për lexim” te cilësimet e lidhjes.
activity-read-only-terminate = Kjo lidhje është shënuar vetëm për lexim, prandaj Schemaic nuk përfundon seanca në të. Hiqni më parë “vetëm për lexim” te cilësimet e lidhjes.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ASISTENTI AI
ai-new-chat-tip = Bisedë e re
ai-settings-tip = Cilësimet e AI
ai-empty-ask = Pyetni për SQL-në tuaj…
ai-empty-not-connected = { $cli } nuk është i lidhur.
ai-input-placeholder = Mesazh…
ai-input-off = Asistenti AI është joaktiv
ai-input-db-down = Nuk jeni lidhur me bazën e të dhënave
ai-speaker-you = JU
ai-result-more-lines = … (+{ $n } rreshta të tjerë)

## Shared dialog buttons (continued)

dialog-close = Mbyll

## Chart modal

chart-title = Grafiku — { $tab }
chart-caption-kind = Grafiku
chart-caption-values = Vlerat
chart-caption-labels = Etiketat
chart-row-number = Numri i rreshtit
chart-kind-bar = Shtylla
chart-kind-line = Vijë
chart-kind-histogram = Histogram
chart-save-tip = Ruaj grafikun…
chart-save-dialog = Ruaj grafikun
chart-file-stem = Grafiku { $name }
chart-nothing-to-save = Nuk ka asgjë për të ruajtur.
chart-no-numbers = Në këtë rezultat nuk ka kolonë me numra për grafik.
chart-all-null = Çdo vlerë në atë kolonë është NULL.
chart-too-large = Këto vlera janë shumë të mëdha për t’u vizatuar.
chart-caption-rows = { $n ->
    [one] { $n } rresht
   *[other] { $n } rreshta
}
chart-caption-skipped-null = { $skipped } NULL, të pavizatuara
chart-caption-skipped = { $skipped } të pavizatuara (NULL, ose përtej { $max })
chart-caption-truncated = rreshtat e lexuar, jo i gjithë rezultati

## Schema editors — shared

editor-preview-sql = Parapamja e SQL
editor-change-count = { $n ->
    [0] Asnjë ndryshim
    [one] 1 ndryshim
   *[other] { $n } ndryshime
}

## View editor

view-editor-title-edit = Redakto pamjen { $view }
view-problem-no-name = Pamja ka nevojë për një emër.
view-problem-no-select = Një pamje ka nevojë për një SELECT që ta përkufizojë.
view-problem-not-a-query = Trupi i një pamjeje duhet të jetë kërkesë — fillon me SELECT, WITH, VALUES ose TABLE.
view-problem-materialized = Schemaic nuk mund të redaktojë një pamje të materializuar — PostgreSQL nuk ka CREATE OR REPLACE për të.
view-problem-hidden = Përkufizimi i kësaj pamjeje është i shifruar ose i fshehur për këtë login, kështu mund të hiqet këtu, por jo të redaktohet.
view-problem-verbatim = Koka e kësaj pamjeje thotë diçka që Schemaic nuk mund ta riformulojë, kështu mund të hiqet këtu, por jo të redaktohet.
view-editor-title-create = Krijo pamje në { $location }
view-editor-section-view = Pamja
view-editor-section-definition = Përkufizimi
view-editor-section-options = Opsionet
view-editor-name = Emri
view-editor-select = Urdhri SELECT
view-editor-check-option = Opsioni CHECK
view-editor-column-names = Emrat e kolonave
view-editor-column-names-placeholder = opsionale — merren nga SELECT kur lihet bosh
view-editor-sql-security = Siguria SQL
view-editor-definer = Përkufizuesi
view-editor-recreate = Rikrijo në vend që të zëvendësohet
view-editor-recreate-hint = PostgreSQL mund vetëm t’i shtojë kolona një pamjeje ekzistuese. Riemërtimi, ndryshimi i tipit ose i renditjes së një kolone kërkon një DROP dhe një CREATE, gjë që merr me vete pamjet e varura dhe lejet.

## Table properties panel

props-title = Vetitë — { $table }
props-reading = Po lexohen statistikat
props-no-stats-view = Një pamje nuk ka hapësirë ruajtjeje të vetën, prandaj serveri nuk raporton statistika për të.
props-no-stats-table = Serveri nuk raportoi statistika për këtë tabelë.
props-on-disk = në disk
props-rows-estimated = rreshta (të vlerësuar)
props-rows-counted = rreshta (të numëruar)
props-rows-not-reported = rreshta — të paraportuar
props-counting = Po numërohen
props-count-rows = Numëro rreshtat
props-count-slow = Një skanim i plotë i tabelës. I ngadaltë për një tabelë të madhe.
props-storage-data = Të dhënat
props-storage-indexes = Indekset
props-storage-free = E lirë
props-section-structure = Struktura
props-columns = Kolonat
props-indexes = Indekset
props-foreign-keys = Çelësat e jashtëm
props-count-columns = { $n ->
    [one] { $n } kolonë
   *[other] { $n } kolona
}
props-count-indexes = { $n ->
    [one] { $n } indeks
   *[other] { $n } indekse
}
props-count-keys = { $n ->
    [one] { $n } çelës
   *[other] { $n } çelësa
}
props-type = Tipi
props-type-view = Pamje
props-type-table = Tabelë
props-section-options = Opsionet
props-engine = Motori
props-row-format = Formati i rreshtit
props-collation = Renditja (collation)
props-next-auto-increment = Vlera pasuese e auto-increment
props-created = Krijuar
props-updated = Përditësuar
props-comment = Komenti
props-section-partitioning = Ndarja
props-partition-of = Ndarje e
props-partition-bound = Kufiri
props-partitioned-by = Ndarë sipas
props-partitions-more = { $n ->
    [one] dhe { $n } ndarje tjetër
   *[other] dhe { $n } ndarje të tjera
}
props-section-indexes = Indekset
props-index-cardinality = kardinaliteti { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } skanim
       *[other] { $shown } skanime
    }
props-index-not-counted = përdorimi nuk numërohet
props-index-unused = nuk është përdorur kurrë që kur u rivendosën numëruesit e serverit
props-never-analysed = Kjo tabelë nuk është analizuar kurrë, prandaj serveri nuk ka vlerësim rreshtash për të. Ekzekutoni ANALYZE, ose numëroni rreshtat.
props-analysed-at = Vlerësuar sipas ANALYZE-s së fundit, { $when }.
props-read-live = Lexuar drejtpërdrejt — ky server ka information_schema_stats_expiry = 0.
props-cached-up-to = Ruajtur në cache nga serveri: këto shifra mund të jenë deri në { $age } të vjetra (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } h
props-age-days = { $n } d
props-vacuum-note = { $pct }% e versioneve të rreshtave të kësaj tabele janë të vdekura dhe ende të parikuperuara. Një VACUUM do ta lironte atë hapësirë.
props-copy = Kopjo

## Shared panel search box

panel-search-placeholder = Kërko…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = tani
ago-minutes = { $n } min më parë
ago-hours = { $n } orë më parë
ago-days = { $n } ditë më parë
ago-weeks = { $n } javë më parë

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } rresht
   *[other] { $n } rreshta
}
outcome-rows-capped = { $n }+ rreshta

## Terminal panel header

term-title = TERMINALI
term-db-cli-tip = Hap CLI-në e bazës së të dhënave
term-db-cli-tip-read-only = Hap CLI-në e bazës së të dhënave — kjo lidhje është shënuar vetëm për lexim
term-restart-tip = Rinis terminalin
term-settings-tip = Cilësimet e terminalit

## Results panel header

results-title = REZULTATET
results-properties-tip = Vetitë e tabelës…
results-monitor-tip = Monitor i drejtpërdrejtë…
results-restore-editor-tip = Rikthe redaktorin
results-collapse-editor-tip = Palos redaktorin

## Query panel header

query-title = KËRKESA
query-no-database = Asnjë bazë të dhënash

## Shared dialog buttons (continued)

dialog-stop = Ndalo

## Copy table modal

copy-table-title = Kopjo { $table }
copy-table-rows-line = Pastaj kopjo çdo rresht të { $table } në të (nuk shfaqet si SQL)
copy-table-into = Kopjo në
copy-table-schema = Skema
copy-table-name = Emri
copy-table-back = ‹ prapa
copy-table-data = Kopjo të dhënat
copy-table-data-hint = Kopjoni rreshtat pasi të krijohet tabela. Kur është çaktivizuar, kopjohet vetëm struktura.
copy-table-what-with-rows = Struktura dhe rreshtat. Çelësat e jashtëm dhe trigger-ët nuk kopjohen, dhe një kolonë e gjeneruar llogaritet nga e para.
copy-table-what-structure = Vetëm struktura. Çelësat e jashtëm dhe trigger-ët nuk kopjohen.
copy-table-choose-database = Zgjidhni bazën e të dhënave ku do të kopjohet.
copy-table-unnamed = Jepini kopjes një emër.
copy-table-onto-itself = Një tabelë nuk mund të kopjohet mbi veten — jepini kopjes një emër tjetër.
copy-table-connection-changed = Lidhja aktive ndryshoi — rihapni “Kopjo te” nga tabela.
copy-table-schema-not-loaded = Skema e { $database } nuk është ngarkuar — zgjerojeni në pemë, pastaj provoni sërish.
copy-table-preview-subject = Kopje e { $table }
copy-table-copying = Po kopjohen rreshtat… { $n }
copy-table-copied = U { $n ->
    [one] kopjua { $n } rresht
   *[other] kopjuan { $n } rreshta
}.
copy-table-stopped = U ndal para se të kopjohej çdo rresht. Tabela u krijua.

## Database, schema and extension editor

database-editor-title-database = Krijo bazë të dhënash
database-problem-no-name = Emri nuk mund të jetë bosh.
database-problem-bad-collation = Emri i një renditjeje (collation) përmban vetëm shkronja, shifra dhe nënvija.
database-editor-title-schema = Krijo skemë
database-editor-title-extension = Krijo zgjerim
database-editor-title-schema-in = Krijo skemë në { $database }
database-editor-title-extension-in = Krijo zgjerim në { $database }
database-editor-section-database = Baza e të dhënave
database-editor-section-schema = Skema
database-editor-section-extension = Zgjerimi
database-editor-name = Emri
database-editor-charset = Grupi i karaktereve
database-editor-collation = Renditja (collation)
database-editor-owner = Pronari
database-editor-server-default = parazgjedhja e serverit
database-editor-owner-you = ju
database-editor-no-extensions = Nuk ka më zgjerime për t’u instaluar
database-editor-no-charsets = Nuk ka grupe karakteresh për t’u sugjeruar
database-editor-no-collations = Nuk ka renditje për t’u sugjeruar
database-editor-no-roles = Nuk ka role në këtë server

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importo në { $database }
script-dialog-title = Ekzekuto një skript SQL
script-section = Skripti SQL
script-choose-file = Zgjidhni një skedar…
script-no-file = Nuk është zgjedhur skedar
script-reading = Po lexohet skedari
script-hint = Skripti mund të krijojë dhe të zgjedhë një bazë të dhënash tjetër nga ajo ku po importoni. Mund të krijojë edhe më shumë se një bazë të dhënash. Kontrolloni skriptin për t’u siguruar që bën atë që prisni.
script-hint-namespace = Skripti mund të krijojë dhe të zgjedhë një bazë të dhënash tjetër nga ajo ku po importoni. Mund të krijojë edhe më shumë se një bazë të dhënash. Hapja e kësaj nga { $namespace } nuk e vendos as skriptin në të: çdo gjë që skedari nuk e kualifikon shkon atje ku thotë search_path i serverit. Kontrolloni skriptin për t’u siguruar që bën atë që prisni.
script-what-it-does = Çfarë bën ky skedar
script-probe-nothing = Ky skedar nuk përmban urdhra që Schemaic mund të ekzekutojë.
script-probe-cut-off = { $size } e parë e këtij skedari është një urdhër i papërfunduar, kështu ende nuk ka asgjë për t’u përmbledhur. Ekzekutimi e dërgon skedarin të plotë.
script-probe-counted = { $count } { $n ->
    [one] urdhër u lexua
   *[other] urdhra u lexuan
} nga { $size } e parë.
script-destroys = { $count } { $n ->
    [one] urdhër
   *[other] urdhra
} në këtë skedar { $n ->
    [one] shkatërron
   *[other] shkatërrojnë
} të dhëna — DROP, TRUNCATE ose DELETE. Kjo nuk mund të zhbëhet nga këtu.
script-unread = U lexua vetëm { $size } e parë e këtij skedari dhe asgjë në të nuk shkatërron të dhëna — por pjesa tjetër nuk u shqyrtua. Mund të bëjë DROP, TRUNCATE ose DELETE, dhe kjo nuk mund të zhbëhet nga këtu.
script-unqualified = { $count } { $n ->
    [one] urdhër
   *[other] urdhra
} në këtë skedar { $n ->
    [one] vepron
   *[other] veprojnë
} mbi çdo rresht të një tabele — një DELETE, UPDATE ose TRUNCATE pa WHERE. Kjo nuk mund të zhbëhet nga këtu.
script-tx-own = Ky skedar hap transaksionin e vet, kështu ose zbatohet i tëri ose aspak. Schemaic nuk shton transaksion të vetin.
script-tx-none = Ky skedar nuk hap transaksion, kështu një dështim në mes lë në vend atë që u ekzekutua para tij.
script-connecting = Po lidhet
script-progress-of = { $done } nga { $total }
script-progress-read = { $done } të lexuara
script-run = Ekzekuto
script-done = U { $n ->
    [one] ekzekutua { $n } urdhër
   *[other] ekzekutuan { $n } urdhra
} nga { $file }.
script-stopped = U ndal pas { $n } { $n ->
    [one] urdhri
   *[other] urdhrash
}.
script-failed = Dështoi: { $message } — { $n } { $n ->
    [one] urdhër u ekzekutua
   *[other] urdhra u ekzekutuan
} para tij.
script-failed-at = Dështoi në rreshtin { $line } të { $file }: { $message } — { $n } { $n ->
    [one] urdhër u ekzekutua
   *[other] urdhra u ekzekutuan
} para tij.
script-durability-rolled-back = { $file } hapi transaksionin e vet, kështu serveri e riktheu gjithë ekzekutimin — nuk u zbatua asgjë, përfshirë urdhrat që patën sukses.
script-durability-applied = { $file } nuk hapi transaksion, kështu çdo urdhër që u ekzekutua mbetet i zbatuar.
script-durability-applied-before = Çdo urdhër që u ekzekutua para se { $file } të hapte transaksionin e vet mbetet i zbatuar; ajo që u ekzekutua brenda transaksionit u rikthye.
script-durability-unknown = Çdo urdhër që është ekzekutuar tashmë mbetet i zbatuar, përveç nëse { $file } hapi transaksionin e vet.

## Schema comparison modal

compare-title = Krahaso skemat
compare-choose-database = Zgjidhni një bazë të dhënash
compare-no-connection = Nuk ka lidhje me këtë motor për t’u krahasuar.
compare-will-change = do të ndryshojë
compare-source-of-truth = burimi i së vërtetës
compare-idle = Zgjidhni një bazë të dhënash për krahasim.
compare-loading = Po lexohen të dyja skemat
compare-select-object = Përzgjidhni një objekt për të parë të dyja anët.
compare-fk-cycle-note = Çelësat e jashtëm të këtyre tabelave formojnë një cikël — asnjë renditje krijimi nuk i plotëson të gjithë, kështu një migrim i gjeneruar mund të refuzohet në mes.
compare-same-but-stored-differently = Këto përputhen strukturalisht — ndryshimi më poshtë është në mënyrën si e ruan motori urdhrin, dhe nuk ka asgjë për t’u migruar.
compare-summary = { $differ } ndryshojnë · { $only_here } vetëm këtu · { $only_there } vetëm atje · { $same } identike
compare-summary-uncertain = { $n } { $n ->
    [one] përputhje
   *[other] përputhje
} për të cilat krahasimi nuk mund të garantojë
compare-filter-placeholder = Filtro sipas emrit
compare-include-identical = Përfshi identiket
compare-select-all = Përzgjidh të gjitha
compare-select-none = Asnjë
compare-hint-held-back = mbajtur jashtë planit; shënimi më sipër shpjegon pse
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = trupi i tij duhet rilexuar nga serveri para se të mund të zbatohet
compare-omitted-unread = ky krahasim nuk ka urdhër për të — përkufizimi i tij nuk mund të lexohej
compare-omitted-no-statement = ky krahasim nuk ka urdhër për ndryshimin që gjeti
compare-held-partition = është { $sub ->
        [yes] nënndarje
       *[no] ndarje
    } e { $parent }, e cila mbahet jashtë — rikrijojini bashkë, me dorë
compare-held-partitioning-differs = ndarja e saj ndryshon, dhe ky krahasim nuk shkruan urdhër që e rindan një tabelë — rikrijojeni me dorë
compare-held-sequence-unwritten = ky krahasim nuk shkruan urdhër për një sekuencë — kopjojeni nga `SHOW CREATE SEQUENCE`, që e shfaq paneli i saj
compare-held-partitioned-unrestated = është e ndarë, dhe asnjë CREATE TABLE këtu nuk e riformulon skemën e saj të ndarjes — krijoni funksionin dhe skemën e ndarjes, pastaj tabelën, me dorë
compare-held-sequence-range = numëruesi i objektivit ka arritur { $at }, jashtë intervalit të ri { $min } deri { $max }, dhe serveri refuzon ta ngushtojë një sekuencë përtej një vlere që e ka dhënë. Riniseni në objektiv (ALTER SEQUENCE … RESTART WITH) në një vlerë që nuk e mban asnjë tabelë, pastaj krahasoni sërish
compare-note-set-aside = { $aside } — { $side ->
        [target] objektivi
       *[source] burimi
    } përmban edhe { $kept }, dhe emrat e objektivit nuk dallojnë shkronjat e mëdha nga të voglat, kështu mund të mbajë vetëm njërin prej të dyve; { $kept } është ai që krahasohet
compare-note-binding = Tipi alias { $alias }: { $bound ->
        [rule] rregulli { $rule }
        [default] vlera e paracaktuar { $default }
       *[both] rregulli { $rule } dhe vlera e paracaktuar { $default }
    } i lidhur me të në burim nuk krahasohet dhe nuk bartet — lidheni me dorë (sp_bindrule / sp_bindefault).
compare-hint-blocked = trupi duhet rilexuar para se kjo të mund të zbatohet
compare-hint-uncertain = një indeks që ky model e lexon vetëm pjesërisht — një përputhje këtu nuk është e sigurt
compare-preview-migration = Parapamja e migrimit
compare-plan-subject = { $n ->
    [one] { $n } objekt në { $database }
   *[other] { $n } objekte në { $database }
}
compare-selected = { $n ->
        [0] Asgjë e përzgjedhur.
        [one] { $n } objekt i përzgjedhur
       *[other] { $n } objekte të përzgjedhura
    }
compare-incomparable = { $left } dhe { $right } nuk mund të krahasohen. Emrat e tipeve, vlerat e paracaktuara dhe format e indekseve nuk barten nga një motor në tjetrin, kështu çdo migrim i gjeneruar nga ndryshimi do të ishte i gabuar.
compare-group-table = Tabelat
compare-group-view = Pamjet
compare-group-trigger = Trigger-ët
compare-kind-enum = enum
compare-kind-domain = domen
compare-kind-xml-schema-collection = koleksion skemash XML
compare-kind-alias-type = tip alias
compare-kind-sequence = sekuencë
compare-kind-table = tabelë
compare-kind-view = pamje
compare-kind-function = funksion
compare-kind-procedure = procedurë
compare-kind-trigger = trigger
compare-kind-event = ngjarje
compare-kind-synonym = sinonim

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Asnjëra bazë të dhënash nuk përmban asgjë për t’u krahasuar.
compare-empty-agrees = Këto dy skema përputhen, objekt për objekt.
compare-empty-agrees-noted = Çdo objekt përputhet, por jo gjithçka mund të krahasohej — shihni shënimet më sipër.
compare-empty-no-match = Asgjë nuk përputhet me atë filtër.
compare-empty-only-identical = Gjithçka që përputhet me atë filtër është identike — aktivizoni “Përfshi identiket” për ta parë.

## Schema editors — shared (continued)

editor-comment-placeholder = çfarë bën

## Event editor

event-editor-title-edit = Redakto ngjarjen { $event }
event-editor-title-create = Krijo ngjarje në { $location }
event-editor-section-event = Ngjarja
event-editor-section-schedule = Orari
event-editor-section-body = Trupi
event-editor-section-options = Opsionet
event-editor-name = Emri
event-editor-body = Trupi
event-editor-runs = Ekzekutohet
event-editor-runs-repeating = Në mënyrë të përsëritur
event-editor-runs-once = Një herë
event-editor-at = Në
event-editor-every = Çdo
event-editor-starts = Fillon
event-editor-ends = Përfundon
event-editor-status = Statusi
event-editor-status-enabled = Aktivizuar
event-editor-status-disabled = Çaktivizuar
event-editor-status-replica = Çaktivizuar në replikë
event-editor-preserve = Ruaje pasi të përfundojë
event-editor-preserve-hint = ON COMPLETION PRESERVE. Joaktiv është parazgjedhja e MySQL dhe do të thotë që serveri e fshin ngjarjen sapo të ketë kaluar ekzekutimi i saj i fundit — gjë që për një ngjarje të njëhershme ndodh menjëherë pas ekzekutimit.
event-editor-definer = Përkufizuesi
event-editor-comment = Komenti
event-editor-reading-source = Po lexohet burimi i ngjarjes…
event-editor-source-stale = Burimi i vërtetë i ngjarjes mbërriti pasi filluat të shkruani, kështu ky Trup është ndërtuar mbi kopjen e katalogut — që nuk është ajo që mban serveri. Mbylleni dhe rihapeni për ta redaktuar.
event-problem-no-name = Ngjarja ka nevojë për një emër.
event-problem-no-time = Një ngjarje e njëhershme ka nevojë për një kohë ekzekutimi.
event-problem-no-interval = Një ngjarje e përsëritur ka nevojë për një interval.
event-problem-no-unit = Një ngjarje e përsëritur ka nevojë për një njësi intervali (DAY, HOUR…).
event-problem-no-body = Ngjarja ka nevojë për një trup.
event-name-clash = Këtu ka tashmë një ngjarje të quajtur { $name }.

## Routine editor

routine-editor-title-edit-function = Redakto funksionin { $routine }
routine-editor-title-edit-procedure = Redakto procedurën { $routine }
routine-editor-title-create-function = Krijo funksion në { $location }
routine-editor-title-create-procedure = Krijo procedurë në { $location }
routine-editor-section-function = Funksioni
routine-editor-section-procedure = Procedura
routine-editor-section-body = Trupi
routine-editor-section-options = Opsionet
routine-editor-name = Emri
routine-editor-parameters = Parametrat
routine-editor-returns = Kthen
routine-editor-returns-placeholder-mssql = int, ose TABLE
routine-editor-language = Gjuha
routine-editor-body = Trupi
routine-editor-recompile = Rikompilo
routine-editor-recompile-hint = WITH RECOMPILE — një plan i ri në çdo thirrje, kurrë një i ruajtur në cache.
routine-editor-schema-binding = Lidhja me skemën
routine-editor-schema-binding-hint = WITH SCHEMABINDING — tabelat dhe pamjet që lexon nuk mund të ndryshohen pa dijeninë e tij.
routine-editor-execute-as = Ekzekuto si
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER ose një përdorues
routine-editor-null-input = Kthen NULL për hyrje NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — trupi nuk ekzekutohet kur një argument është NULL.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatik
routine-editor-comment = Komenti
routine-editor-volatility = Paqëndrueshmëria
routine-editor-strict = Strikt
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Ekzekutohet me të drejtat e pronarit në vend të atyre të thirrësit. Fiksoni një search_path më poshtë kur e përdorni këtë.
routine-editor-security-definer-hint-mysql = Ekzekutohet me të drejtat e përkufizuesit në vend të atyre të thirrësit. Kjo është parazgjedhja e MySQL — çaktivizimi i saj është SQL SECURITY INVOKER.
routine-editor-settings = Cilësimet
routine-editor-add-setting = Shto cilësim
routine-editor-deterministic = Determinist
routine-editor-deterministic-hint = Premton të njëjtin rezultat për të njëjtat argumente. Një server me regjistrim binar refuzon një rutinë jodeterministe, përveç nëse u beson krijuesve.
routine-editor-data-access = Qasja në të dhëna
routine-editor-definer = Përkufizuesi
routine-editor-reading-source = Po lexohet burimi i rutinës…
routine-editor-source-stale = Burimi i vërtetë i rutinës mbërriti pasi filluat të shkruani, kështu ky Trup është ndërtuar mbi kopjen e katalogut — që nuk është ajo që mban serveri. Mbylleni dhe rihapeni për ta redaktuar.
routine-problem-function-no-name = Funksioni ka nevojë për një emër.
routine-problem-procedure-no-name = Procedura ka nevojë për një emër.
routine-problem-function-no-language = Funksioni ka nevojë për një gjuhë (plpgsql, sql).
routine-problem-procedure-no-language = Procedura ka nevojë për një gjuhë (plpgsql, sql).
routine-problem-no-return-type = Funksioni ka nevojë për një tip kthimi.
routine-problem-procedure-returns = Një procedurë nuk kthen asgjë — hiqni tipin e kthimit.
routine-problem-function-no-body = Funksioni ka nevojë për një trup.
routine-problem-procedure-no-body = Procedura ka nevojë për një trup.
routine-problem-bare-parameter = SQL Server e emërton çdo parametër me @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Një procedurë nuk mund të marrë WITH { $option }.
routine-problem-scalar-refuses = Një funksion skalar nuk mund të marrë WITH { $option }.
routine-problem-inline-refuses = Një funksion inline me vlerë tabele nuk mund të marrë WITH { $option }.
routine-problem-multi-refuses = Një funksion me shumë urdhra me vlerë tabele nuk mund të marrë WITH { $option }.
routine-problem-procedure-refuses-native = Një procedurë nuk mund të marrë WITH { $option } pasi kompilohet në mënyrë native.
routine-problem-scalar-refuses-native = Një funksion skalar nuk mund të marrë WITH { $option } pasi kompilohet në mënyrë native.
routine-problem-inline-refuses-native = Një funksion inline me vlerë tabele nuk mund të marrë WITH { $option } pasi kompilohet në mënyrë native.
routine-problem-multi-refuses-native = Një funksion me shumë urdhra me vlerë tabele nuk mund të marrë WITH { $option } pasi kompilohet në mënyrë native.
routine-problem-native-needs-schemabinding = Një rutinë e kompiluar në mënyrë native ka nevojë për WITH SCHEMABINDING — SQL Server i kërkon të dyja bashkë.
routine-problem-inline-with-execute-as = INLINE = ON nuk mund të kombinohet me EXECUTE AS — një funksion që ekzekutohet si dikush tjetër nuk bëhet kurrë inline.
routine-problem-numbered-rename = { $was } kryeson një grup procedurash të numëruara ({ $group }), dhe riemërtimi i tij do t’i fshinte të gjitha. Mbani emrin për ta redaktuar.
routine-problem-trigger-function-args = Një funksion trigger-i nuk merr argumente të deklaruara — ato mbërrijnë në TG_ARGV.
routine-clash-function-refused = Këtu ka tashmë një funksion të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu serveri e refuzon këtë dhe nuk ndryshon asgjë — zgjidhni një emër tjetër, ose fshini më parë { $name }.
routine-clash-procedure-refused = Këtu ka tashmë një procedurë të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu serveri e refuzon këtë dhe nuk ndryshon asgjë — zgjidhni një emër tjetër, ose fshini më parë { $name }.
routine-clash-function-destroys = Këtu ka tashmë një funksion të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu zbatimi i kësaj do ta shkatërronte { $original } dhe do ta linte { $name } siç ishte.
routine-clash-procedure-destroys = Këtu ka tashmë një procedurë të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu zbatimi i kësaj do ta shkatërronte { $original } dhe do ta linte { $name } siç ishte.
routine-clash-function-destroys-new = Këtu ka tashmë një funksion të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu zbatimi i kësaj do ta shkatërronte origjinalin dhe do ta linte { $name } siç ishte.
routine-clash-procedure-destroys-new = Këtu ka tashmë një procedurë të quajtur { $name }. Në këtë motor një riemërtim është një fshirje dhe një krijim, kështu zbatimi i kësaj do ta shkatërronte origjinalin dhe do ta linte { $name } siç ishte.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Kërko llogari
users-loading-accounts = Po ngarkohen llogaritë
users-unsupported = Motori i kësaj lidhjeje nuk ka llogari përdoruesish.
users-none = Ky server nuk raporton llogari.
users-no-match = Asnjë llogari nuk përputhet me “{ $needle }”.
users-pick-account = Zgjidhni një llogari për të parë çfarë mund të bëjë.
users-section-attributes = Atributet
users-section-privileges = Privilegjet
users-attr-authentication = Vërtetimi
users-attr-locked = E kyçur
users-attr-password-expired = Fjalëkalimi ka skaduar
users-attr-superuser = Superpërdorues
users-attr-create-role = Krijon role
users-attr-create-database = Krijon baza të dhënash
users-attr-replication = Replikim
users-attr-bypass-rls = Anashkalon RLS
users-attr-inherit = Trashëgon
users-attr-connection-limit = Kufiri i lidhjeve
users-attr-valid-until = E vlefshme deri
users-attr-type = Tipi
users-attr-disabled = Çaktivizuar
users-attr-default-database = Baza e paracaktuar e të dhënave
users-attr-server-roles = Rolet e serverit
users-attr-login = Login
users-attr-default-schema = Skema e paracaktuar
users-attr-member-of = Anëtar i
users-attr-yes = Po
users-attr-no = Jo
users-login-type-sql = Login SQL
users-login-type-windows = Login Windows
users-login-type-windows-group = Grup Windows
users-kind-user = Përdorues
users-kind-role = Rol
users-kind-login = Login
users-maintained-by-server = mirëmbahet nga serveri
users-loading-privileges = Po ngarkohen privilegjet
users-no-privileges = Kjo llogari nuk ka privilegje.
users-note-my-own-account = Kjo lidhje nuk mund të lexojë mysql.user, kështu kjo nuk është lista e llogarive të serverit — është e vetmja llogari që kjo lidhje mund të shohë, e veta. Lidhuni me një llogari që ka SELECT mbi bazën e të dhënave mysql për të shfletuar të tjerat.
users-note-ms-logins-elsewhere = Azure SQL Database i mban login-et e saj në master, dhe ato nuk menaxhohen këtu: renditen përdoruesit dhe rolet e bazës së të dhënave.
users-note-ms-logins-only = Renditen vetëm login-et e serverit: zgjidhni një bazë të dhënash për të parë përdoruesit dhe rolet e saj.
users-note-ms-short = Këtij login-i i mungon VIEW ANY DEFINITION, kështu serveri i tregon vetëm principalët që mund të shohë — lista mund të jetë e paplotë.
users-note-my-scope = Privilegjet e mbajtura përmes një roli të dhënë nuk zbërthehen: SHOW GRANTS rendit atë që i është dhënë drejtpërdrejt llogarisë dhe rolet ku ajo është anëtare, por jo atë që mbajnë vetë ato role. Hapni privilegjet e vetë rolit për t’i parë.
users-note-pg-scope = Privilegjet e skemave, tabelave dhe sekuencave janë ato në { $database } — PostgreSQL i mban në katalogun e secilës bazë të dhënash, kështu privilegjet në baza të tjera të dhënash nuk renditen. As EXECUTE mbi funksionet dhe procedurat, as privilegjet e dhëna mbi një kolonë të vetme nuk renditen. Privilegjet e mbajtura përmes anëtarësisë në role ose të dhëna për PUBLIC nuk zbërthehen, e as privilegjet që një rol ka duke qenë pronar i një objekti ose superpërdorues.
users-note-pg-no-database = Privilegjet e skemave, tabelave dhe sekuencave nuk renditen: PostgreSQL i mban në katalogun e secilës bazë të dhënash, dhe kjo lidhje nuk ka bazë të dhënash të zgjedhur. Hapni një skedë kërkese në një bazë të dhënash për t’i parë. As EXECUTE mbi funksionet dhe procedurat, as privilegjet e dhëna mbi një kolonë të vetme nuk renditen. Privilegjet e mbajtura përmes anëtarësisë në role ose të dhëna për PUBLIC nuk zbërthehen, e as privilegjet që një rol ka duke qenë pronar i një objekti ose superpërdorues.
users-note-pg-superuser = Ky rol është SUPERUSER: anashkalon çdo kontroll lejesh, kështu mban çdo privilegj mbi çdo objekt, pavarësisht nëse diçka më poshtë e thotë këtë apo jo.
users-note-pg-predefined = { $role } është një nga rolet e paracaktuara të PostgreSQL: ajo që mund të bëjë është e ndërtuar në server dhe jo e dhënë, kështu nuk regjistrohet në asnjë katalog që lexohet këtu.
users-note-pg-owns = { $n ->
    [one] Ky rol është pronar i { $n } objekti në { $database } (skema, tabela, sekuenca ose vetë baza e të dhënave). Një pronar mban çdo privilegj mbi atë që zotëron, dhe PostgreSQL nuk regjistron leje për këtë, kështu asgjë prej saj nuk renditet më poshtë.
   *[other] Ky rol është pronar i { $n } objekteve në { $database } (skema, tabela, sekuenca ose vetë baza e të dhënave). Një pronar mban çdo privilegj mbi atë që zotëron, dhe PostgreSQL nuk regjistron leje për këtë, kështu asgjë prej saj nuk renditet më poshtë.
}
users-note-pg-owns-cluster = { $n ->
    [one] Ky rol është pronar i { $n } objekti në këtë cluster (skema, tabela, sekuenca ose vetë baza e të dhënave). Një pronar mban çdo privilegj mbi atë që zotëron, dhe PostgreSQL nuk regjistron leje për këtë, kështu asgjë prej saj nuk renditet më poshtë.
   *[other] Ky rol është pronar i { $n } objekteve në këtë cluster (skema, tabela, sekuenca ose vetë baza e të dhënave). Një pronar mban çdo privilegj mbi atë që zotëron, dhe PostgreSQL nuk regjistron leje për këtë, kështu asgjë prej saj nuk renditet më poshtë.
}
users-note-ms-unshown = { $n ->
    [one] { $n } leje nuk shfaqet këtu, mbi { $classes }: serveri e raporton mbi një objekt të sigurueshëm që kjo listë nuk mund ta emërtojë.
   *[other] { $n } leje nuk shfaqen këtu, mbi { $classes }: serveri i raporton mbi objekte të sigurueshme që kjo listë nuk mund t’i emërtojë.
}
users-note-ms-pick-database = Lejet e një përdoruesi të bazës së të dhënave janë ato të asaj baze — zgjidhni një për t’i parë.
users-statements-capped = Po shfaqen { $shown } urdhrat e parë nga { $total }. “Kopjo privilegjet” i merr të gjithë.
users-new-account = Llogari e re
users-system-account = Kjo llogari i përket serverit, prandaj Schemaic nuk do ta ndryshojë.
users-reset-password = Rivendos fjalëkalimin
users-gate-no-database = Hapni një skedë kërkese në një bazë të dhënash për të ndryshuar privilegjet — urdhri duhet të ekzekutohet në të.
users-gate-read-only = Kjo lidhje është vetëm për lexim.
users-count = { $n } { $n ->
    [one] llogari
   *[other] llogari
}
users-count-filtered = { $shown } nga { $n } llogari
users-copy-privileges = Kopjo privilegjet

## Schema editors — shared (continued)

editor-row-remove = Hiq

## Trigger editor

trigger-editor-title = Trigger-ët në { $table }
trigger-problem-no-name = Trigger-i ka nevojë për një emër.
trigger-problem-no-table = Një trigger ka nevojë për një tabelë ku të aktivizohet.
trigger-problem-no-event = Një trigger ka nevojë për të paktën një ngjarje që ta aktivizojë.
trigger-problem-pg-no-function = Një trigger PostgreSQL ka nevojë për një funksion për të ekzekutuar.
trigger-problem-pg-body = Një trigger PostgreSQL ekzekuton një funksion, jo një trup — zgjidhni ose shkruani një për ta ekzekutuar.
trigger-problem-truncate-row-level = Një trigger TRUNCATE duhet të jetë FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Një trigger INSTEAD OF duhet të jetë FOR EACH ROW.
trigger-problem-instead-of-view-only = Vetëm një pamje mund të ketë trigger INSTEAD OF — një tabelë merr BEFORE ose AFTER.
trigger-problem-pg-view-row-timing = Trigger-i BEFORE ose AFTER i një pamjeje duhet të jetë FOR EACH STATEMENT — përdorni INSTEAD OF për të vepruar mbi rreshtat.
trigger-problem-update-of-without-update = UPDATE OF emërton kolona në një trigger UPDATE.
trigger-problem-pg-instead-of-columns = Një trigger INSTEAD OF nuk merr listë kolonash — PostgreSQL e aktivizon në çdo UPDATE të pamjes.
trigger-problem-one-event = { $engine } e aktivizon një trigger në një ngjarje të vetme — krijoni një trigger më vete për çdo ngjarje.
trigger-problem-no-truncate = { $engine } nuk ka trigger TRUNCATE.
trigger-problem-sqlite-view-timing = Trigger-i i një pamjeje duhet të jetë INSTEAD OF — SQLite nuk ka trigger BEFORE ose AFTER mbi një pamje.
trigger-problem-sqlite-row-only = SQLite ka vetëm trigger-ë FOR EACH ROW — nuk ka trigger në nivel urdhri.
trigger-problem-no-body = Një trigger { $engine } ka nevojë për një trup.
trigger-problem-sqlite-not-block = Trupi i një trigger-i SQLite duhet të jetë një bllok BEGIN … END që përmban të paktën një urdhër.
trigger-problem-runs-body = Një trigger { $engine } ekzekuton një trup, jo një funksion — shkruani urdhrat që do të ekzekutohen.
trigger-problem-ms-no-before = SQL Server nuk ka trigger BEFORE — përdorni INSTEAD OF për të vepruar para shkrimit.
trigger-problem-ms-view-timing = Në SQL Server trigger-i i një pamjeje duhet të jetë INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server e aktivizon një trigger një herë për urdhër — lexoni rreshtat nga inserted dhe deleted.
trigger-problem-ms-instead-of-rank = Një trigger INSTEAD OF nuk mund të ketë rend aktivizimi — kthejeni çdo ngjarje në “Çdo rend”.
trigger-problem-first-without-event = Aktivizohet i pari në { $event }, për të cilën nuk është trigger.
trigger-problem-last-without-event = Aktivizohet i fundit në { $event }, për të cilën nuk është trigger.
trigger-problem-no-when = { $engine } nuk ka kusht WHEN — vendoseni testin brenda trupit me IF.
trigger-problem-ms-no-update-of = SQL Server nuk ka UPDATE OF — testoni UPDATE(column) brenda trupit.
trigger-problem-my-no-view = MySQL nuk pranon trigger mbi një pamje.
trigger-problem-my-no-instead-of = MySQL nuk ka trigger INSTEAD OF — ato janë të PostgreSQL.
trigger-problem-my-no-update-of = MySQL nuk ka UPDATE OF — një trigger sheh çdo kolonë.
trigger-problem-constraint-trigger = Schemaic nuk mund të redaktojë trigger-in e kufizimit { $name } — nuk i modelon cilësimet e shtyrjes që ai mbart.
trigger-problem-not-editable = Schemaic nuk mund të redaktojë trigger-in { $name } — përkufizimi i tij është i shifruar, i fshehur për këtë login, ose përdor një opsion që Schemaic nuk e modelon. Ndryshojeni në SQL.
trigger-problem-twice = Dy trigger-ë quhen të dy { $name }.
trigger-problem-first-clash = { $n ->
    [2] Vetëm një trigger mund të aktivizohet i pari në { $event } — { $names } dhe { $last } janë caktuar të dy kështu.
   *[other] Vetëm një trigger mund të aktivizohet i pari në { $event } — { $names } dhe { $last } janë caktuar të gjithë kështu.
}
trigger-problem-last-clash = { $n ->
    [2] Vetëm një trigger mund të aktivizohet i fundit në { $event } — { $names } dhe { $last } janë caktuar të dy kështu.
   *[other] Vetëm një trigger mund të aktivizohet i fundit në { $event } — { $names } dhe { $last } janë caktuar të gjithë kështu.
}
trigger-editor-preview-subject = trigger-ët në { $table }
trigger-editor-none-selected = Asnjë trigger i përzgjedhur.
trigger-editor-readonly-constraint = Ky është një trigger kufizimi. Schemaic nuk i modelon cilësimet e shtyrjes që ai mbart, kështu mund të hiqet këtu, por jo të redaktohet — ndryshojeni në SQL.
trigger-editor-readonly-hidden = Përkufizimi i këtij trigger-i është i shifruar ose i fshehur për këtë login, kështu mund të hiqet këtu, por jo të redaktohet.
trigger-editor-readonly-unmodelled = Ky trigger përdor një opsion që Schemaic nuk e modelon, kështu mund të hiqet këtu, por jo të redaktohet — ndryshojeni në SQL.
trigger-editor-section-trigger = Trigger-i
trigger-editor-section-condition = Kushti
trigger-editor-section-action = Veprimi
trigger-editor-name = Emri
trigger-editor-timing = Koha
trigger-editor-event = Ngjarja
trigger-editor-fires = Aktivizohet
trigger-editor-truncate-hint = Vetëm në nivel urdhri.
trigger-editor-not-for-replication-hint = Nuk aktivizohet ndërsa një agjent replikimi shkruan në tabelë.
trigger-editor-rank-any = Çdo rend
trigger-editor-rank-first = Aktivizohet i pari
trigger-editor-rank-last = Aktivizohet i fundit
trigger-editor-when = Kur
trigger-editor-of-columns = Të kolonave
trigger-editor-of-columns-placeholder = opsionale — çdo kolonë kur lihet bosh
trigger-editor-body = Trupi
trigger-editor-function = Funksioni
trigger-editor-edit-function = Redakto
trigger-editor-new-function = Funksion i ri
trigger-editor-arguments = Argumentet
trigger-editor-add-argument = Shto argument

## Schema editors — shared (continued)

editor-row-move-up = Lëvize lart
editor-row-move-down = Lëvize poshtë

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Redakto { $kind } { $name }
object-editor-title-create = Krijo { $kind } në { $location }
object-editor-name = Emri
object-editor-comment = Komenti
object-editor-type = Tipi
object-editor-default = Vlera e paracaktuar
object-editor-section-type = Tipi
object-editor-section-values = Vlerat
object-editor-section-domain = Domeni
object-editor-section-constraints = Kufizimet
object-editor-section-sequence = Sekuenca
object-editor-section-position = Pozicioni
object-editor-section-synonym = Sinonimi
object-editor-section-alias-type = Tipi alias
object-editor-section-xml-schema-collection = Koleksioni i skemave XML
object-editor-section-stands-for = Qëndron për
object-editor-value-placeholder = vlera
object-editor-add-value = Shto vlerë
object-editor-values-order = Sipas rendit të krahasimit — ky është rendi që përdor ORDER BY
object-editor-add-constraint = Shto kufizim
object-editor-no-base-types = Ky motor nuk sugjeron tipe bazë
object-editor-not-null-hint = Çdo kolonë e këtij domeni e refuzon NULL. Aktivizimi dështon nëse ndonjëra mban tashmë një të tillë.
object-editor-stores = Ruan
object-editor-increment = Rritja
object-editor-start = Fillimi
object-editor-cache = Cache
object-editor-minimum = Minimumi
object-editor-maximum = Maksimumi
object-editor-not-whole-number = { $field } duhet të jetë numër i plotë.
object-problem-type-no-name = Tipi ka nevojë për një emër.
object-problem-duplicate-value = Vlera { $value } është renditur më shumë se një herë.
object-problem-empty-value = Një vlerë nuk mund të jetë bosh — PostgreSQL nuk mund ta heqë kurrë.
object-problem-domain-no-name = Domeni ka nevojë për një emër.
object-problem-domain-no-base = Një domen ka nevojë për një tip mbi të cilin të bazohet.
object-problem-constraint-no-name = Çdo kufizim ka nevojë për një emër.
object-problem-constraint-no-predicate = Kufizimi { $name } nuk ka kusht.
object-problem-constraint-twice = Kufizimi { $name } është emërtuar dy herë.
object-problem-sequence-no-name = Sekuenca ka nevojë për një emër.
object-problem-zero-increment = Një sekuencë nuk mund të rritet me 0.
object-problem-min-above-max = Minimumi ({ $min }) është mbi maksimumin ({ $max }).
object-problem-start-outside = Vlera fillestare ({ $start }) është jashtë { $min }..{ $max }.
object-problem-cache-below-one = Cache duhet të jetë të paktën 1.
object-problem-does-not-fit = { $min }..{ $max } nuk përshtatet në { $data_type }.
object-problem-restart-outside = Rinisja në { $restart } është jashtë { $min }..{ $max }.
object-problem-counter-outside = Sekuenca është në { $last }, jashtë intervalit të ri { $min }..{ $max }. Jepini një vlerë rinisjeje brenda intervalit.
object-problem-no-name = Emri nuk mund të jetë bosh.
object-problem-tsql-no-type = Një sekuencë ka nevojë për një tip.
object-problem-start-not-whole = Fillimi duhet të jetë numër i plotë.
object-problem-increment-not-whole = Rritja duhet të jetë numër i plotë.
object-problem-minimum-not-whole = Minimumi duhet të jetë numër i plotë.
object-problem-maximum-not-whole = Maksimumi duhet të jetë numër i plotë.
object-problem-tsql-zero-increment = Rritja nuk mund të jetë 0.
object-problem-min-not-below-max = Minimumi duhet të jetë nën maksimumin.
object-problem-started-outside = Ka filluar në { $start }, jashtë intervalit të ri: riniseni në një vlerë ndërmjet minimumit dhe maksimumit.
object-problem-tsql-start-outside = Fillimi duhet të jetë ndërmjet minimumit dhe maksimumit.
object-problem-cache-not-positive = Madhësia e cache duhet të jetë numër i plotë mbi 0.
object-problem-restart-not-whole = Vlera e rinisjes duhet të jetë numër i plotë.
object-problem-tsql-restart-outside = Vlera e rinisjes duhet të jetë ndërmjet minimumit dhe maksimumit.
object-problem-synonym-no-target = Një sinonim ka nevojë për objektin për të cilin qëndron.
object-problem-alias-no-base = Një tip alias ka nevojë për një tip bazë.
object-editor-cycle = Cikël
object-editor-cycle-hint = Kalo në skajin tjetër të intervalit në vend që të dështojë kur sekuenca shteron.
object-editor-restart-at = Rinis në — numëruesi dha së fundi { $last }
object-editor-restart-at-unused = Rinis në — numëruesi nuk është përdorur
object-editor-restart-not-whole = “Rinis në” duhet të jetë numër i plotë.
object-editor-restart-placeholder = lëreni bosh për të ruajtur pozicionin
object-editor-tsql-restart-placeholder = bosh e ruan, përveç nëse ndryshon tipi
object-editor-owned-by = Në pronësi të
object-editor-detach = Shkëpute nga kolona e saj
object-editor-detach-hint = Sekuenca nuk fshihet më bashkë me kolonën dhe vazhdon të ekzistojë pas saj.
object-editor-cache-hint = Jep vlera nga memoria, më shpejt, me koston e një boshllëku pas rinisjes së serverit.
object-editor-cache-size = Madhësia e cache
object-editor-cache-size-placeholder = parazgjedhja e serverit
object-editor-synonym-server = Serveri
object-editor-synonym-server-placeholder = ky server
object-editor-synonym-database = Baza e të dhënave
object-editor-synonym-database-placeholder = kjo bazë të dhënash
object-editor-synonym-schema = Skema
object-editor-synonym-schema-placeholder = skema e paracaktuar
object-editor-synonym-object = Objekti
object-editor-synonym-object-placeholder = tabelë, pamje ose rutinë
object-editor-base-type = Tipi bazë
object-editor-nullable = Pranon NULL
object-editor-nullable-hint = Një kolonë e këtij tipi pranon NULL, përveç nëse kolona thotë ndryshe.
object-editor-schemas = Skemat

## Account and grant editors (Users and privileges)

account-editor-title-create = Krijo llogari
account-editor-title-reset = Rivendos fjalëkalimin
account-editor-title-privileges = Privilegjet — { $account }
account-editor-account = Llogaria
account-editor-kind = Lloji
account-editor-name = Emri
account-editor-host = Hosti
account-editor-host-placeholder = % (çdo host)
account-editor-for-login = Për login-in
account-editor-for-login-placeholder = login — bosh për një përdorues pa login
account-editor-also-user = Edhe përdorues në { $database }
account-editor-password = Fjalëkalimi
account-editor-confirm = Konfirmo
account-editor-action = Veprimi
account-editor-action-grant = Jep
account-editor-action-revoke = Hiq
account-editor-subject = Objekti
account-editor-subject-privileges = Privilegje
account-editor-subject-role = Rol
account-editor-role = Roli
account-editor-no-roles = Ky server nuk ka role
account-editor-with-admin-option = Me opsionin e administrimit
account-editor-level = Niveli
account-editor-level-global = I gjithë serveri
account-editor-level-database = Baza e të dhënave
account-editor-level-schema = Skema
account-editor-level-table = Tabela
account-editor-level-sequence = Sekuenca
account-editor-no-suggestions = Nuk ka sugjerime
account-editor-section-privileges = Privilegjet
account-editor-with-grant-option = Me opsionin e dhënies
account-editor-needs-role = Emërtoni një rol.
account-editor-needs-privileges = Zgjidhni një nivel, emërtoni atë ku zbatohet dhe shënoni një privilegj.
account-editor-password-required = Kërkohet një fjalëkalim.
account-editor-name-required = Kërkohet një emër.
account-editor-entra-only-user = Nga Microsoft Entra krijohet vetëm një përdorues i bazës së të dhënave.
account-editor-entra-no-password = Një përdorues Entra nuk ka login ose fjalëkalim këtu — hyrjen e bën Entra.
account-editor-login-needs-password = Një login ka nevojë për një fjalëkalim.
account-editor-user-for-login-password = Një përdorues për një login hyn me fjalëkalimin e login-it — lëreni këtë bosh.
account-editor-confirm-password = Shkruajeni fjalëkalimin sërish për ta konfirmuar.
account-editor-passwords-differ = Dy fjalëkalimet ndryshojnë.
account-editor-password-hashed = SQL-ja e parapamë, që është urdhri që ekzekutohet, mbart një hash të fjalëkalimit në formën që ruan ky server, jo vetë fjalëkalimin — përveç nëse ka karaktere që login-i do t’i normalizonte (shkronja me gjerësi të plotë, ligatura, hapësira të pandashme), ose serveri ekzekuton një zgjerim për kontrollin e fjalëkalimeve; këto dërgohen siç janë shkruar.
account-editor-password-as-typed = Fjalëkalimi shfaqet në SQL-në e parapamë, që është urdhri që ekzekutohet.

## Table designer

designer-title-edit = Redakto { $table }
designer-title-create = Krijo tabelë në { $location }
designer-tab-table = Tabela
designer-tab-columns = Kolonat
designer-tab-indexes = Indekset
designer-tab-foreign-keys = Çelësat e jashtëm
designer-tab-checks = Kontrollet
designer-list-add = Shto
designer-no-suggestions = Nuk ka sugjerime
designer-name = Emri
designer-comment = Komenti
designer-table-comment-placeholder = Për çfarë shërben kjo tabelë
designer-engine = Motori
designer-collation = Renditja (collation)
designer-no-column = Asnjë kolonë e përzgjedhur.
designer-type = Tipi
designer-type-hint = Shkruhet ashtu siç është — serveri vendos se çfarë do të thotë.
designer-nullable = Pranon NULL
designer-nullable-hint = Lejo NULL në këtë kolonë.
designer-primary-key = Çelës primar
designer-primary-key-hint = Pjesë e çelësit primar të tabelës, sipas rendit të kolonave.
designer-identity = Identity
designer-auto-increment = Auto-increment
designer-identity-hint = Vlerën e cakton serveri ({ $keyword }).
designer-default = Vlera e paracaktuar
designer-default-hint = Tekst SQL, siç do ta shkruanit: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Gjeneruar nga
designer-generated-hint = Një shprehje, p.sh. qty * price. Një kolonë e gjeneruar nuk merr vlerë të paracaktuar.
designer-on-update = Në përditësim
designer-index-unique-detail = unik
designer-index-redundant-detail = i tepërt
designer-hint-redundant = Indeksi { $index } mbulohet nga { $cover }: çdo kërkim që shërben ai, e shërben edhe ky.
table-problem-no-name = Tabela ka nevojë për një emër.
table-problem-no-columns = Një tabelë ka nevojë për të paktën një kolonë.
table-problem-column-no-name = Çdo kolonë ka nevojë për një emër.
table-problem-column-twice = Dy kolona quhen të dyja { $name }.
table-problem-column-no-type = Kolona { $name } ka nevojë për një tip.
table-problem-key-not-column = Çelësi primar emërton { $column }, që nuk është kolonë.
table-problem-index-no-name = Çdo indeks ka nevojë për një emër.
table-problem-index-twice = Dy indekse quhen të dy { $name }.
table-problem-index-no-columns = Indeksi { $index } nuk ka kolona.
table-problem-xml-one-column = Indeksi { $index } duhet të emërtojë saktësisht një kolonë.
table-problem-xml-no-options = Indeksi { $index } nuk mund të jetë unik, të përfshijë kolona ose të filtrohet: një indeks XML ose hapësinor nuk merr asnjërën prej tyre.
table-problem-xml-no-using = Indeksi { $index } ka nevojë për klauzolën e tij USING XML INDEX … FOR …: një indeks XML dytësor ndërtohet mbi një indeks primar ose selektiv.
table-problem-selective-no-paths = Indeksi { $index } ka nevojë për shtigjet e tij FOR (…): një indeks XML selektiv janë shtigjet që ai promovon.
table-problem-xml-parent-gone = Indeksi { $index } është ndërtuar mbi { $parent }, që nuk është indeks XML primar ose selektiv i kësaj tabele: ai zhduket kur zhduket prindi i tij.
table-problem-not-orderable = Indeksi { $index } nuk mund të renditet: vetëm një indeks columnstore ka ORDER.
table-problem-order-not-column = Indeksi { $index } renditet sipas { $column }, që nuk është kolonë.
table-problem-order-not-stored = Indeksi { $index } renditet sipas { $column }, të cilën nuk e ruan.
table-problem-columnstore-unique = Indeksi { $index } nuk mund të jetë unik: një indeks columnstore ruan kolona, jo çelësa.
table-problem-columnstore-include = Indeksi { $index } nuk mund të përfshijë kolona: një indeks columnstore rendit çdo kolonë që ruan.
table-problem-columnstore-sorted = Indeksi { $index } nuk mund t’i renditë kolonat e tij: një kolonë columnstore nuk ka drejtim.
table-problem-clustered-columnstore-filtered = Indeksi { $index } nuk mund të filtrohet: një indeks columnstore i grupuar ruan çdo rresht.
table-problem-clustered-include = Indeksi { $index } nuk mund të përfshijë kolona: një indeks i grupuar i mban tashmë të gjitha kolonat.
table-problem-unique-include = Indeksi { $index } nuk mund të përfshijë kolona: një kufizim unik nuk ka INCLUDE këtu.
table-problem-clustered-filtered = Indeksi { $index } nuk mund të filtrohet: një indeks i grupuar mbulon çdo rresht.
table-problem-constraint-filtered = Indeksi { $index } nuk mund të filtrohet: ai mbështet një kufizim UNIQUE, i cili këtu nuk merr kusht. Fshijeni dhe shtoni një indeks unik me kushtin.
table-problem-not-partial = Indeksi { $index } nuk mund të filtrohet: ky indeks nuk mund të jetë i pjesshëm këtu.
table-problem-index-not-column = Indeksi { $index } emërton { $column }, që nuk është kolonë.
table-problem-include-not-column = Indeksi { $index } përfshin { $column }, që nuk është kolonë.
table-problem-cluster-clash = Një tabelë mban vetëm një indeks të grupuar, dhe { $clustered } e grupojnë secili - bëjini të gjithë përveç njërit të pagrupuar.
table-problem-cluster-key = çelësi primar
table-problem-cluster-index = indeksi { $index }
table-problem-cluster-and = dhe
table-problem-fk-no-name = Çdo çelës i jashtëm ka nevojë për një emër.
table-problem-fk-twice = Dy çelësa të jashtëm quhen të dy { $name }.
table-problem-fk-no-table = Çelësi i jashtëm { $fk } nuk i referohet asnjë tabele.
table-problem-fk-unpaired = Çelësi i jashtëm { $fk } duhet ta çiftojë çdo kolonë me një që i referohet.
table-problem-fk-not-column = Çelësi i jashtëm { $fk } emërton { $column }, që nuk është kolonë.
table-problem-check-no-name = Çdo kufizim kontrolli ka nevojë për një emër.
table-problem-check-twice = Dy kufizime kontrolli quhen të dy { $name }.
table-problem-check-no-predicate = Kontrolli { $name } nuk ka kusht.
table-problem-without-rowid-no-key = Një tabelë WITHOUT ROWID ka nevojë për një çelës primar.
table-problem-strict-type = Tabelat STRICT pranojnë vetëm INT, INTEGER, REAL, TEXT, BLOB ose ANY — { $column } është { $data_type }.
designer-hint-unindexed-fk = Çelësi i jashtëm { $fk } nuk ka indeks që fillon me kolonat e tij, kështu një fshirje ose përditësim çelësi në tabelën që ai i referohet e skanon këtë.
designer-no-index = Asnjë indeks i përzgjedhur. Çelësi primar caktohet te kolonat.
designer-columns = Kolonat
designer-index-columns-hint-single = Kolona e vetme xml, geometry ose geography që indeksohet.
designer-index-columns-hint-columnstore = Kolonat që ruan, të ndara me presje.
designer-index-columns-hint-prefix = Të ndara me presje, sipas rendit të çelësit. bio(20) është gjatësi prefiksi; shtoni DESC për renditje zbritëse.
designer-index-columns-hint = Të ndara me presje, sipas rendit të çelësit. Shtoni DESC për një kolonë zbritëse.
designer-index-method = Metoda
designer-index-method-hint = Lëreni bosh për parazgjedhjen (btree).
designer-index-filter = Vetëm rreshtat ku
designer-index-filter-hint = Kushti i një indeksi të pjesshëm, pa WHERE.
designer-index-storage = Ruajtja
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Columnstore i grupuar
designer-storage-primary-xml = XML primar
designer-storage-selective-xml = XML selektiv
designer-storage-secondary-xml = XML dytësor
designer-storage-spatial = Hapësinor
designer-index-using = Duke përdorur
designer-index-using-hint = Klauzola pas kolonës: USING XML INDEX … FOR PATH, VALUE ose PROPERTY e një indeksi XML dytësor (ose FOR (path) te një selektiv), [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …) e një indeksi XML selektiv, ose USING … WITH (…) e një indeksi hapësinor.
designer-index-order = Renditja
designer-index-order-hint = Kolonat sipas të cilave renditen segmentet, të ndara me presje — nga SQL Server 2022, dhe nga 2025 për një indeks të pagrupuar, i cili rendit vetëm sipas kolonave që ruan.
designer-index-include = Përfshi
designer-index-include-hint = Kolonat që mbarten në indeks, por nuk janë pjesë e çelësit të tij, të ndara me presje.
designer-index-unique = Unik
designer-index-unique-hint = Refuzo vlerat e dyfishta në këto kolona.
designer-fk-detail-unindexed = → { $table } · pa indeks
designer-no-fk = Asnjë çelës i jashtëm i përzgjedhur.
designer-fk-ref-table = Tabela e referuar
designer-fk-on-delete = Në fshirje
designer-fk-columns-hint = Të ndara me presje, të çiftuara sipas rendit me kolonat e referuara.
designer-fk-ref-columns = Kolonat e referuara
designer-no-check = Asnjë kontroll i përzgjedhur.
designer-check-expression = Shprehja
designer-check-expression-hint = Kushti që duhet të plotësojë çdo rresht. Shkruhet pa CHECK (…).
designer-check-enforced = I zbatuar
designer-check-enforced-hint = Kur është çaktivizuar, kufizimi regjistrohet pa u zbatuar — pranohen si rreshtat ekzistues, ashtu edhe të rinjtë.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Menaxho lidhjet
connections-new = Lidhje e re
connections-import = Importo lidhjet
connections-export = Eksporto lidhjet
connections-duplicate = Dyfisho
connections-delete = Fshi
connections-test = Testo
connections-save = Ruaj
connections-test-failed = Lidhja dështoi
connections-browse = Shfleto…
connections-section-general = Të përgjithshme
connections-section-safeguards = Masat mbrojtëse
connections-section-database = Baza e të dhënave
connections-section-network = Rrjeti
connections-name = Emri
connections-colour = Ngjyra
connections-folder = Dosja
connections-no-folders = Ende nuk ka dosje
connections-folder-hint = Opsionale. Lidhjet me të njëjtën dosje renditen bashkë.
connections-prominent-colour = Ngjyrë e dukshme në redaktor
connections-prominent-colour-hint = Kornizoni redaktorin e kërkesave me ngjyrën e kësaj lidhjeje.
connections-read-only = Vetëm për lexim
connections-read-only-hint = Çaktivizoni redaktimet e qelizave dhe bllokoni kërkesat e shkrimit/DDL në këtë lidhje.
connections-read-only-mssql = SQL Server nuk ka seancë vetëm për lexim, kështu leximet ekzekutohen në një transaksion që Schemaic e rikthen. Efekti i një procedure jashtë bazës së të dhënave i mbijeton kësaj; për garanci, lidhuni me një login që mund vetëm të lexojë.
connections-cli-access = Qasja nga CLI
connections-cli-access-hint = Lejoni linjën e komandave schemaic, dhe agjentët AI përmes schemaic mcp, ta përdorin këtë lidhje.
connections-ai-data-access = Qasja e AI në të dhëna
connections-statement-timeout = Koha maksimale e urdhrit
connections-statement-timeout-hint = Anulon një urdhër në këtë lidhje që zgjat më shumë.
connections-statement-timeout-same = Njësoj si aplikacioni
connections-environment = Mjedisi
env-none = Asnjë
env-local = Lokal
env-development = Zhvillim
env-testing = Testim
env-staging = Staging
env-production = Prodhim
connections-type = Tipi
connections-init-sql = SQL fillestare
connections-init-sql-pragma-hint = Opsionale. Cilësimet PRAGMA ekzekutohen në çdo lidhje që hap Schemaic.
connections-init-sql-set-hint = Opsionale. Urdhrat SET ekzekutohen në çdo lidhje që hap Schemaic — një search path, një zonë kohore, një rol.
connections-init-refused = “{ $statement }”: { $why }
connections-init-only-pragmas = gjatë lidhjes mund të ekzekutohen vetëm cilësime PRAGMA
connections-init-only-sets = gjatë lidhjes mund të ekzekutohen vetëm urdhra SET
connections-init-calls-function = një cilësim nuk mund të thërrasë funksion gjatë lidhjes
connections-init-quoted-name = emri i një cilësimi nuk mund të jetë në thonjëza gjatë lidhjes
connections-init-names-no-setting = kjo nuk emërton asnjë cilësim
connections-init-not-listed = { $name } nuk mund të caktohet gjatë lidhjes — nuk është ndër cilësimet e seancës që Schemaic di se nuk i prekin mbrojtjet e tij
connections-init-not-utf8 = grupi i karaktereve të klientit duhet të mbetet UTF-8 — kontrolli vetëm për lexim i Schemaic e lexon çdo urdhër si UTF-8
connections-init-one-value = një cilësim merr një vlerë gjatë lidhjes, dhe asgjë nuk mund ta pasojë
connections-init-one-value-after-equals = një cilësim merr një vlerë gjatë lidhjes, pas një =
connections-init-transaction = cilësimet e transaksionit nuk mund të bëhen gjatë lidhjes — i cakton mbrojtja vetëm për lexim
connections-init-pragma = ky PRAGMA nuk mund të ekzekutohet gjatë lidhjes — ndryshon skedarin ose mbrojtjen vetëm për lexim
connections-init-server-scope = cilësimet { $scope } ndryshojnë serverin, jo seancën
connections-init-variable = një variabël nuk është cilësim seance — përfundon me grupin e tij të urdhrave
connections-init-on-only = { $name } mund vetëm të aktivizohet gjatë lidhjes — Schemaic mbështetet tek ai
connections-host = Hosti
connections-port = Porti
connections-named-instance-port = Një instancë me emër: portin e jep SQL Server Browser, kështu Porti nuk përdoret. Për të anashkaluar Browser-in, vendosni hostin pa instancën dhe portin ku ajo dëgjon.
connections-database = Baza e të dhënave
connections-database-hint = Opsionale, dhe është baza e të dhënave ku hapet lidhja. Disa ofrues hostimi lejojnë vetëm bazën e tyre dhe refuzojnë një lidhje që nuk e emërton.
connections-authentication = Vërtetimi
ssh-auth-password = Fjalëkalim
ssh-auth-key-pair = Çift çelësash
ssh-auth-agent = Agjent SSH
auth-password = Përdorues dhe fjalëkalim
auth-windows = Windows (kjo llogari)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Përdoruesi
connections-password = Fjalëkalimi
connections-azure-cli-hint = Hyn si përdoruesi që është identifikuar në Azure CLI (ekzekutoni më parë `az login`). Token-i merret kur një lidhje ka nevojë për të dhe nuk ruhet kurrë.
connections-aws-iam-hint = Hyn si përdoruesi më poshtë me një token nga kredencialet e AWS CLI (më parë `aws configure` ose `aws sso login`). Përdoruesi ka nevojë për rolin rds_iam (PostgreSQL) ose për AWSAuthenticationPlugin (MySQL). Token-i merret kur një lidhje ka nevojë për të dhe nuk ruhet kurrë.
connections-gcp-iam-hint = Hyn si përdoruesi më poshtë — përdoruesi IAM ose llogaria e shërbimit që njeh Cloud SQL — me një token nga Google Cloud CLI (më parë `gcloud auth login`). Token-i merret kur një lidhje ka nevojë për të dhe nuk ruhet kurrë.
connections-windows-hint-named = Hyn si llogaria Windows me të cilën ekzekutohet Schemaic ({ $account }). Serveri duhet ta njohë atë llogari si login Windows.
connections-windows-hint = Hyn si llogaria Windows me të cilën ekzekutohet Schemaic. Serveri duhet ta njohë atë llogari si login Windows.
connections-database-file = Skedari i bazës së të dhënave
connections-database-file-dialog = Zgjidhni bazën e të dhënave SQLite
connections-sqlite-hint = Një bazë të dhënash SQLite është një skedar i vetëm, kështu nuk ka server për t’u arritur — as host, përdorues, fjalëkalim apo tunel. Transaksionet manuale ende nuk ofrohen për të: urdhrat konfirmohen sapo dërgohen.
connections-ssh-tunnel = Tunel SSH
connections-ssh-tunnel-hint = Arrini serverin përmes një tuneli SSH.
connections-ssh-host = Hosti SSH
connections-ssh-port = Porti SSH
connections-ssh-user = Përdoruesi SSH
connections-ssh-password = Fjalëkalimi SSH
connections-ssh-agent-hint = Nënshkrimi i delegohet agjentit tuaj SSH që po punon (agjenti OpenSSH / Pageant në Windows). Schemaic nuk ruan asnjë çelës.
connections-jump-hosts = Hostet ndërmjetëse
connections-jump-hosts-hint = Opsionale. Hostet nëpër të cilët kalohet para hostit SSH, më i afërti i pari — secili hyn në mënyrën e caktuar më poshtë.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , çelësi { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , përmes { $jump }
       *[none] {""}
    }
connections-route-jump-host = hosti ndërmjetës { $host }
connections-route-proxy-command = ~/.ssh/config e arrin { $host } përmes një ProxyCommand, të cilin Schemaic nuk mund ta ekzekutojë. Jepeni rrugën si Hostet ndërmjetëse.
connections-route-match = ~/.ssh/config mund ta drejtojë { $host } përmes një blloku Match, të cilin Schemaic nuk e lexon. Jepeni rrugën si Hostet ndërmjetëse.
connections-route-match-destination = ~/.ssh/config mund ta drejtojë { $host } te një host ose port tjetër përmes një blloku Match, të cilin Schemaic nuk e lexon. Jepeni vetë adresën si host SSH.
connections-route-include = ~/.ssh/config përfshin një skedar që Schemaic nuk mundi ta lexonte aty ku zbatohet për { $host }. Jepeni vetë adresën si host SSH, dhe rrugën si Hostet ndërmjetëse.
connections-route-config-jump-password = ~/.ssh/config e arrin { $host } përmes { $jump }, dhe një fjalëkalim u dërgohet vetëm hosteve që emërton kjo lidhje. Shtojeni hostin ndërmjetës te Hostet ndërmjetëse nëse pranon të njëjtin fjalëkalim, ose hyni me një çelës ose me agjentin SSH.
connections-route-jump-own-proxy-jump = ~/.ssh/config e arrin hostin ndërmjetës { $host } përmes { $jump }, dhe Schemaic nuk ndjek ProxyJump-in e vetë një hosti ndërmjetës. Jepeni rrugën si Hostet ndërmjetëse, me { $jump } të parin.
connections-jump-host-error = Hosti ndërmjetës { $number }: { $why }
connections-jump-empty = bosh — hiqni presjen e tepërt
connections-jump-space = “{ $entry }” përmban një hapësirë
connections-jump-unclosed = “{ $entry }” hap një [ që nuk e mbyll
connections-jump-after-bracket = “{ $entry }” ka tekst pas ]
connections-jump-no-host = “{ $entry }” nuk emërton host
connections-jump-not-a-port = “{ $entry }” nuk është port
connections-private-key = Çelësi privat
connections-private-key-dialog = Zgjidhni çelësin privat
connections-passphrase = Frazëkalimi
connections-tls = SSL / TLS
tls-disable = Çaktivizuar
tls-prefer = Preferohet
tls-require = Kërkohet
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Lidhu me tekst të pashifruar.
tls-prefer-description = Shifro nëse serveri e ofron. Pa verifikim.
tls-require-description = Refuzo lidhjen pa shifrim. Pa verifikim.
tls-verify-ca-description = Shifro dhe kontrollo certifikatën kundrejt CA-së.
tls-verify-full-description = Shifro, kontrollo certifikatën dhe kontrollo emrin e hostit.
tls-verify-ca-checks-host = Në MySQL dhe MariaDB kjo kontrollon edhe emrin e hostit, kështu sillet si Verify full — një kufizim i drajverit, jo një cilësim.
tls-floor-azure-cli = Hyrja me Microsoft Entra lidhet në Verify full, pavarësisht çfarë zgjidhet këtu: token-i i saj hap çdo bazë të dhënash Azure SQL që llogaria mund të arrijë.
tls-floor-aws-iam = Hyrja me AWS IAM lidhet të paktën në “Kërkohet”: token-i i saj nuk duhet të kalojë rrjetin i pashifruar. Verify full kërkon paketën CA të RDS si Root CA.
tls-floor-gcp-iam = Hyrja me Google Cloud IAM lidhet të paktën në Verify CA — vendosni si Root CA CA-në e serverit të instancës: token-i i saj hap çdo instancë Cloud SQL që llogaria mund të arrijë.
tls-floor-other = Kjo hyrje lidhet në një mënyrë më të fortë se ajo e zgjedhur këtu.
connections-ca-certificate = Certifikata CA
connections-ca-certificate-dialog = Zgjidhni certifikatën CA
connections-ca-certificate-hint = Lëreni bosh për t’u besuar autoriteteve të certifikimit që u beson sistemi juaj operativ, përfshirë çdo të tillë që ka instaluar organizata juaj. Emërtoni një skedar për t’i besuar atij në vend të tyre.
connections-client-certificate = Certifikata e klientit
connections-client-certificate-dialog = Zgjidhni certifikatën e klientit
connections-client-key = Çelësi i klientit
connections-client-key-dialog = Zgjidhni çelësin e klientit
connections-client-identity-hint = Një certifikatë dhe çelës klienti nevojiten vetëm kur serveri ju kërkon të vërtetoni kush jeni (TLS i ndërsjellë). Çelësi nuk duhet të jetë i mbrojtur me frazëkalim.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importo në { $table }
import-section-source = Burimi
import-section-reading = Leximi
import-section-columns = Kolonat
import-section-preview = Parapamja
import-file = Skedari
import-choose-file = Zgjidhni një skedar…
import-file-dialog = Importo në tabelë
import-no-file = Nuk është zgjedhur skedar
import-unnamed-file = (skedar)
import-format = Formati
import-sheet = Fleta
import-delimiter = Ndarësi
import-header-row = Rreshti i parë është kokë
import-header-row-hint = Përdorni rreshtin e parë si emra kolonash në vend të të dhënave.
import-trim = Hiq hapësirat anësore
import-trim-hint = Hiqni hapësirat rreth çdo vlere — përfshirë brenda thonjëzave.
import-empty-is-null = Fusha bosh është NULL
import-empty-is-null-hint = Një fushë bosh do të thotë NULL dhe jo varg bosh.
import-null-tokens = Vlera të tjera që nënkuptojnë NULL
import-null-tokens-hint = Të ndara me presje, p.sh. NULL, \N, NA.
import-skip = Kapërce
import-nothing-mapped = Asgjë nuk është lidhur, kështu nuk ka asgjë për t’u importuar.
import-values-withheld = Regjistrimi i parë i këtij skedari është shumë i madh për parapamje, kështu u lexuan vetëm emrat e kolonave. Lidhja e kolonave dhe importi e lexojnë të gjithë skedarin.
import-missing-required = Të palidhura, dhe nuk mund të jenë bosh: { $columns }. Importi do të dështojë, përveç nëse baza e të dhënave i plotëson vetë.
import-engine-not-transactional = Motori i ruajtjes i kësaj tabele ({ $engine }) nuk është transaksional, kështu një import i dështuar nuk mund të zhbëhet — rreshtat e ngarkuar para dështimit do të mbeten.
import-problems = { $count ->
    [one] { $count } problem në skedar — nuk u importua asgjë.
   *[other] { $count } probleme në skedar — nuk u importua asgjë.
}
import-problem-at-line = rreshti { $line }: { $message }
import-problem-at-column = rreshti { $line }, { $column }: { $message }
import-issue-not-an-integer = nuk është numër i plotë
import-issue-not-a-number = nuk është numër
import-issue-not-a-boolean = nuk është vlerë e vërtetë/e gabuar
import-issue-cell-error = fleta nuk mundi ta vlerësonte këtë qelizë
import-issue-null-in-not-null = bosh, por kolona nuk mund të jetë NULL
import-issue-field-count = ka { $found } fusha, pritej { $expected }
import-tail-cut = Po shfaqen { $shown } nga { $total }.
import-tail-uncounted = Po shfaqen të gjitha { $total } — kontrolli ndaloi numërimin aty, kështu skedari përmban më shumë.
import-tail-cut-and-uncounted = Po shfaqen { $shown } nga { $total } të parat — kontrolli ndaloi numërimin aty, kështu skedari përmban më shumë.
import-memory-json = Ky skedar JSON është { $size }, dhe një import JSON mbahet në memorie ndërsa ngarkohet — prisni që të kërkojë rreth { $estimate }. Një CSV me të njëjtat të dhëna ngarkohet me memorie konstante.
import-memory-xlsx = Ky libër pune është { $size }, dhe një import Excel mbahet në memorie ndërsa ngarkohet — prisni që të kërkojë përafërsisht { $estimate }, meqë skedari është i ngjeshur. Një CSV i së njëjtës fletë ngarkohet me memorie konstante.
import-read-failed = Skedari nuk mund të lexohej: { $error }
import-no-columns-mapped = Asnjë kolonë e skedarit nuk është lidhur me kolona të tabelës
import-preview-record-too-large = regjistrimi i tij i parë JSON është më i madh se { $mib } MiB që lexon parapamja, kështu nuk ka asgjë për të shfaqur
import-refuse-not-objects = pritej objekte JSON (një varg i tyre, ose një për rresht)
import-refuse-oversize = Ky libër pune është { $size }, dhe një skedar Excel duhet lexuar i tëri para se të mund të shfaqet ndonjë pjesë e tij — { $max } është maksimumi që mund të hapet këtu. Eksportojeni fletën si CSV dhe importoni atë; një CSV ngarkohet me memorie konstante.
import-refuse-oversize-unsized = Ky libër pune është mbi { $max }, dhe një skedar Excel duhet lexuar i tëri para se të mund të shfaqet ndonjë pjesë e tij — { $max } është maksimumi që mund të hapet këtu. Eksportojeni fletën si CSV dhe importoni atë; një CSV ngarkohet me memorie konstante.
import-refuse-inflated = Ky libër pune është { $size } në disk, por shpaketohet në të paktën { $unpacked }, dhe një skedar Excel duhet lexuar i tëri para se të mund të shfaqet ndonjë pjesë e tij — { $ceiling } përmbajtje e shpaketuar është maksimumi që do të hapet këtu, dhe për ta mbajtur nevojitet më shumë memorie se kaq. Eksportojeni fletën si CSV dhe importoni atë; një CSV ngarkohet me memorie konstante.
import-refuse-no-such-sheet = ky libër pune nuk ka fletë të quajtur “{ $sheet }” — ka { $sheets }
import-refuse-no-sheets = ky libër pune nuk ka fleta
import-refuse-too-wide = kjo fletë deklaron { $width } kolona; një fletë pune Excel mban { $max }. Eksportojeni si CSV dhe importoni atë.
import-cancelled = Importi u anulua — transaksioni u rikthye, kështu nuk u shkrua asgjë.
import-done = { $count ->
    [one] U importua { $count } rresht në { $table }.
   *[other] U importuan { $count } rreshta në { $table }.
}
import-next = Tjetër
import-reading = Po lexohet…
import-back = Prapa
import-import = Importo
import-importing = Po importohet…
dialog-read-only = Kjo lidhje është vetëm për lexim.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Eksporto { $database }.{ $schema } në { $format }
dump-title = Eksporto { $database } në { $format }
dump-sql-dialog = Eksporto në SQL
dump-folder-dialog = Eksporto në { $format }
dump-folder-hint = Një skedar { $format } për çdo tabelë, i shkruar në një dosje që zgjidhni ju.
dump-all-selected = Të gjitha { $total } të përzgjedhura
dump-some-selected = { $chosen } nga { $total } të përzgjedhura
dump-select-all = Përzgjidh të gjitha
dump-select-none = Asnjë
dump-reading-tables = Po lexohet lista e tabelave…
dump-no-tables = Kjo bazë të dhënash nuk ka tabela.
dump-tables-unreadable = Lista e tabelave nuk mund të lexohej.
dump-section-contents = Përmbajtja
dump-structure = Struktura
dump-structure-hint = CREATE TABLE, trigger-ët e saj dhe çelësat e jashtëm, të riformuluar pas të dhënave.
dump-data = Të dhënat
dump-data-hint = Çdo rresht, si urdhra INSERT.
dump-other-objects = Objekte të tjera
dump-other-objects-hint = Tipet, sekuencat, rutinat dhe ngjarjet mbi të cilat mbështeten këto tabela.
dump-section-replaying = Riekzekutimi
dump-drop-before-create = Fshi para krijimit
dump-drop-hint-up-front = Fshini çdo tabelë dhe pamje para CREATE-it të saj, që skedari të ngarkohet në një bazë të dhënash që i ka tashmë. Me “Një transaksion” zëvendëson gjithashtu çdo rutinë në skemat e tyre, dhe ndalon para se të fshijë diçka që nuk mund ta rikthejë siç është. Pa “Një transaksion” i lë rutinat siç janë, kështu një riekzekutim në një bazë të dhënash që ende i ka ndalon te e para prej tyre. Pronari dhe lejet e çdo tabele rikthehen në fund.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS para çdo CREATE, që skedari të ngarkohet në një bazë të dhënash që i ka tashmë këto tabela. Zëvendëson gjithashtu çdo rutinë në skemat e tyre. Pronari, lejet dhe siguria e rreshtave e çdo tabele rikthehen në fund.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS para çdo CREATE, që skedari të ngarkohet në një bazë të dhënash që i ka tashmë këto tabela. Zëvendëson gjithashtu çdo rutinë dhe ngjarje në skemat e tyre: në MySQL secila fshihet më parë, gjë që heq çdo leje mbi të, dhe skedari i rijep lejet në fund të tij, aty ku llogaria juaj mund të lexojë mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS para çdo CREATE, që skedari të ngarkohet në një bazë të dhënash që i ka tashmë këto tabela.
dump-one-transaction = Një transaksion
dump-one-transaction-hint = Mbështillni ngarkimin, që një dështim të mos lërë asgjë pas.
dump-disable-fk-checks = Çaktivizo kontrollet e çelësave të jashtëm
dump-fk-hint = Joaktive gjatë ngarkimit, që rendi në të cilin mbërrijnë rreshtat të mos i refuzojë ata.
dump-fk-hint-drops = Joaktive gjatë ngarkimit, që rendi në të cilin mbërrijnë rreshtat të mos i refuzojë ata. Një skedar që fshin tabela i çaktivizon gjithsesi: një fshirje me to aktive fshin rreshtat e tabelave jashtë skedarit përmes veprimeve të tyre ON DELETE.
dump-no-fk-guard-hint = Ky motor nuk ka çelës seance për kontrollet e çelësave të jashtëm; në vend të kësaj, skedari i shton kufizimet pas të dhënave.
dump-export = Eksporto
dump-progress = { $count ->
    [one] Po shkruhet { $table } — { $index } nga { $total }, { $rows } rresht deri tani
   *[other] Po shkruhet { $table } — { $index } nga { $total }, { $rows } rreshta deri tani
}
dump-reading-schema = Po lexohet skema
dump-replace-files = Zëvendëso skedarët
dump-replace-prompt = { $n ->
    [one] { $n } skedar në { $folder } do të zëvendësohet dhe nuk mund të rikuperohet: { $names }.
   *[other] { $n } skedarë në { $folder } do të zëvendësohen dhe nuk mund të rikuperohen: { $names }.
}
dump-replace-prompt-more = { $n } skedarë në { $folder } do të zëvendësohen dhe nuk mund të rikuperohen: { $names }, dhe { $more } të tjerë.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] U shkrua { $n } tabelë.
   *[other] U shkruan { $n } tabela.
}
dump-done-missing = { $n ->
    [one] { $n } tabelë e shënuar nuk u gjet dhe nuk është në skedar: { $names }.
   *[other] { $n } tabela të shënuara nuk u gjetën dhe nuk janë në skedar: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } objekt që kërkoi eksporti nuk është në skedar: { $names }.
   *[other] { $n } objekte që kërkoi eksporti nuk janë në skedar: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Rreshtat e tabelës së brinjëve të grafit { $names } nuk janë në skedar; ajo krijohet bosh.
   *[other] Rreshtat e tabelave të brinjëve të grafit { $names } nuk janë në skedar; ato krijohen bosh.
}
dump-done-refused = { $n ->
    [one] Një riekzekutim në një bazë të dhënash që e ka tashmë këtë ndalon para se të fshijë diçka: skedari nuk mund ta rikthejë siç është: { $names }.
   *[other] Një riekzekutim në një bazë të dhënash që ka tashmë ndonjë prej tyre ndalon para se të fshijë diçka: skedari nuk mund t’i rikthejë siç janë: { $names }.
}
dump-cancelled-partial = Eksporti u anulua — { $name } nuk u ndryshua; ajo që ishte shkruar ndodhet në { $part }
files-done = { $n ->
    [one] U shkrua { $n } skedar.
   *[other] U shkruan { $n } skedarë.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Eksporti u anulua — asnjë skedar nuk përfundoi, kështu nuk u shkrua asgjë në { $folder }.
files-cancelled = { $n ->
    [one] Eksporti u anulua — { $n } skedar përfundoi në { $folder }; tabela që po përpunohej mbeti e pashkruar.
   *[other] Eksporti u anulua — { $n } skedarë përfunduan në { $folder }; tabela që po përpunohej mbeti e pashkruar.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } skedar i shkruar tashmë në { $folder } ruhet.
   *[other] { $message } — { $n } skedarë të shkruar tashmë në { $folder } ruhen.
}
files-replaced = { $n ->
    [one] { $n } skedar ekzistues u zëvendësua: { $names }.
   *[other] { $n } skedarë ekzistues u zëvendësuan: { $names }.
}
files-missing = { $n ->
    [one] { $n } tabelë e shënuar nuk u gjet dhe nuk u bë skedar: { $names }.
   *[other] { $n } tabela të shënuara nuk u gjetën dhe nuk u bënë skedarë: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } nga ~{ $total } rresht
   *[other] { $rows } nga ~{ $total } rreshta
}
export-progress-of = { $count ->
    [one] { $rows } nga { $total } rresht
   *[other] { $rows } nga { $total } rreshta
}
export-progress-rows = { $count ->
    [one] { $rows } rresht
   *[other] { $rows } rreshta
}
export-progress-starting = Po fillon
export-progress-file = Skedari: { $name }
export-progress-writing = Po shkruhet { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Monitor i drejtpërdrejtë
monitor-badge-watching = Në vëzhgim
monitor-badge-paused = Në pauzë
monitor-badge-stopped = Ndalur
monitor-filter-all = Të gjitha
monitor-filter-inserts = Futjet
monitor-filter-updates = Përditësimet
monitor-filter-deletes = Fshirjet
monitor-resume = Rifillo kontrollin periodik
monitor-pause = Pezullo kontrollin periodik
monitor-clear-tip = Pastro regjistrin
monitor-export-tip = Eksporto regjistrin…
monitor-interval = { $secs } s
monitor-clear-title = Të pastrohet regjistri?
monitor-clear-message = { $count ->
    [one] { $count } ndryshim do të hidhej poshtë. Regjistri është e vetmja dëshmi e asaj që mbante një rresht i fshirë — kontrolli periodik nuk do ta raportojë sërish një ndryshim — dhe kjo nuk mund të zhbëhet. Eksportojeni më parë nëse doni ta ruani.
   *[other] { $count } ndryshime do të hidheshin poshtë. Regjistri është e vetmja dëshmi e asaj që mbante një rresht i fshirë — kontrolli periodik nuk do ta raportojë sërish një ndryshim — dhe kjo nuk mund të zhbëhet. Eksportojeni më parë nëse doni ta ruani.
}
monitor-col-time = Koha
monitor-col-action = Veprimi
monitor-no-inserts = Ende nuk ka futje.
monitor-no-updates = Ende nuk ka përditësime.
monitor-no-deletes = Ende nuk ka fshirje.
monitor-empty-stopped = Ndalur.
monitor-empty-paused = Në pauzë.
monitor-waiting = Në pritje
monitor-events-since = { $count ->
    [one] { $count } ngjarje që nga { $since }
   *[other] { $count } ngjarje që nga { $since }
}
monitor-no-changes = Ende nuk ka ndryshime
monitor-partial = vëzhgohen vetëm { $cap } rreshtat e parë sipas çelësit primar
monitor-capped = më të vjetrat po hiqen te kufiri prej { $cap } ndryshimesh
monitor-length-only = { $count ->
    [one] { $columns } është binare, kështu ndryshimet e saj shihen vetëm nga gjatësia
   *[other] { $columns } janë binare, kështu ndryshimet e tyre shihen vetëm nga gjatësia
}
monitor-newest-at-bottom = më të rejat në fund
monitor-export-dialog = Eksporto regjistrin e ndryshimeve

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Krahaso rezultatet
diff-filter-all = Të gjitha
diff-filter-changed = Të ndryshuara
diff-filter-added = Të shtuara
diff-filter-removed = Të hequra
diff-match-rows-by = Përputh rreshtat sipas
diff-whole-row = I gjithë rreshti
diff-export-tip = Eksporto krahasimin…
diff-nothing-to-export = Nuk ka asgjë për të eksportuar.
diff-export-dialog = Eksporto krahasimin
diff-unnamed-file = krahasimi
diff-ran-at = Ekzekutuar në { $time }
diff-side-facts = { $count ->
    [one] { $count } rresht · { $ms } ms
   *[other] { $count } rreshta · { $ms } ms
}
diff-only-in = Vetëm në { $side }: { $columns }
diff-uncompared = Binare, të pakrahasuara: { $columns }
diff-missing-key = të dy rezultatet kanë nevojë për një kolonë të quajtur { $column } për të përputhur rreshtat
diff-binary-key = { $column } mban vlera binare, të cilat nuk krahasohen, kështu nuk mund të përputhë rreshta
diff-summary-identical = { $n ->
        [one] Asnjë ndryshim · { $n } rresht i pandryshuar
       *[other] Asnjë ndryshim · { $n } rreshta të pandryshuar
    }
diff-summary-changed = { $n } të ndryshuar
diff-summary-added = { $n } të shtuar
diff-summary-removed = { $n } të hequr
diff-summary-unchanged = { $n } të pandryshuar
diff-summary-not-compared = { $columns } të pakrahasuara
diff-repeated-keys = { $count ->
    [one] { $count } rresht përsërit një çelës të zënë tashmë në anën e tij, dhe lihet jashtë.
   *[other] { $count } rreshta përsërisin një çelës të zënë tashmë në anën e tyre, dhe lihen jashtë.
}
diff-capped = Një rezultat u kufizua: rreshtat përtej kufirit të tij nuk krahasohen.
diff-col-change = Ndryshimi
diff-no-differences = Asnjë ndryshim.
diff-no-changed = Asnjë rresht i ndryshuar.
diff-no-added = Asnjë rresht i shtuar.
diff-no-removed = Asnjë rresht i hequr.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Para se ta zbatoni
preview-risk-irreversible = Kjo nuk mund të zhbëhet

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Krijo tabelën { $name } me { $columns } kolonë
   *[other] Krijo tabelën { $name } me { $columns } kolona
}
ddl-line-rebuild-table = Rindërto { $name } — nuk mund të ndryshohet në vend, kështu rreshtat kopjohen në një tabelë të re dhe ajo zë vendin e së vjetrës
ddl-line-drop-table = Fshi tabelën
ddl-line-drop-foreign-table = Fshi tabelën e huaj
ddl-line-truncate-table = Fshi çdo rresht
ddl-line-rename-table = Riemërto tabelën në { $to }
ddl-line-add-column = Shto kolonën { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Fshi kolonën { $name }
ddl-line-rebuild-computed-column = Rindërto kolonën e llogaritur { $name } si ({ $expression }) rreth ndryshimit të kolonës
ddl-line-rename-column = Riemërto kolonën { $from } në { $to }{ $retyped ->
        [yes] , dhe tipin e saj nga { $old_type } në { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Zhvendos kolonën { $name } në krye
ddl-line-move-column-after = Zhvendos kolonën { $name } pas { $after }
ddl-line-move-column = Zhvendos kolonën { $name }
ddl-line-retype-column = Ndrysho kolonën { $name } nga { $from } në { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Ndrysho kolonën { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (asnjë)
ddl-line-add-primary-key = Shto çelësin primar ({ $columns })
ddl-line-drop-primary-key = Fshi çelësin primar ({ $columns })
ddl-line-rebuild-primary-key = Rindërto çelësin primar ({ $columns }) rreth ndryshimit të kolonës
ddl-line-change-primary-key = Ndrysho çelësin primar nga ({ $from }) në ({ $to })
ddl-line-add-index = Shto indeksin{ $unique ->
        [yes] {" "}unik
       *[no] {""}
    }{ $has_kind ->
        [yes] {" "}{ $kind }
       *[no] {""}
    } { $name }{ $has_method ->
        [yes] {" "}me { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}mbi ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}duke përfshirë ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}renditur sipas ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}ku { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = columnstore i grupuar
ddl-index-kind-primary-xml = XML primar
ddl-index-kind-selective-xml = XML selektiv
ddl-index-kind-secondary-xml = XML dytësor
ddl-index-kind-spatial = hapësinor
ddl-line-drop-index = Fshi indeksin { $name }
ddl-line-rename-index = Riemërto indeksin { $from } në { $to }
ddl-line-keep-lossy-index = Lëre indeksin { $name } të pandryshuar — Schemaic nuk mund ta lexojë të tërin
ddl-line-add-foreign-key = Shto çelësin e jashtëm { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Fshi çelësin e jashtëm { $name }
ddl-line-add-check = Shto kontrollin { $name } ({ $expression })
ddl-line-drop-check = Fshi kontrollin { $name }
ddl-line-table-options = Cakto për tabelën { $parts }
ddl-table-option-engine = motorin në { $engine }
ddl-table-option-collation = renditjen (collation) në { $collation }
ddl-table-option-comment = komentin
ddl-table-option-comment-cleared = komentin (të pastruar)
ddl-line-create-view = Krijo pamjen { $name }
ddl-line-create-view-index = Krijo pamjen { $name } dhe indeksin e saj { $index }
ddl-line-create-view-indexes = Krijo pamjen { $name } dhe indekset e saj { $indexes }
ddl-line-redefine-view = Ripërkufizo pamjen { $name }
ddl-line-recreate-view = Fshi dhe rikrijo pamjen { $name }
ddl-line-rename-view = Riemërto pamjen në { $to }
ddl-line-drop-view = Fshi pamjen
ddl-line-drop-materialized-view = Fshi pamjen e materializuar
ddl-line-refresh-view = Rindërto rreshtat e pamjes së materializuar — lexuesit bllokohen derisa të përfundojë
ddl-line-refresh-view-concurrently = Rindërto rreshtat e pamjes së materializuar pa bllokuar lexuesit
ddl-line-create-trigger = Krijo trigger-in { $name }
ddl-line-redefine-trigger = Ripërkufizo trigger-in { $name }
ddl-line-recreate-trigger = Rikrijo trigger-in { $name }
ddl-line-recreate-trigger-as = Rikrijo trigger-in { $from } si { $name }
ddl-line-drop-trigger = Fshi trigger-in { $name }
ddl-line-create-routine = Krijo { $what } { $name }
ddl-line-redefine-routine = Ripërkufizo { $what } { $name }
ddl-line-redefine-routine-as = Ripërkufizo { $what } { $from } si { $name }
ddl-line-recreate-routine = Rikrijo { $what } { $name }
ddl-line-recreate-routine-as = Rikrijo { $what } { $from } si { $name }
ddl-line-rename-routine = Riemërto { $what } { $from } në { $to }
ddl-line-drop-routine = Fshi { $what } { $name }
ddl-line-create-event = Krijo ngjarjen { $name }
ddl-line-alter-event = Ndrysho ngjarjen { $name }
ddl-line-alter-event-edits = Ndrysho ngjarjen { $name }: { $edits }
ddl-line-drop-event = Fshi ngjarjen { $name }
ddl-event-edit-rename = riemërto në { $to }
ddl-event-edit-schedule = orari në { $schedule }
ddl-event-edit-status-enabled = statusi në aktivizuar
ddl-event-edit-status-disabled = statusi në çaktivizuar
ddl-event-edit-status-replica = statusi në çaktivizuar në replikë
ddl-event-edit-preserve = ruaje pasi të përfundojë
ddl-event-edit-no-preserve = fshije pasi të përfundojë
ddl-event-edit-definer = përkufizuesi në { $definer }
ddl-event-edit-comment = komenti
ddl-event-edit-body = trupi
ddl-line-create-enum = { $values ->
    [one] Krijo tipin { $name } me { $values } vlerë
   *[other] Krijo tipin { $name } me { $values } vlera
}
ddl-line-add-enum-value = Shto vlerën { $value }
ddl-line-add-enum-value-after = Shto vlerën { $value } pas { $after }
ddl-line-add-enum-value-before = Shto vlerën { $value } para { $before }
ddl-line-rename-enum-value = Riemërto vlerën { $from } në { $to }
ddl-line-recreate-enum = Rikrijo tipin { $name } ({ $values ->
    [one] { $values } vlerë
   *[other] { $values } vlera
}, duke rikonvertuar { $columns ->
    [one] { $columns } kolonë
   *[other] { $columns } kolona
})
ddl-line-create-domain = Krijo domenin { $name } si { $datatype }
ddl-line-set-domain-default = Cakto vlerën e paracaktuar të domenit në { $default }
ddl-line-drop-domain-default = Hiq vlerën e paracaktuar të domenit
ddl-line-domain-not-null = Bëje domenin NOT NULL
ddl-line-domain-nullable = Bëje domenin të pranojë NULL
ddl-line-recreate-domain = Rikrijo domenin { $name } si { $datatype } (ishte { $was }; duke rikonvertuar { $columns ->
    [one] { $columns } kolonë
   *[other] { $columns } kolona
})
ddl-line-create-sequence = Krijo sekuencën { $name }
ddl-line-alter-sequence = Cakto për sekuencën { $edits }
ddl-sequence-edit-type = tipin në { $datatype }
ddl-sequence-edit-increment = rritjen në { $value }
ddl-sequence-edit-minimum = minimumin në { $value }
ddl-sequence-edit-maximum = maksimumin në { $value }
ddl-sequence-edit-start = fillimin në { $value }
ddl-sequence-edit-cache = cache në { $value }
ddl-sequence-edit-cycling-on = ciklin aktiv
ddl-sequence-edit-cycling-off = ciklin joaktiv
ddl-sequence-edit-owner = pronarin në { $owner }
ddl-sequence-edit-no-owner = pronarin në asnjë
ddl-line-restart-sequence = Rinis sekuencën në { $value }
ddl-line-create-object = Krijo { $what } { $name }
ddl-line-replace-object = Fshi { $what } { $name } dhe krijoje përsëri
ddl-line-replace-object-as = Fshi { $what } { $name } dhe krijoje përsëri si { $to }
ddl-line-alter-tsql-sequence = Ndrysho sekuencën: { $clauses }
ddl-line-rename-object = Riemërto { $what } në { $to }
ddl-line-drop-object = Fshi { $what }
ddl-line-set-object-comment = Cakto komentin e objektit ({ $what })
ddl-line-clear-object-comment = Pastro komentin e objektit ({ $what })
ddl-line-create-database = Krijo bazën e të dhënave { $name }
ddl-line-drop-database = Fshi bazën e të dhënave { $name }
ddl-line-create-schema = Krijo skemën { $name }
ddl-line-drop-schema = Fshi skemën { $name }
ddl-line-create-extension = Krijo zgjerimin { $name }
ddl-line-create-user = Krijo përdoruesin { $who }
ddl-line-create-role = Krijo rolin { $who }
ddl-line-create-login = Krijo login-in { $who }
ddl-line-drop-user = Fshi përdoruesin { $who }
ddl-line-drop-role = Fshi rolin { $who }
ddl-line-drop-login = Fshi login-in { $who }
ddl-line-set-password = Cakto fjalëkalimin për { $who }
ddl-line-grant-privileges = Jepi { $privileges } { $who }
ddl-line-revoke-privileges = Hiqi { $privileges } { $who }
ddl-privileges-none = asnjë privilegj
ddl-privileges-count = { $count } privilegje
ddl-line-grant-role = Jepi { $role } { $member }
ddl-line-revoke-role = Hiqi { $role } { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Fshin { $name } dhe e rikrijon, duke kopjuar çdo rresht, dhe rikthen çelësat, indekset, kontrollet, vlerat e paracaktuara dhe trigger-ët e saj, komentet e saj dhe të kolonave të saj, si dhe çelësat e jashtëm që kanë tabela të tjera mbi të. Ndalon para se të fillojë nëse tabela ka diçka tjetër që Schemaic nuk e riformulon — leje, pronar të vetin, një objekt të varur të lidhur me skemën, versionim sistemi, ledger, replikim, ngjeshje, opsione indeksi, një indeks të çaktivizuar, një rregull ose vlerë të paracaktuar të lidhur, një klasifikim ndjeshmërie, një përshkrim mbi një çelës, indeks, vlerë të paracaktuar, kontroll ose trigger. Statistikat e krijuara me dorë nuk barten.
       *[other] Fshin { $name } dhe e rikrijon, duke kopjuar çdo rresht. Çdo gjë rreth tabelës që Schemaic nuk e lexoi nuk bartet.
    }
ddl-risk-drop-table = Fshin tabelën dhe çdo rresht në të.
ddl-risk-drop-foreign-table = Fshin tabelën e huaj: përkufizimi i saj zhduket, dhe rreshtat që lexon mbeten aty ku janë, në serverin e huaj ose në skedar; një pamje ose rutinë që e emërton dështon që nga ky moment.
ddl-risk-truncate-table = Fshin çdo rresht në tabelë. Kjo nuk mund të zhbëhet.
ddl-risk-drop-column = Fshin kolonën { $name } dhe të gjitha të dhënat në të.
ddl-risk-rebuild-computed-column = Fshin kolonën e llogaritur { $name } dhe e shton përsëri, kështu ajo zhvendoset në fund të tabelës.
ddl-risk-column-not-null = Kolona { $name } bëhet NOT NULL — urdhri dështon nëse ndonjë rresht mban NULL.
ddl-risk-column-stops-generated = Kolona { $name } nuk gjenerohet më. Vlerat e saj aktuale ruhen, por asgjë nuk i rillogarit që tani e tutje.
ddl-risk-column-becomes-generated = Kolona { $name } bëhet e gjeneruar nga { $expression }. Çdo vlerë në të zëvendësohet me atë që llogarit shprehja.
ddl-risk-column-expression-changes = Shprehja e kolonës { $name } ndryshon nga { $from } në { $to }. Në PostgreSQL kjo e fshin dhe e shton përsëri kolonën, kështu çdo vlerë rillogaritet dhe kolona zhvendoset në fund të tabelës.
ddl-risk-type-narrows = Ngushtimi i { $subject } nga { $from } në { $to } { $loss ->
        [truncates] i shkurton vlerat që nuk përshtaten më
        [rounds] rrumbullakos çdo vlerë në kolonë
       *[both] i shkurton vlerat që nuk përshtaten më dhe rrumbullakos çdo vlerë në kolonë
    }.
ddl-risk-type-rewrites = Ndryshimi i { $subject } nga { $from } në { $to } rishkruan çdo vlerë; mund të dështojë ose të humbasë saktësi.
ddl-risk-domain-subject = domenit { $name }
ddl-risk-primary-key-dropped = E lë tabelën pa çelës primar — rreshtat nuk mund të redaktohen më nga rrjeta.
ddl-risk-primary-key-rebuilt = Fshin dhe shton përsëri çelësin primar rreth ndryshimit të kolonës. Kur ai është indeksi i grupuar, kjo rishkruan të gjithë tabelën, dhe një çelës i jashtëm në një tabelë tjetër që i referohet e ndalon planin.
ddl-risk-primary-key-replaced = Zëvendëson çelësin primar: { $count ->
        [one] { $columns } nuk është
       *[other] { $columns } nuk janë
    } më unike, dhe vlerat e dyfishta pranohen që tani e tutje. Në MySQL kjo rindërton indeksin e grupuar, gjë që rishkruan të gjithë tabelën.
ddl-risk-drop-foreign-key = Fshin çelësin e jashtëm { $name }. Rreshtat që nuk kanë më prind përkatës pranohen që tani e tutje, dhe të dhënat ekzistuese nuk rishqyrtohen nëse ai shtohet përsëri.
ddl-risk-drop-unique-index = Fshin indeksin unik { $name }. Vlerat e dyfishta pranohen që tani e tutje, dhe rrjeta mund të humbasë çelësin me të cilin po redaktonte rreshtat.
ddl-risk-drop-view = { $materialized ->
        [yes] Fshin pamjen e materializuar. { $cost }
       *[no] Fshin pamjen. { $cost }
    }
ddl-risk-view-cost-sqlite = Pamjet që lexojnë prej saj nuk zgjidhen më derisa ajo të kthehet, dhe trigger-ët e saj INSTEAD OF fshihen bashkë me të dhe nuk rikthehen.
ddl-risk-view-cost-sqlite-restored = Pamjet që lexojnë prej saj nuk zgjidhen më derisa ajo të kthehet, dhe trigger-ët e saj INSTEAD OF fshihen bashkë me të.
ddl-risk-view-cost-sqlserver = Lejet mbi të, përshkrimi i saj dhe trigger-ët e saj INSTEAD OF fshihen bashkë me të dhe nuk rikthehen.
ddl-risk-view-cost-sqlserver-restored = Lejet mbi të dhe përshkrimi i saj fshihen bashkë me të dhe nuk rikthehen; fshihen edhe trigger-ët e saj INSTEAD OF.
ddl-risk-view-cost-other = Pamjet e varura, rregullat, lejet dhe trigger-ët e saj INSTEAD OF fshihen bashkë me të dhe nuk rikthehen.
ddl-risk-view-cost-other-restored = Pamjet e varura, rregullat dhe lejet fshihen bashkë me të dhe nuk rikthehen; fshihen edhe trigger-ët e saj INSTEAD OF.
ddl-risk-recreate-view = Rikrijimi i { $name } e fshin më parë. { $cost } { $why ->
        [sqlite] SQLite nuk ka mënyrë për ta zëvendësuar një pamje në vend, kështu çdo redaktim i saj — përfshirë një riemërtim — ndjek këtë rrugë.
        [sqlserver] sp_rename i SQL Server do ta linte përkufizimin e ruajtur të pamjes duke emërtuar emrin e vjetër, kështu një riemërtim ndjek këtë rrugë.
       *[postgres] PostgreSQL nuk mund të zëvendësojë një pamje kolonat e së cilës ndryshuan emër, tip ose renditje, kështu kjo është e vetmja mënyrë për të zbatuar redaktimin.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } urdhër i fshirë bashkë me të rikthehet më pas, në të njëjtin transaksion.
       *[other] { $count } urdhra të fshirë bashkë me të rikthehen më pas, në të njëjtin transaksion.
    }
ddl-risk-view-indexes-dropped = Ripërkufizimi i pamjes { $view } fshin { $dropped_count ->
        [one] indeksin e saj
       *[other] indekset e saj
    } ({ $dropped }) — ALTER VIEW i SQL Server e bën gjithmonë këtë — { $case ->
        [all] dhe ky plan { $again_count ->
            [one] e ndërton
           *[other] i ndërton
        } sërish pas tij, në të njëjtin transaksion. Në një pamje të madhe kjo zgjat aq sa zgjati ndërtimi { $again_count ->
            [one] i tij
           *[other] i tyre
        } herën e parë. Një indeks me fill factor, ngjeshje, padding, opsione bllokimi, filegroup ose përshkrim e ndalon planin para se të ndryshojë diçka — Schemaic nuk mund t’i ndërtojë sërish këto. Statistikat e krijuara me dorë mbi pamjen nuk barten.
        [none] dhe ky plan nuk { $gone_count ->
            [one] e ndërton
           *[other] i ndërton
        } sërish: pamja me të cilën përshtatet nuk ka indeks të tillë. Statistikat e krijuara me dorë mbi pamjen nuk barten.
       *[some] dhe ky plan ndërton sërish { $again } pas tij, në të njëjtin transaksion, por jo { $gone }, që pamja me të cilën përshtatet nuk e ka. Një indeks me fill factor, ngjeshje, padding, opsione bllokimi, filegroup ose përshkrim e ndalon planin para se të ndryshojë diçka — Schemaic nuk mund t’i ndërtojë sërish këto. Statistikat e krijuara me dorë mbi pamjen nuk barten.
    }
ddl-risk-add-not-null-column = Kolona { $name } është NOT NULL pa vlerë të paracaktuar. Nëse tabela ka tashmë rreshta, nuk ka vlerë për t’u dhënë atyre, dhe motori do ta refuzojë të gjithë planin.
ddl-risk-drop-check = Fshin kontrollin { $name }. Rreshtat që kufizimi i refuzonte pranohen që tani e tutje, dhe të dhënat ekzistuese nuk rishqyrtohen nëse ai shtohet përsëri.
ddl-risk-keep-lossy-index = Redaktimi juaj i indeksit { $name } nuk përfshihet. Ai përdor diçka që Schemaic nuk mund ta rilexojë — një kolonë çelësi me shprehje, një klasë operatori ose një renditje NULLS — dhe zbatimi i redaktimit do të nënkuptonte rikrijimin e indeksit pa të. Redaktojeni këtë indeks në SQL.
ddl-risk-drop-trigger = Fshin trigger-in { $name }. Çfarëdo që bënte në çdo shkrim nuk ndodh më, dhe rreshtave të shkruar që tani e tutje nuk u zbatohet.
ddl-risk-redefine-trigger-in-place = Ripërkufizon trigger-in { $name } në vend — nuk fshihet, kështu lejet e tij mbeten — dhe ajo që bën në çdo shkrim ndryshon që tani e tutje.
ddl-risk-recreate-trigger = Rikrijimi i trigger-it { $name } e fshin më parë. Aty ku DDL nuk është transaksional (MySQL), një përkufizim i ri që serveri e refuzon e lë tabelën pa trigger.
ddl-risk-signature-lost = { $kind ->
        [trigger] Trigger-i
        [function] Funksioni
       *[procedure] Procedura
    } { $name } është i nënshkruar (ADD SIGNATURE), dhe çdo ndryshim i tij e heq nënshkrimin — Schemaic nuk mund ta nënshkruajë sërish pa çelësin e certifikatës. Çfarëdo që i lejonte certifikata të bënte nuk funksionon më derisa të ekzekutoni sërish ADD SIGNATURE.
ddl-risk-redefines-routine = Ripërkufizon { $name }. Çdo gjë që e thërret ekzekuton trupin e ri që tani e tutje, përfshirë trigger-ë në tabela të tjera që ky redaktim nuk i përmendi kurrë.
ddl-risk-routine-parameters-change = Ndryshon listën e parametrave të { $name }. Thirrësit që japin argumentet e vjetra nuk zgjidhen më drejt saj.
ddl-risk-recreate-routine = Rikrijimi i { $held } e fshin më parë. Aty ku DDL nuk është transaksional (MySQL), një përkufizim i ri që serveri e refuzon nuk lë fare { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Privilegjet e dhëna mbi { $held } dhe komenti i saj nuk i mbijetojnë fshirjes, dhe ky plan nuk i rikthen — zbatoni sërish çdo GRANT dhe COMMENT më pas.
       *[no] Privilegjet e dhëna mbi { $held } nuk i mbijetojnë fshirjes, dhe ky plan nuk i rikthen — zbatoni sërish çdo GRANT më pas.
    }
ddl-risk-drop-numbered-group = Fshin { $what } { $name } dhe bashkë me të çdo anëtar të grupit të tij të numëruar: { $members }.
ddl-risk-drop-routine = Fshin { $what } { $name }. PostgreSQL refuzon ndërsa një trigger ende e përdor, kështu ata që e përdorin duhet fshirë më parë.
ddl-risk-drop-event = Fshin ngjarjen { $name }. Orari dhe trupi i saj zhduken bashkë me të — asgjë tjetër në bazën e të dhënave nuk mban një kopje.
ddl-risk-one-shot-event = Kjo ngjarje ekzekutohet një herë dhe pastaj serveri e fshin — kjo është ON COMPLETION NOT PRESERVE. Aktivizoni “Ruaje” për ta mbajtur.
ddl-risk-one-shot-event-altered = Ngjarja { $name } do të ekzekutohet një herë dhe pastaj serveri do ta fshijë — kjo është ON COMPLETION NOT PRESERVE. Aktivizoni “Ruaje” për ta mbajtur.
ddl-risk-add-enum-value = Shtimi i { $value } nuk mund të zhbëhet në vend — PostgreSQL nuk mund të heqë një vlerë enum, kështu tërheqja e saj do të thotë rikrijim i tipit. Vlera gjithashtu nuk mund të përdoret derisa të zbatohet ky plan.
ddl-risk-rename-enum-value = Çdo rresht që mban { $from } lexon { $to } që tani e tutje. Nëse donit ta hiqnit { $from } në vend që ta riemërtonit, fshijeni dhe zbatojeni këtë më vete.
ddl-risk-recreate = Rikrijimi i { $kind ->
        [domain] domenit
       *[type] tipit
    } { $name } e fshin dhe e rindërton. Çdo kolonë më poshtë rikonvertohet përmes tekstit; një vlerë që përkufizimi i ri nuk e pranon e rrëzon të gjithë planin, dhe nuk zbatohet asgjë{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}dhe { $more } të tjera
        }. Një pamje ose funksion i ndërtuar mbi të nuk mund të renditet këtu dhe do ta bënte serverin të refuzonte.
       *[no] . Asgjë nuk e përdor sot, por një pamje ose funksion i ndërtuar mbi të nuk mund të renditet këtu dhe do ta bënte serverin të refuzonte.
    }
ddl-risk-drop-domain-check = Fshin kontrollin { $name }. Asnjë kolonë e këtij domeni nuk kontrollohet më, dhe të dhënat ekzistuese nuk rishqyrtohen nëse ai shtohet përsëri.
ddl-risk-domain-nullable = Çdo kolonë e këtij domeni fillon të pranojë NULL, përfshirë ato në tabela që ky redaktim nuk i përmendi kurrë.
ddl-risk-domain-not-null = Urdhri dështon nëse ndonjë kolonë e këtij domeni mban tashmë NULL.
ddl-risk-drop-sequence = Fshin sekuencën dhe pozicionin që kishte arritur. Serveri refuzon ndërsa vlera e paracaktuar e një kolone ende merr prej saj, kështu ato vlera të paracaktuara duhet të hiqen më parë.
ddl-risk-drop-synonym = Fshin sinonimin. Asgjë nuk e refuzon: një pamje, rutinë ose kërkesë që e emërton dështon që nga ky moment, herën tjetër që ekzekutohet.
ddl-risk-drop-extension = Fshin zgjerimin dhe funksionet, tipet dhe operatorët që ai instaloi. Serveri refuzon ndërsa diçka e juaja ende varet prej tyre — një kolonë e tipit të tij, një indeks ose një server mbi wrapper-in e tij — dhe emërton se çfarë.
ddl-risk-drop-used-object = Fshin { $what }. Serveri refuzon ndërsa një kolonë ende e përdor, kështu atyre kolonave duhet t’u ndryshohet më parë tipi.
ddl-risk-restart-sequence = Rinis numëruesin në { $value }. Vlerat e dhëna tashmë nuk ndryshohen, kështu një çelës që ai arrin sërish përplaset.
ddl-risk-replace-synonym = Fshin sinonimin dhe e krijon përsëri. Asgjë nuk e refuzon fshirjen: dy urdhrat ekzekutohen në një transaksion.
ddl-risk-replace-sequence = Fshin sekuencën dhe e krijon përsëri: pozicioni që kishte arritur humbet, dhe ajo fillon nga e para në { $start }. Serveri e refuzon fshirjen ndërsa vlera e paracaktuar e një kolone ende merr prej saj.
ddl-risk-start-value = vlerën e saj fillestare
ddl-risk-replace-object = Fshin { $what } dhe e krijon përsëri. Serveri e refuzon fshirjen ndërsa një kolonë ende e përdor, kështu atyre kolonave duhet t’u ndryshohet më parë tipi.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Refuzohet ndërsa ka leje mbi të, pronar të vetin, veti të zgjeruara ose një rregull apo vlerë të paracaktuar të lidhur: rikrijimi nuk do t’i rikthente.
       *[no] Refuzohet ndërsa ka leje mbi të, pronar të vetin ose veti të zgjeruara: rikrijimi nuk do t’i rikthente.
    }
ddl-risk-drop-database = Fshin bazën e të dhënave { $name } dhe gjithçka në të — çdo tabelë dhe të gjitha rreshtat e saj, çdo pamje, indeks, rutinë dhe trigger. Asgjë nuk ruhet dhe nuk ka zhbërje: e vetmja rrugë kthimi është një kopje rezervë e marrë para se të ekzekutohet kjo.
ddl-risk-drop-schema = Fshin skemën { $name }. Serveri refuzon ndërsa ajo ende mban tabela, pamje ose rutina, kështu kjo ose merr një skemë bosh ose dështon{ $cascade ->
        [yes] {" "}— Schemaic nuk dërgon kurrë CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Fshin login-in { $who }, dhe bashkë me të çdo leje serveri dhe anëtarësi në role serveri që ai mban — që nuk regjistrohen askund tjetër, kështu një login i krijuar sërish me këtë emër fillon pa to. Serveri refuzon ndërsa diçka është e lidhur si ai, kështu seancat e tij duhet të përfundojnë më parë. Përdoruesit e tij në secilën bazë të dhënash nuk fshihen: mbeten, me lejet e tyre, të palidhur me asnjë login, dhe një login i ri me këtë emër nuk lidhet sërish me ta pa ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Fshin { $who } dhe çdo privilegj që mban. Lejet nuk regjistrohen askund tjetër, kështu rikthimi i llogarisë do të thotë t’i jepni të gjitha sërish nga kujtesa. { $cuts_off ->
        [yes] Çdo gjë e lidhur si ajo dështon në urdhrin e saj të ardhshëm në këtë bazë të dhënash.
       *[no] Çdo gjë ende e lidhur si ajo vazhdon të punojë derisa të shkëputet.
    }
ddl-risk-set-password = Ndryshon fjalëkalimin e { $who }. I vjetri nuk mund të rikuperohet, kështu çdo gjë ende e konfiguruar me të — një aplikacion, një punë e planifikuar, klienti i një zhvilluesi tjetër — nuk lidhet dot derisa të përditësohet me dorë. Seancat e hapura tashmë vazhdojnë të punojnë.
ddl-risk-revoke-privileges = I heq { $privileges } { $who }. Çdo gjë që mbështetet në to — një aplikacion, një punë e planifikuar, një pamje në pronësi të dikujt tjetër — fillon të dështojë në urdhrin e saj të ardhshëm.{ $cascades ->
        [yes] {" "}Heqja mbart CASCADE, kështu u hiqet edhe çdo llogarie së cilës { $who } ia ka dhënë, dhe ajo që ekzekutohet si ato llogari dështon në të njëjtën mënyrë.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Aty ku { $who } e ka të ndaluar (DENY), heqja e anulon ndalimin — gjë që e rikthen qasjen nëse një rol ku bën pjesë e jep atë.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}Në një tabelë fshin gjithashtu çdo dhënie dhe DENY në nivel kolone të { $cleared } që { $who } ka mbi kolonat e tabelës: një kolonë që i ishte ndaluar { $who } nuk është më e ndaluar, dhe një e dhënë më vete nuk është më e dhënë.
       *[no] {""}
    }
ddl-risk-revoke-role = I heq rolin { $role } { $member }, dhe bashkë me të çdo privilegj që mbart roli.
ddl-risk-blank-password = { $who } nuk do të ketë fjalëkalim: kushdo që mund të arrijë serverin mund të hyjë si ai. Caktoni një këtu, ose me ALTER USER më pas.
ddl-risk-host-is-everywhere = Hosti i saj është % — çdo makinë në rrjet, jo vetëm kjo.
ddl-risk-grant-server-wide = I jep { $privileges } { $who } në të gjithë serverin — vetë instancën, jo bazën e të dhënave që po shfletohet.
ddl-risk-grant-every-database = I jep { $privileges } { $who } në çdo bazë të dhënash në këtë server — përfshirë bazat e të dhënave që ende nuk ekzistojnë, jo vetëm atë që po shfletohet.
ddl-risk-grant-keeps-column-denies = Një dhënie e { $privileges } në nivel tabele fshin çdo DENY të saj që { $who } ka mbi një nga kolonat e tabelës, kështu urdhri ia ndalon sërish ato kolona { $who } menjëherë pas dhënies: një kolonë që i ishte ndaluar { $who } mbetet e ndaluar. Aty ku { $who } mban edhe opsionin e dhënies mbi tabelën, serveri refuzon ta ndalojë sërish pa CASCADE, dhe nuk zbatohet asgjë.
ddl-risk-star-refresh = Rifreskon çdo pamje dhe funksion inline në këtë bazë të dhënash që bën select * nga { $table }, drejtpërdrejt ose përmes një sinonimi: SQL Server i lidh ato me kolonat e tabelës sipas pozicionit, dhe përndryshe do të lexonin e shkruanin kolonat e gabuara. Plani dështon nëse ndonjëra prej tyre nuk kompilohet më kundrejt kolonave të reja. Një e tillë në një bazë tjetër të dhënash që bën select * nga { $table } rifreskohet pasi plani konfirmohet, jashtë transaksionit të tij: deri atëherë lexon pozicionet e vjetra, dhe nëse rifreskimi i saj dështon, plani mbetet i zbatuar dhe pamja emërtohet këtu.
ddl-risk-piecewise = SQL Server nuk krijon, ndryshon ose fshin një modul të kompiluar në mënyrë native brenda një transaksioni, kështu ky plan ekzekutohet urdhër pas urdhri dhe nuk mund të ndalet: nëse një urdhër dështon, ata para tij mbeten të zbatuar.
ddl-risk-drop-sequence-table = Fshin sekuencën: vlera që ka arritur humbet, dhe çdo gjë që ende merr prej saj dështon.
ddl-risk-drop-scale = Sipas statistikave të pemës së skemës, kjo është { $rows } rreshta.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = propozimi është për tabelën `{ $proposed }`, por është e hapur `{ $actual }`
propose-no-ops = propozimi nuk kërkon asnjë ndryshim
propose-no-such-column = `{ $table }` nuk ka kolonë të quajtur `{ $name }`
propose-column-exists = `{ $table }` ka tashmë një kolonë të quajtur `{ $name }`
propose-no-such-index = `{ $table }` nuk ka indeks të quajtur `{ $name }`
propose-index-exists = `{ $table }` ka tashmë një indeks të quajtur `{ $name }`
propose-no-such-foreign-key = `{ $table }` nuk ka çelës të jashtëm të quajtur `{ $name }`
propose-no-such-check = `{ $table }` nuk ka kufizim kontrolli të quajtur `{ $name }`
propose-key-width-mismatch = një çelës i jashtëm ka nevojë për aq kolona të referuara sa kolona referuese
propose-empty-index = një indeks ka nevojë për të paktën një kolonë
propose-invalid = tabela e propozuar nuk do të ishte e vlefshme: { $problem }
propose-not-a-table = `{ $name }` është pamje — një propozim mund të ndryshojë vetëm një tabelë
propose-no-such-table = nuk ka tabelë të quajtur `{ $name }` — thirrni list_schema për të parë çfarë ekziston
propose-not-an-expression = { $field ->
        [default] vlera e paracaktuar `{ $text }` nuk është një shprehje e vetme SQL
        [generated] shprehja e gjenerimit `{ $text }` nuk është një shprehje e vetme SQL
       *[check] shprehja e kontrollit `{ $text }` nuk është një shprehje e vetme SQL
    }
propose-not-a-type = `{ $text }` nuk është tip kolone — jepni vetëm tipin, pa vlerë të paracaktuar, pa NOT NULL dhe pa asgjë pas tij
propose-unknown-action = `{ $text }` nuk është veprim çelësi të jashtëm
propose-not-loaded = { $table } nuk është ngarkuar në { $database } tani — hapni bazën e të dhënave në pemën e skemës, ose prisni që të përfundojë një rifreskim, dhe provoni sërish.
propose-nothing-to-change = { $subject } duket tashmë kështu — nuk ka asgjë për të ndryshuar.
propose-no-database = Kjo skedë nuk ka bazë të dhënash të zgjedhur, kështu një ndryshim skeme i zbatuar nga këtu nuk do të kishte ku të shkonte. Zgjidhni më parë një për këtë lidhje — nëse çdo bazë të dhënash është e fshehur, syri te SKEMA është vendi ku mund ta riktheni një.
propose-other-connection = Ky ndryshim ka të bëjë me një skedë në një lidhje tjetër — kaloni në të, ose përzgjidhni lidhjen e saj në pemë, para se të zbatoni një ndryshim skeme.
ai-run-no-database = Kjo skedë nuk ka bazë të dhënash të zgjedhur, kështu një urdhër i ekzekutuar nga këtu nuk do të kishte ku të shkonte. Zgjidhni më parë një për këtë lidhje — nëse çdo bazë të dhënash është e fshehur, syri te SKEMA është vendi ku mund ta riktheni një.
ai-run-other-connection = Kjo bisedë ka të bëjë me një skedë në një lidhje tjetër. Një skedë e re do të hapej në lidhjen e përzgjedhur në pemë, kështu urdhri do të ekzekutohej diku tjetër — kaloni më parë në atë skedë, ose përzgjidhni lidhjen e saj në pemë.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, të cilën ky plan e riemërton në { $to }
ddl-moved-dropped = { $name }, të cilën ky plan e fshin
ddl-withheld-unreplayable-rename = { $count ->
        [one] Riemërtimi i { $was } në { $to } nuk mund ta bartë { $count } trigger-in e saj: SQL-ja e tij emërton { $was }, dhe rishkrimi i saj këtu do të ndryshonte atë që keni shkruar. Riemërtojeni pamjen më vete pasi të keni rikrijuar trigger-ët me emrin e ri.
       *[other] Riemërtimi i { $was } në { $to } nuk mund t’i bartë { $count } trigger-ët e saj: SQL-ja e tyre emërton { $was }, dhe rishkrimi i saj këtu do të ndryshonte atë që keni shkruar. Riemërtojeni pamjen më vete pasi të keni rikrijuar trigger-ët me emrin e ri.
    }
ddl-withheld-numbered-group = { $name } kryeson një grup procedurash të numëruara ({ $members }), dhe ky plan e fshin më parë, gjë që fshin bashkë me të çdo anëtar të grupit. Redaktojeni me emrin dhe llojin e vet, gjë që e ndryshon në vend dhe i mban ata.
ddl-withheld-view-index-create = { $count ->
        [one] Krijimi i pamjes { $view } nuk mund të krijojë indeksin e saj { $indexes } — i një lloji që Schemaic nuk mund ta riformulojë. Krijojeni këtë pamje në SQL.
       *[other] Krijimi i pamjes { $view } nuk mund të krijojë indekset e saj { $indexes } — të një lloji që Schemaic nuk mund ta riformulojë. Krijojeni këtë pamje në SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Redaktimi i pamjes { $view } fshin indeksin e saj { $indexes }, të cilin Schemaic nuk mund ta krijojë sërish — i një lloji që Schemaic nuk mund ta riformulojë. Redaktojeni këtë pamje në SQL.
       *[other] Redaktimi i pamjes { $view } fshin indekset e saj { $indexes }, të cilat Schemaic nuk mund t’i krijojë sërish — të një lloji që Schemaic nuk mund ta riformulojë. Redaktojeni këtë pamje në SQL.
    }
ddl-withheld-tsql-lossy-index = Rindërtimi i { $table } do të fshinte indeksin { $index }: është i një lloji që Schemaic nuk e lexon të plotë
ddl-withheld-tsql-encrypted-trigger = Rindërtimi i { $table } do të fshinte trigger-in { $trigger }: teksti i tij është i shifruar
ddl-withheld-tsql-signed-trigger = Rindërtimi i { $table } do të hiqte nënshkrimin e trigger-it { $trigger }, i cili nuk mund të rikthehet pa çelësin privat të certifikatës
ddl-withheld-tsql-renamed-trigger = Rindërtimi i { $table } me një emër të ri do ta linte trigger-in { $trigger } duke emërtuar emrin e vjetër
ddl-withheld-partly-read = Indeksi { $index } u lexua vetëm pjesërisht — SQLite e mban kushtin e një indeksi të pjesshëm, dhe një çelës me shprehje, vetëm në tekstin CREATE të vetë indeksit{ $why ->
        [may-name] , dhe ai tekst mund të emërtojë { $moved }. Fshijeni indeksin për të vazhduar pa të.
        [not-found] , kështu nuk mund të krijohet nga ajo që u lexua. Fshijeni indeksin për të vazhduar pa të.
        [edited] , kështu ky redaktim i tij nuk mund të shkruhet — ajo që u lexua nuk është i gjithë indeksi. Zhbëni ndryshimin për ta mbajtur indeksin siç është, ose fshijeni.
       *[no-text] {" "}— dhe për këtë nuk ka tekst të tillë, kështu rikrijimi i tij do të ndryshonte atë që indekson. Fshijeni indeksin për të vazhduar pa të.
    }
ddl-withheld-condition-names-dropped = Kushti i indeksit { $index } emërton { $column }, të cilën ky plan e fshin. Redaktoni kushtin ose fshini më parë indeksin.
ddl-withheld-strands-triggers = Kjo tabelë ka { $count ->
        [one] { $count } trigger
       *[other] { $count } trigger-ë
    } mbi të, dhe SQL-ja e tyre riekzekutohet saktësisht siç e ruajti SQLite — që do të emërtonte ende { $moved }. SQLite e pranon një trigger të tillë dhe pastaj refuzon çdo shkrim në tabelë. Fshini ose redaktoni më parë trigger-in, ose riemërtojeni kolonën pa asnjë ndryshim tjetër, që ALTER TABLE i vetë SQLite ta rishkruajë.
ddl-withheld-strands-other-trigger = { $owner } emërton { $moved }. Një rindërtim e riemërton këtë tabelë nën atë trigger dhe e lë SQL-në e tij saktësisht siç ishte, kështu trigger-i do të vazhdonte të emërtonte një kolonë që nuk ekziston më — SQLite e pranon një trigger të tillë dhe pastaj refuzon çdo shkrim në *atë* tabelë. Fshini ose redaktoni më parë trigger-in, ose riemërtojeni kolonën pa asnjë ndryshim tjetër, që ALTER TABLE i vetë SQLite ta rishkruajë.
ddl-withheld-trigger-on = Trigger-i { $name } në { $table }
ddl-withheld-some-trigger = Një trigger në një tabelë tjetër
ddl-withheld-strands-generated-column = Kolona e gjeneruar { $column } llogaritet nga { $moved }. Një rindërtim e shkruan tabelën nga modeli, dhe asgjë nuk e rishkruan atë shprehje — SQLite e refuzon tabelën e re me “no such column” dhe i gjithë plani rikthehet. Redaktoni ose hiqni më parë { $column }.
ddl-withheld-cannot-restate = Deklarata e kësaj tabele mbart { $clauses }, për të cilat modeli i Schemaic nuk ka fushë. Një rindërtim e shkruan tabelën sërish nga ai model, kështu klauzola do të zhdukej nga një tabelë që ende do të dukej e saktë — dhe çdo shkrim që varet prej saj do të sillej ndryshe që atëherë e tutje. Ndryshojeni këtë tabelë me një skript (“Kopjo DDL” është fillimi i një të tilli), ose hiqni më parë klauzolën.
ddl-sqlite-clauses = { $first } dhe { $last }
ddl-sqlite-clause-deferrable = klauzolën DEFERRABLE të një çelësi të jashtëm
ddl-sqlite-clause-on-conflict = një klauzolë ON CONFLICT
ddl-sqlite-clause-desc-key = një kolonë DESC të çelësit primar
ddl-sqlite-clause-collate-key = një COLLATE mbi një kolonë të çelësit primar
ddl-withheld-virtual-table = `{ $name }` është tabelë virtuale. Kolonat e saj janë ndërfaqja e një moduli dhe jo një deklaratë ruajtjeje, dhe të dhënat e saj të vërteta jetojnë në tabela hije që një rindërtim do t’i fshinte — një indeks FTS5 do të zhdukej nga një tabelë që ende do të mbante tekstin. Ndryshojeni me një skript (DROP dhe CREATE VIRTUAL TABLE), ose redaktoni tabelat që ajo indekson.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } pamje
   *[other] { $count } pamje
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } rutinë
   *[other] { $count } rutina
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } trigger
   *[other] { $count } trigger-ë
}
ddl-used-by-count-event = { $count ->
    [one] { $count } ngjarje
   *[other] { $count } ngjarje
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } çelës i jashtëm
   *[other] { $count } çelësa të jashtëm
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } shprehje tabele
   *[other] { $count } shprehje tabelash
}
ddl-used-by-counts = { $first } dhe { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}dhe { $more } të tjera
    }
ddl-used-by-drop = Përdoret nga { $listing }.
ddl-used-by-change = { $subject } përdoret nga { $listing }. { $then ->
        [renamed] Ato emërtojnë ende { $name } pas riemërtimit.
        [dropped] Ato prishen kur fshihet.
       *[retyped] Kontrollojini kundrejt tipit të tij të ri.
    }
ddl-used-by-column = Kolona { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Çelësat e jashtëm ndërmjet këtyre tabelave formojnë një cikël, kështu asnjë renditje krijimi nuk i plotëson të gjithë. Një urdhër do të refuzohet sepse i referohet një tabele që ende nuk ekziston, dhe as MySQL, as MariaDB nuk e rikthejnë DDL-në.
ddl-risk-clash = { $holder } mban ende { $names }, që i duhen krijimit të { $claimer } — { $why ->
        [not-dropped] dhe ky plan nuk e fshin
        [default] dhe nuk mund të fshihet më parë sepse vlera e paracaktuar e një kolone merr prej saj
       *[referenced] dhe nuk mund të fshihet më parë sepse një tabelë tjetër i referohet
    }. { $cost ->
        [no-rollback] Një urdhër do të refuzohet, dhe as MySQL, as MariaDB nuk e rikthejnë DDL-në.
        [rolls-back] Një urdhër do të refuzohet, gjë që e rikthen të gjithë migrimin.
       *[refused] Një urdhër do të refuzohet.
    }

preview-title = Zbato ndryshimet në { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } ndryshim
   *[other] { $count } ndryshime
}
preview-used-by = Përdoret nga
preview-withheld = Ky motor nuk mund të shprehë një pjesë të këtij plani
preview-withheld-hint = Nuk zbatohet asgjë ndërsa kjo është në listë.
preview-omitted = Nuk përfshihen në këtë migrim
preview-omitted-hint = Zbatimi i lë këto objekte siç janë.
preview-copy = Kopjo
preview-open-in-editor = Hap në redaktor
preview-back = Prapa
preview-apply = Zbato
preview-applying = Po zbatohet…
preview-applied = { $count ->
    [one] U zbatua { $count } urdhër në { $subject }.
   *[other] U zbatuan { $count } urdhra në { $subject }.
}
preview-refreshed = Skema u rifreskua.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Importo lidhje
conn-import-url = URL e lidhjes
conn-import-add = Shto
conn-import-url-hint = Një URL lidhjeje, një URL JDBC ose një rresht DATABASE_URL nga një .env — një për rresht.
conn-import-choose-file = Zgjidhni një skedar…
conn-import-scan = Skano klientët e instaluar
conn-import-scanning = Po skanohet…
conn-import-docker = Kontejnerët Docker
conn-import-asking-docker = Po pyetet Docker…
conn-import-looking = Po kërkohen lidhje në këtë kompjuter…
conn-import-none-found = Nuk u gjetën lidhje. DBeaver, IDE-të e JetBrains dhe klientët e linjës së komandave i mbajnë skedarët e tyre në vende të njohura; nëse i juaji është diku tjetër, zgjidheni më sipër.
conn-import-nothing-yet = Ende nuk u gjet asgjë — ngjitni një URL, lexoni një skedar, ose skanoni për klientë që përdorni tashmë.
conn-import-found = { $count ->
    [one] U gjet { $count } lidhje
   *[other] U gjetën { $count } lidhje
}
conn-import-select-all = Përzgjidh të gjitha
conn-import-select-none = Asnjë
conn-import-note-no-password = Asnjë fjalëkalim në burim
conn-import-note-already-saved = E ruajtur tashmë
conn-import-note-unexpanded-path = Shtegu përmban një makro të pazgjeruar
conn-import-note-pgpass = Fjalëkalimi u mor nga .pgpass juaj
conn-import-note-port-assumed = Burimi nuk emërtoi port; ky është ai i paracaktuar
conn-import-note-external-login = Hyn në një mënyrë që Schemaic nuk e mbështet: caktoni një login SQL
conn-import-skipped = { $count ->
    [one] { $count } hyrje nuk u importua: { $names }.
   *[other] { $count } hyrje nuk u importuan: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } hyrje nuk u importua: { $names }, dhe { $rest } të tjera.
   *[other] { $count } hyrje nuk u importuan: { $names }, dhe { $rest } të tjera.
}
conn-import-reason-unsupported-engine = motor i pambështetur ({ $driver })
conn-import-reason-no-server = asnjë server për t’u lidhur
conn-import-reason-not-published = porti i tij nuk është publikuar për këtë kompjuter
conn-import-reason-unreadable = nuk mund të lexohej ({ $error })
conn-import-reason-not-an-export = nuk mund të lexohej (nuk është eksport lidhjesh i Schemaic: { $error })
conn-import-reason-newer-export = nuk mund të lexohej (eksportuar nga një Schemaic më i ri (versioni { $version }) — përditësojeni për ta importuar)
conn-import-url-empty = Ngjitni një URL lidhjeje.
conn-import-url-no-scheme = Asnjë skemë — një URL fillon me mysql://, postgresql://, sqlserver:// ose sqlite:, një varg lidhjeje SQL Server me Server=.
conn-import-url-unknown-scheme = Skemë e panjohur “{ $scheme }” — pritej mysql, mariadb, postgresql, sqlserver ose sqlite.
conn-import-url-no-host = Asnjë host në URL.
conn-import-url-bad-port = “{ $port }” nuk është numër porti.
conn-import-url-no-path = Asnjë skedar baze të dhënash pas sqlite:.
conn-import-url-transport = “{ $server }” arrihet përmes named pipes ose LocalDB — Schemaic lidhet vetëm përmes TCP.
conn-import-url-userinfo = Emri i serverit përmban një @ — një emër përdoruesi ose fjalëkalim shkon në fushën e vet, jo te serveri.
conn-import-url-not-sql-server = Ky varg lidhjeje nuk thotë se është i SQL Server — drajverët .NET të MySQL dhe PostgreSQL shkruajnë të njëjtat fjalë kyçe. Ngjitni në vend të tij URL-në e serverit (mysql://, postgresql://, sqlserver://), ose, për SQL Server, shkruajeni serverin si tcp:host.
conn-import-row-overflow = Po shfaqen { $shown } të parat nga { $total }. { $hidden } të tjerat importohen gjithsesi nëse janë të shënuara — kjo listë ndalet këtu që dritarja të mbetet e shpejtë.
conn-import-import = Importo
conn-import-import-count = Importo { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = Imazh PNG
blob-kind-jpeg = Imazh JPEG
blob-kind-gif = Imazh GIF
blob-kind-bmp = Imazh BMP
blob-kind-webp = Imazh WebP
blob-kind-ico = Ikonë
blob-kind-opaque = Të dhëna binare
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · po shfaqen { $shown } e para
blob-pane-preview = Parapamja
blob-pane-hex = Hex
blob-reading = Po lexohet…
blob-empty = Kjo qelizë është NULL, ose rreshti i saj nuk ekziston më.
blob-preview-too-large = Ky imazh është { $width } × { $height } — shumë i madh për parapamje këtu. Bajtet janë të paprekura: lexojini si Hex, ose ruajini në një skedar.
blob-preview-truncated = U lexuan vetëm { $shown } e para të { $key ->
    [png] këtij imazhi PNG
    [jpeg] këtij imazhi JPEG
    [gif] këtij imazhi GIF
    [bmp] këtij imazhi BMP
    [webp] këtij imazhi WebP
    [ico] kësaj ikone
   *[other] kësaj vlere
}, kështu nuk mund të vizatohet. Lexojini bajtet si Hex.
blob-preview-unreadable = Këto bajte fillojnë si { $key ->
    [png] një imazh PNG
    [jpeg] një imazh JPEG
    [gif] një imazh GIF
    [bmp] një imazh BMP
    [webp] një imazh WebP
    [ico] një ikonë
   *[other] një imazh
}, por nuk mund të lexohen si i tillë. Lexojini si Hex, ose ruajini në një skedar.
blob-save-truncated = Mund të lexoheshin vetëm { $shown } e para nga { $size }, kështu kjo do të ruante vetëm pjesën e parë të vlerës.
blob-load = Ngarko nga skedari
blob-save = Ruaj në skedar
blob-load-dialog = Ngarko vlerën binare nga skedari
blob-save-dialog = Ruaj vlerën binare
blob-saved-to = U ruajt në { $path }
blob-loaded-file = Skedari u ngarkua. Konfirmojeni në rrjetë për ta shkruar.
blob-not-writable = Kjo qelizë nuk mund të shkruhet.
blob-file-too-large = Ai skedar është { $got } — kjo kolonë mban maksimumi { $most }.
blob-not-accepting = Ajo qelizë nuk pranon më vlerë — nuk u përgatit asgjë.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = Diagrami ER
erd-scope = Shtrirja
erd-chip-tables = { $count ->
    [one] { $count } tabelë
   *[other] { $count } tabela
}
erd-chip-relationships = { $count ->
    [one] { $count } marrëdhënie
   *[other] { $count } marrëdhënie
}
erd-chip-hidden = { $count } të palidhura të fshehura
erd-find-placeholder = Gjej tabelë ose kolonë
erd-find-none = Asnjë përputhje
erd-find-matches = { $hits ->
    [one] { $hits } përputhje
   *[other] { $hits } përputhje
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } përputhje në { $tables } tabela
   *[other] { $hits } përputhje në { $tables } tabela
}
erd-more-columns = +{ $count } të tjera
erd-show-less = shfaq më pak
erd-not-loaded = Skema ende nuk është ngarkuar për këtë bazë të dhënash.
erd-no-relationships = Nuk ka marrëdhënie çelësash të jashtëm për diagram.
erd-export-tip = Eksporto diagramin…
erd-copy-as = Kopjo si
erd-export-dialog = Eksporto diagramin
erd-nothing-to-export = Nuk ka asgjë për të eksportuar.
erd-nothing-to-copy = Nuk ka asgjë për të kopjuar.
erd-copied-as = U kopjua si { $format }
erd-clipboard-unavailable = Kujtesa nuk është e disponueshme.
erd-copy-failed = Nuk u kopjua dot: { $error }
erd-saved = U ruajt { $name }
erd-unnamed-file = skedari

## Server variables modal (server_vars_view.rs)

server-vars-title = Variablat e serverit — { $connection }
server-vars-search = Kërko cilësime
server-vars-variables = Variablat
server-vars-status = Statusi
server-vars-loading = Po ngarkohet…
server-vars-unsupported = Motori i kësaj lidhjeje nuk publikon listë të këtij lloji.
server-vars-no-match = Asnjë cilësim që përputhet.
server-vars-more = { $count ->
    [one] Edhe { $count } cilësim përputhet — ngushtoni kërkimin
   *[other] Edhe { $count } cilësime përputhen — ngushtoni kërkimin
}
server-vars-count = { $count ->
    [one] { $count } cilësim
   *[other] { $count } cilësime
}
server-vars-count-filtered = { $shown } nga { $count } cilësime
server-vars-refresh = Rifresko

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Redakto fragmentin — { $name }
snippet-edit-name = Emri
snippet-edit-name-placeholder = Emri i fragmentit
snippet-edit-shortcut = Shkurtorja e zgjerimit
snippet-edit-shortcut-none = asnjë
snippet-edit-query = Kërkesa
snippet-edit-save = Ruaj

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Plani i kërkesës
plan-analyze-unavailable = Analyze nuk ofrohet (urdhri shkruan të dhëna)
plan-analyze-hint = Ekzekuton urdhrin për të matur kohët reale
plan-explaining = Po shpjegohet
plan-potential-issues = Probleme të mundshme
plan-detail-no-index = nuk përdoret asnjë indeks
plan-detail-key-unused = indeksi nuk përdoret për kërkim
plan-my-full-scan = Skanim i plotë i tabelës `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Skanim i plotë i rezultatit ({ $detail }, type = ALL)
plan-my-filesort = Filesort mbi `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort mbi rezultatin (Extra: Using filesort)
plan-my-temporary = Tabelë e përkohshme për `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Tabelë e përkohshme për rezultatin (Extra: Using temporary)
plan-pg-seq-scan = Skanim sekuencial i `{ $table }` (nuk përdoret indeks)
plan-pg-seq-scan-unnamed = Skanim sekuencial i një tabele (nuk përdoret indeks)
plan-pg-sort = Nyje Sort — kërkesa i rendit rreshtat në çast
plan-ms-full-scan = Skanim i plotë i `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Skanim i plotë i një tabele ({ $op })
plan-ms-sort = Sort — kërkesa i rendit rreshtat në çast
plan-ms-spool = { $op } — një tabelë pune e ndërmjetme në tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = asnjë kusht join-i — çdo rresht çiftohet me çdo rresht
plan-server-no-statistics = një kolonë që lexon nuk ka statistika
plan-server-spill = u derdh në tempdb — memoria e caktuar ishte shumë e vogël
plan-server-convert = një konvertim tipi ndikon në plan ({ $issue }): { $expression }
plan-server-unmatched-indexes = një indeks i filtruar nuk mund të përdorej
plan-server-memory-grant = memoria e caktuar u vlerësua gabim
plan-server-wait = priti për { $wait }
plan-server-other = serveri paralajmëron: { $name }
plan-missing-index = Mungon indeksi mbi `{ $table }` ({ $keys })
plan-missing-index-include = Mungon indeksi mbi `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Mungon indeksi mbi `{ $table }` ({ $keys }) — serveri priste një përmirësim prej { $impact }%
plan-missing-index-include-impact = Mungon indeksi mbi `{ $table }` ({ $keys }; include { $include }) — serveri priste një përmirësim prej { $impact }%
plan-ask-ai = Pyet AI-në
plan-ms-operation = Operacioni
plan-ms-object = Objekti
plan-ms-estimated-rows = Rreshta të vlerësuar
plan-ms-estimated-cost = Kosto e vlerësuar
plan-ms-actual-rows = Rreshta realë
plan-ms-executions = Ekzekutime
plan-ms-statement = Urdhri { $n }: { $kind }
plan-ms-statement-in-procedure = Urdhri { $n }: { $kind } në procedurën { $name }
plan-ms-statement-in-function = Urdhri { $n }: { $kind } në funksionin { $name }
plan-ms-statement-in-a-procedure = Urdhri { $n }: { $kind } në një procedurë
plan-ms-statement-in-a-function = Urdhri { $n }: { $kind } në një funksion
plan-ms-plan = Plani
plan-ms-no-plan = Asnjë urdhër në këtë grup nuk ka plan.
plan-ms-unread = Plani nuk mund të lexohej: { $reason }
plan-ms-unread-not-xml = nuk është dokument XML
plan-ms-unread-not-showplan = pritej ShowPlanXML, u gjet { $found }
plan-ms-unread-empty = një dokument bosh

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Përdoret nga — { $object }
used-by-copy = Kopjo
used-by-not-loaded = Skema e { $database } ende nuk është ngarkuar — zgjerojeni në pemë, pastaj pyesni sërish.
used-by-nothing-in = Asgjë në { $database } nuk e përdor.
used-by-nothing = Asgjë nuk e përdor.
used-by-limits = Gjetur duke lexuar përkufizimet në këtë bazë të dhënash: SQL-ja e ndërtuar gjatë ekzekutimit dhe bazat e tjera të të dhënave nuk shihen.
used-by-dropped-with-it = { $detail } · fshihet bashkë me të
used-by-heading-view = { $count ->
    [one] Pamje ({ $count })
   *[other] Pamje ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Rutinë ({ $count })
   *[other] Rutina ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Trigger-ë ({ $count })
}
used-by-heading-event = { $count ->
    [one] Ngjarje ({ $count })
   *[other] Ngjarje ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Çelës i jashtëm ({ $count })
   *[other] Çelësa të jashtëm ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Shprehje tabele ({ $count })
   *[other] Shprehje tabelash ({ $count })
}
used-by-noun-view = pamje
used-by-noun-routine = rutinë
used-by-noun-trigger = trigger
used-by-noun-event = ngjarje
used-by-noun-foreign-key = çelës i jashtëm
used-by-noun-expression = shprehje tabele

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = E errët
theme-light = E çelët

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = KOD
md-code-copy = Kopjo
md-code-insert = Fut
md-code-run = Ekzekuto
md-proposal-title = Ndryshim i propozuar për { $table }
md-proposal-count = { $count ->
    [one] { $count } ndryshim
   *[other] { $count } ndryshime
}
md-proposal-review = Shqyrto ndryshimin…
md-proposal-unreadable = Asistenti propozoi një ndryshim që nuk mund të lexohej — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Zgjidhni…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Tani
cell-calendar-today = Sot
cell-month-1 = Janar
cell-month-2 = Shkurt
cell-month-3 = Mars
cell-month-4 = Prill
cell-month-5 = Maj
cell-month-6 = Qershor
cell-month-7 = Korrik
cell-month-8 = Gusht
cell-month-9 = Shtator
cell-month-10 = Tetor
cell-month-11 = Nëntor
cell-month-12 = Dhjetor
cell-weekday-mon = Hë
cell-weekday-tue = Ma
cell-weekday-wed = Më
cell-weekday-thu = En
cell-weekday-fri = Pr
cell-weekday-sat = Sh
cell-weekday-sun = Di

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Riemërto pseudonimin
alias-rename-title-cte = Riemërto CTE-në
alias-rename-placeholder-alias = Pseudonimi
alias-rename-placeholder-cte = Emri i CTE-së
alias-rename-new-name = Emri i ri
alias-rename-rename = Riemërto

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Këtu nuk ka më asgjë për t’u riemërtuar.
alias-rename-empty = Jepini një emër.
alias-rename-conflict = “{ $name }” është tashmë një emër në këtë kërkesë: riemërtimi me të do të ndryshonte se cilën tabelë lexon një referencë.
alias-rename-unsure = Një referencë këtu mund të lexojë një kolonë tjetër me këtë emër, ose të lexojë këtë aty ku riemërtimi nuk mund ta ndjekë, dhe skema e ngarkuar nuk mund ta dallojë — kështu nuk bëhet asnjë redaktim. Ngarkoni skemën e tabelave, ose riemërtojeni me dorë.
alias-rename-changed = Kërkesa ndryshoi ndërsa kjo ishte e hapur. Mbylleni dhe riemërtojeni sërish.
alias-rename-read-only = Redaktori është vetëm për lexim derisa sugjerimi i AI të pranohet ose të refuzohet.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } përdorim i { $name }
   *[other] { $count } përdorime të { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = SVG i pavlefshëm: { $error }
raster-too-large = Imazhi është shumë i madh për t’u rasterizuar ({ $width }×{ $height })
raster-png-failed = Kodimi PNG dështoi: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Kopjo
grid-menu-copy-value = Kopjo vlerën
grid-menu-copy-formatted = Kopjo të formatuar
grid-menu-copy-as = Kopjo si
grid-menu-paste = Ngjit
grid-menu-fill-down = Mbush poshtë
grid-menu-fill-up = Mbush lart
grid-menu-edit-field = Redakto fushën
grid-menu-edit-row = Redakto rreshtin
grid-menu-view-row = Shiko rreshtin
grid-menu-edit-binary = Redakto vlerën binare
grid-menu-view-binary = Shiko vlerën binare
grid-menu-filter-by-value = Filtro sipas kësaj vlere
grid-menu-exclude-value = Përjashto këtë vlerë
grid-menu-follow-relation = Ndiq marrëdhënien
grid-menu-referenced-by = Referohet nga
grid-menu-set-null = Cakto në NULL
grid-menu-set-default = Cakto në DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Dyfisho rreshtin
   *[other] Dyfisho { $count } rreshta
}
grid-menu-delete-rows = { $count ->
    [one] Fshi rreshtin
   *[other] Fshi { $count } rreshta
}
grid-menu-undo-delete = Zhbëj fshirjen
grid-menu-ai-summary = Përmbledhje me AI
grid-attach-to-chat = Bashkëngjit { $scope } në bisedë
grid-attach-scope-rows = { $rows ->
    [one] { $rows } rresht
   *[other] { $rows } rreshta
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } kolonë
   *[other] { $cols } kolona
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } rresht
   *[other] { $rows } rreshta
} dhe { $cols ->
    [one] { $cols } kolonë
   *[other] { $cols } kolona
}
grid-menu-freeze = Ngri
grid-menu-unfreeze = Shkri
grid-menu-hide-column = Fshih kolonën
grid-menu-show-hidden-columns = Shfaq kolonat e fshehura ({ $count })
grid-menu-show-all = Shfaq të gjitha
grid-menu-format-as = Formato si
grid-format-none = Asnjë
grid-format-timestamp = Vulë kohore
grid-format-grouped = Grupim shifrash
grid-format-bytes = Madhësi skedari
grid-format-bool = Boolean
grid-menu-copy-name = Kopjo emrin
grid-menu-copy-qualified-name = Kopjo emrin e plotë
grid-menu-copy-values = Kopjo vlerat
grid-menu-sql-in-list = Listë SQL IN

## The results grid's toolbar (grid.rs) — the status line, the commit and row
## controls, and the copy, save, AI and chart icons.

grid-stats = { $rows } · { $cols ->
    [one] { $cols } kol.
   *[other] { $cols } kol.
} · { $ms } ms
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } kol.
   *[other] { $cols } kol.
} · { $ms } ms
grid-rows-read = { $n ->
    [one] { $read } rresht
   *[other] { $read } rreshta
}
grid-rows-read-capped = { $n ->
    [one] { $read } rresht
   *[other] { $read } rreshta
} (të kufizuar)
grid-rows-read-of-total = { $n ->
    [one] { $read } nga { $total } rresht
   *[other] { $read } nga { $total } rreshta
}
grid-kept-note = · e mbajtur — vetëm për lexim
grid-arena-note = · { $columns } shumë të mëdha për t’u mbajtur të plota — rreshtat e mëvonshëm shfaqen bosh
grid-sorted-subset = · nënbashkësi e renditur (e kufizuar) — jo renditja e plotë
grid-read-all = lexo të gjithë rreshtat
grid-read-more = lexo { $rows } rreshta
grid-reading = po lexohet…
grid-committing = Po konfirmohet…
grid-commit-tip = { $count ->
    [one] Konfirmo { $count } ndryshim ({ $keys })
   *[other] Konfirmo { $count } ndryshime ({ $keys })
}
grid-discard-tip = Hidh poshtë të gjitha ndryshimet në pritje
grid-copy-sql-tip = Kopjo ndryshimet si SQL
grid-add-row-tip = Shto një rresht
grid-delete-row-tip = Shëno rreshtin e përzgjedhur për fshirje ({ $key })
grid-clone-row-tip = Klono rreshtin e përzgjedhur
grid-copy-results-tip = Kopjo rezultatet…
grid-copy-results-empty = Nuk ka asgjë për të kopjuar — nuk ka rreshta
grid-save-results-tip = Ruaj rezultatet në një skedar…
grid-save-results-empty = Nuk ka asgjë për të ruajtur — nuk ka rreshta
grid-ai-tip = Gjenero rreshta me AI…
grid-ai-generating = Po gjenerohet…
grid-ai-off = Veprimet AI dërgojnë të dhëna — joaktive për këtë lidhje
grid-ai-fill-value = Plotëso vlerën me AI
grid-ai-insert-row = Fut rresht me AI
grid-ai-seed-table = Mbush tabelën me AI
grid-chart-tip = Bëj grafik të rezultateve
grid-chart-empty = Nuk ka asgjë për grafik — nuk ka rreshta
grid-chart-this-result = ky rezultat

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Shiko
grid-rollback-tab = Rikthe { $tab }
grid-find-placeholder = Gjej në rezultate
grid-figure-copied = U kopjua
grid-figure-rows = { $n ->
    [one] { $n }{ $more } rresht
   *[other] { $n }{ $more } rreshta
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } të dallueshme
grid-figure-sum = Shuma { $value }
grid-figure-avg = Mesatarja { $value }
grid-figure-median = Mediana { $value }
grid-figure-min = Min { $value }
grid-figure-max = Maks { $value }
grid-figure-length = { $n ->
    [one] Gjatësia { $n } karakter
   *[other] Gjatësia { $n } karaktere
}
grid-figure-length-range = Gjatësia { $lo }–{ $hi } karaktere
grid-goto-label = Shko te rreshti:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = kujtesa
export-dest-file = skedari
export-note-rows = { $n ->
    [one] U eksportua { $rows } rresht në { $name }
   *[other] U eksportuan { $rows } rreshta në { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } mban bajte që ky format nuk mund t’i bartë: ato qeliza janë bosh
   *[other] { $cols } mbajnë bajte që ky format nuk mund t’i bartë: ato qeliza janë bosh
}
export-note-blanked = { $cols } shumë të mëdha për t’u mbajtur të plota: rreshtat e mëvonshëm janë bosh
export-note-cut = { $cols } u shkurtuan te kufiri i qelizës së Excel prej { $limit } karakteresh
export-failure-partial = { $message } — { $name } nuk u ndryshua; rreshtat që u shkruan janë në { $part }
export-cancelled-partial = Eksporti u anulua — { $name } nuk u ndryshua; rreshtat që u shkruan janë në { $part }
export-cancelled-nothing = Eksporti u anulua — { $name } nuk u ndryshua.
copy-sql-as-null = { $n ->
    [one] U kopjua në kujtesë — { $cols } mban bajte që teksti SQL nuk mund t’i bartë: shkruar si NULL
   *[other] U kopjua në kujtesë — { $cols } mbajnë bajte që teksti SQL nuk mund t’i bartë: shkruar si NULL
}
copy-sql-nothing = { $n ->
    [one] Nuk u kopjua asgjë — { $cols } mban bajte që teksti SQL nuk mund t’i bartë
   *[other] Nuk u kopjua asgjë — { $cols } mbajnë bajte që teksti SQL nuk mund t’i bartë
}
copy-sql-left-out = { $n ->
    [one] U kopjua në kujtesë — { $cols } mban bajte që teksti SQL nuk mund t’i bartë: ato vlera lihen jashtë
   *[other] U kopjua në kujtesë — { $cols } mbajnë bajte që teksti SQL nuk mund t’i bartë: ato vlera lihen jashtë
}

grid-fill-nothing = Nuk ka asgjë për të mbushur: kolonat e përzgjedhura nuk mund të shkruhen, ose qeliza për t’u kopjuar është vlera e pacaktuar e një rreshti të ri ose një DEFAULT në pritje.
grid-paste-outside = { $n } jashtë rrjetës
grid-paste-read-only = { $n } në kolona vetëm për lexim
grid-paste-capped = { $n } mbi kufirin e ngjitjes prej { $cap } qelizash
grid-paste-deleted = { $n } në rreshta të shënuar për fshirje
grid-paste-nothing = Nuk u ngjit asgjë: { $notes }.
grid-paste-partial = { $count ->
        [one] U ngjit { $cells } qelizë, duke kapërcyer { $notes }.
       *[other] U ngjitën { $cells } qeliza, duke kapërcyer { $notes }.
    }
grid-row-not-editable = Kolona `{ $column }` nuk mund të redaktohet.
grid-row-not-nullable = Kolona `{ $column }` nuk mund të jetë NULL.
grid-fill-skipped = { $count ->
    [one] U mbush { $filled } qelizë, duke kapërcyer { $skipped } në rreshta të shënuar për fshirje.
   *[other] U mbushën { $filled } qeliza, duke kapërcyer { $skipped } në rreshta të shënuar për fshirje.
}
grid-attach-summary = { $rows ->
    [one] { $rows } rresht
   *[other] { $rows } rreshta
} × { $cols ->
    [one] { $cols } kolonë
   *[other] { $cols } kolona
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } rresht
   *[other] { $rows } rreshta
} × { $cols ->
    [one] { $cols } kolonë
   *[other] { $cols } kolona
} nga { $table }
grid-attach-summary-capped = { $summary } ({ $taken } të parat nga { $total } të përzgjedhura)
grid-copy-sql-unset = Nuk ka asgjë për të kopjuar si SQL — qelizat e përzgjedhura të rreshtit të ri ende nuk kanë vlerë dhe marrin vlerën e paracaktuar të serverit.
grid-copy-sql-joined = Nuk ka asgjë për të kopjuar si SQL: këta rreshta vijnë nga një kërkesë mbi më shumë se një tabelë.
grid-copy-sql-no-keyed-table = Nuk ka asgjë për të kopjuar si SQL: rreshtat duhet të vijnë nga një tabelë e vetme me çelës.
grid-copy-sql-key-too-large = Nuk ka asgjë për të kopjuar si SQL: çelësi ishte shumë i madh që ky rezultat ta mbante të plotë, kështu disa rreshta nuk mund të emërtohen me të.
grid-copy-sql-pinned = Nuk ka asgjë për të kopjuar si SQL: ky rezultat është i fiksuar, kështu rreshtat e tij janë një pamje e çastit.
grid-copy-sql-read-only = Nuk ka asgjë për të kopjuar si SQL: kjo lidhje është vetëm për lexim.
grid-copy-update-too-large = Nuk ka asgjë për të kopjuar si UPDATE: kolonat e përzgjedhura ishin shumë të mëdha që ky rezultat t’i mbante të plota, dhe disa nga qelizat e tyre shfaqen bosh.
grid-copy-update-nothing-writable = Nuk ka asgjë për të kopjuar si UPDATE: asnjë nga kolonat e përzgjedhura nuk mund të shkruhet.
grid-copy-where-in-no-key = Nuk ka asgjë për të kopjuar si WHERE IN: kjo tabelë nuk ka çelës primar ose unik me të cilin të emërtohen rreshtat.
grid-copy-new-row-no-key = Nuk ka asgjë për të kopjuar: një rresht i ri nuk ka çelës derisa të ruhet.
grid-export-fetched-rows = Rreshtat e lexuar ({ $count })
grid-export-dialog-title = Eksporto rezultatet
grid-export-already-running = Një eksport është tashmë në ekzekutim — nuk u shkrua asgjë.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Rreshti { $row }
grid-row-title-in-table = Rreshti { $row }  ·  { $table }
grid-row-set-value = Cakto vlerën
grid-row-set-null = Cakto NULL
grid-row-edit-binary = Redakto
grid-row-view-binary = Shiko
grid-row-invalid-json = JSON i pavlefshëm: { $error }
grid-row-saving = Po ruhet

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } rresht i prekur · { $ms } ms
   *[other] { $count } rreshta të prekur · { $ms } ms
}
grid-rerun-would-write = Ky urdhër nuk mund të riekzekutohet — do të shkruante në bazën e të dhënave
grid-filter-not-simple = Kjo kërkesë nuk mund të filtrohet — nuk është një SELECT i thjeshtë mbi një tabelë
grid-filter-bracket-in-name = një emër që përmban `]` nuk mund të filtrohet këtu
grid-filter-trailing-input = hyrje e papritur pas kushtit
grid-staged-requery = Konfirmoni ose hidhni poshtë më parë ndryshimet tuaja — rileximi i rreshtave do t’i humbiste
grid-staged-landing = Ndryshimet u përgatitën ndërsa tabela po rilexohej, kështu u mbajtën dhe tabela nuk u rifreskua — konfirmojini ose hidhini poshtë, pastaj rifreskoni.
grid-reread-without-filter = Filtri thërret një funksion që Schemaic nuk e riekzekuton pa u pyetur, kështu pas konfirmimit tabela u rilexua pa filtrin dhe renditjen.
grid-hidden-columns-tip = { $count ->
    [one] { $count } kolonë e fshehur — klikoni për t’i shfaqur të gjitha, ose klikoni me të djathtën një kokë kolone për të zgjedhur
   *[other] { $count } kolona të fshehura — klikoni për t’i shfaqur të gjitha, ose klikoni me të djathtën një kokë kolone për të zgjedhur
}
grid-copied-staged-sql = Ndryshimet në pritje u kopjuan si SQL.
grid-copy-split = { $n ->
        [one] { $n } qelizë e kopjuar përmban një tabulim ose një ndërprerje rreshti — një ngjitje do ta lexojë si më shumë se një qelizë.
       *[other] { $n } qeliza të kopjuara përmbajnë një tabulim ose një ndërprerje rreshti — një ngjitje do ta lexojë secilën prej tyre si më shumë se një qelizë.
    }
grid-duplicate-title = Dyfisho rreshtat
grid-duplicate-question = { $n ->
    [one] Të dyfishohet { $rows } rresht? Ai përgatitet si rresht në pritje dhe nuk shkruhet asgjë derisa të konfirmoni.
   *[other] Të dyfishohen { $rows } rreshta? Ata përgatiten si rreshta në pritje dhe nuk shkruhet asgjë derisa të konfirmoni.
}
grid-nullable-tip = Pranon NULL
grid-cell-auto = <automatik>
grid-cell-required = <i detyrueshëm>
grid-seed-title = Mbush me rreshta
grid-seed-generate = Gjenero

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Skema
footer-panel-ai = Asistenti AI
footer-panel-history = Historiku i kërkesave
footer-panel-snippets = Biblioteka e fragmenteve
footer-panel-activity = Aktiviteti i serverit
footer-panel-terminal = Terminali
footer-panel-narrow = Dritarja është shumë e ngushtë për këtë panel — zgjerojeni për ta rikthyer panelin.
footer-cursor = Rr { $line }, Kol { $col }
footer-indent-spaces = Hapësira: { $width }
footer-indent-tabs = Tabulime: { $width }
footer-spaces = Hapësira
footer-tabs = Tabulime
footer-wrap = Mbështjellje
footer-no-wrap = Pa mbështjellje
footer-read-only = Vetëm për lexim
footer-write-mode = Mënyra e shkrimit
footer-tx-auto = Konfirmim automatik
footer-tx-manual = Manual
footer-tx-open = Të hapur: { $count }
footer-tx-aborted = Transaksioni u ndërpre — riktheni për të vazhduar
footer-tx-lost = Transaksioni humbi
footer-commit = Konfirmo
footer-rollback = Rikthe
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Kërkesa { $n }
lib-result-default-label = Rezultati { $n }
lib-ai-off-note = Asistenti AI është joaktiv. Zgjidhni një CLI agjenti te cilësimet e AI për ta aktivizuar.
lib-test-testing = Po testohet…
lib-test-connected = U lidh
lib-test-failed = Lidhja dështoi

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Minimale
lib-effort-low = E ulët
lib-effort-medium = Mesatare
lib-effort-high = E lartë
lib-effort-extra = Ekstra
lib-effort-max = Maksimale
lib-schema-scope-active = Vetëm baza aktive e të dhënave
lib-schema-scope-all = Të gjitha bazat e të dhënave
lib-schema-scope-none = Asnjë
lib-cursor-block = Bllok
lib-cursor-bar = Vijë vertikale
lib-cursor-underline = Nënvijë

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Asnjë lidhje
header-new-connection = Lidhje e re
header-find-anywhere-tip = Gjej kudo ({ $keys })
header-shortcuts-tip = Shkurtoret e tastierës
header-settings-tip = Cilësimet
header-disconnected = I shkëputur
header-retry = Provo sërish
colour-red = E kuqe
colour-orange = Portokalli
colour-amber = Qelibar
colour-green = E gjelbër
colour-teal = Bruz
colour-blue = Blu
colour-purple = Vjollcë
colour-pink = Rozë

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = “{ $name }” ndryshoi në disk.
disk-reload = Ringarko
disk-keep-mine = Mbaj timen
results-run-hint = Ekzekutoni një kërkesë  ({ $keys })
results-failed = Urdhri dështoi.
results-cancelled = Kërkesa u anulua.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = { $count } të fiksuara · { $size }
results-pinned-tip = Rezultatet e fiksuara mbahen në memorie derisa t’i mbyllni. Mbyllni ato me të cilat keni mbaruar për ta liruar atë.
results-chip-nothing-run = Ende nuk është ekzekutuar asgjë në këtë skedë.
results-chip-kept = e mbajtur — vetëm për lexim
results-menu-pin = Fikso
results-menu-unpin = Çfikso
results-menu-close = Mbyll
results-menu-compare-with = Krahaso me { $result }
results-menu-close-others = Mbyll rezultatet e tjera
results-menu-close-all = Mbyll të gjitha rezultatet

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Kërko kudo

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Nuk ka asgjë për të zgjedhur

widgets-spinner-verbs = Duke gatuar, Duke menduar, Duke medituar, Duke zier, Duke pjekur, Duke fermentuar, Duke përzier, Duke shtrydhur, Duke gdhendur, Duke thurur, Duke qëndisur, Duke endur, Duke farkëtuar, Duke skicuar, Duke vizatuar, Duke kompozuar, Duke orkestruar, Duke harmonizuar, Duke llogaritur, Duke deshifruar, Duke kristalizuar, Duke distiluar, Duke marinuar, Duke erëzuar, Duke karamelizuar, Duke brumosur, Duke ngjizur, Duke çelur, Duke mbirë, Duke lulëzuar, Duke pluskuar, Duke fluturuar, Duke kërcyer, Duke valëvitur, Duke shëtitur, Duke bredhur, Duke ëndërruar, Duke imagjinuar, Duke improvizuar, Duke filozofuar, Duke përsiatur, Duke bluar, Duke shoshitur, Duke tundur, Duke rrotulluar, Duke vërtitur, Duke kurdisur, Duke trilluar, Duke sajuar, Duke stisur, Duke ndërtuar, Duke punuar, Duke gërmuar, Duke sintetizuar, Duke kalibruar, Duke magjepsur, Duke vezulluar, Duke gumëzhuar, Duke cicëruar, Duke kënduar, Duke vallëzuar, Duke rrëshqitur, Duke lundruar, Duke avulluar, Duke kondensuar, Duke kullotur, Duke mbjellë, Duke vjelë, Duke ngrohur, Duke shkrirë, Duke qepur, Duke arnuar, Duke lëmuar, Duke latuar, Duke mprehur, Duke përpunuar, Duke përtypur, Duke shijuar, Duke nuhatur, Duke vëzhguar, Duke peshuar, Duke numëruar yje, Duke frymëzuar, Duke shpikur, Duke zbuluar, Duke gargarisur, Duke fishkëllyer, Duke pëshpëritur, Duke gugatur, Duke kapërdirë, Duke ngatërruar, Duke shpleksur, Duke zbërthyer, Duke kalitur, Duke tymosur, Duke rrahur vezë, Duke hedhur valle, Duke fryrë flluska, Duke ngjyrosur, Duke lustruar

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Kërkoni ndihmë nga asistenti AI.
cmdk-accept = Prano
cmdk-reject = Refuzo
cmdk-no-changes = Nuk sugjerohen ndryshime
cmdk-diff-summary = { $hunks ->
        [one] { $n } pjesë · { $removed } − / { $added } +
       *[other] { $n } pjesë · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = vlera
params-kind-tip = Tekst, numër, NULL ose SQL i papërpunuar — klikoni për ta ndryshuar
params-missing = Asnjë vlerë për { $names }
params-not-a-number = :{ $name } është numër, por përmban “{ $text }”

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Shiko
editor-run-anyway = Ekzekuto gjithsesi
guard-read-only = Lidhje vetëm për lexim.
guard-read-only-refused = Lidhje vetëm për lexim: { $reason }
read-only-several = lejohet vetëm një urdhër i vetëm
read-only-not-a-read = lejohen vetëm kërkesa vetëm për lexim ({ $heads })
read-only-denied = `{ $word }` nuk lejohet në një kërkesë vetëm për lexim
read-only-advances-sequence = `NEXT VALUE FOR` e çon përpara një sekuencë, gjë që një kërkesë vetëm për lexim nuk e bën
read-only-unlisted-call = `{ $name }` nuk është funksion i njohur si vetëm për lexim, prandaj nuk lejohet në një kërkesë vetëm për lexim; refuzohen edhe funksionet nga zgjerimet ose të përkufizuara në bazën e të dhënave
read-only-locks = `{ $clause }` bllokon rreshtat që lexon, gjë që një kërkesë vetëm për lexim nuk e bën; hiqni klauzolën e bllokimit
guard-no-database = Nuk është zgjedhur bazë të dhënash.
guard-writes-one = Ky urdhër ndryshon të dhëna.
guard-writes-several = Këta urdhra ndryshojnë të dhëna.
guard-script = Të ekzekutohet çdo urdhër në { $file }? Një skript mund të krijojë, ndryshojë dhe fshijë objekte, dhe ajo që ndryshon nuk mund të zhbëhet nga këtu.
guard-staged = { $n ->
    [one] Ky ekzekutim zëvendëson rezultatin që mban { $n } ndryshim në pritje — ekzekutimi do ta hidhte poshtë. Konfirmojeni ose hidheni poshtë më parë, ose ekzekutoni gjithsesi.
   *[other] Ky ekzekutim zëvendëson rezultatin që mban { $n } ndryshime në pritje — ekzekutimi do t’i hidhte poshtë. Konfirmojini ose hidhini poshtë më parë, ose ekzekutoni gjithsesi.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL heq çdo rresht në tabelë.
guard-truncate = TRUNCATE heq çdo rresht në tabelë.
guard-no-where = U zbulua një urdhër { $kind } pa klauzolë WHERE.
guard-drop-table = DROP TABLE fshin tabelën dhe çdo rresht në të.
guard-drop-database = DROP DATABASE fshin bazën e të dhënave dhe çdo tabelë në të.
guard-drop-schema = DROP SCHEMA fshin skemën dhe çdo tabelë në të.
guard-drop-owned = DROP OWNED fshin gjithçka që zotëron roli, çdo tabelë bashkë me rreshtat e saj.
guard-drop-cascade = DROP { $obj } … CASCADE fshin çdo kolonë që varet prej tij, bashkë me vlerat e saj.
guard-replace-table = CREATE OR REPLACE TABLE fshin tabelën ekzistuese dhe çdo rresht në të.
guard-replace-database = CREATE OR REPLACE DATABASE fshin bazën ekzistuese të të dhënave dhe çdo tabelë në të.
guard-replace-schema = CREATE OR REPLACE SCHEMA fshin skemën ekzistuese dhe çdo tabelë në të.
guard-truncate-partition = TRUNCATE PARTITION heq çdo rresht në ndarjet që emërton.
guard-drop-partition = DROP PARTITION fshin ndarjen dhe çdo rresht në të.
quickinfo-table = tabela { $name } · { $database }
quickinfo-view = pamja { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } kolonë
       *[other] { $n } kolona
    }
quickinfo-primary-key = çelës primar
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto increment
quickinfo-default = parazgjedhje { $value }
quickinfo-generated = gjeneruar si { $expression }
quickinfo-function = funksioni { $name }({ $arguments })
quickinfo-function-returns = funksioni { $name }({ $arguments }) kthen { $returns }
quickinfo-procedure = procedura { $name }({ $arguments })
quickinfo-procedure-returns = procedura { $name }({ $arguments }) kthen { $returns }
completion-foreign-key = çelës i jashtëm
completion-expand-star = zgjero *
completion-columns = { $count ->
        [one] { $n } kolonë
       *[other] { $n } kolona
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Gabim sintakse
diag-syntax-detail = Gabim sintakse: { $detail }
diag-reserved-alias-backticks = `{ $word }` është fjalë kyçe e rezervuar dhe nuk mund të përdoret si pseudonim (vendoseni midis backtick-ësh)
diag-reserved-alias-double-quotes = `{ $word }` është fjalë kyçe e rezervuar dhe nuk mund të përdoret si pseudonim (vendoseni në thonjëza dyshe)
diag-reserved-alias-brackets = `{ $word }` është fjalë kyçe e rezervuar dhe nuk mund të përdoret si pseudonim (vendoseni në kllapa katrore)
diag-table-not-found = Tabela `{ $table }` nuk u gjet{ $within ->
        [some] {" "}në `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— mos keni parasysh `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = Kolona `{ $column }` nuk u gjet{ $within ->
        [some] {" "}në `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— mos keni parasysh `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` ka pseudonimin `{ $alias }` këtu — kualifikojeni me `{ $alias }`
diag-ambiguous-column = Kolona `{ $column }` është e paqartë
diag-join-without-condition = JOIN nuk ka kusht ON/USING — ky është një cross join; përdorni CROSS JOIN nëse kjo është qëllimi
diag-comma-join-without-where = Join me presje pa WHERE — ky është një cross join; shtoni një WHERE, ose përdorni CROSS JOIN nëse kjo është qëllimi
diag-not-grouped = Kolona `{ $column }` duhet të shfaqet në GROUP BY ose të përdoret në një funksion agregues
diag-insert-row-width = INSERT emërton { $columns } { $columns ->
        [one] kolonë
       *[other] kolona
    }, por ky rresht ka { $values } { $values ->
        [one] vlerë
       *[other] vlera
    }
diag-row-width-without-list = `{ $table }` merr { $needed } { $needed ->
        [one] vlerë
       *[other] vlera
    } pa listë kolonash, por ky rresht ka { $values }
diag-row-width-of-table = `{ $table }` ka { $columns } { $columns ->
        [one] kolonë
       *[other] kolona
    }, por ky rresht ka { $values } { $values ->
        [one] vlerë
       *[other] vlera
    }
diag-nullable-not-in = `{ $column }` mund të jetë NULL, dhe `NOT IN` nuk përputh asnjë rresht sapo nënkërkesa kthen një të tillë — shtoni `WHERE { $column } IS NOT NULL` në të, ose përdorni NOT EXISTS
diag-update-without-where = UPDATE pa WHERE ndryshon çdo rresht në tabelë
diag-delete-without-where = DELETE pa WHERE heq çdo rresht në tabelë
diag-null-comparison = Një krahasim me NULL nuk është kurrë i vërtetë — përdorni { $fix }
diag-misspelled-keyword = `{ $word }` duket si fjalë kyçe e shkruar gabim
diag-misspelled-function = `{ $word }` duket si funksion i shkruar gabim
diag-flow-merge-needs-semicolon = `MERGE` duhet të përfundojë me `;`
diag-flow-if-without-statement = `IF` nuk ka urdhër për të ekzekutuar
diag-flow-else-without-statement = `ELSE` nuk ka urdhër për të ekzekutuar
diag-flow-while-without-statement = `WHILE` nuk ka urdhër për të ekzekutuar
diag-flow-end-try-without-begin-try = `END TRY` pa një `BEGIN TRY` përkatës
diag-flow-end-catch-without-begin-catch = `END CATCH` pa një `BEGIN CATCH` përkatës
diag-flow-end-without-begin = `END` pa një `BEGIN` përkatës
diag-flow-throw-needs-semicolon = Urdhri para `THROW` duhet të përfundojë me `;`
diag-flow-end-try-without-catch = `END TRY` duhet të pasohet nga `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` nuk ka urdhër para `ELSE`-s së tij
diag-flow-else-without-if = `ELSE` pa një `IF` para tij
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` duhet të pasojë një `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` nuk ka `END TRY` përkatës
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` nuk ka `END CATCH` përkatës
diag-flow-begin-without-end = `BEGIN` nuk ka `END` përkatës

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Pyet AI-në
editor-menu-explain = Shpjego
editor-menu-optimise = Optimizo
editor-menu-plan = Plani
editor-menu-copy-single-line = Kopjo si një rresht
editor-menu-make-single-line = Bëje në një rresht
editor-menu-format-statement = Formato urdhrin
editor-menu-find-usages = Gjej përdorimet
editor-menu-create-view = Krijo pamje
cmdk-optimise-label = Optimizo këtë kërkesë
editor-several-carets = Disa kursorë dhe asgjë e përzgjedhur: përzgjidhni çfarë të ekzekutohet, ose lini një kursor

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Ekzekuto aktualin
editor-run-everything = Ekzekuto gjithçka

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Gjej
find-invalid = I pavlefshëm
find-bad-pattern = Nuk është model i vlefshëm
find-bad-pattern-why = Nuk është model i vlefshëm: { $why }
find-match-case = Dallo shkronjat e mëdha
find-whole-word = Fjalë e plotë
find-regex = Shprehje e rregullt
find-scope = Përz
find-in-selection-tip = Gjej në përzgjedhje
find-replace-placeholder = Zëvendëso
find-replace = Zëvendëso
find-replace-all = Të gjitha
goto-line-label = Shko te:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Nuk u gjet klient { $client } (PATH ose WSL).
cli-no-client-on-path = Nuk u gjet klient { $client } në PATH.
cli-no-tool = Nuk u gjet { $tool } në PATH.
cli-no-either-tool = Nuk u gjet as { $first }, as { $second } në PATH.
cli-no-either-client = Nuk u gjet klient as { $first }, as { $second } në PATH.
cli-sqlcmd-windows-in-wsl = Hyrja me Windows kërkon një sqlcmd për Windows, dhe ai që u gjet është brenda WSL. Instaloni sqlcmd në Windows, ose hapeni lidhjen në një skedë kërkese.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = Klienti i gjetur në PATH është një shim .cmd/.bat, të cilin Windows e ekzekuton përmes cmd.exe — ku një emër baze të dhënash që përmban '&' ose '|' do të ishte një komandë e dytë. Drejtojini Cilësimet te programi i vërtetë i ekzekutueshëm, ose hapni një skedë kërkese.
cli-refused-name-equals = Emri i kësaj baze të dhënash përmban '=', të cilin psql do ta lexonte si varg lidhjeje dhe do ta ndiqte drejt një serveri tjetër. Hapeni në një skedë kërkese.
cli-refused-name-uri = Emri i kësaj baze të dhënash fillon si një URI lidhjeje, të cilin psql do ta ndiqte drejt një serveri tjetër. Hapeni në një skedë kërkese.
cli-refused-file-dash = Emri i skedarit të kësaj baze të dhënash fillon me '-', të cilin klienti sqlite3 do ta lexonte si opsion e jo si skedar. Riemërtojeni, ose jepni një shteg (./name.db), dhe hapeni sërish.
cli-refused-sqlcmd-entra = sqlcmd nuk mund të marrë token-in e hyrjes së Azure CLI, kështu kjo lidhje nuk mund të hapet në të. Hapeni në një skedë kërkese.
cli-refused-sqlcmd-host = Hosti i kësaj lidhjeje përmban një karakter që sqlcmd e lexon si pjesë të adresës (',', ';', një '\' të dytë ose një prefiks ':'). Vendoseni portin në fushën e vet dhe hapeni sërish.
cli-refused-sqlcmd-ca = sqlcmd nuk mund të verifikojë kundrejt një skedari CA. Shtojeni CA-në në depon e besimit të sistemit dhe pastroni fushën CA të lidhjes, ose hapeni lidhjen në një skedë kërkese.
cli-refused-sqlcmd-client-cert = SQL Server nuk pranon hyrje me certifikatë klienti, kështu sqlcmd nuk ka ku ta dërgojë atë të kësaj lidhjeje. Pastroni certifikatën dhe çelësin e klientit, dhe hapeni sërish.
cli-refused-wsl-paths = Shtigjet e certifikatave TLS të kësaj lidhjeje janë shtigje Windows, të cilat klienti WSL nuk mund t’i hapë. Instaloni një klient vendas, ose zhvendosini certifikatat brenda WSL.
cli-refused-tunnelled-verify = Kjo lidhje verifikon certifikatën e serverit dhe e arrin atë përmes një tuneli SSH, kështu klientit do t’i thuhej të lidhej me 127.0.0.1 dhe ta kontrollonte certifikatën kundrejt tij — gjë që certifikata e vetë serverit nuk mund ta plotësojë. Hapni një skedë kërkese, ose ulni mënyrën TLS për seancën CLI.
cli-refused-not-container-id = docker ps printoi diçka që nuk është id kontejneri.
cli-refused-file-line-break = Ai emër skedari përmban një ndërprerje rreshti, të cilën klienti do ta lexonte si dy argumente. Zgjidhni një emër tjetër.
cli-refused-role = Roli në SQL-në fillestare të kësaj lidhjeje nuk mund t’i jepet mjetit ashtu siç është.
cli-refused-mysql-source-path = Klienti mysql do ta lexonte shtegun e atij skedari si komanda të vetat (një ';' ose një '\' në të, ose një hapësirë në një skaj). Zhvendoseni ose riemërtojeni, pastaj riktheni.
cli-refused-cloud-token = Kjo lidhje hyn përmes token-it të një CLI cloud, i cili nuk i jepet një klienti të jashtëm. Hapeni në një skedë kërkese.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Nuk mund të përcaktohet nga ekzekutohet Schemaic: { $error }
install-registry-open = Mjedisi juaj i përdoruesit në regjistër nuk mund të hapej: { $error }
install-path-read = PATH-i juaj i përdoruesit nuk mund të lexohej: { $error }
install-path-write = PATH-i juaj i përdoruesit nuk mund të shkruhej: { $error }
install-look-at = { $path } nuk mund të shqyrtohej: { $error }
install-read-link = Lidhja në { $path } nuk mund të lexohej: { $error }
install-create = { $path } nuk mund të krijohej: { $error }
install-remove-old = { $path } i vjetër nuk mund të hiqej: { $error }
install-remove = { $path } nuk mund të hiqej: { $error }
install-path-is-windows = Një hyrje në PATH të përdoruesit është instalim Windows; kjo platformë krijon lidhje në vend të saj.
install-link-is-unix = Lidhja e komandës është instalim për macOS dhe Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Nuk mund të përcaktohet në cilën dosje ndodhet { $exe }.
install-refuse-no-shim = Nuk ka schemaic.com pranë kësaj kopjeje të Schemaic, në { $dir }, kështu nuk ka komandë konsole për t’u vendosur në PATH. Aplikacioni i instaluar e ka një të tillë; një version zhvillimi jo.
install-refuse-semicolon = Kjo kopje e Schemaic ndodhet në { $dir }, emri i së cilës përmban një ';' — PATH do ta lexonte si dy dosje të ndryshme. Zhvendoseni diku pa të dhe provoni sërish.
install-refuse-exe-replaced = { $exe } u zëvendësua ndërsa Schemaic po punonte, kështu një lidhje drejt tij nuk do të tregonte asgjë. Rinisni Schemaic, pastaj instalojeni sërish.
install-refuse-no-home = Nuk ka dosje shtëpie për të mbajtur lidhjen, kështu nuk ka ku të kërkohet.
install-refuse-path-not-text = PATH-i juaj i përdoruesit nuk ruhet si tekst në regjistër, prandaj u la i paprekur.
install-refuse-path-not-unicode = PATH-i juaj i përdoruesit përmban karaktere që nuk mund të rilexohen në mënyrë të sigurt, prandaj u la i paprekur.
install-refuse-portable = Kjo është një kopje portabël, kështu “Hiq” nuk mund të dallojë nëse “Instalo” e vendosi { $dir } në PATH-in tuaj apo ishte aty që më parë me programe të tjera brenda. Hiqeni te Environment Variables nëse e shtoi “Instalo”.
install-refuse-spot-taken = { $link } ekziston tashmë dhe nuk është lidhje, kështu “Instalo” e la të paprekur. Zhvendoseni mënjanë dhe provoni sërish.
install-refuse-points-elsewhere = { $link } tregon tashmë te { $to }. Hiqeni nëse ajo është një kopje e vjetër e Schemaic, pastaj provoni sërish.
install-refuse-unlink-elsewhere = { $link } tregon te { $to }, jo te kjo kopje e Schemaic, kështu “Hiq” e la të paprekur.
install-refuse-unlink-not-ours = { $link } nuk është lidhje e krijuar nga Schemaic, kështu “Hiq” e la të paprekur.
install-report-already = Komanda schemaic është tashmë në PATH-in tuaj, në { $dir }.
install-report-added = { $dir } u shtua në PATH-in tuaj të përdoruesit. Hapni një terminal të ri për të përdorur schemaic — një që është tashmë i hapur mban PATH-in e vjetër.
install-report-linked = { $link } u lidh me { $target }.
install-report-linked-if-not-found = { $link } u lidh me { $target }. Nëse një terminal i ri nuk e gjen schemaic, shtojeni { $dir } në PATH-in tuaj te profili i shell-it.
install-report-linked-not-on-path = { $link } u lidh me { $target }. { $dir } ende nuk është në PATH-in tuaj — shtojeni te profili i shell-it, pastaj hapni një terminal të ri.
install-report-removed = { $dir } u hoq nga PATH-i juaj i përdoruesit. Një terminal që është tashmë i hapur mban PATH-in e vjetër.
install-report-path-absent = { $dir } nuk është në PATH-in tuaj të përdoruesit, kështu nuk kishte asgjë për të hequr.
install-report-unlinked = { $link } u hoq.
install-report-link-absent = Nuk ka { $link }, kështu nuk kishte asgjë për të hequr.
install-report-package = Komanda schemaic në { $dir } erdhi me paketën e instaluar, jo nga “Instalo” — hiqni paketën për ta hequr.
install-hint-adds-folder = Ekzekutoni Schemaic nga një terminal me emër. Shton dosjen e tij në PATH-in tuaj të përdoruesit.
install-hint-links = Ekzekutoni Schemaic nga një terminal me emër. E lidh në ~/.local/bin.
install-hint-folder-on-path = Ekzekutoni Schemaic nga një terminal me emër. Dosja e tij, { $dir }, është tashmë në PATH-in tuaj të përdoruesit.
install-hint-comes-with-app = Ekzekutoni Schemaic nga një terminal me emër. Komanda schemaic vjen me aplikacionin, në { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = Tuneli SSH ende nuk është vendosur
tunnel-could-not-open = Tuneli SSH nuk mund të hapej: { $error }. Rifreskoni skemën për të provuar sërish.
tunnel-server-failed = Lidhja përmes tunelit SSH nuk u realizua: { $error }. Rifreskoni skemën për të provuar sërish.

gate-plan-not-connected = Nuk jeni lidhur — plani nuk u ekzekutua. Serveri nuk iu përgjigj kontrollit të gjendjes, kështu nuk iu dërgua asgjë.
gate-plan-connection-changed = Ndërruat lidhjen ndërsa Schemaic po kontrollonte nëse ishte e arritshme, kështu plani nuk u ekzekutua. Kthehuni dhe pyesni sërish.
gate-plan-tab-moved-on = Ndërruat skedën ndërsa lidhja po rikontrollohej, kështu plani nuk u ekzekutua. Ai përshkruan skedën nga e cila u kërkua — kthehuni te ajo skedë dhe pyesni sërish.
gate-connection-changed = Nuk u ekzekutua në { $name } — ndërruat lidhjen ndërsa Schemaic po kontrollonte nëse ishte e arritshme. Kthehuni dhe provoni sërish.
gate-not-connected = Nuk jeni lidhur me { $name }. Serveri nuk u përgjigj — kontrolloni që po punon dhe që cilësimet e kësaj lidhjeje janë të sakta.
gate-this-connection = këtë lidhje
gate-run-tab-moved-on = Ndërruat skedën ndërsa lidhja po rikontrollohej, kështu urdhri nuk u ekzekutua. Ai u kontrollua kundrejt skedës ku u shkrua, dhe ekzekutimi i tij këtu do ta ekzekutonte diku tjetër. Kthehuni te ajo skedë dhe ekzekutojeni sërish.
gate-commit-not-connected = Nuk jeni lidhur — konfirmimi nuk u provua. Ndryshimet në pritje ruhen.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = { $program } nuk mund të nisej: { $error }. Zgjidhni një shell tjetër te Cilësimet, Terminali.
term-cli-read-only = Kjo lidhje është vetëm për lexim në Schemaic. Klienti i saj i linjës së komandave nuk do të ishte, prandaj nuk hapet këtu.
backup-read-only-server = Kjo lidhje është vetëm për lexim në Schemaic, dhe kopja rezervë e këtij motori shkruhet nga serveri, prandaj nuk u bë asnjë.
backup-dialog-title = Krijo kopje rezervë të bazës së të dhënave
backup-kind-pg-dump = Dump PostgreSQL në format custom
backup-kind-sql = Skript SQL
backup-kind-sqlite = Bazë të dhënash SQLite
backup-not-started = Kopja rezervë nuk nisi: { $error }
backup-done = Kopja rezervë u ruajt në { $path }.
backup-failed = Kopja rezervë dështoi: { $error }
restore-title = Rikthe nga skedari
restore-confirm = Të ekzekutohet { $path } në { $database } te { $connection }? Ajo që krijon ose zëvendëson nuk mund të zhbëhet. Ekzekutohet në terminal, ku Ctrl+C e ndalon.
restore-read-only = Kjo lidhje është vetëm për lexim në Schemaic, prandaj nuk rikthehet asgjë në të.
restore-engine-not-here = Kopjet rezervë të këtij motori nuk rikthehen nga këtu.
restore-unreadable = { $path } nuk mund të lexohej para rikthimit, prandaj nuk u ekzekutua asgjë: { $error }
restore-refused = { $path } kalon në një bazë tjetër të dhënash gjatë ekzekutimit — { $switches } — kështu, i rikthyer në { $database }, do të shkruante atje. Nuk u ekzekutua asgjë. Riktheni në bazën e të dhënave që emërton, ose hiqni ato rreshta nga një kopje e skedarit.
restore-refused-more = { $path } kalon në një bazë tjetër të dhënash gjatë ekzekutimit — { $switches }, dhe { $more } të tjera — kështu, i rikthyer në { $database }, do të shkruante atje. Nuk u ekzekutua asgjë. Riktheni në bazën e të dhënave që emërton, ose hiqni ato rreshta nga një kopje e skedarit.
restore-switch = { $how } `{ $name }` në rreshtin { $line }
restore-switch-unnamed = një { $how } në rreshtin { $line } që nuk emërton asnjë bazë të dhënash që Schemaic mund të lexojë

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Mbyll “{ $name }”
tab-close-unsaved = “{ $name }” ka ndryshime të paruajtura. Mbyllja i hedh poshtë; skedari në disk mbetet siç është. Të mbyllet gjithsesi?
tab-close-all-title = Mbyll të gjitha skedat
tab-close-all-question = Jeni të sigurt që doni të mbyllni të gjitha skedat?
tab-close-others-title = Mbyll skedat e tjera
tab-close-others-question = Jeni të sigurt që doni të mbyllni të gjitha skedat e tjera?
file-open-title = Hap skedar SQL
file-save-title = Ruaj skedarin SQL
file-sql-filter = Skript SQL
file-read-failed = { $path } nuk mund të lexohej: { $error }
file-save-failed = { $path } nuk mund të ruhej: { $error }
file-too-large = { $path } është { $size } — shumë i madh për t’u hapur në një skedë redaktori. Schemaic do ta kalonte pjesën më të madhe të kohës duke e rianalizuar. Ekzekutojeni nga një skedë kërkese, ose përdorni Importo për një skedar të dhënash.
file-too-large-short = Skedari është shumë i madh për t’u hapur.
file-large-title = Të hapet një skedar i madh?
file-large-question = “{ $name }” është { $size }. Schemaic e rianalizon të gjithë dokumentin pak pas çdo pauze në shkrim, kështu një skedar kaq i madh e bën redaktorin të ngadaltë për sa kohë skeda është e hapur. Të hapet gjithsesi?
file-changed-on-disk = { $path } ka ndryshuar në disk që kur u hap. Ruajtja tani do t’i hidhte poshtë ato ndryshime — ringarkoni skedarin (ose përdorni “Ruaj si” me një emër tjetër).
file-lossy-title = Të zëvendësohen bajtet e palexueshme?
file-lossy-question = Schemaic nuk mundi ta lexonte si tekst çdo bajt të “{ $name }” dhe i shfaqi ato bajte si “�”. Ruajtja shkruan atë që shihni, kështu secili prej tyre zëvendësohet përgjithmonë — në të gjithë skedarin, jo vetëm në rreshtat që redaktuat. Të ruhet gjithsesi?
file-replace-title = Të zëvendësohet skedari?
file-replace-question = “{ $name }” ekziston tashmë — “{ $typed }” u ruajt me prapashtesën .sql të shtuar. Të zëvendësohet?
file-reload-title = Ringarko nga disku
file-reload-unsaved = “{ $name }” ka ndryshime të paruajtura. Ringarkimi i hedh poshtë. Të ringarkohet gjithsesi?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Një eksport është tashmë në ekzekutim. Anulojeni ose prisni që të përfundojë.
export-failed = Eksporti dështoi: { $error }
export-worker-died = Eksporti dështoi: procesi punues ndaloi: { $error }
export-not-published = Eksporti dështoi: { $path } nuk mund të publikohej: { $error }
export-not-renamed = Eksporti shkroi { $part }, por nuk mundi ta riemërtonte në { $path }: { $error }
export-nothing-matched = Nuk ka asgjë për të eksportuar — asnjë tabelë nuk përputhej me përzgjedhjen.
export-stopped = Eksporti u anulua.
dump-failed = Dump-i dështoi: { $error }
blob-load-too-large = Ai skedar është { $size } — maksimumi që mund të ngarkohet është { $most }.
blob-load-failed = Ngarkimi dështoi: { $error }
blob-save-failed = Ruajtja dështoi: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = { $path } nuk mund të shkruhej: { $error }
settings-export-title = Eksporto cilësimet
settings-import-title = Importo cilësimet
settings-file-filter = Cilësimet e Schemaic
settings-import-nothing = Nuk ka asgjë për të importuar: çdo cilësim në { $path } përputhet tashmë me tuajat, dhe nuk shton asnjë fragment.
settings-import-question = { $path } ndryshon { $changed ->
        [one] { $changed } cilësim
       *[other] { $changed } cilësime
    } dhe shton { $snippets ->
        [one] { $snippets } fragment
       *[other] { $snippets } fragmente
    }. Cilësimet që ndryshon zëvendësojnë vlerat tuaja aktuale; fragmentet shtohen pranë tuajave.
settings-import-unreadable = nuk mund të lexohej: { $error }
connections-export-title = Eksporto lidhjet
connections-file-filter = Lidhjet e Schemaic
connections-replace-title = Zëvendëso “{ $file }”
connections-replace-question = Në atë dosje ka tashmë një skedar të quajtur “{ $file }”. Eksporti ia shton atë mbaresë emrit që shkruat, kështu dritarja e ruajtjes nuk pyeti për të. Zëvendësimi i tij nuk mund të zhbëhet.
import-choose-file-title = Zgjidhni një skedar lidhjesh
import-no-containers = Nuk po punon asnjë kontejner baze të dhënash.
import-added = { $count ->
        [one] U shtua { $count } lidhje.
       *[other] U shtuan { $count } lidhje.
    }
import-added-foreign = { $count ->
        [one] U shtua { $count } lidhje.
       *[other] U shtuan { $count } lidhje.
    } “Vetëm për lexim” dhe etiketa e mjedisit nuk barten nga një klient tjetër — caktojini te “Menaxho lidhjet” për çdo gjë që ka rëndësi.
conn-delete-title = Fshi “{ $name }”
conn-delete-question = Kjo fshin lidhjen, fjalëkalimin e saj të ruajtur, bisedën e ruajtur me AI, historikun e kërkesave dhe skedat e saj. Nuk mund të zhbëhet.
conn-delete-keyring-kept = Schemaic nuk mundi të arrinte keyring-un e sistemit operativ, kështu fjalëkalimi i ruajtur dhe sekretet SSH të lidhjes së fshirë **janë ende aty**.
conn-delete-keyring-later = Ato do të hiqen herën tjetër që fshihet ose ruhet një lidhje ndërsa keyring-u është i arritshëm. Ndërkohë asnjë lidhje e re nuk mund t’i marrë — id-të e lidhjeve nuk ripërdoren kurrë.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = { $name } nuk mund të lexohej ({ $error }).
notice-unreadable-kept = Skedari i palexueshëm u ruajt si { $name }.corrupt; Schemaic u kthye te kopja e tij rezervë, ose te vlerat e paracaktuara nëse edhe kopja rezervë ishte e palexueshme.
notice-unreadable-swept = Ajo kopje mund të mbajë fjalëkalime të pashifruara, kështu hiqet herën tjetër që fshini një lidhje — rikuperoni prej saj çdo gjë që ju duhet para kësaj.
notice-missing = { $name } mungonte.
notice-missing-restored = Schemaic e rikuperoi nga { $name }{ $from }, që ishte ende aty. Do të rishkruhet herën tjetër që ndryshon ky cilësim — deri atëherë ajo kopje pranë tij është e vetmja.
notice-keyring-unread = { $n ->
    [one] Schemaic nuk mundi të lexonte { $n } sekret të ruajtur nga keyring-u i sistemit operativ.
   *[other] Schemaic nuk mundi të lexonte { $n } sekrete të ruajtura nga keyring-u i sistemit operativ.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic nuk mundi të lexonte { $n } sekret të ruajtur nga keyring-u i sistemit operativ ({ $error }).
   *[other] Schemaic nuk mundi të lexonte { $n } sekrete të ruajtura nga keyring-u i sistemit operativ ({ $error }).
}
notice-keyring-unread-kept = Ato fjalëkalime nuk janë të disponueshme në këtë seancë dhe **nuk** u fshinë — lërini fushat bosh dhe do të rikthehen sapo keyring-u të jetë i arritshëm. Shkrimi i një të riu e mbishkruan sekretin e ruajtur.
notice-in-the-clear = { $n ->
    [one] Schemaic nuk mundi të arrinte keyring-un e sistemit operativ, kështu { $n } fjalëkalim u ruajt **si tekst i thjeshtë** në `connections.json`.
   *[other] Schemaic nuk mundi të arrinte keyring-un e sistemit operativ, kështu { $n } fjalëkalime u ruajtën **si tekst i thjeshtë** në `connections.json`.
}
notice-in-the-clear-where = Ai skedar ndodhet në dosjen që hap butoni *Hap dosjen* te Cilësimet → Të përgjithshme, pranë regjistrit — kontrollojeni para se ta ndani. Ruajtja sërish sapo keyring-u të jetë i arritshëm i kthen ato atje.
notice-undeleted = { $n ->
    [one] { $n } fjalëkalim i pastruar është ende në keyring-un e sistemit operativ — Schemaic nuk mundi ta fshinte.
   *[other] { $n } fjalëkalime të pastruara janë ende në keyring-un e sistemit operativ — Schemaic nuk mundi t’i fshinte.
}
notice-undeleted-after = Fusha është bosh në disk, por nisja e ardhshme do ta plotësojë sërish. Ruajeni sërish sapo keyring-u të jetë i arritshëm.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Nuk u kthye SQL
inline-ai-not-sql = Modeli nuk ktheu SQL
inline-ai-failed = gjenerimi dështoi
ai-fill-no-value = AI nuk ktheu asnjë vlerë.
conn-gone = lidhja nuk ekziston më
tx-session-not-ready = lidhja e transaksionit nuk është gati — kaloni në Konfirmim automatik dhe kthehuni
tx-session-open-failed = lidhja e transaksionit nuk mund të hapej: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = Urdhri u anulua.
tx-committed-before-ddl = Transaksioni ku u ekzekutua kjo nuk ekziston më: MySQL konfirmon para se të ekzekutojë një urdhër DDL, kështu urdhrat që ishin tashmë në të janë të përhershëm dhe “Rikthe” nuk do t’i zhbëjë.
tx-rolled-back-by-server = Transaksioni ku u ekzekutua kjo nuk ekziston më: SQL Server e riktheu, kështu çdo urdhër në të që nga fillimi i tij është zhbërë.
tx-doomed = Transaksioni ku u ekzekutua kjo nuk mund të konfirmohet më: SQL Server e shënoi si të pakonfirmueshëm, kështu “Rikthe” është e vetmja rrugë për të vazhduar, dhe ajo zhbën çdo urdhër në të.
tx-after-own-commit = Transaksioni ku u ekzekutua kjo ka përfunduar, dhe kjo ekzekutoi një COMMIT të vetin para se të dështonte, kështu ajo që erdhi para atij COMMIT ka shumë të ngjarë të jetë konfirmuar e jo zhbërë. Kontrolloni të dhënat para se të ekzekutoni sërish ndonjë pjesë të saj.
tx-lost-on-commit = Lidhja humbi me një transaksion të hapur. Humbi gjatë një COMMIT, kështu nuk dihet nëse { $stmts ->
        [0] transaksioni u konfirmua apo u rikthye
        [one] { $stmts } urdhri i ekzekutuar në të që nga fillimi i tij u konfirmua apo u rikthye
       *[other] { $stmts } urdhrat e ekzekutuar në të që nga fillimi i tij u konfirmuan apo u rikthyen
    }: serveri mund të ketë konfirmuar para se të binte lidhja. Kontrolloni të dhënat para se të ekzekutoni sërish ndonjë pjesë të tyre.
tx-lost-on-implicit-commit = Lidhja humbi me një transaksion të hapur. MySQL konfirmon një transaksion të hapur para se të ekzekutojë këtë urdhër, kështu { $stmts ->
        [0] në të ende nuk ishte ekzekutuar asgjë, kështu nuk kishte asgjë për të konfirmuar
        [one] { $stmts } urdhri i ekzekutuar në të që nga fillimi i tij ka shumë të ngjarë të jetë konfirmuar atëherë, dhe është i përhershëm
       *[other] { $stmts } urdhrat e ekzekutuar në të që nga fillimi i tij ka shumë të ngjarë të jenë konfirmuar atëherë, dhe janë të përhershëm
    }. Kontrolloni të dhënat para se të ekzekutoni sërish ndonjë pjesë të tyre.
tx-lost-rolled-back = Lidhja humbi me një transaksion të hapur. Serveri e rikthen një transaksion të papërfunduar kur mbyllet lidhja e tij, kështu { $stmts ->
        [0] në të ende nuk ishte ekzekutuar asgjë
        [one] { $stmts } urdhri i ekzekutuar në të që nga fillimi i tij është zhbërë
       *[other] { $stmts } urdhrat e ekzekutuar në të që nga fillimi i tij janë zhbërë
    }.
tx-lost-rolled-back-mysql = Lidhja humbi me një transaksion të hapur. Serveri e rikthen një transaksion të papërfunduar kur mbyllet lidhja e tij, kështu { $stmts ->
        [0] në të ende nuk ishte ekzekutuar asgjë
        [one] { $stmts } urdhri i ekzekutuar në të që nga fillimi i tij është zhbërë — përveç një shkrimi në një tabelë që nuk mbështet transaksione (MyISAM, MEMORY), të cilin asnjë rikthim nuk e zhbën
       *[other] { $stmts } urdhrat e ekzekutuar në të që nga fillimi i tij janë zhbërë — përveç një shkrimi në një tabelë që nuk mbështet transaksione (MyISAM, MEMORY), të cilin asnjë rikthim nuk e zhbën
    }.
tx-not-sent-timeout = Koha maksimale e urdhrit skadoi ndërsa ky ekzekutim ende po priste lidhjen e skedës. Një veprim i mëparshëm mbi të — një konfirmim, ose rileximi pas tij — nuk kishte përfunduar, kështu urdhri nuk iu dërgua kurrë serverit. Nuk u ekzekutua asgjë, dhe transaksioni është saktësisht aty ku ishte.
tx-not-sent-cancelled = Ky ekzekutim u anulua ndërsa ende po priste lidhjen e skedës. Një veprim i mëparshëm mbi të — një konfirmim, ose rileximi pas tij — nuk kishte përfunduar, kështu urdhri nuk iu dërgua kurrë serverit. Nuk u ekzekutua asgjë, dhe transaksioni është saktësisht aty ku ishte.
snippet-default-name = Fragment
snippet-delete-title = Fshi fragmentin
snippet-delete-question = Të fshihet “{ $name }”? Ky veprim nuk mund të zhbëhet.
copy-target-read-only = Lidhja e objektivit është vetëm për lexim, kështu nuk u kopjua asnjë rresht.
copy-target-gone = Lidhja e objektivit nuk ekziston më, kështu nuk u kopjua asnjë rresht.
copy-not-started = Nuk u kopjua asnjë rresht: { $error }
copy-counter-not-moved = { $count ->
        [one] U kopjua { $count } rresht, por numëruesi i tabelës nuk u çua përtej tij, kështu futja e ardhshme mund të përplaset: { $error }
       *[other] U kopjuan { $count } rreshta, por numëruesi i tabelës nuk u çua përtej tyre, kështu futja e ardhshme mund të përplaset: { $error }
    }
copy-stopped = Kopjimi ndaloi për shkak të një gabimi: { $error }
compare-reading-failed = Leximi i { $database }: { $error }
monitor-no-row-key = Kjo tabelë nuk ka çelës rreshti — ndryshimet nuk mund të ndiqen.
monitor-restarted = Kolonat e tabelës ndryshuan, kështu ndjekja e ndryshimeve rifilloi nga këtu.
timeout-cancelled-app = U anulua: urdhri zgjati më shumë se koha maksimale e urdhrit prej { $timeout }. Ndryshojeni ose çaktivizojeni te Cilësimet → Kërkesa.
timeout-cancelled-connection = U anulua: urdhri zgjati më shumë se koha maksimale e urdhrit prej { $timeout }. Ndryshojeni ose çaktivizojeni te cilësimet e kësaj lidhjeje.
ai-chat-no-database = Baza e të dhënave nuk mund të arrihet, kështu asistenti nuk mund të niset për këtë lidhje. Kontrolloni lidhjen dhe provoni sërish.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **Asistenti nuk mundi të nisej.** Schemaic i jep çdo procesi agjenti një dosje pune private të vetën, dhe as dosja e përkohshme e sistemit, as dosja e konfigurimit nuk pranuan të krijohej një e tillë — kështu nuk ka ku ta ekzekutojë që të mos jetë dosja e dikujt tjetër. Kontrolloni që dosja e përkohshme të jetë e shkrueshme dhe provoni sërish.
ai-no-reply-file = Nuk mund të krijohej një skedar i përkohshëm për përgjigjen.
ai-inline-no-opencode-config = Konfigurimi i OpenCode nuk mund të shkruhej, kështu gjenerimi nuk nisi (pa të CLI do të punonte me mjetet e veta të aktivizuara).
ai-inline-no-copilot-home = Dosja private e shtëpisë së GitHub Copilot nuk mund të krijohej, kështu gjenerimi nuk nisi (pa të CLI do të ngarkonte konfigurimin tuaj të Copilot, përfshirë serverët MCP dhe hook-et e tij).
ai-inline-no-env-writer = { $cli } është shënuar si i izoluar nga mjedisi i tij, por Schemaic nuk ka mënyrë ta përgatisë atë mjedis, kështu gjenerimi nuk nisi.
ai-inline-no-cursor-rules = Skedari i lejeve të Cursor nuk mund të shkruhej, kështu gjenerimi nuk nisi (pa të CLI shkruan skedarë pa pyetur).
ai-inline-no-rules-writer = { $cli } është shënuar si i kufizuar nga një skedar lejesh, por Schemaic nuk ka mënyrë të shkruajë një të tillë për të, kështu gjenerimi nuk nisi.
ai-no-tools = **Kjo seancë nuk ka mjete për bazën e të dhënave.** { $cause }, kështu asistenti nuk mund të kërkojë asgjë — mund të përgjigjet vetëm nga përmbledhja e skemës që është tashmë në mesazhin e tij. Mbyllja dhe rihapja e panelit do të provojë sërish.
ai-no-endpoint-file = Schemaic nuk mundi të krijonte skedarin privat që i tregon asistentit si të arrijë bazën tuaj të të dhënave
ai-cursor-not-approved = Schemaic nuk mundi t’i miratonte mjetet e tij të bazës së të dhënave me Cursor ({ $error })
ai-antigravity-not-registered = Schemaic nuk mundi t’i regjistronte mjetet e tij të bazës së të dhënave me Antigravity
ai-antigravity-shared = Një dritare tjetër e Schemaic po përdor tashmë Antigravity. Antigravity mban një regjistrim të vetëm MCP për të gjithë kompjuterin, i cili mund të tregojë vetëm një lidhje, kështu asistenti i kësaj dritareje nuk ka mjete për bazën e të dhënave
ai-antigravity-not-claimed = Schemaic nuk mundi të merrte konfigurimin e Antigravity për këtë seancë
ai-refused-no-opencode-config = Schemaic nuk mundi të shkruante konfigurimin që e kufizon OpenCode, kështu asistenti është çaktivizuar — ekzekutimi i tij pa atë skedar do t’i jepte seancës një shell. Kontrolloni që dosja e të dhënave të aplikacionit të jetë e shkrueshme.
ai-refused-no-copilot-home = Schemaic nuk mundi të krijonte dosjen private të shtëpisë së GitHub Copilot, kështu asistenti është çaktivizuar — pa të CLI do të ngarkonte konfigurimin tuaj të Copilot, përfshirë serverët MCP dhe hook-et e tij. Kontrolloni që dosja e të dhënave të aplikacionit të jetë e shkrueshme.
ai-refused-no-cursor-rules = Schemaic nuk mundi të shkruante skedarin e lejeve që e ndalon Cursor të shkruajë skedarë dhe të ekzekutojë komanda, kështu asistenti është çaktivizuar — pa të Cursor shkruan skedarë pa pyetur. Kontrolloni që dosja e të dhënave të aplikacionit të jetë e shkrueshme.
ai-launch-failed-turn = CLI `{ $bin }` nuk mund të nisej ({ $error }). Sigurohuni që { $cli } është i instaluar, ose jepni shtegun e tij te Cilësimet → AI.
ai-launch-failed-session = CLI `{ $bin }` nuk mund të nisej ({ $error }). Sigurohuni që { $cli } është i instaluar (ose caktoni shtegun e tij te Cilësimet → AI).
ai-turn-ended = Radha e { $cli } përfundoi papritur: { $error }
ai-session-ended-badly = Seanca AI përfundoi papritur: { $error }
ai-session-ended = Seanca e { $cli } përfundoi. Pyesni sërish për të nisur një të re.
ai-session-restart-failed = Seanca e { $cli } nuk mund të rinisej pas ndalimit ({ $error }), kështu ka përfunduar. Pyesni sërish për të nisur një të re.
ai-session-not-accepting = Seanca e { $cli } nuk pranon më pyetje. Pyesni sërish për të nisur një të re.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = Konteksti që i dërgohet { $cli } është shumë i madh për një linjë të vetme komande ({ $total } { $unit }; kjo platformë lejon rreth { $limit }). { $levers }
ai-unit-characters = karaktere
ai-unit-bytes = bajte
ai-levers-schema-and-query = Ngushtoni Cilësimet → AI → Konteksti i skemës në bazën aktive të të dhënave (ose Asnjë), ose shkurtoni kërkesën në redaktor.
ai-levers-sample = Ulni qasjen e AI në të dhëna për këtë lidhje nën “Lejoje të lexojë të dhëna”, gjë që ndalon dërgimin e mostrës së rreshtave, ose ngushtoni Cilësimet → AI → Konteksti i skemës.
ai-batch-shim = { $cli } u zgjidh në një shim batch ({ $bin }), të cilit nuk mund t’i jepet një mesazh me shumë rreshta — Windows e ekzekuton një .cmd përmes cmd.exe, dhe ai e refuzon argumentin para se të niset { $cli }. Vendosni te Cilësimet → AI → Shtegu i CLI programin e vërtetë të ekzekutueshëm (një instalim npm zakonisht ka një nën node_modules, pranë shim-it).
ai-cli-exited = CLI { $cli } doli me statusin { $code }
ai-cli-signalled = CLI { $cli } u ndërpre nga një sinjal
ai-constraint-sandboxed = { $cli } ekzekutohet në sandbox: mjetet e tij të integruara nuk mund të shkruajnë skedarë ose të ekzekutojnë komanda, megjithëse mund ta lexojnë ende këtë kompjuter.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Çdo server MCP që keni regjistruar me të është gjithashtu i disponueshëm për këtë seancë, dhe Schemaic nuk mund t’i kufizojë.
    }
ai-constraint-workspace-rules = { $cli } kufizohet vetëm te përgjigjet nga një skedar lejesh që Schemaic shkruan për këtë seancë, jo nga një sandbox: mjeteve të tij u refuzohet shkrimi i skedarëve dhe ekzekutimi i komandave, por mund të lexojnë ende skedarë në këtë kompjuter.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Serverët MCP që keni regjistruar me të nisin gjithashtu me këtë seancë; mjetet e tyre refuzohen, përveç nëse i lejojnë cilësimet tuaja.
    }
ai-constraint-denylist = Ky version i { $cli } nuk e pranon flamurin që zbraz mjetet e tij të integruara, kështu ato mbahen nga një listë ndalimi — më e dobët dhe jo diçka që Schemaic mund ta garantojë. Përditësimi i CLI e rikthen mbylljen e plotë.
ai-constraint-unrestricted = Schemaic nuk mundi t’i kufizonte plotësisht mjetet e integruara të { $cli } për këtë seancë, kështu ai mund të jetë në gjendje të bëjë më shumë se sa t’u përgjigjet pyetjeve për këtë bazë të dhënash.
ai-constraint-unknown = Schemaic nuk mundi të konfirmonte që ky program { $cli } i pranon flamurët që e kufizojnë, kështu asistenti është çaktivizuar. Kontrolloni shtegun te Cilësimet → AI.
ai-seed-empty = modeli nuk ktheu përmbajtje
ai-seed-not-json = përgjigjja nuk ishte JSON i vlefshëm: { $error }
ai-seed-not-rows = përgjigjja nuk ishte një varg JSON me objekte rreshtash

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } nuk është skedar.
import-source-too-big = { $path } është { $size }, përtej { $most } që lexohet këtu.
import-source-unreadable = { $path } nuk mund të lexohej: { $error }
docker-not-found = Docker nuk u gjet — as në PATH, as në WSL.
docker-not-started = Docker nuk mund të nisej: { $error }
docker-timed-out = Docker nuk u përgjigj brenda { $secs } sekondave.
docker-not-waited = Docker nuk mund të pritej: { $error }
docker-answered = Docker u përgjigj: { $error }
docker-exited = Docker doli me { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = lexuesi i skedarit ndaloi papritur: { $error }
script-changed-since-read = Ky skedar ndryshoi në disk pasi u lexua, kështu përmbledhja më sipër nuk është ajo që do të ekzekutohej. Zgjidheni sërish.
script-no-terminator = Ky skedar nuk përmban mbyllës urdhri, kështu nuk është skript SQL që Schemaic mund ta ekzekutojë. Kontrolloni që zgjodhët skedarin që kishit parasysh.
ai-cursor-enable-unfinished = `mcp enable` nuk përfundoi
