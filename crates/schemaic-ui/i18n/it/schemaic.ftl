# Schemaic — Italian catalogue.
#
# Machine-drafted and unreviewed (docs/roadmap.md); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Italian.

## Settings › Appearance › Language

settings-language = Lingua
settings-language-system = Sistema ({ $language })
settings-language-restart = La modifica della lingua avrà effetto al prossimo avvio di Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Dimensione carattere
settings-term-cursor-style = Stile del cursore
settings-term-copy-on-select = Copia alla selezione
settings-term-copy-on-select-hint = Copia il testo selezionato negli appunti appena termina la selezione.
settings-term-blink = Cursore lampeggiante
settings-term-blink-hint = Fai lampeggiare il cursore quando il terminale ha il focus.

## Settings › AI

settings-ai-agent = CLI dell'agente
settings-ai-disabled = Disattivato
settings-ai-off-hint = Non viene inviato nulla ad alcun agente, non viene avviata alcuna CLI e non viene proposta alcuna azione IA. Scegli una CLI per attivare l'assistente.
settings-ai-cli-path = Percorso di { $cli }
settings-ai-cli-path-placeholder = Lascia vuoto per il rilevamento automatico
settings-ai-detected = Rilevato automaticamente: { $path }
settings-ai-detect-failed = Rilevamento automatico non riuscito. { $cli } non trovato — indica un percorso qui sopra.
settings-ai-cli-path-missing = Il file non esiste.
settings-ai-model = Modello
settings-ai-model-placeholder = Lascia vuoto per il modello predefinito della CLI
settings-ai-model-hint = Qualsiasi id accettato da questa CLI — un alias, o uno snapshot datato per fissarne uno tra un aggiornamento e l'altro.
settings-ai-model-hint-with-chips = Qualsiasi id accettato da questa CLI — un alias, o uno snapshot datato per fissarne uno tra un aggiornamento e l'altro. I pulsanti sono scorciatoie, non l'elenco completo.
settings-ai-effort = Impegno
settings-ai-instructions = Istruzioni personalizzate
settings-ai-instructions-placeholder = Dialetto, convenzioni, regole interne…
settings-ai-schema-context = Contesto dello schema
settings-ai-schema-context-hint = Quanta struttura del database accompagna ogni messaggio. Nessuno nega anche gli strumenti dello schema, così l'assistente ti chiede i nomi invece di leggere da sé il catalogo.
settings-ai-data-access = Accesso ai dati
settings-ai-data-access-where = Si imposta per connessione, nelle impostazioni della connessione.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Solo lo schema
ai-data-on-request = Solo ciò che allego
ai-data-full = Lascia leggere i dati
ai-data-schema-only-hint = Solo la struttura — nomi, tipi e definizioni. Nessuna riga lascia mai questo computer e gli allegati sono rifiutati.
ai-data-on-request-hint = L'assistente non legge dati di sua iniziativa. Le righe che alleghi da un risultato lasciano questo computer insieme a quella domanda.
ai-data-full-hint = L'assistente può eseguire da sé query in sola lettura e leggere righe di esempio. Tutto ciò che legge lascia questo computer.
settings-ai-gutter = Filetto colorato sulle risposte
settings-ai-gutter-hint = Segna le risposte dell'assistente con un filetto colorato lungo il bordo destro. Se disattivato, hanno lo stesso margine su entrambi i lati.

## Settings › section list

settings-section-general = Generale
settings-section-editor = Editor
settings-section-formatting = Formattazione del codice
settings-section-query = Query
settings-section-terminal = Terminale
settings-section-ai = Assistente IA
settings-section-appearance = Aspetto

## Settings › shared

settings-off = Disattivato

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } carattere
   *[other] { $n } caratteri
}

## Settings › General

settings-log = File di log
settings-log-hint = La diagnostica, compresi gli arresti anomali, viene scritta in { $path }.
settings-log-hint-none = Non è stato possibile aprire un file di log su questo computer, quindi non viene scritto nulla.
settings-log-open-folder = Apri cartella
settings-bundle = File delle impostazioni
settings-bundle-hint = Le tue preferenze e i tuoi snippet in un unico file, da portare su un altro computer. Connessioni, disposizione delle finestre, shell del terminale e snippet legati a una connessione restano qui.
settings-bundle-export = Esporta
settings-bundle-import = Importa
settings-cli = Riga di comando
settings-cli-install = Installa
settings-cli-installing = Installazione in corso
settings-cli-remove = Rimuovi
settings-cli-removing = Rimozione in corso

## Settings › modal

settings-title = Impostazioni
settings-restore-tabs = Ripristina le schede all'avvio
settings-restore-tabs-hint = Riapri le schede di query dell'ultima sessione all'avvio dell'app.

## Settings › Editor

settings-editor-font-size = Dimensione carattere
settings-editor-predictions = Suggerimenti in linea
settings-editor-predictions-hint = Mostra le parole successive probabili dopo il cursore — il complemento di una parola chiave, la chiave esterna di un join, le colonne da raggruppare o inserire. Tab, o → a fine riga, li accetta per intero, Ctrl+→ (⌘→ su macOS) una parola alla volta, ed Esc li rifiuta.
settings-editor-remembered = Suggerimenti dalle istruzioni passate
settings-editor-remembered-hint = Proponi anche il resto di una riga da un'istruzione che hai eseguito, scritto in un'altra scheda o salvato come snippet. Disattiva per tenere le istruzioni passate fuori dallo schermo.
settings-editor-aliases = Alias delle tabelle
settings-editor-aliases-hint = Quando il completamento scrive una tabella dopo FROM o JOIN, aggiungi un breve alias formato dalle sue iniziali — orders o, order_items oi — e usa l'alias nella condizione ON di un join.
settings-editor-rainbow = Parentesi arcobaleno
settings-editor-rainbow-hint = Colora ogni coppia di parentesi in base alla profondità di annidamento, così una parentesi di chiusura si abbina a colpo d'occhio a quella di apertura.
settings-editor-whitespace = Mostra spazi bianchi
settings-editor-whitespace-hint = Disegna spazi e tabulazioni come segni tenui, così una tabulazione si distingue dagli spazi.
settings-editor-guides = Guide di rientro
settings-editor-guides-hint = Disegna una linea verticale tenue a ogni livello di rientro.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Maiuscole delle parole chiave
settings-format-case-keep = Come digitato
settings-format-case-upper = Maiuscolo
settings-format-case-lower = Minuscolo
settings-format-commas-trailing = Fine riga
settings-format-commas-leading = Inizio riga
settings-format-gap-one = Una riga vuota
settings-format-gap-none = Nessuna riga vuota
settings-format-keyword-case-hint = Come Formatta codice scrive parole chiave quali SELECT e FROM. Nomi, stringhe e commenti mantengono le proprie maiuscole.
settings-format-function-case = Maiuscole delle funzioni
settings-format-function-case-hint = Come Formatta codice scrive le funzioni proprie del motore, quali COUNT e COALESCE, dove vengono chiamate. Le tue funzioni mantengono le proprie maiuscole.
settings-format-type-case = Maiuscole dei tipi
settings-format-type-case-hint = Come Formatta codice scrive i tipi di dati propri del motore, quali INT e VARCHAR. I tipi che hai creato mantengono le proprie maiuscole.
settings-format-commas = Virgole
settings-format-commas-hint = Dove Formatta codice mette la virgola di un elenco che spezza su più righe. Gli elenchi tenuti su una riga, come gli argomenti di una funzione, restano invariati a meno che la larghezza di a capo non li spezzi.
settings-format-statement-gap = Tra le istruzioni
settings-format-statement-gap-hint = Le righe vuote che Formatta codice lascia tra due istruzioni. Come digitato mantiene una riga vuota ovunque ce ne fosse una.
settings-format-wrap = Larghezza di a capo
settings-format-wrap-hint = Un elenco che Formatta codice tiene su una riga, come gli argomenti di una funzione, va a un elemento per riga quando la riga supererebbe questa larghezza.
settings-format-river = Allinea a destra le parole chiave delle clausole
settings-format-river-hint = Allinea SELECT, FROM, WHERE e le altre sul bordo destro, con il corpo di ogni clausola accanto alla sua parola chiave, invece di una parola chiave su una riga a sé.
settings-format-align = Allinea le colonne
settings-format-align-hint = Allinea gli AS degli alias di un elenco SELECT, gli = delle assegnazioni di un UPDATE e i tipi delle colonne di una tabella.
settings-format-join-on = Condizione di join su una riga a sé
settings-format-join-on-hint = Metti l'ON di un join, e gli AND e OR della sua condizione, su una riga a sé sotto il join.

## Settings › Query

settings-query-row-limit = Limite di righe predefinito
settings-query-timeout = Timeout dell'istruzione
settings-query-timeout-hint = Annulla un'istruzione che dura più di così. Disattivato per impostazione predefinita — un'importazione o un report possono legittimamente richiedere tempo.
timeout-off = Nessun timeout
timeout-hours = { $n ->
    [one] { $n } ora
   *[other] { $n } ore
}
timeout-minutes = { $n ->
    [one] { $n } minuto
   *[other] { $n } minuti
}
timeout-seconds = { $n ->
    [one] { $n } secondo
   *[other] { $n } secondi
}
settings-query-confirm-writes = Conferma prima di eseguire scritture
settings-query-confirm-writes-hint = Chiedi prima di eseguire qualsiasi istruzione che modifica dati o schema.
settings-query-validate = Convalida in tempo reale sul database
settings-query-validate-hint = Verifica l'istruzione sotto il cursore sul database mentre scrivi (un PREPARE che non esegue nulla) per mostrare gli errori esatti. Aggiunge un round trip verso il DB a ogni pausa.
settings-query-notify = Notifica quando un'esecuzione termina in background
settings-query-notify-hint = Mostra una notifica sul desktop quando una query, uno script, un'esportazione o un dump durati un secondo o più terminano mentre Schemaic non è la finestra attiva. Il pulsante sulla barra delle applicazioni viene evidenziato in ogni caso. La notifica non include mai l'SQL.
notify-query-finished = Query completata
notify-query-failed = Query non riuscita
notify-script-finished = Script completato
notify-script-failed = Script non riuscito
notify-export-finished = Esportazione completata
notify-export-failed = Esportazione non riuscita
notify-dump-finished = Dump completato
notify-dump-failed = Dump non riuscito
notify-took = Durata: { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Conserva i valori nel registro delle scritture
settings-query-write-log-hint = Ogni scrittura eseguita da Schemaic o da schemaic exec viene aggiunta a write_log.jsonl nella cartella delle impostazioni, e non viene mai cancellata. I valori nel suo SQL sono mascherati a meno che questa opzione sia attiva; una password è mascherata comunque.

## Settings › Appearance

settings-appearance-ui-theme = Tema dell'interfaccia
settings-appearance-editor-theme = Tema dell'editor
settings-appearance-scale = Scala dell'interfaccia

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Scorciatoie

## Shortcuts modal › group headings

shortcuts-group-global = Globali
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Griglia dei risultati
shortcuts-group-schema-tree = Albero dello schema
shortcuts-group-ai-panel = Pannello IA
shortcuts-group-terminal = Terminale
shortcuts-group-er-diagram = Diagramma ER

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Cerca ovunque
shortcuts-command-palette = Tavolozza dei comandi
shortcuts-new-tab = Nuova scheda di query
shortcuts-close-tab = Chiudi scheda di query
shortcuts-next-tab = Scheda successiva
shortcuts-previous-tab = Scheda precedente
shortcuts-jump-to-tab = Vai alla scheda
shortcuts-reopen-tab = Riapri l'ultima scheda chiusa
shortcuts-open-file = Apri file SQL
shortcuts-save-file = Salva file SQL
shortcuts-save-file-as = Salva file SQL con nome
shortcuts-toggle-schema = Mostra/nascondi pannello dello schema
shortcuts-toggle-ai = Mostra/nascondi pannello IA
shortcuts-toggle-terminal = Mostra/nascondi terminale
shortcuts-run-query = Esegui query
shortcuts-autocomplete = Completamento automatico
shortcuts-inline-ai = Modifica IA in linea
shortcuts-find-in-editor = Trova nell'editor
shortcuts-find-replace = Trova e sostituisci
shortcuts-go-to-line = Vai alla riga
shortcuts-matching-bracket = Vai alla parentesi o al BEGIN/END corrispondente
shortcuts-line-comment = Commenta/decommenta riga
shortcuts-block-comment = Commenta/decommenta blocco
shortcuts-toggle-case = Inverti maiuscole/minuscole
shortcuts-join-lines = Unisci righe
shortcuts-duplicate = Duplica riga / selezione
shortcuts-cut = Taglia riga / selezione
shortcuts-format-code = Formatta codice
shortcuts-format-statement = Formatta istruzione
shortcuts-single-line = Riduci a una riga
shortcuts-move-line = Sposta riga su / giù
shortcuts-quick-fix = Applica la correzione rapida sotto il cursore
shortcuts-indent = Aumenta / riduci rientro
shortcuts-take-prediction = Accetta il suggerimento in linea (anche →, a fine riga) / la sua parola successiva
shortcuts-find-in-results = Trova nei risultati
shortcuts-go-to-row = Vai alla riga
shortcuts-copy = Copia
shortcuts-paste-staged = Incolla come modifiche in sospeso
shortcuts-select-all = Seleziona tutto
shortcuts-first-last-cell = Prima / ultima cella
shortcuts-edit-cell = Modifica cella
shortcuts-next-cell = Cella successiva / precedente durante la modifica
shortcuts-commit-edits = Conferma le modifiche
shortcuts-mark-deletion = Contrassegna la riga per l'eliminazione
shortcuts-results-toolbar = Vai alla barra degli strumenti dei risultati
shortcuts-along-toolbar = Spostati lungo la barra degli strumenti (Esc torna alla griglia)
shortcuts-open-selected = Apri la tabella, la colonna o l'oggetto selezionato
shortcuts-context-menu = Menu contestuale della riga selezionata
shortcuts-send-message = Invia messaggio
shortcuts-new-line = Nuova riga
shortcuts-recall-prompt = Richiama il prompt precedente / successivo
shortcuts-copy-paste = Copia / incolla
shortcuts-find-in-diagram = Trova nel diagramma

## Key names in shortcut labels — a keyboard's own legends, so a language
## whose keyboards print another word uses it (German: Strg, Umschalt, Entf).
## Keys written the same everywhere (letters, arrows, Tab, Esc, F6) and macOS's
## Cmd are not here. Read key by key by `shortcuts::keys_label`.

key-ctrl = Ctrl
key-shift = Maiusc
key-alt = Alt
key-enter = Invio
key-del = Canc
key-home = Home
key-end = Fine
key-space = Spazio

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Mostra/nascondi pannello
palette-toggle-panel-hint = schema · ia · terminale · cronologia query · attività del server
palette-panel-schema = Schema
palette-panel-ai = IA
palette-panel-terminal = Terminale
palette-panel-history = Cronologia query
palette-panel-activity = Attività del server
palette-new-tab = Nuova scheda
palette-export-settings = Esporta impostazioni
palette-import-settings = Importa impostazioni
palette-duplicate-tab = Duplica scheda
palette-close-tab = Chiudi scheda
palette-reopen-closed-tab = Riapri scheda chiusa
palette-reopen-closed-tab-hint = <scheda>
palette-open-file = Apri file
palette-open-file-hint = uno script .sql
palette-save-file = Salva file
palette-save-file-as = Salva file con nome
palette-next-tab = Scheda successiva
palette-previous-tab = Scheda precedente
palette-format-code = Formatta codice
palette-format-statement = Formatta istruzione
palette-format-statement-hint = l'istruzione al cursore
palette-make-single-line = Riduci a una riga
palette-make-single-line-hint = la selezione o l'istruzione, su una riga
palette-run = Esegui
palette-run-hint = esegui tutte le istruzioni
palette-go-to-line = Vai alla riga
palette-go-to-line-hint = <numero>
palette-go-to-statement = Vai all'istruzione
palette-search-hint = <ricerca>
palette-history = Cronologia
palette-clear-history = Cancella cronologia
palette-clear-history-hint = connessione corrente
palette-ask-ai = Chiedi all'IA
palette-ask-ai-hint = <prompt>
palette-ask-ai-empty = Scrivi un prompt…
palette-terminal = Terminale
palette-terminal-hint = <comando>
palette-run-in-terminal = Esegui nel terminale
palette-terminal-empty = Scrivi un comando…
palette-ui-theme = Tema interfaccia
palette-ui-theme-hint = chiaro · scuro
palette-editor-theme = Tema editor
palette-editor-theme-hint = <tema>
palette-font-size = Dimensione carattere
palette-increase-font-size = Aumenta dimensione carattere
palette-decrease-font-size = Riduci dimensione carattere
palette-indent-style = Stile di rientro
palette-indent-style-hint = tabulazioni · spazi
palette-indent-spaces = Spazi
palette-indent-tabs = Tabulazioni
palette-indent-width = Larghezza rientro
palette-toggle-word-wrap = Attiva/disattiva a capo automatico
palette-toggle-whitespace = Mostra/nascondi spazi bianchi
palette-toggle-whitespace-hint = mostra spazi e tabulazioni
palette-toggle-indent-guides = Mostra/nascondi guide di rientro
palette-switch-connection = Cambia connessione
palette-switch-connection-hint = <connessione>
palette-server-variables = Variabili del server
palette-server-variables-hint = le impostazioni della connessione attiva
palette-enter-number = Inserisci un numero
palette-nothing-found = Nessun risultato

## Command palette › what a result row is

palette-row-snippet = snippet
palette-row-open-tab = scheda aperta
palette-row-history = cronologia
palette-row-view-body = corpo della vista
palette-row-body = corpo di { $kind }
palette-row-trigger-on = trigger su { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Apri nella CLI
tree-open = Apri
tree-open-in-new-tab = Apri in una nuova scheda
tree-copy-name = Copia nome
tree-copy-qualified-name = Copia nome qualificato
tree-properties = Proprietà
tree-used-by = Usato da
tree-live-monitor = Monitoraggio in tempo reale
tree-er-diagram = Diagramma ER
tree-compare-with = Confronta con
tree-generate = Genera
tree-generate-ddl = Genera DDL
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Aggiorna
tree-collapse-all = Comprimi tutto
tree-favourite = Aggiungi ai preferiti
tree-unfavourite = Rimuovi dai preferiti
tree-colour = Colore
tree-colour-none = Nessuno
tree-hide = Nascondi
tree-import = Importa
tree-export = Esporta
tree-back-up = Esegui backup
tree-restore-from-file = Ripristina da file
tree-copy-to = Copia in
tree-create = Crea
tree-create-table = Tabella
tree-create-view = Vista
tree-create-type = Tipo
tree-create-domain = Dominio
tree-create-sequence = Sequenza
tree-create-synonym = Sinonimo
tree-create-alias-type = Tipo alias
tree-create-xml-schema-collection = Raccolta di schemi XML
tree-create-function = Funzione
tree-create-procedure = Procedura
tree-create-event = Evento
tree-create-extension = Estensione
tree-create-database = Database
tree-create-schema = Schema
tree-create-kind = Crea { $kind }
tree-edit-kind = Modifica { $kind }
tree-folder-create-extension = Crea estensione
tree-edit-table = Modifica tabella
tree-edit-view = Modifica vista
tree-edit-column = Modifica colonna
tree-edit-index = Modifica indice
tree-edit-primary-key = Modifica chiave primaria
tree-edit-foreign-key = Modifica chiave esterna
tree-triggers = Trigger
tree-triggers-count = Trigger ({ $n })
tree-refresh-view = Aggiorna vista
tree-truncate = Svuota
tree-truncate-title = Svuota tabella
tree-truncate-rows = { $n ->
    [one] Eliminare la riga { $rows } di { $table }? L'operazione non si può annullare.
   *[other] Eliminare tutte le { $rows } righe di { $table }? L'operazione non si può annullare.
}
tree-truncate-every-row = Eliminare ogni riga di { $table }? L'operazione non si può annullare.
tree-drop = Elimina
tree-drop-index = Elimina indice
tree-drop-foreign-key = Elimina chiave esterna
tree-ai-explain = Spiegazione IA
tree-users = Utenti e privilegi
tree-server-variables = Variabili del server
tree-new-database = Crea database
tree-show-table-sizes = Mostra dimensioni delle tabelle

## Connection switcher

conn-menu-manage = Gestisci connessioni

## Shared dialog buttons

dialog-cancel = Annulla
dialog-yes = Sì
dialog-no = No

## Open-transaction prompt

tx-prompt-title = Transazione aperta
tx-prompt-body = { $tab } ha una transazione aperta con { $n ->
    [one] 1 istruzione
   *[other] { $n } istruzioni
}. Puoi eseguire il commit per mantenere le modifiche o il rollback per scartarle.
tx-prompt-body-aborted = { $tab } ha una transazione interrotta da un'istruzione non riuscita, con { $n ->
    [one] 1 istruzione
   *[other] { $n } istruzioni
}. Può solo essere annullata con un rollback, che scarta le modifiche.
tx-prompt-rollback = Rollback
tx-prompt-commit = Commit

## Error modal

error-modal-title = Errore
error-modal-none = Nessun errore.
error-modal-ai-fix = Correzione IA
error-modal-explain = Spiega

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = query non riuscita: { $error }
db-connection-failed = connessione non riuscita: { $error }
db-query-cancelled = query annullata

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Non connesso — l'SQL di inizializzazione di questa connessione è stato rifiutato: { $why }
db-refused-transport-entra = L'accesso con Microsoft Entra richiede SSL / TLS in modalità Verify full: il suo token apre ogni database Azure SQL raggiungibile dall'account, quindi il server deve dimostrare il proprio nome prima di riceverne uno.
db-refused-transport-aws = L'accesso con AWS IAM richiede SSL / TLS almeno in modalità Obbligatorio: il suo token è una password per questo server e non deve attraversare la rete in chiaro.
db-refused-transport-gcp = L'accesso con Google Cloud IAM richiede SSL / TLS in modalità Verify CA, con la CA del server dell'istanza come CA radice: il suo token apre ogni istanza Cloud SQL raggiungibile dall'account.
db-refused-transport = Questo accesso richiede una modalità SSL / TLS più rigorosa di quella della connessione.
db-refused-backup-in-terminal = Il backup di un database server si esegue con i suoi strumenti, nel terminale.
db-refused-commit-aborted = la transazione era già interrotta — un'istruzione al suo interno non è riuscita o è stata fermata, quindi il commit scarterebbe il lavoro invece di salvarlo. Non è stato confermato nulla.
db-refused-sqlite-no-manual = Le connessioni SQLite non supportano ancora la modalità di transazione Manuale — le istruzioni vengono eseguite e confermate man mano che vengono inviate
db-refused-mssql-read-only-auto = Una connessione SQL Server in sola lettura funziona in modalità Automatica: SQL Server non ha una transazione in sola lettura in cui tenere una scheda Manuale, e ogni istruzione eseguita in modalità Automatica viene annullata con un rollback
db-refused-client-charset = rifiutato: il set di caratteri client della sessione ({ $charset }) può nascondere un apice all'interno di un carattere, quindi il controllo dell'istruzione non può leggerla come farebbe il server
db-refused-sql-mode = rifiutato: lo sql_mode del server ({ $mode }) interpreta gli apici diversamente dal controllo dell'istruzione, e non è stato possibile modificarlo per questa sessione
db-refused-writing-plan = Qui i piani vengono eseguiti in una transazione in sola lettura, e MySQL e MariaDB non pianificano al suo interno un'istruzione che scrive. Pianifica invece la forma SELECT dell'istruzione.
db-refused-writing-plan-function = Qui i piani vengono eseguiti in una transazione in sola lettura, e MySQL e MariaDB non pianificano al suo interno un'istruzione che scrive. Una funzione che chiama scrive, cosa che qui un piano non può fare.
db-refused-windows-build-only = L'accesso con Windows è disponibile solo nella build Windows di Schemaic.
db-refused-cloud-iam-engine = L'accesso con Cloud IAM è per le connessioni PostgreSQL e MySQL.
db-refused-mssql-cancel-needs-kill = SQL Server non può annullare l'istruzione di un'altra sessione senza terminare la sessione — usa Termina sessione.
db-refused-blank-value = { $has_row ->
        [some] La riga { $row } ha un valore vuoto o di soli spazi per { $column } ({ $sql_type }): SQL Server lo memorizzerebbe come 0 o 1900-01-01 invece di rifiutarlo. Importa i campi vuoti come NULL, oppure compilali.
       *[none] Un valore vuoto o di soli spazi non può essere scritto in { $column } ({ $sql_type }): SQL Server lo memorizzerebbe come 0 o 1900-01-01 invece di rifiutarlo. Imposta la cella a NULL, oppure digita un valore.
    }
db-refused-code-page = { $has_row ->
        [some] La riga { $row } ha un valore per { $column } ({ $sql_type }) che
       *[none] Un valore per { $column } ({ $sql_type })
    } contiene { $has_lost ->
        [some] `{ $character }`, per cui la code page { $code_page } non ha alcun carattere
       *[none] un carattere che la sua code page non possiede
    }: SQL Server lo memorizzerebbe come `?` o come un carattere simile invece di rifiutarlo. Memorizza la colonna come nvarchar, oppure rimuovi il carattere.
db-refused-sqlite-no-file = non è impostato alcun file di database
db-refused-sqlite-memory = :memory: non è un file di database — SQLite conserva un database in memoria solo finché resta aperta la connessione che l'ha creato, e questa app apre una nuova connessione per ogni operazione, quindi ogni scrittura verrebbe segnalata come salvata e poi scartata. Indica il percorso di un file.
db-refused-sqlite-uri = { $file }: questo è un URI, non un file di database — SQLite leggerebbe la parte dopo `?` come opzioni di apertura, e `?mode=memory` apre un database temporaneo che accetta ogni scrittura e non ne conserva nessuna. Indica il percorso del file stesso.
db-refused-sqlite-worker = il worker SQLite non è riuscito: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } non è una colonna di questa tabella, quindi non ha un valore predefinito da scrivere
db-refused-sqlite-generated = { $column } è una colonna generata: non ha un valore predefinito a cui riportarla, e SQLite la calcola dalla riga
db-refused-sqlite-sizes-too-large = Le dimensioni delle tabelle vengono lette solo per un database fino a { $cap }: SQLite le misura leggendo ogni pagina del file, e questo è di { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = tempo scaduto
db-connect-init-failed = l'SQL di inizializzazione `{ $statement }` non è riuscito: { $error }
db-connect-session-retired = la connessione di questa scheda è stata chiusa dopo un Ferma che il server non ha confermato, e non è stato possibile aprirne una nuova; la sua transazione è stata annullata con un rollback. Passa la scheda a Automatico e di nuovo a Manuale per aprirne un'altra.
db-connect-begin-failed = impossibile avviare la transazione: { $error }
db-connect-prepare-session = impossibile preparare la sessione: { $error }
db-connect-read-only-setup = impossibile rendere la sessione di sola lettura: { $error }
db-connect-guard-session = impossibile proteggere la sessione: { $error }
db-connect-plan-setup = impossibile preparare il piano: { $error }
db-connect-no-maintenance-db = nessun database di manutenzione raggiungibile
db-connect-browser-silent = SQL Server Browser su { $server } non ha fornito alcuna porta per l'istanza { $instance }: il servizio Browser non è in esecuzione, la porta UDP 1434 è bloccata lungo il percorso, oppure nessuna istanza lì ha quel nome. Se l'istanza è in ascolto su una porta fissa, imposta l'host a { $server } senza \{ $instance } e la porta a quella.
db-connect-browser-without-tcp = SQL Server Browser su { $server } ha risposto per l'istanza { $instance } ma non ha fornito alcuna porta TCP: l'istanza non è in ascolto su TCP. Abilita TCP/IP per l'istanza in SQL Server Configuration Manager e riavviala.
db-connect-azure-timed-out = Azure SQL non ha completato l'accesso entro { $secs } secondi. Un database serverless in pausa impiega circa un minuto per riprendere, e questo tentativo l'ha risvegliato — riconnettiti tra poco.
db-connect-azure-unavailable = { $server } Un database serverless in pausa impiega circa un minuto per riprendere, e questo tentativo l'ha risvegliato — riconnettiti tra poco.
db-ssh-connect-no-answer = Connessione SSH con { $host }: nessuna risposta entro { $secs } s
db-ssh-auth-no-answer = Autenticazione SSH con { $host }: nessuna risposta entro { $secs } s
db-ssh-auth-error = Errore di autenticazione SSH: { $error }
db-ssh-auth-failed = Autenticazione SSH non riuscita
db-ssh-key-load = Caricamento della chiave SSH non riuscito: { $error }
db-ssh-key-auth = Errore di autenticazione con la chiave SSH: { $error }
db-ssh-agent = Errore dell'agente SSH: { $error }
db-ssh-agent-empty = L'agente SSH non ha identità caricate (esegui `ssh-add`)
db-ssh-agent-auth = Errore di autenticazione dell'agente SSH: { $error }
db-ssh-no-agent = nessun agente SSH trovato (avvia il servizio OpenSSH Authentication Agent, oppure esegui Pageant)
db-ssh-no-agent-socket = nessun agente SSH ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = L'host di salto SSH non ha potuto raggiungere { $host }: { $error }
db-ssh-connect-to = Connessione SSH a { $host } non riuscita: { $error }
db-ssh-connect = Connessione SSH non riuscita: { $error }
db-ssh-no-host = Il tunnel SSH non ha un host
db-ssh-bind = binding del tunnel locale non riuscito: { $error }
db-ssh-named-instance = Un'istanza denominata non è raggiungibile tramite un tunnel SSH: SQL Server Browser risponde con la sua porta via UDP, che il tunnel non trasporta. Imposta l'host a { $server } senza \{ $instance } e la porta a quella su cui l'istanza è in ascolto.
db-ssh-host-key-changed = La chiave host SSH di { $host } è CAMBIATA da quando Schemaic l'ha considerata attendibile la prima volta.
db-ssh-host-key-expected = Attesa  { $key }
db-ssh-host-key-offered = Offerta { $key }
db-ssh-host-key-none = (nessuna chiave registrata)
db-ssh-host-key-changed-advice = Può significare che il server è stato ricostruito o la sua chiave è stata ruotata — oppure che qualcosa sta intercettando la connessione. Non gli è stato inviato nulla. Verifica la nuova impronta digitale per un canale separato (sulla console del server stesso, o con chi lo amministra) prima di considerarla attendibile. Fatto ciò, rimuovi la voce di questo host da { $store } nella cartella di configurazione di Schemaic per registrare la nuova chiave.
db-ssh-host-key-store = Schemaic non ha potuto leggere il proprio registro delle chiavi host SSH attendibili ({ $store }), quindi non può stabilire se la chiave di { $host } ({ $key }) sia quella che avevi già considerato attendibile.
db-ssh-host-key-store-advice = La connessione è stata rifiutata invece di accettare la chiave a scatola chiusa. Ripara o elimina { $store } nella cartella di configurazione di Schemaic — eliminarlo significa che ogni host SSH verrà considerato attendibile da capo alla connessione successiva, quindi fallo solo se puoi verificare le impronte digitali.
db-tls-setup = Configurazione TLS non riuscita: { $error }
db-tls-client-cert = certificato client rifiutato: { $error }
db-tls-trust-store = l'archivio di attendibilità TLS non è utilizzabile: { $error }
db-tls-bad-name = { $name } non è un nome che un certificato può contenere: { $error }
db-tls-ca-empty = il file CA { $path } non contiene certificati
db-tls-ca-unusable = il file CA { $path } contiene un certificato che non può essere usato come radice di attendibilità
db-tls-cert-unreadable = impossibile leggere il file del certificato { $path }: { $error }
db-tls-cert-not-pem = { $path } non è un file di certificato PEM: { $error }
db-tls-cert-is-der = { $path } non contiene alcun certificato PEM — un file DER va prima convertito (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = impossibile leggere il file della chiave { $path }: { $error }
db-tls-key-encrypted = { $path } è una chiave privata protetta da passphrase, che Schemaic non può aprire. Decrittografala prima: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } non è una chiave privata PEM: { $error }
db-sign-in-no-value = { $field ->
        [host] La connessione non ha un host con cui accedere.
       *[user] La connessione non ha un utente con cui accedere.
    }
db-sign-in-leading-dash = { $field ->
        [host] L'host della connessione inizia con «-», che la CLI cloud leggerebbe come un'opzione anziché come l'host.
       *[user] L'utente della connessione inizia con «-», che la CLI cloud leggerebbe come un'opzione anziché come l'utente.
    }
db-sign-in-control = { $field ->
        [host] L'host della connessione contiene un carattere di controllo.
       *[user] L'utente della connessione contiene un carattere di controllo.
    }
db-sign-in-shim-aws = L'AWS CLI trovata è uno shim .cmd (la versione 1 installata con pip), e Schemaic non esegue uno shim tramite cmd.exe. Installa la versione 2 dell'AWS CLI, oppure accedi con una password.
db-sign-in-shim-gcloud = Il gcloud trovato è uno shim .cmd senza un Python incluso accanto, e Schemaic non esegue uno shim tramite cmd.exe. Reinstalla la Google Cloud CLI con il suo programma di installazione, oppure accedi con una password.
db-sign-in-shim-azure = L'Azure CLI trovata è uno shim .cmd/.bat senza Python accanto, e Schemaic non esegue uno shim tramite cmd.exe. Reinstalla l'Azure CLI con il programma di installazione di Microsoft, oppure accedi con un login SQL.
db-sign-in-no-token = La CLI cloud ha risposto senza alcun token.
db-sign-in-more-than-token = La CLI cloud ha risposto con più di un token.
db-sign-in-azure-bad-answer = La risposta dell'Azure CLI non era il token che avrebbe dovuto stampare.
db-sign-in-azure-no-access-token = L'Azure CLI ha risposto senza un token di accesso.
db-sign-in-signed-out-aws = L'AWS CLI non ha credenziali. Esegui `aws configure` o `aws sso login`, poi riconnettiti.
db-sign-in-signed-out-gcloud = La Google Cloud CLI non ha eseguito l'accesso. Esegui `gcloud auth login`, poi riconnettiti.
db-sign-in-signed-out-azure = L'Azure CLI non ha eseguito l'accesso. Esegui `az login`, poi riconnettiti.
db-sign-in-no-token-from = { $cli } non è riuscita a ottenere un token: { $line }
db-sign-in-no-token-from-silent = { $cli } non è riuscita a ottenere un token.
db-sign-in-no-token-from-azure-silent = L'Azure CLI non è riuscita a ottenere un token: non ha indicato alcun motivo
db-sign-in-not-found = { $cli } non è stata trovata. Installala e accedi con essa, oppure accedi con una password.
db-sign-in-not-found-azure = L'Azure CLI (az) non è stata trovata. Installala ed esegui `az login`, oppure accedi con un login SQL.
db-sign-in-no-answer = { $cli } non ha risposto entro { $secs } secondi.
db-sign-in-not-started = Impossibile avviare { $cli }: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = l'esportazione ha smesso di leggere
db-query-no-rows-to-export = quell'istruzione non restituisce righe da esportare
db-query-no-server-settings = Questo motore non ha impostazioni del server da mostrare.
db-query-no-status-list = Questo motore non pubblica un elenco di stato — solo le sue variabili.
db-query-no-import-columns = Nessuna colonna in cui importare
db-query-read-stopped = La lettura si è interrotta: { $error }
db-query-batch-continued = SQL Server prosegue un batch oltre l'errore di un'istruzione a meno che l'errore non termini il batch, quindi le istruzioni successive a quella non riuscita potrebbero essere state eseguite.
db-query-sqlite-locked = Su SQLite una lettura lunga blocca una scrittura: un'esportazione o un'importazione di un'intera tabella mantiene un lock di lettura sul file finché non termina, e nessuna scrittura può iniziare prima (MySQL e PostgreSQL non funzionano così). Attendi o annullala, poi riprova. Per consentire in modo permanente letture e scritture sovrapposte su questo file, passalo alla modalità di journal WAL — `PRAGMA journal_mode = WAL` — che modifica il file stesso, quindi Schemaic non lo farà al posto tuo.
db-query-rollback-failed-too = { $error } — e anche il rollback non è riuscito: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = istruzione { $n } non riuscita: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } istruzione precedente è già stata applicata e non può essere annullata
   *[other] { $failed } — { $n } istruzioni precedenti sono già state applicate e non possono essere annullate
}
ddl-cancelled = il piano è stato annullato
ddl-dangling-foreign-key = il piano lascia una chiave esterna che non punta a nulla: { $row }
ddl-breaks-view = il piano romperebbe la vista { $view }, che legge una colonna che esso rinomina o elimina ({ $why }). Non è stato modificato nulla. Modifica o elimina prima la vista.
ddl-no-databases = SQLite non ha database da creare o eliminare — lì un database è un file, che Schemaic non crea né elimina al posto tuo.
ddl-elsewhere-failed = Le viste in altri database che eseguono select * da { $table } non sono state aggiornate ({ $error }), e leggono e scrivono le colonne sbagliate finché non lo saranno (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] Non è stato possibile aggiornare { $count } vista in un altro database che esegue select * da { $table }; legge e scrive le colonne sbagliate finché non lo sarà (sp_refreshview): { $views }.
       *[other] Non è stato possibile aggiornare { $count } viste in un altro database che eseguono select * da { $table }; leggono e scrivono le colonne sbagliate finché non lo saranno (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] In { $count } database non è stata cercata una vista che esegue select * da { $table }: { $databases }.
       *[other] In { $count } database non è stata cercata una vista che esegue select * da { $table }: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , e altri { $more }
    }
ddl-rollback-failed = { $error } — e anche il rollback non è riuscito: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = l'eliminazione su { $table } ha interessato { $affected } righe (prevista esattamente 1)
write-one-row-update = l'aggiornamento su { $table } ha interessato { $affected } righe (prevista esattamente 1)
write-one-row-insert = l'inserimento in { $table } ha interessato { $affected } righe (prevista esattamente 1)
write-batch-inserted = un batch di { $rows } righe ne ha inserite { $wrote }
write-batch-affected = un batch di { $rows } righe ne ha interessate { $wrote }
write-row-issue = riga { $line }, colonna { $column }: { $problem } ({ $text })
write-row-issue-empty = riga { $line }, colonna { $column }: { $problem } (vuoto)
write-copy-too-long = { $column } contiene più testo di quanto una lettura possa trasportare, quindi la copia si è fermata prima di scriverlo
write-copy-too-long-unnamed = una colonna contiene più testo di quanto una lettura possa trasportare, quindi la copia si è fermata prima di scriverlo
write-copy-bytes = { $column } contiene byte che la copia non può leggere così come sono, quindi si è fermata prima di scriverli
write-copy-not-its-form = il valore letto per { $column } non è nella forma richiesta dalla copia, quindi non è stato scritto nulla
write-copy-not-its-form-unnamed = il valore letto per una colonna non è nella forma richiesta dalla copia, quindi non è stato scritto nulla
write-commit-cancelled = Commit annullato
write-import-cancelled = Importazione annullata
write-rollback-complete = {" "}— tutte le modifiche sono state annullate con un rollback
write-rollback-incomplete = {" "}— il rollback NON le ha annullate: il motore di archiviazione di questa tabella non è transazionale, quindi le righe già scritte restano. Controlla la tabella prima di riprovare.
write-rollback-unknown = {" "}— e il rollback non ha potuto essere confermato: la connessione è caduta prima della risposta del server. Le righe potrebbero essere rimaste o meno. Controlla la tabella prima di riprovare.

## Schema tree › the panel

tree-panel-title = SCHEMA
tree-tip-databases = Mostra o nascondi i database
tree-tip-options = Opzioni dello schema
tree-search-placeholder = Cerca…
tree-loading = Caricamento
tree-no-tables = Nessuna tabella
tree-failed-to-connect = Connessione non riuscita
tree-failed-view = Visualizza
tree-single-user-unread = Utente singolo — non letto
tree-single-user-read = Leggi
tree-count-tables = { $n ->
    [one] { $n } tabella
   *[other] { $n } tabelle
}
tree-count-cols = { $n ->
    [one] { $n } col.
   *[other] { $n } col.
}
tree-count-keys = { $n ->
    [one] { $n } chiave
   *[other] { $n } chiavi
}
tree-count-partitions = { $n ->
    [one] { $n } partizione
   *[other] { $n } partizioni
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = tipo
kind-domain = dominio
kind-sequence = sequenza
kind-function = funzione
kind-procedure = procedura
kind-event = evento
kind-synonym = sinonimo
kind-alias-type = tipo alias
kind-xml-schema-collection = raccolta di schemi XML
kind-extension = estensione
kind-group-type = Tipi
kind-group-domain = Domini
kind-group-sequence = Sequenze
kind-group-function = Funzioni
kind-group-procedure = Procedure
kind-group-event = Eventi
kind-group-synonym = Sinonimi
kind-group-alias-type = Tipi alias
kind-group-xml-schema-collection = Raccolte di schemi XML
kind-group-extension = Estensioni

## Query history panel

history-title = CRONOLOGIA QUERY
history-clear-tip = Cancella la cronologia di questa connessione…
history-clear-title = Cancella cronologia query
history-clear-message = Eliminare { $n ->
    [one] { $n } query registrata
   *[other] { $n } query registrate
} per questa connessione? L'operazione non si può annullare.
history-scope-current = Corrente
history-scope-all = Tutte
history-cut-note = Vengono mostrate le { $shown } più recenti su { $matched } — cerca per restringere.
history-empty = Ancora nessuna query.
history-no-match = Nessuna query corrispondente.
history-open-in-new-tab = Apri in una nuova scheda
history-delete = Elimina
history-failed = Non riuscita

## Snippet library panel

snippets-title = LIBRERIA DI SNIPPET
snippets-save-tip = Salva l'SQL dell'editor come snippet
snippets-no-match = Nessuno snippet corrispondente.
snippets-band-this-connection = QUESTA CONNESSIONE
snippets-band-all-connections = TUTTE LE CONNESSIONI
snippets-builtin = Predefinito
snippets-name-placeholder = Nome dello snippet
snippets-insert = Inserisci nell'editor
snippets-open-in-new-tab = Apri in una nuova scheda
snippets-edit = Modifica
snippets-duplicate = Duplica
snippets-delete = Elimina
snippets-show-in = Mostra in
snippets-scope-this-connection = Questa connessione
snippets-scope-engine = Ogni connessione { $engine }
snippets-scope-all-connections = Tutte le connessioni

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = OGGI
history-band-this-week = QUESTA SETTIMANA
history-band-earlier = PRIMA

## Query tab strip

tabs-tip-unsaved = { $path } — modifiche non salvate
tabs-pin = Fissa
tabs-unpin = Sblocca
tabs-rename = Rinomina
tabs-duplicate = Duplica
tabs-close = Chiudi
tabs-open-file = Apri file
tabs-save = Salva
tabs-save-as = Salva con nome
tabs-reload = Ricarica dal disco
tabs-reopen-last = Riapri l'ultima scheda
tabs-close-others = Chiudi le altre schede
tabs-close-all = Chiudi tutte le schede

## Server Activity panel

activity-title = ATTIVITÀ DEL SERVER
activity-refresh-tip = Aggiorna ora
activity-auto-refresh-off = Aggiornamento automatico disattivato
activity-auto-refresh-every = Aggiornamento automatico ogni { $secs } s
activity-interval-off = Disattivato
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } h { $m } min
activity-age-days = { $d } g { $h } h
activity-view-sessions = Sessioni
activity-view-locks = Lock
activity-search-sessions = Cerca sessioni…
activity-search-locks = Cerca lock…
activity-loading = Caricamento…
activity-sqlite-no-sessions = SQLite non ha sessioni server — viene eseguito all'interno di questo processo, quindi non c'è nient'altro di connesso da vedere.
activity-sessions-no-match = Nessuna sessione corrispondente.
activity-sessions-none = Nessun'altra sessione su questo server.
activity-sessions-undrawn = { $n ->
    [one] Corrisponde ancora { $n } sessione — restringi la ricerca per vederla.
   *[other] Corrispondono altre { $n } sessioni — restringi la ricerca per vederle.
}
activity-locks-no-match = Nessun lock corrispondente.
activity-locks-no-waits = Nessuna attesa di lock su questo server.
activity-locks-none = Nessun'altra sessione detiene un lock.
activity-locks-contended-only = Questo server elenca solo i lock attesi, o che trattengono un'attesa — non tutti i lock di una sessione.
activity-locks-undrawn = { $n ->
    [one] Corrisponde ancora { $n } lock — restringi la ricerca per vederlo.
   *[other] Corrispondono altri { $n } lock — restringi la ricerca per vederli.
}
activity-list-capped = elenco limitato
activity-locks-total = { $n ->
    [one] { $shown } lock
   *[other] { $shown } lock
}
activity-locks-waiting = { $n ->
    [one] { $shown } in attesa
   *[other] { $shown } in attesa
}
activity-state-blocked = Bloccata
activity-state-idle-in-tx = Inattiva in transazione
activity-state-running = In esecuzione
activity-state-idle = Inattiva
activity-count-sessions = { $n ->
    [one] { $figure } sessione
   *[other] { $figure } sessioni
}
activity-count-running = { $figure } in esecuzione
activity-count-blocked = { $figure } bloccate
activity-count-idle-in-tx = { $figure } inattive in transazione
activity-lock-wait = Attesa di lock
activity-lock-wait-sentence = La sessione { $waiter } ({ $waiter_who }) { $waited ->
        [none] attende
       *[other] attende da { $waited }
    } un lock detenuto dalla sessione { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] inattiva in una transazione
        [blocked] a sua volta bloccata
        [running] in esecuzione
       *[idle] inattiva
    }{ $held ->
        [none] {""}
       *[other] {" "}da { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}Anche { $others } altra sessione detiene un lock che le serve.
       *[other] {" "}Anche altre { $others } sessioni detengono un lock che le serve.
    }
activity-kill-holder = Termina { $id }
activity-terminate-tip = Termina la sessione { $id }
activity-read-only-tip = Questa connessione è contrassegnata come di sola lettura.
activity-lock-holds = detiene
activity-lock-waits-for = attende
activity-lock-held-by = detenuto da { $ids }
activity-copy-statement = Copia istruzione
activity-cancel-query = Annulla query
activity-kill-session = Termina sessione
activity-waiting-on = in attesa di un lock detenuto da { $ids }
activity-blocks = { $n ->
        [one] blocca { $n } sessione
       *[other] blocca { $n } sessioni
    }
activity-read-only-cancel = Questa connessione è contrassegnata come di sola lettura, quindi Schemaic non vi annullerà query. Disattiva prima la sola lettura nelle impostazioni della connessione.
activity-read-only-terminate = Questa connessione è contrassegnata come di sola lettura, quindi Schemaic non vi terminerà sessioni. Disattiva prima la sola lettura nelle impostazioni della connessione.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ASSISTENTE IA
ai-new-chat-tip = Nuova chat
ai-settings-tip = Impostazioni IA
ai-empty-ask = Fai una domanda sul tuo SQL…
ai-empty-not-connected = { $cli } non connesso.
ai-input-placeholder = Messaggio…
ai-input-off = L'assistente IA è disattivato
ai-input-db-down = Non connesso al database
ai-speaker-you = TU
ai-result-more-lines = … (+{ $n } altre righe)

## Shared dialog buttons (continued)

dialog-close = Chiudi

## Chart modal

chart-title = Grafico — { $tab }
chart-caption-kind = Grafico
chart-caption-values = Valori
chart-caption-labels = Etichette
chart-row-number = Numero di riga
chart-kind-bar = Barre
chart-kind-line = Linee
chart-kind-histogram = Istogramma
chart-save-tip = Salva il grafico…
chart-save-dialog = Salva grafico
chart-file-stem = Grafico { $name }
chart-nothing-to-save = Niente da salvare.
chart-no-numbers = In questo risultato non c'è alcuna colonna numerica da rappresentare.
chart-all-null = Ogni valore di quella colonna è NULL.
chart-too-large = Questi valori sono troppo grandi da disegnare.
chart-caption-rows = { $n ->
    [one] { $n } riga
   *[other] { $n } righe
}
chart-caption-skipped-null = { $skipped } NULL, non disegnati
chart-caption-skipped = { $skipped } non disegnati (NULL, o oltre { $max })
chart-caption-truncated = le righe lette, non l'intero risultato

## Schema editors — shared

editor-preview-sql = Anteprima SQL
editor-change-count = { $n ->
    [0] Nessuna modifica
    [one] 1 modifica
   *[other] { $n } modifiche
}

## View editor

view-editor-title-edit = Modifica vista { $view }
view-problem-no-name = La vista necessita di un nome.
view-problem-no-select = Una vista necessita di una SELECT che la definisca.
view-problem-not-a-query = Il corpo di una vista deve essere una query — inizia con SELECT, WITH, VALUES o TABLE.
view-problem-materialized = Schemaic non può modificare una vista materializzata — PostgreSQL non ha CREATE OR REPLACE per essa.
view-problem-hidden = La definizione di questa vista è crittografata o nascosta a questo login, quindi qui può essere rimossa ma non modificata.
view-problem-verbatim = L'intestazione di questa vista contiene qualcosa che Schemaic non sa riformulare, quindi qui può essere rimossa ma non modificata.
view-editor-title-create = Crea vista in { $location }
view-editor-section-view = Vista
view-editor-section-definition = Definizione
view-editor-section-options = Opzioni
view-editor-name = Nome
view-editor-select = Istruzione SELECT
view-editor-check-option = Check option
view-editor-column-names = Nomi delle colonne
view-editor-column-names-placeholder = facoltativo — presi dalla SELECT se vuoto
view-editor-sql-security = SQL security
view-editor-definer = Definer
view-editor-recreate = Ricrea invece di sostituire
view-editor-recreate-hint = PostgreSQL può solo aggiungere colonne a una vista esistente. Rinominarne, cambiarne il tipo o riordinarne una richiede un DROP e un CREATE, che si porta via le viste dipendenti e i permessi.

## Table properties panel

props-title = Proprietà — { $table }
props-reading = Lettura delle statistiche
props-no-stats-view = Una vista non ha un'archiviazione propria, quindi il server non riporta statistiche per essa.
props-no-stats-table = Il server non ha riportato statistiche per questa tabella.
props-on-disk = su disco
props-rows-estimated = righe (stimate)
props-rows-counted = righe (contate)
props-rows-not-reported = righe — non riportate
props-counting = Conteggio in corso
props-count-rows = Conta righe
props-count-slow = Una scansione completa della tabella. Lenta su una tabella grande.
props-storage-data = Dati
props-storage-indexes = Indici
props-storage-free = Libero
props-section-structure = Struttura
props-columns = Colonne
props-indexes = Indici
props-foreign-keys = Chiavi esterne
props-count-columns = { $n ->
    [one] { $n } colonna
   *[other] { $n } colonne
}
props-count-indexes = { $n ->
    [one] { $n } indice
   *[other] { $n } indici
}
props-count-keys = { $n ->
    [one] { $n } chiave
   *[other] { $n } chiavi
}
props-type = Tipo
props-type-view = Vista
props-type-table = Tabella
props-section-options = Opzioni
props-engine = Motore
props-row-format = Formato delle righe
props-collation = Collation
props-next-auto-increment = Prossimo auto-increment
props-created = Creata
props-updated = Aggiornata
props-comment = Commento
props-section-partitioning = Partizionamento
props-partition-of = Partizione di
props-partition-bound = Limite
props-partitioned-by = Partizionata per
props-partitions-more = { $n ->
    [one] e { $n } altra partizione
   *[other] e altre { $n } partizioni
}
props-section-indexes = Indici
props-index-cardinality = cardinalità { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } scansione
       *[other] { $shown } scansioni
    }
props-index-not-counted = utilizzo non conteggiato
props-index-unused = mai usato dall'ultimo azzeramento dei contatori del server
props-never-analysed = Questa tabella non è mai stata analizzata, quindi il server non ha una stima delle righe. Esegui ANALYZE, oppure conta le righe.
props-analysed-at = Stima all'ultimo ANALYZE, { $when }.
props-read-live = Letto in tempo reale — questo server ha information_schema_stats_expiry = 0.
props-cached-up-to = Nella cache del server: questi valori possono risalire fino a { $age } fa (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } h
props-age-days = { $n } g
props-vacuum-note = Il { $pct }% delle versioni di riga di questa tabella è morto e non ancora recuperato. Un VACUUM restituirebbe quello spazio.
props-copy = Copia

## Shared panel search box

panel-search-placeholder = Cerca…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = proprio ora
ago-minutes = { $n } min fa
ago-hours = { $n } h fa
ago-days = { $n } g fa
ago-weeks = { $n } sett. fa

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } riga
   *[other] { $n } righe
}
outcome-rows-capped = { $n }+ righe

## Terminal panel header

term-title = TERMINALE
term-db-cli-tip = Apri la CLI del database
term-db-cli-tip-read-only = Apri la CLI del database — questa connessione è contrassegnata come di sola lettura
term-restart-tip = Riavvia il terminale
term-settings-tip = Impostazioni del terminale

## Results panel header

results-title = RISULTATI
results-properties-tip = Proprietà della tabella…
results-monitor-tip = Monitoraggio in tempo reale…
results-restore-editor-tip = Ripristina l'editor
results-collapse-editor-tip = Comprimi l'editor

## Query panel header

query-title = QUERY
query-no-database = Nessun database

## Shared dialog buttons (continued)

dialog-stop = Ferma

## Copy table modal

copy-table-title = Copia { $table }
copy-table-rows-line = Poi copia al suo interno ogni riga di { $table } (non mostrato come SQL)
copy-table-into = Copia in
copy-table-schema = Schema
copy-table-name = Nome
copy-table-back = ‹ indietro
copy-table-data = Copia i dati
copy-table-data-hint = Copia le righe una volta creata la tabella. Se disattivato copia solo la struttura.
copy-table-what-with-rows = La struttura e le righe. Chiavi esterne e trigger non vengono copiati, e una colonna generata viene ricalcolata.
copy-table-what-structure = Solo la struttura. Chiavi esterne e trigger non vengono copiati.
copy-table-choose-database = Scegli il database in cui copiare.
copy-table-unnamed = Dai un nome alla copia.
copy-table-onto-itself = Una tabella non può essere copiata su sé stessa — dai alla copia un altro nome.
copy-table-connection-changed = La connessione attiva è cambiata — riapri Copia in dalla tabella.
copy-table-schema-not-loaded = Lo schema di { $database } non è caricato — espandilo nell'albero, poi riprova.
copy-table-preview-subject = Copia di { $table }
copy-table-copying = Copia delle righe in corso… { $n }
copy-table-copied = { $n ->
    [one] Copiata { $n } riga
   *[other] Copiate { $n } righe
}.
copy-table-stopped = Interrotto prima che tutte le righe fossero copiate. La tabella è stata creata.

## Database, schema and extension editor

database-editor-title-database = Crea database
database-problem-no-name = Il nome non può essere vuoto.
database-problem-bad-collation = Il nome di una collation contiene solo lettere, cifre e trattini bassi.
database-editor-title-schema = Crea schema
database-editor-title-extension = Crea estensione
database-editor-title-schema-in = Crea schema in { $database }
database-editor-title-extension-in = Crea estensione in { $database }
database-editor-section-database = Database
database-editor-section-schema = Schema
database-editor-section-extension = Estensione
database-editor-name = Nome
database-editor-charset = Set di caratteri
database-editor-collation = Collation
database-editor-owner = Proprietario
database-editor-server-default = predefinito del server
database-editor-owner-you = tu
database-editor-no-extensions = Nessuna estensione rimasta da installare
database-editor-no-charsets = Nessun set di caratteri da suggerire
database-editor-no-collations = Nessuna collation da suggerire
database-editor-no-roles = Nessun ruolo su questo server

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importa in { $database }
script-dialog-title = Esegui uno script SQL
script-section = Script SQL
script-choose-file = Scegli un file…
script-no-file = Nessun file scelto
script-reading = Lettura del file
script-hint = Lo script può creare e selezionare un database diverso da quello in cui stai importando. Può anche creare più di un database. Controlla lo script per assicurarti che faccia ciò che ti aspetti.
script-hint-namespace = Lo script può creare e selezionare un database diverso da quello in cui stai importando. Può anche creare più di un database. Aprirlo da { $namespace } non vi colloca nemmeno lo script: tutto ciò che il file non qualifica va dove indica il search_path del server. Controlla lo script per assicurarti che faccia ciò che ti aspetti.
script-what-it-does = Cosa fa questo file
script-probe-nothing = Questo file non contiene istruzioni che Schemaic possa eseguire.
script-probe-cut-off = I primi { $size } di questo file sono un'unica istruzione incompleta, quindi non c'è ancora nulla da riassumere. Eseguendolo si invia il file per intero.
script-probe-counted = { $n ->
    [one] { $count } istruzione letta
   *[other] { $count } istruzioni lette
} dai primi { $size }.
script-destroys = { $n ->
    [one] { $count } istruzione
   *[other] { $count } istruzioni
} in questo file { $n ->
    [one] distrugge
   *[other] distruggono
} dati — DROP, TRUNCATE o DELETE. Da qui non è possibile annullarlo.
script-unread = Sono stati letti solo i primi { $size } di questo file, e niente al loro interno distrugge dati — ma il resto non è stato esaminato. Potrebbe eseguire DROP, TRUNCATE o DELETE, e da qui non è possibile annullarlo.
script-unqualified = { $n ->
    [one] { $count } istruzione
   *[other] { $count } istruzioni
} in questo file { $n ->
    [one] agisce
   *[other] agiscono
} su ogni riga di una tabella — un DELETE, UPDATE o TRUNCATE senza WHERE. Da qui non è possibile annullarlo.
script-tx-own = Questo file apre una propria transazione, quindi viene applicato per intero o per niente. Schemaic non ne aggiunge una propria.
script-tx-none = Questo file non apre alcuna transazione, quindi un errore a metà lascia in vigore ciò che è stato eseguito prima.
script-connecting = Connessione in corso
script-progress-of = { $done } di { $total }
script-progress-read = { $done } letti
script-run = Esegui
script-done = { $n ->
    [one] Eseguita { $n } istruzione
   *[other] Eseguite { $n } istruzioni
} da { $file }.
script-stopped = Interrotto dopo { $n ->
    [one] { $n } istruzione
   *[other] { $n } istruzioni
}.
script-failed = Non riuscito: { $message } — { $n ->
    [one] { $n } istruzione eseguita
   *[other] { $n } istruzioni eseguite
} prima dell'errore.
script-failed-at = Non riuscito alla riga { $line } di { $file }: { $message } — { $n ->
    [one] { $n } istruzione eseguita
   *[other] { $n } istruzioni eseguite
} prima dell'errore.
script-durability-rolled-back = { $file } ha aperto una propria transazione, quindi il server ha annullato l'intera esecuzione con un rollback — non è stato applicato nulla, comprese le istruzioni riuscite.
script-durability-applied = { $file } non ha aperto alcuna transazione, quindi ogni istruzione eseguita resta applicata.
script-durability-applied-before = Ogni istruzione eseguita prima che { $file } aprisse la propria transazione resta applicata; ciò che è stato eseguito all'interno della transazione è stato annullato con un rollback.
script-durability-unknown = Ogni istruzione già eseguita resta applicata, a meno che { $file } non abbia aperto una propria transazione.

## Schema comparison modal

compare-title = Confronta schemi
compare-choose-database = Scegli un database
compare-no-connection = Nessuna connessione di questo motore con cui confrontare.
compare-will-change = verrà modificato
compare-source-of-truth = fonte di riferimento
compare-idle = Scegli un database con cui confrontare.
compare-loading = Lettura di entrambi gli schemi
compare-select-object = Seleziona un oggetto per vederne entrambi i lati.
compare-fk-cycle-note = Le chiavi esterne di queste tabelle formano un ciclo — nessun ordine di creazione le soddisfa tutte, quindi una migrazione generata potrebbe essere rifiutata a metà.
compare-same-but-stored-differently = Sono strutturalmente concordi — la differenza qui sotto riguarda il modo in cui il motore memorizza l'istruzione, e non c'è nulla da migrare.
compare-summary = { $differ } diversi · { $only_here } solo qui · { $only_there } solo là · { $same } identici
compare-summary-uncertain = { $n ->
    [one] { $n } corrispondenza
   *[other] { $n } corrispondenze
} che il confronto non può garantire
compare-filter-placeholder = Filtra per nome
compare-include-identical = Includi identici
compare-select-all = Seleziona tutto
compare-select-none = Nessuno
compare-hint-held-back = escluso dal piano; la nota qui sopra spiega perché
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = il suo corpo deve essere riletto dal server prima di poterlo applicare
compare-omitted-unread = questo confronto non ha alcuna istruzione per esso — non è stato possibile leggerne la definizione
compare-omitted-no-statement = questo confronto non ha alcuna istruzione per la differenza trovata
compare-held-partition = è una { $sub ->
        [yes] sottopartizione
       *[no] partizione
    } di { $parent }, che è esclusa — ricreale insieme, a mano
compare-held-partitioning-differs = il suo partizionamento differisce, e questo confronto non scrive alcuna istruzione che ripartizioni una tabella — ricreala a mano
compare-held-sequence-unwritten = questo confronto non scrive alcuna istruzione per una sequenza — copiala da `SHOW CREATE SEQUENCE`, che il suo riquadro mostra
compare-held-partitioned-unrestated = è partizionata, e nessun CREATE TABLE qui riformula il suo schema di partizione — crea a mano la funzione e lo schema di partizione, poi la tabella
compare-held-sequence-range = il contatore della destinazione ha raggiunto { $at }, fuori dal nuovo intervallo da { $min } a { $max }, e il server rifiuta di restringere una sequenza oltre un valore che ha già distribuito. Riavviala sulla destinazione (ALTER SEQUENCE … RESTART WITH) a un valore che nessuna tabella contiene, poi confronta di nuovo
compare-note-set-aside = { $aside } — { $side ->
        [target] anche la destinazione
       *[source] anche l'origine
    } contiene { $kept }, e i nomi della destinazione ignorano le maiuscole, quindi può contenerne solo uno dei due; { $kept } è quello confrontato
compare-note-binding = Tipo alias { $alias }: { $bound ->
        [rule] la regola { $rule } associata
        [default] il valore predefinito { $default } associato
       *[both] la regola { $rule } e il valore predefinito { $default } associati
    } a esso nell'origine non vengono confrontati né riportati — associali a mano (sp_bindrule / sp_bindefault).
compare-hint-blocked = il corpo deve essere riletto prima di poterlo applicare
compare-hint-uncertain = un indice che questo modello legge solo in parte — qui una corrispondenza non è certa
compare-preview-migration = Anteprima della migrazione
compare-plan-subject = { $n ->
    [one] { $n } oggetto in { $database }
   *[other] { $n } oggetti in { $database }
}
compare-selected = { $n ->
        [0] Nessuna selezione.
        [one] { $n } oggetto selezionato
       *[other] { $n } oggetti selezionati
    }
compare-incomparable = { $left } e { $right } non possono essere confrontati. Nomi dei tipi, valori predefiniti e forme degli indici non si trasferiscono tra motori, quindi qualsiasi migrazione generata dalla differenza sarebbe sbagliata.
compare-group-table = Tabelle
compare-group-view = Viste
compare-group-trigger = Trigger
compare-kind-enum = enum
compare-kind-domain = dominio
compare-kind-xml-schema-collection = raccolta di schemi XML
compare-kind-alias-type = tipo alias
compare-kind-sequence = sequenza
compare-kind-table = tabella
compare-kind-view = vista
compare-kind-function = funzione
compare-kind-procedure = procedura
compare-kind-trigger = trigger
compare-kind-event = evento
compare-kind-synonym = sinonimo

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Nessuno dei due database contiene qualcosa da confrontare.
compare-empty-agrees = Questi due schemi coincidono, oggetto per oggetto.
compare-empty-agrees-noted = Ogni oggetto coincide, ma non tutto ha potuto essere confrontato — vedi le note qui sopra.
compare-empty-no-match = Nulla corrisponde a quel filtro.
compare-empty-only-identical = Tutto ciò che corrisponde a quel filtro è identico — attiva Includi identici per vederlo.

## Schema editors — shared (continued)

editor-comment-placeholder = cosa fa

## Event editor

event-editor-title-edit = Modifica evento { $event }
event-editor-title-create = Crea evento in { $location }
event-editor-section-event = Evento
event-editor-section-schedule = Pianificazione
event-editor-section-body = Corpo
event-editor-section-options = Opzioni
event-editor-name = Nome
event-editor-body = Corpo
event-editor-runs = Esecuzione
event-editor-runs-repeating = Ripetuta
event-editor-runs-once = Una volta
event-editor-at = Alle
event-editor-every = Ogni
event-editor-starts = Inizio
event-editor-ends = Fine
event-editor-status = Stato
event-editor-status-enabled = Abilitato
event-editor-status-disabled = Disabilitato
event-editor-status-replica = Disabilitato sulla replica
event-editor-preserve = Conserva dopo il completamento
event-editor-preserve-hint = ON COMPLETION PRESERVE. Disattivato è l'impostazione predefinita di MySQL e significa che il server elimina l'evento una volta superata la sua ultima esecuzione — che per un evento singolo è subito dopo l'esecuzione.
event-editor-definer = Definer
event-editor-comment = Commento
event-editor-reading-source = Lettura del sorgente dell'evento…
event-editor-source-stale = Il sorgente reale dell'evento è arrivato dopo che hai iniziato a scrivere, quindi questo Corpo si basa sulla copia del catalogo — che non è ciò che il server contiene. Chiudi e riapri per modificarlo.
event-problem-no-name = L'evento necessita di un nome.
event-problem-no-time = Un evento singolo necessita di un orario di esecuzione.
event-problem-no-interval = Un evento ripetuto necessita di un intervallo.
event-problem-no-unit = Un evento ripetuto necessita di un'unità di intervallo (DAY, HOUR…).
event-problem-no-body = L'evento necessita di un corpo.
event-name-clash = Esiste già un evento chiamato { $name }.

## Routine editor

routine-editor-title-edit-function = Modifica funzione { $routine }
routine-editor-title-edit-procedure = Modifica procedura { $routine }
routine-editor-title-create-function = Crea funzione in { $location }
routine-editor-title-create-procedure = Crea procedura in { $location }
routine-editor-section-function = Funzione
routine-editor-section-procedure = Procedura
routine-editor-section-body = Corpo
routine-editor-section-options = Opzioni
routine-editor-name = Nome
routine-editor-parameters = Parametri
routine-editor-returns = Restituisce
routine-editor-returns-placeholder-mssql = int, o TABLE
routine-editor-language = Linguaggio
routine-editor-body = Corpo
routine-editor-recompile = Ricompila
routine-editor-recompile-hint = WITH RECOMPILE — un piano nuovo a ogni chiamata, mai uno in cache.
routine-editor-schema-binding = Associazione allo schema
routine-editor-schema-binding-hint = WITH SCHEMABINDING — le tabelle e le viste che legge non possono essere modificate a sua insaputa.
routine-editor-execute-as = Esegui come
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER o un utente
routine-editor-null-input = Restituisce NULL con input NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — il corpo non viene eseguito quando un argomento è NULL.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatico
routine-editor-comment = Commento
routine-editor-volatility = Volatilità
routine-editor-strict = Strict
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Viene eseguita con i diritti del proprietario anziché del chiamante. Quando lo usi, fissa un search_path qui sotto.
routine-editor-security-definer-hint-mysql = Viene eseguita con i diritti del definer anziché del chiamante. È l'impostazione predefinita di MySQL — disattivarla equivale a SQL SECURITY INVOKER.
routine-editor-settings = Impostazioni
routine-editor-add-setting = Aggiungi impostazione
routine-editor-deterministic = Deterministica
routine-editor-deterministic-hint = Garantisce lo stesso risultato per gli stessi argomenti. Un server con il log binario attivo rifiuta una routine non deterministica a meno che non si fidi dei creatori.
routine-editor-data-access = Accesso ai dati
routine-editor-definer = Definer
routine-editor-reading-source = Lettura del sorgente della routine…
routine-editor-source-stale = Il sorgente reale della routine è arrivato dopo che hai iniziato a scrivere, quindi questo Corpo si basa sulla copia del catalogo — che non è ciò che il server contiene. Chiudi e riapri per modificarlo.
routine-problem-function-no-name = La funzione necessita di un nome.
routine-problem-procedure-no-name = La procedura necessita di un nome.
routine-problem-function-no-language = La funzione necessita di un linguaggio (plpgsql, sql).
routine-problem-procedure-no-language = La procedura necessita di un linguaggio (plpgsql, sql).
routine-problem-no-return-type = La funzione necessita di un tipo di ritorno.
routine-problem-procedure-returns = Una procedura non restituisce nulla — cancella il tipo di ritorno.
routine-problem-function-no-body = La funzione necessita di un corpo.
routine-problem-procedure-no-body = La procedura necessita di un corpo.
routine-problem-bare-parameter = SQL Server denomina ogni parametro con @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Una procedura non può avere WITH { $option }.
routine-problem-scalar-refuses = Una funzione scalare non può avere WITH { $option }.
routine-problem-inline-refuses = Una funzione inline con valori di tabella non può avere WITH { $option }.
routine-problem-multi-refuses = Una funzione con valori di tabella a più istruzioni non può avere WITH { $option }.
routine-problem-procedure-refuses-native = Una procedura non può avere WITH { $option } una volta compilata in modo nativo.
routine-problem-scalar-refuses-native = Una funzione scalare non può avere WITH { $option } una volta compilata in modo nativo.
routine-problem-inline-refuses-native = Una funzione inline con valori di tabella non può avere WITH { $option } una volta compilata in modo nativo.
routine-problem-multi-refuses-native = Una funzione con valori di tabella a più istruzioni non può avere WITH { $option } una volta compilata in modo nativo.
routine-problem-native-needs-schemabinding = Una routine compilata in modo nativo richiede WITH SCHEMABINDING — SQL Server esige le due cose insieme.
routine-problem-inline-with-execute-as = INLINE = ON non può essere combinato con EXECUTE AS — una funzione eseguita come qualcun altro non viene mai resa inline.
routine-problem-numbered-rename = { $was } è a capo di un gruppo di procedure numerate ({ $group }), e rinominarla le eliminerebbe tutte. Mantieni il nome per modificarla.
routine-problem-trigger-function-args = Una funzione trigger non accetta argomenti dichiarati — arrivano in TG_ARGV.
routine-clash-function-refused = Esiste già una funzione chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi il server rifiuta questa e non cambia nulla — scegli un altro nome, oppure elimina prima { $name }.
routine-clash-procedure-refused = Esiste già una procedura chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi il server rifiuta questa e non cambia nulla — scegli un altro nome, oppure elimina prima { $name }.
routine-clash-function-destroys = Esiste già una funzione chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi applicarla distruggerebbe { $original } e lascerebbe { $name } com'era.
routine-clash-procedure-destroys = Esiste già una procedura chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi applicarla distruggerebbe { $original } e lascerebbe { $name } com'era.
routine-clash-function-destroys-new = Esiste già una funzione chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi applicarla distruggerebbe l'originale e lascerebbe { $name } com'era.
routine-clash-procedure-destroys-new = Esiste già una procedura chiamata { $name }. Su questo motore una ridenominazione è un'eliminazione seguita da una creazione, quindi applicarla distruggerebbe l'originale e lascerebbe { $name } com'era.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Cerca account
users-loading-accounts = Caricamento degli account
users-unsupported = Il motore di questa connessione non ha account utente.
users-none = Questo server non riporta alcun account.
users-no-match = Nessun account corrisponde a «{ $needle }».
users-pick-account = Scegli un account per vedere cosa può fare.
users-section-attributes = Attributi
users-section-privileges = Privilegi
users-attr-authentication = Autenticazione
users-attr-locked = Bloccato
users-attr-password-expired = Password scaduta
users-attr-superuser = Superutente
users-attr-create-role = Creazione ruoli
users-attr-create-database = Creazione database
users-attr-replication = Replica
users-attr-bypass-rls = Ignora RLS
users-attr-inherit = Eredita
users-attr-connection-limit = Limite di connessioni
users-attr-valid-until = Valido fino al
users-attr-type = Tipo
users-attr-disabled = Disabilitato
users-attr-default-database = Database predefinito
users-attr-server-roles = Ruoli del server
users-attr-login = Login
users-attr-default-schema = Schema predefinito
users-attr-member-of = Membro di
users-attr-yes = Sì
users-attr-no = No
users-login-type-sql = Login SQL
users-login-type-windows = Login Windows
users-login-type-windows-group = Gruppo Windows
users-kind-user = Utente
users-kind-role = Ruolo
users-kind-login = Login
users-maintained-by-server = gestito dal server
users-loading-privileges = Caricamento dei privilegi
users-no-privileges = Questo account non detiene privilegi.
users-note-my-own-account = Questa connessione non può leggere mysql.user, quindi questo non è l'elenco degli account del server — è l'unico account che questa connessione può vedere, il proprio. Connettiti con un account che abbia SELECT sul database mysql per sfogliare gli altri.
users-note-ms-logins-elsewhere = Azure SQL Database conserva i suoi login in master, e qui non vengono gestiti: sono elencati gli utenti e i ruoli del database.
users-note-ms-logins-only = Sono elencati solo i login del server: scegli un database per vederne utenti e ruoli.
users-note-ms-short = Questo login non ha VIEW ANY DEFINITION, quindi il server gli mostra solo le entità che può vedere — l'elenco potrebbe essere incompleto.
users-note-my-scope = I privilegi detenuti tramite un ruolo concesso non vengono espansi: SHOW GRANTS elenca ciò che è concesso direttamente all'account e i ruoli di cui è membro, ma non ciò che quei ruoli detengono a loro volta. Apri i privilegi del ruolo stesso per vederli.
users-note-pg-scope = I privilegi su schemi, tabelle e sequenze sono quelli in { $database } — PostgreSQL li conserva nel catalogo di ciascun database, quindi i privilegi in altri database non sono elencati. Non sono elencati nemmeno EXECUTE su funzioni e procedure e i privilegi concessi su una singola colonna. I privilegi detenuti tramite l'appartenenza a un ruolo o concessi a PUBLIC non vengono espansi, e neppure quelli che un ruolo detiene in quanto proprietario di un oggetto o superutente.
users-note-pg-no-database = I privilegi su schemi, tabelle e sequenze non sono elencati: PostgreSQL li conserva nel catalogo di ciascun database, e questa connessione non ha alcun database selezionato. Apri una scheda di query su un database per vederli. Non sono elencati nemmeno EXECUTE su funzioni e procedure e i privilegi concessi su una singola colonna. I privilegi detenuti tramite l'appartenenza a un ruolo o concessi a PUBLIC non vengono espansi, e neppure quelli che un ruolo detiene in quanto proprietario di un oggetto o superutente.
users-note-pg-superuser = Questo ruolo è un SUPERUSER: aggira ogni controllo dei permessi, quindi detiene ogni privilegio su ogni oggetto, che quanto segue lo indichi o meno.
users-note-pg-predefined = { $role } è uno dei ruoli predefiniti di PostgreSQL: ciò che può fare è integrato nel server anziché concesso, quindi non è registrato in alcun catalogo letto qui.
users-note-pg-owns = { $n ->
    [one] Questo ruolo possiede { $n } oggetto in { $database } (schemi, tabelle, sequenze o il database stesso). Un proprietario detiene ogni privilegio su ciò che possiede, e PostgreSQL non registra alcuna concessione per questo, quindi nulla di ciò è elencato qui sotto.
   *[other] Questo ruolo possiede { $n } oggetti in { $database } (schemi, tabelle, sequenze o il database stesso). Un proprietario detiene ogni privilegio su ciò che possiede, e PostgreSQL non registra alcuna concessione per questo, quindi nulla di ciò è elencato qui sotto.
}
users-note-pg-owns-cluster = { $n ->
    [one] Questo ruolo possiede { $n } oggetto in questo cluster (schemi, tabelle, sequenze o il database stesso). Un proprietario detiene ogni privilegio su ciò che possiede, e PostgreSQL non registra alcuna concessione per questo, quindi nulla di ciò è elencato qui sotto.
   *[other] Questo ruolo possiede { $n } oggetti in questo cluster (schemi, tabelle, sequenze o il database stesso). Un proprietario detiene ogni privilegio su ciò che possiede, e PostgreSQL non registra alcuna concessione per questo, quindi nulla di ciò è elencato qui sotto.
}
users-note-ms-unshown = { $n ->
    [one] { $n } permesso non è mostrato qui, su { $classes }: il server lo riporta su un'entità a protezione diretta che questo elenco non sa denominare.
   *[other] { $n } permessi non sono mostrati qui, su { $classes }: il server li riporta su entità a protezione diretta che questo elenco non sa denominare.
}
users-note-ms-pick-database = I permessi di un utente di database sono quelli di quel database — scegline uno per vederli.
users-statements-capped = Vengono mostrate le prime { $shown } istruzioni su { $total }. Copia privilegi le prende tutte.
users-new-account = Nuovo account
users-system-account = Questo account appartiene al server, quindi Schemaic non lo modificherà.
users-reset-password = Reimposta password
users-gate-no-database = Apri una scheda di query su un database per modificare i privilegi — l'istruzione deve essere eseguita in uno.
users-gate-read-only = Questa connessione è di sola lettura.
users-count = { $n ->
    [one] { $n } account
   *[other] { $n } account
}
users-count-filtered = { $shown } di { $n } account
users-copy-privileges = Copia privilegi

## Schema editors — shared (continued)

editor-row-remove = Rimuovi

## Trigger editor

trigger-editor-title = Trigger su { $table }
trigger-problem-no-name = Il trigger necessita di un nome.
trigger-problem-no-table = Un trigger necessita di una tabella su cui attivarsi.
trigger-problem-no-event = Un trigger necessita di almeno un evento su cui attivarsi.
trigger-problem-pg-no-function = Un trigger PostgreSQL necessita di una funzione da eseguire.
trigger-problem-pg-body = Un trigger PostgreSQL esegue una funzione, non un corpo — scegline o scrivine una da eseguire.
trigger-problem-truncate-row-level = Un trigger TRUNCATE deve essere FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Un trigger INSTEAD OF deve essere FOR EACH ROW.
trigger-problem-instead-of-view-only = Solo una vista può avere un trigger INSTEAD OF — una tabella accetta BEFORE o AFTER.
trigger-problem-pg-view-row-timing = Un trigger BEFORE o AFTER di una vista deve essere FOR EACH STATEMENT — usa INSTEAD OF per agire sulle righe.
trigger-problem-update-of-without-update = UPDATE OF indica colonne su un trigger UPDATE.
trigger-problem-pg-instead-of-columns = Un trigger INSTEAD OF non accetta un elenco di colonne — PostgreSQL lo attiva su qualsiasi UPDATE della vista.
trigger-problem-one-event = { $engine } attiva un trigger su un solo evento — crea un trigger separato per ogni evento.
trigger-problem-no-truncate = { $engine } non ha trigger TRUNCATE.
trigger-problem-sqlite-view-timing = Il trigger di una vista deve essere INSTEAD OF — SQLite non ha trigger BEFORE o AFTER su una vista.
trigger-problem-sqlite-row-only = SQLite ha solo trigger FOR EACH ROW — non ne esistono a livello di istruzione.
trigger-problem-no-body = Un trigger { $engine } necessita di un corpo.
trigger-problem-sqlite-not-block = Il corpo di un trigger SQLite deve essere un blocco BEGIN … END che contenga almeno un'istruzione.
trigger-problem-runs-body = Un trigger { $engine } esegue un corpo, non una funzione — scrivi le istruzioni da eseguire.
trigger-problem-ms-no-before = SQL Server non ha trigger BEFORE — usa INSTEAD OF per agire prima della scrittura.
trigger-problem-ms-view-timing = Su SQL Server il trigger di una vista deve essere INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server attiva un trigger una volta per istruzione — leggi le righe da inserted e deleted.
trigger-problem-ms-instead-of-rank = Un trigger INSTEAD OF non può avere un ordine di attivazione — riporta ogni evento a qualsiasi ordine.
trigger-problem-first-without-event = Si attiva per primo su { $event }, per cui non è un trigger.
trigger-problem-last-without-event = Si attiva per ultimo su { $event }, per cui non è un trigger.
trigger-problem-no-when = { $engine } non ha la condizione WHEN — metti il test all'interno del corpo con IF.
trigger-problem-ms-no-update-of = SQL Server non ha UPDATE OF — verifica UPDATE(colonna) all'interno del corpo.
trigger-problem-my-no-view = MySQL non accetta trigger su una vista.
trigger-problem-my-no-instead-of = MySQL non ha trigger INSTEAD OF — sono di PostgreSQL.
trigger-problem-my-no-update-of = MySQL non ha UPDATE OF — un trigger vede ogni colonna.
trigger-problem-constraint-trigger = Schemaic non può modificare il trigger di vincolo { $name } — non modella le impostazioni di differimento che porta con sé.
trigger-problem-not-editable = Schemaic non può modificare il trigger { $name } — la sua definizione è crittografata, nascosta a questo login, o usa un'opzione che Schemaic non modella. Modificalo invece in SQL.
trigger-problem-twice = Due trigger si chiamano entrambi { $name }.
trigger-problem-first-clash = { $n ->
    [2] Solo un trigger può attivarsi per primo su { $event } — { $names } e { $last } sono impostati entrambi così.
   *[other] Solo un trigger può attivarsi per primo su { $event } — { $names } e { $last } sono impostati tutti così.
}
trigger-problem-last-clash = { $n ->
    [2] Solo un trigger può attivarsi per ultimo su { $event } — { $names } e { $last } sono impostati entrambi così.
   *[other] Solo un trigger può attivarsi per ultimo su { $event } — { $names } e { $last } sono impostati tutti così.
}
trigger-editor-preview-subject = trigger su { $table }
trigger-editor-none-selected = Nessun trigger selezionato.
trigger-editor-readonly-constraint = Questo è un trigger di vincolo. Schemaic non modella le impostazioni di differimento che porta con sé, quindi qui può essere rimosso ma non modificato — modificalo invece in SQL.
trigger-editor-readonly-hidden = La definizione di questo trigger è crittografata o nascosta a questo login, quindi qui può essere rimosso ma non modificato.
trigger-editor-readonly-unmodelled = Questo trigger usa un'opzione che Schemaic non modella, quindi qui può essere rimosso ma non modificato — modificalo invece in SQL.
trigger-editor-section-trigger = Trigger
trigger-editor-section-condition = Condizione
trigger-editor-section-action = Azione
trigger-editor-name = Nome
trigger-editor-timing = Momento
trigger-editor-event = Evento
trigger-editor-fires = Si attiva
trigger-editor-truncate-hint = Solo a livello di istruzione.
trigger-editor-not-for-replication-hint = Inattivo mentre un agente di replica scrive nella tabella.
trigger-editor-rank-any = Qualsiasi ordine
trigger-editor-rank-first = Si attiva per primo
trigger-editor-rank-last = Si attiva per ultimo
trigger-editor-when = Quando
trigger-editor-of-columns = Delle colonne
trigger-editor-of-columns-placeholder = facoltativo — tutte le colonne se vuoto
trigger-editor-body = Corpo
trigger-editor-function = Funzione
trigger-editor-edit-function = Modifica
trigger-editor-new-function = Nuova funzione
trigger-editor-arguments = Argomenti
trigger-editor-add-argument = Aggiungi argomento

## Schema editors — shared (continued)

editor-row-move-up = Sposta su
editor-row-move-down = Sposta giù

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Modifica { $kind } { $name }
object-editor-title-create = Crea { $kind } in { $location }
object-editor-name = Nome
object-editor-comment = Commento
object-editor-type = Tipo
object-editor-default = Predefinito
object-editor-section-type = Tipo
object-editor-section-values = Valori
object-editor-section-domain = Dominio
object-editor-section-constraints = Vincoli
object-editor-section-sequence = Sequenza
object-editor-section-position = Posizione
object-editor-section-synonym = Sinonimo
object-editor-section-alias-type = Tipo alias
object-editor-section-xml-schema-collection = Raccolta di schemi XML
object-editor-section-stands-for = Rappresenta
object-editor-value-placeholder = valore
object-editor-add-value = Aggiungi valore
object-editor-values-order = In ordine di confronto — è l'ordine usato da ORDER BY
object-editor-add-constraint = Aggiungi vincolo
object-editor-no-base-types = Questo motore non suggerisce tipi di base
object-editor-not-null-hint = Ogni colonna di questo dominio rifiuta NULL. L'attivazione non riesce se una ne contiene già uno.
object-editor-stores = Memorizza
object-editor-increment = Incremento
object-editor-start = Inizio
object-editor-cache = Cache
object-editor-minimum = Minimo
object-editor-maximum = Massimo
object-editor-not-whole-number = { $field } deve essere un numero intero.
object-problem-type-no-name = Il tipo necessita di un nome.
object-problem-duplicate-value = Il valore { $value } è elencato più di una volta.
object-problem-empty-value = Un valore non può essere vuoto — PostgreSQL non potrà mai rimuoverlo.
object-problem-domain-no-name = Il dominio necessita di un nome.
object-problem-domain-no-base = Un dominio necessita di un tipo su cui basarsi.
object-problem-constraint-no-name = Ogni vincolo necessita di un nome.
object-problem-constraint-no-predicate = Il vincolo { $name } non ha alcun predicato.
object-problem-constraint-twice = Il vincolo { $name } è nominato due volte.
object-problem-sequence-no-name = La sequenza necessita di un nome.
object-problem-zero-increment = Una sequenza non può incrementare di 0.
object-problem-min-above-max = Il minimo ({ $min }) è superiore al massimo ({ $max }).
object-problem-start-outside = Il valore iniziale ({ $start }) è fuori da { $min }..{ $max }.
object-problem-cache-below-one = La cache deve essere almeno 1.
object-problem-does-not-fit = { $min }..{ $max } non rientra in { $data_type }.
object-problem-restart-outside = Il riavvio a { $restart } è fuori da { $min }..{ $max }.
object-problem-counter-outside = La sequenza è a { $last }, fuori dal nuovo { $min }..{ $max }. Assegnale un valore di riavvio all'interno dell'intervallo.
object-problem-no-name = Il nome non può essere vuoto.
object-problem-tsql-no-type = Una sequenza necessita di un tipo.
object-problem-start-not-whole = L'inizio deve essere un numero intero.
object-problem-increment-not-whole = L'incremento deve essere un numero intero.
object-problem-minimum-not-whole = Il minimo deve essere un numero intero.
object-problem-maximum-not-whole = Il massimo deve essere un numero intero.
object-problem-tsql-zero-increment = L'incremento non può essere 0.
object-problem-min-not-below-max = Il minimo deve essere inferiore al massimo.
object-problem-started-outside = È iniziata a { $start }, fuori dal nuovo intervallo: riavviala a un valore compreso tra il minimo e il massimo.
object-problem-tsql-start-outside = L'inizio deve essere compreso tra il minimo e il massimo.
object-problem-cache-not-positive = La dimensione della cache deve essere un numero intero maggiore di 0.
object-problem-restart-not-whole = Il valore di riavvio deve essere un numero intero.
object-problem-tsql-restart-outside = Il valore di riavvio deve essere compreso tra il minimo e il massimo.
object-problem-synonym-no-target = Un sinonimo necessita dell'oggetto che rappresenta.
object-problem-alias-no-base = Un tipo alias necessita di un tipo di base.
object-editor-cycle = Ciclo
object-editor-cycle-hint = Ricomincia dall'altra estremità dell'intervallo invece di fallire quando la sequenza si esaurisce.
object-editor-restart-at = Riavvia a — l'ultimo valore distribuito dal contatore è { $last }
object-editor-restart-at-unused = Riavvia a — il contatore non è stato usato
object-editor-restart-not-whole = Riavvia a deve essere un numero intero.
object-editor-restart-placeholder = lascia vuoto per mantenere la posizione
object-editor-tsql-restart-placeholder = se vuoto la mantiene, a meno che il tipo cambi
object-editor-owned-by = Di proprietà di
object-editor-detach = Scollega dalla sua colonna
object-editor-detach-hint = La sequenza non viene più eliminata insieme alla colonna, e le sopravvive.
object-editor-cache-hint = Distribuisci valori dalla memoria, più velocemente, al costo di un salto dopo un riavvio del server.
object-editor-cache-size = Dimensione della cache
object-editor-cache-size-placeholder = predefinita del server
object-editor-synonym-server = Server
object-editor-synonym-server-placeholder = questo server
object-editor-synonym-database = Database
object-editor-synonym-database-placeholder = questo database
object-editor-synonym-schema = Schema
object-editor-synonym-schema-placeholder = lo schema predefinito
object-editor-synonym-object = Oggetto
object-editor-synonym-object-placeholder = tabella, vista o routine
object-editor-base-type = Tipo di base
object-editor-nullable = Ammette NULL
object-editor-nullable-hint = Una colonna di questo tipo accetta NULL a meno che la colonna non indichi diversamente.
object-editor-schemas = Schemi

## Account and grant editors (Users and privileges)

account-editor-title-create = Crea account
account-editor-title-reset = Reimposta password
account-editor-title-privileges = Privilegi — { $account }
account-editor-account = Account
account-editor-kind = Tipo
account-editor-name = Nome
account-editor-host = Host
account-editor-host-placeholder = % (qualsiasi host)
account-editor-for-login = Per il login
account-editor-for-login-placeholder = login — vuoto per un utente senza login
account-editor-also-user = Anche utente in { $database }
account-editor-password = Password
account-editor-confirm = Conferma
account-editor-action = Azione
account-editor-action-grant = Concedi
account-editor-action-revoke = Revoca
account-editor-subject = Oggetto
account-editor-subject-privileges = Privilegi
account-editor-subject-role = Ruolo
account-editor-role = Ruolo
account-editor-no-roles = Questo server non ha ruoli
account-editor-with-admin-option = Con opzione di amministrazione
account-editor-level = Livello
account-editor-level-global = Intero server
account-editor-level-database = Database
account-editor-level-schema = Schema
account-editor-level-table = Tabella
account-editor-level-sequence = Sequenza
account-editor-no-suggestions = Nessun suggerimento
account-editor-section-privileges = Privilegi
account-editor-with-grant-option = Con opzione di concessione
account-editor-needs-role = Indica un ruolo.
account-editor-needs-privileges = Scegli un livello, indica a cosa si applica e spunta un privilegio.
account-editor-password-required = È richiesta una password.
account-editor-name-required = È richiesto un nome.
account-editor-entra-only-user = Da Microsoft Entra si crea solo un utente di database.
account-editor-entra-no-password = Un utente Entra qui non ha login né password — è Entra a farlo accedere.
account-editor-login-needs-password = Un login necessita di una password.
account-editor-user-for-login-password = Un utente legato a un login accede con la password del login — lascia vuoto questo campo.
account-editor-confirm-password = Digita di nuovo la password per confermarla.
account-editor-passwords-differ = Le due password non coincidono.
account-editor-password-hashed = L'SQL in anteprima, che è l'istruzione che viene eseguita, contiene un hash della password nella forma in cui questo server la memorizza, non la password stessa — a meno che non contenga caratteri che il login normalizzerebbe (lettere a larghezza intera, legature, spazi unificatori), o che il server usi un'estensione di verifica delle password; in quei casi viene inviata così come digitata.
account-editor-password-as-typed = La password compare nell'SQL in anteprima, che è l'istruzione che viene eseguita.

## Table designer

designer-title-edit = Modifica { $table }
designer-title-create = Crea tabella in { $location }
designer-tab-table = Tabella
designer-tab-columns = Colonne
designer-tab-indexes = Indici
designer-tab-foreign-keys = Chiavi esterne
designer-tab-checks = Controlli
designer-list-add = Aggiungi
designer-no-suggestions = Nessun suggerimento
designer-name = Nome
designer-comment = Commento
designer-table-comment-placeholder = A cosa serve questa tabella
designer-engine = Motore
designer-collation = Collation
designer-no-column = Nessuna colonna selezionata.
designer-type = Tipo
designer-type-hint = Scritto così com'è — è il server a deciderne il significato.
designer-nullable = Ammette NULL
designer-nullable-hint = Consenti NULL in questa colonna.
designer-primary-key = Chiave primaria
designer-primary-key-hint = Parte della chiave primaria della tabella, nell'ordine delle colonne.
designer-identity = Identity
designer-auto-increment = Auto-increment
designer-identity-hint = Il valore viene assegnato dal server ({ $keyword }).
designer-default = Predefinito
designer-default-hint = Testo SQL, come lo scriveresti: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Generata da
designer-generated-hint = Un'espressione, ad es. qty * price. Una colonna generata non accetta un valore predefinito.
designer-on-update = On update
designer-index-unique-detail = univoco
designer-index-redundant-detail = ridondante
designer-hint-redundant = L'indice { $index } è coperto da { $cover }: ogni ricerca che serve, la serve anche quello.
table-problem-no-name = La tabella necessita di un nome.
table-problem-no-columns = Una tabella necessita di almeno una colonna.
table-problem-column-no-name = Ogni colonna necessita di un nome.
table-problem-column-twice = Due colonne si chiamano entrambe { $name }.
table-problem-column-no-type = La colonna { $name } necessita di un tipo.
table-problem-key-not-column = La chiave primaria indica { $column }, che non è una colonna.
table-problem-index-no-name = Ogni indice necessita di un nome.
table-problem-index-twice = Due indici si chiamano entrambi { $name }.
table-problem-index-no-columns = L'indice { $index } non ha colonne.
table-problem-xml-one-column = L'indice { $index } deve indicare esattamente una colonna.
table-problem-xml-no-options = L'indice { $index } non può essere univoco, includere colonne o essere filtrato: un indice XML o spaziale non accetta nessuna di queste cose.
table-problem-xml-no-using = L'indice { $index } necessita della clausola USING XML INDEX … FOR …: un indice XML secondario si basa su uno primario o selettivo.
table-problem-selective-no-paths = L'indice { $index } necessita dei percorsi FOR (…): un indice XML selettivo è costituito dai percorsi che promuove.
table-problem-xml-parent-gone = L'indice { $index } si basa su { $parent }, che non è un indice XML primario o selettivo di questa tabella: viene eliminato insieme al suo indice padre.
table-problem-not-orderable = L'indice { $index } non può essere ordinato: solo un indice columnstore ha un ORDER.
table-problem-order-not-column = L'indice { $index } è ordinato per { $column }, che non è una colonna.
table-problem-order-not-stored = L'indice { $index } è ordinato per { $column }, che non memorizza.
table-problem-columnstore-unique = L'indice { $index } non può essere univoco: un indice columnstore memorizza colonne, non chiavi.
table-problem-columnstore-include = L'indice { $index } non può includere colonne: un indice columnstore elenca ogni colonna che memorizza.
table-problem-columnstore-sorted = L'indice { $index } non può ordinare le sue colonne: una colonna columnstore non ha direzione.
table-problem-clustered-columnstore-filtered = L'indice { $index } non può essere filtrato: un indice columnstore cluster memorizza ogni riga.
table-problem-clustered-include = L'indice { $index } non può includere colonne: un indice cluster contiene già ogni colonna.
table-problem-unique-include = L'indice { $index } non può includere colonne: qui un vincolo univoco non ha INCLUDE.
table-problem-clustered-filtered = L'indice { $index } non può essere filtrato: un indice cluster copre ogni riga.
table-problem-constraint-filtered = L'indice { $index } non può essere filtrato: sostiene un vincolo UNIQUE, che qui non accetta condizioni. Eliminalo e aggiungi un indice univoco con la condizione.
table-problem-not-partial = L'indice { $index } non può essere filtrato: qui questo indice non può essere parziale.
table-problem-index-not-column = L'indice { $index } indica { $column }, che non è una colonna.
table-problem-include-not-column = L'indice { $index } include { $column }, che non è una colonna.
table-problem-cluster-clash = Una tabella contiene un solo indice cluster, e { $clustered } la organizzano ciascuno in cluster — rendi non cluster tutti tranne uno.
table-problem-cluster-key = la chiave primaria
table-problem-cluster-index = l'indice { $index }
table-problem-cluster-and = e
table-problem-fk-no-name = Ogni chiave esterna necessita di un nome.
table-problem-fk-twice = Due chiavi esterne si chiamano entrambe { $name }.
table-problem-fk-no-table = La chiave esterna { $fk } non fa riferimento ad alcuna tabella.
table-problem-fk-unpaired = La chiave esterna { $fk } deve abbinare ogni colonna a una a cui fa riferimento.
table-problem-fk-not-column = La chiave esterna { $fk } indica { $column }, che non è una colonna.
table-problem-check-no-name = Ogni vincolo CHECK necessita di un nome.
table-problem-check-twice = Due vincoli CHECK si chiamano entrambi { $name }.
table-problem-check-no-predicate = Il controllo { $name } non ha alcun predicato.
table-problem-without-rowid-no-key = Una tabella WITHOUT ROWID necessita di una chiave primaria.
table-problem-strict-type = Le tabelle STRICT accettano solo INT, INTEGER, REAL, TEXT, BLOB o ANY — { $column } è { $data_type }.
designer-hint-unindexed-fk = La chiave esterna { $fk } non ha alcun indice che inizi con le sue colonne, quindi un'eliminazione o un aggiornamento di chiave sulla tabella a cui fa riferimento esegue una scansione di questa.
designer-no-index = Nessun indice selezionato. La chiave primaria si trova sulle colonne.
designer-columns = Colonne
designer-index-columns-hint-single = L'unica colonna xml, geometry o geography che indicizza.
designer-index-columns-hint-columnstore = Colonne che memorizza, separate da virgole.
designer-index-columns-hint-prefix = Separate da virgole, in ordine di chiave. bio(20) è una lunghezza di prefisso; aggiungi DESC per l'ordine decrescente.
designer-index-columns-hint = Separate da virgole, in ordine di chiave. Aggiungi DESC per una colonna decrescente.
designer-index-method = Metodo
designer-index-method-hint = Lascia vuoto per il valore predefinito (btree).
designer-index-filter = Solo le righe in cui
designer-index-filter-hint = La condizione di un indice parziale, senza il WHERE.
designer-index-storage = Archiviazione
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Columnstore cluster
designer-storage-primary-xml = XML primario
designer-storage-selective-xml = XML selettivo
designer-storage-secondary-xml = XML secondario
designer-storage-spatial = Spaziale
designer-index-using = Using
designer-index-using-hint = La clausola dopo la colonna: USING XML INDEX … FOR PATH, VALUE o PROPERTY di un indice XML secondario (o FOR (percorso) su uno selettivo), [WITH XMLNAMESPACES (…)] FOR (nome = '/percorso' [AS …], …) di un indice XML selettivo, o USING … WITH (…) di un indice spaziale.
designer-index-order = Ordine
designer-index-order-hint = Colonne, separate da virgole, in base alle quali sono ordinati i segmenti — da SQL Server 2022, e da 2025 per un indice non cluster, che ordina solo per colonne che memorizza.
designer-index-include = Includi
designer-index-include-hint = Colonne, separate da virgole, contenute nell'indice ma non parte della sua chiave.
designer-index-unique = Univoco
designer-index-unique-hint = Rifiuta valori duplicati su queste colonne.
designer-fk-detail-unindexed = → { $table } · nessun indice
designer-no-fk = Nessuna chiave esterna selezionata.
designer-fk-ref-table = Tabella di riferimento
designer-fk-on-delete = On delete
designer-fk-columns-hint = Separate da virgole, abbinate in ordine alle colonne di riferimento.
designer-fk-ref-columns = Colonne di riferimento
designer-no-check = Nessun controllo selezionato.
designer-check-expression = Espressione
designer-check-expression-hint = La condizione che ogni riga deve soddisfare. Scritta senza CHECK (…).
designer-check-enforced = Applicato
designer-check-enforced-hint = Se disattivato, il vincolo viene registrato senza essere applicato — sia le righe esistenti sia quelle nuove vengono accettate.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Gestisci connessioni
connections-new = Nuova connessione
connections-import = Importa connessioni
connections-export = Esporta connessioni
connections-duplicate = Duplica
connections-delete = Elimina
connections-test = Prova
connections-save = Salva
connections-test-failed = Connessione non riuscita
connections-browse = Sfoglia…
connections-section-general = Generale
connections-section-safeguards = Protezioni
connections-section-database = Database
connections-section-network = Rete
connections-name = Nome
connections-colour = Colore
connections-folder = Cartella
connections-no-folders = Ancora nessuna cartella
connections-folder-hint = Facoltativo. Le connessioni con la stessa cartella vengono elencate insieme.
connections-prominent-colour = Colore in evidenza nell'editor
connections-prominent-colour-hint = Incornicia l'editor di query con il colore di questa connessione.
connections-read-only = Sola lettura
connections-read-only-hint = Disabilita la modifica delle celle e blocca le query di scrittura/DDL su questa connessione.
connections-read-only-mssql = SQL Server non ha sessioni in sola lettura, quindi le letture vengono eseguite in una transazione che Schemaic annulla con un rollback. L'effetto di una procedura al di fuori del database sopravvive a ciò; per avere una garanzia, connettiti con un login che può solo leggere.
connections-cli-access = Accesso dalla CLI
connections-cli-access-hint = Consenti alla riga di comando schemaic, e agli agenti IA tramite schemaic mcp, di usare questa connessione.
connections-ai-data-access = Accesso dell'IA ai dati
connections-statement-timeout = Timeout dell'istruzione
connections-statement-timeout-hint = Annulla un'istruzione su questa connessione che dura di più.
connections-statement-timeout-same = Come l'app
connections-environment = Ambiente
env-none = Nessuno
env-local = Locale
env-development = Sviluppo
env-testing = Test
env-staging = Staging
env-production = Produzione
connections-type = Tipo
connections-init-sql = SQL di inizializzazione
connections-init-sql-pragma-hint = Facoltativo. Impostazioni PRAGMA eseguite su ogni connessione che Schemaic apre.
connections-init-sql-set-hint = Facoltativo. Istruzioni SET eseguite su ogni connessione che Schemaic apre — un search path, un fuso orario, un ruolo.
connections-init-refused = «{ $statement }»: { $why }
connections-init-only-pragmas = alla connessione possono essere eseguite solo impostazioni PRAGMA
connections-init-only-sets = alla connessione possono essere eseguite solo istruzioni SET
connections-init-calls-function = alla connessione un'impostazione non può chiamare una funzione
connections-init-quoted-name = alla connessione il nome di un'impostazione non può essere tra virgolette
connections-init-names-no-setting = non indica alcuna impostazione
connections-init-not-listed = { $name } non può essere impostato alla connessione — non è tra le impostazioni di sessione che Schemaic sa non interferire con le sue protezioni
connections-init-not-utf8 = il set di caratteri client deve restare UTF-8 — il controllo di sola lettura di Schemaic legge ogni istruzione come UTF-8
connections-init-one-value = alla connessione un'impostazione accetta un solo valore, e nient'altro può seguirlo
connections-init-one-value-after-equals = alla connessione un'impostazione accetta un solo valore, dopo un =
connections-init-transaction = le impostazioni di transazione non possono essere definite alla connessione — le imposta la protezione di sola lettura
connections-init-pragma = quel PRAGMA non può essere eseguito alla connessione — modifica il file o la protezione di sola lettura
connections-init-server-scope = le impostazioni { $scope } modificano il server, non la sessione
connections-init-variable = una variabile non è un'impostazione di sessione — termina con il suo batch
connections-init-on-only = { $name } può essere solo attivato alla connessione — Schemaic vi fa affidamento
connections-host = Host
connections-port = Porta
connections-named-instance-port = Un'istanza denominata: la porta la fornisce SQL Server Browser, quindi Porta non viene usata. Per fare a meno del Browser, imposta l'host senza l'istanza e la porta su cui è in ascolto.
connections-database = Database
connections-database-hint = Facoltativo; è il database in cui si apre la connessione. Alcuni provider in hosting consentono solo il proprio e rifiutano una connessione che non lo indica.
connections-authentication = Autenticazione
ssh-auth-password = Password
ssh-auth-key-pair = Coppia di chiavi
ssh-auth-agent = Agente SSH
auth-password = Utente e password
auth-windows = Windows (questo account)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Utente
connections-password = Password
connections-azure-cli-hint = Accede come l'utente con cui è stato eseguito l'accesso all'Azure CLI (esegui prima `az login`). Il token viene ottenuto quando una connessione ne ha bisogno e non viene mai salvato.
connections-aws-iam-hint = Accede come l'utente indicato sotto con un token dalle credenziali dell'AWS CLI (prima `aws configure` o `aws sso login`). L'utente necessita del ruolo rds_iam (PostgreSQL) o dell'AWSAuthenticationPlugin (MySQL). Il token viene ottenuto quando una connessione ne ha bisogno e non viene mai salvato.
connections-gcp-iam-hint = Accede come l'utente indicato sotto — l'utente IAM o l'account di servizio noto a Cloud SQL — con un token dalla Google Cloud CLI (prima `gcloud auth login`). Il token viene ottenuto quando una connessione ne ha bisogno e non viene mai salvato.
connections-windows-hint-named = Accede come l'account Windows con cui è in esecuzione Schemaic ({ $account }). Il server deve conoscere quell'account come login Windows.
connections-windows-hint = Accede come l'account Windows con cui è in esecuzione Schemaic. Il server deve conoscere quell'account come login Windows.
connections-database-file = File di database
connections-database-file-dialog = Seleziona database SQLite
connections-sqlite-hint = Un database SQLite è un singolo file, quindi non c'è alcun server da raggiungere — niente host, utente, password o tunnel. Le transazioni manuali non sono ancora disponibili su di esso: le istruzioni vengono confermate man mano che vengono inviate.
connections-ssh-tunnel = Tunnel SSH
connections-ssh-tunnel-hint = Raggiungi il server tramite un tunnel SSH.
connections-ssh-host = Host SSH
connections-ssh-port = Porta SSH
connections-ssh-user = Utente SSH
connections-ssh-password = Password SSH
connections-ssh-agent-hint = La firma è delegata al tuo agente SSH in esecuzione (agente OpenSSH / Pageant su Windows). Schemaic non memorizza alcuna chiave.
connections-jump-hosts = Host di salto
connections-jump-hosts-hint = Facoltativo. Host da attraversare prima dell'host SSH, dal più vicino — ciascuno accede nel modo impostato sotto.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , chiave { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , tramite { $jump }
       *[none] {""}
    }
connections-route-jump-host = host di salto { $host }
connections-route-proxy-command = ~/.ssh/config raggiunge { $host } tramite un ProxyCommand, che Schemaic non può eseguire. Indica invece il percorso come Host di salto.
connections-route-match = ~/.ssh/config potrebbe instradare { $host } tramite un blocco Match, che Schemaic non legge. Indica invece il percorso come Host di salto.
connections-route-match-destination = ~/.ssh/config potrebbe indirizzare { $host } a un altro host o porta tramite un blocco Match, che Schemaic non legge. Indica invece l'indirizzo stesso come host SSH.
connections-route-include = ~/.ssh/config include un file che Schemaic non ha potuto leggere nel punto in cui si applica a { $host }. Indica l'indirizzo stesso come host SSH, e il percorso come Host di salto.
connections-route-config-jump-password = ~/.ssh/config raggiunge { $host } tramite { $jump }, e una password viene inviata solo agli host indicati da questa connessione. Aggiungi l'host di salto a Host di salto se accetta la stessa password, oppure accedi con una chiave o con l'agente SSH.
connections-route-jump-own-proxy-jump = ~/.ssh/config raggiunge l'host di salto { $host } tramite { $jump }, e Schemaic non segue il ProxyJump di un host di salto. Indica invece il percorso come Host di salto, con { $jump } per primo.
connections-jump-host-error = Host di salto { $number }: { $why }
connections-jump-empty = vuoto — rimuovi la virgola in eccesso
connections-jump-space = «{ $entry }» contiene uno spazio
connections-jump-unclosed = «{ $entry }» apre una [ che non chiude
connections-jump-after-bracket = «{ $entry }» ha del testo dopo la sua ]
connections-jump-no-host = «{ $entry }» non indica alcun host
connections-jump-not-a-port = «{ $entry }» non è una porta
connections-private-key = Chiave privata
connections-private-key-dialog = Seleziona chiave privata
connections-passphrase = Passphrase
connections-tls = SSL / TLS
tls-disable = Disattivato
tls-prefer = Preferito
tls-require = Obbligatorio
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Connetti in chiaro.
tls-prefer-description = Crittografa se il server lo offre. Nessuna verifica.
tls-require-description = Rifiuta di connetterti senza crittografia. Nessuna verifica.
tls-verify-ca-description = Crittografa e verifica il certificato rispetto alla CA.
tls-verify-full-description = Crittografa, verifica il certificato e verifica il nome host.
tls-verify-ca-checks-host = Su MySQL e MariaDB verifica anche il nome host, quindi si comporta come Verify full — un limite del driver, non un'impostazione.
tls-floor-azure-cli = L'accesso con Microsoft Entra si connette in Verify full qualunque cosa sia scelta qui: il suo token apre ogni database Azure SQL raggiungibile dall'account.
tls-floor-aws-iam = L'accesso con AWS IAM si connette almeno in Obbligatorio: il suo token non deve attraversare la rete in chiaro. Verify full richiede il bundle CA di RDS come CA radice.
tls-floor-gcp-iam = L'accesso con Google Cloud IAM si connette almeno in Verify CA — imposta come CA radice la CA del server dell'istanza: il suo token apre ogni istanza Cloud SQL raggiungibile dall'account.
tls-floor-other = Questo accesso si connette in una modalità più rigorosa di quella scelta qui.
connections-ca-certificate = Certificato CA
connections-ca-certificate-dialog = Seleziona certificato CA
connections-ca-certificate-hint = Lascia vuoto per considerare attendibili le autorità di certificazione ritenute attendibili dal tuo sistema operativo, comprese quelle installate dalla tua organizzazione. Indica un file per considerare attendibile quello invece.
connections-client-certificate = Certificato client
connections-client-certificate-dialog = Seleziona certificato client
connections-client-key = Chiave client
connections-client-key-dialog = Seleziona chiave client
connections-client-identity-hint = Un certificato e una chiave client servono solo dove il server ti chiede di dimostrare chi sei (TLS reciproco). La chiave non deve essere protetta da passphrase.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importa in { $table }
import-section-source = Origine
import-section-reading = Lettura
import-section-columns = Colonne
import-section-preview = Anteprima
import-file = File
import-choose-file = Scegli un file…
import-file-dialog = Importa nella tabella
import-no-file = Nessun file scelto
import-unnamed-file = (file)
import-format = Formato
import-sheet = Foglio
import-delimiter = Delimitatore
import-header-row = La prima riga è un'intestazione
import-header-row-hint = Usa la prima riga come nomi delle colonne invece che come dati.
import-trim = Rimuovi spazi iniziali e finali
import-trim-hint = Elimina gli spazi attorno a ogni valore — anche dentro le virgolette.
import-empty-is-null = Campo vuoto è NULL
import-empty-is-null-hint = Un campo vuoto significa NULL anziché una stringa vuota.
import-null-tokens = Altri valori che significano NULL
import-null-tokens-hint = Separati da virgole, ad es. NULL, \N, NA.
import-skip = Salta
import-nothing-mapped = Non è mappato nulla, quindi non c'è nulla da importare.
import-values-withheld = Il primo record di questo file è troppo grande per l'anteprima, quindi sono stati letti solo i nomi delle colonne. La mappatura e l'importazione leggono l'intero file.
import-missing-required = Non mappate, e non possono essere vuote: { $columns }. L'importazione non riuscirà a meno che il database non le compili.
import-engine-not-transactional = Il motore di archiviazione di questa tabella ({ $engine }) non è transazionale, quindi un'importazione non riuscita non può essere annullata — le righe caricate prima dell'errore resteranno.
import-problems = { $count ->
    [one] { $count } problema nel file — non è stato importato nulla.
   *[other] { $count } problemi nel file — non è stato importato nulla.
}
import-problem-at-line = riga { $line }: { $message }
import-problem-at-column = riga { $line }, { $column }: { $message }
import-issue-not-an-integer = non è un numero intero
import-issue-not-a-number = non è un numero
import-issue-not-a-boolean = non è un valore vero/falso
import-issue-cell-error = il foglio non ha potuto calcolare questa cella
import-issue-null-in-not-null = vuoto, ma la colonna non può essere NULL
import-issue-field-count = ha { $found } campi, previsti { $expected }
import-tail-cut = Ne vengono mostrati { $shown } su { $total }.
import-tail-uncounted = Vengono mostrati tutti i { $total } — il controllo ha smesso di contare lì, quindi il file ne contiene di più.
import-tail-cut-and-uncounted = Ne vengono mostrati { $shown } sui primi { $total } — il controllo ha smesso di contare lì, quindi il file ne contiene di più.
import-memory-json = Questo file JSON è di { $size }, e un'importazione JSON viene tenuta in memoria durante il caricamento — prevedi che richieda circa { $estimate }. Un CSV con gli stessi dati si carica con memoria costante.
import-memory-xlsx = Questa cartella di lavoro è di { $size }, e un'importazione Excel viene tenuta in memoria durante il caricamento — prevedi che richieda all'incirca { $estimate }, dato che il file è compresso. Un CSV dello stesso foglio si carica con memoria costante.
import-read-failed = Impossibile leggere il file: { $error }
import-no-columns-mapped = Nessuna colonna del file è mappata a colonne della tabella
import-preview-record-too-large = il suo primo record JSON supera i { $mib } MiB letti dall'anteprima, quindi non c'è nulla da mostrare
import-refuse-not-objects = attesi oggetti JSON (un array di oggetti, o uno per riga)
import-refuse-oversize = Questa cartella di lavoro è di { $size }, e un file Excel deve essere letto per intero prima di poterne mostrare una parte — { $max } è il massimo che si può aprire. Esporta il foglio come CSV e importa quello; un CSV si carica con memoria costante.
import-refuse-oversize-unsized = Questa cartella di lavoro supera { $max }, e un file Excel deve essere letto per intero prima di poterne mostrare una parte — { $max } è il massimo che si può aprire. Esporta il foglio come CSV e importa quello; un CSV si carica con memoria costante.
import-refuse-inflated = Questa cartella di lavoro è di { $size } su disco ma si decomprime in almeno { $unpacked }, e un file Excel deve essere letto per intero prima di poterne mostrare una parte — { $ceiling } di contenuto decompresso è il massimo che si apre, e per contenerlo serve più memoria di così. Esporta il foglio come CSV e importa quello; un CSV si carica con memoria costante.
import-refuse-no-such-sheet = questa cartella di lavoro non ha alcun foglio chiamato «{ $sheet }» — ha { $sheets }
import-refuse-no-sheets = questa cartella di lavoro non ha fogli
import-refuse-too-wide = questo foglio dichiara { $width } colonne; un foglio di lavoro Excel ne contiene { $max }. Esportalo come CSV e importa quello.
import-cancelled = Importazione annullata — la transazione è stata annullata con un rollback, quindi non è stato scritto nulla.
import-done = { $count ->
    [one] Importata { $count } riga in { $table }.
   *[other] Importate { $count } righe in { $table }.
}
import-next = Avanti
import-reading = Lettura…
import-back = Indietro
import-import = Importa
import-importing = Importazione…
dialog-read-only = Questa connessione è di sola lettura.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Esporta { $database }.{ $schema } in { $format }
dump-title = Esporta { $database } in { $format }
dump-sql-dialog = Esporta in SQL
dump-folder-dialog = Esporta in { $format }
dump-folder-hint = Un file { $format } per tabella, scritto in una cartella a tua scelta.
dump-all-selected = Tutte le { $total } selezionate
dump-some-selected = { $chosen } di { $total } selezionate
dump-select-all = Seleziona tutto
dump-select-none = Nessuna
dump-reading-tables = Lettura dell'elenco delle tabelle…
dump-no-tables = Questo database non ha tabelle.
dump-tables-unreadable = Non è stato possibile leggere l'elenco delle tabelle.
dump-section-contents = Contenuto
dump-structure = Struttura
dump-structure-hint = CREATE TABLE, i suoi trigger e le chiavi esterne, riformulate dopo i dati.
dump-data = Dati
dump-data-hint = Ogni riga, come istruzioni INSERT.
dump-other-objects = Altri oggetti
dump-other-objects-hint = Tipi, sequenze, routine ed eventi su cui si basano queste tabelle.
dump-section-replaying = Riesecuzione
dump-drop-before-create = Elimina prima di creare
dump-drop-hint-up-front = Elimina ogni tabella e vista prima del suo CREATE, così il file si carica su un database che le contiene già. Con Una transazione sostituisce anche ogni routine nei loro schemi, e si ferma prima di eliminare qualsiasi cosa che non possa ripristinare così com'è. Senza Una transazione lascia le routine come sono, quindi una riesecuzione su un database che le contiene ancora si ferma alla prima. Proprietario e permessi di ogni tabella vengono ripristinati alla fine.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS prima di ogni CREATE, così il file si carica su un database che contiene già queste tabelle. Sostituisce anche ogni routine nei loro schemi. Proprietario, permessi e sicurezza di riga di ogni tabella vengono ripristinati alla fine.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS prima di ogni CREATE, così il file si carica su un database che contiene già queste tabelle. Sostituisce anche ogni routine ed evento nei loro schemi: su MySQL ciascuno viene prima eliminato, il che revoca ogni permesso su di esso, e il file li concede di nuovo alla fine se il tuo account può leggere mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS prima di ogni CREATE, così il file si carica su un database che contiene già queste tabelle.
dump-one-transaction = Una transazione
dump-one-transaction-hint = Racchiude il caricamento, così un errore non lascia nulla dietro di sé.
dump-disable-fk-checks = Disattiva i controlli delle chiavi esterne
dump-fk-hint = Disattivati per tutta la durata del caricamento, così l'ordine in cui arrivano le righe non può farle rifiutare.
dump-fk-hint-drops = Disattivati per tutta la durata del caricamento, così l'ordine in cui arrivano le righe non può farle rifiutare. Un file che elimina tabelle li disattiva comunque: un'eliminazione con i controlli attivi cancella le righe di tabelle esterne al file tramite le loro azioni ON DELETE.
dump-no-fk-guard-hint = Questo motore non ha un interruttore di sessione per i controlli delle chiavi esterne; il file aggiunge invece i vincoli dopo i dati.
dump-export = Esporta
dump-progress = { $count ->
    [one] Scrittura di { $table } — { $index } di { $total }, { $rows } riga finora
   *[other] Scrittura di { $table } — { $index } di { $total }, { $rows } righe finora
}
dump-reading-schema = Lettura dello schema
dump-replace-files = Sostituisci file
dump-replace-prompt = { $n ->
    [one] { $n } file in { $folder } verrà sostituito, e non potrà essere recuperato: { $names }.
   *[other] { $n } file in { $folder } verranno sostituiti, e non potranno essere recuperati: { $names }.
}
dump-replace-prompt-more = { $n } file in { $folder } verranno sostituiti, e non potranno essere recuperati: { $names }, e altri { $more }.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] Scritta { $n } tabella.
   *[other] Scritte { $n } tabelle.
}
dump-done-missing = { $n ->
    [one] { $n } tabella spuntata non è stata trovata e non è nel file: { $names }.
   *[other] { $n } tabelle spuntate non sono state trovate e non sono nel file: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } oggetto richiesto dall'esportazione non è nel file: { $names }.
   *[other] { $n } oggetti richiesti dall'esportazione non sono nel file: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Le righe della tabella di archi del grafo { $names } non sono nel file; viene creata vuota.
   *[other] Le righe delle tabelle di archi del grafo { $names } non sono nel file; vengono create vuote.
}
dump-done-refused = { $n ->
    [one] Una riesecuzione su un database che contiene già questo oggetto si ferma prima di eliminare qualsiasi cosa: il file non può ripristinarlo così com'è: { $names }.
   *[other] Una riesecuzione su un database che contiene già uno di questi oggetti si ferma prima di eliminare qualsiasi cosa: il file non può ripristinarli così come sono: { $names }.
}
dump-cancelled-partial = Esportazione annullata — { $name } non è stato modificato; ciò che era stato scritto si trova in { $part }
files-done = { $n ->
    [one] Scritto { $n } file.
   *[other] Scritti { $n } file.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Esportazione annullata — nessun file è stato completato, quindi non è stato scritto nulla in { $folder }.
files-cancelled = { $n ->
    [one] Esportazione annullata — { $n } file è stato completato in { $folder }; la tabella in corso non è stata scritta.
   *[other] Esportazione annullata — { $n } file sono stati completati in { $folder }; la tabella in corso non è stata scritta.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } file già scritto in { $folder } viene conservato.
   *[other] { $message } — { $n } file già scritti in { $folder } vengono conservati.
}
files-replaced = { $n ->
    [one] { $n } file esistente è stato sostituito: { $names }.
   *[other] { $n } file esistenti sono stati sostituiti: { $names }.
}
files-missing = { $n ->
    [one] { $n } tabella spuntata non è stata trovata e non è diventata un file: { $names }.
   *[other] { $n } tabelle spuntate non sono state trovate e non sono diventate file: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } di ~{ $total } riga
   *[other] { $rows } di ~{ $total } righe
}
export-progress-of = { $count ->
    [one] { $rows } di { $total } riga
   *[other] { $rows } di { $total } righe
}
export-progress-rows = { $count ->
    [one] { $rows } riga
   *[other] { $rows } righe
}
export-progress-starting = Avvio
export-progress-file = File: { $name }
export-progress-writing = Scrittura di { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Monitoraggio in tempo reale
monitor-badge-watching = In osservazione
monitor-badge-paused = In pausa
monitor-badge-stopped = Fermato
monitor-filter-all = Tutto
monitor-filter-inserts = Inserimenti
monitor-filter-updates = Aggiornamenti
monitor-filter-deletes = Eliminazioni
monitor-resume = Riprendi il polling
monitor-pause = Sospendi il polling
monitor-clear-tip = Cancella il registro
monitor-export-tip = Esporta il registro…
monitor-interval = { $secs } s
monitor-clear-title = Cancellare il registro?
monitor-clear-message = { $count ->
    [one] { $count } modifica verrebbe scartata. Il registro è l'unica traccia di ciò che conteneva una riga eliminata — il polling non segnalerà di nuovo una modifica — e l'operazione non si può annullare. Esportalo prima se vuoi conservarlo.
   *[other] { $count } modifiche verrebbero scartate. Il registro è l'unica traccia di ciò che conteneva una riga eliminata — il polling non segnalerà di nuovo una modifica — e l'operazione non si può annullare. Esportalo prima se vuoi conservarlo.
}
monitor-col-time = Ora
monitor-col-action = Azione
monitor-no-inserts = Ancora nessun inserimento.
monitor-no-updates = Ancora nessun aggiornamento.
monitor-no-deletes = Ancora nessuna eliminazione.
monitor-empty-stopped = Fermato.
monitor-empty-paused = In pausa.
monitor-waiting = In attesa
monitor-events-since = { $count ->
    [one] { $count } evento dalle { $since }
   *[other] { $count } eventi dalle { $since }
}
monitor-no-changes = Ancora nessuna modifica
monitor-partial = vengono osservate solo le prime { $cap } righe per chiave primaria
monitor-capped = le più vecchie vengono scartate al limite di { $cap } modifiche
monitor-length-only = { $count ->
    [one] { $columns } è binaria, quindi le sue modifiche vengono rilevate solo dalla lunghezza
   *[other] { $columns } sono binarie, quindi le loro modifiche vengono rilevate solo dalla lunghezza
}
monitor-newest-at-bottom = più recenti in fondo
monitor-export-dialog = Esporta registro delle modifiche

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Confronta risultati
diff-filter-all = Tutto
diff-filter-changed = Modificate
diff-filter-added = Aggiunte
diff-filter-removed = Rimosse
diff-match-rows-by = Abbina le righe per
diff-whole-row = Riga intera
diff-export-tip = Esporta il confronto…
diff-nothing-to-export = Niente da esportare.
diff-export-dialog = Esporta confronto
diff-unnamed-file = il confronto
diff-ran-at = Eseguita alle { $time }
diff-side-facts = { $count ->
    [one] { $count } riga · { $ms } ms
   *[other] { $count } righe · { $ms } ms
}
diff-only-in = Solo in { $side }: { $columns }
diff-uncompared = Binarie, non confrontate: { $columns }
diff-missing-key = entrambi i risultati necessitano di una colonna chiamata { $column } per abbinare le righe
diff-binary-key = { $column } contiene valori binari, che non vengono confrontati, quindi non può abbinare le righe
diff-summary-identical = { $n ->
        [one] Nessuna differenza · { $n } riga invariata
       *[other] Nessuna differenza · { $n } righe invariate
    }
diff-summary-changed = { $n } modificate
diff-summary-added = { $n } aggiunte
diff-summary-removed = { $n } rimosse
diff-summary-unchanged = { $n } invariate
diff-summary-not-compared = { $columns } non confrontate
diff-repeated-keys = { $count ->
    [one] { $count } riga ripete una chiave già usata dal suo lato, ed è esclusa.
   *[other] { $count } righe ripetono una chiave già usata dal loro lato, e sono escluse.
}
diff-capped = Un risultato è stato limitato: le righe oltre il limite non vengono confrontate.
diff-col-change = Modifica
diff-no-differences = Nessuna differenza.
diff-no-changed = Nessuna riga modificata.
diff-no-added = Nessuna riga aggiunta.
diff-no-removed = Nessuna riga rimossa.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Prima di applicare
preview-risk-irreversible = L'operazione non si può annullare

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Crea la tabella { $name } con { $columns } colonna
   *[other] Crea la tabella { $name } con { $columns } colonne
}
ddl-line-rebuild-table = Ricostruisci { $name } — non può essere modificata sul posto, quindi le righe vengono copiate in una nuova tabella che prende il posto di quella vecchia
ddl-line-drop-table = Elimina la tabella
ddl-line-drop-foreign-table = Elimina la tabella esterna
ddl-line-truncate-table = Elimina ogni riga
ddl-line-rename-table = Rinomina la tabella in { $to }
ddl-line-add-column = Aggiungi la colonna { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Elimina la colonna { $name }
ddl-line-rebuild-computed-column = Ricostruisci la colonna calcolata { $name } come ({ $expression }) attorno alla modifica della colonna
ddl-line-rename-column = Rinomina la colonna { $from } in { $to }{ $retyped ->
        [yes] , e il suo tipo da { $old_type } a { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Sposta la colonna { $name } all'inizio
ddl-line-move-column-after = Sposta la colonna { $name } dopo { $after }
ddl-line-move-column = Sposta la colonna { $name }
ddl-line-retype-column = Cambia la colonna { $name } da { $from } a { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Modifica la colonna { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (nessuno)
ddl-line-add-primary-key = Aggiungi la chiave primaria ({ $columns })
ddl-line-drop-primary-key = Elimina la chiave primaria ({ $columns })
ddl-line-rebuild-primary-key = Ricostruisci la chiave primaria ({ $columns }) attorno alla modifica della colonna
ddl-line-change-primary-key = Cambia la chiave primaria da ({ $from }) a ({ $to })
ddl-line-add-index = Aggiungi l'indice { $has_kind ->
        [yes] { $kind }{" "}
       *[no] {""}
    }{ $unique ->
        [yes] univoco{" "}
       *[no] {""}
    }{ $name }{ $has_method ->
        [yes] {" "}con { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}su ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}che include ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}ordinato per ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}dove { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = columnstore cluster
ddl-index-kind-primary-xml = XML primario
ddl-index-kind-selective-xml = XML selettivo
ddl-index-kind-secondary-xml = XML secondario
ddl-index-kind-spatial = spaziale
ddl-line-drop-index = Elimina l'indice { $name }
ddl-line-rename-index = Rinomina l'indice { $from } in { $to }
ddl-line-keep-lossy-index = Lascia invariato l'indice { $name } — Schemaic non riesce a leggerlo tutto
ddl-line-add-foreign-key = Aggiungi la chiave esterna { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Elimina la chiave esterna { $name }
ddl-line-add-check = Aggiungi il controllo { $name } ({ $expression })
ddl-line-drop-check = Elimina il controllo { $name }
ddl-line-table-options = Imposta per la tabella { $parts }
ddl-table-option-engine = il motore a { $engine }
ddl-table-option-collation = la collation a { $collation }
ddl-table-option-comment = il commento
ddl-table-option-comment-cleared = il commento (cancellato)
ddl-line-create-view = Crea la vista { $name }
ddl-line-create-view-index = Crea la vista { $name } e il suo indice { $index }
ddl-line-create-view-indexes = Crea la vista { $name } e i suoi indici { $indexes }
ddl-line-redefine-view = Ridefinisci la vista { $name }
ddl-line-recreate-view = Elimina e ricrea la vista { $name }
ddl-line-rename-view = Rinomina la vista in { $to }
ddl-line-drop-view = Elimina la vista
ddl-line-drop-materialized-view = Elimina la vista materializzata
ddl-line-refresh-view = Ricostruisci le righe della vista materializzata — i lettori restano bloccati finché non termina
ddl-line-refresh-view-concurrently = Ricostruisci le righe della vista materializzata senza bloccare i lettori
ddl-line-create-trigger = Crea il trigger { $name }
ddl-line-redefine-trigger = Ridefinisci il trigger { $name }
ddl-line-recreate-trigger = Ricrea il trigger { $name }
ddl-line-recreate-trigger-as = Ricrea il trigger { $from } come { $name }
ddl-line-drop-trigger = Elimina il trigger { $name }
ddl-line-create-routine = Crea { $what } { $name }
ddl-line-redefine-routine = Ridefinisci { $what } { $name }
ddl-line-redefine-routine-as = Ridefinisci { $what } { $from } come { $name }
ddl-line-recreate-routine = Ricrea { $what } { $name }
ddl-line-recreate-routine-as = Ricrea { $what } { $from } come { $name }
ddl-line-rename-routine = Rinomina { $what } { $from } in { $to }
ddl-line-drop-routine = Elimina { $what } { $name }
ddl-line-create-event = Crea l'evento { $name }
ddl-line-alter-event = Modifica l'evento { $name }
ddl-line-alter-event-edits = Modifica l'evento { $name }: { $edits }
ddl-line-drop-event = Elimina l'evento { $name }
ddl-event-edit-rename = rinomina in { $to }
ddl-event-edit-schedule = pianificazione a { $schedule }
ddl-event-edit-status-enabled = stato ad abilitato
ddl-event-edit-status-disabled = stato a disabilitato
ddl-event-edit-status-replica = stato a disabilitato sulla replica
ddl-event-edit-preserve = conservalo dopo il completamento
ddl-event-edit-no-preserve = eliminalo dopo il completamento
ddl-event-edit-definer = definer a { $definer }
ddl-event-edit-comment = commento
ddl-event-edit-body = corpo
ddl-line-create-enum = { $values ->
    [one] Crea il tipo { $name } con { $values } valore
   *[other] Crea il tipo { $name } con { $values } valori
}
ddl-line-add-enum-value = Aggiungi il valore { $value }
ddl-line-add-enum-value-after = Aggiungi il valore { $value } dopo { $after }
ddl-line-add-enum-value-before = Aggiungi il valore { $value } prima di { $before }
ddl-line-rename-enum-value = Rinomina il valore { $from } in { $to }
ddl-line-recreate-enum = Ricrea il tipo { $name } ({ $values ->
    [one] { $values } valore
   *[other] { $values } valori
}, riconvertendo { $columns ->
    [one] { $columns } colonna
   *[other] { $columns } colonne
})
ddl-line-create-domain = Crea il dominio { $name } come { $datatype }
ddl-line-set-domain-default = Imposta il valore predefinito del dominio a { $default }
ddl-line-drop-domain-default = Elimina il valore predefinito del dominio
ddl-line-domain-not-null = Rendi il dominio NOT NULL
ddl-line-domain-nullable = Rendi il dominio nullable
ddl-line-recreate-domain = Ricrea il dominio { $name } come { $datatype } (era { $was }; riconvertendo { $columns ->
    [one] { $columns } colonna
   *[other] { $columns } colonne
})
ddl-line-create-sequence = Crea la sequenza { $name }
ddl-line-alter-sequence = Imposta per la sequenza { $edits }
ddl-sequence-edit-type = il tipo a { $datatype }
ddl-sequence-edit-increment = l'incremento a { $value }
ddl-sequence-edit-minimum = il minimo a { $value }
ddl-sequence-edit-maximum = il massimo a { $value }
ddl-sequence-edit-start = l'inizio a { $value }
ddl-sequence-edit-cache = la cache a { $value }
ddl-sequence-edit-cycling-on = il ciclo attivo
ddl-sequence-edit-cycling-off = il ciclo disattivato
ddl-sequence-edit-owner = il proprietario a { $owner }
ddl-sequence-edit-no-owner = nessun proprietario
ddl-line-restart-sequence = Riavvia la sequenza a { $value }
ddl-line-create-object = Crea { $what } { $name }
ddl-line-replace-object = Elimina { $what } { $name } e ricrealo
ddl-line-replace-object-as = Elimina { $what } { $name } e ricrealo come { $to }
ddl-line-alter-tsql-sequence = Modifica la sequenza: { $clauses }
ddl-line-rename-object = Rinomina { $what } in { $to }
ddl-line-drop-object = Elimina { $what }
ddl-line-set-object-comment = Imposta il commento di { $what }
ddl-line-clear-object-comment = Cancella il commento di { $what }
ddl-line-create-database = Crea il database { $name }
ddl-line-drop-database = Elimina il database { $name }
ddl-line-create-schema = Crea lo schema { $name }
ddl-line-drop-schema = Elimina lo schema { $name }
ddl-line-create-extension = Crea l'estensione { $name }
ddl-line-create-user = Crea l'utente { $who }
ddl-line-create-role = Crea il ruolo { $who }
ddl-line-create-login = Crea il login { $who }
ddl-line-drop-user = Elimina l'utente { $who }
ddl-line-drop-role = Elimina il ruolo { $who }
ddl-line-drop-login = Elimina il login { $who }
ddl-line-set-password = Imposta la password di { $who }
ddl-line-grant-privileges = Concedi { $privileges } a { $who }
ddl-line-revoke-privileges = Revoca { $privileges } a { $who }
ddl-privileges-none = nessun privilegio
ddl-privileges-count = { $count } privilegi
ddl-line-grant-role = Concedi { $role } a { $member }
ddl-line-revoke-role = Revoca { $role } a { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Elimina { $name } e lo ricrea, copiando ogni riga, e ripristina chiavi, indici, controlli, valori predefiniti e trigger, i commenti della tabella e delle sue colonne, e le chiavi esterne che altre tabelle hanno su di essa. Si ferma prima di iniziare se la tabella ha qualcos'altro che Schemaic non riformula — permessi, un proprietario proprio, un dipendente associato allo schema, il controllo delle versioni di sistema, un ledger, la replica, la compressione, opzioni degli indici, un indice disabilitato, una regola o un valore predefinito associati, una classificazione di riservatezza, una descrizione su una chiave, un indice, un valore predefinito, un controllo o un trigger. Le statistiche create a mano non vengono riportate.
       *[other] Elimina { $name } e lo ricrea, copiando ogni riga. Tutto ciò che riguarda la tabella e che Schemaic non ha letto non viene riportato.
    }
ddl-risk-drop-table = Elimina la tabella e ogni sua riga.
ddl-risk-drop-foreign-table = Elimina la tabella esterna: la sua definizione scompare, e le righe che legge restano dove sono, sul server esterno o nel file; una vista o una routine che la nomina da quel momento non funziona più.
ddl-risk-truncate-table = Elimina ogni riga della tabella. L'operazione non si può annullare.
ddl-risk-drop-column = Elimina la colonna { $name } e tutti i dati che contiene.
ddl-risk-rebuild-computed-column = Elimina la colonna calcolata { $name } e la riaggiunge, quindi si sposta alla fine della tabella.
ddl-risk-column-not-null = La colonna { $name } diventa NOT NULL — l'istruzione non riesce se una riga contiene NULL.
ddl-risk-column-stops-generated = La colonna { $name } smette di essere generata. I valori attuali vengono mantenuti, ma da ora nulla li ricalcola.
ddl-risk-column-becomes-generated = La colonna { $name } diventa generata da { $expression }. Ogni suo valore viene sostituito da ciò che calcola l'espressione.
ddl-risk-column-expression-changes = L'espressione della colonna { $name } cambia da { $from } a { $to }. Su PostgreSQL questo elimina e riaggiunge la colonna, quindi ogni valore viene ricalcolato e la colonna si sposta alla fine della tabella.
ddl-risk-type-narrows = Restringere { $subject } da { $from } a { $to } { $loss ->
        [truncates] tronca i valori che non rientrano più
        [rounds] arrotonda ogni valore della colonna
       *[both] tronca i valori che non rientrano più e arrotonda ogni valore della colonna
    }.
ddl-risk-type-rewrites = Cambiare { $subject } da { $from } a { $to } riscrive ogni valore; può non riuscire o perdere precisione.
ddl-risk-domain-subject = il dominio { $name }
ddl-risk-primary-key-dropped = Lascia la tabella senza chiave primaria — le righe non potranno più essere modificate dalla griglia.
ddl-risk-primary-key-rebuilt = Elimina e riaggiunge la chiave primaria attorno alla modifica della colonna. Quando è l'indice cluster, questo riscrive l'intera tabella, e una chiave esterna in un'altra tabella che vi fa riferimento blocca il piano.
ddl-risk-primary-key-replaced = Sostituisce la chiave primaria: { $count ->
        [one] { $columns } non è più univoca
       *[other] { $columns } non sono più univoche
    }, e da ora sono accettati duplicati. Su MySQL questo ricostruisce l'indice cluster, il che riscrive l'intera tabella.
ddl-risk-drop-foreign-key = Elimina la chiave esterna { $name }. Da ora sono accettate righe senza un padre corrispondente, e i dati esistenti non vengono riesaminati se la si riaggiunge.
ddl-risk-drop-unique-index = Elimina l'indice univoco { $name }. Da ora sono accettati valori duplicati, e la griglia potrebbe perdere la chiave con cui modificava le righe.
ddl-risk-drop-view = { $materialized ->
        [yes] Elimina la vista materializzata. { $cost }
       *[no] Elimina la vista. { $cost }
    }
ddl-risk-view-cost-sqlite = Le viste che selezionano da essa non vengono più risolte finché non torna, e i suoi trigger INSTEAD OF vengono eliminati con essa e non vengono ripristinati.
ddl-risk-view-cost-sqlite-restored = Le viste che selezionano da essa non vengono più risolte finché non torna, e i suoi trigger INSTEAD OF vengono eliminati con essa.
ddl-risk-view-cost-sqlserver = I permessi su di essa, la sua descrizione e i suoi trigger INSTEAD OF vengono eliminati con essa e non vengono ripristinati.
ddl-risk-view-cost-sqlserver-restored = I permessi su di essa e la sua descrizione vengono eliminati con essa e non vengono ripristinati; vengono eliminati anche i suoi trigger INSTEAD OF.
ddl-risk-view-cost-other = Viste dipendenti, regole, permessi e i suoi trigger INSTEAD OF vengono eliminati con essa e non vengono ripristinati.
ddl-risk-view-cost-other-restored = Viste dipendenti, regole e permessi vengono eliminati con essa e non vengono ripristinati; vengono eliminati anche i suoi trigger INSTEAD OF.
ddl-risk-recreate-view = Ricreare { $name } prima la elimina. { $cost } { $why ->
        [sqlite] SQLite non ha modo di sostituire una vista sul posto, quindi ogni modifica — compresa una ridenominazione — segue questa strada.
        [sqlserver] sp_rename di SQL Server lascerebbe la definizione memorizzata della vista con il nome vecchio, quindi una ridenominazione segue questa strada.
       *[postgres] PostgreSQL non può sostituire una vista le cui colonne hanno cambiato nome, tipo o ordine, quindi questo è l'unico modo per applicare la modifica.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } istruzione eliminata con essa viene ripristinata in seguito, nella stessa transazione.
       *[other] { $count } istruzioni eliminate con essa vengono ripristinate in seguito, nella stessa transazione.
    }
ddl-risk-view-indexes-dropped = Ridefinire la vista { $view } elimina { $dropped_count ->
        [one] il suo indice
       *[other] i suoi indici
    } ({ $dropped }) — ALTER VIEW di SQL Server lo fa sempre — { $case ->
        [all] e questo piano { $again_count ->
            [one] lo ricostruisce
           *[other] li ricostruisce
        } subito dopo, nella stessa transazione. Su una vista grande ciò richiede tanto tempo quanto ne è servito per { $again_count ->
            [one] costruirlo
           *[other] costruirli
        } la prima volta. Un indice con fill factor, compressione, padding, opzioni di lock, un filegroup o una descrizione blocca il piano prima di qualsiasi modifica — Schemaic non può ricostruirli. Le statistiche create a mano sulla vista non vengono riportate.
        [none] e questo piano non { $gone_count ->
            [one] lo ricostruisce
           *[other] li ricostruisce
        }: la vista a cui deve corrispondere non ha un indice del genere. Le statistiche create a mano sulla vista non vengono riportate.
       *[some] e questo piano ricostruisce { $again } subito dopo, nella stessa transazione, ma non { $gone }, che la vista a cui deve corrispondere non ha. Un indice con fill factor, compressione, padding, opzioni di lock, un filegroup o una descrizione blocca il piano prima di qualsiasi modifica — Schemaic non può ricostruirli. Le statistiche create a mano sulla vista non vengono riportate.
    }
ddl-risk-add-not-null-column = La colonna { $name } è NOT NULL senza valore predefinito. Se la tabella ha già delle righe non c'è un valore da assegnare loro, e il motore rifiuterà l'intero piano.
ddl-risk-drop-check = Elimina il controllo { $name }. Da ora sono accettate le righe che il vincolo rifiutava, e i dati esistenti non vengono riesaminati se lo si riaggiunge.
ddl-risk-keep-lossy-index = La tua modifica all'indice { $name } non è inclusa. Usa qualcosa che Schemaic non sa rileggere — una colonna chiave con espressione, una classe di operatori o un ordinamento NULLS — e applicare la modifica significherebbe ricreare l'indice senza di esso. Modifica invece questo indice in SQL.
ddl-risk-drop-trigger = Elimina il trigger { $name }. Ciò che faceva a ogni scrittura smette di accadere, e alle righe scritte da ora non verrà applicato.
ddl-risk-redefine-trigger-in-place = Ridefinisce il trigger { $name } sul posto — non viene eliminato, quindi i suoi permessi restano — e ciò che fa a ogni scrittura cambia da ora.
ddl-risk-recreate-trigger = Ricreare il trigger { $name } prima lo elimina. Dove il DDL non è transazionale (MySQL), una nuova definizione rifiutata dal server lascia la tabella senza trigger.
ddl-risk-signature-lost = { $kind ->
        [trigger] Il trigger
        [function] La funzione
       *[procedure] La procedura
    } { $name } ha una firma (ADD SIGNATURE), e qualsiasi modifica la rimuove — Schemaic non può firmarla di nuovo senza la chiave del certificato. Ciò che il certificato le consentiva di fare smette di funzionare finché non esegui di nuovo ADD SIGNATURE.
ddl-risk-redefines-routine = Ridefinisce { $name }. Tutto ciò che la chiama esegue da ora il nuovo corpo, compresi i trigger su altre tabelle che questa modifica non ha mai menzionato.
ddl-risk-routine-parameters-change = Cambia l'elenco dei parametri di { $name }. I chiamanti che passano i vecchi argomenti non vi vengono più risolti.
ddl-risk-recreate-routine = Ricreare { $held } prima la elimina. Dove il DDL non è transazionale (MySQL), una nuova definizione rifiutata dal server non lascia alcuna { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] I privilegi concessi su { $held } e il suo commento non sopravvivono all'eliminazione, e questo piano non li ripristina — riapplica in seguito ogni GRANT e COMMENT.
       *[no] I privilegi concessi su { $held } non sopravvivono all'eliminazione, e questo piano non li ripristina — riapplica in seguito ogni GRANT.
    }
ddl-risk-drop-numbered-group = Elimina { $what } { $name } e con essa ogni membro del suo gruppo numerato: { $members }.
ddl-risk-drop-routine = Elimina { $what } { $name }. PostgreSQL la rifiuta finché un trigger la usa ancora, quindi quelli che lo fanno vanno eliminati prima.
ddl-risk-drop-event = Elimina l'evento { $name }. La sua pianificazione e il suo corpo scompaiono con esso — nient'altro nel database ne conserva una copia.
ddl-risk-one-shot-event = Questo evento viene eseguito una volta e poi il server lo elimina — è ON COMPLETION NOT PRESERVE. Attiva Conserva per mantenerlo.
ddl-risk-one-shot-event-altered = L'evento { $name } verrà eseguito una volta e poi il server lo eliminerà — è ON COMPLETION NOT PRESERVE. Attiva Conserva per mantenerlo.
ddl-risk-add-enum-value = L'aggiunta di { $value } non può essere annullata sul posto — PostgreSQL non può rimuovere un valore di enum, quindi tornare indietro significa ricreare il tipo. Inoltre il valore non può essere usato finché questo piano non viene applicato.
ddl-risk-rename-enum-value = Ogni riga che contiene { $from } da ora legge { $to }. Se intendevi rimuovere { $from } anziché rinominarlo, eliminalo e applica la modifica da sola.
ddl-risk-recreate = Ricreare il { $kind ->
        [domain] dominio
       *[type] tipo
    } { $name } lo elimina e lo ricostruisce. Ogni colonna qui sotto viene riconvertita passando per il testo; un valore che la nuova definizione non accetta fa fallire l'intero piano, e non viene applicato nulla{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}e altre { $more }
        }. Una vista o una funzione basata su di esso non può essere elencata qui e farebbe rifiutare il server.
       *[no] . Oggi nulla lo usa, ma una vista o una funzione basata su di esso non può essere elencata qui e farebbe rifiutare il server.
    }
ddl-risk-drop-domain-check = Elimina il controllo { $name }. Ogni colonna di questo dominio smette di essere controllata, e i dati esistenti non vengono riesaminati se lo si riaggiunge.
ddl-risk-domain-nullable = Ogni colonna di questo dominio inizia ad accettare NULL, comprese quelle in tabelle che questa modifica non ha mai menzionato.
ddl-risk-domain-not-null = L'istruzione non riesce se una colonna di questo dominio contiene già NULL.
ddl-risk-drop-sequence = Elimina la sequenza e la posizione che aveva raggiunto. Il server la rifiuta finché il valore predefinito di una colonna attinge ancora da essa, quindi quei valori predefiniti vanno rimossi prima.
ddl-risk-drop-synonym = Elimina il sinonimo. Nulla lo impedisce: una vista, una routine o una query che lo nomina non funzionerà più, alla sua prossima esecuzione.
ddl-risk-drop-extension = Elimina l'estensione e le funzioni, i tipi e gli operatori che ha installato. Il server la rifiuta finché qualcosa di tuo dipende ancora da essi — una colonna del suo tipo, un indice o un server sul suo wrapper — e indica cosa.
ddl-risk-drop-used-object = Elimina { $what }. Il server lo rifiuta finché una colonna lo usa ancora, quindi quelle colonne devono prima cambiare tipo.
ddl-risk-restart-sequence = Riavvia il contatore a { $value }. I valori già distribuiti non cambiano, quindi una chiave che raggiunge di nuovo entra in conflitto.
ddl-risk-replace-synonym = Elimina il sinonimo e lo ricrea. Nulla impedisce l'eliminazione: le due istruzioni vengono eseguite in un'unica transazione.
ddl-risk-replace-sequence = Elimina la sequenza e la ricrea: la posizione raggiunta va persa, e ricomincia da { $start }. Il server rifiuta l'eliminazione finché il valore predefinito di una colonna attinge ancora da essa.
ddl-risk-start-value = il suo valore iniziale
ddl-risk-replace-object = Elimina { $what } e lo ricrea. Il server rifiuta l'eliminazione finché una colonna lo usa ancora, quindi quelle colonne devono prima cambiare tipo.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Rifiutato finché ha permessi su di sé, un proprietario proprio, proprietà estese o una regola o un valore predefinito associati: la ricreazione non li ripristinerebbe.
       *[no] Rifiutato finché ha permessi su di sé, un proprietario proprio o proprietà estese: la ricreazione non li ripristinerebbe.
    }
ddl-risk-drop-database = Elimina il database { $name } e tutto ciò che contiene — ogni tabella con tutte le sue righe, ogni vista, indice, routine e trigger. Non viene conservato nulla e non si può annullare: l'unico modo per tornare indietro è un backup eseguito prima di questa operazione.
ddl-risk-drop-schema = Elimina lo schema { $name }. Il server lo rifiuta finché contiene ancora tabelle, viste o routine, quindi questo accetta uno schema vuoto oppure non riesce{ $cascade ->
        [yes] {" "}— Schemaic non invia mai CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Elimina il login { $who }, e con esso ogni permesso del server e ogni appartenenza a ruoli del server che detiene — non registrati altrove, quindi un login ricreato con questo nome parte senza di essi. Il server lo rifiuta finché qualcosa è connesso con esso, quindi le sue sessioni devono prima terminare. I suoi utenti in ciascun database non vengono eliminati: restano, con i loro permessi, non associati ad alcun login, e un nuovo login con questo nome non si ricollega a essi senza ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Elimina { $who } e ogni privilegio che detiene. I permessi non sono registrati altrove, quindi ripristinare l'account significa concederli tutti di nuovo a memoria. { $cuts_off ->
        [yes] Qualsiasi cosa connessa con esso non riesce alla sua prossima istruzione in questo database.
       *[no] Qualsiasi cosa ancora connessa con esso continua a funzionare finché non si disconnette.
    }
ddl-risk-set-password = Cambia la password di { $who }. Quella vecchia non può essere recuperata, quindi tutto ciò che è ancora configurato con essa — un'applicazione, un job, il client di un altro sviluppatore — non riesce a connettersi finché non viene aggiornato a mano. Le sessioni già aperte continuano a funzionare.
ddl-risk-revoke-privileges = Toglie { $privileges } a { $who }. Tutto ciò che vi fa affidamento — un'applicazione, un job, una vista di proprietà di qualcun altro — inizia a non riuscire alla sua prossima istruzione.{ $cascades ->
        [yes] {" "}La revoca include CASCADE, quindi viene tolto anche a ogni account a cui { $who } lo ha concesso, e ciò che viene eseguito come quegli account non riesce allo stesso modo.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Dove invece a { $who } è negato (DENY), la revoca rimuove il divieto — il che restituisce l'accesso se un ruolo di cui fa parte lo concede.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}Su una tabella elimina anche ogni concessione e DENY a livello di colonna di { $cleared } che { $who } ha sulle colonne della tabella: una colonna negata a { $who } non è più negata, e una concessa a sé non è più concessa.
       *[no] {""}
    }
ddl-risk-revoke-role = Toglie il ruolo { $role } a { $member }, e con esso ogni privilegio che il ruolo comporta.
ddl-risk-blank-password = { $who } non avrà password: chiunque possa raggiungere il server può accedere con esso. Impostane una qui, o in seguito con ALTER USER.
ddl-risk-host-is-everywhere = Il suo host è % — ogni computer della rete, non solo questo.
ddl-risk-grant-server-wide = Concede { $privileges } a { $who } sull'intero server — l'istanza stessa, non il database che stai esplorando.
ddl-risk-grant-every-database = Concede { $privileges } a { $who } su ogni database di questo server — compresi i database che non esistono ancora, non solo quello che stai esplorando.
ddl-risk-grant-keeps-column-denies = Una concessione di { $privileges } a livello di tabella elimina ogni DENY di esso che { $who } ha su una delle colonne della tabella, quindi l'istruzione nega di nuovo quelle colonne a { $who } subito dopo la concessione: una colonna negata a { $who } resta negata. Dove { $who } detiene anche l'opzione di concessione sulla tabella, il server rifiuta di negarla di nuovo senza CASCADE, e non viene applicato nulla.
ddl-risk-star-refresh = Aggiorna ogni vista e funzione inline di questo database che esegue select * da { $table }, direttamente o tramite un sinonimo: SQL Server le associa alle colonne della tabella per posizione, e altrimenti leggerebbero e scriverebbero le colonne sbagliate. Il piano non riesce se una di esse non si compila più con le nuove colonne. Una in un altro database che esegue select * da { $table } viene aggiornata dopo il commit del piano, fuori dalla sua transazione: fino ad allora legge le vecchie posizioni, e se il suo aggiornamento non riesce il piano resta applicato e la vista viene indicata qui.
ddl-risk-piecewise = SQL Server non crea, modifica né elimina un modulo compilato in modo nativo all'interno di una transazione, quindi questo piano esegue un'istruzione alla volta e non può essere fermato: se un'istruzione non riesce, quelle precedenti restano applicate.
ddl-risk-drop-sequence-table = Elimina la sequenza: il valore raggiunto va perso, e tutto ciò che attinge ancora da essa non riesce.
ddl-risk-drop-scale = Secondo le statistiche dell'albero dello schema, sono { $rows } righe.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = la proposta riguarda la tabella `{ $proposed }`, ma è aperta `{ $actual }`
propose-no-ops = la proposta non chiede alcuna modifica
propose-no-such-column = `{ $table }` non ha alcuna colonna chiamata `{ $name }`
propose-column-exists = `{ $table }` ha già una colonna chiamata `{ $name }`
propose-no-such-index = `{ $table }` non ha alcun indice chiamato `{ $name }`
propose-index-exists = `{ $table }` ha già un indice chiamato `{ $name }`
propose-no-such-foreign-key = `{ $table }` non ha alcuna chiave esterna chiamata `{ $name }`
propose-no-such-check = `{ $table }` non ha alcun vincolo CHECK chiamato `{ $name }`
propose-key-width-mismatch = una chiave esterna necessita di tante colonne di riferimento quante sono quelle che fanno riferimento
propose-empty-index = un indice necessita di almeno una colonna
propose-invalid = la tabella proposta non sarebbe valida: { $problem }
propose-not-a-table = `{ $name }` è una vista — una proposta può modificare solo una tabella
propose-no-such-table = nessuna tabella chiamata `{ $name }` — chiama list_schema per vedere cosa esiste
propose-not-an-expression = { $field ->
        [default] il valore predefinito `{ $text }` non è una singola espressione SQL
        [generated] l'espressione di generazione `{ $text }` non è una singola espressione SQL
       *[check] l'espressione di controllo `{ $text }` non è una singola espressione SQL
    }
propose-not-a-type = `{ $text }` non è un tipo di colonna — indica solo il tipo, senza valore predefinito, senza NOT NULL e senza nulla dopo
propose-unknown-action = `{ $text }` non è un'azione di chiave esterna
propose-not-loaded = { $table } non è caricata in { $database } in questo momento — apri il database nell'albero dello schema, o attendi il termine di un aggiornamento, e riprova.
propose-nothing-to-change = { $subject } è già così — non c'è nulla da modificare.
propose-no-database = Questa scheda non ha alcun database selezionato, quindi una modifica dello schema applicata da qui non avrebbe dove andare. Scegline prima uno per questa connessione — se tutti i database sono nascosti, l'occhio di SCHEMA è il punto in cui riportarne uno.
propose-other-connection = Questa modifica riguarda una scheda su un'altra connessione — passa a quella scheda, o seleziona la sua connessione nell'albero, prima di applicare una modifica dello schema.
ai-run-no-database = Questa scheda non ha alcun database selezionato, quindi un'istruzione eseguita da qui non avrebbe dove andare. Scegline prima uno per questa connessione — se tutti i database sono nascosti, l'occhio di SCHEMA è il punto in cui riportarne uno.
ai-run-other-connection = Questa chat riguarda una scheda su un'altra connessione. Una nuova scheda si aprirebbe sulla connessione selezionata nell'albero, quindi l'istruzione verrebbe eseguita altrove — passa prima a quella scheda, o seleziona la sua connessione nell'albero.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, che questo piano rinomina in { $to }
ddl-moved-dropped = { $name }, che questo piano elimina
ddl-withheld-unreplayable-rename = { $count ->
        [one] Rinominare { $was } in { $to } non può portare con sé il suo trigger: il suo SQL nomina { $was }, e riscriverlo qui cambierebbe ciò che hai scritto. Rinomina la vista da sola dopo aver ricreato il trigger con il nuovo nome.
       *[other] Rinominare { $was } in { $to } non può portare con sé i suoi { $count } trigger: il loro SQL nomina { $was }, e riscriverlo qui cambierebbe ciò che hai scritto. Rinomina la vista da sola dopo aver ricreato i trigger con il nuovo nome.
    }
ddl-withheld-numbered-group = { $name } è a capo di un gruppo di procedure numerate ({ $members }), e questo piano lo elimina prima, il che elimina con esso ogni membro del gruppo. Modificalo con il suo nome e tipo, il che lo altera sul posto e li conserva.
ddl-withheld-view-index-create = { $count ->
        [one] La creazione della vista { $view } non può creare il suo indice { $indexes } — di un tipo che Schemaic non sa riformulare. Crea invece questa vista in SQL.
       *[other] La creazione della vista { $view } non può creare i suoi indici { $indexes } — di un tipo che Schemaic non sa riformulare. Crea invece questa vista in SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] La modifica della vista { $view } elimina il suo indice { $indexes }, che Schemaic non può ricreare — di un tipo che Schemaic non sa riformulare. Modifica invece questa vista in SQL.
       *[other] La modifica della vista { $view } elimina i suoi indici { $indexes }, che Schemaic non può ricreare — di un tipo che Schemaic non sa riformulare. Modifica invece questa vista in SQL.
    }
ddl-withheld-tsql-lossy-index = Ricostruire { $table } eliminerebbe l'indice { $index }: è di un tipo che Schemaic non legge per intero
ddl-withheld-tsql-encrypted-trigger = Ricostruire { $table } eliminerebbe il trigger { $trigger }: il suo testo è crittografato
ddl-withheld-tsql-signed-trigger = Ricostruire { $table } eliminerebbe la firma sul trigger { $trigger }, che non può essere ripristinata senza la chiave privata del certificato
ddl-withheld-tsql-renamed-trigger = Ricostruire { $table } con un nuovo nome lascerebbe il trigger { $trigger } con il nome vecchio
ddl-withheld-partly-read = L'indice { $index } è stato letto solo in parte — SQLite conserva il predicato di un indice parziale, e una chiave con espressione, solo nel testo CREATE dell'indice stesso{ $why ->
        [may-name] , e quel testo potrebbe nominare { $moved }. Elimina l'indice per procedere senza di esso.
        [not-found] , quindi non può essere creato da ciò che è stato letto. Elimina l'indice per procedere senza di esso.
        [edited] , quindi questa modifica non può essere scritta — ciò che è stato letto non è l'intero indice. Annulla la modifica per mantenere l'indice com'è, oppure eliminalo.
       *[no-text] {" "}— e per questo non esiste un testo del genere, quindi ricrearlo cambierebbe ciò che indicizza. Elimina l'indice per procedere senza di esso.
    }
ddl-withheld-condition-names-dropped = La condizione dell'indice { $index } nomina { $column }, che questo piano elimina. Modifica la condizione o elimina prima l'indice.
ddl-withheld-strands-triggers = Questa tabella ha { $count ->
        [one] { $count } trigger
       *[other] { $count } trigger
    }, e il loro SQL viene rieseguito esattamente come SQLite l'ha memorizzato — il che nominerebbe ancora { $moved }. SQLite accetta un trigger del genere e poi rifiuta ogni scrittura nella tabella. Elimina o modifica prima il trigger, oppure rinomina la colonna senza altre modifiche, così che l'ALTER TABLE di SQLite possa riscriverlo.
ddl-withheld-strands-other-trigger = { $owner } nomina { $moved }. Una ricostruzione rinomina questa tabella sotto quel trigger e ne lascia l'SQL esattamente com'era, quindi il trigger continuerebbe a nominare una colonna che non c'è più — SQLite accetta un trigger del genere e poi rifiuta ogni scrittura in *quella* tabella. Elimina o modifica prima il trigger, oppure rinomina la colonna senza altre modifiche, così che l'ALTER TABLE di SQLite possa riscriverlo.
ddl-withheld-trigger-on = Il trigger { $name } su { $table }
ddl-withheld-some-trigger = Un trigger su un'altra tabella
ddl-withheld-strands-generated-column = La colonna generata { $column } viene calcolata da { $moved }. Una ricostruzione scrive la tabella dal modello, e nulla riscrive quell'espressione — SQLite rifiuta la nuova tabella con "no such column" e l'intero piano viene annullato. Modifica o rimuovi prima { $column }.
ddl-withheld-cannot-restate = La dichiarazione di questa tabella contiene { $clauses }, per cui il modello di Schemaic non ha alcun campo. Una ricostruzione riscrive la tabella da quel modello, quindi la clausola scomparirebbe da una tabella che sembrerebbe ancora corretta — e ogni scrittura che ne dipende si comporterebbe diversamente da quel momento. Modifica invece questa tabella con uno script (Copia DDL ne è l'inizio), oppure rimuovi prima la clausola.
ddl-sqlite-clauses = { $first } e { $last }
ddl-sqlite-clause-deferrable = la clausola DEFERRABLE di una chiave esterna
ddl-sqlite-clause-on-conflict = una clausola ON CONFLICT
ddl-sqlite-clause-desc-key = una colonna di chiave primaria DESC
ddl-sqlite-clause-collate-key = un COLLATE su una colonna di chiave primaria
ddl-withheld-virtual-table = `{ $name }` è una tabella virtuale. Le sue colonne sono l'interfaccia di un modulo e non una dichiarazione di archiviazione, e i suoi dati reali risiedono in tabelle ombra che una ricostruzione eliminerebbe — un indice FTS5 scomparirebbe da una tabella che conterrebbe ancora il testo. Modificala invece con uno script (DROP e CREATE VIRTUAL TABLE), oppure modifica le tabelle che indicizza.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } vista
   *[other] { $count } viste
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } routine
   *[other] { $count } routine
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } trigger
   *[other] { $count } trigger
}
ddl-used-by-count-event = { $count ->
    [one] { $count } evento
   *[other] { $count } eventi
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } chiave esterna
   *[other] { $count } chiavi esterne
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } espressione di tabella
   *[other] { $count } espressioni di tabella
}
ddl-used-by-counts = { $first } e { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}e altri { $more }
    }
ddl-used-by-drop = Usato da { $listing }.
ddl-used-by-change = { $subject } è usato da { $listing }. { $then ->
        [renamed] Dopo la ridenominazione continuano a nominare { $name }.
        [dropped] Smettono di funzionare quando viene eliminato.
       *[retyped] Verificali rispetto al nuovo tipo.
    }
ddl-used-by-column = La colonna { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Le chiavi esterne tra queste tabelle formano un ciclo, quindi nessun ordine di creazione le soddisfa tutte. Un'istruzione verrà rifiutata perché fa riferimento a una tabella che non esiste ancora, e né MySQL né MariaDB annullano il DDL con un rollback.
ddl-risk-clash = { $holder } contiene ancora { $names }, necessario per creare { $claimer } — { $why ->
        [not-dropped] e questo piano non la elimina
        [default] e non può essere eliminata prima perché il valore predefinito di una colonna attinge da essa
       *[referenced] e non può essere eliminata prima perché un'altra tabella vi fa riferimento
    }. { $cost ->
        [no-rollback] Un'istruzione verrà rifiutata, e né MySQL né MariaDB annullano il DDL con un rollback.
        [rolls-back] Un'istruzione verrà rifiutata, il che annulla l'intera migrazione.
       *[refused] Un'istruzione verrà rifiutata.
    }

preview-title = Applica modifiche a { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } modifica
   *[other] { $count } modifiche
}
preview-used-by = Usato da
preview-withheld = Questo motore non può esprimere parte di questo piano
preview-withheld-hint = Non viene applicato nulla finché questo è elencato.
preview-omitted = Non incluso in questa migrazione
preview-omitted-hint = L'applicazione lascia questi oggetti come sono.
preview-copy = Copia
preview-open-in-editor = Apri nell'editor
preview-back = Indietro
preview-apply = Applica
preview-applying = Applicazione…
preview-applied = { $count ->
    [one] Applicata { $count } istruzione a { $subject }.
   *[other] Applicate { $count } istruzioni a { $subject }.
}
preview-refreshed = Lo schema è stato aggiornato.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Importa connessioni
conn-import-url = URL di connessione
conn-import-add = Aggiungi
conn-import-url-hint = Un URL di connessione, un URL JDBC o una riga DATABASE_URL presa da un .env — uno per riga.
conn-import-choose-file = Scegli un file…
conn-import-scan = Cerca nei client installati
conn-import-scanning = Ricerca…
conn-import-docker = Container Docker
conn-import-asking-docker = Interrogazione di Docker…
conn-import-looking = Ricerca di connessioni su questo computer…
conn-import-none-found = Nessuna connessione trovata. DBeaver, gli IDE JetBrains e i client da riga di comando tengono i loro file in posizioni note; se il tuo è altrove, sceglilo qui sopra.
conn-import-nothing-yet = Ancora niente — incolla un URL, leggi un file o cerca i client che usi già.
conn-import-found = { $count ->
    [one] { $count } connessione trovata
   *[other] { $count } connessioni trovate
}
conn-import-select-all = Seleziona tutto
conn-import-select-none = Nessuna
conn-import-note-no-password = Nessuna password nell'origine
conn-import-note-already-saved = Già salvata
conn-import-note-unexpanded-path = Il percorso contiene una macro non espansa
conn-import-note-pgpass = Password presa dal tuo .pgpass
conn-import-note-port-assumed = L'origine non indicava una porta; questa è quella predefinita
conn-import-note-external-login = Accede in un modo che Schemaic non supporta: imposta un login SQL
conn-import-skipped = { $count ->
    [one] { $count } voce non è stata importata: { $names }.
   *[other] { $count } voci non sono state importate: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } voce non è stata importata: { $names }, e altre { $rest }.
   *[other] { $count } voci non sono state importate: { $names }, e altre { $rest }.
}
conn-import-reason-unsupported-engine = motore non supportato ({ $driver })
conn-import-reason-no-server = nessun server a cui connettersi
conn-import-reason-not-published = la sua porta non è pubblicata su questo computer
conn-import-reason-unreadable = impossibile leggerla ({ $error })
conn-import-reason-not-an-export = impossibile leggerla (non è un'esportazione di connessioni di Schemaic: { $error })
conn-import-reason-newer-export = impossibile leggerla (esportata da una versione più recente di Schemaic (versione { $version }) — aggiorna per importarla)
conn-import-url-empty = Incolla un URL di connessione.
conn-import-url-no-scheme = Nessuno schema — un URL inizia con mysql://, postgresql://, sqlserver:// o sqlite:, una stringa di connessione SQL Server con Server=.
conn-import-url-unknown-scheme = Schema «{ $scheme }» sconosciuto — previsto mysql, mariadb, postgresql, sqlserver o sqlite.
conn-import-url-no-host = Nessun host nell'URL.
conn-import-url-bad-port = «{ $port }» non è un numero di porta.
conn-import-url-no-path = Nessun file di database dopo sqlite:.
conn-import-url-transport = «{ $server }» si raggiunge tramite named pipe o LocalDB — Schemaic si connette solo tramite TCP.
conn-import-url-userinfo = Il nome del server contiene una @ — un nome utente o una password vanno nel proprio campo, non nel server.
conn-import-url-not-sql-server = Questa stringa di connessione non indica di essere di SQL Server — i driver .NET di MySQL e PostgreSQL usano le stesse parole chiave. Incolla invece l'URL del server (mysql://, postgresql://, sqlserver://), oppure, per SQL Server, scrivi il server come tcp:host.
conn-import-row-overflow = Vengono mostrate le prime { $shown } su { $total }. Le altre { $hidden } vengono comunque importate se spuntate — questo elenco si ferma qui perché la finestra resti reattiva.
conn-import-import = Importa
conn-import-import-count = Importa { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = Immagine PNG
blob-kind-jpeg = Immagine JPEG
blob-kind-gif = Immagine GIF
blob-kind-bmp = Immagine BMP
blob-kind-webp = Immagine WebP
blob-kind-ico = Icona
blob-kind-opaque = Dati binari
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · vengono mostrati i primi { $shown }
blob-pane-preview = Anteprima
blob-pane-hex = Hex
blob-reading = Lettura…
blob-empty = Questa cella è NULL, oppure la sua riga non esiste più.
blob-preview-too-large = Questa immagine è di { $width } × { $height } — troppo grande per l'anteprima qui. I byte sono intatti: leggili in Hex, oppure salvali in un file.
blob-preview-truncated = Sono stati letti solo i primi { $shown } { $key ->
    [png] di questa immagine PNG, quindi non può essere disegnata
    [jpeg] di questa immagine JPEG, quindi non può essere disegnata
    [gif] di questa immagine GIF, quindi non può essere disegnata
    [bmp] di questa immagine BMP, quindi non può essere disegnata
    [webp] di questa immagine WebP, quindi non può essere disegnata
    [ico] di questa icona, quindi non può essere disegnata
   *[other] di questo valore, quindi non può essere disegnato
}. Leggi i byte in Hex.
blob-preview-unreadable = Questi byte iniziano come { $key ->
    [png] un'immagine PNG
    [jpeg] un'immagine JPEG
    [gif] un'immagine GIF
    [bmp] un'immagine BMP
    [webp] un'immagine WebP
    [ico] un'icona
   *[other] un'immagine
} ma non possono essere letti come tale. Leggili in Hex, oppure salvali in un file.
blob-save-truncated = È stato possibile leggere solo i primi { $shown } di { $size }, quindi verrebbe salvata solo la parte iniziale del valore.
blob-load = Carica da file
blob-save = Salva in file
blob-load-dialog = Carica valore binario da file
blob-save-dialog = Salva valore binario
blob-saved-to = Salvato in { $path }
blob-loaded-file = File caricato. Esegui il commit nella griglia per scriverlo.
blob-not-writable = Questa cella non può essere scritta.
blob-file-too-large = Quel file è di { $got } — questa colonna contiene al massimo { $most }.
blob-not-accepting = Quella cella non accetta più un valore — non è stato messo in sospeso nulla.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = Diagramma ER
erd-scope = Ambito
erd-chip-tables = { $count ->
    [one] { $count } tabella
   *[other] { $count } tabelle
}
erd-chip-relationships = { $count ->
    [one] { $count } relazione
   *[other] { $count } relazioni
}
erd-chip-hidden = { $count ->
    [one] { $count } non correlata nascosta
   *[other] { $count } non correlate nascoste
}
erd-find-placeholder = Trova tabella o colonna
erd-find-none = Nessuna corrispondenza
erd-find-matches = { $hits ->
    [one] { $hits } corrispondenza
   *[other] { $hits } corrispondenze
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } corrispondenza in { $tables } tabelle
   *[other] { $hits } corrispondenze in { $tables } tabelle
}
erd-more-columns = +{ $count } altre
erd-show-less = mostra meno
erd-not-loaded = Lo schema di questo database non è ancora caricato.
erd-no-relationships = Nessuna relazione di chiave esterna da rappresentare.
erd-export-tip = Esporta il diagramma…
erd-copy-as = Copia come
erd-export-dialog = Esporta diagramma
erd-nothing-to-export = Niente da esportare.
erd-nothing-to-copy = Niente da copiare.
erd-copied-as = Copiato come { $format }
erd-clipboard-unavailable = Gli appunti non sono disponibili.
erd-copy-failed = Impossibile copiare: { $error }
erd-saved = Salvato { $name }
erd-unnamed-file = il file

## Server variables modal (server_vars_view.rs)

server-vars-title = Variabili del server — { $connection }
server-vars-search = Cerca impostazioni
server-vars-variables = Variabili
server-vars-status = Stato
server-vars-loading = Caricamento…
server-vars-unsupported = Il motore di questa connessione non pubblica un elenco di questo tipo.
server-vars-no-match = Nessuna impostazione corrispondente.
server-vars-more = { $count ->
    [one] Corrisponde ancora { $count } impostazione — restringi la ricerca
   *[other] Corrispondono altre { $count } impostazioni — restringi la ricerca
}
server-vars-count = { $count ->
    [one] { $count } impostazione
   *[other] { $count } impostazioni
}
server-vars-count-filtered = { $shown } di { $count } impostazioni
server-vars-refresh = Aggiorna

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Modifica snippet — { $name }
snippet-edit-name = Nome
snippet-edit-name-placeholder = Nome dello snippet
snippet-edit-shortcut = Abbreviazione di espansione
snippet-edit-shortcut-none = nessuna
snippet-edit-query = Query
snippet-edit-save = Salva

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Piano della query
plan-analyze-unavailable = Analyze non disponibile (l'istruzione scrive dati)
plan-analyze-hint = Esegue l'istruzione per misurare i tempi reali
plan-explaining = Analisi del piano
plan-potential-issues = Possibili problemi
plan-detail-no-index = nessun indice usato
plan-detail-key-unused = indice non usato per la ricerca
plan-my-full-scan = Scansione completa della tabella `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Scansione completa del risultato ({ $detail }, type = ALL)
plan-my-filesort = Filesort su `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort sul risultato (Extra: Using filesort)
plan-my-temporary = Tabella temporanea per `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Tabella temporanea per il risultato (Extra: Using temporary)
plan-pg-seq-scan = Scansione sequenziale su `{ $table }` (nessun indice usato)
plan-pg-seq-scan-unnamed = Scansione sequenziale su una tabella (nessun indice usato)
plan-pg-sort = Nodo Sort — la query ordina le righe al volo
plan-ms-full-scan = Scansione completa di `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Scansione completa di una tabella ({ $op })
plan-ms-sort = Sort — la query ordina le righe al volo
plan-ms-spool = { $op } — una tabella di lavoro intermedia in tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = nessun predicato di join — ogni riga viene abbinata a ogni riga
plan-server-no-statistics = una colonna che legge non ha statistiche
plan-server-spill = riversato in tempdb — la sua concessione di memoria era troppo piccola
plan-server-convert = una conversione di tipo influisce sul piano ({ $issue }): { $expression }
plan-server-unmatched-indexes = non è stato possibile usare un indice filtrato
plan-server-memory-grant = la sua concessione di memoria è stata stimata male
plan-server-wait = ha atteso { $wait }
plan-server-other = il server avvisa: { $name }
plan-missing-index = Indice mancante su `{ $table }` ({ $keys })
plan-missing-index-include = Indice mancante su `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Indice mancante su `{ $table }` ({ $keys }) — il server prevedeva un miglioramento del { $impact }%
plan-missing-index-include-impact = Indice mancante su `{ $table }` ({ $keys }; include { $include }) — il server prevedeva un miglioramento del { $impact }%
plan-ask-ai = Chiedi all'IA
plan-ms-operation = Operazione
plan-ms-object = Oggetto
plan-ms-estimated-rows = Righe stimate
plan-ms-estimated-cost = Costo stimato
plan-ms-actual-rows = Righe effettive
plan-ms-executions = Esecuzioni
plan-ms-statement = Istruzione { $n }: { $kind }
plan-ms-statement-in-procedure = Istruzione { $n }: { $kind } nella procedura { $name }
plan-ms-statement-in-function = Istruzione { $n }: { $kind } nella funzione { $name }
plan-ms-statement-in-a-procedure = Istruzione { $n }: { $kind } in una procedura
plan-ms-statement-in-a-function = Istruzione { $n }: { $kind } in una funzione
plan-ms-plan = Piano
plan-ms-no-plan = Nessuna istruzione in questo batch ha un piano.
plan-ms-unread = Impossibile leggere il piano: { $reason }
plan-ms-unread-not-xml = non è un documento XML
plan-ms-unread-not-showplan = previsto ShowPlanXML, trovato { $found }
plan-ms-unread-empty = un documento vuoto

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Usato da — { $object }
used-by-copy = Copia
used-by-not-loaded = Lo schema di { $database } non è ancora stato caricato — espandilo nell'albero, poi chiedi di nuovo.
used-by-nothing-in = Nulla in { $database } lo usa.
used-by-nothing = Nulla lo usa.
used-by-limits = Trovati leggendo le definizioni in questo database: l'SQL costruito in fase di esecuzione e gli altri database non vengono visti.
used-by-dropped-with-it = { $detail } · eliminato insieme
used-by-heading-view = { $count ->
    [one] Vista ({ $count })
   *[other] Viste ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Routine ({ $count })
   *[other] Routine ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Trigger ({ $count })
}
used-by-heading-event = { $count ->
    [one] Evento ({ $count })
   *[other] Eventi ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Chiave esterna ({ $count })
   *[other] Chiavi esterne ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Espressione di tabella ({ $count })
   *[other] Espressioni di tabella ({ $count })
}
used-by-noun-view = vista
used-by-noun-routine = routine
used-by-noun-trigger = trigger
used-by-noun-event = evento
used-by-noun-foreign-key = chiave esterna
used-by-noun-expression = espressione di tabella

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Scuro
theme-light = Chiaro

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = CODICE
md-code-copy = Copia
md-code-insert = Inserisci
md-code-run = Esegui
md-proposal-title = Modifica proposta a { $table }
md-proposal-count = { $count ->
    [one] { $count } modifica
   *[other] { $count } modifiche
}
md-proposal-review = Esamina la modifica…
md-proposal-unreadable = L'assistente ha proposto una modifica che non è stato possibile leggere — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Scegli…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Adesso
cell-calendar-today = Oggi
cell-month-1 = Gennaio
cell-month-2 = Febbraio
cell-month-3 = Marzo
cell-month-4 = Aprile
cell-month-5 = Maggio
cell-month-6 = Giugno
cell-month-7 = Luglio
cell-month-8 = Agosto
cell-month-9 = Settembre
cell-month-10 = Ottobre
cell-month-11 = Novembre
cell-month-12 = Dicembre
cell-weekday-mon = Lu
cell-weekday-tue = Ma
cell-weekday-wed = Me
cell-weekday-thu = Gi
cell-weekday-fri = Ve
cell-weekday-sat = Sa
cell-weekday-sun = Do

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Rinomina alias
alias-rename-title-cte = Rinomina CTE
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = Nome della CTE
alias-rename-new-name = Nuovo nome
alias-rename-rename = Rinomina

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Qui non c'è più nulla da rinominare.
alias-rename-empty = Assegna un nome.
alias-rename-conflict = «{ $name }» è già un nome in questa query: rinominare con esso cambierebbe la tabella letta da un riferimento.
alias-rename-unsure = Un riferimento qui potrebbe leggere un'altra colonna con questo nome, o leggere questa dove la ridenominazione non può seguirla, e lo schema caricato non può stabilirlo — quindi non viene apportata alcuna modifica. Carica lo schema delle tabelle, oppure rinomina a mano.
alias-rename-changed = La query è cambiata mentre questa finestra era aperta. Chiudila e rinomina di nuovo.
alias-rename-read-only = L'editor è di sola lettura finché il suggerimento dell'IA non viene accettato o rifiutato.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } utilizzo di { $name }
   *[other] { $count } utilizzi di { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = SVG non valido: { $error }
raster-too-large = L'immagine è troppo grande per essere rasterizzata ({ $width }×{ $height })
raster-png-failed = Codifica PNG non riuscita: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Copia
grid-menu-copy-value = Copia valore
grid-menu-copy-formatted = Copia formattato
grid-menu-copy-as = Copia come
grid-menu-paste = Incolla
grid-menu-fill-down = Riempi verso il basso
grid-menu-fill-up = Riempi verso l'alto
grid-menu-edit-field = Modifica campo
grid-menu-edit-row = Modifica riga
grid-menu-view-row = Visualizza riga
grid-menu-edit-binary = Modifica binario
grid-menu-view-binary = Visualizza binario
grid-menu-filter-by-value = Filtra per questo valore
grid-menu-exclude-value = Escludi questo valore
grid-menu-follow-relation = Segui la relazione
grid-menu-referenced-by = Referenziata da
grid-menu-set-null = Imposta a NULL
grid-menu-set-default = Imposta a DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Duplica riga
   *[other] Duplica { $count } righe
}
grid-menu-delete-rows = { $count ->
    [one] Elimina riga
   *[other] Elimina { $count } righe
}
grid-menu-undo-delete = Annulla eliminazione
grid-menu-ai-summary = Riepilogo IA
grid-attach-to-chat = Allega { $scope } alla chat
grid-attach-scope-rows = { $rows ->
    [one] { $rows } riga
   *[other] { $rows } righe
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } colonna
   *[other] { $cols } colonne
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } riga
   *[other] { $rows } righe
} e { $cols ->
    [one] { $cols } colonna
   *[other] { $cols } colonne
}
grid-menu-freeze = Blocca
grid-menu-unfreeze = Sblocca
grid-menu-hide-column = Nascondi colonna
grid-menu-show-hidden-columns = Mostra colonne nascoste ({ $count })
grid-menu-show-all = Mostra tutto
grid-menu-format-as = Formatta come
grid-format-none = Nessuno
grid-format-timestamp = Timestamp
grid-format-grouped = Separatore delle migliaia
grid-format-bytes = Dimensione file
grid-format-bool = Booleano
grid-menu-copy-name = Copia nome
grid-menu-copy-qualified-name = Copia nome qualificato
grid-menu-copy-values = Copia valori
grid-menu-sql-in-list = Elenco SQL IN

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
    [one] { $read } riga
   *[other] { $read } righe
}
grid-rows-read-capped = { $n ->
    [one] { $read } riga
   *[other] { $read } righe
} (limitate)
grid-rows-read-of-total = { $n ->
    [one] { $read } di { $total } riga
   *[other] { $read } di { $total } righe
}
grid-kept-note = · conservato — sola lettura
grid-arena-note = · { $columns } troppo grandi per essere contenute per intero — le righe successive appaiono vuote
grid-sorted-subset = · sottoinsieme ordinato (limitato) — non l'ordine completo
grid-read-all = leggi tutte le righe
grid-read-more = leggi { $rows } righe
grid-reading = lettura…
grid-committing = Commit in corso…
grid-commit-tip = { $count ->
    [one] Conferma { $count } modifica ({ $keys })
   *[other] Conferma { $count } modifiche ({ $keys })
}
grid-discard-tip = Scarta tutte le modifiche in sospeso
grid-copy-sql-tip = Copia le modifiche come SQL
grid-add-row-tip = Aggiungi una riga
grid-delete-row-tip = Contrassegna la riga selezionata per l'eliminazione ({ $key })
grid-clone-row-tip = Clona la riga selezionata
grid-copy-results-tip = Copia i risultati…
grid-copy-results-empty = Niente da copiare — nessuna riga
grid-save-results-tip = Salva i risultati in un file…
grid-save-results-empty = Niente da salvare — nessuna riga
grid-ai-tip = Genera righe con l'IA…
grid-ai-generating = Generazione…
grid-ai-off = Le azioni IA inviano dati — disattivate per questa connessione
grid-ai-fill-value = Riempi valore con l'IA
grid-ai-insert-row = Inserisci riga con l'IA
grid-ai-seed-table = Popola tabella con l'IA
grid-chart-tip = Crea un grafico dei risultati
grid-chart-empty = Niente da rappresentare — nessuna riga
grid-chart-this-result = questo risultato

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Visualizza
grid-rollback-tab = Rollback di { $tab }
grid-find-placeholder = Trova nei risultati
grid-figure-copied = Copiato
grid-figure-rows = { $n ->
    [one] { $n }{ $more } riga
   *[other] { $n }{ $more } righe
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } distinti
grid-figure-sum = Somma { $value }
grid-figure-avg = Media { $value }
grid-figure-median = Mediana { $value }
grid-figure-min = Min { $value }
grid-figure-max = Max { $value }
grid-figure-length = { $n ->
    [one] Lunghezza { $n } car.
   *[other] Lunghezza { $n } car.
}
grid-figure-length-range = Lunghezza { $lo }–{ $hi } car.
grid-goto-label = Vai alla riga:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = gli appunti
export-dest-file = il file
export-note-rows = { $n ->
    [one] Esportata { $rows } riga in { $name }
   *[other] Esportate { $rows } righe in { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } contiene byte che questo formato non può trasportare: quelle celle sono vuote
   *[other] { $cols } contengono byte che questo formato non può trasportare: quelle celle sono vuote
}
export-note-blanked = { $cols } troppo grandi per essere contenute per intero: le righe successive sono vuote
export-note-cut = { $cols } troncate al limite di { $limit } caratteri per cella di Excel
export-failure-partial = { $message } — { $name } non è stato modificato; le righe scritte si trovano in { $part }
export-cancelled-partial = Esportazione annullata — { $name } non è stato modificato; le righe scritte si trovano in { $part }
export-cancelled-nothing = Esportazione annullata — { $name } non è stato modificato.
copy-sql-as-null = { $n ->
    [one] Copiato negli appunti — { $cols } contiene byte che il testo SQL non può trasportare: scritti come NULL
   *[other] Copiato negli appunti — { $cols } contengono byte che il testo SQL non può trasportare: scritti come NULL
}
copy-sql-nothing = { $n ->
    [one] Non è stato copiato nulla — { $cols } contiene byte che il testo SQL non può trasportare
   *[other] Non è stato copiato nulla — { $cols } contengono byte che il testo SQL non può trasportare
}
copy-sql-left-out = { $n ->
    [one] Copiato negli appunti — { $cols } contiene byte che il testo SQL non può trasportare: quei valori sono esclusi
   *[other] Copiato negli appunti — { $cols } contengono byte che il testo SQL non può trasportare: quei valori sono esclusi
}

grid-fill-nothing = Niente da riempire: le colonne selezionate non possono essere scritte, oppure la cella da copiare è il valore non impostato di una nuova riga o un DEFAULT in sospeso.
grid-paste-outside = { $n } fuori dalla griglia
grid-paste-read-only = { $n } in colonne di sola lettura
grid-paste-capped = { $n } oltre il limite di incolla di { $cap } celle
grid-paste-deleted = { $n } in righe contrassegnate per l'eliminazione
grid-paste-nothing = Non è stato incollato nulla: { $notes }.
grid-paste-partial = { $count ->
        [one] Incollata { $cells } cella, saltando { $notes }.
       *[other] Incollate { $cells } celle, saltando { $notes }.
    }
grid-row-not-editable = La colonna `{ $column }` non è modificabile.
grid-row-not-nullable = La colonna `{ $column }` non può essere NULL.
grid-fill-skipped = { $count ->
    [one] Riempita { $filled } cella, saltandone { $skipped } in righe contrassegnate per l'eliminazione.
   *[other] Riempite { $filled } celle, saltandone { $skipped } in righe contrassegnate per l'eliminazione.
}
grid-attach-summary = { $rows ->
    [one] { $rows } riga
   *[other] { $rows } righe
} × { $cols ->
    [one] { $cols } colonna
   *[other] { $cols } colonne
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } riga
   *[other] { $rows } righe
} × { $cols ->
    [one] { $cols } colonna
   *[other] { $cols } colonne
} da { $table }
grid-attach-summary-capped = { $summary } (le prime { $taken } di { $total } selezionate)
grid-copy-sql-unset = Niente da copiare come SQL — le celle selezionate della nuova riga non hanno ancora un valore, e prendono il valore predefinito del server.
grid-copy-sql-joined = Niente da copiare come SQL: queste righe provengono da una query su più tabelle.
grid-copy-sql-no-keyed-table = Niente da copiare come SQL: le righe devono provenire da un'unica tabella con una chiave.
grid-copy-sql-key-too-large = Niente da copiare come SQL: la chiave era troppo grande perché questo risultato la contenesse per intero, quindi alcune righe non possono essere identificate da essa.
grid-copy-sql-pinned = Niente da copiare come SQL: questo risultato è fissato, quindi le sue righe sono un'istantanea.
grid-copy-sql-read-only = Niente da copiare come SQL: questa connessione è di sola lettura.
grid-copy-update-too-large = Niente da copiare come UPDATE: le colonne selezionate erano troppo grandi perché questo risultato le contenesse per intero, e alcune delle loro celle appaiono vuote.
grid-copy-update-nothing-writable = Niente da copiare come UPDATE: nessuna delle colonne selezionate può essere scritta.
grid-copy-where-in-no-key = Niente da copiare come WHERE IN: questa tabella non ha una chiave primaria o univoca con cui identificare le righe.
grid-copy-new-row-no-key = Niente da copiare: una nuova riga non ha chiave finché non viene salvata.
grid-export-fetched-rows = Righe lette ({ $count })
grid-export-dialog-title = Esporta risultati
grid-export-already-running = È già in corso un'esportazione — non è stato scritto nulla.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Riga { $row }
grid-row-title-in-table = Riga { $row }  ·  { $table }
grid-row-set-value = Imposta valore
grid-row-set-null = Imposta NULL
grid-row-edit-binary = Modifica
grid-row-view-binary = Visualizza
grid-row-invalid-json = JSON non valido: { $error }
grid-row-saving = Salvataggio

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } riga interessata · { $ms } ms
   *[other] { $count } righe interessate · { $ms } ms
}
grid-rerun-would-write = Impossibile rieseguire questa istruzione — scriverebbe nel database
grid-filter-not-simple = Impossibile filtrare questa query — non è una semplice SELECT su una sola tabella
grid-filter-bracket-in-name = qui non è possibile filtrare un nome che contiene `]`
grid-filter-trailing-input = input inatteso dopo la condizione
grid-staged-requery = Conferma o scarta prima le modifiche — rileggere le righe le eliminerebbe
grid-staged-landing = Sono state messe in sospeso delle modifiche mentre la tabella veniva riletta, quindi sono state mantenute e la tabella non è stata aggiornata — confermale o scartale, poi aggiorna.
grid-reread-without-filter = Il filtro chiama una funzione che Schemaic non riesegue senza che venga chiesto, quindi dopo il commit la tabella è stata riletta senza filtro e ordinamento.
grid-hidden-columns-tip = { $count ->
    [one] { $count } colonna nascosta — fai clic per mostrare tutto, o fai clic destro su un'intestazione per scegliere
   *[other] { $count } colonne nascoste — fai clic per mostrare tutto, o fai clic destro su un'intestazione per scegliere
}
grid-copied-staged-sql = Modifiche in sospeso copiate come SQL.
grid-copy-split = { $n ->
        [one] { $n } cella copiata contiene una tabulazione o un a capo — un incolla la leggerà come più di una cella.
       *[other] { $n } celle copiate contengono una tabulazione o un a capo — un incolla leggerà ciascuna di esse come più di una cella.
    }
grid-duplicate-title = Duplica righe
grid-duplicate-question = { $n ->
    [one] Duplicare { $rows } riga? Viene messa in sospeso come riga in attesa e non viene scritto nulla finché non esegui il commit.
   *[other] Duplicare { $rows } righe? Vengono messe in sospeso come righe in attesa e non viene scritto nulla finché non esegui il commit.
}
grid-nullable-tip = Ammette NULL
grid-cell-auto = <auto>
grid-cell-required = <obbligatorio>
grid-seed-title = Popola righe
grid-seed-generate = Genera

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Schema
footer-panel-ai = Assistente IA
footer-panel-history = Cronologia query
footer-panel-snippets = Libreria di snippet
footer-panel-activity = Attività del server
footer-panel-terminal = Terminale
footer-panel-narrow = La finestra è troppo stretta per questo pannello — allargala per far tornare il pannello.
footer-cursor = Riga { $line }, col { $col }
footer-indent-spaces = Spazi: { $width }
footer-indent-tabs = Tabulazioni: { $width }
footer-spaces = Spazi
footer-tabs = Tabulazioni
footer-wrap = A capo
footer-no-wrap = Senza a capo
footer-read-only = Sola lettura
footer-write-mode = Modalità scrittura
footer-tx-auto = Commit automatico
footer-tx-manual = Manuale
footer-tx-open = Aperta · { $count }
footer-tx-aborted = Transazione interrotta — esegui il rollback per continuare
footer-tx-lost = Transazione persa
footer-commit = Commit
footer-rollback = Rollback
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Query { $n }
lib-result-default-label = Risultato { $n }
lib-ai-off-note = L'assistente IA è disattivato. Scegli una CLI dell'agente nelle impostazioni IA per attivarlo.
lib-test-testing = Prova in corso…
lib-test-connected = Connesso
lib-test-failed = Connessione non riuscita

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Minimo
lib-effort-low = Basso
lib-effort-medium = Medio
lib-effort-high = Alto
lib-effort-extra = Extra
lib-effort-max = Massimo
lib-schema-scope-active = Solo il database attivo
lib-schema-scope-all = Tutti i database
lib-schema-scope-none = Nessuno
lib-cursor-block = Blocco
lib-cursor-bar = Barra
lib-cursor-underline = Sottolineato

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Nessuna connessione
header-new-connection = Nuova connessione
header-find-anywhere-tip = Cerca ovunque ({ $keys })
header-shortcuts-tip = Scorciatoie da tastiera
header-settings-tip = Impostazioni
header-disconnected = Disconnesso
header-retry = Riprova
colour-red = Rosso
colour-orange = Arancione
colour-amber = Ambra
colour-green = Verde
colour-teal = Verde acqua
colour-blue = Blu
colour-purple = Viola
colour-pink = Rosa

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = «{ $name }» è cambiato sul disco.
disk-reload = Ricarica
disk-keep-mine = Mantieni la mia versione
results-run-hint = Esegui una query  ({ $keys })
results-failed = Istruzione non riuscita.
results-cancelled = Query annullata.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = { $count } fissati · { $size }
results-pinned-tip = I risultati fissati restano in memoria finché non li chiudi. Chiudi quelli che non ti servono più per liberarla.
results-chip-nothing-run = In questa scheda non è ancora stato eseguito nulla.
results-chip-kept = conservato — sola lettura
results-menu-pin = Fissa
results-menu-unpin = Sblocca
results-menu-close = Chiudi
results-menu-compare-with = Confronta con { $result }
results-menu-close-others = Chiudi gli altri risultati
results-menu-close-all = Chiudi tutti i risultati

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Cerca ovunque

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Nessuna scelta disponibile

widgets-spinner-verbs = Rimuginare, Ponderare, Meditare, Elucubrare, Almanaccare, Arzigogolare, Fantasticare, Bighellonare, Gironzolare, Zigzagare, Ciondolare, Svolazzare, Frullare, Mescolare, Rimestare, Impastare, Lievitare, Infornare, Sobbollire, Marinare, Insaporire, Mantecare, Spadellare, Flambare, Caramellare, Glassare, Guarnire, Distillare, Fermentare, Macerare, Decantare, Cogitare, Congetturare, Calcolare, Elaborare, Escogitare, Architettare, Orchestrare, Armonizzare, Comporre, Improvvisare, Canticchiare, Fischiettare, Tamburellare, Scarabocchiare, Abbozzare, Cesellare, Forgiare, Plasmare, Intrecciare, Sbrogliare, Dipanare, Decifrare, Districare, Ingegnarsi, Arrovellarsi, Lambiccarsi, Baloccarsi, Gingillarsi, Trastullarsi, Sgambettare, Saltellare, Piroettare, Volteggiare, Levitare, Orbitare, Germogliare, Sbocciare, Covare, Cristallizzare, Evaporare, Sublimare, Turbinare, Vorticare, Ondeggiare, Serpeggiare, Zampettare, Sgattaiolare, Trafficare, Armeggiare, Smanettare, Spippolare, Pasticciare, Tergiversare, Divagare, Vagabondare, Sognare, Immaginare, Ideare, Intuire, Dedurre, Sintetizzare, Trasmutare, Lucidare, Spolverare, Sferruzzare, Ricamare, Imbastire, Rammendare, Ruminare

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Chiedi aiuto all'assistente IA.
cmdk-accept = Accetta
cmdk-reject = Rifiuta
cmdk-no-changes = Nessuna modifica suggerita
cmdk-diff-summary = { $hunks ->
        [one] { $n } blocco · { $removed } − / { $added } +
       *[other] { $n } blocchi · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = valore
params-kind-tip = Testo, numero, NULL o SQL grezzo — fai clic per cambiare
params-missing = Nessun valore per { $names }
params-not-a-number = :{ $name } è un numero, ma contiene «{ $text }»

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Visualizza
editor-run-anyway = Esegui comunque
guard-read-only = Connessione di sola lettura.
guard-read-only-refused = Connessione di sola lettura: { $reason }
read-only-several = è consentita una sola istruzione
read-only-not-a-read = sono consentite solo query in sola lettura ({ $heads })
read-only-denied = `{ $word }` non è consentito in una query in sola lettura
read-only-advances-sequence = `NEXT VALUE FOR` fa avanzare una sequenza, cosa che una query in sola lettura non fa
read-only-unlisted-call = `{ $name }` non è una funzione nota come di sola lettura, quindi non è consentita in una query in sola lettura; vengono rifiutate anche le funzioni delle estensioni o definite nel database
read-only-locks = `{ $clause }` blocca le righe che legge, cosa che una query in sola lettura non fa; rimuovi la clausola di lock
guard-no-database = Nessun database selezionato.
guard-writes-one = Questa istruzione modifica dati.
guard-writes-several = Queste istruzioni modificano dati.
guard-script = Eseguire ogni istruzione di { $file }? Uno script può creare, modificare ed eliminare oggetti, e ciò che modifica non può essere annullato da qui.
guard-staged = { $n ->
    [one] Questa esecuzione sostituisce il risultato che contiene { $n } modifica in sospeso — eseguirla la scarterebbe. Confermala o scartala prima, oppure esegui comunque.
   *[other] Questa esecuzione sostituisce il risultato che contiene { $n } modifiche in sospeso — eseguirla le scarterebbe. Confermale o scartale prima, oppure esegui comunque.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL rimuove ogni riga della tabella.
guard-truncate = TRUNCATE rimuove ogni riga della tabella.
guard-no-where = Rilevata un'istruzione { $kind } senza clausola WHERE.
guard-drop-table = DROP TABLE elimina la tabella e ogni sua riga.
guard-drop-database = DROP DATABASE elimina il database e ogni sua tabella.
guard-drop-schema = DROP SCHEMA elimina lo schema e ogni sua tabella.
guard-drop-owned = DROP OWNED elimina tutto ciò che il ruolo possiede, con ogni tabella e le sue righe.
guard-drop-cascade = DROP { $obj } … CASCADE elimina ogni colonna che ne dipende, con i suoi valori.
guard-replace-table = CREATE OR REPLACE TABLE elimina la tabella esistente e ogni sua riga.
guard-replace-database = CREATE OR REPLACE DATABASE elimina il database esistente e ogni sua tabella.
guard-replace-schema = CREATE OR REPLACE SCHEMA elimina lo schema esistente e ogni sua tabella.
guard-truncate-partition = TRUNCATE PARTITION rimuove ogni riga delle partizioni che indica.
guard-drop-partition = DROP PARTITION elimina la partizione e ogni sua riga.
quickinfo-table = tabella { $name } · { $database }
quickinfo-view = vista { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } colonna
       *[other] { $n } colonne
    }
quickinfo-primary-key = chiave primaria
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto increment
quickinfo-default = predefinito { $value }
quickinfo-generated = generata come { $expression }
quickinfo-function = funzione { $name }({ $arguments })
quickinfo-function-returns = funzione { $name }({ $arguments }) restituisce { $returns }
quickinfo-procedure = procedura { $name }({ $arguments })
quickinfo-procedure-returns = procedura { $name }({ $arguments }) restituisce { $returns }
completion-foreign-key = chiave esterna
completion-expand-star = espandi *
completion-columns = { $count ->
        [one] { $n } colonna
       *[other] { $n } colonne
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Errore di sintassi
diag-syntax-detail = Errore di sintassi: { $detail }
diag-reserved-alias-backticks = `{ $word }` è una parola chiave riservata e non può essere usata come alias (racchiudila tra backtick)
diag-reserved-alias-double-quotes = `{ $word }` è una parola chiave riservata e non può essere usata come alias (racchiudila tra virgolette doppie)
diag-reserved-alias-brackets = `{ $word }` è una parola chiave riservata e non può essere usata come alias (racchiudila tra parentesi quadre)
diag-table-not-found = Tabella `{ $table }` non trovata{ $within ->
        [some] {" "}in `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— intendevi `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = Colonna `{ $column }` non trovata{ $within ->
        [some] {" "}in `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— intendevi `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = Qui `{ $name }` ha l'alias `{ $alias }` — qualifica con `{ $alias }`
diag-ambiguous-column = La colonna `{ $column }` è ambigua
diag-join-without-condition = Il JOIN non ha una condizione ON/USING — questo è un cross join; usa CROSS JOIN se è voluto
diag-comma-join-without-where = Join con virgola senza WHERE — questo è un cross join; aggiungi un WHERE, oppure usa CROSS JOIN se è voluto
diag-not-grouped = La colonna `{ $column }` deve comparire in GROUP BY o essere usata in un'aggregazione
diag-insert-row-width = INSERT indica { $columns } { $columns ->
        [one] colonna
       *[other] colonne
    } ma questa riga ha { $values } { $values ->
        [one] valore
       *[other] valori
    }
diag-row-width-without-list = `{ $table }` accetta { $needed } { $needed ->
        [one] valore
       *[other] valori
    } senza un elenco di colonne, ma questa riga ne ha { $values }
diag-row-width-of-table = `{ $table }` ha { $columns } { $columns ->
        [one] colonna
       *[other] colonne
    } ma questa riga ha { $values } { $values ->
        [one] valore
       *[other] valori
    }
diag-nullable-not-in = `{ $column }` può essere NULL, e `NOT IN` non corrisponde ad alcuna riga appena la sottoquery ne restituisce uno — aggiungi `WHERE { $column } IS NOT NULL` alla sottoquery, oppure usa NOT EXISTS
diag-update-without-where = UPDATE senza WHERE modifica ogni riga della tabella
diag-delete-without-where = DELETE senza WHERE rimuove ogni riga della tabella
diag-null-comparison = Un confronto con NULL non è mai vero — usa { $fix }
diag-misspelled-keyword = `{ $word }` sembra una parola chiave scritta male
diag-misspelled-function = `{ $word }` sembra una funzione scritta male
diag-flow-merge-needs-semicolon = `MERGE` deve terminare con `;`
diag-flow-if-without-statement = `IF` non ha alcuna istruzione da eseguire
diag-flow-else-without-statement = `ELSE` non ha alcuna istruzione da eseguire
diag-flow-while-without-statement = `WHILE` non ha alcuna istruzione da eseguire
diag-flow-end-try-without-begin-try = `END TRY` senza un `BEGIN TRY` corrispondente
diag-flow-end-catch-without-begin-catch = `END CATCH` senza un `BEGIN CATCH` corrispondente
diag-flow-end-without-begin = `END` senza un `BEGIN` corrispondente
diag-flow-throw-needs-semicolon = L'istruzione prima di `THROW` deve terminare con `;`
diag-flow-end-try-without-catch = `END TRY` deve essere seguito da `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` non ha alcuna istruzione prima del suo `ELSE`
diag-flow-else-without-if = `ELSE` senza un `IF` che lo preceda
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` deve seguire un `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` non ha un `END TRY` corrispondente
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` non ha un `END CATCH` corrispondente
diag-flow-begin-without-end = `BEGIN` non ha un `END` corrispondente

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Chiedi all'IA
editor-menu-explain = Spiega
editor-menu-optimise = Ottimizza
editor-menu-plan = Piano
editor-menu-copy-single-line = Copia su una riga
editor-menu-make-single-line = Riduci a una riga
editor-menu-format-statement = Formatta istruzione
editor-menu-find-usages = Trova utilizzi
editor-menu-create-view = Crea vista
cmdk-optimise-label = Ottimizza questa query
editor-several-carets = Più cursori e nessuna selezione: seleziona cosa eseguire, oppure lascia un solo cursore

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Esegui l'istruzione corrente
editor-run-everything = Esegui tutto

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Trova
find-invalid = Non valida
find-bad-pattern = Non è un pattern valido
find-bad-pattern-why = Non è un pattern valido: { $why }
find-match-case = Maiuscole/minuscole
find-whole-word = Parola intera
find-regex = Espressione regolare
find-scope = Sel
find-in-selection-tip = Trova nella selezione
find-replace-placeholder = Sostituisci
find-replace = Sostituisci
find-replace-all = Tutto
goto-line-label = Vai a:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Nessun client { $client } trovato (PATH o WSL).
cli-no-client-on-path = Nessun client { $client } trovato nel PATH.
cli-no-tool = Nessun { $tool } trovato nel PATH.
cli-no-either-tool = Nessun { $first } o { $second } trovato nel PATH.
cli-no-either-client = Nessun client { $first } o { $second } trovato nel PATH.
cli-sqlcmd-windows-in-wsl = L'accesso con Windows richiede un sqlcmd per Windows, e quello trovato si trova in WSL. Installa sqlcmd su Windows, oppure apri la connessione in una scheda di query.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = Il client trovato nel tuo PATH è uno shim .cmd/.bat, che Windows esegue tramite cmd.exe — dove un nome di database che contiene '&' o '|' diventerebbe un secondo comando. Indica nelle Impostazioni l'eseguibile reale, oppure apri invece una scheda di query.
cli-refused-name-equals = Il nome di questo database contiene '=', che psql leggerebbe come una stringa di connessione, seguendola verso un altro server. Aprilo invece in una scheda di query.
cli-refused-name-uri = Il nome di questo database inizia come un URI di connessione, che psql seguirebbe verso un altro server. Aprilo invece in una scheda di query.
cli-refused-file-dash = Il nome di questo file di database inizia con '-', che il client sqlite3 leggerebbe come un'opzione anziché come il file. Rinominalo, oppure indicane un percorso (./name.db), e aprilo di nuovo.
cli-refused-sqlcmd-entra = A sqlcmd non si può passare il token di accesso dell'Azure CLI, quindi questa connessione non può essere aperta in esso. Aprila invece in una scheda di query.
cli-refused-sqlcmd-host = L'host di questa connessione contiene un carattere che sqlcmd legge come parte dell'indirizzo (',', ';', un secondo '\' o un prefisso ':'). Imposta la porta nel suo campo, e aprila di nuovo.
cli-refused-sqlcmd-ca = sqlcmd non può effettuare la verifica rispetto a un file CA. Aggiungi la CA all'archivio di attendibilità del sistema e svuota il campo CA della connessione, oppure apri la connessione in una scheda di query.
cli-refused-sqlcmd-client-cert = SQL Server non accetta l'accesso con un certificato client, quindi sqlcmd non ha dove inviare quello di questa connessione. Rimuovi il certificato e la chiave client, e aprila di nuovo.
cli-refused-wsl-paths = I percorsi dei certificati TLS di questa connessione sono percorsi Windows, che il client WSL non può aprire. Installa un client nativo, oppure sposta i certificati in WSL.
cli-refused-tunnelled-verify = Questa connessione verifica il certificato del server e lo raggiunge tramite un tunnel SSH, quindi al client verrebbe detto di connettersi a 127.0.0.1 e verificare il certificato rispetto a quell'indirizzo — cosa che il certificato del server non può soddisfare. Apri invece una scheda di query, oppure abbassa la modalità TLS per la sessione CLI.
cli-refused-not-container-id = docker ps ha stampato qualcosa che non è un id di container.
cli-refused-file-line-break = Quel nome di file contiene un a capo, che il client leggerebbe come due argomenti. Scegli un altro nome.
cli-refused-role = Il ruolo nell'SQL di inizializzazione di questa connessione non può essere passato allo strumento così com'è.
cli-refused-mysql-source-path = Il client mysql leggerebbe il percorso di quel file come comandi propri (contiene un ';' o un '\', o uno spazio a un'estremità). Spostalo o rinominalo, poi ripristinalo.
cli-refused-cloud-token = Questa connessione accede tramite il token di una CLI cloud, che non viene passato a un client esterno. Aprila invece in una scheda di query.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Impossibile stabilire da dove viene eseguito Schemaic: { $error }
install-registry-open = Impossibile aprire il tuo ambiente utente nel registro: { $error }
install-path-read = Impossibile leggere il tuo PATH utente: { $error }
install-path-write = Impossibile scrivere il tuo PATH utente: { $error }
install-look-at = Impossibile esaminare { $path }: { $error }
install-read-link = Impossibile leggere il collegamento in { $path }: { $error }
install-create = Impossibile creare { $path }: { $error }
install-remove-old = Impossibile rimuovere il vecchio { $path }: { $error }
install-remove = Impossibile rimuovere { $path }: { $error }
install-path-is-windows = Una voce nel PATH utente è un'installazione Windows; su questa piattaforma si crea invece un collegamento.
install-link-is-unix = Il collegamento del comando è un'installazione macOS e Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Impossibile stabilire in quale cartella si trova { $exe }.
install-refuse-no-shim = Accanto a questa copia di Schemaic, in { $dir }, non c'è alcun schemaic.com, quindi non c'è alcun comando da console da mettere nel PATH. L'app installata ne ha uno; una build di sviluppo no.
install-refuse-semicolon = Questa copia di Schemaic si trova in { $dir }, il cui nome contiene un ';' — il PATH lo leggerebbe come due cartelle diverse. Spostala in un percorso senza e riprova.
install-refuse-exe-replaced = { $exe } è stato sostituito mentre Schemaic era in esecuzione, quindi un collegamento a esso non punterebbe a nulla. Riavvia Schemaic, poi esegui di nuovo Installa.
install-refuse-no-home = Non esiste una cartella home in cui mettere il collegamento, quindi non c'è dove cercare.
install-refuse-path-not-text = Il tuo PATH utente non è memorizzato come testo nel registro, quindi è stato lasciato com'è.
install-refuse-path-not-unicode = Il tuo PATH utente contiene caratteri che non possono essere riletti in modo sicuro, quindi è stato lasciato com'è.
install-refuse-portable = Questa è una copia portabile, quindi Rimuovi non può stabilire se { $dir } è stata aggiunta al tuo PATH da Installa o se c'era già con altri programmi. Toglila da Variabili d'ambiente se l'ha aggiunta Installa.
install-refuse-spot-taken = { $link } esiste già e non è un collegamento, quindi Installa l'ha lasciato com'è. Spostalo altrove e riprova.
install-refuse-points-elsewhere = { $link } punta già a { $to }. Rimuovilo se è una vecchia copia di Schemaic, poi riprova.
install-refuse-unlink-elsewhere = { $link } punta a { $to }, non a questa copia di Schemaic, quindi Rimuovi l'ha lasciato com'è.
install-refuse-unlink-not-ours = { $link } non è un collegamento creato da Schemaic, quindi Rimuovi l'ha lasciato com'è.
install-report-already = Il comando schemaic è già nel tuo PATH, in { $dir }.
install-report-added = { $dir } aggiunta al tuo PATH utente. Apri un nuovo terminale per usare schemaic — uno già aperto mantiene il vecchio PATH.
install-report-linked = Creato il collegamento { $link } a { $target }.
install-report-linked-if-not-found = Creato il collegamento { $link } a { $target }. Se un nuovo terminale non trova schemaic, aggiungi { $dir } al PATH nel profilo della shell.
install-report-linked-not-on-path = Creato il collegamento { $link } a { $target }. { $dir } non è ancora nel tuo PATH — aggiungilo nel profilo della shell, poi apri un nuovo terminale.
install-report-removed = { $dir } rimossa dal tuo PATH utente. Un terminale già aperto mantiene il vecchio PATH.
install-report-path-absent = { $dir } non è nel tuo PATH utente, quindi non c'era nulla da rimuovere.
install-report-unlinked = { $link } rimosso.
install-report-link-absent = { $link } non esiste, quindi non c'era nulla da rimuovere.
install-report-package = Il comando schemaic in { $dir } è arrivato con il pacchetto installato, non da Installa — rimuovi il pacchetto per rimuoverlo.
install-hint-adds-folder = Esegui Schemaic da un terminale per nome. Aggiunge la sua cartella al tuo PATH utente.
install-hint-links = Esegui Schemaic da un terminale per nome. Crea un collegamento in ~/.local/bin.
install-hint-folder-on-path = Esegui Schemaic da un terminale per nome. La sua cartella, { $dir }, è già nel tuo PATH utente.
install-hint-comes-with-app = Esegui Schemaic da un terminale per nome. Il comando schemaic è incluso nell'app, in { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = Il tunnel SSH non è ancora stabilito
tunnel-could-not-open = Non è stato possibile aprire il tunnel SSH: { $error }. Aggiorna lo schema per riprovare.
tunnel-server-failed = Impossibile connettersi tramite il tunnel SSH: { $error }. Aggiorna lo schema per riprovare.

gate-plan-not-connected = Non connesso — il piano non è stato eseguito. Il server non ha risposto al controllo di stato, quindi non gli è stato inviato nulla.
gate-plan-connection-changed = Hai cambiato connessione mentre Schemaic verificava che fosse raggiungibile, quindi il piano non è stato eseguito. Torna indietro e chiedi di nuovo.
gate-plan-tab-moved-on = Hai cambiato scheda mentre la connessione veniva ricontrollata, quindi il piano non è stato eseguito. Descrive la scheda da cui è stato chiesto — torna a quella scheda e chiedi di nuovo.
gate-connection-changed = Non eseguito su { $name } — hai cambiato connessione mentre Schemaic verificava che fosse raggiungibile. Torna indietro e riprova.
gate-not-connected = Non connesso a { $name }. Il server non ha risposto — verifica che sia in esecuzione e che le impostazioni di questa connessione siano corrette.
gate-this-connection = questa connessione
gate-run-tab-moved-on = Hai cambiato scheda mentre la connessione veniva ricontrollata, quindi l'istruzione non è stata eseguita. È stata verificata rispetto alla scheda in cui è stata scritta, ed eseguirla qui la eseguirebbe altrove. Torna a quella scheda ed eseguila di nuovo.
gate-commit-not-connected = Non connesso — il commit non è stato tentato. Le modifiche in sospeso vengono mantenute.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = Impossibile avviare { $program }: { $error }. Scegli un'altra shell in Impostazioni, Terminale.
term-cli-read-only = Questa connessione è di sola lettura in Schemaic. Il suo client da riga di comando non lo sarebbe, quindi non viene aperto qui.
backup-read-only-server = Questa connessione è di sola lettura in Schemaic, e il backup di questo motore viene scritto dal server, quindi non ne è stato eseguito nessuno.
backup-dialog-title = Backup del database
backup-kind-pg-dump = Dump PostgreSQL in formato custom
backup-kind-sql = Script SQL
backup-kind-sqlite = Database SQLite
backup-not-started = Il backup non è stato avviato: { $error }
backup-done = Backup eseguito in { $path }.
backup-failed = Il backup non è riuscito: { $error }
restore-title = Ripristina da file
restore-confirm = Eseguire { $path } in { $database } su { $connection }? Ciò che crea o sostituisce non può essere annullato. Viene eseguito nel terminale, dove Ctrl+C lo interrompe.
restore-read-only = Questa connessione è di sola lettura in Schemaic, quindi non vi viene ripristinato nulla.
restore-engine-not-here = I backup di questo motore non si ripristinano da qui.
restore-unreadable = Non è stato possibile leggere { $path } prima di ripristinarlo, quindi non è stato eseguito nulla: { $error }
restore-refused = { $path } passa a un altro database durante l'esecuzione — { $switches } — quindi, ripristinato in { $database }, scriverebbe invece lì. Non è stato eseguito nulla. Ripristinalo nel database che indica, oppure rimuovi quelle righe da una copia del file.
restore-refused-more = { $path } passa a un altro database durante l'esecuzione — { $switches }, e altre { $more } — quindi, ripristinato in { $database }, scriverebbe invece lì. Non è stato eseguito nulla. Ripristinalo nel database che indica, oppure rimuovi quelle righe da una copia del file.
restore-switch = { $how } `{ $name }` alla riga { $line }
restore-switch-unnamed = un { $how } alla riga { $line } che non indica alcun database leggibile da Schemaic

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Chiudi «{ $name }»
tab-close-unsaved = «{ $name }» ha modifiche non salvate. La chiusura le scarta; il file sul disco resta com'è. Chiudere comunque?
tab-close-all-title = Chiudi tutte le schede
tab-close-all-question = Vuoi davvero chiudere tutte le schede?
tab-close-others-title = Chiudi le altre schede
tab-close-others-question = Vuoi davvero chiudere tutte le altre schede?
file-open-title = Apri file SQL
file-save-title = Salva file SQL
file-sql-filter = Script SQL
file-read-failed = Impossibile leggere { $path }: { $error }
file-save-failed = Impossibile salvare { $path }: { $error }
file-too-large = { $path } è di { $size } — troppo grande per essere aperto in una scheda dell'editor. Schemaic passerebbe la maggior parte del tempo a rianalizzarlo. Eseguilo da una scheda di query, oppure usa Importa per un file di dati.
file-too-large-short = Il file è troppo grande per essere aperto.
file-large-title = Aprire un file di grandi dimensioni?
file-large-question = «{ $name }» è di { $size }. Schemaic rianalizza l'intero documento poco dopo ogni pausa nella digitazione, quindi un file di queste dimensioni rallenta la risposta dell'editor per tutto il tempo in cui la scheda resta aperta. Aprirlo comunque?
file-changed-on-disk = { $path } è cambiato sul disco da quando è stato aperto. Salvare ora scarterebbe quelle modifiche — ricarica invece il file (o usa Salva con nome con un nome diverso).
file-lossy-title = Sostituire i byte illeggibili?
file-lossy-question = Schemaic non è riuscito a leggere ogni byte di «{ $name }» come testo e ha mostrato quei byte come «�». Il salvataggio scrive ciò che vedi, quindi ognuno di essi viene sostituito in modo permanente — nell'intero file, non solo nelle righe che hai modificato. Salvare comunque?
file-replace-title = Sostituire il file?
file-replace-question = «{ $name }» esiste già — «{ $typed }» è stato salvato con l'aggiunta dell'estensione .sql. Sostituirlo?
file-reload-title = Ricarica dal disco
file-reload-unsaved = «{ $name }» ha modifiche non salvate. Ricaricare le scarta. Ricaricare comunque?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = È già in corso un'esportazione. Annullala o attendi che termini.
export-failed = Esportazione non riuscita: { $error }
export-worker-died = Esportazione non riuscita: il worker si è interrotto: { $error }
export-not-published = Esportazione non riuscita: non è stato possibile pubblicare { $path }: { $error }
export-not-renamed = L'esportazione ha scritto { $part } ma non è riuscita a rinominarlo in { $path }: { $error }
export-nothing-matched = Niente da esportare — nessuna tabella corrisponde alla selezione.
export-stopped = Esportazione annullata.
dump-failed = Dump non riuscito: { $error }
blob-load-too-large = Quel file è di { $size } — il massimo caricabile è { $most }.
blob-load-failed = Caricamento non riuscito: { $error }
blob-save-failed = Salvataggio non riuscito: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = Impossibile scrivere { $path }: { $error }
settings-export-title = Esporta impostazioni
settings-import-title = Importa impostazioni
settings-file-filter = Impostazioni di Schemaic
settings-import-nothing = Niente da importare: ogni impostazione in { $path } corrisponde già alle tue, e non aggiunge snippet.
settings-import-question = { $path } modifica { $changed ->
        [one] { $changed } impostazione
       *[other] { $changed } impostazioni
    } e aggiunge { $snippets ->
        [one] { $snippets } snippet
       *[other] { $snippets } snippet
    }. Le impostazioni che modifica sostituiscono i tuoi valori attuali; gli snippet vengono aggiunti accanto ai tuoi.
settings-import-unreadable = impossibile leggerlo: { $error }
connections-export-title = Esporta connessioni
connections-file-filter = Connessioni di Schemaic
connections-replace-title = Sostituisci «{ $file }»
connections-replace-question = In quella cartella esiste già un file chiamato «{ $file }». L'esportazione aggiunge quella estensione al nome che hai digitato, quindi la finestra di salvataggio non lo ha chiesto. La sostituzione non si può annullare.
import-choose-file-title = Scegli un file di connessioni
import-no-containers = Nessun container di database in esecuzione.
import-added = { $count ->
        [one] Aggiunta { $count } connessione.
       *[other] Aggiunte { $count } connessioni.
    }
import-added-foreign = { $count ->
        [one] Aggiunta { $count } connessione.
       *[other] Aggiunte { $count } connessioni.
    } La sola lettura e il badge dell'ambiente non vengono importati da un altro client — impostali in Gestisci connessioni per tutto ciò che conta.
conn-delete-title = Elimina «{ $name }»
conn-delete-question = Questa operazione elimina la connessione, la password memorizzata, la conversazione IA salvata, la cronologia delle query e le schede. Non si può annullare.
conn-delete-keyring-kept = Schemaic non è riuscito a raggiungere il portachiavi del sistema operativo, quindi la password memorizzata e i segreti SSH della connessione eliminata **sono ancora lì**.
conn-delete-keyring-later = Verranno rimossi la prossima volta che una connessione viene eliminata o salvata mentre il portachiavi è raggiungibile. Nel frattempo nessuna nuova connessione può riceverli — gli id delle connessioni non vengono mai riutilizzati.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = Non è stato possibile leggere { $name } ({ $error }).
notice-unreadable-kept = Il file illeggibile è stato conservato come { $name }.corrupt; Schemaic è ricorso al backup, o ai valori predefiniti se anche il backup era illeggibile.
notice-unreadable-swept = Quella copia può contenere password in chiaro, quindi viene rimossa la prossima volta che elimini una connessione — recupera prima ciò che ti serve.
notice-missing = { $name } mancava.
notice-missing-restored = Schemaic l'ha recuperato da { $name }{ $from }, che era ancora presente. Verrà riscritto la prossima volta che questa impostazione cambia — fino ad allora quella copia è l'unica.
notice-keyring-unread = { $n ->
    [one] Schemaic non è riuscito a leggere { $n } segreto salvato dal portachiavi del sistema operativo.
   *[other] Schemaic non è riuscito a leggere { $n } segreti salvati dal portachiavi del sistema operativo.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic non è riuscito a leggere { $n } segreto salvato dal portachiavi del sistema operativo ({ $error }).
   *[other] Schemaic non è riuscito a leggere { $n } segreti salvati dal portachiavi del sistema operativo ({ $error }).
}
notice-keyring-unread-kept = Quelle password non sono disponibili in questa sessione e **non** sono state eliminate — lascia vuoti i campi e torneranno quando il portachiavi sarà raggiungibile. Digitarne una nuova sovrascrive il segreto memorizzato.
notice-in-the-clear = { $n ->
    [one] Schemaic non è riuscito a raggiungere il portachiavi del sistema operativo, quindi { $n } password è stata salvata **in chiaro** in `connections.json`.
   *[other] Schemaic non è riuscito a raggiungere il portachiavi del sistema operativo, quindi { $n } password sono state salvate **in chiaro** in `connections.json`.
}
notice-in-the-clear-where = Quel file si trova nella cartella aperta dal pulsante *Apri cartella* di Impostazioni → Generale, accanto al log — controllalo prima di condividerlo. Salvare di nuovo quando il portachiavi è raggiungibile le riporta al suo interno.
notice-undeleted = { $n ->
    [one] { $n } password cancellata è ancora nel portachiavi del sistema operativo — Schemaic non è riuscito a eliminarla.
   *[other] { $n } password cancellate sono ancora nel portachiavi del sistema operativo — Schemaic non è riuscito a eliminarle.
}
notice-undeleted-after = Il campo è vuoto sul disco, ma al prossimo avvio verrà ricompilato. Salva di nuovo quando il portachiavi è raggiungibile.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Nessun SQL restituito
inline-ai-not-sql = Il modello non ha restituito SQL
inline-ai-failed = generazione non riuscita
ai-fill-no-value = L'IA non ha restituito alcun valore.
conn-gone = la connessione non esiste più
tx-session-not-ready = la connessione della transazione non è pronta — passa a Commit automatico e torna indietro
tx-session-open-failed = impossibile aprire una connessione per la transazione: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = L'istruzione è stata annullata.
tx-committed-before-ddl = La transazione in cui è stata eseguita non esiste più: MySQL esegue il commit prima di eseguire un'istruzione DDL, quindi le istruzioni già al suo interno sono permanenti e Rollback non le annullerà.
tx-rolled-back-by-server = La transazione in cui è stata eseguita non esiste più: SQL Server l'ha annullata con un rollback, quindi ogni istruzione eseguita al suo interno dall'inizio è annullata.
tx-doomed = La transazione in cui è stata eseguita non può più eseguire il commit: SQL Server l'ha contrassegnata come non confermabile, quindi Rollback è l'unico modo per proseguire, e annulla ogni istruzione al suo interno.
tx-after-own-commit = La transazione in cui è stata eseguita è terminata, e questa ha eseguito un proprio COMMIT prima di non riuscire, quindi ciò che precedeva quel COMMIT è probabilmente confermato anziché annullato. Controlla i dati prima di rieseguirne una parte.
tx-lost-on-commit = La connessione è stata persa con una transazione aperta. È stata persa durante un COMMIT, quindi non si sa se { $stmts ->
        [0] la transazione sia stata confermata o annullata
        [one] l'unica istruzione eseguita al suo interno dall'inizio sia stata confermata o annullata
       *[other] le { $stmts } istruzioni eseguite al suo interno dall'inizio siano state confermate o annullate
    }: il server potrebbe aver eseguito il commit prima che la connessione cadesse. Controlla i dati prima di rieseguirne una parte.
tx-lost-on-implicit-commit = La connessione è stata persa con una transazione aperta. MySQL esegue il commit di una transazione aperta prima di eseguire questa istruzione, quindi { $stmts ->
        [0] non era ancora stato eseguito nulla al suo interno, e non c'era nulla da confermare
        [one] l'unica istruzione eseguita al suo interno dall'inizio è stata molto probabilmente confermata in quel momento, ed è permanente
       *[other] le { $stmts } istruzioni eseguite al suo interno dall'inizio sono state molto probabilmente confermate in quel momento, e sono permanenti
    }. Controlla i dati prima di rieseguirne una parte.
tx-lost-rolled-back = La connessione è stata persa con una transazione aperta. Il server annulla una transazione non completata quando la sua connessione termina, quindi { $stmts ->
        [0] non era ancora stato eseguito nulla al suo interno
        [one] l'unica istruzione eseguita al suo interno dall'inizio è annullata
       *[other] le { $stmts } istruzioni eseguite al suo interno dall'inizio sono annullate
    }.
tx-lost-rolled-back-mysql = La connessione è stata persa con una transazione aperta. Il server annulla una transazione non completata quando la sua connessione termina, quindi { $stmts ->
        [0] non era ancora stato eseguito nulla al suo interno
        [one] l'unica istruzione eseguita al suo interno dall'inizio è annullata — tranne una scrittura in una tabella che non supporta le transazioni (MyISAM, MEMORY), che nessun rollback annulla
       *[other] le { $stmts } istruzioni eseguite al suo interno dall'inizio sono annullate — tranne una scrittura in una tabella che non supporta le transazioni (MyISAM, MEMORY), che nessun rollback annulla
    }.
tx-not-sent-timeout = Il timeout dell'istruzione è scattato mentre questa esecuzione attendeva ancora la connessione della scheda. Un'operazione precedente su di essa — un commit, o la rilettura che lo segue — non era terminata, quindi l'istruzione non è mai stata inviata al server. Non è stato eseguito nulla, e la transazione è esattamente dov'era.
tx-not-sent-cancelled = Questa esecuzione è stata annullata mentre attendeva ancora la connessione della scheda. Un'operazione precedente su di essa — un commit, o la rilettura che lo segue — non era terminata, quindi l'istruzione non è mai stata inviata al server. Non è stato eseguito nulla, e la transazione è esattamente dov'era.
snippet-default-name = Snippet
snippet-delete-title = Elimina snippet
snippet-delete-question = Eliminare «{ $name }»? L'operazione non si può annullare.
copy-target-read-only = La connessione di destinazione è di sola lettura, quindi non è stata copiata alcuna riga.
copy-target-gone = La connessione di destinazione non esiste più, quindi non è stata copiata alcuna riga.
copy-not-started = Non è stata copiata alcuna riga: { $error }
copy-counter-not-moved = { $count ->
        [one] È stata copiata { $count } riga, ma il contatore della tabella non è stato spostato oltre di essa, quindi il prossimo inserimento potrebbe entrare in conflitto: { $error }
       *[other] Sono state copiate { $count } righe, ma il contatore della tabella non è stato spostato oltre di esse, quindi il prossimo inserimento potrebbe entrare in conflitto: { $error }
    }
copy-stopped = La copia si è interrotta per un errore: { $error }
compare-reading-failed = Lettura di { $database }: { $error }
monitor-no-row-key = Nessuna chiave di riga per questa tabella — le modifiche non possono essere tracciate.
monitor-restarted = Le colonne della tabella sono cambiate, quindi il tracciamento delle modifiche è ripartito da qui.
timeout-cancelled-app = Annullata: l'istruzione è durata più del timeout dell'istruzione di { $timeout }. Modificalo o disattivalo in Impostazioni → Query.
timeout-cancelled-connection = Annullata: l'istruzione è durata più del timeout dell'istruzione di { $timeout }. Modificalo o disattivalo nelle impostazioni di questa connessione.
ai-chat-no-database = Impossibile raggiungere il database, quindi l'assistente non può essere avviato per questa connessione. Controlla la connessione e riprova.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **Non è stato possibile avviare l'assistente.** Schemaic assegna a ogni processo dell'agente una propria cartella di lavoro privata, e né la cartella temporanea di sistema né quella di configurazione hanno potuto ospitarne una — quindi non c'è un posto dove eseguirlo che non sia la cartella di qualcun altro. Verifica che la cartella temporanea sia scrivibile e riprova.
ai-no-reply-file = Impossibile creare un file temporaneo per la risposta.
ai-inline-no-opencode-config = Impossibile scrivere la configurazione di OpenCode, quindi la generazione non è stata avviata (senza di essa la CLI verrebbe eseguita con i propri strumenti abilitati).
ai-inline-no-copilot-home = Impossibile creare la cartella home privata di GitHub Copilot, quindi la generazione non è stata avviata (senza di essa la CLI caricherebbe la tua configurazione di Copilot, compresi i suoi server MCP e hook).
ai-inline-no-env-writer = { $cli } è contrassegnato come isolato dal suo ambiente, ma Schemaic non ha modo di predisporre quell'ambiente, quindi la generazione non è stata avviata.
ai-inline-no-cursor-rules = Impossibile scrivere il file dei permessi di Cursor, quindi la generazione non è stata avviata (senza di esso la CLI scrive file senza chiedere).
ai-inline-no-rules-writer = { $cli } è contrassegnato come limitato da un file dei permessi, ma Schemaic non ha modo di scriverne uno per esso, quindi la generazione non è stata avviata.
ai-no-tools = **Questa sessione non ha strumenti per il database.** { $cause }, quindi l'assistente non può cercare nulla — può solo rispondere in base allo schema riassuntivo già presente nel suo prompt. Chiudere e riaprire il pannello farà un nuovo tentativo.
ai-no-endpoint-file = Schemaic non è riuscito a creare il file privato che indica all'assistente come raggiungere il tuo database
ai-cursor-not-approved = Schemaic non è riuscito ad approvare i propri strumenti per il database con Cursor ({ $error })
ai-antigravity-not-registered = Schemaic non è riuscito a registrare i propri strumenti per il database con Antigravity
ai-antigravity-shared = Un'altra finestra di Schemaic sta già usando Antigravity. Antigravity mantiene un'unica registrazione MCP per tutto il computer, che può puntare a una sola connessione, quindi l'assistente di questa finestra non ha strumenti per il database
ai-antigravity-not-claimed = Schemaic non è riuscito a riservare la configurazione di Antigravity per questa sessione
ai-refused-no-opencode-config = Schemaic non è riuscito a scrivere la configurazione che limita OpenCode, quindi l'assistente è disattivato — eseguirlo senza quel file darebbe alla sessione una shell. Verifica che la cartella dei dati dell'app sia scrivibile.
ai-refused-no-copilot-home = Schemaic non è riuscito a creare la cartella home privata di GitHub Copilot, quindi l'assistente è disattivato — senza di essa la CLI caricherebbe la tua configurazione di Copilot, compresi i suoi server MCP e hook. Verifica che la cartella dei dati dell'app sia scrivibile.
ai-refused-no-cursor-rules = Schemaic non è riuscito a scrivere il file dei permessi che impedisce a Cursor di scrivere file ed eseguire comandi, quindi l'assistente è disattivato — senza di esso Cursor scrive file senza chiedere. Verifica che la cartella dei dati dell'app sia scrivibile.
ai-launch-failed-turn = Impossibile avviare la CLI `{ $bin }` ({ $error }). Assicurati che { $cli } sia installato, oppure indicane il percorso in Impostazioni → IA.
ai-launch-failed-session = Impossibile avviare la CLI `{ $bin }` ({ $error }). Assicurati che { $cli } sia installato (o imposta il suo percorso in Impostazioni → IA).
ai-turn-ended = Il turno di { $cli } è terminato inaspettatamente: { $error }
ai-session-ended-badly = La sessione IA è terminata inaspettatamente: { $error }
ai-session-ended = La sessione di { $cli } è terminata. Chiedi di nuovo per avviarne una nuova.
ai-session-restart-failed = Non è stato possibile riavviare la sessione di { $cli } dopo l'interruzione ({ $error }), quindi è terminata. Chiedi di nuovo per avviarne una nuova.
ai-session-not-accepting = La sessione di { $cli } non accetta più domande. Chiedi di nuovo per avviarne una nuova.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = Il contesto inviato a { $cli } è troppo grande per una sola riga di comando ({ $total } { $unit }; questa piattaforma ne consente circa { $limit }). { $levers }
ai-unit-characters = caratteri
ai-unit-bytes = byte
ai-levers-schema-and-query = Restringi Impostazioni → IA → Contesto dello schema al database attivo (o a Nessuno), oppure accorcia la query nell'editor.
ai-levers-sample = Abbassa l'accesso dell'IA ai dati di questa connessione sotto «Lascia leggere i dati», il che interrompe l'invio del campione di righe, oppure restringi Impostazioni → IA → Contesto dello schema.
ai-batch-shim = { $cli } si risolve in uno shim batch ({ $bin }), a cui non si può passare un prompt su più righe — Windows esegue un .cmd tramite cmd.exe, che rifiuta l'argomento prima che { $cli } venga avviato. Imposta Impostazioni → IA → Percorso della CLI sull'eseguibile reale (un'installazione npm di solito ne ha uno in node_modules, accanto allo shim).
ai-cli-exited = la CLI { $cli } è terminata con lo stato { $code }
ai-cli-signalled = la CLI { $cli } è stata terminata da un segnale
ai-constraint-sandboxed = { $cli } viene eseguito in sandbox: i suoi strumenti integrati non possono scrivere file né eseguire comandi, anche se possono ancora leggere questo computer.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Anche i server MCP che hai registrato con esso sono disponibili per questa sessione, e Schemaic non può limitarli.
    }
ai-constraint-workspace-rules = { $cli } è limitato a rispondere da un file dei permessi che Schemaic scrive per questa sessione, non da una sandbox: ai suoi strumenti è negato scrivere file ed eseguire comandi, ma possono ancora leggere file su questo computer.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Anche i server MCP che hai registrato con esso si avviano con questa sessione; i loro strumenti vengono rifiutati a meno che le tue impostazioni non li consentano.
    }
ai-constraint-denylist = Questa build di { $cli } non accetta il flag che svuota i suoi strumenti integrati, quindi vengono trattenuti da una lista di esclusione — più debole, e non qualcosa che Schemaic possa garantire. Aggiornare la CLI ripristina la protezione completa.
ai-constraint-unrestricted = Schemaic non è riuscito a limitare completamente gli strumenti integrati di { $cli } per questa sessione, quindi potrebbe essere in grado di fare più che rispondere a domande su questo database.
ai-constraint-unknown = Schemaic non ha potuto confermare che questo binario di { $cli } accetti i flag che lo limitano, quindi l'assistente è disattivato. Controlla il percorso in Impostazioni → IA.
ai-seed-empty = il modello non ha restituito alcun contenuto
ai-seed-not-json = la risposta non era JSON valido: { $error }
ai-seed-not-rows = la risposta non era un array JSON di oggetti riga

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } non è un file.
import-source-too-big = { $path } è di { $size }, oltre i { $most } che vengono letti.
import-source-unreadable = Non è stato possibile leggere { $path }: { $error }
docker-not-found = Docker non è stato trovato — né nel PATH né in WSL.
docker-not-started = Impossibile avviare Docker: { $error }
docker-timed-out = Docker non ha risposto entro { $secs } secondi.
docker-not-waited = Impossibile attendere Docker: { $error }
docker-answered = Docker ha risposto: { $error }
docker-exited = Docker è terminato con { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = il lettore del file si è interrotto inaspettatamente: { $error }
script-changed-since-read = Questo file è cambiato sul disco dopo essere stato letto, quindi il riepilogo qui sopra non corrisponde a ciò che verrebbe eseguito. Sceglilo di nuovo.
script-no-terminator = Questo file non contiene alcun terminatore di istruzione, quindi non è uno script SQL che Schemaic possa eseguire. Verifica di aver scelto il file giusto.
ai-cursor-enable-unfinished = `mcp enable` non è terminato
