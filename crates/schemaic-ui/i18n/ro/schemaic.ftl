# Schemaic — Romanian catalogue.
#
# Machine-drafted and unreviewed (docs/architecture.md, The string catalogue); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Romanian.

## Settings › Appearance › Language

settings-language = Limbă
settings-language-system = Sistem ({ $language })
settings-language-restart = Schimbarea limbii se aplică la următoarea pornire a Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Dimensiunea fontului
settings-term-cursor-style = Stilul cursorului
settings-term-copy-on-select = Copiază la selectare
settings-term-copy-on-select-hint = Copiază textul selectat în clipboard imediat ce se încheie selecția.
settings-term-blink = Cursor intermitent
settings-term-blink-hint = Cursorul clipește cât timp terminalul are focusul.

## Settings › AI

settings-ai-agent = Agent CLI
settings-ai-disabled = Dezactivat
settings-ai-off-hint = Nu se trimite nimic niciunui agent, nu se pornește niciun agent CLI și nu se oferă nicio acțiune AI. Alege un CLI pentru a porni asistentul.
settings-ai-cli-path = Calea { $cli }
settings-ai-cli-path-placeholder = Lasă gol pentru detectare automată
settings-ai-detected = Detectat automat: { $path }
settings-ai-detect-failed = Detectarea automată a eșuat. { $cli } nu a fost găsit — indică o cale mai sus.
settings-ai-cli-path-missing = Fișierul nu există.
settings-ai-model = Model
settings-ai-model-placeholder = Lasă gol pentru modelul implicit al agentului
settings-ai-model-hint = Orice id acceptat de acest CLI — un alias sau un snapshot datat, pentru a fixa un model între actualizări.
settings-ai-model-hint-with-chips = Orice id acceptat de acest CLI — un alias sau un snapshot datat, pentru a fixa un model între actualizări. Butoanele sunt scurtături, nu lista completă.
settings-ai-effort = Efort
settings-ai-instructions = Instrucțiuni personalizate
settings-ai-instructions-placeholder = Dialect, convenții, reguli ale casei…
settings-ai-schema-context = Context de schemă
settings-ai-schema-context-hint = Câtă structură a bazei de date însoțește fiecare mesaj. Niciuna ascunde și instrumentele de schemă, așa că asistentul îți cere numele în loc să citească singur catalogul.
settings-ai-data-access = Acces la date
settings-ai-data-access-where = Se setează pentru fiecare conexiune, în setările conexiunii.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Doar schema
ai-data-on-request = Doar ce atașez eu
ai-data-full = Poate citi date
ai-data-schema-only-hint = Doar structura — nume, tipuri și definiții. Niciun rând nu părăsește acest calculator, iar atașarea este refuzată.
ai-data-on-request-hint = Asistentul nu citește date din proprie inițiativă. Rândurile pe care le atașezi dintr-un rezultat părăsesc acest calculator odată cu întrebarea.
ai-data-full-hint = Asistentul poate rula singur interogări doar pentru citire și poate citi rânduri de probă. Tot ce citește părăsește acest calculator.
settings-ai-gutter = Linie de accent la răspunsuri
settings-ai-gutter-hint = Marchează răspunsurile asistentului cu o linie colorată de-a lungul marginii drepte. Dezactivat, au aceeași margine pe ambele părți.

## Settings › section list

settings-section-general = General
settings-section-editor = Editor
settings-section-formatting = Formatarea codului
settings-section-query = Interogare
settings-section-terminal = Terminal
settings-section-ai = Asistent AI
settings-section-appearance = Aspect

## Settings › shared

settings-off = Dezactivat

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } caracter
    [few] { $n } caractere
   *[other] { $n } de caractere
}

## Settings › General

settings-log = Fișier jurnal
settings-log-hint = Diagnosticele, inclusiv blocările, sunt scrise în { $path }.
settings-log-hint-none = Nu s-a putut deschide niciun fișier jurnal pe acest calculator, așa că nu se scrie nimic într-unul.
settings-log-open-folder = Deschide dosarul
settings-bundle = Fișier de setări
settings-bundle-hint = Preferințele și fragmentele tale într-un singur fișier, de dus pe alt calculator. Conexiunile, aranjarea ferestrei, shell-ul terminalului și fragmentele specifice unei conexiuni rămân aici.
settings-bundle-export = Exportă
settings-bundle-import = Importă
settings-cli = Linie de comandă
settings-cli-install = Instalează
settings-cli-installing = Se instalează
settings-cli-remove = Elimină
settings-cli-removing = Se elimină

## Settings › modal

settings-title = Setări
settings-restore-tabs = Restaurează filele la pornire
settings-restore-tabs-hint = Redeschide filele de interogare din ultima sesiune la pornirea aplicației.

## Settings › Editor

settings-editor-font-size = Dimensiunea fontului
settings-editor-predictions = Predicții în linie
settings-editor-predictions-hint = Arată cuvintele probabile de după cursor — perechea unui cuvânt cheie, cheia străină a unui join, coloanele după care grupezi sau în care inserezi. Tab, sau → la capăt de rând, le acceptă întregi, Ctrl+→ (⌘→ pe macOS) câte un cuvânt, iar Escape le respinge.
settings-editor-remembered = Predicții din instrucțiuni anterioare
settings-editor-remembered-hint = Oferă și restul unui rând dintr-o instrucțiune pe care ai rulat-o, ai scris-o în altă filă sau ai salvat-o ca fragment. Dezactivează pentru a ține instrucțiunile anterioare departe de ecran.
settings-editor-aliases = Aliasuri de tabel
settings-editor-aliases-hint = Când completarea scrie un tabel după FROM sau JOIN, adaugă după el un alias scurt format din inițialele lui — orders o, order_items oi — și folosește aliasul în condiția ON a unui join.
settings-editor-rainbow = Paranteze curcubeu
settings-editor-rainbow-hint = Colorează fiecare pereche de paranteze după adâncimea imbricării, ca o paranteză de închidere să poată fi potrivită dintr-o privire cu cea de deschidere.
settings-editor-whitespace = Arată spațiile albe
settings-editor-whitespace-hint = Desenează spațiile și tabulatorii ca semne estompate, ca un tabulator să poată fi deosebit de spații.
settings-editor-guides = Ghidaje de indentare
settings-editor-guides-hint = Desenează o linie verticală estompată la fiecare nivel de indentare.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Majuscule la cuvinte cheie
settings-format-case-keep = Ca la tastare
settings-format-case-upper = Majuscule
settings-format-case-lower = Minuscule
settings-format-commas-trailing = La capăt de rând
settings-format-commas-leading = La început de rând
settings-format-gap-one = Un rând gol
settings-format-gap-none = Niciun rând gol
settings-format-keyword-case-hint = Cum scrie Formatează codul cuvintele cheie precum SELECT și FROM. Numele, șirurile și comentariile își păstrează forma.
settings-format-function-case = Majuscule la funcții
settings-format-function-case-hint = Cum scrie Formatează codul funcțiile proprii ale motorului, precum COUNT și COALESCE, acolo unde sunt apelate. Funcțiile tale își păstrează forma.
settings-format-type-case = Majuscule la tipuri
settings-format-type-case-hint = Cum scrie Formatează codul tipurile de date proprii ale motorului, precum INT și VARCHAR. Tipurile create de tine își păstrează forma.
settings-format-commas = Virgule
settings-format-commas-hint = Unde pune Formatează codul virgula unei liste pe care o împarte pe mai multe rânduri. Listele ținute pe un rând, precum argumentele unei funcții, rămân neatinse dacă lățimea de încadrare nu le împarte.
settings-format-statement-gap = Între instrucțiuni
settings-format-statement-gap-hint = Rândurile goale pe care Formatează codul le lasă între două instrucțiuni. Ca la tastare păstrează un rând gol oriunde exista unul.
settings-format-wrap = Lățime de încadrare
settings-format-wrap-hint = O listă pe care Formatează codul o ține pe un rând, precum argumentele unei funcții, trece la un element pe rând de îndată ce rândul ar depăși această lățime.
settings-format-river = Aliniază la dreapta cuvintele cheie ale clauzelor
settings-format-river-hint = Aliniază SELECT, FROM, WHERE și celelalte pe marginea lor dreaptă, cu corpul fiecărei clauze lângă cuvântul ei cheie, în loc de un cuvânt cheie pe un rând separat.
settings-format-align = Aliniază coloanele
settings-format-align-hint = Aliniază AS-ul aliasurilor dintr-o listă de selecție, semnul = al atribuirilor unui UPDATE și tipurile coloanelor unui tabel.
settings-format-join-on = Condiția de join pe rând separat
settings-format-join-on-hint = Pune ON-ul unui join, precum și AND și OR din condiția lui, pe rânduri separate sub join.

## Settings › Query

settings-query-row-limit = Limită implicită de rânduri
settings-query-timeout = Timp limită pentru instrucțiuni
settings-query-timeout-hint = Anulează o instrucțiune care rulează mai mult de atât. Dezactivat implicit — un import sau un raport poate dura pe bună dreptate o vreme.
timeout-off = Fără timp limită
timeout-hours = { $n ->
    [one] { $n } oră
    [few] { $n } ore
   *[other] { $n } de ore
}
timeout-minutes = { $n ->
    [one] { $n } minut
    [few] { $n } minute
   *[other] { $n } de minute
}
timeout-seconds = { $n ->
    [one] { $n } secundă
    [few] { $n } secunde
   *[other] { $n } de secunde
}
settings-query-confirm-writes = Confirmă înainte de a rula scrieri
settings-query-confirm-writes-hint = Întreabă înainte de a executa orice instrucțiune care modifică date sau schema.
settings-query-validate = Validare live în baza de date
settings-query-validate-hint = Verifică instrucțiunea de sub cursor în baza de date pe măsură ce tastezi (un PREPARE care nu execută) pentru a afișa erorile exacte. Adaugă un drum dus-întors la baza de date la fiecare pauză.
settings-query-notify = Notifică la terminarea unei rulări în fundal
settings-query-notify-hint = Arată o notificare pe desktop când o interogare, un script, un export sau un dump care a durat cel puțin o secundă se termină în timp ce Schemaic nu este fereastra activă. Butonul din bara de activități este evidențiat oricum. Notificarea nu include niciodată SQL-ul.
notify-query-finished = Interogare terminată
notify-query-failed = Interogare eșuată
notify-script-finished = Script terminat
notify-script-failed = Script eșuat
notify-export-finished = Export terminat
notify-export-failed = Export eșuat
notify-dump-finished = Dump terminat
notify-dump-failed = Dump eșuat
notify-took = A durat { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Păstrează valorile în jurnalul de scrieri
settings-query-write-log-hint = Fiecare scriere pe care o rulează Schemaic sau schemaic exec este adăugată în write_log.jsonl din dosarul de setări și nu este ștearsă niciodată. Valorile din SQL-ul ei sunt mascate dacă opțiunea nu e activă; o parolă este mascată oricum.

## Settings › Appearance

settings-appearance-ui-theme = Tema interfeței
settings-appearance-editor-theme = Tema editorului
settings-appearance-scale = Scara interfeței

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Scurtături

## Shortcuts modal › group headings

shortcuts-group-global = Globale
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Grila de rezultate
shortcuts-group-schema-tree = Arborele schemei
shortcuts-group-ai-panel = Panoul AI
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = Diagramă ER

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Caută oriunde
shortcuts-command-palette = Paleta de comenzi
shortcuts-new-tab = Filă nouă de interogare
shortcuts-close-tab = Închide fila de interogare
shortcuts-next-tab = Fila următoare
shortcuts-previous-tab = Fila anterioară
shortcuts-jump-to-tab = Salt la filă
shortcuts-reopen-tab = Redeschide ultima filă închisă
shortcuts-open-file = Deschide fișier SQL
shortcuts-save-file = Salvează fișierul SQL
shortcuts-save-file-as = Salvează fișierul SQL ca
shortcuts-toggle-schema = Comută panoul schemei
shortcuts-toggle-ai = Comută panoul AI
shortcuts-toggle-terminal = Comută terminalul
shortcuts-run-query = Rulează interogarea
shortcuts-autocomplete = Completare automată
shortcuts-inline-ai = Editare AI în linie
shortcuts-find-in-editor = Caută în editor
shortcuts-find-replace = Caută și înlocuiește
shortcuts-go-to-line = Salt la rând
shortcuts-matching-bracket = Salt la paranteza pereche sau la BEGIN/END
shortcuts-line-comment = Comută comentariul de rând
shortcuts-block-comment = Comută comentariul de bloc
shortcuts-toggle-case = Comută majusculele
shortcuts-join-lines = Unește rândurile
shortcuts-duplicate = Duplică rândul / selecția
shortcuts-cut = Taie rândul / selecția
shortcuts-format-code = Formatează codul
shortcuts-format-statement = Formatează instrucțiunea
shortcuts-single-line = Adu pe un singur rând
shortcuts-move-line = Mută rândul în sus / în jos
shortcuts-quick-fix = Aplică remedierea rapidă de sub cursor
shortcuts-indent = Indentează / deindentează
shortcuts-take-prediction = Acceptă predicția în linie (și →, la capăt de rând) / următorul ei cuvânt
shortcuts-find-in-results = Caută în rezultate
shortcuts-go-to-row = Salt la rând
shortcuts-copy = Copiază
shortcuts-paste-staged = Lipește ca modificări în așteptare
shortcuts-select-all = Selectează tot
shortcuts-first-last-cell = Prima / ultima celulă
shortcuts-edit-cell = Editează celula
shortcuts-next-cell = Celula următoare / anterioară în timpul editării
shortcuts-commit-edits = Fă commit modificărilor
shortcuts-mark-deletion = Marchează rândul pentru ștergere
shortcuts-results-toolbar = Mergi la bara de instrumente a rezultatelor
shortcuts-along-toolbar = Deplasare în bara de instrumente (Esc revine la grilă)
shortcuts-open-selected = Deschide tabelul, coloana sau obiectul selectat
shortcuts-context-menu = Meniul contextual al rândului selectat
shortcuts-send-message = Trimite mesajul
shortcuts-new-line = Rând nou
shortcuts-recall-prompt = Promptul anterior / următor
shortcuts-copy-paste = Copiază / lipește
shortcuts-find-in-diagram = Caută în diagramă

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
key-space = Spațiu

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Comută panoul
palette-toggle-panel-hint = schemă · ai · terminal · istoric interogări · activitate server
palette-panel-schema = Schemă
palette-panel-ai = AI
palette-panel-terminal = Terminal
palette-panel-history = Istoric interogări
palette-panel-activity = Activitate server
palette-new-tab = Filă nouă
palette-export-settings = Exportă setările
palette-import-settings = Importă setările
palette-duplicate-tab = Duplică fila
palette-close-tab = Închide fila
palette-reopen-closed-tab = Redeschide o filă închisă
palette-reopen-closed-tab-hint = <filă>
palette-open-file = Deschide fișier
palette-open-file-hint = un script .sql
palette-save-file = Salvează fișierul
palette-save-file-as = Salvează fișierul ca
palette-next-tab = Fila următoare
palette-previous-tab = Fila anterioară
palette-format-code = Formatează codul
palette-format-statement = Formatează instrucțiunea
palette-format-statement-hint = instrucțiunea de la cursor
palette-make-single-line = Adu pe un singur rând
palette-make-single-line-hint = selecția sau instrucțiunea, pe un rând
palette-run = Rulează
palette-run-hint = rulează toate instrucțiunile
palette-go-to-line = Salt la rândul
palette-go-to-line-hint = <număr>
palette-go-to-statement = Salt la instrucțiunea
palette-search-hint = <căutare>
palette-history = Istoric
palette-clear-history = Golește istoricul
palette-clear-history-hint = conexiunea curentă
palette-ask-ai = Întreabă AI
palette-ask-ai-hint = <prompt>
palette-ask-ai-empty = Scrie un prompt…
palette-terminal = Terminal
palette-terminal-hint = <comandă>
palette-run-in-terminal = Rulează în terminal
palette-terminal-empty = Scrie o comandă…
palette-ui-theme = Tema interfeței
palette-ui-theme-hint = luminoasă · întunecată
palette-editor-theme = Tema editorului
palette-editor-theme-hint = <temă>
palette-font-size = Dimensiunea fontului
palette-increase-font-size = Mărește fontul
palette-decrease-font-size = Micșorează fontul
palette-indent-style = Stilul indentării
palette-indent-style-hint = tabulatori · spații
palette-indent-spaces = Spații
palette-indent-tabs = Tabulatori
palette-indent-width = Lățimea indentării
palette-toggle-word-wrap = Comută încadrarea textului
palette-toggle-whitespace = Comută spațiile albe
palette-toggle-whitespace-hint = arată spațiile și tabulatorii
palette-toggle-indent-guides = Comută ghidajele de indentare
palette-switch-connection = Schimbă conexiunea
palette-switch-connection-hint = <conexiune>
palette-server-variables = Variabile server
palette-server-variables-hint = setările conexiunii active
palette-enter-number = Introdu un număr
palette-nothing-found = Nu s-a găsit nimic

## Command palette › what a result row is

palette-row-snippet = fragment
palette-row-open-tab = filă deschisă
palette-row-history = istoric
palette-row-view-body = corpul vizualizării
palette-row-body = corp { $kind }
palette-row-trigger-on = trigger pe { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Deschide în CLI
tree-open = Deschide
tree-open-in-new-tab = Deschide în filă nouă
tree-copy-name = Copiază numele
tree-copy-qualified-name = Copiază numele calificat
tree-properties = Proprietăți
tree-used-by = Folosit de
tree-live-monitor = Monitor live
tree-er-diagram = Diagramă ER
tree-compare-with = Compară cu
tree-generate = Generează
tree-generate-ddl = Generează DDL
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Reîmprospătează
tree-collapse-all = Restrânge tot
tree-favourite = Adaugă la favorite
tree-unfavourite = Elimină din favorite
tree-colour = Culoare
tree-colour-none = Niciuna
tree-hide = Ascunde
tree-import = Importă
tree-export = Exportă
tree-back-up = Copie de rezervă
tree-restore-from-file = Restaurează din fișier
tree-copy-to = Copiază în
tree-create = Creează
tree-create-table = Tabel
tree-create-view = Vizualizare
tree-create-type = Tip
tree-create-domain = Domeniu
tree-create-sequence = Secvență
tree-create-synonym = Sinonim
tree-create-alias-type = Tip alias
tree-create-xml-schema-collection = Colecție de scheme XML
tree-create-function = Funcție
tree-create-procedure = Procedură
tree-create-event = Eveniment
tree-create-extension = Extensie
tree-create-database = Bază de date
tree-create-schema = Schemă
tree-create-kind = Creează { $kind }
tree-edit-kind = Editează { $kind }
tree-folder-create-extension = Creează extensie
tree-edit-table = Editează tabelul
tree-edit-view = Editează vizualizarea
tree-edit-column = Editează coloana
tree-edit-index = Editează indexul
tree-edit-primary-key = Editează cheia primară
tree-edit-foreign-key = Editează cheia străină
tree-triggers = Triggere
tree-triggers-count = Triggere ({ $n })
tree-refresh-view = Reîmprospătează vizualizarea
tree-truncate = Golește
tree-truncate-title = Golește tabelul
tree-truncate-rows = { $n ->
    [one] Ștergi rândul { $rows } din { $table }? Acțiunea nu poate fi anulată.
    [few] Ștergi toate cele { $rows } rânduri din { $table }? Acțiunea nu poate fi anulată.
   *[other] Ștergi toate cele { $rows } de rânduri din { $table }? Acțiunea nu poate fi anulată.
}
tree-truncate-every-row = Ștergi toate rândurile din { $table }? Acțiunea nu poate fi anulată.
tree-drop = Elimină
tree-drop-index = Elimină indexul
tree-drop-foreign-key = Elimină cheia străină
tree-ai-explain = Explicație AI
tree-users = Utilizatori și privilegii
tree-server-variables = Variabile server
tree-new-database = Creează bază de date
tree-show-table-sizes = Arată dimensiunile tabelelor

## Connection switcher

conn-menu-manage = Gestionează conexiunile

## Shared dialog buttons

dialog-cancel = Anulează
dialog-yes = Da
dialog-no = Nu

## Open-transaction prompt

tx-prompt-title = Tranzacție deschisă
tx-prompt-body = { $tab } are o tranzacție deschisă cu { $n ->
    [one] o instrucțiune
    [few] { $n } instrucțiuni
   *[other] { $n } de instrucțiuni
} în ea. Poți face commit pentru a păstra modificările sau rollback pentru a renunța la ele.
tx-prompt-body-aborted = { $tab } are o tranzacție anulată de o instrucțiune eșuată, cu { $n ->
    [one] o instrucțiune
    [few] { $n } instrucțiuni
   *[other] { $n } de instrucțiuni
} în ea. Se poate face doar rollback, ceea ce renunță la modificări.
tx-prompt-rollback = Rollback
tx-prompt-commit = Commit

## Error modal

error-modal-title = Eroare
error-modal-none = Nicio eroare.
error-modal-ai-fix = Remediere AI
error-modal-explain = Explică

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = interogarea a eșuat: { $error }
db-connection-failed = conectarea a eșuat: { $error }
db-query-cancelled = interogare anulată

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Neconectat — SQL-ul de inițializare al acestei conexiuni a fost refuzat: { $why }
db-refused-transport-entra = Autentificarea Microsoft Entra necesită SSL / TLS la nivelul Verify full: tokenul ei deschide fiecare bază de date Azure SQL la care contul are acces, așa că serverul trebuie să-și dovedească numele înainte să primească unul.
db-refused-transport-aws = Autentificarea AWS IAM necesită SSL / TLS cel puțin la nivelul Require: tokenul ei este o parolă pentru acest server și nu trebuie să traverseze rețeaua necriptat.
db-refused-transport-gcp = Autentificarea Google Cloud IAM necesită SSL / TLS la nivelul Verify CA, cu CA-ul de server al instanței ca Root CA: tokenul ei deschide fiecare instanță Cloud SQL la care contul are acces.
db-refused-transport = Această autentificare necesită un mod SSL / TLS mai strict decât cel al conexiunii.
db-refused-backup-in-terminal = Copia de rezervă a unei baze de date de server se face cu uneltele proprii ale acesteia, în terminal.
db-refused-commit-aborted = tranzacția era deja anulată — o instrucțiune din ea a eșuat sau a fost oprită, așa că un commit ar renunța la lucru în loc să-l salveze. Nu s-a făcut commit la nimic.
db-refused-sqlite-no-manual = Conexiunile SQLite nu acceptă încă modul de tranzacție manual — instrucțiunile rulează și se confirmă pe măsură ce sunt trimise
db-refused-mssql-read-only-auto = O conexiune SQL Server doar pentru citire rulează în modul Auto: SQL Server nu are o tranzacție doar pentru citire în care să țină o filă Manual, iar fiecare instrucțiune rulată în modul Auto este anulată prin rollback
db-refused-client-charset = refuzat: setul de caractere al clientului din sesiune ({ $charset }) poate ascunde un apostrof într-un caracter, așa că verificarea instrucțiunii nu o poate citi la fel ca serverul
db-refused-sql-mode = refuzat: sql_mode al serverului ({ $mode }) citește ghilimelele altfel decât verificarea instrucțiunii și nu a putut fi schimbat pentru această sesiune
db-refused-writing-plan = Planurile rulează aici într-o tranzacție doar pentru citire, iar MySQL și MariaDB nu planifică în ea o instrucțiune care scrie. Planifică în schimb forma SELECT a instrucțiunii.
db-refused-writing-plan-function = Planurile rulează aici într-o tranzacție doar pentru citire, iar MySQL și MariaDB nu planifică în ea o instrucțiune care scrie. O funcție pe care o apelează scrie, lucru pe care un plan de aici nu are voie să-l facă.
db-refused-windows-build-only = Autentificarea Windows este disponibilă doar în versiunea Schemaic pentru Windows.
db-refused-cloud-iam-engine = Autentificarea Cloud IAM este pentru conexiuni PostgreSQL și MySQL.
db-refused-mssql-cancel-needs-kill = SQL Server nu poate anula instrucțiunea altei sesiuni fără a încheia sesiunea — folosește Închide sesiunea.
db-refused-shadowed-function = Refuzat înainte ca instrucțiunea ta să ruleze: { $schema }.{ $name } este o funcție proprie a acestei baze de date cu numele unei funcții predefinite PostgreSQL pe care interogările doar de citire o pot apela, iar PostgreSQL o poate apela în locul ei, așa că verificarea doar de citire nu poate garanta ce ar rula aici o interogare. Scoate { $schema } din calea de căutare a conexiunii sau cere proprietarului bazei de date să o șteargă ori să o redenumească.
db-refused-shadowed-operator = Refuzat înainte ca instrucțiunea ta să ruleze: { $schema } conține operatorul { $name }, iar PostgreSQL îl poate folosi în locul unuia predefinit, așa că verificarea doar de citire nu poate garanta ce ar rula aici o interogare. Scoate { $schema } din calea de căutare a conexiunii sau cere proprietarului bazei de date să îl șteargă.
db-refused-blank-value = { $has_row ->
        [some] Rândul { $row } are o valoare goală sau albă pentru { $column } ({ $sql_type }): SQL Server ar stoca-o ca 0 sau 1900-01-01 în loc s-o refuze. Importă câmpurile goale ca NULL sau completează-le.
       *[none] O valoare goală sau albă nu poate fi scrisă în { $column } ({ $sql_type }): SQL Server ar stoca-o ca 0 sau 1900-01-01 în loc s-o refuze. Setează celula la NULL sau scrie o valoare.
    }
db-refused-code-page = { $has_row ->
        [some] Rândul { $row } are o valoare pentru { $column } ({ $sql_type }) care
       *[none] O valoare pentru { $column } ({ $sql_type })
    } conține { $has_lost ->
        [some] `{ $character }`, pentru care pagina de cod { $code_page } nu are caracter
       *[none] un caracter pentru care pagina ei de cod nu are caracter
    }: SQL Server l-ar stoca drept `?` sau un caracter asemănător în loc să-l refuze. Stochează coloana ca nvarchar sau elimină caracterul.
db-refused-sqlite-no-file = nu este setat niciun fișier de bază de date
db-refused-sqlite-memory = :memory: nu este un fișier de bază de date — SQLite păstrează o bază de date în memorie doar cât trăiește conexiunea care a creat-o, iar această aplicație deschide o conexiune nouă pentru fiecare operație, așa că fiecare scriere ar fi raportată ca salvată și apoi pierdută. Indică calea către un fișier.
db-refused-sqlite-uri = { $file }: acesta este un URI, nu un fișier de bază de date — SQLite ar citi partea de după `?` ca opțiuni de deschidere, iar `?mode=memory` deschide o bază de date temporară care acceptă orice scriere și nu păstrează nimic. Indică calea către fișierul propriu-zis.
db-refused-sqlite-worker = procesul de lucru SQLite a eșuat: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } nu este o coloană a acestui tabel, așa că nu are o valoare implicită de scris
db-refused-sqlite-generated = { $column } este o coloană generată: nu are o valoare implicită la care să fie readusă, iar SQLite o calculează din rând
db-refused-sqlite-sizes-too-large = Dimensiunile tabelelor se citesc doar pentru o bază de date de până la { $cap }: SQLite le măsoară citind fiecare pagină a fișierului, iar acesta are { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = timpul a expirat
db-connect-init-failed = SQL-ul de inițializare `{ $statement }` a eșuat: { $error }
db-connect-session-retired = conexiunea acestei file a fost închisă după o oprire pe care serverul nu a confirmat-o, iar una nouă nu a putut fi deschisă; tranzacția ei a fost anulată prin rollback. Comută fila pe Auto și înapoi pe Manual pentru a deschide alta.
db-connect-begin-failed = tranzacția nu a putut fi începută: { $error }
db-connect-prepare-session = sesiunea nu a putut fi pregătită: { $error }
db-connect-read-only-setup = sesiunea nu a putut fi trecută pe doar citire: { $error }
db-connect-guard-session = sesiunea nu a putut fi protejată: { $error }
db-connect-plan-setup = planul nu a putut fi pregătit: { $error }
db-connect-no-maintenance-db = nicio bază de date de întreținere accesibilă
db-connect-browser-silent = SQL Server Browser de pe { $server } nu a dat niciun port pentru instanța { $instance }: serviciul Browser nu rulează, portul UDP 1434 este blocat pe drum sau nicio instanță de acolo nu are acest nume. Dacă instanța ascultă pe un port fix, setează gazda la { $server } fără \{ $instance } și portul la acela.
db-connect-browser-without-tcp = SQL Server Browser de pe { $server } a răspuns pentru instanța { $instance }, dar nu a dat niciun port TCP: instanța nu ascultă pe TCP. Activează TCP/IP pentru ea în SQL Server Configuration Manager și repornește-o.
db-connect-azure-timed-out = Azure SQL nu a terminat autentificarea în { $secs } secunde. O bază de date serverless aflată în pauză are nevoie de aproximativ un minut ca să repornească, iar această încercare a trezit-o — conectează-te din nou în scurt timp.
db-connect-azure-unavailable = { $server } O bază de date serverless aflată în pauză are nevoie de aproximativ un minut ca să repornească, iar această încercare a trezit-o — conectează-te din nou în scurt timp.
db-ssh-connect-no-answer = Conectare SSH cu { $host }: niciun răspuns în { $secs } s
db-ssh-auth-no-answer = Autentificare SSH cu { $host }: niciun răspuns în { $secs } s
db-ssh-auth-error = Eroare de autentificare SSH: { $error }
db-ssh-auth-failed = Autentificarea SSH a eșuat
db-ssh-key-load = Încărcarea cheii SSH a eșuat: { $error }
db-ssh-key-auth = Eroare de autentificare cu cheia SSH: { $error }
db-ssh-agent = Eroare a agentului SSH: { $error }
db-ssh-agent-empty = Agentul SSH nu are identități încărcate (rulează `ssh-add`)
db-ssh-agent-auth = Eroare de autentificare prin agentul SSH: { $error }
db-ssh-no-agent = nu s-a găsit niciun agent SSH (pornește serviciul OpenSSH Authentication Agent sau rulează Pageant)
db-ssh-no-agent-socket = niciun agent SSH ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = Gazda intermediară SSH nu a putut ajunge la { $host }: { $error }
db-ssh-connect-to = Conectarea SSH la { $host } a eșuat: { $error }
db-ssh-connect = Conectarea SSH a eșuat: { $error }
db-ssh-no-host = Tunelul SSH nu are gazdă
db-ssh-bind = legarea tunelului local a eșuat: { $error }
db-ssh-named-instance = O instanță cu nume nu poate fi accesată printr-un tunel SSH: SQL Server Browser răspunde cu portul ei prin UDP, pe care tunelul nu îl transportă. Setează gazda la { $server } fără \{ $instance } și portul la cel pe care ascultă instanța.
db-ssh-host-key-changed = Cheia de gazdă SSH pentru { $host } s-a SCHIMBAT de când Schemaic a considerat-o de încredere prima dată.
db-ssh-host-key-expected = Așteptată { $key }
db-ssh-host-key-offered = Oferită   { $key }
db-ssh-host-key-none = (nicio cheie înregistrată)
db-ssh-host-key-changed-advice = Asta poate însemna că serverul a fost reconstruit sau cheia i-a fost schimbată — sau că ceva interceptează conexiunea. Nu i s-a trimis nimic. Verifică noua amprentă pe alt canal (pe consola serverului sau cu cine îl administrează) înainte de a o considera de încredere. După aceea, elimină intrarea acestei gazde din { $store } din directorul de configurare Schemaic pentru a înregistra noua cheie.
db-ssh-host-key-store = Schemaic nu a putut citi evidența cheilor de gazdă SSH de încredere ({ $store }), așa că nu poate spune dacă cheia lui { $host } ({ $key }) este cea în care ai avut încredere înainte.
db-ssh-host-key-store-advice = Conexiunea a fost refuzată în loc ca cheia să fie acceptată din prima. Repară sau șterge { $store } din directorul de configurare Schemaic — ștergerea înseamnă că fiecare gazdă SSH va fi considerată din nou de încredere la următoarea conectare, așa că fă asta doar dacă poți verifica amprentele.
db-tls-setup = Configurarea TLS a eșuat: { $error }
db-tls-client-cert = certificatul clientului a fost respins: { $error }
db-tls-trust-store = depozitul de încredere TLS este inutilizabil: { $error }
db-tls-bad-name = { $name } nu este un nume pe care un certificat îl poate purta: { $error }
db-tls-ca-empty = Fișierul CA { $path } nu conține niciun certificat
db-tls-ca-unusable = Fișierul CA { $path } conține un certificat care nu poate fi folosit ca ancoră de încredere
db-tls-cert-unreadable = fișierul de certificat { $path } nu poate fi citit: { $error }
db-tls-cert-not-pem = { $path } nu este un fișier de certificat PEM: { $error }
db-tls-cert-is-der = { $path } nu conține niciun certificat PEM — un fișier DER trebuie mai întâi convertit (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = fișierul de cheie { $path } nu poate fi citit: { $error }
db-tls-key-encrypted = { $path } este o cheie privată protejată cu frază de acces, pe care Schemaic nu o poate deschide. Decriptează-o mai întâi: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } nu este o cheie privată PEM: { $error }
db-sign-in-no-value = { $field ->
        [host] Conexiunea nu are o gazdă cu care să se autentifice.
       *[user] Conexiunea nu are un utilizator cu care să se autentifice.
    }
db-sign-in-leading-dash = { $field ->
        [host] Gazda conexiunii începe cu „-”, pe care CLI-ul cloud l-ar citi ca opțiune, nu ca gazdă.
       *[user] Utilizatorul conexiunii începe cu „-”, pe care CLI-ul cloud l-ar citi ca opțiune, nu ca utilizator.
    }
db-sign-in-control = { $field ->
        [host] Gazda conexiunii conține un caracter de control.
       *[user] Utilizatorul conexiunii conține un caracter de control.
    }
db-sign-in-shim-aws = AWS CLI găsit este un shim .cmd (versiunea 1 instalată cu pip), iar Schemaic nu rulează un shim prin cmd.exe. Instalează AWS CLI versiunea 2 sau autentifică-te cu o parolă.
db-sign-in-shim-gcloud = gcloud găsit este un shim .cmd fără un Python inclus alături, iar Schemaic nu rulează un shim prin cmd.exe. Reinstalează Google Cloud CLI cu programul lui de instalare sau autentifică-te cu o parolă.
db-sign-in-shim-azure = Azure CLI găsit este un shim .cmd/.bat fără Python alături, iar Schemaic nu rulează un shim prin cmd.exe. Reinstalează Azure CLI cu programul de instalare Microsoft sau autentifică-te cu un login SQL.
db-sign-in-no-token = CLI-ul cloud a răspuns fără niciun token.
db-sign-in-more-than-token = CLI-ul cloud a răspuns cu mai mult decât un token.
db-sign-in-azure-bad-answer = Răspunsul Azure CLI nu a fost tokenul pe care trebuia să-l afișeze.
db-sign-in-azure-no-access-token = Azure CLI a răspuns fără un token de acces.
db-sign-in-signed-out-aws = AWS CLI nu are credențiale. Rulează `aws configure` sau `aws sso login`, apoi conectează-te din nou.
db-sign-in-signed-out-gcloud = Google Cloud CLI nu este autentificat. Rulează `gcloud auth login`, apoi conectează-te din nou.
db-sign-in-signed-out-azure = Azure CLI nu este autentificat. Rulează `az login`, apoi conectează-te din nou.
db-sign-in-no-token-from = { $cli } nu a putut obține un token: { $line }
db-sign-in-no-token-from-silent = { $cli } nu a putut obține un token.
db-sign-in-no-token-from-azure-silent = Azure CLI nu a putut obține un token: nu a dat niciun motiv
db-sign-in-not-found = { $cli } nu a fost găsit. Instalează-l și autentifică-te cu el sau autentifică-te cu o parolă.
db-sign-in-not-found-azure = Azure CLI (az) nu a fost găsit. Instalează-l și rulează `az login` sau autentifică-te cu un login SQL.
db-sign-in-no-answer = { $cli } nu a răspuns în { $secs } secunde.
db-sign-in-not-started = { $cli } nu a putut fi pornit: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = exportul a încetat să citească
db-query-no-rows-to-export = acea instrucțiune nu returnează rânduri de exportat
db-query-no-server-settings = Acest motor nu are setări de server de afișat.
db-query-no-status-list = Acest motor nu publică o listă de stare — doar variabilele sale.
db-query-no-import-columns = Nicio coloană în care să se importe
db-query-read-stopped = Citirea s-a oprit: { $error }
db-query-batch-continued = SQL Server continuă un lot după eroarea unei instrucțiuni, cu excepția cazului în care eroarea încheie lotul, așa că instrucțiunile de după cea eșuată pot să fi rulat.
db-query-sqlite-locked = În SQLite o citire lungă blochează o scriere: un export sau un import al unui tabel întreg ține o blocare de citire pe fișier până se termină, iar nicio scriere nu poate începe până atunci (MySQL și PostgreSQL nu funcționează așa). Așteaptă sau anulează operația, apoi încearcă din nou. Ca citirile și scrierile să se poată suprapune definitiv pe acest fișier, trece-l pe modul de jurnal WAL — `PRAGMA journal_mode = WAL` — care modifică fișierul însuși, așa că Schemaic nu o va face în locul tău.
db-query-rollback-failed-too = { $error } — iar rollback-ul a eșuat și el: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = instrucțiunea { $n } a eșuat: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } instrucțiune anterioară a fost deja aplicată și nu poate fi anulată prin rollback
    [few] { $failed } — { $n } instrucțiuni anterioare au fost deja aplicate și nu pot fi anulate prin rollback
   *[other] { $failed } — { $n } de instrucțiuni anterioare au fost deja aplicate și nu pot fi anulate prin rollback
}
ddl-cancelled = planul a fost anulat
ddl-dangling-foreign-key = planul lasă o cheie străină care nu indică nimic: { $row }
ddl-breaks-view = planul ar strica vizualizarea { $view }, care citește o coloană pe care o redenumește sau o elimină ({ $why }). Nu s-a modificat nimic. Editează sau elimină mai întâi vizualizarea.
ddl-no-databases = SQLite nu are baze de date de creat sau de eliminat — acolo o bază de date este un fișier, pe care Schemaic nu îl creează și nu îl șterge în locul tău.
ddl-elsewhere-failed = Vizualizările din alte baze de date care fac select * din { $table } nu au fost reîmprospătate ({ $error }) și citesc și scriu coloanele greșite până când vor fi (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } vizualizare dintr-o altă bază de date care face select * din { $table } nu a putut fi reîmprospătată și citește și scrie coloanele greșite până când va fi (sp_refreshview): { $views }.
        [few] { $count } vizualizări dintr-o altă bază de date care fac select * din { $table } nu au putut fi reîmprospătate și citesc și scriu coloanele greșite până când vor fi (sp_refreshview): { $views }.
       *[other] { $count } de vizualizări dintr-o altă bază de date care fac select * din { $table } nu au putut fi reîmprospătate și citesc și scriu coloanele greșite până când vor fi (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } bază de date nu a fost căutată pentru o vizualizare care face select * din { $table }: { $databases }.
        [few] { $count } baze de date nu au fost căutate pentru o vizualizare care face select * din { $table }: { $databases }.
       *[other] { $count } de baze de date nu au fost căutate pentru o vizualizare care face select * din { $table }: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] {" "}și încă { $more }
    }
ddl-rollback-failed = { $error } — iar rollback-ul a eșuat și el: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = ștergerea din { $table } a afectat rânduri: { $affected } (se aștepta exact 1)
write-one-row-update = actualizarea în { $table } a afectat rânduri: { $affected } (se aștepta exact 1)
write-one-row-insert = inserarea în { $table } a afectat rânduri: { $affected } (se aștepta exact 1)
write-batch-inserted = un lot de rânduri ({ $rows }) a inserat { $wrote }
write-batch-affected = un lot de rânduri ({ $rows }) a afectat { $wrote }
write-row-issue = rândul { $line }, coloana { $column }: { $problem } ({ $text })
write-row-issue-empty = rândul { $line }, coloana { $column }: { $problem } (gol)
write-copy-too-long = { $column } conține mai mult text decât poate transporta o citire, așa că copierea s-a oprit înainte de a-l scrie
write-copy-too-long-unnamed = o coloană conține mai mult text decât poate transporta o citire, așa că copierea s-a oprit înainte de a-l scrie
write-copy-bytes = { $column } conține octeți pe care copierea nu îi poate citi așa cum sunt, așa că s-a oprit înainte de a-i scrie
write-copy-not-its-form = valoarea citită pentru { $column } nu are forma cerută de copiere, așa că nu s-a scris nimic
write-copy-not-its-form-unnamed = valoarea citită pentru o coloană nu are forma cerută de copiere, așa că nu s-a scris nimic
write-commit-cancelled = Commit anulat
write-import-cancelled = Import anulat
write-rollback-complete = {" "}— s-a făcut rollback la toate modificările
write-rollback-incomplete = {" "}— rollback-ul NU le-a anulat: motorul de stocare al acestui tabel nu este tranzacțional, așa că rândurile deja scrise rămân. Verifică tabelul înainte de a reîncerca.
write-rollback-unknown = {" "}— iar rollback-ul nu a putut fi confirmat: conexiunea a eșuat înainte ca serverul să răspundă. Rândurile pot sau nu să fi rămas. Verifică tabelul înainte de a reîncerca.

## Schema tree › the panel

tree-panel-title = SCHEMĂ
tree-tip-databases = Arată sau ascunde bazele de date
tree-tip-options = Opțiuni pentru schemă
tree-search-placeholder = Caută…
tree-loading = Se încarcă
tree-no-tables = Niciun tabel
tree-failed-to-connect = Conectarea a eșuat
tree-failed-view = Vezi
tree-single-user-unread = Un singur utilizator — necitită
tree-single-user-read = Citește
tree-count-tables = { $n ->
    [one] { $n } tabel
    [few] { $n } tabele
   *[other] { $n } de tabele
}
tree-count-cols = { $n ->
    [one] { $n } col.
   *[other] { $n } col.
}
tree-count-keys = { $n ->
    [one] { $n } cheie
    [few] { $n } chei
   *[other] { $n } de chei
}
tree-count-partitions = { $n ->
    [one] { $n } partiție
    [few] { $n } partiții
   *[other] { $n } de partiții
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = tip
kind-domain = domeniu
kind-sequence = secvență
kind-function = funcție
kind-procedure = procedură
kind-event = eveniment
kind-synonym = sinonim
kind-alias-type = tip alias
kind-xml-schema-collection = colecție de scheme XML
kind-extension = extensie
kind-group-type = Tipuri
kind-group-domain = Domenii
kind-group-sequence = Secvențe
kind-group-function = Funcții
kind-group-procedure = Proceduri
kind-group-event = Evenimente
kind-group-synonym = Sinonime
kind-group-alias-type = Tipuri alias
kind-group-xml-schema-collection = Colecții de scheme XML
kind-group-extension = Extensii

## Query history panel

history-title = ISTORIC INTEROGĂRI
history-clear-tip = Golește istoricul acestei conexiuni…
history-clear-title = Golește istoricul interogărilor
history-clear-message = Ștergi { $n } { $n ->
    [one] interogare înregistrată
    [few] interogări înregistrate
   *[other] de interogări înregistrate
} pentru această conexiune? Acțiunea nu poate fi anulată.
history-scope-current = Curentă
history-scope-all = Toate
history-cut-note = Se afișează cele mai noi { $shown } din { $matched } — caută pentru a restrânge.
history-empty = Nicio interogare încă.
history-no-match = Nicio interogare potrivită.
history-open-in-new-tab = Deschide în filă nouă
history-delete = Șterge
history-failed = Eșuată

## Snippet library panel

snippets-title = BIBLIOTECĂ DE FRAGMENTE
snippets-save-tip = Salvează SQL-ul din editor ca fragment
snippets-no-match = Niciun fragment potrivit.
snippets-band-this-connection = ACEASTĂ CONEXIUNE
snippets-band-all-connections = TOATE CONEXIUNILE
snippets-builtin = Predefinit
snippets-name-placeholder = Numele fragmentului
snippets-insert = Inserează în editor
snippets-open-in-new-tab = Deschide în filă nouă
snippets-edit = Editează
snippets-duplicate = Duplică
snippets-delete = Șterge
snippets-show-in = Arată în
snippets-scope-this-connection = Această conexiune
snippets-scope-engine = Fiecare conexiune { $engine }
snippets-scope-all-connections = Toate conexiunile

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = AZI
history-band-this-week = SĂPTĂMÂNA ACEASTA
history-band-earlier = MAI DEMULT

## Query tab strip

tabs-tip-unsaved = { $path } — modificări nesalvate
tabs-pin = Fixează
tabs-unpin = Anulează fixarea
tabs-rename = Redenumește
tabs-duplicate = Duplică
tabs-close = Închide
tabs-open-file = Deschide fișier
tabs-save = Salvează
tabs-save-as = Salvează ca
tabs-reload = Reîncarcă de pe disc
tabs-reopen-last = Redeschide ultima filă
tabs-close-others = Închide celelalte file
tabs-close-all = Închide toate filele

## Server Activity panel

activity-title = ACTIVITATE SERVER
activity-refresh-tip = Reîmprospătează acum
activity-auto-refresh-off = Reîmprospătare automată oprită
activity-auto-refresh-every = Reîmprospătare automată la fiecare { $secs } s
activity-interval-off = Dezactivat
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } h { $m } min
activity-age-days = { $d } z { $h } h
activity-view-sessions = Sesiuni
activity-view-locks = Blocări
activity-search-sessions = Caută sesiuni…
activity-search-locks = Caută blocări…
activity-loading = Se încarcă…
activity-sqlite-no-sessions = SQLite nu are sesiuni de server — rulează în interiorul acestui proces, așa că nu există nimic altceva conectat de văzut.
activity-sessions-no-match = Nicio sesiune potrivită.
activity-sessions-none = Nicio altă sesiune pe acest server.
activity-sessions-undrawn = { $n ->
    [one] Încă { $n } sesiune se potrivește — restrânge căutarea ca s-o vezi.
    [few] Încă { $n } sesiuni se potrivesc — restrânge căutarea ca să le vezi.
   *[other] Încă { $n } de sesiuni se potrivesc — restrânge căutarea ca să le vezi.
}
activity-locks-no-match = Nicio blocare potrivită.
activity-locks-no-waits = Nicio așteptare de blocare pe acest server.
activity-locks-none = Nicio altă sesiune nu deține o blocare.
activity-locks-contended-only = Acest server listează doar blocările așteptate sau care țin în loc o așteptare — nu fiecare blocare a unei sesiuni.
activity-locks-undrawn = { $n ->
    [one] Încă { $n } blocare se potrivește — restrânge căutarea ca s-o vezi.
    [few] Încă { $n } blocări se potrivesc — restrânge căutarea ca să le vezi.
   *[other] Încă { $n } de blocări se potrivesc — restrânge căutarea ca să le vezi.
}
activity-list-capped = listă limitată
activity-locks-total = { $n ->
    [one] { $shown } blocare
    [few] { $shown } blocări
   *[other] { $shown } de blocări
}
activity-locks-waiting = { $n ->
    [one] { $shown } în așteptare
   *[other] { $shown } în așteptare
}
activity-state-blocked = Blocată
activity-state-idle-in-tx = Inactivă în tranz.
activity-state-running = Rulează
activity-state-idle = Inactivă
activity-count-sessions = { $n ->
    [one] { $figure } sesiune
    [few] { $figure } sesiuni
   *[other] { $figure } de sesiuni
}
activity-count-running = { $figure } rulează
activity-count-blocked = { $figure } blocate
activity-count-idle-in-tx = { $figure } inactive în tranz.
activity-lock-wait = Așteptare de blocare
activity-lock-wait-sentence = Sesiunea { $waiter } ({ $waiter_who }) { $waited ->
        [none] așteaptă
       *[other] așteaptă de { $waited }
    } o blocare deținută de sesiunea { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] inactivă în tranzacție
        [blocked] ea însăși blocată
        [running] care rulează
       *[idle] inactivă
    }{ $held ->
        [none] {""}
       *[other] {" "}de { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}Încă { $others } sesiune deține o blocare de care are nevoie.
        [few] {" "}Încă { $others } sesiuni dețin o blocare de care are nevoie.
       *[other] {" "}Încă { $others } de sesiuni dețin o blocare de care are nevoie.
    }
activity-kill-holder = Închide { $id }
activity-terminate-tip = Închide sesiunea { $id }
activity-read-only-tip = Această conexiune este marcată ca doar pentru citire.
activity-lock-holds = deține
activity-lock-waits-for = așteaptă
activity-lock-held-by = deținută de { $ids }
activity-copy-statement = Copiază instrucțiunea
activity-cancel-query = Anulează interogarea
activity-kill-session = Închide sesiunea
activity-waiting-on = așteaptă o blocare deținută de { $ids }
activity-blocks = { $n ->
        [one] blochează { $n } sesiune
        [few] blochează { $n } sesiuni
       *[other] blochează { $n } de sesiuni
    }
activity-read-only-cancel = Această conexiune este marcată ca doar pentru citire, așa că Schemaic nu anulează interogări pe ea. Dezactivează mai întâi doar citire în setările conexiunii.
activity-read-only-terminate = Această conexiune este marcată ca doar pentru citire, așa că Schemaic nu închide sesiuni pe ea. Dezactivează mai întâi doar citire în setările conexiunii.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ASISTENT AI
ai-new-chat-tip = Conversație nouă
ai-settings-tip = Setări AI
ai-empty-ask = Întreabă despre SQL-ul tău…
ai-empty-not-connected = { $cli } nu este conectat.
ai-input-placeholder = Mesaj…
ai-input-off = Asistentul AI este dezactivat
ai-input-db-down = Neconectat la baza de date
ai-speaker-you = TU
ai-result-more-lines = { $n ->
    [one] … (+{ $n } rând)
    [few] … (+{ $n } rânduri)
   *[other] … (+{ $n } de rânduri)
}

## Shared dialog buttons (continued)

dialog-close = Închide

## Chart modal

chart-title = Grafic — { $tab }
chart-caption-kind = Grafic
chart-caption-values = Valori
chart-caption-labels = Etichete
chart-row-number = Numărul rândului
chart-kind-bar = Bare
chart-kind-line = Linie
chart-kind-histogram = Histogramă
chart-save-tip = Salvează graficul…
chart-save-dialog = Salvează graficul
chart-file-stem = grafic { $name }
chart-nothing-to-save = Nimic de salvat.
chart-no-numbers = Nicio coloană de numere de reprezentat în acest rezultat.
chart-all-null = Fiecare valoare din acea coloană este NULL.
chart-too-large = Aceste valori sunt prea mari pentru a fi desenate.
chart-caption-rows = { $n ->
    [one] { $n } rând
    [few] { $n } rânduri
   *[other] { $n } de rânduri
}
chart-caption-skipped-null = { $skipped } NULL, nedesenate
chart-caption-skipped = { $skipped } nedesenate (NULL sau peste { $max })
chart-caption-truncated = rândurile citite, nu întregul rezultat

## Schema editors — shared

editor-preview-sql = Previzualizează SQL
editor-change-count = { $n ->
    [0] Nicio modificare
    [one] 1 modificare
    [few] { $n } modificări
   *[other] { $n } de modificări
}

## View editor

view-editor-title-edit = Editează vizualizarea { $view }
view-problem-no-name = Vizualizarea are nevoie de un nume.
view-problem-no-select = O vizualizare are nevoie de un SELECT care s-o definească.
view-problem-not-a-query = Corpul unei vizualizări trebuie să fie o interogare — începe cu SELECT, WITH, VALUES sau TABLE.
view-problem-materialized = Schemaic nu poate edita o vizualizare materializată — PostgreSQL nu are CREATE OR REPLACE pentru ea.
view-problem-hidden = Definiția acestei vizualizări este criptată sau ascunsă pentru acest login, așa că poate fi eliminată aici, dar nu editată.
view-problem-verbatim = Antetul acestei vizualizări spune ceva ce Schemaic nu poate reformula, așa că poate fi eliminată aici, dar nu editată.
view-editor-title-create = Creează vizualizare în { $location }
view-editor-section-view = Vizualizare
view-editor-section-definition = Definiție
view-editor-section-options = Opțiuni
view-editor-name = Nume
view-editor-select = Instrucțiune SELECT
view-editor-check-option = Opțiune de verificare
view-editor-column-names = Numele coloanelor
view-editor-column-names-placeholder = opțional — preluate din SELECT dacă e gol
view-editor-sql-security = Securitate SQL
view-editor-definer = Definitor
view-editor-recreate = Recreează în loc de înlocuire
view-editor-recreate-hint = PostgreSQL poate doar adăuga coloane la o vizualizare existentă. Redenumirea, schimbarea tipului sau reordonarea uneia necesită un DROP și un CREATE, care iau cu ele vizualizările dependente și permisiunile acordate.

## Table properties panel

props-title = Proprietăți — { $table }
props-reading = Se citesc statisticile
props-no-stats-view = O vizualizare nu are stocare proprie, așa că serverul nu raportează statistici pentru ea.
props-no-stats-table = Serverul nu a raportat statistici pentru acest tabel.
props-on-disk = pe disc
props-rows-estimated = rânduri (estimat)
props-rows-counted = rânduri (numărate)
props-rows-not-reported = rânduri — neraportat
props-counting = Se numără
props-count-rows = Numără rândurile
props-count-slow = O scanare completă a tabelului. Lentă pe unul mare.
props-storage-data = Date
props-storage-indexes = Indecși
props-storage-free = Liber
props-section-structure = Structură
props-columns = Coloane
props-indexes = Indecși
props-foreign-keys = Chei străine
props-count-columns = { $n ->
    [one] { $n } coloană
    [few] { $n } coloane
   *[other] { $n } de coloane
}
props-count-indexes = { $n ->
    [one] { $n } index
    [few] { $n } indecși
   *[other] { $n } de indecși
}
props-count-keys = { $n ->
    [one] { $n } cheie
    [few] { $n } chei
   *[other] { $n } de chei
}
props-type = Tip
props-type-view = Vizualizare
props-type-table = Tabel
props-section-options = Opțiuni
props-engine = Motor
props-row-format = Format de rând
props-collation = Colaționare
props-next-auto-increment = Următorul auto-increment
props-created = Creat
props-updated = Actualizat
props-comment = Comentariu
props-section-partitioning = Partiționare
props-partition-of = Partiție a
props-partition-bound = Limită
props-partitioned-by = Partiționat după
props-partitions-more = { $n ->
    [one] și încă { $n } partiție
    [few] și încă { $n } partiții
   *[other] și încă { $n } de partiții
}
props-section-indexes = Indecși
props-index-cardinality = cardinalitate { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } scanare
        [few] { $shown } scanări
       *[other] { $shown } de scanări
    }
props-index-not-counted = utilizare nenumărată
props-index-unused = nefolosit niciodată de la resetarea contoarelor serverului
props-never-analysed = Acest tabel nu a fost analizat niciodată, așa că serverul nu are o estimare a rândurilor pentru el. Rulează ANALYZE sau numără rândurile.
props-analysed-at = Estimat la ultimul ANALYZE, { $when }.
props-read-live = Citit live — acest server are information_schema_stats_expiry = 0.
props-cached-up-to = Memorat în cache de server: aceste cifre pot avea o vechime de până la { $age } (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } h
props-age-days = { $n } z
props-vacuum-note = { $pct }% din versiunile de rând ale acestui tabel sunt moarte și încă nerecuperate. Un VACUUM ar elibera acel spațiu.
props-copy = Copiază

## Shared panel search box

panel-search-placeholder = Caută…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = chiar acum
ago-minutes = acum { $n } min
ago-hours = acum { $n } h
ago-days = acum { $n } z
ago-weeks = acum { $n } săpt.

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } rând
    [few] { $n } rânduri
   *[other] { $n } de rânduri
}
outcome-rows-capped = { $n }+ rânduri

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = Deschide CLI-ul bazei de date
term-db-cli-tip-read-only = Deschide CLI-ul bazei de date — această conexiune este marcată ca doar pentru citire
term-restart-tip = Repornește terminalul
term-settings-tip = Setările terminalului

## Results panel header

results-title = REZULTATE
results-properties-tip = Proprietățile tabelului…
results-monitor-tip = Monitor live…
results-restore-editor-tip = Restaurează editorul
results-collapse-editor-tip = Restrânge editorul

## Query panel header

query-title = INTEROGARE
query-no-database = Nicio bază de date

## Shared dialog buttons (continued)

dialog-stop = Oprește

## Copy table modal

copy-table-title = Copiază { $table }
copy-table-rows-line = Apoi copiază fiecare rând din { $table } în el (neafișat ca SQL)
copy-table-into = Copiază în
copy-table-schema = Schemă
copy-table-name = Nume
copy-table-back = ‹ înapoi
copy-table-data = Copiază datele
copy-table-data-hint = Copiază rândurile după ce tabelul este creat. Dezactivat, se copiază doar structura.
copy-table-what-with-rows = Structura și rândurile. Cheile străine și triggerele nu sunt copiate, iar o coloană generată este recalculată.
copy-table-what-structure = Doar structura. Cheile străine și triggerele nu sunt copiate.
copy-table-choose-database = Alege baza de date în care să copiezi.
copy-table-unnamed = Dă un nume copiei.
copy-table-onto-itself = Un tabel nu poate fi copiat peste el însuși — dă copiei alt nume.
copy-table-connection-changed = Conexiunea activă s-a schimbat — redeschide Copiază în din tabel.
copy-table-schema-not-loaded = Schema bazei { $database } nu este încărcată — extinde-o în arbore, apoi încearcă din nou.
copy-table-preview-subject = Copie a { $table }
copy-table-copying = Se copiază rândurile… { $n }
copy-table-copied = { $n ->
    [one] S-a copiat { $n } rând.
    [few] S-au copiat { $n } rânduri.
   *[other] S-au copiat { $n } de rânduri.
}
copy-table-stopped = Oprit înainte ca toate rândurile să fie copiate. Tabelul a fost creat.

## Database, schema and extension editor

database-editor-title-database = Creează bază de date
database-problem-no-name = Numele nu poate fi gol.
database-problem-bad-collation = Numele unei colaționări conține doar litere, cifre și caractere de subliniere.
database-editor-title-schema = Creează schemă
database-editor-title-extension = Creează extensie
database-editor-title-schema-in = Creează schemă în { $database }
database-editor-title-extension-in = Creează extensie în { $database }
database-editor-section-database = Bază de date
database-editor-section-schema = Schemă
database-editor-section-extension = Extensie
database-editor-name = Nume
database-editor-charset = Set de caractere
database-editor-collation = Colaționare
database-editor-owner = Proprietar
database-editor-server-default = implicit pe server
database-editor-owner-you = tu
database-editor-no-extensions = Nu mai sunt extensii de instalat
database-editor-no-charsets = Niciun set de caractere de sugerat
database-editor-no-collations = Nicio colaționare de sugerat
database-editor-no-roles = Niciun rol pe acest server

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importă în { $database }
script-dialog-title = Rulează un script SQL
script-section = Script SQL
script-choose-file = Alege un fișier…
script-no-file = Niciun fișier ales
script-reading = Se citește fișierul
script-hint = Scriptul poate crea și selecta altă bază de date decât cea în care imporți. Poate crea și mai multe baze de date. Verifică scriptul ca să te asiguri că face ce aștepți.
script-hint-namespace = Scriptul poate crea și selecta altă bază de date decât cea în care imporți. Poate crea și mai multe baze de date. Deschiderea din { $namespace } nici nu pune scriptul acolo: tot ce fișierul nu califică ajunge unde spune search_path al serverului. Verifică scriptul ca să te asiguri că face ce aștepți.
script-what-it-does = Ce face acest fișier
script-probe-nothing = Acest fișier nu conține instrucțiuni pe care Schemaic le poate rula.
script-probe-cut-off = Primii { $size } din acest fișier sunt o singură instrucțiune neterminată, așa că nu există încă nimic de rezumat. Rularea trimite fișierul întreg.
script-probe-counted = { $n ->
    [one] { $count } instrucțiune
    [few] { $count } instrucțiuni
   *[other] { $count } de instrucțiuni
} citite din primii { $size }.
script-destroys = { $n ->
    [one] { $count } instrucțiune din acest fișier distruge
    [few] { $count } instrucțiuni din acest fișier distrug
   *[other] { $count } de instrucțiuni din acest fișier distrug
} date — DROP, TRUNCATE sau DELETE. Acest lucru nu poate fi anulat de aici.
script-unread = Doar primii { $size } din acest fișier au fost citiți și nimic din ei nu distruge date — dar restul nu a fost verificat. Poate face DROP, TRUNCATE sau DELETE, iar asta nu poate fi anulat de aici.
script-unqualified = { $n ->
    [one] { $count } instrucțiune din acest fișier acționează
    [few] { $count } instrucțiuni din acest fișier acționează
   *[other] { $count } de instrucțiuni din acest fișier acționează
} asupra fiecărui rând al unui tabel — un DELETE, UPDATE sau TRUNCATE fără WHERE. Acest lucru nu poate fi anulat de aici.
script-tx-own = Acest fișier își deschide propria tranzacție, așa că se aplică fie în întregime, fie deloc. Schemaic nu adaugă una proprie.
script-tx-none = Acest fișier nu deschide nicio tranzacție, așa că un eșec la jumătate lasă în vigoare ce a rulat înainte.
script-connecting = Se conectează
script-progress-of = { $done } din { $total }
script-progress-read = { $done } citiți
script-run = Rulează
script-done = { $n ->
    [one] S-a rulat { $n } instrucțiune
    [few] S-au rulat { $n } instrucțiuni
   *[other] S-au rulat { $n } de instrucțiuni
} din { $file }.
script-stopped = { $n ->
    [one] Oprit după { $n } instrucțiune
    [few] Oprit după { $n } instrucțiuni
   *[other] Oprit după { $n } de instrucțiuni
}.
script-failed = Eșuat: { $message } — { $n ->
    [one] { $n } instrucțiune a rulat
    [few] { $n } instrucțiuni au rulat
   *[other] { $n } de instrucțiuni au rulat
} înainte.
script-failed-at = Eșuat la rândul { $line } din { $file }: { $message } — { $n ->
    [one] { $n } instrucțiune a rulat
    [few] { $n } instrucțiuni au rulat
   *[other] { $n } de instrucțiuni au rulat
} înainte.
script-durability-rolled-back = { $file } și-a deschis propria tranzacție, așa că serverul a făcut rollback la întreaga rulare — nu s-a aplicat nimic, nici instrucțiunile reușite.
script-durability-applied = { $file } nu a deschis nicio tranzacție, așa că fiecare instrucțiune care a rulat rămâne aplicată.
script-durability-applied-before = Orice instrucțiune care a rulat înainte ca { $file } să-și deschidă propria tranzacție rămâne aplicată; ce a rulat în tranzacție a fost anulat prin rollback.
script-durability-unknown = Orice instrucțiune care a rulat deja rămâne aplicată, cu excepția cazului în care { $file } și-a deschis propria tranzacție.

## Schema comparison modal

compare-title = Compară schemele
compare-choose-database = Alege o bază de date
compare-no-connection = Nicio conexiune a acestui motor cu care să compari.
compare-will-change = va fi modificată
compare-source-of-truth = sursa de referință
compare-idle = Alege o bază de date cu care să compari.
compare-loading = Se citesc ambele scheme
compare-select-object = Selectează un obiect pentru a vedea ambele părți.
compare-fk-cycle-note = Cheile străine ale acestor tabele formează un ciclu — nicio ordine de creare nu le satisface pe toate, așa că o migrare generată poate fi refuzată la jumătate.
compare-same-but-stored-differently = Acestea coincid structural — diferența de mai jos ține de modul în care motorul stochează instrucțiunea și nu există nimic de migrat.
compare-summary = { $differ } diferite · { $only_here } doar aici · { $only_there } doar acolo · { $same } identice
compare-summary-uncertain = { $n ->
    [one] { $n } potrivire
    [few] { $n } potriviri
   *[other] { $n } de potriviri
} pe care comparația nu le poate garanta
compare-filter-placeholder = Filtrează după nume
compare-include-identical = Include identicele
compare-select-all = Selectează tot
compare-select-none = Nimic
compare-hint-held-back = reținut din plan; nota de mai sus explică de ce
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = corpul trebuie recitit de pe server înainte de a putea fi aplicat
compare-omitted-unread = această comparație nu are o instrucțiune pentru el — definiția nu a putut fi citită
compare-omitted-no-statement = această comparație nu are o instrucțiune pentru diferența găsită
compare-held-partition = este o { $sub ->
        [yes] subpartiție
       *[no] partiție
    } a { $parent }, care este reținut — recreează-le împreună, manual
compare-held-partitioning-differs = partiționarea diferă, iar această comparație nu scrie nicio instrucțiune care repartiționează un tabel — recreează-l manual
compare-held-sequence-unwritten = această comparație nu scrie nicio instrucțiune pentru o secvență — copiaz-o din `SHOW CREATE SEQUENCE`, pe care o arată panoul ei
compare-held-partitioned-unrestated = este partiționat și niciun CREATE TABLE de aici nu îi reformulează schema de partiționare — creează manual funcția și schema de partiționare, apoi tabelul
compare-held-sequence-range = contorul țintei a ajuns la { $at }, în afara noului interval { $min }–{ $max }, iar serverul refuză să restrângă o secvență dincolo de o valoare pe care a distribuit-o. Repornește-o pe țintă (ALTER SEQUENCE … RESTART WITH) la o valoare pe care nu o conține niciun tabel, apoi compară din nou
compare-note-set-aside = { $aside } — { $side ->
        [target] ținta
       *[source] sursa
    } conține și { $kept }, iar numele țintei nu țin cont de majuscule, așa că poate conține doar unul dintre cele două; { $kept } este cel comparat
compare-note-binding = Tipul alias { $alias }: { $bound ->
        [rule] regula { $rule }
        [default] valoarea implicită { $default }
       *[both] regula { $rule } și valoarea implicită { $default }
    } legate de el pe sursă nu sunt comparate și nici preluate — leagă-le manual (sp_bindrule / sp_bindefault).
compare-hint-blocked = corpul trebuie recitit înainte ca acesta să poată fi aplicat
compare-hint-uncertain = un index pe care acest model îl citește doar parțial — o potrivire aici nu e sigură
compare-preview-migration = Previzualizează migrarea
compare-plan-subject = { $n ->
    [one] { $n } obiect în { $database }
    [few] { $n } obiecte în { $database }
   *[other] { $n } de obiecte în { $database }
}
compare-selected = { $n ->
        [0] Nimic selectat.
        [one] { $n } obiect selectat
        [few] { $n } obiecte selectate
       *[other] { $n } de obiecte selectate
    }
compare-incomparable = { $left } și { $right } nu pot fi comparate. Numele tipurilor, valorile implicite și forma indecșilor nu se transferă între motoare, așa că orice migrare generată din diferență ar fi greșită.
compare-group-table = Tabele
compare-group-view = Vizualizări
compare-group-trigger = Triggere
compare-kind-enum = enum
compare-kind-domain = domeniu
compare-kind-xml-schema-collection = colecție de scheme XML
compare-kind-alias-type = tip alias
compare-kind-sequence = secvență
compare-kind-table = tabel
compare-kind-view = vizualizare
compare-kind-function = funcție
compare-kind-procedure = procedură
compare-kind-trigger = trigger
compare-kind-event = eveniment
compare-kind-synonym = sinonim

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Niciuna dintre baze de date nu conține ceva de comparat.
compare-empty-agrees = Aceste două scheme coincid, obiect cu obiect.
compare-empty-agrees-noted = Toate obiectele coincid, dar nu totul a putut fi comparat — vezi notele de mai sus.
compare-empty-no-match = Nimic nu se potrivește cu acel filtru.
compare-empty-only-identical = Tot ce se potrivește cu acel filtru este identic — activează Include identicele pentru a vedea.

## Schema editors — shared (continued)

editor-comment-placeholder = ce face

## Event editor

event-editor-title-edit = Editează evenimentul { $event }
event-editor-title-create = Creează eveniment în { $location }
event-editor-section-event = Eveniment
event-editor-section-schedule = Programare
event-editor-section-body = Corp
event-editor-section-options = Opțiuni
event-editor-name = Nume
event-editor-body = Corp
event-editor-runs = Rulează
event-editor-runs-repeating = Repetat
event-editor-runs-once = O dată
event-editor-at = La
event-editor-every = La fiecare
event-editor-starts = Începe
event-editor-ends = Se termină
event-editor-status = Stare
event-editor-status-enabled = Activat
event-editor-status-disabled = Dezactivat
event-editor-status-replica = Dezactivat pe replică
event-editor-preserve = Păstrează după încheiere
event-editor-preserve-hint = ON COMPLETION PRESERVE. Dezactivat este implicit în MySQL și înseamnă că serverul șterge evenimentul după ce ultima rulare a trecut — ceea ce, pentru unul singular, înseamnă imediat după ce rulează.
event-editor-definer = Definitor
event-editor-comment = Comentariu
event-editor-reading-source = Se citește sursa evenimentului…
event-editor-source-stale = Sursa reală a evenimentului a sosit după ce ai început să tastezi, așa că acest Corp se bazează pe copia din catalog — care nu este ce deține serverul. Închide și redeschide pentru a-l edita.
event-problem-no-name = Evenimentul are nevoie de un nume.
event-problem-no-time = Un eveniment singular are nevoie de o oră la care să ruleze.
event-problem-no-interval = Un eveniment repetat are nevoie de un interval.
event-problem-no-unit = Un eveniment repetat are nevoie de o unitate de interval (DAY, HOUR…).
event-problem-no-body = Evenimentul are nevoie de un corp.
event-name-clash = Există deja aici un eveniment numit { $name }.

## Routine editor

routine-editor-title-edit-function = Editează funcția { $routine }
routine-editor-title-edit-procedure = Editează procedura { $routine }
routine-editor-title-create-function = Creează funcție în { $location }
routine-editor-title-create-procedure = Creează procedură în { $location }
routine-editor-section-function = Funcție
routine-editor-section-procedure = Procedură
routine-editor-section-body = Corp
routine-editor-section-options = Opțiuni
routine-editor-name = Nume
routine-editor-parameters = Parametri
routine-editor-returns = Returnează
routine-editor-returns-placeholder-mssql = int sau TABLE
routine-editor-language = Limbaj
routine-editor-body = Corp
routine-editor-recompile = Recompilare
routine-editor-recompile-hint = WITH RECOMPILE — un plan nou la fiecare apel, niciodată unul din cache.
routine-editor-schema-binding = Legare de schemă
routine-editor-schema-binding-hint = WITH SCHEMABINDING — tabelele și vizualizările pe care le citește nu pot fi modificate pe la spatele ei.
routine-editor-execute-as = Execută ca
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER sau un utilizator
routine-editor-null-input = Returnează NULL la intrare NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — corpul nu rulează când un argument este NULL.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automat
routine-editor-comment = Comentariu
routine-editor-volatility = Volatilitate
routine-editor-strict = Strict
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Rulează cu drepturile proprietarului în loc de cele ale apelantului. Fixează un search_path mai jos când folosești asta.
routine-editor-security-definer-hint-mysql = Rulează cu drepturile definitorului în loc de cele ale apelantului. Este implicit în MySQL — dezactivarea înseamnă SQL SECURITY INVOKER.
routine-editor-settings = Setări
routine-editor-add-setting = Adaugă setare
routine-editor-deterministic = Determinist
routine-editor-deterministic-hint = Promite același rezultat pentru aceleași argumente. Un server cu jurnalizare binară refuză o rutină nedeterministă dacă nu are încredere în creatori.
routine-editor-data-access = Acces la date
routine-editor-definer = Definitor
routine-editor-reading-source = Se citește sursa rutinei…
routine-editor-source-stale = Sursa reală a rutinei a sosit după ce ai început să tastezi, așa că acest Corp se bazează pe copia din catalog — care nu este ce deține serverul. Închide și redeschide pentru a-l edita.
routine-problem-function-no-name = Funcția are nevoie de un nume.
routine-problem-procedure-no-name = Procedura are nevoie de un nume.
routine-problem-function-no-language = Funcția are nevoie de un limbaj (plpgsql, sql).
routine-problem-procedure-no-language = Procedura are nevoie de un limbaj (plpgsql, sql).
routine-problem-no-return-type = Funcția are nevoie de un tip returnat.
routine-problem-procedure-returns = O procedură nu returnează nimic — golește tipul returnat.
routine-problem-function-no-body = Funcția are nevoie de un corp.
routine-problem-procedure-no-body = Procedura are nevoie de un corp.
routine-problem-bare-parameter = SQL Server numește fiecare parametru cu @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = O procedură nu poate primi WITH { $option }.
routine-problem-scalar-refuses = O funcție scalară nu poate primi WITH { $option }.
routine-problem-inline-refuses = O funcție inline cu valori de tabel nu poate primi WITH { $option }.
routine-problem-multi-refuses = O funcție cu mai multe instrucțiuni și valori de tabel nu poate primi WITH { $option }.
routine-problem-procedure-refuses-native = O procedură nu poate primi WITH { $option } după ce este compilată nativ.
routine-problem-scalar-refuses-native = O funcție scalară nu poate primi WITH { $option } după ce este compilată nativ.
routine-problem-inline-refuses-native = O funcție inline cu valori de tabel nu poate primi WITH { $option } după ce este compilată nativ.
routine-problem-multi-refuses-native = O funcție cu mai multe instrucțiuni și valori de tabel nu poate primi WITH { $option } după ce este compilată nativ.
routine-problem-native-needs-schemabinding = O rutină compilată nativ are nevoie de WITH SCHEMABINDING — SQL Server le cere pe amândouă împreună.
routine-problem-inline-with-execute-as = INLINE = ON nu poate fi combinat cu EXECUTE AS — o funcție care rulează în numele altcuiva nu este niciodată inline.
routine-problem-numbered-rename = { $was } conduce un grup de proceduri numerotate ({ $group }), iar redenumirea ar elimina tot grupul. Păstrează numele pentru a o edita.
routine-problem-trigger-function-args = O funcție de trigger nu primește argumente declarate — ele sosesc în TG_ARGV.
routine-clash-function-refused = Există deja aici o funcție numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că serverul o refuză și nu se schimbă nimic — alege alt nume sau elimină mai întâi { $name }.
routine-clash-procedure-refused = Există deja aici o procedură numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că serverul o refuză și nu se schimbă nimic — alege alt nume sau elimină mai întâi { $name }.
routine-clash-function-destroys = Există deja aici o funcție numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că aplicarea ar distruge { $original } și ar lăsa { $name } neschimbată.
routine-clash-procedure-destroys = Există deja aici o procedură numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că aplicarea ar distruge { $original } și ar lăsa { $name } neschimbată.
routine-clash-function-destroys-new = Există deja aici o funcție numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că aplicarea ar distruge originalul și ar lăsa { $name } neschimbată.
routine-clash-procedure-destroys-new = Există deja aici o procedură numită { $name }. Pe acest motor o redenumire este o eliminare și o creare, așa că aplicarea ar distruge originalul și ar lăsa { $name } neschimbată.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Caută conturi
users-loading-accounts = Se încarcă conturile
users-unsupported = Motorul acestei conexiuni nu are conturi de utilizator.
users-none = Acest server nu raportează niciun cont.
users-no-match = Niciun cont nu se potrivește cu „{ $needle }”.
users-pick-account = Alege un cont pentru a vedea ce are voie să facă.
users-section-attributes = Atribute
users-section-privileges = Privilegii
users-attr-authentication = Autentificare
users-attr-locked = Blocat
users-attr-password-expired = Parolă expirată
users-attr-superuser = Superutilizator
users-attr-create-role = Creare de roluri
users-attr-create-database = Creare de baze de date
users-attr-replication = Replicare
users-attr-bypass-rls = Ocolește RLS
users-attr-inherit = Moștenire
users-attr-connection-limit = Limită de conexiuni
users-attr-valid-until = Valabil până la
users-attr-type = Tip
users-attr-disabled = Dezactivat
users-attr-default-database = Bază de date implicită
users-attr-server-roles = Roluri de server
users-attr-login = Login
users-attr-default-schema = Schemă implicită
users-attr-member-of = Membru al
users-attr-yes = Da
users-attr-no = Nu
users-login-type-sql = Login SQL
users-login-type-windows = Login Windows
users-login-type-windows-group = Grup Windows
users-kind-user = Utilizator
users-kind-role = Rol
users-kind-login = Login
users-maintained-by-server = gestionat de server
users-loading-privileges = Se încarcă privilegiile
users-no-privileges = Acest cont nu deține privilegii.
users-note-my-own-account = Această conexiune nu poate citi mysql.user, așa că aceasta nu este lista de conturi a serverului — este singurul cont pe care această conexiune îl poate vedea, al ei. Conectează-te cu un cont care are SELECT pe baza de date mysql pentru a le vedea pe celelalte.
users-note-ms-logins-elsewhere = Azure SQL Database își păstrează loginurile în master, iar acestea nu sunt gestionate aici: sunt listați utilizatorii și rolurile bazei de date.
users-note-ms-logins-only = Sunt listate doar loginurile serverului: alege o bază de date pentru a-i vedea utilizatorii și rolurile.
users-note-ms-short = Acestui login îi lipsește VIEW ANY DEFINITION, așa că serverul îi arată doar principalii pe care îi poate vedea — lista poate fi incompletă.
users-note-my-scope = Privilegiile deținute printr-un rol acordat nu sunt detaliate: SHOW GRANTS listează ce este acordat direct contului și rolurile al căror membru este, dar nu și ce dețin acele roluri. Deschide privilegiile rolului pentru a le vedea.
users-note-pg-scope = Privilegiile pe scheme, tabele și secvențe sunt cele din { $database } — PostgreSQL le păstrează în catalogul propriu al fiecărei baze de date, așa că privilegiile din alte baze de date nu sunt listate. Nici EXECUTE pe funcții și proceduri și nici privilegiile acordate pe o singură coloană nu sunt listate. Privilegiile deținute prin apartenența la un rol sau acordate lui PUBLIC nu sunt detaliate, și nici privilegiile pe care un rol le are deținând un obiect sau fiind superutilizator.
users-note-pg-no-database = Privilegiile pe scheme, tabele și secvențe nu sunt listate: PostgreSQL le păstrează în catalogul propriu al fiecărei baze de date, iar această conexiune nu are nicio bază de date selectată. Deschide o filă de interogare pe o bază de date pentru a le vedea. Nici EXECUTE pe funcții și proceduri și nici privilegiile acordate pe o singură coloană nu sunt listate. Privilegiile deținute prin apartenența la un rol sau acordate lui PUBLIC nu sunt detaliate, și nici privilegiile pe care un rol le are deținând un obiect sau fiind superutilizator.
users-note-pg-superuser = Acest rol este SUPERUSER: ocolește orice verificare de permisiuni, așa că deține toate privilegiile pe toate obiectele, indiferent ce spune lista de mai jos.
users-note-pg-predefined = { $role } este unul dintre rolurile predefinite ale PostgreSQL: ce poate face este încorporat în server, nu acordat, așa că nu este înregistrat în niciun catalog citit aici.
users-note-pg-owns = { $n ->
    [one] Acest rol deține { $n } obiect în { $database } (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
    [few] Acest rol deține { $n } obiecte în { $database } (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
   *[other] Acest rol deține { $n } de obiecte în { $database } (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
}
users-note-pg-owns-cluster = { $n ->
    [one] Acest rol deține { $n } obiect în acest cluster (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
    [few] Acest rol deține { $n } obiecte în acest cluster (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
   *[other] Acest rol deține { $n } de obiecte în acest cluster (scheme, tabele, secvențe sau baza de date însăși). Un proprietar are toate privilegiile asupra a ceea ce deține, iar PostgreSQL nu înregistrează nicio acordare pentru asta, așa că nimic din acestea nu apare mai jos.
}
users-note-ms-unshown = { $n ->
    [one] { $n } permisiune nu este afișată aici, pe { $classes }: serverul o raportează pe un securizabil pe care această listă nu îl poate numi.
    [few] { $n } permisiuni nu sunt afișate aici, pe { $classes }: serverul le raportează pe securizabile pe care această listă nu le poate numi.
   *[other] { $n } de permisiuni nu sunt afișate aici, pe { $classes }: serverul le raportează pe securizabile pe care această listă nu le poate numi.
}
users-note-ms-pick-database = Permisiunile unui utilizator de bază de date sunt ale acelei baze de date — alege una pentru a le vedea.
users-statements-capped = Instrucțiuni afișate: primele { $shown } din { $total }. Copiază privilegiile le preia pe toate.
users-new-account = Cont nou
users-system-account = Acest cont aparține serverului, așa că Schemaic nu îl va modifica.
users-reset-password = Resetează parola
users-gate-no-database = Deschide o filă de interogare pe o bază de date pentru a modifica privilegiile — instrucțiunea trebuie să ruleze într-una.
users-gate-read-only = Această conexiune este doar pentru citire.
users-count = { $n ->
    [one] { $n } cont
    [few] { $n } conturi
   *[other] { $n } de conturi
}
users-count-filtered = Conturi: { $shown } din { $n }
users-copy-privileges = Copiază privilegiile

## Schema editors — shared (continued)

editor-row-remove = Elimină

## Trigger editor

trigger-editor-title = Triggere pe { $table }
trigger-problem-no-name = Triggerul are nevoie de un nume.
trigger-problem-no-table = Un trigger are nevoie de un tabel pe care să se declanșeze.
trigger-problem-no-event = Un trigger are nevoie de cel puțin un eveniment la care să se declanșeze.
trigger-problem-pg-no-function = Un trigger PostgreSQL are nevoie de o funcție de executat.
trigger-problem-pg-body = Un trigger PostgreSQL rulează o funcție, nu un corp — alege sau scrie una de executat.
trigger-problem-truncate-row-level = Un trigger TRUNCATE trebuie să fie FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Un trigger INSTEAD OF trebuie să fie FOR EACH ROW.
trigger-problem-instead-of-view-only = Doar o vizualizare poate avea un trigger INSTEAD OF — un tabel primește BEFORE sau AFTER.
trigger-problem-pg-view-row-timing = Un trigger BEFORE sau AFTER al unei vizualizări trebuie să fie FOR EACH STATEMENT — folosește INSTEAD OF pentru a acționa asupra rândurilor.
trigger-problem-update-of-without-update = UPDATE OF numește coloane pe un trigger UPDATE.
trigger-problem-pg-instead-of-columns = Un trigger INSTEAD OF nu primește o listă de coloane — PostgreSQL îl declanșează la orice UPDATE al vizualizării.
trigger-problem-one-event = { $engine } declanșează un trigger la un singur eveniment — creează câte un trigger separat pentru fiecare eveniment.
trigger-problem-no-truncate = { $engine } nu are trigger TRUNCATE.
trigger-problem-sqlite-view-timing = Triggerul unei vizualizări trebuie să fie INSTEAD OF — SQLite nu are triggere BEFORE sau AFTER pe o vizualizare.
trigger-problem-sqlite-row-only = SQLite are doar triggere FOR EACH ROW — nu există unul la nivel de instrucțiune.
trigger-problem-no-body = Un trigger { $engine } are nevoie de un corp.
trigger-problem-sqlite-not-block = Corpul unui trigger SQLite trebuie să fie un bloc BEGIN … END care conține cel puțin o instrucțiune.
trigger-problem-runs-body = Un trigger { $engine } rulează un corp, nu o funcție — scrie instrucțiunile de rulat.
trigger-problem-ms-no-before = SQL Server nu are trigger BEFORE — folosește INSTEAD OF pentru a acționa înainte de scriere.
trigger-problem-ms-view-timing = În SQL Server, triggerul unei vizualizări trebuie să fie INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server declanșează un trigger o dată pe instrucțiune — citește rândurile din inserted și deleted.
trigger-problem-ms-instead-of-rank = Un trigger INSTEAD OF nu poate avea o ordine de declanșare — setează fiecare eveniment înapoi la orice ordine.
trigger-problem-first-without-event = Se declanșează primul la { $event }, pentru care nu este trigger.
trigger-problem-last-without-event = Se declanșează ultimul la { $event }, pentru care nu este trigger.
trigger-problem-no-when = { $engine } nu are condiție WHEN — pune testul în corp, cu IF.
trigger-problem-ms-no-update-of = SQL Server nu are UPDATE OF — testează UPDATE(coloană) în corp.
trigger-problem-my-no-view = MySQL nu acceptă triggere pe o vizualizare.
trigger-problem-my-no-instead-of = MySQL nu are trigger INSTEAD OF — acelea sunt ale PostgreSQL.
trigger-problem-my-no-update-of = MySQL nu are UPDATE OF — un trigger vede fiecare coloană.
trigger-problem-constraint-trigger = Schemaic nu poate edita triggerul de constrângere { $name } — nu modelează setările de amânare pe care le are.
trigger-problem-not-editable = Schemaic nu poate edita triggerul { $name } — definiția lui este criptată, ascunsă pentru acest login sau folosește o opțiune pe care Schemaic nu o modelează. Modifică-l în SQL.
trigger-problem-twice = Două triggere se numesc amândouă { $name }.
trigger-problem-first-clash = { $n ->
    [2] Doar un trigger se poate declanșa primul la { $event } — { $names } și { $last } sunt setate amândouă așa.
   *[other] Doar un trigger se poate declanșa primul la { $event } — { $names } și { $last } sunt toate setate așa.
}
trigger-problem-last-clash = { $n ->
    [2] Doar un trigger se poate declanșa ultimul la { $event } — { $names } și { $last } sunt setate amândouă așa.
   *[other] Doar un trigger se poate declanșa ultimul la { $event } — { $names } și { $last } sunt toate setate așa.
}
trigger-editor-preview-subject = triggere pe { $table }
trigger-editor-none-selected = Niciun trigger selectat.
trigger-editor-readonly-constraint = Acesta este un trigger de constrângere. Schemaic nu modelează setările de amânare pe care le are, așa că poate fi eliminat aici, dar nu editat — modifică-l în SQL.
trigger-editor-readonly-hidden = Definiția acestui trigger este criptată sau ascunsă pentru acest login, așa că poate fi eliminat aici, dar nu editat.
trigger-editor-readonly-unmodelled = Acest trigger folosește o opțiune pe care Schemaic nu o modelează, așa că poate fi eliminat aici, dar nu editat — modifică-l în SQL.
trigger-editor-section-trigger = Trigger
trigger-editor-section-condition = Condiție
trigger-editor-section-action = Acțiune
trigger-editor-name = Nume
trigger-editor-timing = Moment
trigger-editor-event = Eveniment
trigger-editor-fires = Se declanșează
trigger-editor-truncate-hint = Doar la nivel de instrucțiune.
trigger-editor-not-for-replication-hint = Inactiv cât timp un agent de replicare scrie în tabel.
trigger-editor-rank-any = Orice ordine
trigger-editor-rank-first = Se declanșează primul
trigger-editor-rank-last = Se declanșează ultimul
trigger-editor-when = Când
trigger-editor-of-columns = Pe coloanele
trigger-editor-of-columns-placeholder = opțional — toate coloanele dacă e gol
trigger-editor-body = Corp
trigger-editor-function = Funcție
trigger-editor-edit-function = Editează
trigger-editor-new-function = Funcție nouă
trigger-editor-arguments = Argumente
trigger-editor-add-argument = Adaugă argument

## Schema editors — shared (continued)

editor-row-move-up = Mută în sus
editor-row-move-down = Mută în jos

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Editează { $kind } { $name }
object-editor-title-create = Creează { $kind } în { $location }
object-editor-name = Nume
object-editor-comment = Comentariu
object-editor-type = Tip
object-editor-default = Implicit
object-editor-section-type = Tip
object-editor-section-values = Valori
object-editor-section-domain = Domeniu
object-editor-section-constraints = Constrângeri
object-editor-section-sequence = Secvență
object-editor-section-position = Poziție
object-editor-section-synonym = Sinonim
object-editor-section-alias-type = Tip alias
object-editor-section-xml-schema-collection = Colecție de scheme XML
object-editor-section-stands-for = Reprezintă
object-editor-value-placeholder = valoare
object-editor-add-value = Adaugă valoare
object-editor-values-order = În ordinea de comparare — aceasta este ordinea folosită de ORDER BY
object-editor-add-constraint = Adaugă constrângere
object-editor-no-base-types = Acest motor nu sugerează tipuri de bază
object-editor-not-null-hint = Fiecare coloană a acestui domeniu respinge NULL. Activarea eșuează dacă una conține deja un NULL.
object-editor-stores = Stochează
object-editor-increment = Increment
object-editor-start = Start
object-editor-cache = Cache
object-editor-minimum = Minim
object-editor-maximum = Maxim
object-editor-not-whole-number = { $field } trebuie să fie un număr întreg.
object-problem-type-no-name = Tipul are nevoie de un nume.
object-problem-duplicate-value = Valoarea { $value } este listată de mai multe ori.
object-problem-empty-value = O valoare nu poate fi goală — PostgreSQL nu o mai poate elimina niciodată.
object-problem-domain-no-name = Domeniul are nevoie de un nume.
object-problem-domain-no-base = Un domeniu are nevoie de un tip pe care să se bazeze.
object-problem-constraint-no-name = Fiecare constrângere are nevoie de un nume.
object-problem-constraint-no-predicate = Constrângerea { $name } nu are predicat.
object-problem-constraint-twice = Constrângerea { $name } apare de două ori.
object-problem-sequence-no-name = Secvența are nevoie de un nume.
object-problem-zero-increment = O secvență nu poate avea incrementul 0.
object-problem-min-above-max = Minimul ({ $min }) este peste maxim ({ $max }).
object-problem-start-outside = Valoarea de start ({ $start }) este în afara intervalului { $min }..{ $max }.
object-problem-cache-below-one = Cache-ul trebuie să fie cel puțin 1.
object-problem-does-not-fit = { $min }..{ $max } nu încape în { $data_type }.
object-problem-restart-outside = Repornirea la { $restart } este în afara intervalului { $min }..{ $max }.
object-problem-counter-outside = Secvența este la { $last }, în afara noului interval { $min }..{ $max }. Dă-i o valoare de repornire din interval.
object-problem-no-name = Numele nu poate fi gol.
object-problem-tsql-no-type = O secvență are nevoie de un tip.
object-problem-start-not-whole = Startul trebuie să fie un număr întreg.
object-problem-increment-not-whole = Incrementul trebuie să fie un număr întreg.
object-problem-minimum-not-whole = Minimul trebuie să fie un număr întreg.
object-problem-maximum-not-whole = Maximul trebuie să fie un număr întreg.
object-problem-tsql-zero-increment = Incrementul nu poate fi 0.
object-problem-min-not-below-max = Minimul trebuie să fie sub maxim.
object-problem-started-outside = A pornit la { $start }, în afara noului interval: repornește-o la o valoare între minim și maxim.
object-problem-tsql-start-outside = Startul trebuie să fie între minim și maxim.
object-problem-cache-not-positive = Dimensiunea cache-ului trebuie să fie un număr întreg mai mare decât 0.
object-problem-restart-not-whole = Valoarea de repornire trebuie să fie un număr întreg.
object-problem-tsql-restart-outside = Valoarea de repornire trebuie să fie între minim și maxim.
object-problem-synonym-no-target = Un sinonim are nevoie de obiectul pe care îl reprezintă.
object-problem-alias-no-base = Un tip alias are nevoie de un tip de bază.
object-editor-cycle = Ciclic
object-editor-cycle-hint = Revine la celălalt capăt al intervalului în loc să eșueze când secvența se epuizează.
object-editor-restart-at = Repornește la — contorul a distribuit ultima dată { $last }
object-editor-restart-at-unused = Repornește la — contorul nu a fost folosit
object-editor-restart-not-whole = Repornește la trebuie să fie un număr întreg.
object-editor-restart-placeholder = lasă gol pentru a păstra poziția
object-editor-tsql-restart-placeholder = gol o păstrează, dacă tipul nu se schimbă
object-editor-owned-by = Deținută de
object-editor-detach = Detașează de coloana ei
object-editor-detach-hint = Secvența nu mai este eliminată odată cu coloana și îi supraviețuiește.
object-editor-cache-hint = Distribuie valori din memorie, mai rapid, cu prețul unui gol după o repornire a serverului.
object-editor-cache-size = Dimensiunea cache-ului
object-editor-cache-size-placeholder = implicit pe server
object-editor-synonym-server = Server
object-editor-synonym-server-placeholder = acest server
object-editor-synonym-database = Bază de date
object-editor-synonym-database-placeholder = această bază de date
object-editor-synonym-schema = Schemă
object-editor-synonym-schema-placeholder = schema implicită
object-editor-synonym-object = Obiect
object-editor-synonym-object-placeholder = tabel, vizualizare sau rutină
object-editor-base-type = Tip de bază
object-editor-nullable = Acceptă NULL
object-editor-nullable-hint = O coloană de acest tip acceptă NULL dacă nu specifică altfel.
object-editor-schemas = Scheme

## Account and grant editors (Users and privileges)

account-editor-title-create = Creează cont
account-editor-title-reset = Resetează parola
account-editor-title-privileges = Privilegii — { $account }
account-editor-account = Cont
account-editor-kind = Tip
account-editor-name = Nume
account-editor-host = Gazdă
account-editor-host-placeholder = % (orice gazdă)
account-editor-for-login = Pentru loginul
account-editor-for-login-placeholder = login — gol pentru un utilizator fără login
account-editor-also-user = Și utilizator în { $database }
account-editor-password = Parolă
account-editor-confirm = Confirmare
account-editor-action = Acțiune
account-editor-action-grant = Acordă
account-editor-action-revoke = Revocă
account-editor-subject = Subiect
account-editor-subject-privileges = Privilegii
account-editor-subject-role = Rol
account-editor-role = Rol
account-editor-no-roles = Acest server nu are roluri
account-editor-with-admin-option = Cu opțiunea de administrare
account-editor-level = Nivel
account-editor-level-global = Întregul server
account-editor-level-database = Bază de date
account-editor-level-schema = Schemă
account-editor-level-table = Tabel
account-editor-level-sequence = Secvență
account-editor-no-suggestions = Nicio sugestie
account-editor-section-privileges = Privilegii
account-editor-with-grant-option = Cu opțiunea de acordare
account-editor-needs-role = Numește un rol.
account-editor-needs-privileges = Alege un nivel, numește obiectul la care se aplică și bifează un privilegiu.
account-editor-password-required = Este necesară o parolă.
account-editor-name-required = Este necesar un nume.
account-editor-entra-only-user = Din Microsoft Entra se poate crea doar un utilizator de bază de date.
account-editor-entra-no-password = Un utilizator Entra nu are aici login sau parolă — Entra îl autentifică.
account-editor-login-needs-password = Un login are nevoie de o parolă.
account-editor-user-for-login-password = Un utilizator pentru un login se autentifică cu parola loginului — lasă acest câmp gol.
account-editor-confirm-password = Tastează parola din nou pentru a o confirma.
account-editor-passwords-differ = Cele două parole diferă.
account-editor-password-hashed = SQL-ul previzualizat, care este instrucțiunea ce rulează, conține un hash al parolei în forma în care o stochează acest server, nu parola însăși — cu excepția cazului în care are caractere pe care loginul le-ar normaliza (litere de lățime completă, ligaturi, spații neseparabile) sau serverul rulează o extensie de verificare a parolelor; atunci este trimisă așa cum a fost tastată.
account-editor-password-as-typed = Parola apare în SQL-ul previzualizat, care este instrucțiunea ce rulează.

## Table designer

designer-title-edit = Editează { $table }
designer-title-create = Creează tabel în { $location }
designer-tab-table = Tabel
designer-tab-columns = Coloane
designer-tab-indexes = Indecși
designer-tab-foreign-keys = Chei străine
designer-tab-checks = Verificări
designer-list-add = Adaugă
designer-no-suggestions = Nicio sugestie
designer-name = Nume
designer-comment = Comentariu
designer-table-comment-placeholder = La ce folosește acest tabel
designer-engine = Motor
designer-collation = Colaționare
designer-no-column = Nicio coloană selectată.
designer-type = Tip
designer-type-hint = Scris direct — serverul decide ce înseamnă.
designer-nullable = Acceptă NULL
designer-nullable-hint = Permite NULL în această coloană.
designer-primary-key = Cheie primară
designer-primary-key-hint = Parte a cheii primare a tabelului, în ordinea coloanelor.
designer-identity = Identity
designer-auto-increment = Auto-increment
designer-identity-hint = Serverul atribuie valoarea ({ $keyword }).
designer-default = Implicit
designer-default-hint = Text SQL, așa cum l-ai scrie: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Generată din
designer-generated-hint = O expresie, de ex. qty * price. O coloană generată nu primește valoare implicită.
designer-on-update = La actualizare
designer-index-unique-detail = unic
designer-index-redundant-detail = redundant
designer-hint-redundant = Indexul { $index } este acoperit de { $cover }: orice căutare pe care o servește o servește și acela.
table-problem-no-name = Tabelul are nevoie de un nume.
table-problem-no-columns = Un tabel are nevoie de cel puțin o coloană.
table-problem-column-no-name = Fiecare coloană are nevoie de un nume.
table-problem-column-twice = Două coloane se numesc amândouă { $name }.
table-problem-column-no-type = Coloana { $name } are nevoie de un tip.
table-problem-key-not-column = Cheia primară numește { $column }, care nu este o coloană.
table-problem-index-no-name = Fiecare index are nevoie de un nume.
table-problem-index-twice = Doi indecși se numesc amândoi { $name }.
table-problem-index-no-columns = Indexul { $index } nu are coloane.
table-problem-xml-one-column = Indexul { $index } trebuie să numească exact o coloană.
table-problem-xml-no-options = Indexul { $index } nu poate fi unic, nu poate include coloane și nu poate fi filtrat: un index XML sau spațial nu acceptă niciuna dintre acestea.
table-problem-xml-no-using = Indexul { $index } are nevoie de clauza USING XML INDEX … FOR …: un index XML secundar se construiește pe unul primar sau selectiv.
table-problem-selective-no-paths = Indexul { $index } are nevoie de căile FOR (…): un index XML selectiv este format din căile pe care le promovează.
table-problem-xml-parent-gone = Indexul { $index } se construiește pe { $parent }, care nu este un index XML primar sau selectiv al acestui tabel: dispare odată cu părintele său.
table-problem-not-orderable = Indexul { $index } nu poate fi ordonat: doar un index columnstore are ORDER.
table-problem-order-not-column = Indexul { $index } ordonează după { $column }, care nu este o coloană.
table-problem-order-not-stored = Indexul { $index } ordonează după { $column }, pe care nu o stochează.
table-problem-columnstore-unique = Indexul { $index } nu poate fi unic: un index columnstore stochează coloane, nu chei.
table-problem-columnstore-include = Indexul { $index } nu poate include coloane: un index columnstore listează fiecare coloană pe care o stochează.
table-problem-columnstore-sorted = Indexul { $index } nu își poate sorta coloanele: o coloană columnstore nu are direcție.
table-problem-clustered-columnstore-filtered = Indexul { $index } nu poate fi filtrat: un index columnstore cluster stochează fiecare rând.
table-problem-clustered-include = Indexul { $index } nu poate include coloane: un index cluster conține deja fiecare coloană.
table-problem-unique-include = Indexul { $index } nu poate include coloane: o constrângere unică nu are INCLUDE aici.
table-problem-clustered-filtered = Indexul { $index } nu poate fi filtrat: un index cluster acoperă fiecare rând.
table-problem-constraint-filtered = Indexul { $index } nu poate fi filtrat: susține o constrângere UNIQUE, care nu primește condiție aici. Elimină-l și adaugă un index unic cu condiția.
table-problem-not-partial = Indexul { $index } nu poate fi filtrat: acest index nu poate fi parțial aici.
table-problem-index-not-column = Indexul { $index } numește { $column }, care nu este o coloană.
table-problem-include-not-column = Indexul { $index } include { $column }, care nu este o coloană.
table-problem-cluster-clash = Un tabel are un singur index cluster, iar { $clustered } îl grupează fiecare - fă-i pe toți, în afară de unul, nonclustered.
table-problem-cluster-key = cheia primară
table-problem-cluster-index = indexul { $index }
table-problem-cluster-and = și
table-problem-fk-no-name = Fiecare cheie străină are nevoie de un nume.
table-problem-fk-twice = Două chei străine se numesc amândouă { $name }.
table-problem-fk-no-table = Cheia străină { $fk } nu face referire la niciun tabel.
table-problem-fk-unpaired = Cheia străină { $fk } trebuie să împerecheze fiecare coloană cu una la care face referire.
table-problem-fk-not-column = Cheia străină { $fk } numește { $column }, care nu este o coloană.
table-problem-check-no-name = Fiecare constrângere de verificare are nevoie de un nume.
table-problem-check-twice = Două constrângeri de verificare se numesc amândouă { $name }.
table-problem-check-no-predicate = Verificarea { $name } nu are predicat.
table-problem-without-rowid-no-key = Un tabel WITHOUT ROWID are nevoie de o cheie primară.
table-problem-strict-type = Tabelele STRICT acceptă doar INT, INTEGER, REAL, TEXT, BLOB sau ANY — { $column } este { $data_type }.
designer-hint-unindexed-fk = Cheia străină { $fk } nu are un index care începe cu coloanele ei, așa că o ștergere sau o actualizare de cheie în tabelul la care face referire scanează acest tabel.
designer-no-index = Niciun index selectat. Cheia primară se află pe coloane.
designer-columns = Coloane
designer-index-columns-hint-single = Singura coloană xml, geometry sau geography pe care o indexează.
designer-index-columns-hint-columnstore = Coloanele pe care le stochează, separate prin virgulă.
designer-index-columns-hint-prefix = Separate prin virgulă, în ordinea cheii. bio(20) este o lungime de prefix; adaugă DESC pentru sortare descendentă.
designer-index-columns-hint = Separate prin virgulă, în ordinea cheii. Adaugă DESC pentru o coloană descendentă.
designer-index-method = Metodă
designer-index-method-hint = Lasă gol pentru implicit (btree).
designer-index-filter = Doar rândurile unde
designer-index-filter-hint = Condiția unui index parțial, fără WHERE.
designer-index-storage = Stocare
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Columnstore cluster
designer-storage-primary-xml = XML primar
designer-storage-selective-xml = XML selectiv
designer-storage-secondary-xml = XML secundar
designer-storage-spatial = Spațial
designer-index-using = Using
designer-index-using-hint = Clauza de după coloană: USING XML INDEX … FOR PATH, VALUE sau PROPERTY pentru un index XML secundar (sau FOR (cale) pentru unul selectiv), [WITH XMLNAMESPACES (…)] FOR (nume = '/cale' [AS …], …) pentru un index XML selectiv sau USING … WITH (…) pentru un index spațial.
designer-index-order = Ordine
designer-index-order-hint = Coloanele după care sunt sortate segmentele, separate prin virgulă — de la SQL Server 2022, și de la 2025 pentru un index nonclustered, care sortează doar după coloanele pe care le stochează.
designer-index-include = Include
designer-index-include-hint = Coloane separate prin virgulă, purtate în index, dar care nu fac parte din cheia lui.
designer-index-unique = Unic
designer-index-unique-hint = Refuză valorile duplicate pe aceste coloane.
designer-fk-detail-unindexed = → { $table } · fără index
designer-no-fk = Nicio cheie străină selectată.
designer-fk-ref-table = Tabel referit
designer-fk-on-delete = La ștergere
designer-fk-columns-hint = Separate prin virgulă, împerecheate în ordine cu coloanele referite.
designer-fk-ref-columns = Coloane referite
designer-no-check = Nicio verificare selectată.
designer-check-expression = Expresie
designer-check-expression-hint = Condiția pe care trebuie să o îndeplinească fiecare rând. Scrisă fără CHECK (…).
designer-check-enforced = Aplicată
designer-check-enforced-hint = Dezactivat, constrângerea este înregistrată fără a fi aplicată — rândurile existente și cele noi sunt acceptate deopotrivă.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Gestionează conexiunile
connections-new = Conexiune nouă
connections-import = Importă conexiuni
connections-export = Exportă conexiuni
connections-duplicate = Duplică
connections-delete = Șterge
connections-test = Testează
connections-save = Salvează
connections-test-failed = Conectarea a eșuat
connections-browse = Răsfoiește…
connections-section-general = General
connections-section-safeguards = Măsuri de siguranță
connections-section-database = Bază de date
connections-section-network = Rețea
connections-name = Nume
connections-colour = Culoare
connections-folder = Dosar
connections-no-folders = Niciun dosar încă
connections-folder-hint = Opțional. Conexiunile din același dosar sunt listate împreună.
connections-prominent-colour = Culoare evidentă în editor
connections-prominent-colour-hint = Încadrează editorul de interogări în culoarea acestei conexiuni.
connections-read-only = Doar citire
connections-read-only-hint = Dezactivează editarea celulelor și blochează interogările de scriere/DDL pe această conexiune.
connections-read-only-mssql = SQL Server nu are sesiuni doar pentru citire, așa că citirile rulează într-o tranzacție pe care Schemaic o anulează prin rollback. Efectul unei proceduri în afara bazei de date supraviețuiește; pentru o garanție, conectează-te cu un login care poate doar citi.
connections-cli-access = Acces CLI
connections-cli-access-hint = Permite liniei de comandă schemaic și agenților AI, prin schemaic mcp, să folosească această conexiune.
connections-ai-data-access = Acces AI la date
connections-statement-timeout = Timp limită pentru instrucțiuni
connections-statement-timeout-hint = Anulează o instrucțiune pe această conexiune care rulează mai mult.
connections-statement-timeout-same = La fel ca aplicația
connections-environment = Mediu
env-none = Niciunul
env-local = Local
env-development = Dezvoltare
env-testing = Testare
env-staging = Staging
env-production = Producție
connections-type = Tip
connections-init-sql = SQL de inițializare
connections-init-sql-pragma-hint = Opțional. Setări PRAGMA rulate pe fiecare conexiune pe care o deschide Schemaic.
connections-init-sql-set-hint = Opțional. Instrucțiuni SET rulate pe fiecare conexiune pe care o deschide Schemaic — un search path, un fus orar, un rol.
connections-init-refused = „{ $statement }”: { $why }
connections-init-only-pragmas = doar setările PRAGMA pot rula la conectare
connections-init-only-sets = doar instrucțiunile SET pot rula la conectare
connections-init-calls-function = o setare nu poate apela o funcție la conectare
connections-init-quoted-name = numele unei setări nu poate fi între ghilimele la conectare
connections-init-names-no-setting = aceasta nu numește nicio setare
connections-init-not-listed = { $name } nu poate fi setat la conectare — nu este una dintre setările de sesiune despre care Schemaic știe că nu îi afectează protecțiile
connections-init-not-utf8 = setul de caractere al clientului trebuie să rămână UTF-8 — verificarea de doar citire a Schemaic citește fiecare instrucțiune ca UTF-8
connections-init-one-value = o setare primește o singură valoare la conectare și nimic nu o poate urma
connections-init-one-value-after-equals = o setare primește o singură valoare la conectare, după un =
connections-init-transaction = setările de tranzacție nu pot fi făcute la conectare — protecția de doar citire le setează
connections-init-pragma = acel PRAGMA nu poate rula la conectare — modifică fișierul sau protecția de doar citire
connections-init-server-scope = setările { $scope } modifică serverul, nu sesiunea
connections-init-variable = o variabilă nu este o setare de sesiune — se încheie odată cu lotul ei
connections-init-on-only = { $name } poate fi doar activat la conectare — Schemaic se bazează pe el
connections-host = Gazdă
connections-port = Port
connections-named-instance-port = O instanță cu nume: SQL Server Browser îi dă portul, așa că Port nu este folosit. Pentru a ocoli Browser, setează gazda fără instanță și portul pe care ascultă.
connections-database = Bază de date
connections-database-hint = Opțional; baza de date în care se deschide conexiunea. Unii furnizori găzduiți permit doar baza lor și refuză o conexiune care nu o numește.
connections-authentication = Autentificare
ssh-auth-password = Parolă
ssh-auth-key-pair = Pereche de chei
ssh-auth-agent = Agent SSH
auth-password = Utilizator și parolă
auth-windows = Windows (acest cont)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Utilizator
connections-password = Parolă
connections-azure-cli-hint = Se autentifică drept cine este autentificat în Azure CLI (rulează mai întâi `az login`). Tokenul este obținut când o conexiune are nevoie de el și nu este salvat niciodată.
connections-aws-iam-hint = Se autentifică drept utilizatorul de mai jos, cu un token din credențialele AWS CLI (mai întâi `aws configure` sau `aws sso login`). Utilizatorul are nevoie de rolul rds_iam (PostgreSQL) sau de AWSAuthenticationPlugin (MySQL). Tokenul este obținut când o conexiune are nevoie de el și nu este salvat niciodată.
connections-gcp-iam-hint = Se autentifică drept utilizatorul de mai jos — utilizatorul IAM sau contul de serviciu cunoscut de Cloud SQL — cu un token din Google Cloud CLI (mai întâi `gcloud auth login`). Tokenul este obținut când o conexiune are nevoie de el și nu este salvat niciodată.
connections-windows-hint-named = Se autentifică drept contul Windows sub care rulează Schemaic ({ $account }). Serverul trebuie să cunoască acest cont ca login Windows.
connections-windows-hint = Se autentifică drept contul Windows sub care rulează Schemaic. Serverul trebuie să cunoască acest cont ca login Windows.
connections-database-file = Fișier de bază de date
connections-database-file-dialog = Selectează baza de date SQLite
connections-sqlite-hint = O bază de date SQLite este un singur fișier, așa că nu există server de accesat — nici gazdă, utilizator, parolă sau tunel. Tranzacțiile manuale nu sunt încă disponibile pe ea: instrucțiunile se confirmă pe măsură ce sunt trimise.
connections-ssh-tunnel = Tunel SSH
connections-ssh-tunnel-hint = Accesează serverul printr-un tunel SSH.
connections-ssh-host = Gazdă SSH
connections-ssh-port = Port SSH
connections-ssh-user = Utilizator SSH
connections-ssh-password = Parolă SSH
connections-ssh-agent-hint = Semnarea este delegată agentului SSH care rulează (agentul OpenSSH / Pageant pe Windows). Schemaic nu stochează nicio cheie.
connections-jump-hosts = Gazde intermediare
connections-jump-hosts-hint = Opțional. Gazde prin care se trece înainte de gazda SSH, cea mai apropiată prima — fiecare se autentifică în modul setat mai jos.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , cheia { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , prin { $jump }
       *[none] {""}
    }
connections-route-jump-host = gazda intermediară { $host }
connections-route-proxy-command = ~/.ssh/config ajunge la { $host } printr-un ProxyCommand, pe care Schemaic nu îl poate rula. Indică ruta în Gazde intermediare.
connections-route-match = ~/.ssh/config poate direcționa { $host } printr-un bloc Match, pe care Schemaic nu îl citește. Indică ruta în Gazde intermediare.
connections-route-match-destination = ~/.ssh/config poate redirecționa { $host } către altă gazdă sau alt port printr-un bloc Match, pe care Schemaic nu îl citește. Indică adresa însăși ca gazdă SSH.
connections-route-include = ~/.ssh/config include un fișier pe care Schemaic nu l-a putut citi acolo unde se aplică pentru { $host }. Indică adresa însăși ca gazdă SSH și ruta în Gazde intermediare.
connections-route-config-jump-password = ~/.ssh/config ajunge la { $host } prin { $jump }, iar o parolă este trimisă doar gazdelor numite de această conexiune. Adaugă gazda intermediară în Gazde intermediare dacă primește aceeași parolă sau autentifică-te cu o cheie ori cu agentul SSH.
connections-route-jump-own-proxy-jump = ~/.ssh/config ajunge la gazda intermediară { $host } prin { $jump }, iar Schemaic nu urmează ProxyJump-ul propriu al unei gazde intermediare. Indică ruta în Gazde intermediare, cu { $jump } prima.
connections-jump-host-error = Gazda intermediară { $number }: { $why }
connections-jump-empty = goală — elimină virgula în plus
connections-jump-space = „{ $entry }” conține un spațiu
connections-jump-unclosed = „{ $entry }” deschide un [ pe care nu îl închide
connections-jump-after-bracket = „{ $entry }” are text după ]
connections-jump-no-host = „{ $entry }” nu numește nicio gazdă
connections-jump-not-a-port = „{ $entry }” nu este un port
connections-private-key = Cheie privată
connections-private-key-dialog = Selectează cheia privată
connections-passphrase = Frază de acces
connections-tls = SSL / TLS
tls-disable = Disable
tls-prefer = Prefer
tls-require = Require
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Conectare în text simplu.
tls-prefer-description = Criptează dacă serverul oferă. Fără verificare.
tls-require-description = Refuză conectarea fără criptare. Fără verificare.
tls-verify-ca-description = Criptează și verifică certificatul față de CA.
tls-verify-full-description = Criptează, verifică certificatul și verifică numele gazdei.
tls-verify-ca-checks-host = Pe MySQL și MariaDB acesta verifică și numele gazdei, deci se comportă ca Verify full — o limitare a driverului, nu o setare.
tls-floor-azure-cli = Autentificarea Microsoft Entra se conectează la Verify full indiferent ce alegi aici: tokenul ei deschide fiecare bază de date Azure SQL la care contul are acces.
tls-floor-aws-iam = Autentificarea AWS IAM se conectează cel puțin la Require: tokenul ei nu trebuie să traverseze rețeaua necriptat. Verify full necesită pachetul CA RDS ca Root CA.
tls-floor-gcp-iam = Autentificarea Google Cloud IAM se conectează cel puțin la Verify CA — setează Root CA la CA-ul de server al instanței: tokenul ei deschide fiecare instanță Cloud SQL la care contul are acces.
tls-floor-other = Această autentificare se conectează într-un mod mai strict decât cel ales aici.
connections-ca-certificate = Certificat CA
connections-ca-certificate-dialog = Selectează certificatul CA
connections-ca-certificate-hint = Lasă gol pentru a avea încredere în autoritățile de certificare în care are încredere sistemul de operare, inclusiv cele instalate de organizația ta. Indică un fișier pentru a avea încredere în acela.
connections-client-certificate = Certificat client
connections-client-certificate-dialog = Selectează certificatul client
connections-client-key = Cheie client
connections-client-key-dialog = Selectează cheia client
connections-client-identity-hint = Un certificat și o cheie client sunt necesare doar acolo unde serverul îți cere să dovedești cine ești (TLS mutual). Cheia nu trebuie să fie protejată cu frază de acces.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importă în { $table }
import-section-source = Sursă
import-section-reading = Citire
import-section-columns = Coloane
import-section-preview = Previzualizare
import-file = Fișier
import-choose-file = Alege un fișier…
import-file-dialog = Importă în tabel
import-no-file = Niciun fișier ales
import-unnamed-file = (fișier)
import-format = Format
import-sheet = Foaie
import-delimiter = Delimitator
import-header-row = Primul rând este antet
import-header-row-hint = Folosește primul rând ca nume de coloane, nu ca date.
import-trim = Elimină spațiile albe
import-trim-hint = Elimină spațiile din jurul fiecărei valori — inclusiv din interiorul ghilimelelor.
import-empty-is-null = Câmpul gol este NULL
import-empty-is-null-hint = Un câmp gol înseamnă NULL, nu un șir gol.
import-null-tokens = Alte valori care înseamnă NULL
import-null-tokens-hint = Separate prin virgulă, de ex. NULL, \N, NA.
import-skip = Omite
import-nothing-mapped = Nimic nu este mapat, așa că nu există nimic de importat.
import-values-withheld = Prima înregistrare a acestui fișier este prea mare pentru previzualizare, așa că au fost citite doar numele coloanelor. Maparea și importul citesc întregul fișier.
import-missing-required = Nemapate și nu pot fi goale: { $columns }. Importul va eșua dacă baza de date nu le completează.
import-engine-not-transactional = Motorul de stocare al acestui tabel ({ $engine }) nu este tranzacțional, așa că un import eșuat nu poate fi anulat — rândurile încărcate înainte de eșec vor rămâne.
import-problems = { $count ->
    [one] { $count } problemă în fișier — nu s-a importat nimic.
    [few] { $count } probleme în fișier — nu s-a importat nimic.
   *[other] { $count } de probleme în fișier — nu s-a importat nimic.
}
import-problem-at-line = rândul { $line }: { $message }
import-problem-at-column = rândul { $line }, { $column }: { $message }
import-issue-not-an-integer = nu este un număr întreg
import-issue-not-a-number = nu este un număr
import-issue-not-a-boolean = nu este o valoare adevărat/fals
import-issue-cell-error = foaia nu a putut evalua această celulă
import-issue-null-in-not-null = gol, dar coloana nu poate fi NULL
import-issue-field-count = are câmpuri: { $found }, așteptate: { $expected }
import-tail-cut = Se afișează { $shown } din { $total }.
import-tail-uncounted = Se afișează toate cele { $total } — verificarea s-a oprit din numărat acolo, așa că fișierul conține mai multe.
import-tail-cut-and-uncounted = Se afișează { $shown } din primele { $total } — verificarea s-a oprit din numărat acolo, așa că fișierul conține mai multe.
import-memory-json = Acest fișier JSON are { $size }, iar un import JSON este ținut în memorie cât se încarcă — așteaptă-te să aibă nevoie de aproximativ { $estimate }. Un CSV cu aceleași date se încarcă în memorie constantă.
import-memory-xlsx = Acest registru are { $size }, iar un import Excel este ținut în memorie cât se încarcă — așteaptă-te să aibă nevoie de aproximativ { $estimate }, deoarece fișierul este comprimat. Un CSV cu aceeași foaie se încarcă în memorie constantă.
import-read-failed = Fișierul nu a putut fi citit: { $error }
import-no-columns-mapped = Nicio coloană din fișier nu este mapată la coloane din tabel
import-preview-record-too-large = prima sa înregistrare JSON este mai mare decât cei { $mib } MiB pe care îi citește previzualizarea, așa că nu există nimic de afișat
import-refuse-not-objects = se așteptau obiecte JSON (un tablou de obiecte sau câte unul pe rând)
import-refuse-oversize = Acest registru are { $size }, iar un fișier Excel trebuie citit în întregime înainte ca ceva din el să poată fi afișat — { $max } este maximul pe care îl poate deschide. Exportă foaia ca CSV și importă-l pe acela; un CSV se încarcă în memorie constantă.
import-refuse-oversize-unsized = Acest registru are peste { $max }, iar un fișier Excel trebuie citit în întregime înainte ca ceva din el să poată fi afișat — { $max } este maximul pe care îl poate deschide. Exportă foaia ca CSV și importă-l pe acela; un CSV se încarcă în memorie constantă.
import-refuse-inflated = Acest registru are { $size } pe disc, dar se dezarhivează la cel puțin { $unpacked }, iar un fișier Excel trebuie citit în întregime înainte ca ceva din el să poată fi afișat — { $ceiling } de conținut dezarhivat este maximul pe care îl deschide, iar pentru a-l ține are nevoie de mai multă memorie decât atât. Exportă foaia ca CSV și importă-l pe acela; un CSV se încarcă în memorie constantă.
import-refuse-no-such-sheet = acest registru nu are o foaie numită „{ $sheet }” — are { $sheets }
import-refuse-no-sheets = acest registru nu are foi
import-refuse-too-wide = această foaie declară { $width } coloane; o foaie Excel are { $max }. Exporteaz-o ca CSV și importă-l pe acela.
import-cancelled = Import anulat — tranzacția a fost anulată prin rollback, așa că nu s-a scris nimic.
import-done = { $count ->
    [one] S-a importat { $count } rând în { $table }.
    [few] S-au importat { $count } rânduri în { $table }.
   *[other] S-au importat { $count } de rânduri în { $table }.
}
import-next = Următorul
import-reading = Se citește…
import-back = Înapoi
import-import = Importă
import-importing = Se importă…
dialog-read-only = Această conexiune este doar pentru citire.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Exportă { $database }.{ $schema } în { $format }
dump-title = Exportă { $database } în { $format }
dump-sql-dialog = Exportă în SQL
dump-folder-dialog = Exportă în { $format }
dump-folder-hint = Câte un fișier { $format } pentru fiecare tabel, scris într-un dosar ales de tine.
dump-all-selected = Toate cele { $total } selectate
dump-some-selected = { $chosen } din { $total } selectate
dump-select-all = Selectează tot
dump-select-none = Nimic
dump-reading-tables = Se citește lista de tabele…
dump-no-tables = Această bază de date nu are tabele.
dump-tables-unreadable = Lista de tabele nu a putut fi citită.
dump-section-contents = Conținut
dump-structure = Structură
dump-structure-hint = CREATE TABLE, triggerele sale și cheile străine, reformulate după date.
dump-data = Date
dump-data-hint = Fiecare rând, ca instrucțiuni INSERT.
dump-other-objects = Alte obiecte
dump-other-objects-hint = Tipurile, secvențele, rutinele și evenimentele de care depind aceste tabele.
dump-section-replaying = Rejucare
dump-drop-before-create = Elimină înainte de creare
dump-drop-hint-up-front = Elimină fiecare tabel și vizualizare înainte de CREATE, ca fișierul să se încarce într-o bază de date care le conține deja. Cu O singură tranzacție înlocuiește și fiecare rutină din schemele lor și se oprește înainte de a elimina ceva ce nu poate pune la loc așa cum era. Fără O singură tranzacție lasă rutinele așa cum sunt, așa că o rejucare într-o bază de date care le are încă se oprește la prima. Proprietarul și permisiunile fiecărui tabel sunt puse la loc la final.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS înainte de fiecare CREATE, ca fișierul să se încarce într-o bază de date care conține deja aceste tabele. Înlocuiește și fiecare rutină din schemele lor. Proprietarul, permisiunile acordate și securitatea la nivel de rând ale fiecărui tabel sunt puse la loc la final.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS înainte de fiecare CREATE, ca fișierul să se încarce într-o bază de date care conține deja aceste tabele. Înlocuiește și fiecare rutină și eveniment din schemele lor: pe MySQL fiecare este eliminat mai întâi, ceea ce revocă toate permisiunile acordate pe el, iar fișierul le acordă din nou la final, acolo unde contul tău poate citi mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS înainte de fiecare CREATE, ca fișierul să se încarce într-o bază de date care conține deja aceste tabele.
dump-one-transaction = O singură tranzacție
dump-one-transaction-hint = Încadrează încărcarea, ca un eșec să nu lase nimic în urmă.
dump-disable-fk-checks = Dezactivează verificările cheilor străine
dump-fk-hint = Dezactivate pe durata încărcării, ca ordinea în care sosesc rândurile să nu le poată refuza.
dump-fk-hint-drops = Dezactivate pe durata încărcării, ca ordinea în care sosesc rândurile să nu le poată refuza. Un fișier care elimină tabele le dezactivează oricum: o eliminare cu ele active șterge rândurile tabelelor din afara fișierului prin acțiunile lor ON DELETE.
dump-no-fk-guard-hint = Acest motor nu are un comutator de sesiune pentru verificările cheilor străine; în schimb, fișierul adaugă constrângerile după date.
dump-export = Exportă
dump-progress = { $count ->
    [one] Se scrie { $table } — { $index } din { $total }, { $rows } rând până acum
    [few] Se scrie { $table } — { $index } din { $total }, { $rows } rânduri până acum
   *[other] Se scrie { $table } — { $index } din { $total }, { $rows } de rânduri până acum
}
dump-reading-schema = Se citește schema
dump-replace-files = Înlocuiește fișierele
dump-replace-prompt = { $n ->
    [one] { $n } fișier din { $folder } va fi înlocuit și nu poate fi recuperat: { $names }.
    [few] { $n } fișiere din { $folder } vor fi înlocuite și nu pot fi recuperate: { $names }.
   *[other] { $n } de fișiere din { $folder } vor fi înlocuite și nu pot fi recuperate: { $names }.
}
dump-replace-prompt-more = { $n ->
    [few] { $n } fișiere din { $folder } vor fi înlocuite și nu pot fi recuperate: { $names } și încă { $more }.
   *[other] { $n } de fișiere din { $folder } vor fi înlocuite și nu pot fi recuperate: { $names } și încă { $more }.
}
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] S-a scris { $n } tabel.
    [few] S-au scris { $n } tabele.
   *[other] S-au scris { $n } de tabele.
}
dump-done-missing = { $n ->
    [one] { $n } tabel bifat nu a fost găsit și nu este în fișier: { $names }.
    [few] { $n } tabele bifate nu au fost găsite și nu sunt în fișier: { $names }.
   *[other] { $n } de tabele bifate nu au fost găsite și nu sunt în fișier: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } obiect cerut de export nu este în fișier: { $names }.
    [few] { $n } obiecte cerute de export nu sunt în fișier: { $names }.
   *[other] { $n } de obiecte cerute de export nu sunt în fișier: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Rândurile tabelului de muchii de graf { $names } nu sunt în fișier; este creat gol.
   *[other] Rândurile tabelelor de muchii de graf { $names } nu sunt în fișier; sunt create goale.
}
dump-done-refused = { $n ->
    [one] O rejucare într-o bază de date care îl conține deja se oprește înainte de a elimina ceva: fișierul nu îl poate pune la loc așa cum este: { $names }.
   *[other] O rejucare într-o bază de date care conține deja oricare dintre acestea se oprește înainte de a elimina ceva: fișierul nu le poate pune la loc așa cum sunt: { $names }.
}
dump-cancelled-partial = Export anulat — { $name } nu a fost modificat; ce se scrisese este în { $part }
files-done = { $n ->
    [one] S-a scris { $n } fișier.
    [few] S-au scris { $n } fișiere.
   *[other] S-au scris { $n } de fișiere.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Export anulat — niciun fișier nu a fost terminat, așa că nu s-a scris nimic în { $folder }.
files-cancelled = { $n ->
    [one] Export anulat — { $n } fișier a fost terminat în { $folder }; tabelul în curs a rămas nescris.
    [few] Export anulat — { $n } fișiere au fost terminate în { $folder }; tabelul în curs a rămas nescris.
   *[other] Export anulat — { $n } de fișiere au fost terminate în { $folder }; tabelul în curs a rămas nescris.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } fișier deja scris în { $folder } este păstrat.
    [few] { $message } — { $n } fișiere deja scrise în { $folder } sunt păstrate.
   *[other] { $message } — { $n } de fișiere deja scrise în { $folder } sunt păstrate.
}
files-replaced = { $n ->
    [one] { $n } fișier existent a fost înlocuit: { $names }.
    [few] { $n } fișiere existente au fost înlocuite: { $names }.
   *[other] { $n } de fișiere existente au fost înlocuite: { $names }.
}
files-missing = { $n ->
    [one] { $n } tabel bifat nu a fost găsit și nu a devenit fișier: { $names }.
    [few] { $n } tabele bifate nu au fost găsite și nu au devenit fișiere: { $names }.
   *[other] { $n } de tabele bifate nu au fost găsite și nu au devenit fișiere: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } din ~{ $total } rând
    [few] { $rows } din ~{ $total } rânduri
   *[other] { $rows } din ~{ $total } de rânduri
}
export-progress-of = { $count ->
    [one] { $rows } din { $total } rând
    [few] { $rows } din { $total } rânduri
   *[other] { $rows } din { $total } de rânduri
}
export-progress-rows = { $count ->
    [one] { $rows } rând
    [few] { $rows } rânduri
   *[other] { $rows } de rânduri
}
export-progress-starting = Se pornește
export-progress-file = Fișier: { $name }
export-progress-writing = Se scrie { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Monitor live
monitor-badge-watching = Se urmărește
monitor-badge-paused = În pauză
monitor-badge-stopped = Oprit
monitor-filter-all = Toate
monitor-filter-inserts = Inserări
monitor-filter-updates = Actualizări
monitor-filter-deletes = Ștergeri
monitor-resume = Reia interogarea periodică
monitor-pause = Întrerupe interogarea periodică
monitor-clear-tip = Golește jurnalul
monitor-export-tip = Exportă jurnalul…
monitor-interval = { $secs } s
monitor-clear-title = Golești jurnalul?
monitor-clear-message = { $count ->
    [one] { $count } modificare ar fi eliminată. Jurnalul este singura evidență a ceea ce conținea un rând șters — interogarea periodică nu va mai raporta o modificare — și acțiunea nu poate fi anulată. Exportă-l mai întâi dacă vrei să-l păstrezi.
    [few] { $count } modificări ar fi eliminate. Jurnalul este singura evidență a ceea ce conținea un rând șters — interogarea periodică nu va mai raporta o modificare — și acțiunea nu poate fi anulată. Exportă-l mai întâi dacă vrei să-l păstrezi.
   *[other] { $count } de modificări ar fi eliminate. Jurnalul este singura evidență a ceea ce conținea un rând șters — interogarea periodică nu va mai raporta o modificare — și acțiunea nu poate fi anulată. Exportă-l mai întâi dacă vrei să-l păstrezi.
}
monitor-col-time = Ora
monitor-col-action = Acțiune
monitor-no-inserts = Nicio inserare încă.
monitor-no-updates = Nicio actualizare încă.
monitor-no-deletes = Nicio ștergere încă.
monitor-empty-stopped = Oprit.
monitor-empty-paused = În pauză.
monitor-waiting = Se așteaptă
monitor-events-since = { $count ->
    [one] { $count } eveniment din { $since }
    [few] { $count } evenimente din { $since }
   *[other] { $count } de evenimente din { $since }
}
monitor-no-changes = Nicio modificare încă
monitor-partial = sunt urmărite doar primele { $cap } rânduri după cheia primară
monitor-capped = cele mai vechi se elimină la limita de { $cap } modificări
monitor-length-only = { $count ->
    [one] { $columns } este binară, așa că modificările ei sunt văzute doar după lungime
   *[other] { $columns } sunt binare, așa că modificările lor sunt văzute doar după lungime
}
monitor-newest-at-bottom = cele mai noi jos
monitor-export-dialog = Exportă jurnalul de modificări

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Compară rezultatele
diff-filter-all = Toate
diff-filter-changed = Modificate
diff-filter-added = Adăugate
diff-filter-removed = Eliminate
diff-match-rows-by = Potrivește rândurile după
diff-whole-row = Rândul întreg
diff-export-tip = Exportă comparația…
diff-nothing-to-export = Nimic de exportat.
diff-export-dialog = Exportă comparația
diff-unnamed-file = comparația
diff-ran-at = Rulat la { $time }
diff-side-facts = { $count ->
    [one] { $count } rând · { $ms } ms
    [few] { $count } rânduri · { $ms } ms
   *[other] { $count } de rânduri · { $ms } ms
}
diff-only-in = Doar în { $side }: { $columns }
diff-uncompared = Binare, necomparate: { $columns }
diff-missing-key = ambele rezultate au nevoie de o coloană numită { $column } după care să potrivească rândurile
diff-binary-key = { $column } conține valori binare, care nu sunt comparate, așa că nu poate potrivi rânduri
diff-summary-identical = { $n ->
        [one] Nicio diferență · { $n } rând nemodificat
        [few] Nicio diferență · { $n } rânduri nemodificate
       *[other] Nicio diferență · { $n } de rânduri nemodificate
    }
diff-summary-changed = { $n } modificate
diff-summary-added = { $n } adăugate
diff-summary-removed = { $n } eliminate
diff-summary-unchanged = { $n } nemodificate
diff-summary-not-compared = { $columns } necomparate
diff-repeated-keys = { $count ->
    [one] { $count } rând repetă o cheie deja folosită de partea sa și este omis.
    [few] { $count } rânduri repetă o cheie deja folosită de partea lor și sunt omise.
   *[other] { $count } de rânduri repetă o cheie deja folosită de partea lor și sunt omise.
}
diff-capped = Un rezultat a fost limitat: rândurile de după limită nu sunt comparate.
diff-col-change = Modificare
diff-no-differences = Nicio diferență.
diff-no-changed = Niciun rând modificat.
diff-no-added = Niciun rând adăugat.
diff-no-removed = Niciun rând eliminat.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Înainte de a aplica
preview-risk-irreversible = Acest lucru nu poate fi anulat

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Creează tabelul { $name } cu { $columns } coloană
    [few] Creează tabelul { $name } cu { $columns } coloane
   *[other] Creează tabelul { $name } cu { $columns } de coloane
}
ddl-line-rebuild-table = Reconstruiește { $name } — nu poate fi modificat pe loc, așa că rândurile sunt copiate într-un tabel nou, care îi ia locul celui vechi
ddl-line-drop-table = Elimină tabelul
ddl-line-drop-foreign-table = Elimină tabelul extern
ddl-line-truncate-table = Șterge toate rândurile
ddl-line-rename-table = Redenumește tabelul în { $to }
ddl-line-add-column = Adaugă coloana { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Elimină coloana { $name }
ddl-line-rebuild-computed-column = Reconstruiește coloana calculată { $name } ca ({ $expression }) în jurul modificării coloanei
ddl-line-rename-column = Redenumește coloana { $from } în { $to }{ $retyped ->
        [yes] , iar tipul ei din { $old_type } în { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Mută coloana { $name } pe primul loc
ddl-line-move-column-after = Mută coloana { $name } după { $after }
ddl-line-move-column = Mută coloana { $name }
ddl-line-retype-column = Modifică coloana { $name } din { $from } în { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Modifică coloana { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (niciuna)
ddl-line-add-primary-key = Adaugă cheia primară ({ $columns })
ddl-line-drop-primary-key = Elimină cheia primară ({ $columns })
ddl-line-rebuild-primary-key = Reconstruiește cheia primară ({ $columns }) în jurul modificării coloanei
ddl-line-change-primary-key = Modifică cheia primară din ({ $from }) în ({ $to })
ddl-line-add-index = Adaugă indexul{ $unique ->
        [yes] {" "}unic
       *[no] {""}
    }{ $has_kind ->
        [yes] {" "}{ $kind }
       *[no] {""}
    } { $name }{ $has_method ->
        [yes] {" "}folosind { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}pe ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}incluzând ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}ordonat după ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}unde { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = columnstore cluster
ddl-index-kind-primary-xml = XML primar
ddl-index-kind-selective-xml = XML selectiv
ddl-index-kind-secondary-xml = XML secundar
ddl-index-kind-spatial = spațial
ddl-line-drop-index = Elimină indexul { $name }
ddl-line-rename-index = Redenumește indexul { $from } în { $to }
ddl-line-keep-lossy-index = Lasă indexul { $name } neschimbat — Schemaic nu îl poate citi complet
ddl-line-add-foreign-key = Adaugă cheia străină { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Elimină cheia străină { $name }
ddl-line-add-check = Adaugă verificarea { $name } ({ $expression })
ddl-line-drop-check = Elimină verificarea { $name }
ddl-line-table-options = Setează pentru tabel { $parts }
ddl-table-option-engine = motorul la { $engine }
ddl-table-option-collation = colaționarea la { $collation }
ddl-table-option-comment = comentariul
ddl-table-option-comment-cleared = comentariul (golit)
ddl-line-create-view = Creează vizualizarea { $name }
ddl-line-create-view-index = Creează vizualizarea { $name } și indexul ei { $index }
ddl-line-create-view-indexes = Creează vizualizarea { $name } și indecșii ei { $indexes }
ddl-line-redefine-view = Redefinește vizualizarea { $name }
ddl-line-recreate-view = Elimină și recreează vizualizarea { $name }
ddl-line-rename-view = Redenumește vizualizarea în { $to }
ddl-line-drop-view = Elimină vizualizarea
ddl-line-drop-materialized-view = Elimină vizualizarea materializată
ddl-line-refresh-view = Reconstruiește rândurile vizualizării materializate — cititorii sunt blocați până la final
ddl-line-refresh-view-concurrently = Reconstruiește rândurile vizualizării materializate fără a bloca cititorii
ddl-line-create-trigger = Creează triggerul { $name }
ddl-line-redefine-trigger = Redefinește triggerul { $name }
ddl-line-recreate-trigger = Recreează triggerul { $name }
ddl-line-recreate-trigger-as = Recreează triggerul { $from } ca { $name }
ddl-line-drop-trigger = Elimină triggerul { $name }
ddl-line-create-routine = Creează { $what } { $name }
ddl-line-redefine-routine = Redefinește { $what } { $name }
ddl-line-redefine-routine-as = Redefinește { $what } { $from } ca { $name }
ddl-line-recreate-routine = Recreează { $what } { $name }
ddl-line-recreate-routine-as = Recreează { $what } { $from } ca { $name }
ddl-line-rename-routine = Redenumește { $what } { $from } în { $to }
ddl-line-drop-routine = Elimină { $what } { $name }
ddl-line-create-event = Creează evenimentul { $name }
ddl-line-alter-event = Modifică evenimentul { $name }
ddl-line-alter-event-edits = Modifică evenimentul { $name }: { $edits }
ddl-line-drop-event = Elimină evenimentul { $name }
ddl-event-edit-rename = redenumire în { $to }
ddl-event-edit-schedule = programare la { $schedule }
ddl-event-edit-status-enabled = stare: activat
ddl-event-edit-status-disabled = stare: dezactivat
ddl-event-edit-status-replica = stare: dezactivat pe replică
ddl-event-edit-preserve = păstrat după încheiere
ddl-event-edit-no-preserve = șters după încheiere
ddl-event-edit-definer = definitor: { $definer }
ddl-event-edit-comment = comentariu
ddl-event-edit-body = corp
ddl-line-create-enum = { $values ->
    [one] Creează tipul { $name } cu { $values } valoare
    [few] Creează tipul { $name } cu { $values } valori
   *[other] Creează tipul { $name } cu { $values } de valori
}
ddl-line-add-enum-value = Adaugă valoarea { $value }
ddl-line-add-enum-value-after = Adaugă valoarea { $value } după { $after }
ddl-line-add-enum-value-before = Adaugă valoarea { $value } înainte de { $before }
ddl-line-rename-enum-value = Redenumește valoarea { $from } în { $to }
ddl-line-recreate-enum = Recreează tipul { $name } ({ $values ->
    [one] { $values } valoare
    [few] { $values } valori
   *[other] { $values } de valori
}, reconvertind { $columns ->
    [one] { $columns } coloană
    [few] { $columns } coloane
   *[other] { $columns } de coloane
})
ddl-line-create-domain = Creează domeniul { $name } ca { $datatype }
ddl-line-set-domain-default = Setează valoarea implicită a domeniului la { $default }
ddl-line-drop-domain-default = Elimină valoarea implicită a domeniului
ddl-line-domain-not-null = Fă domeniul NOT NULL
ddl-line-domain-nullable = Fă domeniul să accepte NULL
ddl-line-recreate-domain = Recreează domeniul { $name } ca { $datatype } (era { $was }; reconvertind { $columns ->
    [one] { $columns } coloană
    [few] { $columns } coloane
   *[other] { $columns } de coloane
})
ddl-line-create-sequence = Creează secvența { $name }
ddl-line-alter-sequence = Setează pentru secvență { $edits }
ddl-sequence-edit-type = tipul la { $datatype }
ddl-sequence-edit-increment = incrementul la { $value }
ddl-sequence-edit-minimum = minimul la { $value }
ddl-sequence-edit-maximum = maximul la { $value }
ddl-sequence-edit-start = startul la { $value }
ddl-sequence-edit-cache = cache-ul la { $value }
ddl-sequence-edit-cycling-on = ciclarea activată
ddl-sequence-edit-cycling-off = ciclarea dezactivată
ddl-sequence-edit-owner = proprietarul la { $owner }
ddl-sequence-edit-no-owner = fără proprietar
ddl-line-restart-sequence = Repornește secvența la { $value }
ddl-line-create-object = Creează { $what } { $name }
ddl-line-replace-object = Elimină { $what } { $name } și creează-l din nou
ddl-line-replace-object-as = Elimină { $what } { $name } și creează-l din nou ca { $to }
ddl-line-alter-tsql-sequence = Modifică secvența: { $clauses }
ddl-line-rename-object = Redenumește { $what } în { $to }
ddl-line-drop-object = Elimină { $what }
ddl-line-set-object-comment = Setează comentariul pentru { $what }
ddl-line-clear-object-comment = Golește comentariul pentru { $what }
ddl-line-create-database = Creează baza de date { $name }
ddl-line-drop-database = Elimină baza de date { $name }
ddl-line-create-schema = Creează schema { $name }
ddl-line-drop-schema = Elimină schema { $name }
ddl-line-create-extension = Creează extensia { $name }
ddl-line-create-user = Creează utilizatorul { $who }
ddl-line-create-role = Creează rolul { $who }
ddl-line-create-login = Creează loginul { $who }
ddl-line-drop-user = Elimină utilizatorul { $who }
ddl-line-drop-role = Elimină rolul { $who }
ddl-line-drop-login = Elimină loginul { $who }
ddl-line-set-password = Setează parola pentru { $who }
ddl-line-grant-privileges = Acordă { $privileges } lui { $who }
ddl-line-revoke-privileges = Revocă { $privileges } de la { $who }
ddl-privileges-none = niciun privilegiu
ddl-privileges-count = { $count ->
    [one] { $count } privilegiu
    [few] { $count } privilegii
   *[other] { $count } de privilegii
}
ddl-line-grant-role = Acordă { $role } lui { $member }
ddl-line-revoke-role = Revocă { $role } de la { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Elimină { $name } și îl recreează, copiind fiecare rând, și pune la loc cheile, indecșii, verificările, valorile implicite și triggerele, comentariile lui și ale coloanelor, precum și cheile străine pe care alte tabele le au spre el. Se oprește înainte de a începe dacă tabelul are orice altceva ce Schemaic nu reformulează — permisiuni, un proprietar propriu, un dependent legat de schemă, versionare de sistem, un registru ledger, replicare, compresie, opțiuni de index, un index dezactivat, o regulă sau valoare implicită legată, o clasificare de sensibilitate, o descriere pe o cheie, un index, o valoare implicită, o verificare sau un trigger. Statisticile create manual nu sunt preluate.
       *[other] Elimină { $name } și îl recreează, copiind fiecare rând. Orice aspect al tabelului pe care Schemaic nu l-a citit nu este preluat.
    }
ddl-risk-drop-table = Elimină tabelul și toate rândurile din el.
ddl-risk-drop-foreign-table = Elimină tabelul extern: definiția lui dispare, iar rândurile pe care le citește rămân unde sunt, pe serverul extern sau în fișier; o vizualizare sau o rutină care îl numește eșuează de atunci înainte.
ddl-risk-truncate-table = Șterge toate rândurile din tabel. Acțiunea nu poate fi anulată.
ddl-risk-drop-column = Elimină coloana { $name } și toate datele din ea.
ddl-risk-rebuild-computed-column = Elimină coloana calculată { $name } și o adaugă din nou, așa că ajunge la sfârșitul tabelului.
ddl-risk-column-not-null = Coloana { $name } devine NOT NULL — instrucțiunea eșuează dacă vreun rând conține NULL.
ddl-risk-column-stops-generated = Coloana { $name } nu mai este generată. Valorile actuale sunt păstrate, dar nimic nu le mai recalculează de acum înainte.
ddl-risk-column-becomes-generated = Coloana { $name } devine generată din { $expression }. Fiecare valoare din ea este înlocuită cu ce calculează expresia.
ddl-risk-column-expression-changes = Expresia coloanei { $name } se schimbă din { $from } în { $to }. Pe PostgreSQL aceasta elimină și readaugă coloana, așa că fiecare valoare este recalculată și coloana ajunge la sfârșitul tabelului.
ddl-risk-type-narrows = Restrângerea { $subject } din { $from } în { $to } { $loss ->
        [truncates] trunchiază valorile care nu mai încap
        [rounds] rotunjește fiecare valoare din coloană
       *[both] trunchiază valorile care nu mai încap și rotunjește fiecare valoare din coloană
    }.
ddl-risk-type-rewrites = Schimbarea { $subject } din { $from } în { $to } rescrie fiecare valoare; poate eșua sau pierde precizie.
ddl-risk-domain-subject = domeniului { $name }
ddl-risk-primary-key-dropped = Lasă tabelul fără cheie primară — rândurile nu mai pot fi editate din grilă.
ddl-risk-primary-key-rebuilt = Elimină și readaugă cheia primară în jurul modificării coloanei. Când este indexul cluster, rescrie întregul tabel, iar o cheie străină din alt tabel care face referire la ea oprește planul.
ddl-risk-primary-key-replaced = Înlocuiește cheia primară: { $count ->
        [one] { $columns } nu mai este unică
       *[other] { $columns } nu mai sunt unice
    }, iar duplicatele sunt acceptate de acum înainte. Pe MySQL aceasta reconstruiește indexul cluster, ceea ce rescrie întregul tabel.
ddl-risk-drop-foreign-key = Elimină cheia străină { $name }. Rândurile care nu mai au un părinte corespunzător sunt acceptate de acum înainte, iar datele existente nu sunt reverificate dacă este adăugată din nou.
ddl-risk-drop-unique-index = Elimină indexul unic { $name }. Valorile duplicate sunt acceptate de acum înainte, iar grila poate pierde cheia după care edita rândurile.
ddl-risk-drop-view = { $materialized ->
        [yes] Elimină vizualizarea materializată. { $cost }
       *[no] Elimină vizualizarea. { $cost }
    }
ddl-risk-view-cost-sqlite = Vizualizările care selectează din ea nu se mai rezolvă până când revine, iar triggerele ei INSTEAD OF sunt eliminate odată cu ea și nu sunt restaurate.
ddl-risk-view-cost-sqlite-restored = Vizualizările care selectează din ea nu se mai rezolvă până când revine, iar triggerele ei INSTEAD OF sunt eliminate odată cu ea.
ddl-risk-view-cost-sqlserver = Permisiunile acordate pe ea, descrierea și triggerele ei INSTEAD OF sunt eliminate odată cu ea și nu sunt restaurate.
ddl-risk-view-cost-sqlserver-restored = Permisiunile acordate pe ea și descrierea ei sunt eliminate odată cu ea și nu sunt restaurate; triggerele ei INSTEAD OF sunt și ele eliminate.
ddl-risk-view-cost-other = Vizualizările dependente, regulile, permisiunile acordate și triggerele ei INSTEAD OF sunt eliminate odată cu ea și nu sunt restaurate.
ddl-risk-view-cost-other-restored = Vizualizările dependente, regulile și permisiunile acordate sunt eliminate odată cu ea și nu sunt restaurate; triggerele ei INSTEAD OF sunt și ele eliminate.
ddl-risk-recreate-view = Recrearea { $name } o elimină mai întâi. { $cost } { $why ->
        [sqlite] SQLite nu are o modalitate de a înlocui o vizualizare pe loc, așa că fiecare editare a uneia — inclusiv o redenumire — urmează această cale.
        [sqlserver] sp_rename din SQL Server ar lăsa definiția stocată a vizualizării cu numele vechi, așa că o redenumire urmează această cale.
       *[postgres] PostgreSQL nu poate înlocui o vizualizare ale cărei coloane și-au schimbat numele, tipul sau ordinea, așa că aceasta este singura cale de a aplica editarea.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } instrucțiune eliminată odată cu ea este pusă la loc după aceea, în aceeași tranzacție.
        [few] { $count } instrucțiuni eliminate odată cu ea sunt puse la loc după aceea, în aceeași tranzacție.
       *[other] { $count } de instrucțiuni eliminate odată cu ea sunt puse la loc după aceea, în aceeași tranzacție.
    }
ddl-risk-view-indexes-dropped = Redefinirea vizualizării { $view } îi elimină { $dropped_count ->
        [one] indexul
       *[other] indecșii
    } ({ $dropped }) — ALTER VIEW din SQL Server face mereu asta — { $case ->
        [all] iar acest plan { $again_count ->
            [one] îl reconstruiește
           *[other] îi reconstruiește
        } după aceea, în aceeași tranzacție. Pe o vizualizare mare durează cât a durat construirea { $again_count ->
            [one] lui
           *[other] lor
        } inițial. Un index cu fill factor, compresie, padding, opțiuni de blocare, un filegroup sau o descriere oprește planul înainte de orice modificare — Schemaic nu le poate reconstrui. Statisticile create manual pe vizualizare nu sunt preluate.
        [none] iar acest plan nu { $gone_count ->
            [one] îl reconstruiește
           *[other] îi reconstruiește
        }: vizualizarea cu care trebuie să coincidă nu are un asemenea index. Statisticile create manual pe vizualizare nu sunt preluate.
       *[some] iar acest plan reconstruiește { $again } după aceea, în aceeași tranzacție, dar nu și { $gone }, pe care vizualizarea cu care trebuie să coincidă nu îl are. Un index cu fill factor, compresie, padding, opțiuni de blocare, un filegroup sau o descriere oprește planul înainte de orice modificare — Schemaic nu le poate reconstrui. Statisticile create manual pe vizualizare nu sunt preluate.
    }
ddl-risk-add-not-null-column = Coloana { $name } este NOT NULL fără valoare implicită. Dacă tabelul are deja rânduri, nu există o valoare care să le fie dată, iar motorul va refuza întregul plan.
ddl-risk-drop-check = Elimină verificarea { $name }. Rândurile pe care constrângerea le refuza sunt acceptate de acum înainte, iar datele existente nu sunt reverificate dacă este adăugată din nou.
ddl-risk-keep-lossy-index = Editarea ta a indexului { $name } nu este inclusă. Acesta folosește ceva ce Schemaic nu poate citi înapoi — o coloană de cheie de tip expresie, o clasă de operatori sau o ordonare NULLS — iar aplicarea editării ar însemna recrearea indexului fără acestea. Editează acest index în SQL.
ddl-risk-drop-trigger = Elimină triggerul { $name }. Ce făcea la fiecare scriere încetează, iar rândurile scrise de acum înainte nu vor mai fi afectate de el.
ddl-risk-redefine-trigger-in-place = Redefinește triggerul { $name } pe loc — nu este eliminat, așa că permisiunile lui rămân — iar ce face la fiecare scriere se schimbă de acum înainte.
ddl-risk-recreate-trigger = Recrearea triggerului { $name } îl elimină mai întâi. Acolo unde DDL nu este tranzacțional (MySQL), o definiție nouă respinsă de server lasă tabelul fără trigger.
ddl-risk-signature-lost = { $kind ->
        [trigger] Triggerul { $name } este semnat
        [function] Funcția { $name } este semnată
       *[procedure] Procedura { $name } este semnată
    } (ADD SIGNATURE), iar orice modificare elimină semnătura — Schemaic nu o poate semna din nou fără cheia certificatului. Ce îi permitea certificatul să facă nu mai funcționează până când rulezi din nou ADD SIGNATURE.
ddl-risk-redefines-routine = Redefinește { $name }. Tot ce o apelează rulează noul corp de acum înainte, inclusiv triggere pe alte tabele pe care această editare nu le-a menționat niciodată.
ddl-risk-routine-parameters-change = Modifică lista de parametri a { $name }. Apelanții care transmit argumentele vechi nu se mai rezolvă la ea.
ddl-risk-recreate-routine = Recrearea { $held } o elimină mai întâi. Acolo unde DDL nu este tranzacțional (MySQL), o definiție nouă respinsă de server nu lasă deloc { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Privilegiile acordate pe { $held } și comentariul ei nu supraviețuiesc eliminării, iar acest plan nu le restaurează — reaplică după aceea orice GRANT și COMMENT.
       *[no] Privilegiile acordate pe { $held } nu supraviețuiesc eliminării, iar acest plan nu le restaurează — reaplică după aceea orice GRANT.
    }
ddl-risk-drop-numbered-group = Elimină { $what } { $name } și, odată cu ea, fiecare membru al grupului ei numerotat: { $members }.
ddl-risk-drop-routine = Elimină { $what } { $name }. PostgreSQL refuză cât timp un trigger încă o folosește, așa că acelea trebuie eliminate mai întâi.
ddl-risk-drop-event = Elimină evenimentul { $name }. Programarea și corpul lui dispar odată cu el — nimic altceva din baza de date nu păstrează o copie.
ddl-risk-one-shot-event = Acest eveniment rulează o singură dată, apoi serverul îl șterge — acesta este ON COMPLETION NOT PRESERVE. Activează Păstrează după încheiere pentru a-l păstra.
ddl-risk-one-shot-event-altered = Evenimentul { $name } va rula o singură dată, apoi serverul îl va șterge — acesta este ON COMPLETION NOT PRESERVE. Activează Păstrează după încheiere pentru a-l păstra.
ddl-risk-add-enum-value = Adăugarea { $value } nu poate fi anulată pe loc — PostgreSQL nu poate elimina o valoare enum, așa că retragerea ei înseamnă recrearea tipului. De asemenea, valoarea nu poate fi folosită până când acest plan nu este aplicat.
ddl-risk-rename-enum-value = Fiecare rând care conține { $from } va citi { $to } de acum înainte. Dacă voiai să elimini { $from } în loc să-l redenumești, șterge-l și aplică separat această modificare.
ddl-risk-recreate = Recrearea { $kind ->
        [domain] domeniului
       *[type] tipului
    } { $name } îl elimină și îl reconstruiește. Fiecare coloană de mai jos este reconvertită prin text; o valoare pe care noua definiție nu o acceptă face întregul plan să eșueze și nu se aplică nimic{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}și încă { $more }
        }. O vizualizare sau o funcție construită pe el nu poate fi listată aici și ar face serverul să refuze.
       *[no] . Nimic nu îl folosește azi, dar o vizualizare sau o funcție construită pe el nu poate fi listată aici și ar face serverul să refuze.
    }
ddl-risk-drop-domain-check = Elimină verificarea { $name }. Nicio coloană a acestui domeniu nu mai este verificată, iar datele existente nu sunt reverificate dacă este adăugată din nou.
ddl-risk-domain-nullable = Fiecare coloană a acestui domeniu începe să accepte NULL, inclusiv cele din tabele pe care această editare nu le-a menționat niciodată.
ddl-risk-domain-not-null = Instrucțiunea eșuează dacă vreo coloană a acestui domeniu conține deja NULL.
ddl-risk-drop-sequence = Elimină secvența și poziția la care ajunsese. Serverul refuză cât timp valoarea implicită a unei coloane încă extrage din ea, așa că acele valori implicite trebuie eliminate mai întâi.
ddl-risk-drop-synonym = Elimină sinonimul. Nimic nu refuză asta: o vizualizare, o rutină sau o interogare care îl numește eșuează de atunci înainte, la următoarea rulare.
ddl-risk-drop-extension = Elimină extensia și funcțiile, tipurile și operatorii pe care i-a instalat. Serverul refuză cât timp ceva de-al tău încă depinde de ele — o coloană de tipul ei, un index sau un server pe wrapperul ei — și numește ce anume.
ddl-risk-drop-used-object = Elimină { $what }. Serverul refuză cât timp o coloană încă îl folosește, așa că acele coloane trebuie să-și schimbe mai întâi tipul.
ddl-risk-restart-sequence = Repornește contorul la { $value }. Valorile deja distribuite nu se schimbă, așa că o cheie la care ajunge din nou intră în conflict.
ddl-risk-replace-synonym = Elimină sinonimul și îl creează din nou. Nimic nu refuză eliminarea: cele două instrucțiuni rulează într-o singură tranzacție.
ddl-risk-replace-sequence = Elimină secvența și o creează din nou: poziția la care ajunsese se pierde și reîncepe de la { $start }. Serverul refuză eliminarea cât timp valoarea implicită a unei coloane încă extrage din ea.
ddl-risk-start-value = valoarea ei de start
ddl-risk-replace-object = Elimină { $what } și îl creează din nou. Serverul refuză eliminarea cât timp o coloană încă îl folosește, așa că acele coloane trebuie să-și schimbe mai întâi tipul.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Refuzat cât timp are permisiuni pe el, un proprietar propriu, proprietăți extinse sau o regulă ori o valoare implicită legată: recrearea nu le-ar restaura.
       *[no] Refuzat cât timp are permisiuni pe el, un proprietar propriu sau proprietăți extinse: recrearea nu le-ar restaura.
    }
ddl-risk-drop-database = Elimină baza de date { $name } și tot ce conține — fiecare tabel cu toate rândurile lui, fiecare vizualizare, index, rutină și trigger. Nu se păstrează nimic și nu există anulare: singura cale înapoi este o copie de rezervă făcută înainte de rulare.
ddl-risk-drop-schema = Elimină schema { $name }. Serverul refuză cât timp încă conține tabele, vizualizări sau rutine, așa că fie elimină o schemă goală, fie eșuează{ $cascade ->
        [yes] {" "}— Schemaic nu trimite niciodată CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Elimină loginul { $who } și, odată cu el, fiecare permisiune de server și apartenență la roluri de server pe care o are — neînregistrate în altă parte, așa că un login creat din nou cu acest nume pornește fără ele. Serverul refuză cât timp ceva este conectat cu el, așa că sesiunile lui trebuie încheiate mai întâi. Utilizatorii lui din fiecare bază de date nu sunt eliminați: rămân, cu permisiunile lor, mapați la niciun login, iar un login nou cu acest nume nu se reatașează la ei fără ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Elimină { $who } și fiecare privilegiu pe care îl deține. Permisiunile acordate nu sunt înregistrate în altă parte, așa că refacerea contului înseamnă acordarea lor din nou, din memorie. { $cuts_off ->
        [yes] Orice este conectat cu el eșuează la următoarea instrucțiune în această bază de date.
       *[no] Orice este încă conectat cu el continuă să ruleze până se deconectează.
    }
ddl-risk-set-password = Schimbă parola lui { $who }. Cea veche nu poate fi recuperată, așa că orice este încă configurat cu ea — o aplicație, un job, clientul altui dezvoltator — nu se mai poate conecta până nu este actualizat manual. Sesiunile deja deschise continuă să ruleze.
ddl-risk-revoke-privileges = Îi retrage lui { $who } { $privileges }. Orice se bazează pe asta — o aplicație, un job, o vizualizare deținută de altcineva — începe să eșueze la următoarea instrucțiune.{ $cascades ->
        [yes] {" "}Revocarea include CASCADE, așa că este retrasă și de la fiecare cont căruia { $who } i-a acordat-o, iar ce rulează sub acele conturi eșuează la fel.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Acolo unde lui { $who } i se refuză în schimb (DENY), revocarea ridică refuzul — ceea ce redă accesul dacă un rol din care face parte îl acordă.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}Pe un tabel șterge și fiecare acordare și DENY la nivel de coloană pentru { $cleared } pe care { $who } le are pe coloanele tabelului: o coloană refuzată lui { $who } nu mai este refuzată, iar una acordată separat nu mai este acordată.
       *[no] {""}
    }
ddl-risk-revoke-role = Îi retrage lui { $member } rolul { $role } și, odată cu el, fiecare privilegiu pe care îl conferă rolul.
ddl-risk-blank-password = { $who } nu va avea parolă: oricine poate accesa serverul se poate autentifica cu el. Setează una aici sau ulterior cu ALTER USER.
ddl-risk-host-is-everywhere = Gazda lui este % — orice calculator din rețea, nu doar acesta.
ddl-risk-grant-server-wide = Acordă { $privileges } lui { $who } pe întregul server — instanța însăși, nu baza de date răsfoită.
ddl-risk-grant-every-database = Acordă { $privileges } lui { $who } pe fiecare bază de date de pe acest server — inclusiv baze de date care încă nu există, nu doar cea răsfoită.
ddl-risk-grant-keeps-column-denies = O acordare la nivel de tabel a { $privileges } șterge orice DENY al acestuia pe care { $who } îl are pe una dintre coloanele tabelului, așa că instrucțiunea refuză din nou acele coloane lui { $who } imediat după acordare: o coloană refuzată lui { $who } rămâne refuzată. Acolo unde { $who } deține și opțiunea de acordare pe tabel, serverul refuză să o refuze din nou fără CASCADE și nu se aplică nimic.
ddl-risk-star-refresh = Reîmprospătează fiecare vizualizare și funcție inline din această bază de date care face select * din { $table }, direct sau printr-un sinonim: SQL Server le leagă de coloanele tabelului după poziție și altfel ar citi și scrie coloanele greșite. Planul eșuează dacă una dintre ele nu se mai compilează cu noile coloane. Una dintr-o altă bază de date care face select * din { $table } este reîmprospătată după commit-ul planului, în afara tranzacției sale: până atunci citește pozițiile vechi, iar dacă reîmprospătarea ei eșuează, planul rămâne aplicat și vizualizarea este numită aici.
ddl-risk-piecewise = SQL Server nu creează, nu modifică și nu elimină un modul compilat nativ într-o tranzacție, așa că acest plan rulează câte o instrucțiune pe rând și nu poate fi oprit: dacă o instrucțiune eșuează, cele dinaintea ei rămân aplicate.
ddl-risk-drop-sequence-table = Elimină secvența: valoarea la care ajunsese se pierde, iar orice extrage încă din ea eșuează.
ddl-risk-drop-scale = Conform statisticilor din arborele schemei, numărul de rânduri este { $rows }.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = propunerea este pentru tabelul `{ $proposed }`, dar este deschis `{ $actual }`
propose-no-ops = propunerea nu cere nicio modificare
propose-no-such-column = `{ $table }` nu are o coloană numită `{ $name }`
propose-column-exists = `{ $table }` are deja o coloană numită `{ $name }`
propose-no-such-index = `{ $table }` nu are un index numit `{ $name }`
propose-index-exists = `{ $table }` are deja un index numit `{ $name }`
propose-no-such-foreign-key = `{ $table }` nu are o cheie străină numită `{ $name }`
propose-no-such-check = `{ $table }` nu are o constrângere de verificare numită `{ $name }`
propose-key-width-mismatch = o cheie străină are nevoie de tot atâtea coloane referite câte coloane care fac referire are
propose-empty-index = un index are nevoie de cel puțin o coloană
propose-invalid = tabelul propus nu ar fi valid: { $problem }
propose-not-a-table = `{ $name }` este o vizualizare — o propunere poate modifica doar un tabel
propose-no-such-table = niciun tabel numit `{ $name }` — apelează list_schema pentru a vedea ce există
propose-not-an-expression = { $field ->
        [default] valoarea implicită `{ $text }` nu este o singură expresie SQL
        [generated] expresia generată `{ $text }` nu este o singură expresie SQL
       *[check] expresia de verificare `{ $text }` nu este o singură expresie SQL
    }
propose-not-a-type = `{ $text }` nu este un tip de coloană — indică doar tipul, fără valoare implicită, fără NOT NULL și fără nimic după el
propose-unknown-action = `{ $text }` nu este o acțiune de cheie străină
propose-not-loaded = { $table } nu este încărcat în { $database } acum — deschide baza de date în arborele schemei sau așteaptă terminarea unei reîmprospătări și încearcă din nou.
propose-nothing-to-change = { $subject } arată deja așa — nu există nimic de modificat.
propose-no-database = Această filă nu are nicio bază de date selectată, așa că o modificare de schemă aplicată de aici nu ar avea unde să ajungă. Alege mai întâi una pentru această conexiune — dacă toate bazele de date sunt ascunse, ochiul din SCHEMĂ este locul de unde poți readuce una.
propose-other-connection = Această modificare se referă la o filă de pe altă conexiune — comută la ea sau selectează conexiunea ei în arbore înainte de a aplica o modificare de schemă.
ai-run-no-database = Această filă nu are nicio bază de date selectată, așa că o instrucțiune rulată de aici nu ar avea unde să ajungă. Alege mai întâi una pentru această conexiune — dacă toate bazele de date sunt ascunse, ochiul din SCHEMĂ este locul de unde poți readuce una.
ai-run-other-connection = Această conversație se referă la o filă de pe altă conexiune. O filă nouă s-ar deschide pe conexiunea selectată în arbore, așa că instrucțiunea ar rula în altă parte — comută mai întâi la acea filă sau selectează conexiunea ei în arbore.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, pe care acest plan o redenumește în { $to }
ddl-moved-dropped = { $name }, pe care acest plan o elimină
ddl-withheld-unreplayable-rename = { $count ->
        [one] Redenumirea { $was } în { $to } nu poate prelua triggerul ei: SQL-ul lui numește { $was }, iar rescrierea lui aici ar schimba ce ai scris. Redenumește vizualizarea separat după ce recreezi triggerul cu noul nume.
        [few] Redenumirea { $was } în { $to } nu poate prelua cele { $count } triggere ale ei: SQL-ul lor numește { $was }, iar rescrierea lui aici ar schimba ce ai scris. Redenumește vizualizarea separat după ce recreezi triggerele cu noul nume.
       *[other] Redenumirea { $was } în { $to } nu poate prelua cele { $count } de triggere ale ei: SQL-ul lor numește { $was }, iar rescrierea lui aici ar schimba ce ai scris. Redenumește vizualizarea separat după ce recreezi triggerele cu noul nume.
    }
ddl-withheld-numbered-group = { $name } conduce un grup de proceduri numerotate ({ $members }), iar acest plan o elimină mai întâi, ceea ce elimină odată cu ea fiecare membru al grupului. Editeaz-o sub propriul nume și tip, ceea ce o modifică pe loc și îi păstrează.
ddl-withheld-view-index-create = { $count ->
        [one] Crearea vizualizării { $view } nu poate crea indexul ei { $indexes } — de un tip pe care Schemaic nu îl poate reformula. Creează această vizualizare în SQL.
       *[other] Crearea vizualizării { $view } nu poate crea indecșii ei { $indexes } — de un tip pe care Schemaic nu îl poate reformula. Creează această vizualizare în SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Editarea vizualizării { $view } îi elimină indexul { $indexes }, pe care Schemaic nu îl poate crea din nou — de un tip pe care Schemaic nu îl poate reformula. Editează această vizualizare în SQL.
       *[other] Editarea vizualizării { $view } îi elimină indecșii { $indexes }, pe care Schemaic nu îi poate crea din nou — de un tip pe care Schemaic nu îl poate reformula. Editează această vizualizare în SQL.
    }
ddl-withheld-tsql-lossy-index = Reconstruirea { $table } ar elimina indexul { $index }: este de un tip pe care Schemaic nu îl citește complet
ddl-withheld-tsql-encrypted-trigger = Reconstruirea { $table } ar elimina triggerul { $trigger }: textul lui este criptat
ddl-withheld-tsql-signed-trigger = Reconstruirea { $table } ar elimina semnătura de pe triggerul { $trigger }, care nu poate fi pusă la loc fără cheia privată a certificatului
ddl-withheld-tsql-renamed-trigger = Reconstruirea { $table } sub un nume nou ar lăsa triggerul { $trigger } cu numele vechi
ddl-withheld-partly-read = Indexul { $index } a fost citit doar parțial — SQLite păstrează predicatul unui index parțial și o cheie de tip expresie doar în textul CREATE propriu al indexului{ $why ->
        [may-name] , iar acel text poate numi { $moved }. Elimină indexul pentru a continua fără el.
        [not-found] , așa că nu poate fi creat din ce s-a citit. Elimină indexul pentru a continua fără el.
        [edited] , așa că această editare a lui nu poate fi scrisă — ce s-a citit nu este întregul index. Anulează modificarea pentru a păstra indexul așa cum este sau elimină-l.
       *[no-text] {" "}— iar pentru acesta nu există un asemenea text, așa că recrearea lui ar schimba ce indexează. Elimină indexul pentru a continua fără el.
    }
ddl-withheld-condition-names-dropped = Condiția indexului { $index } numește { $column }, pe care acest plan o elimină. Editează condiția sau elimină mai întâi indexul.
ddl-withheld-strands-triggers = Acest tabel are { $count ->
        [one] { $count } trigger
        [few] { $count } triggere
       *[other] { $count } de triggere
    } pe el, iar SQL-ul lor este rejucat exact cum l-a stocat SQLite — care ar numi în continuare { $moved }. SQLite acceptă un asemenea trigger și apoi refuză orice scriere în tabel. Elimină sau editează mai întâi triggerul ori redenumește coloana fără nicio altă modificare, ca ALTER TABLE propriu al SQLite să-l poată rescrie.
ddl-withheld-strands-other-trigger = { $owner } numește { $moved }. O reconstruire redenumește acest tabel pe sub acel trigger și îi lasă SQL-ul exact cum era, așa că triggerul ar numi în continuare o coloană care nu mai există — SQLite acceptă un asemenea trigger și apoi refuză orice scriere în *acel* tabel. Elimină sau editează mai întâi triggerul ori redenumește coloana fără nicio altă modificare, ca ALTER TABLE propriu al SQLite să-l poată rescrie.
ddl-withheld-trigger-on = Triggerul { $name } pe { $table }
ddl-withheld-some-trigger = Un trigger pe alt tabel
ddl-withheld-strands-generated-column = Coloana generată { $column } se calculează din { $moved }. O reconstruire scrie tabelul din model și nimic nu rescrie acea expresie — SQLite refuză noul tabel cu „no such column” și întregul plan este anulat prin rollback. Editează sau elimină mai întâi { $column }.
ddl-withheld-cannot-restate = Declarația acestui tabel conține { $clauses }, pentru care modelul Schemaic nu are câmp. O reconstruire scrie tabelul înapoi din acel model, așa că clauza ar dispărea dintr-un tabel care ar părea în continuare corect — iar fiecare scriere care depinde de ea s-ar comporta altfel de atunci înainte. Modifică acest tabel cu un script (Copiază DDL este începutul unuia) sau elimină mai întâi clauza.
ddl-sqlite-clauses = { $first } și { $last }
ddl-sqlite-clause-deferrable = clauza DEFERRABLE a unei chei străine
ddl-sqlite-clause-on-conflict = o clauză ON CONFLICT
ddl-sqlite-clause-desc-key = o coloană de cheie primară DESC
ddl-sqlite-clause-collate-key = un COLLATE pe o coloană de cheie primară
ddl-withheld-virtual-table = `{ $name }` este un tabel virtual. Coloanele lui sunt interfața unui modul, nu o declarație de stocare, iar datele reale se află în tabele umbră pe care o reconstruire le-ar elimina — un index FTS5 ar dispărea dintr-un tabel care încă ar conține textul. Modifică-l cu un script (DROP și CREATE VIRTUAL TABLE) sau editează tabelele pe care le indexează.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } vizualizare
    [few] { $count } vizualizări
   *[other] { $count } de vizualizări
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } rutină
    [few] { $count } rutine
   *[other] { $count } de rutine
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } trigger
    [few] { $count } triggere
   *[other] { $count } de triggere
}
ddl-used-by-count-event = { $count ->
    [one] { $count } eveniment
    [few] { $count } evenimente
   *[other] { $count } de evenimente
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } cheie străină
    [few] { $count } chei străine
   *[other] { $count } de chei străine
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } expresie de tabel
    [few] { $count } expresii de tabel
   *[other] { $count } de expresii de tabel
}
ddl-used-by-counts = { $first } și { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}și încă { $more }
    }
ddl-used-by-drop = Folosit de { $listing }.
ddl-used-by-change = { $subject } este folosit de { $listing }. { $then ->
        [renamed] Acestea numesc în continuare { $name } după redenumire.
        [dropped] Acestea se strică atunci când este eliminat.
       *[retyped] Verifică-le în raport cu noul tip.
    }
ddl-used-by-column = Coloana { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Cheile străine dintre aceste tabele formează un ciclu, așa că nicio ordine de creare nu le satisface pe toate. O instrucțiune va fi refuzată pentru că face referire la un tabel care încă nu există, iar nici MySQL, nici MariaDB nu anulează DDL prin rollback.
ddl-risk-clash = { $holder } încă deține { $names }, de care are nevoie crearea { $claimer } — { $why ->
        [not-dropped] iar acest plan nu îl elimină
        [default] și nu poate fi eliminat mai întâi deoarece valoarea implicită a unei coloane extrage din el
       *[referenced] și nu poate fi eliminat mai întâi deoarece alt tabel face referire la el
    }. { $cost ->
        [no-rollback] O instrucțiune va fi refuzată, iar nici MySQL, nici MariaDB nu anulează DDL prin rollback.
        [rolls-back] O instrucțiune va fi refuzată, ceea ce anulează întreaga migrare prin rollback.
       *[refused] O instrucțiune va fi refuzată.
    }

preview-title = Aplică modificările pe { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } modificare
    [few] { $count } modificări
   *[other] { $count } de modificări
}
preview-used-by = Folosit de
preview-withheld = Acest motor nu poate exprima o parte din acest plan
preview-withheld-hint = Nu se aplică nimic cât timp acesta este listat.
preview-omitted = Neincluse în această migrare
preview-omitted-hint = Aplicarea lasă aceste obiecte așa cum sunt.
preview-copy = Copiază
preview-open-in-editor = Deschide în editor
preview-back = Înapoi
preview-apply = Aplică
preview-applying = Se aplică…
preview-applied = { $count ->
    [one] S-a aplicat { $count } instrucțiune pe { $subject }.
    [few] S-au aplicat { $count } instrucțiuni pe { $subject }.
   *[other] S-au aplicat { $count } de instrucțiuni pe { $subject }.
}
preview-refreshed = Schema a fost reîmprospătată.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Importă conexiuni
conn-import-url = URL de conexiune
conn-import-add = Adaugă
conn-import-url-hint = Un URL de conexiune, un URL JDBC sau o linie DATABASE_URL dintr-un .env — câte unul pe rând.
conn-import-choose-file = Alege un fișier…
conn-import-scan = Scanează clienții instalați
conn-import-scanning = Se scanează…
conn-import-docker = Containere Docker
conn-import-asking-docker = Se interoghează Docker…
conn-import-looking = Se caută conexiuni pe acest calculator…
conn-import-none-found = Nu s-au găsit conexiuni. DBeaver, IDE-urile JetBrains și clienții de linie de comandă își țin fișierele în locuri cunoscute; dacă al tău este în altă parte, alege-l mai sus.
conn-import-nothing-yet = Nimic găsit încă — lipește un URL, citește un fișier sau scanează clienții pe care îi folosești deja.
conn-import-found = { $count ->
    [one] { $count } conexiune găsită
    [few] { $count } conexiuni găsite
   *[other] { $count } de conexiuni găsite
}
conn-import-select-all = Selectează tot
conn-import-select-none = Nimic
conn-import-note-no-password = Nicio parolă în sursă
conn-import-note-already-saved = Deja salvată
conn-import-note-unexpanded-path = Calea conține un macro neexpandat
conn-import-note-pgpass = Parolă preluată din propriul tău .pgpass
conn-import-note-port-assumed = Sursa nu a numit niciun port; acesta este cel implicit
conn-import-note-external-login = Se autentifică într-un mod pe care Schemaic nu îl suportă: setează un login SQL
conn-import-skipped = { $count ->
    [one] { $count } intrare nu a fost importată: { $names }.
    [few] { $count } intrări nu au fost importate: { $names }.
   *[other] { $count } de intrări nu au fost importate: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } intrare nu a fost importată: { $names } și încă { $rest }.
    [few] { $count } intrări nu au fost importate: { $names } și încă { $rest }.
   *[other] { $count } de intrări nu au fost importate: { $names } și încă { $rest }.
}
conn-import-reason-unsupported-engine = motor neacceptat ({ $driver })
conn-import-reason-no-server = niciun server la care să se conecteze
conn-import-reason-not-published = portul lui nu este publicat pe acest calculator
conn-import-reason-unreadable = nu a putut fi citită ({ $error })
conn-import-reason-not-an-export = nu a putut fi citită (nu este un export de conexiuni Schemaic: { $error })
conn-import-reason-newer-export = nu a putut fi citită (exportată de un Schemaic mai nou (versiunea { $version }) — actualizează pentru a o importa)
conn-import-url-empty = Lipește un URL de conexiune.
conn-import-url-no-scheme = Nicio schemă — un URL începe cu mysql://, postgresql://, sqlserver:// sau sqlite:, un șir de conexiune SQL Server cu Server=.
conn-import-url-unknown-scheme = Schemă necunoscută „{ $scheme }” — se aștepta mysql, mariadb, postgresql, sqlserver sau sqlite.
conn-import-url-no-host = Nicio gazdă în URL.
conn-import-url-bad-port = „{ $port }” nu este un număr de port.
conn-import-url-no-path = Niciun fișier de bază de date după sqlite:.
conn-import-url-transport = „{ $server }” este accesat prin named pipes sau LocalDB — Schemaic se conectează doar prin TCP.
conn-import-url-userinfo = Numele serverului conține un @ — un nume de utilizator sau o parolă se pune în câmpul propriu, nu în server.
conn-import-url-not-sql-server = Acest șir de conexiune nu spune că este al SQL Server — driverele .NET pentru MySQL și PostgreSQL scriu aceleași cuvinte cheie. Lipește în schimb URL-ul serverului (mysql://, postgresql://, sqlserver://) sau, pentru SQL Server, scrie serverul ca tcp:host.
conn-import-row-overflow = Se afișează primele { $shown } din { $total }. Celelalte { $hidden } sunt importate oricum dacă sunt bifate — lista se oprește aici ca fereastra să rămână receptivă.
conn-import-import = Importă
conn-import-import-count = Importă { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = Imagine PNG
blob-kind-jpeg = Imagine JPEG
blob-kind-gif = Imagine GIF
blob-kind-bmp = Imagine BMP
blob-kind-webp = Imagine WebP
blob-kind-ico = Pictogramă
blob-kind-opaque = Date binare
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · se afișează primii { $shown }
blob-pane-preview = Previzualizare
blob-pane-hex = Hex
blob-reading = Se citește…
blob-empty = Această celulă este NULL sau rândul ei nu mai există.
blob-preview-too-large = Această imagine are { $width } × { $height } — prea mare pentru previzualizare aici. Octeții sunt intacți: citește-i ca Hex sau salvează-i într-un fișier.
blob-preview-truncated = Au fost citiți doar primii { $shown } din { $key ->
    [png] această imagine PNG
    [jpeg] această imagine JPEG
    [gif] această imagine GIF
    [bmp] această imagine BMP
    [webp] această imagine WebP
    [ico] această pictogramă
   *[other] această valoare
}, așa că nu poate fi desenată. Citește octeții ca Hex.
blob-preview-unreadable = Acești octeți încep ca { $key ->
    [png] o imagine PNG
    [jpeg] o imagine JPEG
    [gif] o imagine GIF
    [bmp] o imagine BMP
    [webp] o imagine WebP
    [ico] o pictogramă
   *[other] o imagine
}, dar nu pot fi citiți ca atare. Citește-i ca Hex sau salvează-i într-un fișier.
blob-save-truncated = Doar primii { $shown } din { $size } au putut fi citiți, așa că s-ar salva doar începutul valorii.
blob-load = Încarcă din fișier
blob-save = Salvează în fișier
blob-load-dialog = Încarcă valoarea binară din fișier
blob-save-dialog = Salvează valoarea binară
blob-saved-to = Salvat în { $path }
blob-loaded-file = Fișier încărcat. Fă commit în grilă pentru a-l scrie.
blob-not-writable = Această celulă nu poate fi scrisă.
blob-file-too-large = Acel fișier are { $got } — această coloană conține cel mult { $most }.
blob-not-accepting = Acea celulă nu mai acceptă o valoare — nu s-a pus nimic în așteptare.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = Diagramă ER
erd-scope = Domeniu
erd-chip-tables = { $count ->
    [one] { $count } tabel
    [few] { $count } tabele
   *[other] { $count } de tabele
}
erd-chip-relationships = { $count ->
    [one] { $count } relație
    [few] { $count } relații
   *[other] { $count } de relații
}
erd-chip-hidden = { $count ->
    [one] { $count } neconectat ascuns
    [few] { $count } neconectate ascunse
   *[other] { $count } de neconectate ascunse
}
erd-find-placeholder = Caută tabel sau coloană
erd-find-none = Nicio potrivire
erd-find-matches = { $hits ->
    [one] { $hits } potrivire
    [few] { $hits } potriviri
   *[other] { $hits } de potriviri
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } potrivire în { $tables } tabele
    [few] { $hits } potriviri în { $tables } tabele
   *[other] { $hits } de potriviri în { $tables } tabele
}
erd-more-columns = +încă { $count }
erd-show-less = arată mai puțin
erd-not-loaded = Schema nu este încă încărcată pentru această bază de date.
erd-no-relationships = Nicio relație de cheie străină de reprezentat.
erd-export-tip = Exportă diagrama…
erd-copy-as = Copiază ca
erd-export-dialog = Exportă diagrama
erd-nothing-to-export = Nimic de exportat.
erd-nothing-to-copy = Nimic de copiat.
erd-copied-as = Copiat ca { $format }
erd-clipboard-unavailable = Clipboardul nu este disponibil.
erd-copy-failed = Nu s-a putut copia: { $error }
erd-saved = S-a salvat { $name }
erd-unnamed-file = fișierul

## Server variables modal (server_vars_view.rs)

server-vars-title = Variabile server — { $connection }
server-vars-search = Caută setări
server-vars-variables = Variabile
server-vars-status = Stare
server-vars-loading = Se încarcă…
server-vars-unsupported = Motorul acestei conexiuni nu publică o listă de acest tip.
server-vars-no-match = Nicio setare potrivită.
server-vars-more = { $count ->
    [one] Încă { $count } setare se potrivește — restrânge căutarea
    [few] Încă { $count } setări se potrivesc — restrânge căutarea
   *[other] Încă { $count } de setări se potrivesc — restrânge căutarea
}
server-vars-count = { $count ->
    [one] { $count } setare
    [few] { $count } setări
   *[other] { $count } de setări
}
server-vars-count-filtered = Setări: { $shown } din { $count }
server-vars-refresh = Reîmprospătează

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Editează fragmentul — { $name }
snippet-edit-name = Nume
snippet-edit-name-placeholder = Numele fragmentului
snippet-edit-shortcut = Scurtătură de expandare
snippet-edit-shortcut-none = niciuna
snippet-edit-query = Interogare
snippet-edit-save = Salvează

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Plan de interogare
plan-analyze-unavailable = Analyze indisponibil (instrucțiunea scrie date)
plan-analyze-hint = Execută instrucțiunea pentru a măsura timpii reali
plan-explaining = Se explică
plan-potential-issues = Probleme potențiale
plan-detail-no-index = niciun index folosit
plan-detail-key-unused = index nefolosit pentru căutare
plan-my-full-scan = Scanare completă a tabelului `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Scanare completă a rezultatului ({ $detail }, type = ALL)
plan-my-filesort = Filesort pe `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort pe rezultat (Extra: Using filesort)
plan-my-temporary = Tabel temporar pentru `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Tabel temporar pentru rezultat (Extra: Using temporary)
plan-pg-seq-scan = Scanare secvențială pe `{ $table }` (niciun index folosit)
plan-pg-seq-scan-unnamed = Scanare secvențială pe un tabel (niciun index folosit)
plan-pg-sort = Nod Sort — interogarea sortează rândurile din mers
plan-ms-full-scan = Scanare completă a `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Scanare completă a unui tabel ({ $op })
plan-ms-sort = Sort — interogarea sortează rândurile din mers
plan-ms-spool = { $op } — un tabel de lucru intermediar în tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = niciun predicat de join — fiecare rând se împerechează cu fiecare rând
plan-server-no-statistics = o coloană pe care o citește nu are statistici
plan-server-spill = s-a revărsat în tempdb — memoria alocată a fost prea mică
plan-server-convert = o conversie de tip afectează planul ({ $issue }): { $expression }
plan-server-unmatched-indexes = un index filtrat nu a putut fi folosit
plan-server-memory-grant = memoria alocată a fost estimată greșit
plan-server-wait = a așteptat după { $wait }
plan-server-other = serverul avertizează: { $name }
plan-missing-index = Index lipsă pe `{ $table }` ({ $keys })
plan-missing-index-include = Index lipsă pe `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Index lipsă pe `{ $table }` ({ $keys }) — serverul a estimat o îmbunătățire de { $impact }%
plan-missing-index-include-impact = Index lipsă pe `{ $table }` ({ $keys }; include { $include }) — serverul a estimat o îmbunătățire de { $impact }%
plan-ask-ai = Întreabă AI
plan-ms-operation = Operație
plan-ms-object = Obiect
plan-ms-estimated-rows = Rânduri estimate
plan-ms-estimated-cost = Cost estimat
plan-ms-actual-rows = Rânduri reale
plan-ms-executions = Execuții
plan-ms-statement = Instrucțiunea { $n }: { $kind }
plan-ms-statement-in-procedure = Instrucțiunea { $n }: { $kind } în procedura { $name }
plan-ms-statement-in-function = Instrucțiunea { $n }: { $kind } în funcția { $name }
plan-ms-statement-in-a-procedure = Instrucțiunea { $n }: { $kind } într-o procedură
plan-ms-statement-in-a-function = Instrucțiunea { $n }: { $kind } într-o funcție
plan-ms-plan = Plan
plan-ms-no-plan = Nicio instrucțiune din acest lot nu are un plan.
plan-ms-unread = Planul nu a putut fi citit: { $reason }
plan-ms-unread-not-xml = nu este un document XML
plan-ms-unread-not-showplan = se aștepta ShowPlanXML, s-a găsit { $found }
plan-ms-unread-empty = un document gol

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Folosit de — { $object }
used-by-copy = Copiază
used-by-not-loaded = Schema bazei { $database } nu s-a încărcat încă — extinde-o în arbore, apoi întreabă din nou.
used-by-nothing-in = Nimic din { $database } nu îl folosește.
used-by-nothing = Nimic nu îl folosește.
used-by-limits = Găsite prin citirea definițiilor din această bază de date: SQL-ul construit la rulare și alte baze de date nu sunt văzute.
used-by-dropped-with-it = { $detail } · eliminat odată cu el
used-by-heading-view = { $count ->
    [one] Vizualizare ({ $count })
   *[other] Vizualizări ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Rutină ({ $count })
   *[other] Rutine ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Triggere ({ $count })
}
used-by-heading-event = { $count ->
    [one] Eveniment ({ $count })
   *[other] Evenimente ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Cheie străină ({ $count })
   *[other] Chei străine ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Expresie de tabel ({ $count })
   *[other] Expresii de tabel ({ $count })
}
used-by-noun-view = vizualizare
used-by-noun-routine = rutină
used-by-noun-trigger = trigger
used-by-noun-event = eveniment
used-by-noun-foreign-key = cheie străină
used-by-noun-expression = expresie de tabel

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Întunecată
theme-light = Luminoasă

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = COD
md-code-copy = Copiază
md-code-insert = Inserează
md-code-run = Rulează
md-proposal-title = Modificare propusă pentru { $table }
md-proposal-count = { $count ->
    [one] { $count } modificare
    [few] { $count } modificări
   *[other] { $count } de modificări
}
md-proposal-review = Examinează modificarea…
md-proposal-unreadable = Asistentul a propus o modificare care nu a putut fi citită — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Alege…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Acum
cell-calendar-today = Azi
cell-month-1 = ianuarie
cell-month-2 = februarie
cell-month-3 = martie
cell-month-4 = aprilie
cell-month-5 = mai
cell-month-6 = iunie
cell-month-7 = iulie
cell-month-8 = august
cell-month-9 = septembrie
cell-month-10 = octombrie
cell-month-11 = noiembrie
cell-month-12 = decembrie
cell-weekday-mon = Lu
cell-weekday-tue = Ma
cell-weekday-wed = Mi
cell-weekday-thu = Jo
cell-weekday-fri = Vi
cell-weekday-sat = Sâ
cell-weekday-sun = Du

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Redenumește aliasul
alias-rename-title-cte = Redenumește CTE-ul
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = Numele CTE
alias-rename-new-name = Nume nou
alias-rename-rename = Redenumește

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Nu mai există nimic aici de redenumit.
alias-rename-empty = Dă-i un nume.
alias-rename-conflict = „{ $name }” este deja un nume în această interogare: redenumirea în el ar schimba tabelul pe care îl citește o referință.
alias-rename-unsure = O referință de aici poate citi o altă coloană cu acest nume sau o poate citi pe aceasta acolo unde redenumirea nu poate ajunge, iar schema încărcată nu poate spune — așa că nu se face nicio editare. Încarcă schema tabelelor sau redenumește manual.
alias-rename-changed = Interogarea s-a schimbat cât timp acesta era deschis. Închide-l și redenumește din nou.
alias-rename-read-only = Editorul este doar pentru citire până când sugestia AI este acceptată sau respinsă.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } utilizare a { $name }
    [few] { $count } utilizări ale { $name }
   *[other] { $count } de utilizări ale { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = SVG nevalid: { $error }
raster-too-large = Imaginea este prea mare pentru rasterizare ({ $width }×{ $height })
raster-png-failed = Codificarea PNG a eșuat: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Copiază
grid-menu-copy-value = Copiază valoarea
grid-menu-copy-formatted = Copiază formatat
grid-menu-copy-as = Copiază ca
grid-menu-paste = Lipește
grid-menu-fill-down = Umple în jos
grid-menu-fill-up = Umple în sus
grid-menu-edit-field = Editează câmpul
grid-menu-edit-row = Editează rândul
grid-menu-view-row = Vezi rândul
grid-menu-edit-binary = Editează binar
grid-menu-view-binary = Vezi binar
grid-menu-filter-by-value = Filtrează după această valoare
grid-menu-exclude-value = Exclude această valoare
grid-menu-follow-relation = Urmează relația
grid-menu-referenced-by = Referit de
grid-menu-set-null = Setează la NULL
grid-menu-set-default = Setează la DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Duplică rândul
    [few] Duplică { $count } rânduri
   *[other] Duplică { $count } de rânduri
}
grid-menu-delete-rows = { $count ->
    [one] Șterge rândul
    [few] Șterge { $count } rânduri
   *[other] Șterge { $count } de rânduri
}
grid-menu-undo-delete = Anulează ștergerea
grid-menu-ai-summary = Rezumat AI
grid-attach-to-chat = Atașează { $scope } la conversație
grid-attach-scope-rows = { $rows ->
    [one] { $rows } rând
    [few] { $rows } rânduri
   *[other] { $rows } de rânduri
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } coloană
    [few] { $cols } coloane
   *[other] { $cols } de coloane
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } rând
    [few] { $rows } rânduri
   *[other] { $rows } de rânduri
} și { $cols ->
    [one] { $cols } coloană
    [few] { $cols } coloane
   *[other] { $cols } de coloane
}
grid-menu-freeze = Îngheață
grid-menu-unfreeze = Dezgheață
grid-menu-hide-column = Ascunde coloana
grid-menu-show-hidden-columns = Arată coloanele ascunse ({ $count })
grid-menu-show-all = Arată tot
grid-menu-format-as = Formatează ca
grid-format-none = Niciunul
grid-format-timestamp = Marcaj de timp
grid-format-grouped = Gruparea cifrelor
grid-format-bytes = Dimensiune fișier
grid-format-bool = Boolean
grid-menu-copy-name = Copiază numele
grid-menu-copy-qualified-name = Copiază numele calificat
grid-menu-copy-values = Copiază valorile
grid-menu-sql-in-list = Listă SQL IN

## The results grid's toolbar (grid.rs) — the status line, the commit and row
## controls, and the copy, save, AI and chart icons.

grid-stats = { $rows } · { $cols ->
    [one] { $cols } col.
   *[other] { $cols } col.
} · { $ms } ms
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } col.
   *[other] { $cols } col.
} · { $ms } ms
grid-rows-read = { $n ->
    [one] { $read } rând
    [few] { $read } rânduri
   *[other] { $read } de rânduri
}
grid-rows-read-capped = { $n ->
    [one] { $read } rând
    [few] { $read } rânduri
   *[other] { $read } de rânduri
} (limitat)
grid-rows-read-of-total = { $n ->
    [one] { $read } din { $total } rând
    [few] { $read } din { $total } rânduri
   *[other] { $read } din { $total } de rânduri
}
grid-kept-note = · păstrat — doar citire
grid-arena-note = · { $columns } prea mari pentru a fi păstrate complet — rândurile ulterioare apar goale
grid-sorted-subset = · subset sortat (limitat) — nu ordinea completă
grid-read-all = citește toate rândurile
grid-read-more = citește încă { $rows } rânduri
grid-reading = se citește…
grid-committing = Se face commit…
grid-commit-tip = { $count ->
    [one] Fă commit la { $count } modificare ({ $keys })
    [few] Fă commit la { $count } modificări ({ $keys })
   *[other] Fă commit la { $count } de modificări ({ $keys })
}
grid-discard-tip = Renunță la toate modificările în așteptare
grid-copy-sql-tip = Copiază modificările ca SQL
grid-add-row-tip = Adaugă un rând
grid-delete-row-tip = Marchează rândul selectat pentru ștergere ({ $key })
grid-clone-row-tip = Clonează rândul selectat
grid-copy-results-tip = Copiază rezultatele…
grid-copy-results-empty = Nimic de copiat — niciun rând
grid-save-results-tip = Salvează rezultatele într-un fișier…
grid-save-results-empty = Nimic de salvat — niciun rând
grid-ai-tip = Generează rânduri cu AI…
grid-ai-generating = Se generează…
grid-ai-off = Acțiunile AI trimit date — dezactivate pentru această conexiune
grid-ai-fill-value = Completare AI a valorii
grid-ai-insert-row = Inserare AI a unui rând
grid-ai-seed-table = Populare AI a tabelului
grid-chart-tip = Reprezintă grafic rezultatele
grid-chart-empty = Nimic de reprezentat — niciun rând
grid-chart-this-result = acest rezultat

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Vezi
grid-rollback-tab = Rollback pentru { $tab }
grid-find-placeholder = Caută în rezultate
grid-figure-copied = Copiat
grid-figure-rows = { $n ->
    [one] { $n }{ $more } rând
    [few] { $n }{ $more } rânduri
   *[other] { $n }{ $more } de rânduri
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } distincte
grid-figure-sum = Suma { $value }
grid-figure-avg = Media { $value }
grid-figure-median = Mediana { $value }
grid-figure-min = Min { $value }
grid-figure-max = Max { $value }
grid-figure-length = { $n ->
    [one] Lungime { $n } caracter
    [few] Lungime { $n } caractere
   *[other] Lungime { $n } de caractere
}
grid-figure-length-range = Lungime { $lo }–{ $hi } caractere
grid-goto-label = Salt la rândul:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = clipboard
export-dest-file = fișier
export-note-rows = { $n ->
    [one] S-a exportat { $rows } rând în { $name }
    [few] S-au exportat { $rows } rânduri în { $name }
   *[other] S-au exportat { $rows } de rânduri în { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } conține octeți pe care acest format nu îi poate transporta: acele celule sunt goale
   *[other] { $cols } conțin octeți pe care acest format nu îi poate transporta: acele celule sunt goale
}
export-note-blanked = { $cols } prea mari pentru a fi păstrate complet: rândurile ulterioare sunt goale
export-note-cut = { $cols } tăiate la limita de { $limit } caractere a unei celule Excel
export-failure-partial = { $message } — { $name } nu a fost modificat; rândurile scrise sunt în { $part }
export-cancelled-partial = Export anulat — { $name } nu a fost modificat; rândurile scrise sunt în { $part }
export-cancelled-nothing = Export anulat — { $name } nu a fost modificat.
copy-sql-as-null = { $n ->
    [one] Copiat în clipboard — { $cols } conține octeți pe care textul SQL nu îi poate transporta: scriși ca NULL
   *[other] Copiat în clipboard — { $cols } conțin octeți pe care textul SQL nu îi poate transporta: scriși ca NULL
}
copy-sql-nothing = { $n ->
    [one] Nimic copiat — { $cols } conține octeți pe care textul SQL nu îi poate transporta
   *[other] Nimic copiat — { $cols } conțin octeți pe care textul SQL nu îi poate transporta
}
copy-sql-left-out = { $n ->
    [one] Copiat în clipboard — { $cols } conține octeți pe care textul SQL nu îi poate transporta: acele valori sunt omise
   *[other] Copiat în clipboard — { $cols } conțin octeți pe care textul SQL nu îi poate transporta: acele valori sunt omise
}

grid-fill-nothing = Nimic de completat: coloanele selectate nu pot fi scrise sau celula de copiat este valoarea nesetată a unui rând nou ori un DEFAULT în așteptare.
grid-paste-outside = { $n } în afara grilei
grid-paste-read-only = { $n } în coloane doar pentru citire
grid-paste-capped = { $n } peste limita de lipire de { $cap } celule
grid-paste-deleted = { $n } în rânduri marcate pentru ștergere
grid-paste-nothing = Nimic lipit: { $notes }.
grid-paste-partial = { $count ->
        [one] S-a lipit { $cells } celulă, omițând { $notes }.
        [few] S-au lipit { $cells } celule, omițând { $notes }.
       *[other] S-au lipit { $cells } de celule, omițând { $notes }.
    }
grid-row-not-editable = Coloana `{ $column }` nu poate fi editată.
grid-row-not-nullable = Coloana `{ $column }` nu poate fi NULL.
grid-fill-skipped = { $count ->
    [one] S-a completat { $filled } celulă, omițând { $skipped } din rânduri marcate pentru ștergere.
    [few] S-au completat { $filled } celule, omițând { $skipped } din rânduri marcate pentru ștergere.
   *[other] S-au completat { $filled } de celule, omițând { $skipped } din rânduri marcate pentru ștergere.
}
grid-attach-summary = { $rows ->
    [one] { $rows } rând
    [few] { $rows } rânduri
   *[other] { $rows } de rânduri
} × { $cols ->
    [one] { $cols } coloană
    [few] { $cols } coloane
   *[other] { $cols } de coloane
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } rând
    [few] { $rows } rânduri
   *[other] { $rows } de rânduri
} × { $cols ->
    [one] { $cols } coloană
    [few] { $cols } coloane
   *[other] { $cols } de coloane
} din { $table }
grid-attach-summary-capped = { $summary } (primele { $taken } din { $total } selectate)
grid-copy-sql-unset = Nimic de copiat ca SQL — celulele selectate ale rândului nou nu au încă valoare și primesc valoarea implicită a serverului.
grid-copy-sql-joined = Nimic de copiat ca SQL: aceste rânduri provin dintr-o interogare pe mai multe tabele.
grid-copy-sql-no-keyed-table = Nimic de copiat ca SQL: rândurile trebuie să provină dintr-un singur tabel cu cheie.
grid-copy-sql-key-too-large = Nimic de copiat ca SQL: cheia a fost prea mare pentru ca acest rezultat să o păstreze complet, așa că unele rânduri nu pot fi identificate prin ea.
grid-copy-sql-pinned = Nimic de copiat ca SQL: acest rezultat este fixat, așa că rândurile lui sunt un instantaneu.
grid-copy-sql-read-only = Nimic de copiat ca SQL: această conexiune este doar pentru citire.
grid-copy-update-too-large = Nimic de copiat ca UPDATE: coloanele selectate au fost prea mari pentru ca acest rezultat să le păstreze complet, iar unele dintre celulele lor apar goale.
grid-copy-update-nothing-writable = Nimic de copiat ca UPDATE: niciuna dintre coloanele selectate nu poate fi scrisă.
grid-copy-where-in-no-key = Nimic de copiat ca WHERE IN: acest tabel nu are o cheie primară sau unică după care să-și identifice rândurile.
grid-copy-new-row-no-key = Nimic de copiat: un rând nou nu are cheie până nu este salvat.
grid-export-fetched-rows = Rânduri preluate ({ $count })
grid-export-dialog-title = Exportă rezultatele
grid-export-already-running = Un export rulează deja — nu s-a scris nimic.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Rândul { $row }
grid-row-title-in-table = Rândul { $row }  ·  { $table }
grid-row-set-value = Setează valoarea
grid-row-set-null = Setează NULL
grid-row-edit-binary = Editează
grid-row-view-binary = Vezi
grid-row-invalid-json = JSON nevalid: { $error }
grid-row-saving = Se salvează

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } rând afectat · { $ms } ms
    [few] { $count } rânduri afectate · { $ms } ms
   *[other] { $count } de rânduri afectate · { $ms } ms
}
grid-rerun-would-write = Această instrucțiune nu poate fi rulată din nou — ar scrie în baza de date
grid-filter-not-simple = Această interogare nu poate fi filtrată — nu este un SELECT simplu pe un singur tabel
grid-filter-bracket-in-name = un nume care conține `]` nu poate fi filtrat aici
grid-filter-trailing-input = text neașteptat după condiție
grid-staged-requery = Fă mai întâi commit sau renunță la modificări — recitirea rândurilor le-ar arunca
grid-staged-landing = Au fost puse în așteptare modificări în timp ce tabelul era recitit, așa că au fost păstrate și tabelul nu a fost reîmprospătat — fă commit sau renunță la ele, apoi reîmprospătează.
grid-reread-without-filter = Filtrul apelează o funcție pe care Schemaic nu o rulează din nou fără să fie întrebat, așa că după commit tabelul a fost recitit fără filtru și sortare.
grid-hidden-columns-tip = { $count ->
    [one] { $count } coloană ascunsă — clic pentru a le arăta pe toate sau clic dreapta pe un antet pentru a alege
    [few] { $count } coloane ascunse — clic pentru a le arăta pe toate sau clic dreapta pe un antet pentru a alege
   *[other] { $count } de coloane ascunse — clic pentru a le arăta pe toate sau clic dreapta pe un antet pentru a alege
}
grid-copied-staged-sql = Modificările în așteptare au fost copiate ca SQL.
grid-copy-split = { $n ->
        [one] { $n } celulă copiată conține un tab sau un salt de rând — o lipire o va citi ca mai multe celule.
        [few] { $n } celule copiate conțin un tab sau un salt de rând — o lipire va citi fiecare dintre ele ca mai multe celule.
       *[other] { $n } de celule copiate conțin un tab sau un salt de rând — o lipire va citi fiecare dintre ele ca mai multe celule.
    }
grid-duplicate-title = Duplică rândurile
grid-duplicate-question = { $n ->
    [one] Duplici { $rows } rând? Este pus în așteptare ca rând nou și nu se scrie nimic până nu faci commit.
    [few] Duplici { $rows } rânduri? Sunt puse în așteptare ca rânduri noi și nu se scrie nimic până nu faci commit.
   *[other] Duplici { $rows } de rânduri? Sunt puse în așteptare ca rânduri noi și nu se scrie nimic până nu faci commit.
}
grid-nullable-tip = Acceptă NULL
grid-cell-auto = <auto>
grid-cell-required = <obligatoriu>
grid-seed-title = Populează cu rânduri
grid-seed-generate = Generează

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Schemă
footer-panel-ai = Asistent AI
footer-panel-history = Istoric interogări
footer-panel-snippets = Bibliotecă de fragmente
footer-panel-activity = Activitate server
footer-panel-terminal = Terminal
footer-panel-narrow = Fereastra este prea îngustă pentru acest panou — lărgește-o pentru a readuce panoul.
footer-cursor = Rd. { $line }, Col. { $col }
footer-indent-spaces = Spații: { $width }
footer-indent-tabs = Tabulatori: { $width }
footer-spaces = Spații
footer-tabs = Tabulatori
footer-wrap = Încadrare
footer-no-wrap = Fără încadrare
footer-read-only = Doar citire
footer-write-mode = Mod scriere
footer-tx-auto = Auto-commit
footer-tx-manual = Manual
footer-tx-open = Deschisă ({ $count })
footer-tx-aborted = Tranzacție anulată — fă rollback pentru a continua
footer-tx-lost = Tranzacție pierdută
footer-commit = Commit
footer-rollback = Rollback
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Interogare { $n }
lib-result-default-label = Rezultat { $n }
lib-ai-off-note = Asistentul AI este dezactivat. Alege un agent CLI în setările AI pentru a-l activa.
lib-test-testing = Se testează…
lib-test-connected = Conectat
lib-test-failed = Conectarea a eșuat

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Minim
lib-effort-low = Scăzut
lib-effort-medium = Mediu
lib-effort-high = Ridicat
lib-effort-extra = Extra
lib-effort-max = Maxim
lib-schema-scope-active = Doar baza de date activă
lib-schema-scope-all = Toate bazele de date
lib-schema-scope-none = Niciuna
lib-cursor-block = Bloc
lib-cursor-bar = Bară
lib-cursor-underline = Subliniere

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Nicio conexiune
header-new-connection = Conexiune nouă
header-find-anywhere-tip = Caută oriunde ({ $keys })
header-shortcuts-tip = Scurtături de tastatură
header-settings-tip = Setări
header-disconnected = Deconectat
header-retry = Reîncearcă
colour-red = Roșu
colour-orange = Portocaliu
colour-amber = Chihlimbar
colour-green = Verde
colour-teal = Turcoaz
colour-blue = Albastru
colour-purple = Mov
colour-pink = Roz

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = „{ $name }” s-a modificat pe disc.
disk-reload = Reîncarcă
disk-keep-mine = Păstrează varianta mea
results-run-hint = Rulează o interogare  ({ $keys })
results-failed = Instrucțiunea a eșuat.
results-cancelled = Interogare anulată.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = Fixate: { $count } · { $size }
results-pinned-tip = Rezultatele fixate sunt ținute în memorie până le închizi. Închide-le pe cele de care nu mai ai nevoie pentru a elibera memoria.
results-chip-nothing-run = Nu s-a rulat încă nimic în această filă.
results-chip-kept = păstrat — doar citire
results-menu-pin = Fixează
results-menu-unpin = Anulează fixarea
results-menu-close = Închide
results-menu-compare-with = Compară cu { $result }
results-menu-close-others = Închide celelalte rezultate
results-menu-close-all = Închide toate rezultatele

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Caută peste tot

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Nimic de ales

widgets-spinner-verbs = Cugetând, Meditând, Chibzuind, Rumegând, Plămădind, Frământând, Dospind, Fierbând, Clocotind, Gătind, Coacând, Prăjind, Călind, Marinând, Asezonând, Condimentând, Amestecând, Tocând, Glazurând, Caramelizând, Distilând, Fermentând, Infuzând, Macerând, Cernând, Țesând, Croșetând, Împletind, Brodând, Cizelând, Șlefuind, Făurind, Ciocănind, Meșterind, Năzdrăvănind, Plănuind, Urzind, Uneltind, Ticluind, Născocind, Visând, Hoinărind, Rătăcind, Zburdând, Țopăind, Dansând, Valsând, Tangoind, Fredonând, Fluierând, Cântând, Orchestrând, Compunând, Improvizând, Socotind, Calculând, Descifrând, Deducând, Filozofând, Contemplând, Ruminând, Pândind, Iscodind, Scotocind, Cotrobăind, Răscolind, Săpând, Explorând, Navigând, Orbitând, Levitând, Plutind, Învârtind, Rotind, Spiralând, Vârtejind, Bâzâind, Zumzăind, Clipocind, Murmurând, Sclipind, Strălucind, Scânteind, Cristalizând, Germinând, Încolțind, Înflorind, Polenizând, Altoind, Cultivând, Clocind, Incubând, Metamorfozând, Transmutând, Alchimizând, Vrăjind, Fermecând, Descântând, Ghicind, Prevestind, Tânjind, Tălmăcind, Lustruind, Ronțăind, Molfăind, Sorbind, Gustând, Pritocind, Tricotând

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Cere ajutorul asistentului AI.
cmdk-accept = Acceptă
cmdk-reject = Respinge
cmdk-no-changes = Nicio modificare sugerată
cmdk-diff-summary = { $hunks ->
        [one] { $n } bloc · { $removed } − / { $added } +
        [few] { $n } blocuri · { $removed } − / { $added } +
       *[other] { $n } de blocuri · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = valoare
params-kind-tip = Text, număr, NULL sau SQL brut — clic pentru a schimba
params-missing = Nicio valoare pentru { $names }
params-not-a-number = :{ $name } este un număr, dar conține „{ $text }”

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Vezi
editor-run-anyway = Rulează oricum
guard-read-only = Conexiune doar pentru citire.
guard-read-only-refused = Conexiune doar pentru citire: { $reason }
read-only-several = este permisă o singură instrucțiune
read-only-not-a-read = sunt permise doar interogări de citire ({ $heads })
read-only-denied = `{ $word }` nu este permis într-o interogare doar pentru citire
read-only-advances-sequence = `NEXT VALUE FOR` avansează o secvență, ceea ce o interogare doar pentru citire nu face
read-only-unlisted-call = `{ $name }` nu este o funcție cunoscută ca fiind doar pentru citire, așa că nu este permisă într-o interogare doar pentru citire; funcțiile din extensii sau definite în baza de date sunt și ele refuzate
read-only-locks = `{ $clause }` blochează rândurile pe care le citește, ceea ce o interogare doar pentru citire nu face; elimină clauza de blocare
guard-no-database = Nicio bază de date selectată.
guard-writes-one = Această instrucțiune modifică date.
guard-writes-several = Aceste instrucțiuni modifică date.
guard-script = Rulezi fiecare instrucțiune din { $file }? Un script poate crea, modifica și elimina obiecte, iar ce modifică nu poate fi anulat de aici.
guard-staged = { $n ->
    [one] Această rulare înlocuiește rezultatul care conține { $n } modificare în așteptare — rularea ar renunța la ea. Fă mai întâi commit sau renunță la ea ori rulează oricum.
    [few] Această rulare înlocuiește rezultatul care conține { $n } modificări în așteptare — rularea ar renunța la ele. Fă mai întâi commit sau renunță la ele ori rulează oricum.
   *[other] Această rulare înlocuiește rezultatul care conține { $n } de modificări în așteptare — rularea ar renunța la ele. Fă mai întâi commit sau renunță la ele ori rulează oricum.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL elimină toate rândurile din tabel.
guard-truncate = TRUNCATE elimină toate rândurile din tabel.
guard-no-where = A fost detectată o instrucțiune { $kind } fără clauza WHERE.
guard-drop-table = DROP TABLE șterge tabelul și toate rândurile din el.
guard-drop-database = DROP DATABASE șterge baza de date și toate tabelele din ea.
guard-drop-schema = DROP SCHEMA șterge schema și toate tabelele din ea.
guard-drop-owned = DROP OWNED șterge tot ce deține rolul, inclusiv fiecare tabel cu rândurile lui.
guard-drop-cascade = DROP { $obj } … CASCADE șterge fiecare coloană care depinde de el, cu valorile ei.
guard-replace-table = CREATE OR REPLACE TABLE șterge tabelul existent și toate rândurile din el.
guard-replace-database = CREATE OR REPLACE DATABASE șterge baza de date existentă și toate tabelele din ea.
guard-replace-schema = CREATE OR REPLACE SCHEMA șterge schema existentă și toate tabelele din ea.
guard-truncate-partition = TRUNCATE PARTITION elimină toate rândurile din partițiile pe care le numește.
guard-drop-partition = DROP PARTITION șterge partiția și toate rândurile din ea.
quickinfo-table = tabel { $name } · { $database }
quickinfo-view = vizualizare { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } coloană
        [few] { $n } coloane
       *[other] { $n } de coloane
    }
quickinfo-primary-key = cheie primară
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto increment
quickinfo-default = implicit { $value }
quickinfo-generated = generată ca { $expression }
quickinfo-function = funcția { $name }({ $arguments })
quickinfo-function-returns = funcția { $name }({ $arguments }) returnează { $returns }
quickinfo-procedure = procedura { $name }({ $arguments })
quickinfo-procedure-returns = procedura { $name }({ $arguments }) returnează { $returns }
completion-foreign-key = cheie străină
completion-expand-star = extinde *
completion-columns = { $count ->
        [one] { $n } coloană
        [few] { $n } coloane
       *[other] { $n } de coloane
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Eroare de sintaxă
diag-syntax-detail = Eroare de sintaxă: { $detail }
diag-reserved-alias-backticks = `{ $word }` este un cuvânt cheie rezervat și nu poate fi folosit ca alias (pune-l între backticks)
diag-reserved-alias-double-quotes = `{ $word }` este un cuvânt cheie rezervat și nu poate fi folosit ca alias (pune-l între ghilimele duble)
diag-reserved-alias-brackets = `{ $word }` este un cuvânt cheie rezervat și nu poate fi folosit ca alias (pune-l între paranteze drepte)
diag-table-not-found = Tabelul `{ $table }` nu a fost găsit{ $within ->
        [some] {" "}în `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— te refereai la `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = Coloana `{ $column }` nu a fost găsită{ $within ->
        [some] {" "}în `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— te refereai la `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` are aici aliasul `{ $alias }` — califică cu `{ $alias }`
diag-ambiguous-column = Coloana `{ $column }` este ambiguă
diag-join-without-condition = JOIN nu are condiție ON/USING — acesta este un cross join; folosește CROSS JOIN dacă asta intenționezi
diag-comma-join-without-where = Join cu virgulă fără WHERE — acesta este un cross join; adaugă un WHERE sau folosește CROSS JOIN dacă asta intenționezi
diag-not-grouped = Coloana `{ $column }` trebuie să apară în GROUP BY sau să fie folosită într-un agregat
diag-insert-row-width = INSERT numește { $columns } { $columns ->
        [one] coloană
        [few] coloane
       *[other] de coloane
    }, dar acest rând are { $values } { $values ->
        [one] valoare
        [few] valori
       *[other] de valori
    }
diag-row-width-without-list = `{ $table }` primește { $needed } { $needed ->
        [one] valoare
        [few] valori
       *[other] de valori
    } fără listă de coloane, dar acest rând are { $values }
diag-row-width-of-table = `{ $table }` are { $columns } { $columns ->
        [one] coloană
        [few] coloane
       *[other] de coloane
    }, dar acest rând are { $values } { $values ->
        [one] valoare
        [few] valori
       *[other] de valori
    }
diag-nullable-not-in = `{ $column }` poate fi NULL, iar `NOT IN` nu potrivește niciun rând de îndată ce subinterogarea returnează unul — adaugă-i `WHERE { $column } IS NOT NULL` sau folosește NOT EXISTS
diag-update-without-where = UPDATE fără WHERE modifică toate rândurile din tabel
diag-delete-without-where = DELETE fără WHERE elimină toate rândurile din tabel
diag-null-comparison = O comparație cu NULL nu este niciodată adevărată — folosește { $fix }
diag-misspelled-keyword = `{ $word }` pare un cuvânt cheie scris greșit
diag-misspelled-function = `{ $word }` pare o funcție scrisă greșit
diag-flow-merge-needs-semicolon = `MERGE` trebuie să se termine cu `;`
diag-flow-if-without-statement = `IF` nu are nicio instrucțiune de rulat
diag-flow-else-without-statement = `ELSE` nu are nicio instrucțiune de rulat
diag-flow-while-without-statement = `WHILE` nu are nicio instrucțiune de rulat
diag-flow-end-try-without-begin-try = `END TRY` fără un `BEGIN TRY` corespunzător
diag-flow-end-catch-without-begin-catch = `END CATCH` fără un `BEGIN CATCH` corespunzător
diag-flow-end-without-begin = `END` fără un `BEGIN` corespunzător
diag-flow-throw-needs-semicolon = Instrucțiunea dinaintea lui `THROW` trebuie să se termine cu `;`
diag-flow-end-try-without-catch = `END TRY` trebuie urmat de `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` nu are nicio instrucțiune înainte de `ELSE`
diag-flow-else-without-if = `ELSE` fără un `IF` înaintea lui
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` trebuie să urmeze unui `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` nu are un `END TRY` corespunzător
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` nu are un `END CATCH` corespunzător
diag-flow-begin-without-end = `BEGIN` nu are un `END` corespunzător

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Întreabă AI
editor-menu-explain = Explică
editor-menu-optimise = Optimizează
editor-menu-plan = Plan
editor-menu-copy-single-line = Copiază pe un singur rând
editor-menu-make-single-line = Adu pe un singur rând
editor-menu-format-statement = Formatează instrucțiunea
editor-menu-find-usages = Găsește utilizările
editor-menu-create-view = Creează vizualizare
cmdk-optimise-label = Optimizează această interogare
editor-several-carets = Mai multe cursoare și nimic selectat: selectează ce vrei să rulezi sau lasă un singur cursor

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Rulează instrucțiunea curentă
editor-run-everything = Rulează tot

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Caută
find-invalid = Nevalid
find-bad-pattern = Model nevalid
find-bad-pattern-why = Model nevalid: { $why }
find-match-case = Potrivește majusculele
find-whole-word = Cuvânt întreg
find-regex = Expresie regulată
find-scope = Sel
find-in-selection-tip = Caută în selecție
find-replace-placeholder = Înlocuiește
find-replace = Înlocuiește
find-replace-all = Tot
goto-line-label = Salt la:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Nu s-a găsit niciun client { $client } (PATH sau WSL).
cli-no-client-on-path = Nu s-a găsit niciun client { $client } în PATH.
cli-no-tool = Nu s-a găsit { $tool } în PATH.
cli-no-either-tool = Nu s-a găsit nici { $first }, nici { $second } în PATH.
cli-no-either-client = Nu s-a găsit niciun client { $first } sau { $second } în PATH.
cli-sqlcmd-windows-in-wsl = Autentificarea Windows necesită un sqlcmd pentru Windows, iar cel găsit este în WSL. Instalează sqlcmd pe Windows sau deschide conexiunea într-o filă de interogare.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = Clientul găsit în PATH este un shim .cmd/.bat, pe care Windows îl rulează prin cmd.exe — unde un nume de bază de date care conține '&' sau '|' ar deveni o a doua comandă. Indică în Setări executabilul real sau deschide în schimb o filă de interogare.
cli-refused-name-equals = Numele acestei baze de date conține '=', pe care psql l-ar citi ca șir de conexiune și l-ar urma către alt server. Deschide-o în schimb într-o filă de interogare.
cli-refused-name-uri = Numele acestei baze de date începe ca un URI de conexiune, pe care psql l-ar urma către alt server. Deschide-o în schimb într-o filă de interogare.
cli-refused-file-dash = Numele acestui fișier de bază de date începe cu '-', pe care clientul sqlite3 l-ar citi ca opțiune, nu ca fișier. Redenumește-l sau indică o cale (./name.db) și deschide-l din nou.
cli-refused-sqlcmd-entra = sqlcmd nu poate primi tokenul de autentificare al Azure CLI, așa că această conexiune nu se poate deschide în el. Deschide-o în schimb într-o filă de interogare.
cli-refused-sqlcmd-host = Gazda acestei conexiuni conține un caracter pe care sqlcmd îl citește ca parte a adresei (',', ';', un al doilea '\' sau un prefix ':'). Setează portul în câmpul lui și deschide-o din nou.
cli-refused-sqlcmd-ca = sqlcmd nu poate verifica față de un fișier CA. Adaugă CA-ul în depozitul de încredere al sistemului și golește câmpul CA al conexiunii sau deschide conexiunea într-o filă de interogare.
cli-refused-sqlcmd-client-cert = SQL Server nu autentifică cu certificat client, așa că sqlcmd nu are unde trimite certificatul acestei conexiuni. Golește certificatul și cheia client și deschide-o din nou.
cli-refused-wsl-paths = Căile certificatelor TLS ale acestei conexiuni sunt căi Windows, pe care clientul din WSL nu le poate deschide. Instalează un client nativ sau mută certificatele în WSL.
cli-refused-tunnelled-verify = Această conexiune verifică certificatul serverului și ajunge la el printr-un tunel SSH, așa că clientului i s-ar spune să se conecteze la 127.0.0.1 și să verifice certificatul față de această adresă — lucru pe care certificatul serverului nu îl poate satisface. Deschide în schimb o filă de interogare sau coboară modul TLS pentru sesiunea CLI.
cli-refused-not-container-id = docker ps a afișat ceva ce nu este un id de container.
cli-refused-file-line-break = Acel nume de fișier conține un salt de rând, pe care clientul l-ar citi ca două argumente. Alege alt nume.
cli-refused-role = Rolul din SQL-ul de inițializare al acestei conexiuni nu poate fi transmis uneltei așa cum este.
cli-refused-mysql-source-path = Clientul mysql ar citi calea acelui fișier ca pe comenzi proprii (un ';' sau un '\' în ea ori un spațiu la un capăt). Mută-l sau redenumește-l, apoi restaurează-l.
cli-refused-cloud-token = Această conexiune se autentifică prin tokenul unui CLI cloud, care nu este transmis unui client extern. Deschide-o în schimb într-o filă de interogare.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Nu se poate determina de unde rulează Schemaic: { $error }
install-registry-open = Mediul de utilizator nu a putut fi deschis în registru: { $error }
install-path-read = PATH-ul de utilizator nu a putut fi citit: { $error }
install-path-write = PATH-ul de utilizator nu a putut fi scris: { $error }
install-look-at = { $path } nu a putut fi examinat: { $error }
install-read-link = Linkul de la { $path } nu a putut fi citit: { $error }
install-create = { $path } nu a putut fi creat: { $error }
install-remove-old = Vechiul { $path } nu a putut fi eliminat: { $error }
install-remove = { $path } nu a putut fi eliminat: { $error }
install-path-is-windows = O intrare în PATH-ul de utilizator este o instalare Windows; această platformă folosește în schimb un link.
install-link-is-unix = Legarea comenzii este o instalare macOS și Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Nu se poate determina în ce dosar se află { $exe }.
install-refuse-no-shim = Nu există niciun schemaic.com lângă această copie Schemaic, în { $dir }, așa că nu există o comandă de consolă de pus în PATH. Aplicația instalată are una; o versiune de dezvoltare nu are.
install-refuse-semicolon = Această copie Schemaic se află în { $dir }, al cărui nume conține un ';' — PATH l-ar citi ca două dosare diferite. Mut-o într-un loc fără acest caracter și încearcă din nou.
install-refuse-exe-replaced = { $exe } a fost înlocuit în timp ce Schemaic rula, așa că un link spre el nu ar indica nimic. Repornește Schemaic, apoi instalează din nou.
install-refuse-no-home = Nu există un director personal care să conțină linkul, așa că nu există unde căuta.
install-refuse-path-not-text = PATH-ul de utilizator nu este stocat ca text în registru, așa că a fost lăsat neatins.
install-refuse-path-not-unicode = PATH-ul de utilizator conține caractere care nu pot fi citite înapoi în siguranță, așa că a fost lăsat neatins.
install-refuse-portable = Aceasta este o copie portabilă, așa că Elimină nu poate spune dacă Instalează a pus { $dir } în PATH sau dacă era deja acolo, cu alte programe în el. Scoate-l din Variabile de mediu dacă l-a adăugat Instalează.
install-refuse-spot-taken = { $link } există deja și nu este un link, așa că Instalează l-a lăsat neatins. Mută-l deoparte și încearcă din nou.
install-refuse-points-elsewhere = { $link } indică deja spre { $to }. Elimină-l dacă este o copie veche a Schemaic, apoi încearcă din nou.
install-refuse-unlink-elsewhere = { $link } indică spre { $to }, nu spre această copie Schemaic, așa că Elimină l-a lăsat neatins.
install-refuse-unlink-not-ours = { $link } nu este un link creat de Schemaic, așa că Elimină l-a lăsat neatins.
install-report-already = Comanda schemaic este deja în PATH, în { $dir }.
install-report-added = { $dir } a fost adăugat în PATH-ul de utilizator. Deschide un terminal nou pentru a folosi schemaic — unul deja deschis păstrează vechiul PATH.
install-report-linked = { $link } a fost legat la { $target }.
install-report-linked-if-not-found = { $link } a fost legat la { $target }. Dacă un terminal nou nu găsește schemaic, adaugă { $dir } în PATH în profilul shell-ului.
install-report-linked-not-on-path = { $link } a fost legat la { $target }. { $dir } nu este încă în PATH — adaugă-l în profilul shell-ului, apoi deschide un terminal nou.
install-report-removed = { $dir } a fost eliminat din PATH-ul de utilizator. Un terminal deja deschis păstrează vechiul PATH.
install-report-path-absent = { $dir } nu este în PATH-ul de utilizator, așa că nu era nimic de eliminat.
install-report-unlinked = { $link } a fost eliminat.
install-report-link-absent = Nu există { $link }, așa că nu era nimic de eliminat.
install-report-package = Comanda schemaic din { $dir } a venit cu pachetul instalat, nu de la Instalează — elimină pachetul pentru a o elimina.
install-hint-adds-folder = Rulează Schemaic dintr-un terminal după nume. Adaugă dosarul lui în PATH-ul de utilizator.
install-hint-links = Rulează Schemaic dintr-un terminal după nume. Îl leagă în ~/.local/bin.
install-hint-folder-on-path = Rulează Schemaic dintr-un terminal după nume. Dosarul lui, { $dir }, este deja în PATH-ul de utilizator.
install-hint-comes-with-app = Rulează Schemaic dintr-un terminal după nume. Comanda schemaic vine cu aplicația, la { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = Tunelul SSH nu este încă stabilit
tunnel-could-not-open = Tunelul SSH nu a putut fi deschis: { $error }. Reîmprospătează schema pentru a încerca din nou.
tunnel-server-failed = Conectarea prin tunelul SSH a eșuat: { $error }. Reîmprospătează schema pentru a încerca din nou.

gate-plan-not-connected = Neconectat — planul nu a fost rulat. Serverul nu a răspuns la verificarea stării, așa că nu i s-a trimis nimic.
gate-plan-connection-changed = Ai schimbat conexiunea în timp ce Schemaic verifica dacă e accesibilă, așa că planul nu a fost rulat. Revino și cere din nou.
gate-plan-tab-moved-on = Ai schimbat fila în timp ce conexiunea era reverificată, așa că planul nu a fost rulat. El descrie fila din care a fost cerut — revino la acea filă și cere din nou.
gate-connection-changed = Nu s-a rulat pe { $name } — ai schimbat conexiunea în timp ce Schemaic verifica dacă e accesibilă. Revino și încearcă din nou.
gate-not-connected = Neconectat la { $name }. Serverul nu a răspuns — verifică dacă rulează și dacă setările acestei conexiuni sunt corecte.
gate-this-connection = această conexiune
gate-run-tab-moved-on = Ai schimbat fila în timp ce conexiunea era reverificată, așa că instrucțiunea nu a fost rulată. A fost verificată în raport cu fila în care a fost scrisă, iar rularea ei aici ar rula-o în altă parte. Revino la acea filă și rulează-o din nou.
gate-commit-not-connected = Neconectat — nu s-a încercat commit-ul. Modificările în așteptare sunt păstrate.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = { $program } nu a putut fi pornit: { $error }. Alege alt shell în Setări, Terminal.
term-cli-read-only = Această conexiune este doar pentru citire în Schemaic. Clientul ei de linie de comandă nu ar fi, așa că nu este deschis aici.
backup-read-only-server = Această conexiune este doar pentru citire în Schemaic, iar copia de rezervă a acestui motor este scrisă de server, așa că nu s-a făcut niciuna.
backup-dialog-title = Copie de rezervă a bazei de date
backup-kind-pg-dump = Dump PostgreSQL în format custom
backup-kind-sql = Script SQL
backup-kind-sqlite = Bază de date SQLite
backup-not-started = Copia de rezervă nu a pornit: { $error }
backup-done = Copie de rezervă salvată în { $path }.
backup-failed = Copia de rezervă a eșuat: { $error }
restore-title = Restaurează din fișier
restore-confirm = Rulezi { $path } în { $database } pe { $connection }? Ce creează sau înlocuiește nu poate fi anulat. Rulează în terminal, unde Ctrl+C îl oprește.
restore-read-only = Această conexiune este doar pentru citire în Schemaic, așa că nu se restaurează nimic în ea.
restore-engine-not-here = Copiile de rezervă ale acestui motor nu se restaurează de aici.
restore-unreadable = { $path } nu a putut fi citit înainte de restaurare, așa că nu s-a rulat nimic: { $error }
restore-refused = { $path } trece la altă bază de date în timp ce rulează — { $switches } — așa că, restaurat în { $database }, ar scrie în schimb acolo. Nu s-a rulat nimic. Restaurează-l în baza de date pe care o numește sau elimină acele rânduri dintr-o copie a fișierului.
restore-refused-more = { $path } trece la altă bază de date în timp ce rulează — { $switches } și încă { $more } — așa că, restaurat în { $database }, ar scrie în schimb acolo. Nu s-a rulat nimic. Restaurează-l în baza de date pe care o numește sau elimină acele rânduri dintr-o copie a fișierului.
restore-switch = { $how } `{ $name }` la rândul { $line }
restore-switch-unnamed = un { $how } la rândul { $line } care nu numește nicio bază de date pe care Schemaic o poate citi

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Închide „{ $name }”
tab-close-unsaved = „{ $name }” are modificări nesalvate. Închiderea renunță la ele; fișierul de pe disc rămâne neschimbat. Închizi oricum?
tab-close-all-title = Închide toate filele
tab-close-all-question = Sigur vrei să închizi toate filele?
tab-close-others-title = Închide celelalte file
tab-close-others-question = Sigur vrei să închizi toate celelalte file?
file-open-title = Deschide fișier SQL
file-save-title = Salvează fișierul SQL
file-sql-filter = Script SQL
file-read-failed = { $path } nu a putut fi citit: { $error }
file-save-failed = { $path } nu a putut fi salvat: { $error }
file-too-large = { $path } are { $size } — prea mare pentru a fi deschis într-o filă de editor. Schemaic și-ar petrece mare parte din timp reanalizându-l. Rulează-l dintr-o filă de interogare sau folosește Import pentru un fișier de date.
file-too-large-short = Fișierul este prea mare pentru a fi deschis.
file-large-title = Deschizi un fișier mare?
file-large-question = „{ $name }” are { $size }. Schemaic reanalizează întregul document la scurt timp după fiecare pauză în tastare, așa că un fișier de această dimensiune face editorul lent cât timp fila este deschisă. Îl deschizi oricum?
file-changed-on-disk = { $path } s-a modificat pe disc de când a fost deschis. Salvarea acum ar elimina acele modificări — reîncarcă fișierul (sau Salvează ca, cu alt nume).
file-lossy-title = Înlocuiești octeții ilizibili?
file-lossy-question = Schemaic nu a putut citi ca text fiecare octet din „{ $name }” și i-a afișat ca „�”. Salvarea scrie ce vezi, așa că fiecare dintre ei este înlocuit definitiv — în tot fișierul, nu doar în rândurile pe care le-ai editat. Salvezi oricum?
file-replace-title = Înlocuiești fișierul?
file-replace-question = „{ $name }” există deja — „{ $typed }” a fost salvat cu extensia .sql adăugată. Îl înlocuiești?
file-reload-title = Reîncarcă de pe disc
file-reload-unsaved = „{ $name }” are modificări nesalvate. Reîncărcarea renunță la ele. Reîncarci oricum?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Un export rulează deja. Anulează-l sau așteaptă să se termine.
export-failed = Exportul a eșuat: { $error }
export-worker-died = Exportul a eșuat: procesul de lucru s-a oprit: { $error }
export-not-published = Exportul a eșuat: { $path } nu a putut fi publicat: { $error }
export-not-renamed = Exportul a scris { $part }, dar nu l-a putut redenumi în { $path }: { $error }
export-nothing-matched = Nimic de exportat — niciun tabel nu s-a potrivit cu selecția.
export-stopped = Export anulat.
dump-failed = Dump-ul a eșuat: { $error }
blob-load-too-large = Acel fișier are { $size } — maximul care poate fi încărcat este { $most }.
blob-load-failed = Încărcarea a eșuat: { $error }
blob-save-failed = Salvarea a eșuat: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = { $path } nu a putut fi scris: { $error }
settings-export-title = Exportă setările
settings-import-title = Importă setările
settings-file-filter = Setări Schemaic
settings-import-nothing = Nimic de importat: fiecare setare din { $path } coincide deja cu ale tale și nu adaugă fragmente.
settings-import-question = { $path } modifică { $changed ->
        [one] { $changed } setare
        [few] { $changed } setări
       *[other] { $changed } de setări
    } și adaugă { $snippets ->
        [one] { $snippets } fragment
        [few] { $snippets } fragmente
       *[other] { $snippets } de fragmente
    }. Setările pe care le modifică îți înlocuiesc valorile actuale; fragmentele sunt adăugate lângă ale tale.
settings-import-unreadable = nu a putut fi citit: { $error }
connections-export-title = Exportă conexiunile
connections-file-filter = Conexiuni Schemaic
connections-replace-title = Înlocuiește „{ $file }”
connections-replace-question = În acel dosar există deja un fișier numit „{ $file }”. Exportul adaugă această terminație la numele pe care l-ai tastat, așa că dialogul de salvare nu a întrebat despre el. Înlocuirea nu poate fi anulată.
import-choose-file-title = Alege un fișier de conexiuni
import-no-containers = Nu rulează niciun container de baze de date.
import-added = { $count ->
        [one] S-a adăugat { $count } conexiune.
        [few] S-au adăugat { $count } conexiuni.
       *[other] S-au adăugat { $count } de conexiuni.
    }
import-added-foreign = { $count ->
        [one] S-a adăugat { $count } conexiune.
        [few] S-au adăugat { $count } conexiuni.
       *[other] S-au adăugat { $count } de conexiuni.
    } Doar citire și eticheta de mediu nu sunt preluate din alt client — setează-le în Gestionează conexiunile pentru tot ce contează.
conn-delete-title = Șterge „{ $name }”
conn-delete-question = Aceasta șterge conexiunea, parola stocată, conversația AI salvată, istoricul interogărilor și filele ei. Acțiunea nu poate fi anulată.
conn-delete-keyring-kept = Schemaic nu a putut accesa keyring-ul sistemului, așa că parola stocată și secretele SSH ale conexiunii șterse **sunt încă în el**.
conn-delete-keyring-later = Vor fi eliminate data viitoare când o conexiune este ștearsă sau salvată cât timp keyring-ul este accesibil. Între timp nicio conexiune nouă nu le poate primi — id-urile conexiunilor nu sunt niciodată refolosite.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = { $name } nu a putut fi citit ({ $error }).
notice-unreadable-kept = Fișierul ilizibil a fost păstrat ca { $name }.corrupt; Schemaic a revenit la copia de rezervă sau la valorile implicite, dacă nici copia de rezervă nu a putut fi citită.
notice-unreadable-swept = Acea copie poate conține parole în clar, așa că este eliminată data viitoare când ștergi o conexiune — recuperează din ea tot ce îți trebuie înainte de atunci.
notice-missing = { $name } lipsea.
notice-missing-restored = Schemaic l-a recuperat din { $name }{ $from }, care era încă acolo. Va fi rescris data viitoare când această setare se schimbă — până atunci acea copie alăturată este singura.
notice-keyring-unread = { $n ->
    [one] Schemaic nu a putut citi { $n } secret salvat din keyring-ul sistemului.
    [few] Schemaic nu a putut citi { $n } secrete salvate din keyring-ul sistemului.
   *[other] Schemaic nu a putut citi { $n } de secrete salvate din keyring-ul sistemului.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic nu a putut citi { $n } secret salvat din keyring-ul sistemului ({ $error }).
    [few] Schemaic nu a putut citi { $n } secrete salvate din keyring-ul sistemului ({ $error }).
   *[other] Schemaic nu a putut citi { $n } de secrete salvate din keyring-ul sistemului ({ $error }).
}
notice-keyring-unread-kept = Acele parole nu sunt disponibile în această sesiune și **nu** au fost șterse — lasă câmpurile goale și vor reveni după ce keyring-ul devine accesibil. Tastarea uneia noi o suprascrie pe cea stocată.
notice-in-the-clear = { $n ->
    [one] Schemaic nu a putut accesa keyring-ul sistemului, așa că { $n } parolă a fost salvată **ca text simplu** în `connections.json`.
    [few] Schemaic nu a putut accesa keyring-ul sistemului, așa că { $n } parole au fost salvate **ca text simplu** în `connections.json`.
   *[other] Schemaic nu a putut accesa keyring-ul sistemului, așa că { $n } de parole au fost salvate **ca text simplu** în `connections.json`.
}
notice-in-the-clear-where = Acel fișier se află în dosarul deschis de butonul *Deschide dosarul* din Setări → General, lângă jurnal — verifică înainte de a-l partaja. O nouă salvare, după ce keyring-ul devine accesibil, le mută înapoi.
notice-undeleted = { $n ->
    [one] { $n } parolă golită este încă în keyring-ul sistemului — Schemaic nu a putut-o șterge.
    [few] { $n } parole golite sunt încă în keyring-ul sistemului — Schemaic nu le-a putut șterge.
   *[other] { $n } de parole golite sunt încă în keyring-ul sistemului — Schemaic nu le-a putut șterge.
}
notice-undeleted-after = Câmpul este gol pe disc, dar următoarea lansare îl va completa la loc. Salvează din nou după ce keyring-ul devine accesibil.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Nu s-a returnat niciun SQL
inline-ai-not-sql = Modelul nu a returnat SQL
inline-ai-failed = generarea a eșuat
ai-fill-no-value = AI-ul nu a returnat nicio valoare.
conn-gone = conexiunea nu mai există
tx-session-not-ready = conexiunea tranzacției nu este pregătită — comută pe Auto-commit și înapoi
tx-session-open-failed = nu s-a putut deschide o conexiune de tranzacție: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = Instrucțiunea a fost anulată.
tx-committed-before-ddl = Tranzacția în care a rulat aceasta nu mai există: MySQL face commit înainte de a rula o instrucțiune DDL, așa că instrucțiunile deja din ea sunt permanente și Rollback nu le va anula.
tx-rolled-back-by-server = Tranzacția în care a rulat aceasta nu mai există: SQL Server a făcut rollback, așa că fiecare instrucțiune din ea de la început este anulată.
tx-doomed = Tranzacția în care a rulat aceasta nu mai poate face commit: SQL Server a marcat-o ca imposibil de confirmat, așa că Rollback este singura cale de urmat și anulează fiecare instrucțiune din ea.
tx-after-own-commit = Tranzacția în care a rulat aceasta s-a încheiat, iar aceasta a rulat un COMMIT propriu înainte să eșueze, așa că ce a fost înainte de acel COMMIT este probabil confirmat, nu anulat. Verifică datele înainte de a rula din nou ceva din ele.
tx-lost-on-commit = Conexiunea s-a pierdut cu o tranzacție deschisă. S-a pierdut la un COMMIT, așa că nu se știe dacă { $stmts ->
        [0] tranzacția a fost confirmată
        [one] { $stmts } instrucțiune rulată în ea de la început a fost confirmată
        [few] cele { $stmts } instrucțiuni rulate în ea de la început au fost confirmate
       *[other] cele { $stmts } de instrucțiuni rulate în ea de la început au fost confirmate
    } sau anulate prin rollback: serverul poate să fi făcut commit înainte ca conexiunea să cadă. Verifică datele înainte de a rula din nou ceva din ele.
tx-lost-on-implicit-commit = Conexiunea s-a pierdut cu o tranzacție deschisă. MySQL face commit unei tranzacții deschise înainte de a rula această instrucțiune, așa că { $stmts ->
        [0] nu rulase încă nimic în ea, deci nu era nimic de confirmat
        [one] { $stmts } instrucțiune rulată în ea de la început a fost cel mai probabil confirmată atunci și este permanentă
        [few] cele { $stmts } instrucțiuni rulate în ea de la început au fost cel mai probabil confirmate atunci și sunt permanente
       *[other] cele { $stmts } de instrucțiuni rulate în ea de la început au fost cel mai probabil confirmate atunci și sunt permanente
    }. Verifică datele înainte de a rula din nou ceva din ele.
tx-lost-rolled-back = Conexiunea s-a pierdut cu o tranzacție deschisă. Serverul face rollback unei tranzacții neterminate când conexiunea ei se încheie, așa că { $stmts ->
        [0] nu rulase încă nimic în ea
        [one] { $stmts } instrucțiune rulată în ea de la început este anulată
        [few] cele { $stmts } instrucțiuni rulate în ea de la început sunt anulate
       *[other] cele { $stmts } de instrucțiuni rulate în ea de la început sunt anulate
    }.
tx-lost-rolled-back-mysql = Conexiunea s-a pierdut cu o tranzacție deschisă. Serverul face rollback unei tranzacții neterminate când conexiunea ei se încheie, așa că { $stmts ->
        [0] nu rulase încă nimic în ea
        [one] { $stmts } instrucțiune rulată în ea de la început este anulată — cu excepția unei scrieri într-un tabel care nu acceptă tranzacții (MyISAM, MEMORY), pe care niciun rollback nu o anulează
        [few] cele { $stmts } instrucțiuni rulate în ea de la început sunt anulate — cu excepția unei scrieri într-un tabel care nu acceptă tranzacții (MyISAM, MEMORY), pe care niciun rollback nu o anulează
       *[other] cele { $stmts } de instrucțiuni rulate în ea de la început sunt anulate — cu excepția unei scrieri într-un tabel care nu acceptă tranzacții (MyISAM, MEMORY), pe care niciun rollback nu o anulează
    }.
tx-not-sent-timeout = Timpul limită pentru instrucțiuni a expirat cât această rulare încă aștepta conexiunea filei. O operație anterioară pe ea — un commit sau recitirea de după unul — nu se terminase, așa că instrucțiunea nu a fost trimisă niciodată la server. Nu a rulat nimic, iar tranzacția este exact unde era.
tx-not-sent-cancelled = Această rulare a fost anulată cât încă aștepta conexiunea filei. O operație anterioară pe ea — un commit sau recitirea de după unul — nu se terminase, așa că instrucțiunea nu a fost trimisă niciodată la server. Nu a rulat nimic, iar tranzacția este exact unde era.
snippet-default-name = Fragment
snippet-delete-title = Șterge fragmentul
snippet-delete-question = Ștergi „{ $name }”? Acțiunea nu poate fi anulată.
copy-target-read-only = Conexiunea țintă este doar pentru citire, așa că nu s-au copiat rânduri.
copy-target-gone = Conexiunea țintă nu mai există, așa că nu s-au copiat rânduri.
copy-not-started = Nu s-au copiat rânduri: { $error }
copy-counter-not-moved = { $count ->
        [one] { $count } rând a fost copiat, dar contorul tabelului nu a fost mutat dincolo de el, așa că următoarea inserare poate intra în conflict: { $error }
        [few] { $count } rânduri au fost copiate, dar contorul tabelului nu a fost mutat dincolo de ele, așa că următoarea inserare poate intra în conflict: { $error }
       *[other] { $count } de rânduri au fost copiate, dar contorul tabelului nu a fost mutat dincolo de ele, așa că următoarea inserare poate intra în conflict: { $error }
    }
copy-stopped = Copierea s-a oprit la o eroare: { $error }
compare-reading-failed = Citirea { $database }: { $error }
monitor-no-row-key = Nicio cheie de rând pentru acest tabel — modificările nu pot fi urmărite.
monitor-restarted = Coloanele tabelului s-au schimbat, așa că urmărirea modificărilor a repornit de aici.
timeout-cancelled-app = Anulat: instrucțiunea a rulat mai mult decât timpul limită de { $timeout }. Schimbă-l sau dezactivează-l în Setări → Interogare.
timeout-cancelled-connection = Anulat: instrucțiunea a rulat mai mult decât timpul limită de { $timeout }. Schimbă-l sau dezactivează-l în setările acestei conexiuni.
ai-chat-no-database = Baza de date nu poate fi accesată, așa că asistentul nu poate fi pornit pentru această conexiune. Verifică conexiunea și încearcă din nou.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **Asistentul nu a putut porni.** Schemaic dă fiecărui proces de agent un director de lucru privat, iar nici directorul temporar al sistemului, nici directorul de configurare nu au permis crearea unuia — așa că nu există unde să ruleze care să nu fie directorul altcuiva. Verifică dacă directorul temporar permite scrierea și încearcă din nou.
ai-no-reply-file = Nu s-a putut crea un fișier temporar pentru răspuns.
ai-inline-no-opencode-config = Configurația OpenCode nu a putut fi scrisă, așa că generarea nu a pornit (fără ea, CLI-ul ar rula cu propriile unelte activate).
ai-inline-no-copilot-home = Directorul personal privat al GitHub Copilot nu a putut fi creat, așa că generarea nu a pornit (fără el, CLI-ul ar încărca propria ta configurație Copilot, inclusiv serverele MCP și hookurile ei).
ai-inline-no-env-writer = { $cli } este marcat ca izolat prin mediul său, dar Schemaic nu are cum să pregătească acel mediu, așa că generarea nu a pornit.
ai-inline-no-cursor-rules = Fișierul de permisiuni al Cursor nu a putut fi scris, așa că generarea nu a pornit (fără el, CLI-ul scrie fișiere fără să întrebe).
ai-inline-no-rules-writer = { $cli } este marcat ca restricționat printr-un fișier de permisiuni, dar Schemaic nu are cum să scrie unul pentru el, așa că generarea nu a pornit.
ai-no-tools = **Această sesiune nu are unelte de bază de date.** { $cause }, așa că asistentul nu poate căuta nimic — poate răspunde doar din schița de schemă aflată deja în promptul său. Închiderea și redeschiderea panoului va încerca din nou.
ai-no-endpoint-file = Schemaic nu a putut crea fișierul privat care îi spune asistentului cum să ajungă la baza ta de date
ai-cursor-not-approved = Schemaic nu și-a putut aproba uneltele de bază de date în Cursor ({ $error })
ai-antigravity-not-registered = Schemaic nu și-a putut înregistra uneltele de bază de date în Antigravity
ai-antigravity-shared = O altă fereastră Schemaic folosește deja Antigravity. Antigravity păstrează o singură înregistrare MCP pe tot calculatorul, care poate indica o singură conexiune, așa că asistentul acestei ferestre nu are unelte de bază de date
ai-antigravity-not-claimed = Schemaic nu a putut prelua configurația Antigravity pentru această sesiune
ai-refused-no-opencode-config = Schemaic nu a putut scrie configurația care restricționează OpenCode, așa că asistentul este dezactivat — rularea fără acel fișier i-ar da sesiunii un shell. Verifică dacă directorul de date al aplicației permite scrierea.
ai-refused-no-copilot-home = Schemaic nu a putut crea directorul personal privat al GitHub Copilot, așa că asistentul este dezactivat — fără el, CLI-ul ar încărca propria ta configurație Copilot, inclusiv serverele MCP și hookurile ei. Verifică dacă directorul de date al aplicației permite scrierea.
ai-refused-no-cursor-rules = Schemaic nu a putut scrie fișierul de permisiuni care împiedică Cursor să scrie fișiere și să ruleze comenzi, așa că asistentul este dezactivat — fără el, Cursor scrie fișiere fără să întrebe. Verifică dacă directorul de date al aplicației permite scrierea.
ai-launch-failed-turn = CLI-ul `{ $bin }` nu a putut fi lansat ({ $error }). Asigură-te că { $cli } este instalat sau indică o cale în Setări → AI.
ai-launch-failed-session = CLI-ul `{ $bin }` nu a putut fi lansat ({ $error }). Asigură-te că { $cli } este instalat (sau setează-i calea în Setări → AI).
ai-turn-ended = Tura { $cli } s-a încheiat neașteptat: { $error }
ai-session-ended-badly = Sesiunea AI s-a încheiat neașteptat: { $error }
ai-session-ended = Sesiunea { $cli } s-a încheiat. Întreabă din nou pentru a începe una nouă.
ai-session-restart-failed = Sesiunea { $cli } nu a putut fi repornită după oprire ({ $error }), așa că s-a încheiat. Întreabă din nou pentru a începe una nouă.
ai-session-not-accepting = Sesiunea { $cli } nu mai acceptă întrebări. Întreabă din nou pentru a începe una nouă.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = Contextul trimis către { $cli } este prea mare pentru o singură linie de comandă ({ $unit }: { $total }; această platformă permite aproximativ { $limit }). { $levers }
ai-unit-characters = caractere
ai-unit-bytes = octeți
ai-levers-schema-and-query = Restrânge Setări → AI → Context de schemă la baza de date activă (sau la Niciuna) ori scurtează interogarea din editor.
ai-levers-sample = Coboară accesul AI la date al acestei conexiuni sub „Poate citi date”, ceea ce oprește trimiterea eșantionului de rânduri, sau restrânge Setări → AI → Context de schemă.
ai-batch-shim = { $cli } s-a rezolvat la un shim batch ({ $bin }), căruia nu i se poate da un prompt pe mai multe rânduri — Windows rulează un .cmd prin cmd.exe, care refuză argumentul înainte ca { $cli } să pornească. Setează Setări → AI → calea CLI la executabilul real (o instalare npm are de obicei unul în node_modules, lângă shim).
ai-cli-exited = CLI-ul { $cli } s-a încheiat cu starea { $code }
ai-cli-signalled = CLI-ul { $cli } a fost oprit de un semnal
ai-constraint-sandboxed = { $cli } rulează izolat: uneltele lui încorporate nu pot scrie fișiere sau rula comenzi, deși pot încă citi acest calculator.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Orice server MCP pe care l-ai înregistrat la el este disponibil și acestei sesiuni, iar Schemaic nu îl poate restricționa.
    }
ai-constraint-workspace-rules = { $cli } este limitat la a răspunde printr-un fișier de permisiuni pe care Schemaic îl scrie pentru această sesiune, nu printr-un sandbox: uneltelor lui li se refuză scrierea fișierelor și rularea comenzilor, dar pot încă citi fișiere de pe acest calculator.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Serverele MCP pe care le-ai înregistrat la el pornesc și ele odată cu această sesiune; uneltele lor sunt refuzate dacă setările tale nu le permit.
    }
ai-constraint-denylist = Această versiune { $cli } nu acceptă opțiunea care îi golește uneltele încorporate, așa că acestea sunt blocate printr-o listă de interdicții — mai slab și nu ceva ce Schemaic poate garanta. Actualizarea CLI-ului restabilește izolarea completă.
ai-constraint-unrestricted = Schemaic nu a putut restricționa complet uneltele încorporate ale { $cli } pentru această sesiune, așa că ar putea face mai mult decât să răspundă la întrebări despre această bază de date.
ai-constraint-unknown = Schemaic nu a putut confirma că acest executabil { $cli } acceptă opțiunile care îl restricționează, așa că asistentul este dezactivat. Verifică calea în Setări → AI.
ai-seed-empty = modelul nu a returnat niciun conținut
ai-seed-not-json = răspunsul nu a fost JSON valid: { $error }
ai-seed-not-rows = răspunsul nu a fost un tablou JSON de obiecte rând

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } nu este un fișier.
import-source-too-big = { $path } are { $size }, peste cei { $most } pe care îi citește aceasta.
import-source-unreadable = { $path } nu a putut fi citit: { $error }
docker-not-found = Docker nu a fost găsit — nici în PATH, nici în WSL.
docker-not-started = Docker nu a putut fi pornit: { $error }
docker-timed-out = Docker nu a răspuns în { $secs } secunde.
docker-not-waited = Nu s-a putut aștepta după Docker: { $error }
docker-answered = Docker a răspuns: { $error }
docker-exited = Docker s-a încheiat cu { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = cititorul de fișiere s-a oprit neașteptat: { $error }
script-changed-since-read = Acest fișier s-a modificat pe disc după ce a fost citit, așa că rezumatul de mai sus nu este ce ar rula. Alege-l din nou.
script-no-terminator = Acest fișier nu conține niciun terminator de instrucțiune, așa că nu este un script SQL pe care Schemaic îl poate rula. Verifică dacă ai ales fișierul dorit.
ai-cursor-enable-unfinished = `mcp enable` nu s-a terminat
