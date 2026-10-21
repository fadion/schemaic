# Schemaic — Dutch catalogue.
#
# Machine-drafted and unreviewed (docs/architecture.md, The string catalogue); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Dutch.

## Settings › Appearance › Language

settings-language = Taal
settings-language-system = Systeem ({ $language })
settings-language-restart = De nieuwe taal wordt actief wanneer Schemaic de volgende keer start.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Lettergrootte
settings-term-cursor-style = Cursorstijl
settings-term-copy-on-select = Kopiëren bij selecteren
settings-term-copy-on-select-hint = Geselecteerde tekst naar het klembord kopiëren zodra een selectie eindigt.
settings-term-blink = Knipperende cursor
settings-term-blink-hint = De cursor laten knipperen zolang de terminal focus heeft.

## Settings › AI

settings-ai-agent = Agent-CLI
settings-ai-disabled = Uitgeschakeld
settings-ai-off-hint = Er wordt niets naar een agent gestuurd, er wordt geen agent-CLI gestart en er wordt geen AI-actie aangeboden. Kies een CLI om de assistent aan te zetten.
settings-ai-cli-path = Pad naar { $cli }
settings-ai-cli-path-placeholder = Leeg laten voor automatische detectie
settings-ai-detected = Automatisch gevonden: { $path }
settings-ai-detect-failed = Automatische detectie mislukt. { $cli } niet gevonden — geef hierboven een pad op.
settings-ai-cli-path-missing = Bestand bestaat niet.
settings-ai-model = Model
settings-ai-model-placeholder = Leeg laten voor de standaard van de agent-CLI
settings-ai-model-hint = Elke id die deze CLI accepteert — een alias, of een gedateerde snapshot om er één vast te zetten over upgrades heen.
settings-ai-model-hint-with-chips = Elke id die deze CLI accepteert — een alias, of een gedateerde snapshot om er één vast te zetten over upgrades heen. De knoppen zijn snelkoppelingen, niet de volledige lijst.
settings-ai-effort = Inspanning
settings-ai-instructions = Eigen instructies
settings-ai-instructions-placeholder = Dialect, conventies, huisregels…
settings-ai-schema-context = Schemacontext
settings-ai-schema-context-hint = Hoeveel databasestructuur met elk bericht meegaat. Geen houdt ook de schematools achter, zodat de assistent jou om namen vraagt in plaats van zelf de catalogus te lezen.
settings-ai-data-access = Toegang tot gegevens
settings-ai-data-access-where = Per verbinding in te stellen, in de instellingen van de verbinding.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Alleen schema
ai-data-on-request = Alleen wat ik bijvoeg
ai-data-full = Gegevens laten lezen
ai-data-schema-only-hint = Alleen structuur — namen, typen en definities. Geen enkele rij verlaat deze computer, en bijvoegen wordt geweigerd.
ai-data-on-request-hint = De assistent leest zelf geen gegevens. Rijen die je uit een resultaat bijvoegt, verlaten deze computer met die vraag.
ai-data-full-hint = De assistent mag zelf alleen-lezen query's uitvoeren en voorbeeldrijen lezen. Alles wat hij leest, verlaat deze computer.
settings-ai-gutter = Accentlijn bij antwoorden
settings-ai-gutter-hint = De antwoorden van de assistent markeren met een gekleurde lijn langs de rechterrand. Uit geeft ze aan beide kanten dezelfde marge.

## Settings › section list

settings-section-general = Algemeen
settings-section-editor = Editor
settings-section-formatting = Code-opmaak
settings-section-query = Query
settings-section-terminal = Terminal
settings-section-ai = AI-assistent
settings-section-appearance = Weergave

## Settings › shared

settings-off = Uit

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } teken
   *[other] { $n } tekens
}

## Settings › General

settings-log = Logbestand
settings-log-hint = Diagnostische gegevens, crashes inbegrepen, worden geschreven naar { $path }.
settings-log-hint-none = Op deze computer kon geen logbestand worden geopend, dus er wordt niets naar een logbestand geschreven.
settings-log-open-folder = Map openen
settings-bundle = Instellingenbestand
settings-bundle-hint = Je voorkeuren en fragmenten in één bestand, om mee te nemen naar een andere computer. Verbindingen, de vensterindeling, de shell van de terminal en verbindingsspecifieke fragmenten blijven hier.
settings-bundle-export = Exporteren
settings-bundle-import = Importeren
settings-cli = Opdrachtregel
settings-cli-install = Installeren
settings-cli-installing = Bezig met installeren
settings-cli-remove = Verwijderen
settings-cli-removing = Bezig met verwijderen

## Settings › modal

settings-title = Instellingen
settings-restore-tabs = Tabbladen herstellen bij opstarten
settings-restore-tabs-hint = De querytabbladen van je vorige sessie opnieuw openen wanneer de app start.

## Settings › Editor

settings-editor-font-size = Lettergrootte
settings-editor-predictions = Inline voorspellingen
settings-editor-predictions-hint = De waarschijnlijke volgende woorden na de cursor tonen — de tegenhanger van een trefwoord, de refererende sleutel van een join, de kolommen om op te groeperen of in te voegen. Tab, of → aan het eind van een regel, neemt ze in hun geheel over, Ctrl+→ (⌘→ op macOS) woord voor woord, en Escape wijst ze af.
settings-editor-remembered = Voorspellingen uit eerdere instructies
settings-editor-remembered-hint = Ook de rest van een regel aanbieden uit een instructie die je hebt uitgevoerd, in een ander tabblad hebt geschreven of als fragment hebt opgeslagen. Zet dit uit om eerdere instructies van het scherm te houden.
settings-editor-aliases = Tabelaliassen
settings-editor-aliases-hint = Wanneer de aanvulling een tabel na FROM of JOIN schrijft, er een korte alias van de beginletters achter zetten — orders o, order_items oi — en de alias gebruiken in de ON-voorwaarde van een join.
settings-editor-rainbow = Regenbooghaakjes
settings-editor-rainbow-hint = Elk paar haakjes kleuren naar hoe diep het genest is, zodat een sluithaakje in één oogopslag bij zijn openingshaakje te vinden is.
settings-editor-whitespace = Witruimte tonen
settings-editor-whitespace-hint = Spaties en tabs als vage tekens weergeven, zodat een tab van spaties te onderscheiden is.
settings-editor-guides = Inspringhulplijnen
settings-editor-guides-hint = Bij elk inspringniveau een vage verticale lijn tekenen.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Hoofdletters bij trefwoorden
settings-format-case-keep = Zoals getypt
settings-format-case-upper = Hoofdletters
settings-format-case-lower = Kleine letters
settings-format-commas-trailing = Eind van de regel
settings-format-commas-leading = Begin van de regel
settings-format-gap-one = Eén lege regel
settings-format-gap-none = Geen lege regel
settings-format-keyword-case-hint = Hoe Code opmaken trefwoorden zoals SELECT en FROM schrijft. Namen, strings en commentaar houden hun hoofdletters.
settings-format-function-case = Hoofdletters bij functies
settings-format-function-case-hint = Hoe Code opmaken de eigen functies van de engine, zoals COUNT en COALESCE, schrijft waar ze worden aangeroepen. Je eigen functies houden hun hoofdletters.
settings-format-type-case = Hoofdletters bij typen
settings-format-type-case-hint = Hoe Code opmaken de eigen gegevenstypen van de engine, zoals INT en VARCHAR, schrijft. Typen die je zelf hebt gemaakt, houden hun hoofdletters.
settings-format-commas = Komma's
settings-format-commas-hint = Waar Code opmaken de komma plaatst van een lijst die het over regels verdeelt. Lijsten die op één regel blijven, zoals de argumenten van een functie, blijven ongemoeid tenzij de terugloopbreedte ze afbreekt.
settings-format-statement-gap = Tussen instructies
settings-format-statement-gap-hint = De lege regels die Code opmaken tussen twee instructies laat. Zoals getypt houdt een lege regel overal waar er een was.
settings-format-wrap = Terugloopbreedte
settings-format-wrap-hint = Een lijst die Code opmaken op één regel houdt, zoals de argumenten van een functie, krijgt één item per regel zodra die regel voorbij deze breedte zou lopen.
settings-format-river = Clausuletrefwoorden rechts uitlijnen
settings-format-river-hint = SELECT, FROM, WHERE en de rest aan hun rechterrand uitlijnen, met de inhoud van elke clausule naast het trefwoord, in plaats van een trefwoord op een eigen regel.
settings-format-align = Kolommen uitlijnen
settings-format-align-hint = De AS van de aliassen in een selectielijst, de = van de toewijzingen in een UPDATE en de typen van de kolommen van een tabel onder elkaar zetten.
settings-format-join-on = Join-voorwaarde op een eigen regel
settings-format-join-on-hint = De ON van een join, en de AND en OR van de voorwaarde, op een eigen regel onder de join zetten.

## Settings › Query

settings-query-row-limit = Standaard rijlimiet
settings-query-timeout = Time-out voor instructies
settings-query-timeout-hint = Een instructie annuleren die langer loopt dan dit. Standaard uit — een import of een rapport kan terecht een tijd duren.
timeout-off = Geen time-out
timeout-hours = { $n ->
    [one] { $n } uur
   *[other] { $n } uur
}
timeout-minutes = { $n ->
    [one] { $n } minuut
   *[other] { $n } minuten
}
timeout-seconds = { $n ->
    [one] { $n } seconde
   *[other] { $n } seconden
}
settings-query-confirm-writes = Bevestigen voor het uitvoeren van schrijfacties
settings-query-confirm-writes-hint = Vragen voordat een instructie wordt uitgevoerd die gegevens of het schema wijzigt.
settings-query-validate = Live databasevalidatie
settings-query-validate-hint = De instructie onder de cursor tijdens het typen tegen de database controleren (een PREPARE die niets uitvoert) om exacte fouten te tonen. Kost bij elke pauze een extra rondgang naar de database.
settings-query-notify = Melden wanneer een uitvoering op de achtergrond klaar is
settings-query-notify-hint = Een bureaubladmelding tonen wanneer een query, script, export of dump die een seconde of langer duurde eindigt terwijl Schemaic niet het actieve venster is. De taakbalkknop wordt hoe dan ook gemarkeerd. De melding bevat nooit de SQL.
notify-query-finished = Query voltooid
notify-query-failed = Query mislukt
notify-script-finished = Script voltooid
notify-script-failed = Script mislukt
notify-export-finished = Export voltooid
notify-export-failed = Export mislukt
notify-dump-finished = Dump voltooid
notify-dump-failed = Dump mislukt
notify-took = Duurde { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Waarden in het schrijflogboek bewaren
settings-query-write-log-hint = Elke schrijfactie die Schemaic of schemaic exec uitvoert, wordt toegevoegd aan write_log.jsonl in de instellingenmap en nooit gewist. Waarden in de SQL ervan worden gemaskeerd, tenzij dit aan staat; een wachtwoord wordt hoe dan ook gemaskeerd.

## Settings › Appearance

settings-appearance-ui-theme = Interfacethema
settings-appearance-editor-theme = Editorthema
settings-appearance-scale = Interfaceschaal

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Sneltoetsen

## Shortcuts modal › group headings

shortcuts-group-global = Algemeen
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Resultatenraster
shortcuts-group-schema-tree = Schemaboom
shortcuts-group-ai-panel = AI-paneel
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = ER-diagram

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Overal zoeken
shortcuts-command-palette = Opdrachtpalet
shortcuts-new-tab = Nieuw querytabblad
shortcuts-close-tab = Querytabblad sluiten
shortcuts-next-tab = Volgend tabblad
shortcuts-previous-tab = Vorig tabblad
shortcuts-jump-to-tab = Naar tabblad springen
shortcuts-reopen-tab = Laatst gesloten tabblad heropenen
shortcuts-open-file = SQL-bestand openen
shortcuts-save-file = SQL-bestand opslaan
shortcuts-save-file-as = SQL-bestand opslaan als
shortcuts-toggle-schema = Schemapaneel aan/uit
shortcuts-toggle-ai = AI-paneel aan/uit
shortcuts-toggle-terminal = Terminal aan/uit
shortcuts-run-query = Query uitvoeren
shortcuts-autocomplete = Automatisch aanvullen
shortcuts-inline-ai = Inline AI-bewerking
shortcuts-find-in-editor = Zoeken in editor
shortcuts-find-replace = Zoeken en vervangen
shortcuts-go-to-line = Ga naar regel
shortcuts-matching-bracket = Naar het bijbehorende haakje of BEGIN/END springen
shortcuts-line-comment = Regelcommentaar aan/uit
shortcuts-block-comment = Blokcommentaar aan/uit
shortcuts-toggle-case = Hoofd-/kleine letters wisselen
shortcuts-join-lines = Regels samenvoegen
shortcuts-duplicate = Regel / selectie dupliceren
shortcuts-cut = Regel / selectie knippen
shortcuts-format-code = Code opmaken
shortcuts-format-statement = Instructie opmaken
shortcuts-single-line = Op één regel zetten
shortcuts-move-line = Regel omhoog / omlaag verplaatsen
shortcuts-quick-fix = De snelle oplossing onder de cursor toepassen
shortcuts-indent = Inspringen / uitspringen
shortcuts-take-prediction = De inline voorspelling overnemen (ook →, aan het eind van een regel) / het volgende woord ervan
shortcuts-find-in-results = Zoeken in resultaten
shortcuts-go-to-row = Ga naar rij
shortcuts-copy = Kopiëren
shortcuts-paste-staged = Plakken als klaargezette wijzigingen
shortcuts-select-all = Alles selecteren
shortcuts-first-last-cell = Eerste / laatste cel
shortcuts-edit-cell = Cel bewerken
shortcuts-next-cell = Volgende / vorige cel tijdens bewerken
shortcuts-commit-edits = Wijzigingen doorvoeren
shortcuts-mark-deletion = Rij markeren voor verwijdering
shortcuts-results-toolbar = Naar de resultatenwerkbalk gaan
shortcuts-along-toolbar = Langs de werkbalk bewegen (Esc keert terug naar het raster)
shortcuts-open-selected = De geselecteerde tabel, kolom of het geselecteerde object openen
shortcuts-context-menu = Contextmenu voor de geselecteerde rij
shortcuts-send-message = Bericht versturen
shortcuts-new-line = Nieuwe regel
shortcuts-recall-prompt = Vorige / volgende prompt terughalen
shortcuts-copy-paste = Kopiëren / plakken
shortcuts-find-in-diagram = Zoeken in het diagram

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
key-space = Spatie

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Paneel aan/uit
palette-toggle-panel-hint = schema · ai · terminal · querygeschiedenis · serveractiviteit
palette-panel-schema = Schema
palette-panel-ai = AI
palette-panel-terminal = Terminal
palette-panel-history = Querygeschiedenis
palette-panel-activity = Serveractiviteit
palette-new-tab = Nieuw tabblad
palette-export-settings = Instellingen exporteren
palette-import-settings = Instellingen importeren
palette-duplicate-tab = Tabblad dupliceren
palette-close-tab = Tabblad sluiten
palette-reopen-closed-tab = Gesloten tabblad heropenen
palette-reopen-closed-tab-hint = <tabblad>
palette-open-file = Bestand openen
palette-open-file-hint = een .sql-script
palette-save-file = Bestand opslaan
palette-save-file-as = Bestand opslaan als
palette-next-tab = Volgend tabblad
palette-previous-tab = Vorig tabblad
palette-format-code = Code opmaken
palette-format-statement = Instructie opmaken
palette-format-statement-hint = de instructie bij de cursor
palette-make-single-line = Op één regel zetten
palette-make-single-line-hint = de selectie of instructie, op één regel
palette-run = Uitvoeren
palette-run-hint = alle instructies uitvoeren
palette-go-to-line = Ga naar regel
palette-go-to-line-hint = <nummer>
palette-go-to-statement = Ga naar instructie
palette-search-hint = <zoekterm>
palette-history = Geschiedenis
palette-clear-history = Querygeschiedenis wissen
palette-clear-history-hint = huidige verbinding
palette-ask-ai = AI vragen
palette-ask-ai-hint = <prompt>
palette-ask-ai-empty = Typ een prompt…
palette-terminal = Terminal
palette-terminal-hint = <opdracht>
palette-run-in-terminal = In terminal uitvoeren
palette-terminal-empty = Typ een opdracht…
palette-ui-theme = UI-thema
palette-ui-theme-hint = licht · donker
palette-editor-theme = Editorthema
palette-editor-theme-hint = <thema>
palette-font-size = Lettergrootte
palette-increase-font-size = Letters vergroten
palette-decrease-font-size = Letters verkleinen
palette-indent-style = Inspringstijl
palette-indent-style-hint = tabs · spaties
palette-indent-spaces = Spaties
palette-indent-tabs = Tabs
palette-indent-width = Inspringbreedte
palette-toggle-word-wrap = Terugloop aan/uit
palette-toggle-whitespace = Witruimte aan/uit
palette-toggle-whitespace-hint = spaties en tabs tonen
palette-toggle-indent-guides = Inspringhulplijnen aan/uit
palette-switch-connection = Verbinding wisselen
palette-switch-connection-hint = <verbinding>
palette-server-variables = Servervariabelen
palette-server-variables-hint = de instellingen van de actieve verbinding
palette-enter-number = Voer een getal in
palette-nothing-found = Niets gevonden

## Command palette › what a result row is

palette-row-snippet = fragment
palette-row-open-tab = open tabblad
palette-row-history = geschiedenis
palette-row-view-body = viewdefinitie
palette-row-body = definitie ({ $kind })
palette-row-trigger-on = trigger op { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Openen in CLI
tree-open = Openen
tree-open-in-new-tab = Openen in nieuw tabblad
tree-copy-name = Naam kopiëren
tree-copy-qualified-name = Gekwalificeerde naam kopiëren
tree-properties = Eigenschappen
tree-used-by = Gebruikt door
tree-live-monitor = Livemonitor
tree-er-diagram = ER-diagram
tree-compare-with = Vergelijken met
tree-generate = Genereren
tree-generate-ddl = DDL genereren
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Vernieuwen
tree-collapse-all = Alles samenvouwen
tree-favourite = Favoriet maken
tree-unfavourite = Uit favorieten
tree-colour = Kleur
tree-colour-none = Geen
tree-hide = Verbergen
tree-import = Importeren
tree-export = Exporteren
tree-back-up = Back-up maken
tree-restore-from-file = Herstellen uit bestand
tree-copy-to = Kopiëren naar
tree-create = Maken
tree-create-table = Tabel
tree-create-view = View
tree-create-type = Type
tree-create-domain = Domein
tree-create-sequence = Reeks
tree-create-synonym = Synoniem
tree-create-alias-type = Aliastype
tree-create-xml-schema-collection = XML-schemaverzameling
tree-create-function = Functie
tree-create-procedure = Procedure
tree-create-event = Event
tree-create-extension = Extensie
tree-create-database = Database
tree-create-schema = Schema
tree-create-kind = Maken: { $kind }
tree-edit-kind = Bewerken: { $kind }
tree-folder-create-extension = Extensie maken
tree-edit-table = Tabel bewerken
tree-edit-view = View bewerken
tree-edit-column = Kolom bewerken
tree-edit-index = Index bewerken
tree-edit-primary-key = Primaire sleutel bewerken
tree-edit-foreign-key = Refererende sleutel bewerken
tree-triggers = Triggers
tree-triggers-count = Triggers ({ $n })
tree-refresh-view = View vernieuwen
tree-truncate = Leegmaken
tree-truncate-title = Tabel leegmaken
tree-truncate-rows = { $n ->
    [one] De ene rij ({ $rows }) in { $table } verwijderen? Dit kan niet ongedaan worden gemaakt.
   *[other] Alle { $rows } rijen in { $table } verwijderen? Dit kan niet ongedaan worden gemaakt.
}
tree-truncate-every-row = Elke rij in { $table } verwijderen? Dit kan niet ongedaan worden gemaakt.
tree-drop = Verwijderen
tree-drop-index = Index verwijderen
tree-drop-foreign-key = Refererende sleutel verwijderen
tree-ai-explain = AI-uitleg
tree-users = Gebruikers en bevoegdheden
tree-server-variables = Servervariabelen
tree-new-database = Database maken
tree-show-table-sizes = Tabelgroottes tonen

## Connection switcher

conn-menu-manage = Verbindingen beheren

## Shared dialog buttons

dialog-cancel = Annuleren
dialog-yes = Ja
dialog-no = Nee

## Open-transaction prompt

tx-prompt-title = Open transactie
tx-prompt-body = { $tab } heeft een open transactie met { $n ->
    [one] 1 instructie
   *[other] { $n } instructies
} erin. Je kunt doorvoeren om de wijzigingen te behouden, of terugdraaien om ze te verwerpen.
tx-prompt-body-aborted = { $tab } heeft een transactie die door een mislukte instructie is afgebroken, met { $n ->
    [one] 1 instructie
   *[other] { $n } instructies
} erin. Ze kan alleen worden teruggedraaid, waarmee de wijzigingen worden verworpen.
tx-prompt-rollback = Terugdraaien
tx-prompt-commit = Doorvoeren

## Error modal

error-modal-title = Fout
error-modal-none = Geen fout.
error-modal-ai-fix = AI-oplossing
error-modal-explain = Uitleggen

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = query mislukt: { $error }
db-connection-failed = verbinding mislukt: { $error }
db-query-cancelled = query geannuleerd

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Niet verbonden — de init-SQL van deze verbinding is geweigerd: { $why }
db-refused-transport-entra = Aanmelden met Microsoft Entra vereist SSL / TLS op Verify full: het token opent elke Azure SQL-database die het account kan bereiken, dus de server moet zijn naam bewijzen voordat hij er een krijgt.
db-refused-transport-aws = Aanmelden met AWS IAM vereist SSL / TLS van minstens Vereist: het token is een wachtwoord voor deze server en mag niet onversleuteld over het netwerk gaan.
db-refused-transport-gcp = Aanmelden met Google Cloud IAM vereist SSL / TLS op Verify CA, met de server-CA van de instantie als Root CA: het token opent elke Cloud SQL-instantie die het account kan bereiken.
db-refused-transport = Deze aanmeldmethode vereist een sterkere SSL / TLS-modus dan die van de verbinding.
db-refused-backup-in-terminal = Van een database op een server wordt een back-up gemaakt met de eigen tools van de server, in de terminal.
db-refused-commit-aborted = de transactie was al afgebroken — een instructie erin is mislukt of gestopt, dus doorvoeren verwerpt het werk in plaats van het op te slaan. Er is niets doorgevoerd.
db-refused-sqlite-no-manual = SQLite-verbindingen ondersteunen de handmatige transactiemodus nog niet — instructies worden uitgevoerd en doorgevoerd zodra ze worden verstuurd
db-refused-mssql-read-only-auto = Een alleen-lezen SQL Server-verbinding draait in de modus Auto: SQL Server heeft geen alleen-lezen transactie waarin een Handmatig-tabblad kan worden vastgehouden, en elke instructie die de modus Auto uitvoert wordt teruggedraaid
db-refused-client-charset = geweigerd: de clienttekenset van de sessie ({ $charset }) kan een aanhalingsteken in een teken verbergen, zodat de instructiecontrole de instructie niet kan lezen zoals de server dat zou doen
db-refused-sql-mode = geweigerd: de sql_mode van de server ({ $mode }) leest aanhalingstekens anders dan de instructiecontrole, en kon voor deze sessie niet worden gewijzigd
db-refused-writing-plan = Plannen worden hier in een alleen-lezen transactie uitgevoerd, en MySQL en MariaDB maken daarbinnen geen plan van een instructie die schrijft. Maak in plaats daarvan een plan van de SELECT-vorm van de instructie.
db-refused-writing-plan-function = Plannen worden hier in een alleen-lezen transactie uitgevoerd, en MySQL en MariaDB maken daarbinnen geen plan van een instructie die schrijft. Een functie die ze aanroept schrijft, en dat mag een plan hier niet.
db-refused-windows-build-only = Aanmelden met Windows is alleen beschikbaar in de Windows-versie van Schemaic.
db-refused-cloud-iam-engine = Aanmelden met Cloud IAM is bedoeld voor PostgreSQL- en MySQL-verbindingen.
db-refused-mssql-cancel-needs-kill = SQL Server kan de instructie van een andere sessie niet annuleren zonder de sessie te beëindigen — gebruik Sessie beëindigen.
db-refused-blank-value = { $has_row ->
        [some] Rij { $row } heeft een lege of blanco waarde voor { $column } ({ $sql_type }): SQL Server zou die opslaan als 0 of 1900-01-01 in plaats van haar te weigeren. Importeer lege velden als NULL, of vul ze in.
       *[none] Een lege of blanco waarde kan niet naar { $column } ({ $sql_type }) worden geschreven: SQL Server zou die opslaan als 0 of 1900-01-01 in plaats van haar te weigeren. Zet de cel op NULL, of typ een waarde.
    }
db-refused-code-page = { $has_row ->
        [some] Rij { $row } heeft een waarde voor { $column } ({ $sql_type }) met
       *[none] Een waarde voor { $column } ({ $sql_type }) bevat
    } { $has_lost ->
        [some] `{ $character }`, waarvoor codepagina { $code_page } geen teken heeft
       *[none] een teken waarvoor de codepagina geen teken heeft
    }: SQL Server zou het opslaan als `?` of een lijkend teken in plaats van het te weigeren. Sla de kolom op als nvarchar, of verwijder het teken.
db-refused-sqlite-no-file = er is geen databasebestand ingesteld
db-refused-sqlite-memory = :memory: is geen databasebestand — SQLite bewaart een database in het geheugen alleen zolang de verbinding bestaat die haar heeft gemaakt, en deze app opent per bewerking een nieuwe verbinding, dus elke schrijfactie zou als opgeslagen worden gemeld en daarna worden weggegooid. Geef het pad naar een bestand op.
db-refused-sqlite-uri = { $file }: dit is een URI, geen databasebestand — SQLite zou het deel na `?` lezen als openopties, en `?mode=memory` opent een kladdatabase die elke schrijfactie accepteert en niets bewaart. Geef het pad naar het bestand zelf op.
db-refused-sqlite-worker = de SQLite-worker is mislukt: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } is geen kolom van deze tabel, dus er is geen standaardwaarde om te schrijven
db-refused-sqlite-generated = { $column } is een gegenereerde kolom: er is geen standaardwaarde om haar op terug te zetten, en SQLite berekent haar uit de rij
db-refused-sqlite-sizes-too-large = Tabelgroottes worden alleen gelezen voor een database tot { $cap }: SQLite meet ze door elke pagina van het bestand te lezen, en dit bestand is { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = time-out
db-connect-init-failed = init-SQL `{ $statement }` mislukt: { $error }
db-connect-session-retired = de verbinding van dit tabblad is gesloten na een Stop die de server niet heeft bevestigd, en er kon geen nieuwe worden geopend; de transactie ervan is teruggedraaid. Zet het tabblad op Auto en terug op Handmatig om een nieuwe te openen.
db-connect-begin-failed = kon de transactie niet starten: { $error }
db-connect-prepare-session = kon de sessie niet voorbereiden: { $error }
db-connect-read-only-setup = kon de sessie niet alleen-lezen maken: { $error }
db-connect-guard-session = kon de sessie niet beveiligen: { $error }
db-connect-plan-setup = kon het plan niet opzetten: { $error }
db-connect-no-maintenance-db = geen onderhoudsdatabase bereikbaar
db-connect-browser-silent = SQL Server Browser op { $server } gaf geen poort voor de instantie { $instance }: de Browser-service draait niet, UDP-poort 1434 wordt onderweg geblokkeerd, of geen instantie daar heeft die naam. Als de instantie op een vaste poort luistert, stel de host dan in op { $server } zonder \{ $instance } en de poort op die poort.
db-connect-browser-without-tcp = SQL Server Browser op { $server } antwoordde voor de instantie { $instance }, maar gaf geen TCP-poort: de instantie luistert niet op TCP. Schakel TCP/IP ervoor in via SQL Server Configuration Manager en start haar opnieuw.
db-connect-azure-timed-out = Azure SQL heeft het aanmelden niet binnen { $secs } seconden afgerond. Een serverless database die is gepauzeerd, heeft ongeveer een minuut nodig om te hervatten, en deze poging heeft haar gewekt — maak zo dadelijk opnieuw verbinding.
db-connect-azure-unavailable = { $server } Een serverless database die is gepauzeerd, heeft ongeveer een minuut nodig om te hervatten, en deze poging heeft haar gewekt — maak zo dadelijk opnieuw verbinding.
db-ssh-connect-no-answer = SSH-verbinding met { $host }: geen antwoord binnen { $secs } s
db-ssh-auth-no-answer = SSH-authenticatie met { $host }: geen antwoord binnen { $secs } s
db-ssh-auth-error = SSH-authenticatiefout: { $error }
db-ssh-auth-failed = SSH-authenticatie mislukt
db-ssh-key-load = Laden van SSH-sleutel mislukt: { $error }
db-ssh-key-auth = Fout bij authenticatie met SSH-sleutel: { $error }
db-ssh-agent = SSH-agentfout: { $error }
db-ssh-agent-empty = SSH-agent heeft geen identiteiten geladen (voer `ssh-add` uit)
db-ssh-agent-auth = Fout bij authenticatie via SSH-agent: { $error }
db-ssh-no-agent = geen SSH-agent gevonden (start de service OpenSSH Authentication Agent, of start Pageant)
db-ssh-no-agent-socket = geen SSH-agent ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = SSH-jumphost kon { $host } niet bereiken: { $error }
db-ssh-connect-to = SSH-verbinding met { $host } mislukt: { $error }
db-ssh-connect = SSH-verbinding mislukt: { $error }
db-ssh-no-host = SSH-tunnel heeft geen host
db-ssh-bind = binden van lokale tunnel mislukt: { $error }
db-ssh-named-instance = Een benoemde instantie is niet bereikbaar via een SSH-tunnel: SQL Server Browser antwoordt met de poort via UDP, en dat draagt de tunnel niet. Stel de host in op { $server } zonder \{ $instance } en de poort op die waarop de instantie luistert.
db-ssh-host-key-changed = De SSH-hostsleutel voor { $host } is GEWIJZIGD sinds Schemaic hem voor het eerst vertrouwde.
db-ssh-host-key-expected = Verwacht    { $key }
db-ssh-host-key-offered = Aangeboden  { $key }
db-ssh-host-key-none = (geen sleutel vastgelegd)
db-ssh-host-key-changed-advice = Dit kan betekenen dat de server opnieuw is opgebouwd of dat zijn sleutel is vervangen — of dat iets de verbinding onderschept. Er is niets naartoe gestuurd. Controleer de nieuwe vingerafdruk via een ander kanaal (op de eigen console van de server, of bij de beheerder ervan) voordat je hem vertrouwt. Verwijder daarna de vermelding van deze host uit { $store } in de configuratiemap van Schemaic om de nieuwe sleutel vast te leggen.
db-ssh-host-key-store = Schemaic kon zijn lijst van vertrouwde SSH-hostsleutels ({ $store }) niet lezen, dus het kan niet bepalen of de sleutel van { $host } ({ $key }) dezelfde is als die je eerder vertrouwde.
db-ssh-host-key-store-advice = De verbinding is geweigerd in plaats van de sleutel op het eerste gezicht te vertrouwen. Repareer of verwijder { $store } in de configuratiemap van Schemaic — verwijderen betekent dat elke SSH-host bij de volgende verbinding opnieuw wordt vertrouwd, dus doe dat alleen als je de vingerafdrukken kunt controleren.
db-tls-setup = TLS-instelling mislukt: { $error }
db-tls-client-cert = clientcertificaat geweigerd: { $error }
db-tls-trust-store = TLS-vertrouwensarchief is onbruikbaar: { $error }
db-tls-bad-name = { $name } is geen naam die een certificaat kan dragen: { $error }
db-tls-ca-empty = CA-bestand { $path } bevat geen certificaten
db-tls-ca-unusable = CA-bestand { $path } bevat een certificaat dat niet als vertrouwensanker kan worden gebruikt
db-tls-cert-unreadable = kan certificaatbestand { $path } niet lezen: { $error }
db-tls-cert-not-pem = { $path } is geen PEM-certificaatbestand: { $error }
db-tls-cert-is-der = { $path } bevat geen PEM-certificaat — een DER-bestand moet eerst worden omgezet (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = kan sleutelbestand { $path } niet lezen: { $error }
db-tls-key-encrypted = { $path } is een met een wachtzin beveiligde privésleutel, die Schemaic niet kan openen. Ontsleutel hem eerst: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } is geen PEM-privésleutel: { $error }
db-sign-in-no-value = { $field ->
        [host] De verbinding heeft geen host om mee aan te melden.
       *[user] De verbinding heeft geen gebruiker om mee aan te melden.
    }
db-sign-in-leading-dash = { $field ->
        [host] De host van de verbinding begint met ‘-’, wat de cloud-CLI als een optie zou lezen in plaats van als de host.
       *[user] De gebruiker van de verbinding begint met ‘-’, wat de cloud-CLI als een optie zou lezen in plaats van als de gebruiker.
    }
db-sign-in-control = { $field ->
        [host] De host van de verbinding bevat een stuurteken.
       *[user] De gebruiker van de verbinding bevat een stuurteken.
    }
db-sign-in-shim-aws = De gevonden AWS CLI is een .cmd-shim (de met pip geïnstalleerde versie 1), en Schemaic voert geen shim uit via cmd.exe. Installeer versie 2 van de AWS CLI, of meld je aan met een wachtwoord.
db-sign-in-shim-gcloud = De gevonden gcloud is een .cmd-shim zonder meegeleverde Python ernaast, en Schemaic voert geen shim uit via cmd.exe. Installeer de Google Cloud CLI opnieuw met het installatieprogramma, of meld je aan met een wachtwoord.
db-sign-in-shim-azure = De gevonden Azure CLI is een .cmd/.bat-shim zonder Python ernaast, en Schemaic voert geen shim uit via cmd.exe. Installeer de Azure CLI opnieuw met het installatieprogramma van Microsoft, of meld je aan met een SQL-aanmelding.
db-sign-in-no-token = De cloud-CLI antwoordde zonder token.
db-sign-in-more-than-token = De cloud-CLI antwoordde met meer dan een token.
db-sign-in-azure-bad-answer = Het antwoord van de Azure CLI was niet het token dat hij had moeten afdrukken.
db-sign-in-azure-no-access-token = De Azure CLI antwoordde zonder toegangstoken.
db-sign-in-signed-out-aws = De AWS CLI heeft geen referenties. Voer `aws configure` of `aws sso login` uit en maak daarna opnieuw verbinding.
db-sign-in-signed-out-gcloud = De Google Cloud CLI is niet aangemeld. Voer `gcloud auth login` uit en maak daarna opnieuw verbinding.
db-sign-in-signed-out-azure = De Azure CLI is niet aangemeld. Voer `az login` uit en maak daarna opnieuw verbinding.
db-sign-in-no-token-from = De { $cli } kon geen token krijgen: { $line }
db-sign-in-no-token-from-silent = De { $cli } kon geen token krijgen.
db-sign-in-no-token-from-azure-silent = De Azure CLI kon geen token krijgen: hij gaf geen reden
db-sign-in-not-found = De { $cli } is niet gevonden. Installeer hem en meld je ermee aan, of meld je aan met een wachtwoord.
db-sign-in-not-found-azure = De Azure CLI (az) is niet gevonden. Installeer hem en voer `az login` uit, of meld je aan met een SQL-aanmelding.
db-sign-in-no-answer = De { $cli } antwoordde niet binnen { $secs } seconden.
db-sign-in-not-started = De { $cli } kon niet worden gestart: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = de export is gestopt met lezen
db-query-no-rows-to-export = die instructie levert geen rijen op om te exporteren
db-query-no-server-settings = Deze engine heeft geen serverinstellingen om te tonen.
db-query-no-status-list = Deze engine publiceert geen statuslijst — alleen zijn variabelen.
db-query-no-import-columns = Geen kolommen om in te importeren
db-query-read-stopped = Het lezen is gestopt: { $error }
db-query-batch-continued = SQL Server gaat na een fout in een instructie verder met de batch, tenzij de fout de batch beëindigt, dus de instructies na de mislukte kunnen zijn uitgevoerd.
db-query-sqlite-locked = In SQLite blokkeert een lange leesactie een schrijfactie: een export of import van een hele tabel houdt een leesvergrendeling op het bestand vast tot die klaar is, en er kan geen schrijfactie beginnen voordat dat zo is (MySQL en PostgreSQL werken niet zo). Wacht erop of annuleer hem en probeer het daarna opnieuw. Om lezen en schrijven op dit bestand voorgoed te laten overlappen, zet je het in de journaalmodus WAL — `PRAGMA journal_mode = WAL` — wat het bestand zelf wijzigt, dus Schemaic doet dat niet voor je.
db-query-rollback-failed-too = { $error } — en ook het terugdraaien is mislukt: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = instructie { $n } is mislukt: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } eerdere instructie is al toegepast en kan niet worden teruggedraaid
   *[other] { $failed } — { $n } eerdere instructies zijn al toegepast en kunnen niet worden teruggedraaid
}
ddl-cancelled = het plan is geannuleerd
ddl-dangling-foreign-key = het plan laat een refererende sleutel achter die nergens naar verwijst: { $row }
ddl-breaks-view = het plan zou de view { $view } breken, die een kolom leest die het hernoemt of verwijdert ({ $why }). Er is niets gewijzigd. Bewerk of verwijder eerst de view.
ddl-no-databases = SQLite heeft geen databases om te maken of te verwijderen — een database is daar een bestand, en Schemaic maakt of verwijdert dat niet voor je.
ddl-elsewhere-failed = De views in andere databases die select * uit { $table } doen, zijn niet vernieuwd ({ $error }), en lezen en schrijven de verkeerde kolommen tot dat wel gebeurt (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } view in een andere database die select * uit { $table } doet, kon niet worden vernieuwd, en leest en schrijft de verkeerde kolommen tot dat wel gebeurt (sp_refreshview): { $views }.
       *[other] { $count } views in een andere database die select * uit { $table } doen, konden niet worden vernieuwd, en lezen en schrijven de verkeerde kolommen tot dat wel gebeurt (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } database is niet doorzocht op een view die select * uit { $table } doet: { $databases }.
       *[other] { $count } databases zijn niet doorzocht op een view die select * uit { $table } doet: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] {" "}en nog { $more }
    }
ddl-rollback-failed = { $error } — en ook het terugdraaien is mislukt: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = delete op { $table } trof { $affected } rijen (precies 1 verwacht)
write-one-row-update = update op { $table } trof { $affected } rijen (precies 1 verwacht)
write-one-row-insert = insert in { $table } trof { $affected } rijen (precies 1 verwacht)
write-batch-inserted = een batch van { $rows } rijen voegde { $wrote } in
write-batch-affected = een batch van { $rows } rijen trof { $wrote }
write-row-issue = regel { $line }, kolom { $column }: { $problem } ({ $text })
write-row-issue-empty = regel { $line }, kolom { $column }: { $problem } (leeg)
write-copy-too-long = { $column } bevat meer tekst dan één leesactie kan dragen, dus het kopiëren is gestopt voordat die werd geschreven
write-copy-too-long-unnamed = een kolom bevat meer tekst dan één leesactie kan dragen, dus het kopiëren is gestopt voordat die werd geschreven
write-copy-bytes = { $column } bevat bytes die het kopiëren niet ongewijzigd kan lezen, dus het is gestopt voordat ze werden geschreven
write-copy-not-its-form = de waarde die voor { $column } is gelezen, heeft niet de vorm die het kopiëren vroeg, dus er is niets geschreven
write-copy-not-its-form-unnamed = de waarde die voor een kolom is gelezen, heeft niet de vorm die het kopiëren vroeg, dus er is niets geschreven
write-commit-cancelled = Doorvoeren geannuleerd
write-import-cancelled = Importeren geannuleerd
write-rollback-complete = {" "}— alle wijzigingen teruggedraaid
write-rollback-incomplete = {" "}— het terugdraaien heeft ze NIET ongedaan gemaakt: de opslagengine van deze tabel is niet transactioneel, dus de al geschreven rijen blijven staan. Controleer de tabel voordat je het opnieuw probeert.
write-rollback-unknown = {" "}— en het terugdraaien kon niet worden bevestigd: de verbinding viel weg voordat de server antwoordde. Er kunnen nog rijen staan, of niet. Controleer de tabel voordat je het opnieuw probeert.

## Schema tree › the panel

tree-panel-title = SCHEMA
tree-tip-databases = Databases tonen of verbergen
tree-tip-options = Schema-opties
tree-search-placeholder = Zoeken…
tree-loading = Laden
tree-no-tables = Geen tabellen
tree-failed-to-connect = Verbinden mislukt
tree-failed-view = Bekijken
tree-single-user-unread = Eén gebruiker — niet gelezen
tree-single-user-read = Lezen
tree-count-tables = { $n ->
    [one] { $n } tabel
   *[other] { $n } tabellen
}
tree-count-cols = { $n ->
    [one] { $n } kol.
   *[other] { $n } kol.
}
tree-count-keys = { $n ->
    [one] { $n } sleutel
   *[other] { $n } sleutels
}
tree-count-partitions = { $n ->
    [one] { $n } partitie
   *[other] { $n } partities
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = type
kind-domain = domein
kind-sequence = reeks
kind-function = functie
kind-procedure = procedure
kind-event = event
kind-synonym = synoniem
kind-alias-type = aliastype
kind-xml-schema-collection = XML-schemaverzameling
kind-extension = extensie
kind-group-type = Typen
kind-group-domain = Domeinen
kind-group-sequence = Reeksen
kind-group-function = Functies
kind-group-procedure = Procedures
kind-group-event = Events
kind-group-synonym = Synoniemen
kind-group-alias-type = Aliastypen
kind-group-xml-schema-collection = XML-schemaverzamelingen
kind-group-extension = Extensies

## Query history panel

history-title = QUERYGESCHIEDENIS
history-clear-tip = De geschiedenis van deze verbinding wissen…
history-clear-title = Querygeschiedenis wissen
history-clear-message = { $n } vastgelegde { $n ->
    [one] query
   *[other] query's
} voor deze verbinding verwijderen? Dit kan niet ongedaan worden gemaakt.
history-scope-current = Huidige
history-scope-all = Alle
history-cut-note = De nieuwste { $shown } van { $matched } worden getoond — zoek om te verfijnen.
history-empty = Nog geen query's.
history-no-match = Geen overeenkomende query's.
history-open-in-new-tab = Openen in nieuw tabblad
history-delete = Verwijderen
history-failed = Mislukt

## Snippet library panel

snippets-title = FRAGMENTENBIBLIOTHEEK
snippets-save-tip = De SQL van de editor als fragment opslaan
snippets-no-match = Geen fragment komt overeen.
snippets-band-this-connection = DEZE VERBINDING
snippets-band-all-connections = ALLE VERBINDINGEN
snippets-builtin = Ingebouwd
snippets-name-placeholder = Naam van fragment
snippets-insert = Invoegen in editor
snippets-open-in-new-tab = Openen in nieuw tabblad
snippets-edit = Bewerken
snippets-duplicate = Dupliceren
snippets-delete = Verwijderen
snippets-show-in = Tonen in
snippets-scope-this-connection = Deze verbinding
snippets-scope-engine = Elke { $engine }-verbinding
snippets-scope-all-connections = Alle verbindingen

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = VANDAAG
history-band-this-week = DEZE WEEK
history-band-earlier = EERDER

## Query tab strip

tabs-tip-unsaved = { $path } — niet-opgeslagen wijzigingen
tabs-pin = Vastmaken
tabs-unpin = Losmaken
tabs-rename = Naam wijzigen
tabs-duplicate = Dupliceren
tabs-close = Sluiten
tabs-open-file = Bestand openen
tabs-save = Opslaan
tabs-save-as = Opslaan als
tabs-reload = Opnieuw laden van schijf
tabs-reopen-last = Laatste tabblad heropenen
tabs-close-others = Andere tabbladen sluiten
tabs-close-all = Alle tabbladen sluiten

## Server Activity panel

activity-title = SERVERACTIVITEIT
activity-refresh-tip = Nu vernieuwen
activity-auto-refresh-off = Automatisch vernieuwen uit
activity-auto-refresh-every = Elke { $secs } s automatisch vernieuwen
activity-interval-off = Uit
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } u { $m } min
activity-age-days = { $d } d { $h } u
activity-view-sessions = Sessies
activity-view-locks = Vergrendelingen
activity-search-sessions = Sessies doorzoeken…
activity-search-locks = Vergrendelingen doorzoeken…
activity-loading = Laden…
activity-sqlite-no-sessions = SQLite heeft geen serversessies — het draait binnen dit proces, dus er is verder niets verbonden om te zien.
activity-sessions-no-match = Geen overeenkomende sessies.
activity-sessions-none = Geen andere sessies op deze server.
activity-sessions-undrawn = { $n ->
    [one] Nog { $n } sessie komt overeen — verfijn de zoekopdracht om haar te zien.
   *[other] Nog { $n } sessies komen overeen — verfijn de zoekopdracht om ze te zien.
}
activity-locks-no-match = Geen overeenkomende vergrendelingen.
activity-locks-no-waits = Geen wachtende vergrendelingen op deze server.
activity-locks-none = Geen andere sessie heeft een vergrendeling.
activity-locks-contended-only = Deze server toont alleen vergrendelingen waarop wordt gewacht, of die een wachtende ophouden — niet elke vergrendeling die een sessie heeft.
activity-locks-undrawn = { $n ->
    [one] Nog { $n } vergrendeling komt overeen — verfijn de zoekopdracht om haar te zien.
   *[other] Nog { $n } vergrendelingen komen overeen — verfijn de zoekopdracht om ze te zien.
}
activity-list-capped = lijst afgekapt
activity-locks-total = { $n ->
    [one] { $shown } vergrendeling
   *[other] { $shown } vergrendelingen
}
activity-locks-waiting = { $n ->
    [one] { $shown } wachtend
   *[other] { $shown } wachtend
}
activity-state-blocked = Geblokkeerd
activity-state-idle-in-tx = Inactief in tx
activity-state-running = Actief
activity-state-idle = Inactief
activity-count-sessions = { $n ->
    [one] { $figure } sessie
   *[other] { $figure } sessies
}
activity-count-running = { $figure } actief
activity-count-blocked = { $figure } geblokkeerd
activity-count-idle-in-tx = { $figure } inactief in tx
activity-lock-wait = Wachten op vergrendeling
activity-lock-wait-sentence = Sessie { $waiter } ({ $waiter_who }) { $waited ->
        [none] wacht op
       *[other] wacht al { $waited } op
    } een vergrendeling van sessie { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] inactief in een transactie
        [blocked] zelf geblokkeerd
        [running] actief
       *[idle] inactief
    }{ $held ->
        [none] {""}
       *[other] {" "}(al { $held })
    }.{ $others ->
        [0] {""}
        [one] {" "}{ $others } andere sessie heeft ook een vergrendeling die de wachtende nodig heeft.
       *[other] {" "}{ $others } andere sessies hebben ook een vergrendeling die de wachtende nodig heeft.
    }
activity-kill-holder = { $id } beëindigen
activity-terminate-tip = Sessie { $id } beëindigen
activity-read-only-tip = Deze verbinding is gemarkeerd als alleen-lezen.
activity-lock-holds = heeft
activity-lock-waits-for = wacht op
activity-lock-held-by = vastgehouden door { $ids }
activity-copy-statement = Instructie kopiëren
activity-cancel-query = Query annuleren
activity-kill-session = Sessie beëindigen
activity-waiting-on = wacht op een vergrendeling van { $ids }
activity-blocks = { $n ->
        [one] blokkeert { $n } sessie
       *[other] blokkeert { $n } sessies
    }
activity-read-only-cancel = Deze verbinding is gemarkeerd als alleen-lezen, dus Schemaic annuleert er geen query's op. Schakel eerst alleen-lezen uit in de instellingen van de verbinding.
activity-read-only-terminate = Deze verbinding is gemarkeerd als alleen-lezen, dus Schemaic beëindigt er geen sessies op. Schakel eerst alleen-lezen uit in de instellingen van de verbinding.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = AI-ASSISTENT
ai-new-chat-tip = Nieuwe chat
ai-settings-tip = AI-instellingen
ai-empty-ask = Stel een vraag over je SQL…
ai-empty-not-connected = { $cli } niet verbonden.
ai-input-placeholder = Bericht…
ai-input-off = De AI-assistent staat uit
ai-input-db-down = Niet verbonden met de database
ai-speaker-you = JIJ
ai-result-more-lines = … (+{ $n } regels meer)

## Shared dialog buttons (continued)

dialog-close = Sluiten

## Chart modal

chart-title = Grafiek — { $tab }
chart-caption-kind = Grafiek
chart-caption-values = Waarden
chart-caption-labels = Labels
chart-row-number = Rijnummer
chart-kind-bar = Staaf
chart-kind-line = Lijn
chart-kind-histogram = Histogram
chart-save-tip = De grafiek opslaan…
chart-save-dialog = Grafiek opslaan
chart-file-stem = { $name } grafiek
chart-nothing-to-save = Niets om op te slaan.
chart-no-numbers = Geen kolom met getallen om in dit resultaat in een grafiek te zetten.
chart-all-null = Elke waarde in die kolom is NULL.
chart-too-large = Deze waarden zijn te groot om te tekenen.
chart-caption-rows = { $n ->
    [one] { $n } rij
   *[other] { $n } rijen
}
chart-caption-skipped-null = { $skipped } NULL, niet getekend
chart-caption-skipped = { $skipped } niet getekend (NULL, of voorbij { $max })
chart-caption-truncated = de gelezen rijen, niet het hele resultaat

## Schema editors — shared

editor-preview-sql = Voorbeeld van SQL
editor-change-count = { $n ->
    [0] Geen wijzigingen
    [one] 1 wijziging
   *[other] { $n } wijzigingen
}

## View editor

view-editor-title-edit = View { $view } bewerken
view-problem-no-name = De view heeft een naam nodig.
view-problem-no-select = Een view heeft een SELECT nodig die hem definieert.
view-problem-not-a-query = De inhoud van een view moet een query zijn — die begint met SELECT, WITH, VALUES of TABLE.
view-problem-materialized = Schemaic kan geen materialized view bewerken — PostgreSQL heeft er geen CREATE OR REPLACE voor.
view-problem-hidden = De definitie van deze view is versleuteld of verborgen voor deze aanmelding, dus hij kan hier worden verwijderd, maar niet bewerkt.
view-problem-verbatim = De kop van deze view bevat iets wat Schemaic niet kan herformuleren, dus hij kan hier worden verwijderd, maar niet bewerkt.
view-editor-title-create = View maken in { $location }
view-editor-section-view = View
view-editor-section-definition = Definitie
view-editor-section-options = Opties
view-editor-name = Naam
view-editor-select = SELECT-instructie
view-editor-check-option = Check option
view-editor-column-names = Kolomnamen
view-editor-column-names-placeholder = optioneel — uit de SELECT genomen wanneer leeg
view-editor-sql-security = SQL security
view-editor-definer = Definer
view-editor-recreate = Opnieuw maken in plaats van vervangen
view-editor-recreate-hint = PostgreSQL kan aan een bestaande view alleen kolommen toevoegen. Een kolom hernoemen, van type veranderen of verplaatsen vereist een DROP en een CREATE, waarbij afhankelijke views en rechten meegaan.

## Table properties panel

props-title = Eigenschappen — { $table }
props-reading = Statistieken lezen
props-no-stats-view = Een view heeft geen eigen opslag, dus de server rapporteert er geen statistieken voor.
props-no-stats-table = De server heeft geen statistieken voor deze tabel gerapporteerd.
props-on-disk = op schijf
props-rows-estimated = rijen (geschat)
props-rows-counted = rijen (geteld)
props-rows-not-reported = rijen — niet gerapporteerd
props-counting = Tellen
props-count-rows = Rijen tellen
props-count-slow = Een volledige scan van de tabel. Traag bij een grote tabel.
props-storage-data = Gegevens
props-storage-indexes = Indexen
props-storage-free = Vrij
props-section-structure = Structuur
props-columns = Kolommen
props-indexes = Indexen
props-foreign-keys = Refererende sleutels
props-count-columns = { $n ->
    [one] { $n } kolom
   *[other] { $n } kolommen
}
props-count-indexes = { $n ->
    [one] { $n } index
   *[other] { $n } indexen
}
props-count-keys = { $n ->
    [one] { $n } sleutel
   *[other] { $n } sleutels
}
props-type = Type
props-type-view = View
props-type-table = Tabel
props-section-options = Opties
props-engine = Engine
props-row-format = Rijformaat
props-collation = Sortering
props-next-auto-increment = Volgende auto-increment
props-created = Gemaakt
props-updated = Bijgewerkt
props-comment = Opmerking
props-section-partitioning = Partitionering
props-partition-of = Partitie van
props-partition-bound = Grens
props-partitioned-by = Gepartitioneerd op
props-partitions-more = { $n ->
    [one] en nog { $n } partitie
   *[other] en nog { $n } partities
}
props-section-indexes = Indexen
props-index-cardinality = cardinaliteit { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } scan
       *[other] { $shown } scans
    }
props-index-not-counted = gebruik niet geteld
props-index-unused = nooit gebruikt sinds de tellers van de server zijn gereset
props-never-analysed = Deze tabel is nooit geanalyseerd, dus de server heeft er geen schatting van het aantal rijen voor. Voer ANALYZE uit, of tel de rijen.
props-analysed-at = Geschat volgens de laatste ANALYZE, { $when }.
props-read-live = Live gelezen — deze server heeft information_schema_stats_expiry = 0.
props-cached-up-to = In de servercache: deze cijfers kunnen tot { $age } oud zijn (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } u
props-age-days = { $n } d
props-vacuum-note = { $pct }% van de rijversies van deze tabel is dood en nog niet vrijgegeven. Een VACUUM zou die ruimte teruggeven.
props-copy = Kopiëren

## Shared panel search box

panel-search-placeholder = Zoeken…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = zojuist
ago-minutes = { $n } min geleden
ago-hours = { $n } u geleden
ago-days = { $n } d geleden
ago-weeks = { $n } wk geleden

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } rij
   *[other] { $n } rijen
}
outcome-rows-capped = { $n }+ rijen

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = De database-CLI openen
term-db-cli-tip-read-only = De database-CLI openen — deze verbinding is gemarkeerd als alleen-lezen
term-restart-tip = De terminal herstarten
term-settings-tip = Terminalinstellingen

## Results panel header

results-title = RESULTATEN
results-properties-tip = Tabeleigenschappen…
results-monitor-tip = Livemonitor…
results-restore-editor-tip = De editor herstellen
results-collapse-editor-tip = De editor samenvouwen

## Query panel header

query-title = QUERY
query-no-database = Geen database

## Shared dialog buttons (continued)

dialog-stop = Stoppen

## Copy table modal

copy-table-title = { $table } kopiëren
copy-table-rows-line = Daarna elke rij van { $table } erin kopiëren (niet als SQL getoond)
copy-table-into = Kopiëren naar
copy-table-schema = Schema
copy-table-name = Naam
copy-table-back = ‹ terug
copy-table-data = Gegevens kopiëren
copy-table-data-hint = De rijen kopiëren zodra de tabel is gemaakt. Uit kopieert alleen de structuur.
copy-table-what-with-rows = De structuur en de rijen. Refererende sleutels en triggers worden niet gekopieerd, en een gegenereerde kolom wordt opnieuw berekend.
copy-table-what-structure = Alleen de structuur. Refererende sleutels en triggers worden niet gekopieerd.
copy-table-choose-database = Kies de database om naar te kopiëren.
copy-table-unnamed = Geef de kopie een naam.
copy-table-onto-itself = Een tabel kan niet op zichzelf worden gekopieerd — geef de kopie een andere naam.
copy-table-connection-changed = De actieve verbinding is gewijzigd — open Kopiëren naar opnieuw vanuit de tabel.
copy-table-schema-not-loaded = Het schema van { $database } is niet geladen — vouw het uit in de boom en probeer het opnieuw.
copy-table-preview-subject = Kopie van { $table }
copy-table-copying = Rijen kopiëren… { $n }
copy-table-copied = { $n } { $n ->
    [one] rij
   *[other] rijen
} gekopieerd.
copy-table-stopped = Gestopt voordat elke rij was gekopieerd. De tabel is gemaakt.

## Database, schema and extension editor

database-editor-title-database = Database maken
database-problem-no-name = De naam mag niet leeg zijn.
database-problem-bad-collation = Een sorteringsnaam bevat alleen letters, cijfers en underscores.
database-editor-title-schema = Schema maken
database-editor-title-extension = Extensie maken
database-editor-title-schema-in = Schema maken in { $database }
database-editor-title-extension-in = Extensie maken in { $database }
database-editor-section-database = Database
database-editor-section-schema = Schema
database-editor-section-extension = Extensie
database-editor-name = Naam
database-editor-charset = Tekenset
database-editor-collation = Sortering
database-editor-owner = Eigenaar
database-editor-server-default = standaard van de server
database-editor-owner-you = jij
database-editor-no-extensions = Geen extensies meer om te installeren
database-editor-no-charsets = Geen tekensets om voor te stellen
database-editor-no-collations = Geen sorteringen om voor te stellen
database-editor-no-roles = Geen rollen op deze server

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importeren in { $database }
script-dialog-title = Een SQL-script uitvoeren
script-section = SQL-script
script-choose-file = Kies een bestand…
script-no-file = Geen bestand gekozen
script-reading = Het bestand lezen
script-hint = Het script kan een andere database maken en selecteren dan die waarin je importeert. Het kan ook meer dan één database maken. Controleer het script om zeker te weten dat het doet wat je verwacht.
script-hint-namespace = Het script kan een andere database maken en selecteren dan die waarin je importeert. Het kan ook meer dan één database maken. Dit openen vanuit { $namespace } zet het script er ook niet in: alles wat het bestand niet kwalificeert, komt terecht waar de search_path van de server aangeeft. Controleer het script om zeker te weten dat het doet wat je verwacht.
script-what-it-does = Wat dit bestand doet
script-probe-nothing = Dit bestand bevat geen instructies die Schemaic kan uitvoeren.
script-probe-cut-off = De eerste { $size } van dit bestand zijn één onvoltooide instructie, dus er valt nog niets samen te vatten. Uitvoeren stuurt het hele bestand.
script-probe-counted = { $count } { $n ->
    [one] instructie
   *[other] instructies
} gelezen uit de eerste { $size }.
script-destroys = { $count } { $n ->
    [one] instructie
   *[other] instructies
} in dit bestand { $n ->
    [one] vernietigt
   *[other] vernietigen
} gegevens — DROP, TRUNCATE of DELETE. Dat kan hier niet ongedaan worden gemaakt.
script-unread = Alleen de eerste { $size } van dit bestand zijn gelezen, en niets daarin vernietigt gegevens — maar naar de rest is niet gekeken. Het kan DROP, TRUNCATE of DELETE bevatten, en dat kan hier niet ongedaan worden gemaakt.
script-unqualified = { $count } { $n ->
    [one] instructie
   *[other] instructies
} in dit bestand { $n ->
    [one] werkt
   *[other] werken
} op elke rij van een tabel — een DELETE, UPDATE of TRUNCATE zonder WHERE. Dat kan hier niet ongedaan worden gemaakt.
script-tx-own = Dit bestand opent een eigen transactie, dus het slaagt in zijn geheel of helemaal niet. Schemaic voegt er zelf geen toe.
script-tx-none = Dit bestand opent geen transactie, dus bij een fout halverwege blijft staan wat ervoor is uitgevoerd.
script-connecting = Verbinden
script-progress-of = { $done } van { $total }
script-progress-read = { $done } gelezen
script-run = Uitvoeren
script-done = { $n } { $n ->
    [one] instructie
   *[other] instructies
} uit { $file } uitgevoerd.
script-stopped = Gestopt na { $n } { $n ->
    [one] instructie
   *[other] instructies
}.
script-failed = Mislukt: { $message } — { $n } { $n ->
    [one] instructie was
   *[other] instructies waren
} ervoor uitgevoerd.
script-failed-at = Mislukt op regel { $line } van { $file }: { $message } — { $n } { $n ->
    [one] instructie was
   *[other] instructies waren
} ervoor uitgevoerd.
script-durability-rolled-back = { $file } opende een eigen transactie, dus de server heeft de hele uitvoering teruggedraaid — er is niets toegepast, ook niet de instructies die slaagden.
script-durability-applied = { $file } opende geen transactie, dus elke instructie die is uitgevoerd, blijft toegepast.
script-durability-applied-before = Elke instructie die werd uitgevoerd voordat { $file } een eigen transactie opende, blijft toegepast; wat binnen de transactie werd uitgevoerd, is teruggedraaid.
script-durability-unknown = Elke instructie die al is uitgevoerd, blijft toegepast, tenzij { $file } een eigen transactie opende.

## Schema comparison modal

compare-title = Schema's vergelijken
compare-choose-database = Kies een database
compare-no-connection = Geen verbinding met deze engine om mee te vergelijken.
compare-will-change = wordt gewijzigd
compare-source-of-truth = bron van waarheid
compare-idle = Kies een database om mee te vergelijken.
compare-loading = Beide schema's lezen
compare-select-object = Selecteer een object om beide kanten te zien.
compare-fk-cycle-note = De refererende sleutels van deze tabellen vormen een kringloop — geen volgorde van aanmaken voldoet aan allemaal, dus een gegenereerde migratie kan halverwege worden geweigerd.
compare-same-but-stored-differently = Deze komen structureel overeen — het verschil hieronder zit in hoe de engine de instructie opslaat, en er valt niets te migreren.
compare-summary = { $differ } verschillend · { $only_here } alleen hier · { $only_there } alleen daar · { $same } identiek
compare-summary-uncertain = { $n } { $n ->
    [one] overeenkomst
   *[other] overeenkomsten
} waarvoor de vergelijking niet kan instaan
compare-filter-placeholder = Filteren op naam
compare-include-identical = Identieke meenemen
compare-select-all = Alles selecteren
compare-select-none = Niets
compare-hint-held-back = uit het plan gehouden; de opmerking hierboven zegt waarom
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = de definitie moet opnieuw van de server worden gelezen voordat dit kan worden toegepast
compare-omitted-unread = deze vergelijking heeft er geen instructie voor — de definitie kon niet worden gelezen
compare-omitted-no-statement = deze vergelijking heeft geen instructie voor het gevonden verschil
compare-held-partition = het is een { $sub ->
        [yes] subpartitie
       *[no] partitie
    } van { $parent }, dat wordt tegengehouden — maak ze samen opnieuw, met de hand
compare-held-partitioning-differs = de partitionering verschilt, en deze vergelijking schrijft geen instructie die een tabel opnieuw partitioneert — maak hem met de hand opnieuw
compare-held-sequence-unwritten = deze vergelijking schrijft geen instructie voor een reeks — kopieer die uit `SHOW CREATE SEQUENCE`, die het paneel ervan toont
compare-held-partitioned-unrestated = hij is gepartitioneerd, en geen CREATE TABLE hier herhaalt het partitieschema — maak met de hand de partitiefunctie en het schema, daarna de tabel
compare-held-sequence-range = de teller van het doel staat op { $at }, buiten het nieuwe bereik { $min } tot { $max }, en de server weigert een reeks te versmallen voorbij een waarde die hij al heeft uitgegeven. Herstart hem op het doel (ALTER SEQUENCE … RESTART WITH) op een waarde die geen tabel bevat, en vergelijk daarna opnieuw
compare-note-set-aside = { $aside } — { $side ->
        [target] het doel
       *[source] de bron
    } bevat ook { $kept }, en de namen van het doel negeren hoofdletters, dus het kan maar één van de twee bevatten; { $kept } is degene die is vergeleken
compare-note-binding = Aliastype { $alias }: { $bound ->
        [rule] de regel { $rule }
        [default] de standaardwaarde { $default }
       *[both] de regel { $rule } en de standaardwaarde { $default }
    } die er op de bron aan gebonden is, wordt niet vergeleken of meegenomen — bind die met de hand (sp_bindrule / sp_bindefault).
compare-hint-blocked = de definitie moet opnieuw worden gelezen voordat dit kan worden toegepast
compare-hint-uncertain = een index die dit model maar deels leest — een overeenkomst hier is niet zeker
compare-preview-migration = Voorbeeld van migratie
compare-plan-subject = { $n ->
    [one] { $n } object in { $database }
   *[other] { $n } objecten in { $database }
}
compare-selected = { $n ->
        [0] Niets geselecteerd.
        [one] { $n } object geselecteerd
       *[other] { $n } objecten geselecteerd
    }
compare-incomparable = { $left } en { $right } kunnen niet worden vergeleken. Typenamen, standaardwaarden en indexvormen zijn niet overdraagbaar tussen engines, dus elke migratie die uit het verschil wordt gegenereerd, zou fout zijn.
compare-group-table = Tabellen
compare-group-view = Views
compare-group-trigger = Triggers
compare-kind-enum = enum
compare-kind-domain = domein
compare-kind-xml-schema-collection = XML-schemaverzameling
compare-kind-alias-type = aliastype
compare-kind-sequence = reeks
compare-kind-table = tabel
compare-kind-view = view
compare-kind-function = functie
compare-kind-procedure = procedure
compare-kind-trigger = trigger
compare-kind-event = event
compare-kind-synonym = synoniem

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Geen van beide databases bevat iets om te vergelijken.
compare-empty-agrees = Deze twee schema's komen overeen, object voor object.
compare-empty-agrees-noted = Elk object komt overeen, maar niet alles kon worden vergeleken — zie de opmerkingen hierboven.
compare-empty-no-match = Niets komt overeen met dat filter.
compare-empty-only-identical = Alles wat met dat filter overeenkomt, is identiek — zet Identieke meenemen aan om het te zien.

## Schema editors — shared (continued)

editor-comment-placeholder = wat het doet

## Event editor

event-editor-title-edit = Event { $event } bewerken
event-editor-title-create = Event maken in { $location }
event-editor-section-event = Event
event-editor-section-schedule = Planning
event-editor-section-body = Inhoud
event-editor-section-options = Opties
event-editor-name = Naam
event-editor-body = Inhoud
event-editor-runs = Wordt uitgevoerd
event-editor-runs-repeating = Herhalend
event-editor-runs-once = Eenmalig
event-editor-at = Op
event-editor-every = Elke
event-editor-starts = Begint
event-editor-ends = Eindigt
event-editor-status = Status
event-editor-status-enabled = Ingeschakeld
event-editor-status-disabled = Uitgeschakeld
event-editor-status-replica = Uitgeschakeld op replica
event-editor-preserve = Bewaren na voltooiing
event-editor-preserve-hint = ON COMPLETION PRESERVE. Uit is de standaard van MySQL en betekent dat de server het event verwijdert zodra de laatste uitvoering voorbij is — wat bij een eenmalig event direct na de uitvoering is.
event-editor-definer = Definer
event-editor-comment = Opmerking
event-editor-reading-source = De broncode van het event lezen…
event-editor-source-stale = De echte broncode van het event kwam binnen nadat je begon te typen, dus deze Inhoud is gebouwd op de kopie uit de catalogus — en dat is niet wat de server heeft. Sluit en open opnieuw om hem te bewerken.
event-problem-no-name = Het event heeft een naam nodig.
event-problem-no-time = Een eenmalig event heeft een tijdstip nodig om op te worden uitgevoerd.
event-problem-no-interval = Een herhalend event heeft een interval nodig.
event-problem-no-unit = Een herhalend event heeft een intervaleenheid nodig (DAY, HOUR…).
event-problem-no-body = Het event heeft inhoud nodig.
event-name-clash = Er is hier al een event met de naam { $name }.

## Routine editor

routine-editor-title-edit-function = Functie { $routine } bewerken
routine-editor-title-edit-procedure = Procedure { $routine } bewerken
routine-editor-title-create-function = Functie maken in { $location }
routine-editor-title-create-procedure = Procedure maken in { $location }
routine-editor-section-function = Functie
routine-editor-section-procedure = Procedure
routine-editor-section-body = Inhoud
routine-editor-section-options = Opties
routine-editor-name = Naam
routine-editor-parameters = Parameters
routine-editor-returns = Retourneert
routine-editor-returns-placeholder-mssql = int, of TABLE
routine-editor-language = Taal
routine-editor-body = Inhoud
routine-editor-recompile = Opnieuw compileren
routine-editor-recompile-hint = WITH RECOMPILE — bij elke aanroep een nieuw plan, nooit een plan uit de cache.
routine-editor-schema-binding = Schemabinding
routine-editor-schema-binding-hint = WITH SCHEMABINDING — de tabellen en views die het leest, kunnen niet onder het object vandaan worden gewijzigd.
routine-editor-execute-as = Uitvoeren als
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER of een gebruiker
routine-editor-null-input = Retourneert NULL bij NULL-invoer
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — de inhoud wordt niet uitgevoerd als een argument NULL is.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatisch
routine-editor-comment = Opmerking
routine-editor-volatility = Volatiliteit
routine-editor-strict = Strict
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Wordt uitgevoerd met de rechten van de eigenaar in plaats van die van de aanroeper. Zet hieronder een vaste search_path wanneer je dit gebruikt.
routine-editor-security-definer-hint-mysql = Wordt uitgevoerd met de rechten van de definer in plaats van die van de aanroeper. Dit is de standaard van MySQL — uitzetten is SQL SECURITY INVOKER.
routine-editor-settings = Instellingen
routine-editor-add-setting = Instelling toevoegen
routine-editor-deterministic = Deterministisch
routine-editor-deterministic-hint = Belooft hetzelfde resultaat voor dezelfde argumenten. Een server met binaire logging weigert een niet-deterministische routine, tenzij hij makers vertrouwt.
routine-editor-data-access = Toegang tot gegevens
routine-editor-definer = Definer
routine-editor-reading-source = De broncode van de routine lezen…
routine-editor-source-stale = De echte broncode van de routine kwam binnen nadat je begon te typen, dus deze Inhoud is gebouwd op de kopie uit de catalogus — en dat is niet wat de server heeft. Sluit en open opnieuw om hem te bewerken.
routine-problem-function-no-name = De functie heeft een naam nodig.
routine-problem-procedure-no-name = De procedure heeft een naam nodig.
routine-problem-function-no-language = De functie heeft een taal nodig (plpgsql, sql).
routine-problem-procedure-no-language = De procedure heeft een taal nodig (plpgsql, sql).
routine-problem-no-return-type = De functie heeft een retourtype nodig.
routine-problem-procedure-returns = Een procedure retourneert niets — maak het retourtype leeg.
routine-problem-function-no-body = De functie heeft inhoud nodig.
routine-problem-procedure-no-body = De procedure heeft inhoud nodig.
routine-problem-bare-parameter = SQL Server geeft elke parameter een naam met @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Een procedure kan geen WITH { $option } hebben.
routine-problem-scalar-refuses = Een scalaire functie kan geen WITH { $option } hebben.
routine-problem-inline-refuses = Een inline tabelwaardefunctie kan geen WITH { $option } hebben.
routine-problem-multi-refuses = Een tabelwaardefunctie met meerdere instructies kan geen WITH { $option } hebben.
routine-problem-procedure-refuses-native = Een procedure kan geen WITH { $option } hebben zodra ze systeemeigen is gecompileerd.
routine-problem-scalar-refuses-native = Een scalaire functie kan geen WITH { $option } hebben zodra ze systeemeigen is gecompileerd.
routine-problem-inline-refuses-native = Een inline tabelwaardefunctie kan geen WITH { $option } hebben zodra ze systeemeigen is gecompileerd.
routine-problem-multi-refuses-native = Een tabelwaardefunctie met meerdere instructies kan geen WITH { $option } hebben zodra ze systeemeigen is gecompileerd.
routine-problem-native-needs-schemabinding = Een systeemeigen gecompileerde routine heeft WITH SCHEMABINDING nodig — SQL Server vereist die twee samen.
routine-problem-inline-with-execute-as = INLINE = ON kan niet worden gecombineerd met EXECUTE AS — een functie die als iemand anders wordt uitgevoerd, wordt nooit inline gemaakt.
routine-problem-numbered-rename = { $was } staat aan het hoofd van een genummerde proceduregroep ({ $group }), en de naam wijzigen zou ze allemaal verwijderen. Behoud de naam om hem te bewerken.
routine-problem-trigger-function-args = Een triggerfunctie heeft geen gedeclareerde argumenten — die komen binnen in TG_ARGV.
routine-clash-function-refused = Er is hier al een functie met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus de server weigert dit en er verandert niets — kies een andere naam, of verwijder eerst { $name }.
routine-clash-procedure-refused = Er is hier al een procedure met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus de server weigert dit en er verandert niets — kies een andere naam, of verwijder eerst { $name }.
routine-clash-function-destroys = Er is hier al een functie met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus dit toepassen zou { $original } vernietigen en { $name } laten zoals het was.
routine-clash-procedure-destroys = Er is hier al een procedure met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus dit toepassen zou { $original } vernietigen en { $name } laten zoals het was.
routine-clash-function-destroys-new = Er is hier al een functie met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus dit toepassen zou het origineel vernietigen en { $name } laten zoals het was.
routine-clash-procedure-destroys-new = Er is hier al een procedure met de naam { $name }. Op deze engine is hernoemen een drop en een create, dus dit toepassen zou het origineel vernietigen en { $name } laten zoals het was.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Accounts doorzoeken
users-loading-accounts = Accounts laden
users-unsupported = De engine van deze verbinding heeft geen gebruikersaccounts.
users-none = Deze server rapporteert geen accounts.
users-no-match = Geen account komt overeen met ‘{ $needle }’.
users-pick-account = Kies een account om te zien wat het mag.
users-section-attributes = Kenmerken
users-section-privileges = Bevoegdheden
users-attr-authentication = Authenticatie
users-attr-locked = Vergrendeld
users-attr-password-expired = Wachtwoord verlopen
users-attr-superuser = Superuser
users-attr-create-role = Rol maken
users-attr-create-database = Database maken
users-attr-replication = Replicatie
users-attr-bypass-rls = RLS omzeilen
users-attr-inherit = Overnemen
users-attr-connection-limit = Verbindingslimiet
users-attr-valid-until = Geldig tot
users-attr-type = Type
users-attr-disabled = Uitgeschakeld
users-attr-default-database = Standaarddatabase
users-attr-server-roles = Serverrollen
users-attr-login = Aanmelding
users-attr-default-schema = Standaardschema
users-attr-member-of = Lid van
users-attr-yes = Ja
users-attr-no = Nee
users-login-type-sql = SQL-aanmelding
users-login-type-windows = Windows-aanmelding
users-login-type-windows-group = Windows-groep
users-kind-user = Gebruiker
users-kind-role = Rol
users-kind-login = Aanmelding
users-maintained-by-server = beheerd door de server
users-loading-privileges = Bevoegdheden laden
users-no-privileges = Dit account heeft geen bevoegdheden.
users-note-my-own-account = Deze verbinding kan mysql.user niet lezen, dus dit is niet de accountlijst van de server — het is het enige account dat deze verbinding kan zien, het eigen account. Maak verbinding als een account met SELECT op de database mysql om de rest te bekijken.
users-note-ms-logins-elsewhere = Azure SQL Database bewaart zijn aanmeldingen in master, en die worden hier niet beheerd: vermeld zijn de gebruikers en rollen van de database.
users-note-ms-logins-only = Alleen de aanmeldingen van de server worden vermeld: kies een database om de gebruikers en rollen ervan te zien.
users-note-ms-short = Deze aanmelding heeft geen VIEW ANY DEFINITION, dus de server toont haar alleen de principals die ze mag zien — de lijst kan onvolledig zijn.
users-note-my-scope = Bevoegdheden via een toegekende rol worden niet uitgevouwen: SHOW GRANTS toont wat rechtstreeks aan het account is toegekend, en de rollen waarvan het lid is, maar niet wat die rollen zelf hebben. Open de eigen bevoegdheden van de rol om die te zien.
users-note-pg-scope = Schema-, tabel- en reeksbevoegdheden zijn die in { $database } — PostgreSQL bewaart ze in de eigen catalogus van elke database, dus bevoegdheden in andere databases worden niet vermeld. EXECUTE op functies en procedures, en bevoegdheden die op één kolom zijn toegekend, worden ook niet vermeld. Bevoegdheden via rollidmaatschap of toegekend aan PUBLIC worden niet uitgevouwen, en evenmin de bevoegdheden die een rol heeft doordat hij een object bezit of superuser is.
users-note-pg-no-database = Schema-, tabel- en reeksbevoegdheden worden niet vermeld: PostgreSQL bewaart ze in de eigen catalogus van elke database, en voor deze verbinding is geen database geselecteerd. Open een querytabblad op een database om ze te zien. EXECUTE op functies en procedures, en bevoegdheden die op één kolom zijn toegekend, worden ook niet vermeld. Bevoegdheden via rollidmaatschap of toegekend aan PUBLIC worden niet uitgevouwen, en evenmin de bevoegdheden die een rol heeft doordat hij een object bezit of superuser is.
users-note-pg-superuser = Deze rol is SUPERUSER: hij omzeilt elke machtigingscontrole, dus hij heeft elke bevoegdheid op elk object, of iets hieronder dat nu zegt of niet.
users-note-pg-predefined = { $role } is een van de vooraf gedefinieerde rollen van PostgreSQL: wat hij kan, is in de server ingebouwd in plaats van toegekend, dus het staat in geen enkele catalogus die hier wordt gelezen.
users-note-pg-owns = { $n ->
    [one] Deze rol bezit { $n } object in { $database } (schema's, tabellen, reeksen of de database zelf). Een eigenaar heeft elke bevoegdheid op wat hij bezit, en PostgreSQL legt daar geen toekenning voor vast, dus niets daarvan staat hieronder.
   *[other] Deze rol bezit { $n } objecten in { $database } (schema's, tabellen, reeksen of de database zelf). Een eigenaar heeft elke bevoegdheid op wat hij bezit, en PostgreSQL legt daar geen toekenning voor vast, dus niets daarvan staat hieronder.
}
users-note-pg-owns-cluster = { $n ->
    [one] Deze rol bezit { $n } object in dit cluster (schema's, tabellen, reeksen of de database zelf). Een eigenaar heeft elke bevoegdheid op wat hij bezit, en PostgreSQL legt daar geen toekenning voor vast, dus niets daarvan staat hieronder.
   *[other] Deze rol bezit { $n } objecten in dit cluster (schema's, tabellen, reeksen of de database zelf). Een eigenaar heeft elke bevoegdheid op wat hij bezit, en PostgreSQL legt daar geen toekenning voor vast, dus niets daarvan staat hieronder.
}
users-note-ms-unshown = { $n ->
    [one] { $n } machtiging wordt hier niet getoond, op { $classes }: de server rapporteert haar op een beveiligbaar object dat deze lijst niet kan benoemen.
   *[other] { $n } machtigingen worden hier niet getoond, op { $classes }: de server rapporteert ze op beveiligbare objecten die deze lijst niet kan benoemen.
}
users-note-ms-pick-database = De machtigingen van een databasegebruiker horen bij die database — kies er een om ze te zien.
users-statements-capped = De eerste { $shown } van { $total } instructies worden getoond. Bevoegdheden kopiëren neemt ze allemaal mee.
users-new-account = Nieuw account
users-system-account = Dit account hoort bij de server, dus Schemaic wijzigt het niet.
users-reset-password = Wachtwoord opnieuw instellen
users-gate-no-database = Open een querytabblad op een database om bevoegdheden te wijzigen — de instructie moet in een database worden uitgevoerd.
users-gate-read-only = Deze verbinding is alleen-lezen.
users-count = { $n } { $n ->
    [one] account
   *[other] accounts
}
users-count-filtered = { $shown } van { $n } accounts
users-copy-privileges = Bevoegdheden kopiëren

## Schema editors — shared (continued)

editor-row-remove = Verwijderen

## Trigger editor

trigger-editor-title = Triggers op { $table }
trigger-problem-no-name = De trigger heeft een naam nodig.
trigger-problem-no-table = Een trigger heeft een tabel nodig om op af te gaan.
trigger-problem-no-event = Een trigger heeft minstens één event nodig om op af te gaan.
trigger-problem-pg-no-function = Een PostgreSQL-trigger heeft een functie nodig om uit te voeren.
trigger-problem-pg-body = Een PostgreSQL-trigger voert een functie uit, geen inhoud — kies of schrijf er een om uit te voeren.
trigger-problem-truncate-row-level = Een TRUNCATE-trigger moet FOR EACH STATEMENT zijn.
trigger-problem-instead-of-row-only = Een INSTEAD OF-trigger moet FOR EACH ROW zijn.
trigger-problem-instead-of-view-only = Alleen een view kan een INSTEAD OF-trigger hebben — een tabel krijgt BEFORE of AFTER.
trigger-problem-pg-view-row-timing = Een BEFORE- of AFTER-trigger op een view moet FOR EACH STATEMENT zijn — gebruik INSTEAD OF om op rijen te werken.
trigger-problem-update-of-without-update = UPDATE OF noemt kolommen bij een UPDATE-trigger.
trigger-problem-pg-instead-of-columns = Een INSTEAD OF-trigger heeft geen kolomlijst — PostgreSQL laat hem afgaan bij elke UPDATE van de view.
trigger-problem-one-event = { $engine } laat een trigger op één event afgaan — maak per event een aparte trigger.
trigger-problem-no-truncate = { $engine } heeft geen TRUNCATE-trigger.
trigger-problem-sqlite-view-timing = Een trigger op een view moet INSTEAD OF zijn — SQLite heeft geen BEFORE- of AFTER-trigger op een view.
trigger-problem-sqlite-row-only = SQLite heeft alleen FOR EACH ROW-triggers — er is geen trigger op instructieniveau.
trigger-problem-no-body = Een { $engine }-trigger heeft inhoud nodig.
trigger-problem-sqlite-not-block = De inhoud van een SQLite-trigger moet een BEGIN … END-blok zijn met minstens één instructie.
trigger-problem-runs-body = Een { $engine }-trigger voert inhoud uit, geen functie — schrijf de instructies die moeten worden uitgevoerd.
trigger-problem-ms-no-before = SQL Server heeft geen BEFORE-trigger — gebruik INSTEAD OF om vóór de schrijfactie te handelen.
trigger-problem-ms-view-timing = Een trigger op een view moet in SQL Server INSTEAD OF zijn.
trigger-problem-ms-statement-only = SQL Server laat een trigger één keer per instructie afgaan — lees de rijen uit inserted en deleted.
trigger-problem-ms-instead-of-rank = Een INSTEAD OF-trigger kan geen uitvoeringsvolgorde hebben — zet elk event terug op willekeurige volgorde.
trigger-problem-first-without-event = Hij gaat als eerste af op { $event }, maar is daar geen trigger voor.
trigger-problem-last-without-event = Hij gaat als laatste af op { $event }, maar is daar geen trigger voor.
trigger-problem-no-when = { $engine } heeft geen WHEN-voorwaarde — zet de test in de inhoud met IF.
trigger-problem-ms-no-update-of = SQL Server heeft geen UPDATE OF — test UPDATE(kolom) in de inhoud.
trigger-problem-my-no-view = MySQL accepteert geen trigger op een view.
trigger-problem-my-no-instead-of = MySQL heeft geen INSTEAD OF-trigger — die zijn van PostgreSQL.
trigger-problem-my-no-update-of = MySQL heeft geen UPDATE OF — een trigger ziet elke kolom.
trigger-problem-constraint-trigger = Schemaic kan de constraint trigger { $name } niet bewerken — het modelleert de uitstelinstellingen die zo'n trigger heeft niet.
trigger-problem-not-editable = Schemaic kan de trigger { $name } niet bewerken — de definitie is versleuteld, verborgen voor deze aanmelding, of gebruikt een optie die Schemaic niet modelleert. Wijzig hem in plaats daarvan in SQL.
trigger-problem-twice = Twee triggers heten allebei { $name }.
trigger-problem-first-clash = { $n ->
    [2] Maar één trigger kan als eerste afgaan op { $event } — { $names } en { $last } staan daar allebei op.
   *[other] Maar één trigger kan als eerste afgaan op { $event } — { $names } en { $last } staan daar allemaal op.
}
trigger-problem-last-clash = { $n ->
    [2] Maar één trigger kan als laatste afgaan op { $event } — { $names } en { $last } staan daar allebei op.
   *[other] Maar één trigger kan als laatste afgaan op { $event } — { $names } en { $last } staan daar allemaal op.
}
trigger-editor-preview-subject = triggers op { $table }
trigger-editor-none-selected = Geen trigger geselecteerd.
trigger-editor-readonly-constraint = Dit is een constraint trigger. Schemaic modelleert de uitstelinstellingen die zo'n trigger heeft niet, dus hij kan hier worden verwijderd, maar niet bewerkt — wijzig hem in plaats daarvan in SQL.
trigger-editor-readonly-hidden = De definitie van deze trigger is versleuteld of verborgen voor deze aanmelding, dus hij kan hier worden verwijderd, maar niet bewerkt.
trigger-editor-readonly-unmodelled = Deze trigger gebruikt een optie die Schemaic niet modelleert, dus hij kan hier worden verwijderd, maar niet bewerkt — wijzig hem in plaats daarvan in SQL.
trigger-editor-section-trigger = Trigger
trigger-editor-section-condition = Voorwaarde
trigger-editor-section-action = Actie
trigger-editor-name = Naam
trigger-editor-timing = Moment
trigger-editor-event = Event
trigger-editor-fires = Gaat af
trigger-editor-truncate-hint = Alleen op instructieniveau.
trigger-editor-not-for-replication-hint = Stil terwijl een replicatieagent naar de tabel schrijft.
trigger-editor-rank-any = Willekeurige volgorde
trigger-editor-rank-first = Gaat als eerste af
trigger-editor-rank-last = Gaat als laatste af
trigger-editor-when = Wanneer
trigger-editor-of-columns = Van kolommen
trigger-editor-of-columns-placeholder = optioneel — elke kolom wanneer leeg
trigger-editor-body = Inhoud
trigger-editor-function = Functie
trigger-editor-edit-function = Bewerken
trigger-editor-new-function = Nieuwe functie
trigger-editor-arguments = Argumenten
trigger-editor-add-argument = Argument toevoegen

## Schema editors — shared (continued)

editor-row-move-up = Omhoog
editor-row-move-down = Omlaag

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Bewerken: { $kind } { $name }
object-editor-title-create = Maken: { $kind } in { $location }
object-editor-name = Naam
object-editor-comment = Opmerking
object-editor-type = Type
object-editor-default = Standaardwaarde
object-editor-section-type = Type
object-editor-section-values = Waarden
object-editor-section-domain = Domein
object-editor-section-constraints = Beperkingen
object-editor-section-sequence = Reeks
object-editor-section-position = Positie
object-editor-section-synonym = Synoniem
object-editor-section-alias-type = Aliastype
object-editor-section-xml-schema-collection = XML-schemaverzameling
object-editor-section-stands-for = Staat voor
object-editor-value-placeholder = waarde
object-editor-add-value = Waarde toevoegen
object-editor-values-order = In vergelijkingsvolgorde — dit is de volgorde die ORDER BY gebruikt
object-editor-add-constraint = Beperking toevoegen
object-editor-no-base-types = Deze engine stelt geen basistypen voor
object-editor-not-null-hint = Elke kolom van dit domein weigert NULL. Aanzetten mislukt als er al een NULL in staat.
object-editor-stores = Slaat op
object-editor-increment = Stapgrootte
object-editor-start = Begin
object-editor-cache = Cache
object-editor-minimum = Minimum
object-editor-maximum = Maximum
object-editor-not-whole-number = { $field } moet een geheel getal zijn.
object-problem-type-no-name = Het type heeft een naam nodig.
object-problem-duplicate-value = De waarde { $value } staat er meer dan één keer in.
object-problem-empty-value = Een waarde mag niet leeg zijn — PostgreSQL kan haar nooit verwijderen.
object-problem-domain-no-name = Het domein heeft een naam nodig.
object-problem-domain-no-base = Een domein heeft een type nodig waarop het is gebaseerd.
object-problem-constraint-no-name = Elke beperking heeft een naam nodig.
object-problem-constraint-no-predicate = Beperking { $name } heeft geen predicaat.
object-problem-constraint-twice = Beperking { $name } komt twee keer voor.
object-problem-sequence-no-name = De reeks heeft een naam nodig.
object-problem-zero-increment = Een reeks kan niet met 0 ophogen.
object-problem-min-above-max = Het minimum ({ $min }) ligt boven het maximum ({ $max }).
object-problem-start-outside = De beginwaarde ({ $start }) ligt buiten { $min }..{ $max }.
object-problem-cache-below-one = De cache moet minstens 1 zijn.
object-problem-does-not-fit = { $min }..{ $max } past niet in { $data_type }.
object-problem-restart-outside = Herstarten op { $restart } ligt buiten { $min }..{ $max }.
object-problem-counter-outside = De reeks staat op { $last }, buiten het nieuwe { $min }..{ $max }. Geef een herstartwaarde binnen het bereik op.
object-problem-no-name = De naam mag niet leeg zijn.
object-problem-tsql-no-type = Een reeks heeft een type nodig.
object-problem-start-not-whole = Het begin moet een geheel getal zijn.
object-problem-increment-not-whole = De stapgrootte moet een geheel getal zijn.
object-problem-minimum-not-whole = Het minimum moet een geheel getal zijn.
object-problem-maximum-not-whole = Het maximum moet een geheel getal zijn.
object-problem-tsql-zero-increment = De stapgrootte mag niet 0 zijn.
object-problem-min-not-below-max = Het minimum moet onder het maximum liggen.
object-problem-started-outside = De reeks begon op { $start }, buiten het nieuwe bereik: herstart op een waarde tussen het minimum en het maximum.
object-problem-tsql-start-outside = Het begin moet tussen het minimum en het maximum liggen.
object-problem-cache-not-positive = De cachegrootte moet een geheel getal boven 0 zijn.
object-problem-restart-not-whole = De herstartwaarde moet een geheel getal zijn.
object-problem-tsql-restart-outside = De herstartwaarde moet tussen het minimum en het maximum liggen.
object-problem-synonym-no-target = Een synoniem heeft het object nodig waarvoor het staat.
object-problem-alias-no-base = Een aliastype heeft een basistype nodig.
object-editor-cycle = Cyclisch
object-editor-cycle-hint = Doorlopen naar het andere eind van het bereik in plaats van te mislukken wanneer de reeks op is.
object-editor-restart-at = Herstarten op — de teller gaf als laatste { $last } uit
object-editor-restart-at-unused = Herstarten op — de teller is nog niet gebruikt
object-editor-restart-not-whole = Herstarten op moet een geheel getal zijn.
object-editor-restart-placeholder = leeg laten om de positie te behouden
object-editor-tsql-restart-placeholder = leeg behoudt hem, tenzij het type wijzigt
object-editor-owned-by = Eigendom van
object-editor-detach = Loskoppelen van de kolom
object-editor-detach-hint = De reeks wordt niet meer samen met de kolom verwijderd, en overleeft haar.
object-editor-cache-hint = Waarden uit het geheugen uitgeven, sneller, ten koste van een gat na een herstart van de server.
object-editor-cache-size = Cachegrootte
object-editor-cache-size-placeholder = standaard van de server
object-editor-synonym-server = Server
object-editor-synonym-server-placeholder = deze server
object-editor-synonym-database = Database
object-editor-synonym-database-placeholder = deze database
object-editor-synonym-schema = Schema
object-editor-synonym-schema-placeholder = het standaardschema
object-editor-synonym-object = Object
object-editor-synonym-object-placeholder = tabel, view of routine
object-editor-base-type = Basistype
object-editor-nullable = Nullable
object-editor-nullable-hint = Een kolom van dit type accepteert NULL, tenzij de kolom iets anders zegt.
object-editor-schemas = Schema's

## Account and grant editors (Users and privileges)

account-editor-title-create = Account maken
account-editor-title-reset = Wachtwoord opnieuw instellen
account-editor-title-privileges = Bevoegdheden — { $account }
account-editor-account = Account
account-editor-kind = Soort
account-editor-name = Naam
account-editor-host = Host
account-editor-host-placeholder = % (elke host)
account-editor-for-login = Voor aanmelding
account-editor-for-login-placeholder = aanmelding — leeg voor een gebruiker zonder aanmelding
account-editor-also-user = Ook een gebruiker in { $database }
account-editor-password = Wachtwoord
account-editor-confirm = Bevestigen
account-editor-action = Actie
account-editor-action-grant = Toekennen
account-editor-action-revoke = Intrekken
account-editor-subject = Onderwerp
account-editor-subject-privileges = Bevoegdheden
account-editor-subject-role = Rol
account-editor-role = Rol
account-editor-no-roles = Deze server heeft geen rollen
account-editor-with-admin-option = Met admin option
account-editor-level = Niveau
account-editor-level-global = Hele server
account-editor-level-database = Database
account-editor-level-schema = Schema
account-editor-level-table = Tabel
account-editor-level-sequence = Reeks
account-editor-no-suggestions = Geen suggesties
account-editor-section-privileges = Bevoegdheden
account-editor-with-grant-option = Met grant option
account-editor-needs-role = Geef een rol op.
account-editor-needs-privileges = Kies een niveau, geef aan waarop het van toepassing is en vink een bevoegdheid aan.
account-editor-password-required = Een wachtwoord is verplicht.
account-editor-name-required = Een naam is verplicht.
account-editor-entra-only-user = Vanuit Microsoft Entra wordt alleen een databasegebruiker gemaakt.
account-editor-entra-no-password = Een Entra-gebruiker heeft hier geen aanmelding of wachtwoord — Entra meldt hem aan.
account-editor-login-needs-password = Een aanmelding heeft een wachtwoord nodig.
account-editor-user-for-login-password = Een gebruiker voor een aanmelding meldt zich aan met het wachtwoord van de aanmelding — laat dit leeg.
account-editor-confirm-password = Typ het wachtwoord nogmaals om het te bevestigen.
account-editor-passwords-differ = De twee wachtwoorden verschillen.
account-editor-password-hashed = De SQL in het voorbeeld, de instructie die wordt uitgevoerd, bevat een hash van het wachtwoord in de vorm waarin deze server het opslaat, niet het wachtwoord zelf — tenzij het tekens bevat die de aanmelding zou normaliseren (letters van volle breedte, ligaturen, vaste spaties), of de server een extensie voor wachtwoordcontrole draait; dan wordt het verstuurd zoals getypt.
account-editor-password-as-typed = Het wachtwoord staat in de SQL van het voorbeeld, de instructie die wordt uitgevoerd.

## Table designer

designer-title-edit = { $table } bewerken
designer-title-create = Tabel maken in { $location }
designer-tab-table = Tabel
designer-tab-columns = Kolommen
designer-tab-indexes = Indexen
designer-tab-foreign-keys = Refererende sleutels
designer-tab-checks = Controles
designer-list-add = Toevoegen
designer-no-suggestions = Geen suggesties
designer-name = Naam
designer-comment = Opmerking
designer-table-comment-placeholder = Waar deze tabel voor is
designer-engine = Engine
designer-collation = Sortering
designer-no-column = Geen kolom geselecteerd.
designer-type = Type
designer-type-hint = Rechtstreeks doorgegeven — de server bepaalt wat het betekent.
designer-nullable = Nullable
designer-nullable-hint = NULL toestaan in deze kolom.
designer-primary-key = Primaire sleutel
designer-primary-key-hint = Deel van de primaire sleutel van de tabel, in kolomvolgorde.
designer-identity = Identity
designer-auto-increment = Auto-increment
designer-identity-hint = De server kent de waarde toe ({ $keyword }).
designer-default = Standaardwaarde
designer-default-hint = SQL-tekst, zoals je die zou schrijven: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Gegenereerd uit
designer-generated-hint = Een expressie, bijv. qty * price. Een gegenereerde kolom heeft geen standaardwaarde.
designer-on-update = Bij update
designer-index-unique-detail = uniek
designer-index-redundant-detail = overbodig
designer-hint-redundant = Index { $index } wordt gedekt door { $cover }: elke opzoeking die hij bedient, bedient die ook.
table-problem-no-name = De tabel heeft een naam nodig.
table-problem-no-columns = Een tabel heeft minstens één kolom nodig.
table-problem-column-no-name = Elke kolom heeft een naam nodig.
table-problem-column-twice = Twee kolommen heten allebei { $name }.
table-problem-column-no-type = Kolom { $name } heeft een type nodig.
table-problem-key-not-column = De primaire sleutel noemt { $column }, en dat is geen kolom.
table-problem-index-no-name = Elke index heeft een naam nodig.
table-problem-index-twice = Twee indexen heten allebei { $name }.
table-problem-index-no-columns = Index { $index } heeft geen kolommen.
table-problem-xml-one-column = Index { $index } moet precies één kolom noemen.
table-problem-xml-no-options = Index { $index } kan niet uniek zijn, geen kolommen insluiten en niet gefilterd zijn: een XML- of ruimtelijke index accepteert niets daarvan.
table-problem-xml-no-using = Index { $index } heeft de clausule USING XML INDEX … FOR … nodig: een secundaire XML-index wordt gebouwd op een primaire of selectieve.
table-problem-selective-no-paths = Index { $index } heeft de FOR (…)-paden nodig: een selectieve XML-index bestaat uit de paden die hij promoveert.
table-problem-xml-parent-gone = Index { $index } is gebouwd op { $parent }, en dat is geen primaire of selectieve XML-index van deze tabel: hij verdwijnt wanneer zijn ouder verdwijnt.
table-problem-not-orderable = Index { $index } kan niet worden geordend: alleen een columnstore-index heeft een ORDER.
table-problem-order-not-column = Index { $index } ordent op { $column }, en dat is geen kolom.
table-problem-order-not-stored = Index { $index } ordent op { $column }, die hij niet opslaat.
table-problem-columnstore-unique = Index { $index } kan niet uniek zijn: een columnstore-index slaat kolommen op, geen sleutels.
table-problem-columnstore-include = Index { $index } kan geen kolommen insluiten: een columnstore-index noemt elke kolom die hij opslaat.
table-problem-columnstore-sorted = Index { $index } kan zijn kolommen niet sorteren: een columnstore-kolom heeft geen richting.
table-problem-clustered-columnstore-filtered = Index { $index } kan niet worden gefilterd: een geclusterde columnstore-index slaat elke rij op.
table-problem-clustered-include = Index { $index } kan geen kolommen insluiten: een geclusterde index bevat al elke kolom.
table-problem-unique-include = Index { $index } kan geen kolommen insluiten: een unieke beperking heeft hier geen INCLUDE.
table-problem-clustered-filtered = Index { $index } kan niet worden gefilterd: een geclusterde index dekt elke rij.
table-problem-constraint-filtered = Index { $index } kan niet worden gefilterd: hij ondersteunt een UNIQUE-beperking, die hier geen voorwaarde accepteert. Verwijder hem en voeg een unieke index met de voorwaarde toe.
table-problem-not-partial = Index { $index } kan niet worden gefilterd: deze index kan hier niet partieel zijn.
table-problem-index-not-column = Index { $index } noemt { $column }, en dat is geen kolom.
table-problem-include-not-column = Index { $index } sluit { $column } in, en dat is geen kolom.
table-problem-cluster-clash = Een tabel heeft maar één geclusterde index, en { $clustered } clusteren hem elk - maak ze op één na allemaal niet-geclusterd.
table-problem-cluster-key = de primaire sleutel
table-problem-cluster-index = index { $index }
table-problem-cluster-and = en
table-problem-fk-no-name = Elke refererende sleutel heeft een naam nodig.
table-problem-fk-twice = Twee refererende sleutels heten allebei { $name }.
table-problem-fk-no-table = Refererende sleutel { $fk } verwijst naar geen enkele tabel.
table-problem-fk-unpaired = Refererende sleutel { $fk } moet elke kolom koppelen aan een kolom waarnaar hij verwijst.
table-problem-fk-not-column = Refererende sleutel { $fk } noemt { $column }, en dat is geen kolom.
table-problem-check-no-name = Elke controlebeperking heeft een naam nodig.
table-problem-check-twice = Twee controlebeperkingen heten allebei { $name }.
table-problem-check-no-predicate = Controle { $name } heeft geen predicaat.
table-problem-without-rowid-no-key = Een WITHOUT ROWID-tabel heeft een primaire sleutel nodig.
table-problem-strict-type = STRICT-tabellen accepteren alleen INT, INTEGER, REAL, TEXT, BLOB of ANY — { $column } is { $data_type }.
designer-hint-unindexed-fk = Refererende sleutel { $fk } heeft geen index die met zijn kolommen begint, dus een delete of sleutelwijziging op de tabel waarnaar hij verwijst, scant deze tabel.
designer-no-index = Geen index geselecteerd. De primaire sleutel staat bij de kolommen.
designer-columns = Kolommen
designer-index-columns-hint-single = De ene xml-, geometry- of geography-kolom die hij indexeert.
designer-index-columns-hint-columnstore = Kolommen die hij opslaat, gescheiden door komma's.
designer-index-columns-hint-prefix = Gescheiden door komma's, in sleutelvolgorde. bio(20) is een prefixlengte; voeg DESC toe om aflopend te sorteren.
designer-index-columns-hint = Gescheiden door komma's, in sleutelvolgorde. Voeg DESC toe voor een aflopende kolom.
designer-index-method = Methode
designer-index-method-hint = Leeg laten voor de standaard (btree).
designer-index-filter = Alleen rijen waar
designer-index-filter-hint = De voorwaarde van een partiële index, zonder de WHERE.
designer-index-storage = Opslag
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Geclusterde columnstore
designer-storage-primary-xml = Primaire XML
designer-storage-selective-xml = Selectieve XML
designer-storage-secondary-xml = Secundaire XML
designer-storage-spatial = Ruimtelijk
designer-index-using = Using
designer-index-using-hint = De clausule na de kolom: de USING XML INDEX … FOR PATH, VALUE of PROPERTY van een secundaire XML-index (of FOR (path) op een selectieve), de [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …) van een selectieve XML-index, of de USING … WITH (…) van een ruimtelijke index.
designer-index-order = Volgorde
designer-index-order-hint = Kolommen waarop de segmenten worden gesorteerd, gescheiden door komma's — vanaf SQL Server 2022, en vanaf 2025 voor een niet-geclusterde index, die alleen sorteert op kolommen die hij opslaat.
designer-index-include = Insluiten
designer-index-include-hint = Kolommen die in de index worden meegenomen maar geen deel van de sleutel zijn, gescheiden door komma's.
designer-index-unique = Uniek
designer-index-unique-hint = Dubbele waarden over deze kolommen weigeren.
designer-fk-detail-unindexed = → { $table } · geen index
designer-no-fk = Geen refererende sleutel geselecteerd.
designer-fk-ref-table = Verwijst naar tabel
designer-fk-on-delete = Bij delete
designer-fk-columns-hint = Gescheiden door komma's, op volgorde gekoppeld aan de kolommen waarnaar wordt verwezen.
designer-fk-ref-columns = Verwijst naar kolommen
designer-no-check = Geen controle geselecteerd.
designer-check-expression = Expressie
designer-check-expression-hint = De voorwaarde waaraan elke rij moet voldoen. Geschreven zonder CHECK (…).
designer-check-enforced = Afgedwongen
designer-check-enforced-hint = Uit legt de beperking vast zonder haar toe te passen — bestaande en nieuwe rijen worden allebei geaccepteerd.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Verbindingen beheren
connections-new = Nieuwe verbinding
connections-import = Verbindingen importeren
connections-export = Verbindingen exporteren
connections-duplicate = Dupliceren
connections-delete = Verwijderen
connections-test = Testen
connections-save = Opslaan
connections-test-failed = Verbinding mislukt
connections-browse = Bladeren…
connections-section-general = Algemeen
connections-section-safeguards = Beveiligingen
connections-section-database = Database
connections-section-network = Netwerk
connections-name = Naam
connections-colour = Kleur
connections-folder = Map
connections-no-folders = Nog geen mappen
connections-folder-hint = Optioneel. Verbindingen met dezelfde map worden samen getoond.
connections-prominent-colour = Opvallende kleur in editor
connections-prominent-colour-hint = De queryeditor omlijsten in de kleur van deze verbinding.
connections-read-only = Alleen-lezen
connections-read-only-hint = Celbewerkingen uitschakelen en schrijf-/DDL-query's op deze verbinding blokkeren.
connections-read-only-mssql = SQL Server heeft geen alleen-lezen sessie, dus leesacties worden uitgevoerd in een transactie die Schemaic terugdraait. Het effect van een procedure buiten de database overleeft dat; voor een garantie maak je verbinding met een aanmelding die alleen kan lezen.
connections-cli-access = CLI-toegang
connections-cli-access-hint = De opdrachtregel schemaic, en AI-agents via schemaic mcp, deze verbinding laten gebruiken.
connections-ai-data-access = AI-toegang tot gegevens
connections-statement-timeout = Time-out voor instructies
connections-statement-timeout-hint = Annuleert een instructie op deze verbinding die langer loopt.
connections-statement-timeout-same = Zelfde als de app
connections-environment = Omgeving
env-none = Geen
env-local = Lokaal
env-development = Ontwikkeling
env-testing = Test
env-staging = Staging
env-production = Productie
connections-type = Type
connections-init-sql = Init-SQL
connections-init-sql-pragma-hint = Optioneel. PRAGMA-instellingen die worden uitgevoerd op elke verbinding die Schemaic opent.
connections-init-sql-set-hint = Optioneel. SET-instructies die worden uitgevoerd op elke verbinding die Schemaic opent — een zoekpad, een tijdzone, een rol.
connections-init-refused = ‘{ $statement }’: { $why }
connections-init-only-pragmas = alleen PRAGMA-instellingen kunnen bij het verbinden worden uitgevoerd
connections-init-only-sets = alleen SET-instructies kunnen bij het verbinden worden uitgevoerd
connections-init-calls-function = een instelling kan bij het verbinden geen functie aanroepen
connections-init-quoted-name = de naam van een instelling kan bij het verbinden niet tussen aanhalingstekens staan
connections-init-names-no-setting = dat noemt geen instelling
connections-init-not-listed = { $name } kan bij het verbinden niet worden ingesteld — het is geen van de sessie-instellingen waarvan Schemaic weet dat ze zijn beveiligingen ongemoeid laten
connections-init-not-utf8 = de clienttekenset moet UTF-8 blijven — de alleen-lezencontrole van Schemaic leest elke instructie als UTF-8
connections-init-one-value = een instelling krijgt bij het verbinden één waarde, en daarna mag niets volgen
connections-init-one-value-after-equals = een instelling krijgt bij het verbinden één waarde, na een =
connections-init-transaction = transactie-instellingen kunnen niet bij het verbinden worden gemaakt — de alleen-lezenbeveiliging stelt ze in
connections-init-pragma = die PRAGMA kan niet bij het verbinden worden uitgevoerd — hij wijzigt het bestand of de alleen-lezenbeveiliging
connections-init-server-scope = { $scope }-instellingen wijzigen de server, niet de sessie
connections-init-variable = een variabele is geen sessie-instelling — ze eindigt met haar batch
connections-init-on-only = { $name } kan bij het verbinden alleen worden aangezet — Schemaic is ervan afhankelijk
connections-host = Host
connections-port = Poort
connections-named-instance-port = Een benoemde instantie: SQL Server Browser geeft de poort, dus Poort wordt niet gebruikt. Om de Browser over te slaan, stel je de host in zonder de instantie en de poort waarop ze luistert.
connections-database = Database
connections-database-hint = Optioneel, en de database waarin de verbinding opent. Sommige gehoste aanbieders staan alleen hun eigen database toe en weigeren een verbinding die die niet noemt.
connections-authentication = Authenticatie
ssh-auth-password = Wachtwoord
ssh-auth-key-pair = Sleutelpaar
ssh-auth-agent = SSH-agent
auth-password = Gebruiker en wachtwoord
auth-windows = Windows (dit account)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Gebruiker
connections-password = Wachtwoord
connections-azure-cli-hint = Meldt aan als degene die bij de Azure CLI is aangemeld (voer eerst `az login` uit). Het token wordt opgehaald wanneer een verbinding er een nodig heeft en nooit opgeslagen.
connections-aws-iam-hint = Meldt aan als de gebruiker hieronder met een token uit de referenties van de AWS CLI (eerst `aws configure` of `aws sso login`). De gebruiker heeft de rol rds_iam (PostgreSQL) of de AWSAuthenticationPlugin (MySQL) nodig. Het token wordt opgehaald wanneer een verbinding er een nodig heeft en nooit opgeslagen.
connections-gcp-iam-hint = Meldt aan als de gebruiker hieronder — de IAM-gebruiker of het serviceaccount dat Cloud SQL kent — met een token van de Google Cloud CLI (eerst `gcloud auth login`). Het token wordt opgehaald wanneer een verbinding er een nodig heeft en nooit opgeslagen.
connections-windows-hint-named = Meldt aan als het Windows-account waaronder Schemaic draait ({ $account }). De server moet dat account kennen als Windows-aanmelding.
connections-windows-hint = Meldt aan als het Windows-account waaronder Schemaic draait. De server moet dat account kennen als Windows-aanmelding.
connections-database-file = Databasebestand
connections-database-file-dialog = SQLite-database selecteren
connections-sqlite-hint = Een SQLite-database is één bestand, dus er is geen server om te bereiken — geen host, gebruiker, wachtwoord of tunnel. Handmatige transacties zijn er nog niet beschikbaar: instructies worden doorgevoerd zodra ze worden verstuurd.
connections-ssh-tunnel = SSH-tunnel
connections-ssh-tunnel-hint = De server bereiken via een SSH-tunnel.
connections-ssh-host = SSH-host
connections-ssh-port = SSH-poort
connections-ssh-user = SSH-gebruiker
connections-ssh-password = SSH-wachtwoord
connections-ssh-agent-hint = Ondertekenen wordt overgelaten aan je draaiende SSH-agent (OpenSSH-agent / Pageant op Windows). Schemaic slaat geen sleutel op.
connections-jump-hosts = Jumphosts
connections-jump-hosts-hint = Optioneel. Hosts om te passeren vóór de SSH-host, dichtstbijzijnde eerst — elk meldt aan op de manier die hieronder is ingesteld.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , sleutel { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , via { $jump }
       *[none] {""}
    }
connections-route-jump-host = jumphost { $host }
connections-route-proxy-command = ~/.ssh/config bereikt { $host } via een ProxyCommand, dat Schemaic niet kan uitvoeren. Geef de route in plaats daarvan op als Jumphosts.
connections-route-match = ~/.ssh/config kan { $host } via een Match-blok routeren, en dat leest Schemaic niet. Geef de route in plaats daarvan op als Jumphosts.
connections-route-match-destination = ~/.ssh/config kan { $host } via een Match-blok naar een andere host of poort laten wijzen, en dat leest Schemaic niet. Geef in plaats daarvan het adres zelf op als SSH-host.
connections-route-include = ~/.ssh/config sluit een bestand in dat Schemaic niet kon lezen, waar het van toepassing is op { $host }. Geef het adres zelf op als SSH-host, en de route als Jumphosts.
connections-route-config-jump-password = ~/.ssh/config bereikt { $host } via { $jump }, en een wachtwoord wordt alleen verstuurd naar de hosts die deze verbinding noemt. Voeg de jumphost toe aan Jumphosts als hij hetzelfde wachtwoord gebruikt, of meld je aan met een sleutel of de SSH-agent.
connections-route-jump-own-proxy-jump = ~/.ssh/config bereikt jumphost { $host } via { $jump }, en Schemaic volgt de eigen ProxyJump van een jumphost niet. Geef de route in plaats daarvan op als Jumphosts, met { $jump } eerst.
connections-jump-host-error = Jumphost { $number }: { $why }
connections-jump-empty = leeg — verwijder de overbodige komma
connections-jump-space = ‘{ $entry }’ bevat een spatie
connections-jump-unclosed = ‘{ $entry }’ opent een [ die het niet sluit
connections-jump-after-bracket = ‘{ $entry }’ heeft tekst na de ]
connections-jump-no-host = ‘{ $entry }’ noemt geen host
connections-jump-not-a-port = ‘{ $entry }’ is geen poort
connections-private-key = Privésleutel
connections-private-key-dialog = Privésleutel selecteren
connections-passphrase = Wachtzin
connections-tls = SSL / TLS
tls-disable = Uitgeschakeld
tls-prefer = Voorkeur
tls-require = Vereist
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Verbinden in platte tekst.
tls-prefer-description = Versleutelen als de server het aanbiedt. Geen verificatie.
tls-require-description = Weigeren te verbinden zonder versleuteling. Geen verificatie.
tls-verify-ca-description = Versleutelen, en het certificaat controleren tegen de CA.
tls-verify-full-description = Versleutelen, het certificaat controleren en de hostnaam controleren.
tls-verify-ca-checks-host = Op MySQL en MariaDB controleert dit ook de hostnaam, dus het gedraagt zich als Verify full — een beperking van het stuurprogramma, geen instelling.
tls-floor-azure-cli = Aanmelden met Microsoft Entra verbindt op Verify full, wat hier ook is gekozen: het token opent elke Azure SQL-database die het account kan bereiken.
tls-floor-aws-iam = Aanmelden met AWS IAM verbindt op minstens Vereist: het token mag niet onversleuteld over het netwerk gaan. Verify full heeft de RDS-CA-bundel als Root CA nodig.
tls-floor-gcp-iam = Aanmelden met Google Cloud IAM verbindt op minstens Verify CA — stel Root CA in op de server-CA van de instantie: het token opent elke Cloud SQL-instantie die het account kan bereiken.
tls-floor-other = Deze aanmeldmethode verbindt op een sterkere modus dan die hier is gekozen.
connections-ca-certificate = CA-certificaat
connections-ca-certificate-dialog = CA-certificaat selecteren
connections-ca-certificate-hint = Leeg laten om de certificeringsinstanties te vertrouwen die je besturingssysteem vertrouwt, inclusief die je organisatie heeft geïnstalleerd. Geef een bestand op om in plaats daarvan dat te vertrouwen.
connections-client-certificate = Clientcertificaat
connections-client-certificate-dialog = Clientcertificaat selecteren
connections-client-key = Clientsleutel
connections-client-key-dialog = Clientsleutel selecteren
connections-client-identity-hint = Een clientcertificaat en -sleutel zijn alleen nodig waar de server vraagt te bewijzen wie je bent (wederzijdse TLS). De sleutel mag niet met een wachtzin zijn beveiligd.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importeren in { $table }
import-section-source = Bron
import-section-reading = Lezen
import-section-columns = Kolommen
import-section-preview = Voorbeeld
import-file = Bestand
import-choose-file = Kies een bestand…
import-file-dialog = Importeren in tabel
import-no-file = Geen bestand gekozen
import-unnamed-file = (bestand)
import-format = Formaat
import-sheet = Werkblad
import-delimiter = Scheidingsteken
import-header-row = Eerste rij is een kop
import-header-row-hint = De eerste rij als kolomnamen gebruiken in plaats van als gegevens.
import-trim = Witruimte bijsnijden
import-trim-hint = Spaties rond elke waarde verwijderen — ook binnen aanhalingstekens.
import-empty-is-null = Leeg veld is NULL
import-empty-is-null-hint = Een leeg veld betekent NULL in plaats van een lege string.
import-null-tokens = Andere waarden die NULL betekenen
import-null-tokens-hint = Gescheiden door komma's, bijv. NULL, \N, NA.
import-skip = Overslaan
import-nothing-mapped = Er is niets toegewezen, dus er is niets te importeren.
import-values-withheld = Het eerste record van dit bestand is te groot voor een voorbeeld, dus alleen de kolomnamen zijn gelezen. Toewijzen en importeren lezen het hele bestand.
import-missing-required = Niet toegewezen, en mag niet leeg zijn: { $columns }. De import mislukt, tenzij de database ze invult.
import-engine-not-transactional = De opslagengine van deze tabel ({ $engine }) is niet transactioneel, dus een mislukte import kan niet ongedaan worden gemaakt — de rijen die vóór de fout zijn geladen, blijven staan.
import-problems = { $count ->
    [one] { $count } probleem in het bestand — er is niets geïmporteerd.
   *[other] { $count } problemen in het bestand — er is niets geïmporteerd.
}
import-problem-at-line = regel { $line }: { $message }
import-problem-at-column = regel { $line }, { $column }: { $message }
import-issue-not-an-integer = geen geheel getal
import-issue-not-a-number = geen getal
import-issue-not-a-boolean = geen waar/onwaar-waarde
import-issue-cell-error = het werkblad kon deze cel niet evalueren
import-issue-null-in-not-null = leeg, maar de kolom mag niet NULL zijn
import-issue-field-count = heeft { $found } velden, { $expected } verwacht
import-tail-cut = { $shown } van { $total } worden getoond.
import-tail-uncounted = Alle { $total } worden getoond — de controle is daar gestopt met tellen, dus het bestand bevat er meer.
import-tail-cut-and-uncounted = { $shown } van de eerste { $total } worden getoond — de controle is daar gestopt met tellen, dus het bestand bevat er meer.
import-memory-json = Dit JSON-bestand is { $size }, en een JSON-import wordt tijdens het laden in het geheugen gehouden — reken op ongeveer { $estimate }. Een CSV met dezelfde gegevens laadt met constant geheugengebruik.
import-memory-xlsx = Deze werkmap is { $size }, en een Excel-import wordt tijdens het laden in het geheugen gehouden — reken op ruwweg { $estimate }, omdat het bestand gecomprimeerd is. Een CSV van hetzelfde werkblad laadt met constant geheugengebruik.
import-read-failed = Kon het bestand niet lezen: { $error }
import-no-columns-mapped = Er zijn geen bestandskolommen aan tabelkolommen toegewezen
import-preview-record-too-large = het eerste JSON-record is groter dan de { $mib } MiB die het voorbeeld leest, dus er is niets te tonen
import-refuse-not-objects = JSON-objecten verwacht (een array ervan, of één per regel)
import-refuse-oversize = Deze werkmap is { $size }, en een Excel-bestand moet in zijn geheel worden gelezen voordat er iets van kan worden getoond — { $max } is het maximum dat hiermee kan worden geopend. Exporteer het werkblad als CSV en importeer dat; een CSV laadt met constant geheugengebruik.
import-refuse-oversize-unsized = Deze werkmap is groter dan { $max }, en een Excel-bestand moet in zijn geheel worden gelezen voordat er iets van kan worden getoond — { $max } is het maximum dat hiermee kan worden geopend. Exporteer het werkblad als CSV en importeer dat; een CSV laadt met constant geheugengebruik.
import-refuse-inflated = Deze werkmap is { $size } op schijf, maar pakt uit tot minstens { $unpacked }, en een Excel-bestand moet in zijn geheel worden gelezen voordat er iets van kan worden getoond — { $ceiling } aan uitgepakte inhoud is het maximum dat hiermee wordt geopend, en het vasthouden daarvan kost meer geheugen dan dat. Exporteer het werkblad als CSV en importeer dat; een CSV laadt met constant geheugengebruik.
import-refuse-no-such-sheet = deze werkmap heeft geen werkblad met de naam ‘{ $sheet }’ — ze heeft { $sheets }
import-refuse-no-sheets = deze werkmap heeft geen werkbladen
import-refuse-too-wide = dit werkblad claimt { $width } kolommen; een Excel-werkblad bevat er { $max }. Exporteer het als CSV en importeer dat.
import-cancelled = Importeren geannuleerd — de transactie is teruggedraaid, dus er is niets geschreven.
import-done = { $count ->
    [one] { $count } rij geïmporteerd in { $table }.
   *[other] { $count } rijen geïmporteerd in { $table }.
}
import-next = Volgende
import-reading = Lezen…
import-back = Terug
import-import = Importeren
import-importing = Importeren…
dialog-read-only = Deze verbinding is alleen-lezen.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = { $database }.{ $schema } exporteren naar { $format }
dump-title = { $database } exporteren naar { $format }
dump-sql-dialog = Exporteren naar SQL
dump-folder-dialog = Exporteren naar { $format }
dump-folder-hint = Eén { $format }-bestand per tabel, geschreven in een map die je kiest.
dump-all-selected = Alle { $total } geselecteerd
dump-some-selected = { $chosen } van { $total } geselecteerd
dump-select-all = Alles selecteren
dump-select-none = Niets
dump-reading-tables = De tabellenlijst lezen…
dump-no-tables = Deze database heeft geen tabellen.
dump-tables-unreadable = De tabellenlijst kon niet worden gelezen.
dump-section-contents = Inhoud
dump-structure = Structuur
dump-structure-hint = CREATE TABLE, de triggers ervan, en de refererende sleutels, na de gegevens opnieuw opgegeven.
dump-data = Gegevens
dump-data-hint = Elke rij, als INSERT-instructies.
dump-other-objects = Andere objecten
dump-other-objects-hint = Typen, reeksen, routines en events waarop deze tabellen steunen.
dump-section-replaying = Terugspelen
dump-drop-before-create = Verwijderen vóór aanmaken
dump-drop-hint-up-front = Elke tabel en view verwijderen vóór de CREATE ervan, zodat het bestand laadt in een database die ze al bevat. Met Eén transactie worden ook alle routines in hun schema's vervangen, en wordt er gestopt voordat er iets wordt verwijderd dat niet ongewijzigd kan worden teruggezet. Zonder Eén transactie blijven routines zoals ze zijn, dus terugspelen in een database die ze nog heeft, stopt bij de eerste. De eigenaar en machtigingen van elke tabel worden aan het eind teruggezet.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS vóór elke CREATE, zodat het bestand laadt in een database die deze tabellen al bevat. Ook worden alle routines in hun schema's vervangen. De eigenaar, rechten en rijbeveiliging van elke tabel worden aan het eind teruggezet.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS vóór elke CREATE, zodat het bestand laadt in een database die deze tabellen al bevat. Ook worden alle routines en events in hun schema's vervangen: op MySQL wordt elk eerst verwijderd, wat alle rechten erop intrekt, en het bestand kent ze aan het eind opnieuw toe waar je account mysql.procs_priv kan lezen.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS vóór elke CREATE, zodat het bestand laadt in een database die deze tabellen al bevat.
dump-one-transaction = Eén transactie
dump-one-transaction-hint = Het laden omsluiten, zodat een fout niets achterlaat.
dump-disable-fk-checks = Controles op refererende sleutels uitschakelen
dump-fk-hint = Uit zolang het laden duurt, zodat de volgorde waarin rijen binnenkomen ze niet kan laten weigeren.
dump-fk-hint-drops = Uit zolang het laden duurt, zodat de volgorde waarin rijen binnenkomen ze niet kan laten weigeren. Een bestand dat tabellen verwijdert, zet ze hoe dan ook uit: een drop terwijl ze aan staan, verwijdert via hun ON DELETE-acties de rijen van tabellen buiten het bestand.
dump-no-fk-guard-hint = Deze engine heeft geen sessieschakelaar voor controles op refererende sleutels; het bestand voegt de beperkingen in plaats daarvan na de gegevens toe.
dump-export = Exporteren
dump-progress = { $count ->
    [one] { $table } schrijven — { $index } van { $total }, tot nu toe { $rows } rij
   *[other] { $table } schrijven — { $index } van { $total }, tot nu toe { $rows } rijen
}
dump-reading-schema = Het schema lezen
dump-replace-files = Bestanden vervangen
dump-replace-prompt = { $n ->
    [one] { $n } bestand in { $folder } wordt vervangen en kan niet worden hersteld: { $names }.
   *[other] { $n } bestanden in { $folder } worden vervangen en kunnen niet worden hersteld: { $names }.
}
dump-replace-prompt-more = { $n } bestanden in { $folder } worden vervangen en kunnen niet worden hersteld: { $names } en nog { $more }.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] { $n } tabel geschreven.
   *[other] { $n } tabellen geschreven.
}
dump-done-missing = { $n ->
    [one] { $n } aangevinkte tabel niet gevonden en staat niet in het bestand: { $names }.
   *[other] { $n } aangevinkte tabellen niet gevonden en staan niet in het bestand: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } object waarom de export vroeg, staat niet in het bestand: { $names }.
   *[other] { $n } objecten waarom de export vroeg, staan niet in het bestand: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] De rijen van graph-edgetabel { $names } staan niet in het bestand; ze wordt leeg aangemaakt.
   *[other] De rijen van graph-edgetabellen { $names } staan niet in het bestand; ze worden leeg aangemaakt.
}
dump-done-refused = { $n ->
    [one] Terugspelen in een database die dit al bevat, stopt voordat er iets wordt verwijderd: het bestand kan het niet ongewijzigd terugzetten: { $names }.
   *[other] Terugspelen in een database die een van deze al bevat, stopt voordat er iets wordt verwijderd: het bestand kan ze niet ongewijzigd terugzetten: { $names }.
}
dump-cancelled-partial = Exporteren geannuleerd — { $name } is niet gewijzigd; wat al was geschreven, staat in { $part }
files-done = { $n ->
    [one] { $n } bestand geschreven.
   *[other] { $n } bestanden geschreven.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Exporteren geannuleerd — er is geen bestand voltooid, dus er is niets naar { $folder } geschreven.
files-cancelled = { $n ->
    [one] Exporteren geannuleerd — { $n } bestand is voltooid in { $folder }; de tabel die bezig was, is niet geschreven.
   *[other] Exporteren geannuleerd — { $n } bestanden zijn voltooid in { $folder }; de tabel die bezig was, is niet geschreven.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } bestand dat al naar { $folder } is geschreven, blijft bewaard.
   *[other] { $message } — { $n } bestanden die al naar { $folder } zijn geschreven, blijven bewaard.
}
files-replaced = { $n ->
    [one] { $n } bestaand bestand is vervangen: { $names }.
   *[other] { $n } bestaande bestanden zijn vervangen: { $names }.
}
files-missing = { $n ->
    [one] { $n } aangevinkte tabel niet gevonden en is geen bestand geworden: { $names }.
   *[other] { $n } aangevinkte tabellen niet gevonden en zijn geen bestand geworden: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } van ~{ $total } rij
   *[other] { $rows } van ~{ $total } rijen
}
export-progress-of = { $count ->
    [one] { $rows } van { $total } rij
   *[other] { $rows } van { $total } rijen
}
export-progress-rows = { $count ->
    [one] { $rows } rij
   *[other] { $rows } rijen
}
export-progress-starting = Starten
export-progress-file = Bestand: { $name }
export-progress-writing = { $name } schrijven

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Livemonitor
monitor-badge-watching = Bewaken
monitor-badge-paused = Gepauzeerd
monitor-badge-stopped = Gestopt
monitor-filter-all = Alle
monitor-filter-inserts = Inserts
monitor-filter-updates = Updates
monitor-filter-deletes = Deletes
monitor-resume = Peilen hervatten
monitor-pause = Peilen pauzeren
monitor-clear-tip = Het logboek wissen
monitor-export-tip = Het logboek exporteren…
monitor-interval = { $secs } s
monitor-clear-title = Het logboek wissen?
monitor-clear-message = { $count ->
    [one] { $count } wijziging zou worden verworpen. Het logboek is de enige vastlegging van wat een verwijderde rij bevatte — het peilen meldt een wijziging niet nog eens — en dit kan niet ongedaan worden gemaakt. Exporteer het eerst als je het wilt bewaren.
   *[other] { $count } wijzigingen zouden worden verworpen. Het logboek is de enige vastlegging van wat een verwijderde rij bevatte — het peilen meldt een wijziging niet nog eens — en dit kan niet ongedaan worden gemaakt. Exporteer het eerst als je het wilt bewaren.
}
monitor-col-time = Tijd
monitor-col-action = Actie
monitor-no-inserts = Nog geen inserts.
monitor-no-updates = Nog geen updates.
monitor-no-deletes = Nog geen deletes.
monitor-empty-stopped = Gestopt.
monitor-empty-paused = Gepauzeerd.
monitor-waiting = Wachten
monitor-events-since = { $count ->
    [one] { $count } gebeurtenis sinds { $since }
   *[other] { $count } gebeurtenissen sinds { $since }
}
monitor-no-changes = Nog geen wijzigingen
monitor-partial = alleen de eerste { $cap } rijen op primaire sleutel worden bewaakt
monitor-capped = de oudste vallen weg bij de limiet van { $cap } wijzigingen
monitor-length-only = { $count ->
    [one] { $columns } is binair, dus de wijzigingen ervan worden alleen aan de lengte gezien
   *[other] { $columns } zijn binair, dus de wijzigingen ervan worden alleen aan de lengte gezien
}
monitor-newest-at-bottom = nieuwste onderaan
monitor-export-dialog = Wijzigingslogboek exporteren

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Resultaten vergelijken
diff-filter-all = Alle
diff-filter-changed = Gewijzigd
diff-filter-added = Toegevoegd
diff-filter-removed = Verwijderd
diff-match-rows-by = Rijen koppelen op
diff-whole-row = Hele rij
diff-export-tip = De vergelijking exporteren…
diff-nothing-to-export = Niets om te exporteren.
diff-export-dialog = Vergelijking exporteren
diff-unnamed-file = de vergelijking
diff-ran-at = Uitgevoerd om { $time }
diff-side-facts = { $count ->
    [one] { $count } rij · { $ms } ms
   *[other] { $count } rijen · { $ms } ms
}
diff-only-in = Alleen in { $side }: { $columns }
diff-uncompared = Binair, niet vergeleken: { $columns }
diff-missing-key = beide resultaten hebben een kolom met de naam { $column } nodig om rijen op te koppelen
diff-binary-key = { $column } bevat binaire waarden, die niet worden vergeleken, dus er kunnen geen rijen op worden gekoppeld
diff-summary-identical = { $n ->
        [one] Geen verschillen · { $n } rij ongewijzigd
       *[other] Geen verschillen · { $n } rijen ongewijzigd
    }
diff-summary-changed = { $n } gewijzigd
diff-summary-added = { $n } toegevoegd
diff-summary-removed = { $n } verwijderd
diff-summary-unchanged = { $n } ongewijzigd
diff-summary-not-compared = { $columns } niet vergeleken
diff-repeated-keys = { $count ->
    [one] { $count } rij herhaalt een sleutel die aan haar kant al bezet is, en wordt weggelaten.
   *[other] { $count } rijen herhalen een sleutel die aan hun kant al bezet is, en worden weggelaten.
}
diff-capped = Een resultaat is afgekapt: rijen voorbij de limiet worden niet vergeleken.
diff-col-change = Wijziging
diff-no-differences = Geen verschillen.
diff-no-changed = Geen gewijzigde rijen.
diff-no-added = Geen toegevoegde rijen.
diff-no-removed = Geen verwijderde rijen.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Voordat je toepast
preview-risk-irreversible = Dit kan niet ongedaan worden gemaakt

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Tabel { $name } maken met { $columns } kolom
   *[other] Tabel { $name } maken met { $columns } kolommen
}
ddl-line-rebuild-table = { $name } opnieuw opbouwen — dit kan niet ter plekke worden gewijzigd, dus de rijen worden naar een nieuwe tabel gekopieerd die de plaats van de oude inneemt
ddl-line-drop-table = De tabel verwijderen
ddl-line-drop-foreign-table = De foreign table verwijderen
ddl-line-truncate-table = Elke rij verwijderen
ddl-line-rename-table = De tabel hernoemen naar { $to }
ddl-line-add-column = Kolom { $name } { $datatype } toevoegen{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Kolom { $name } verwijderen
ddl-line-rebuild-computed-column = Berekende kolom { $name } als ({ $expression }) opnieuw opbouwen rond de kolomwijziging
ddl-line-rename-column = Kolom { $from } hernoemen naar { $to }{ $retyped ->
        [yes] , en het type wijzigen van { $old_type } in { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Kolom { $name } naar het begin verplaatsen
ddl-line-move-column-after = Kolom { $name } na { $after } plaatsen
ddl-line-move-column = Kolom { $name } verplaatsen
ddl-line-retype-column = Kolom { $name } wijzigen van { $from } in { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Kolom { $name } wijzigen{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (geen)
ddl-line-add-primary-key = Primaire sleutel toevoegen ({ $columns })
ddl-line-drop-primary-key = De primaire sleutel verwijderen ({ $columns })
ddl-line-rebuild-primary-key = De primaire sleutel ({ $columns }) opnieuw opbouwen rond de kolomwijziging
ddl-line-change-primary-key = De primaire sleutel wijzigen van ({ $from }) in ({ $to })
ddl-line-add-index = Toevoegen: { $unique ->
        [yes] unieke{" "}
       *[no] {""}
    }{ $has_kind ->
        [yes] { $kind }{" "}
       *[no] {""}
    }index { $name }{ $has_method ->
        [yes] {" "}met { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}op ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}inclusief ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}geordend op ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}waar { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = geclusterde columnstore
ddl-index-kind-primary-xml = primaire XML
ddl-index-kind-selective-xml = selectieve XML
ddl-index-kind-secondary-xml = secundaire XML
ddl-index-kind-spatial = ruimtelijke
ddl-line-drop-index = Index { $name } verwijderen
ddl-line-rename-index = Index { $from } hernoemen naar { $to }
ddl-line-keep-lossy-index = Index { $name } ongewijzigd laten — Schemaic kan hem niet volledig lezen
ddl-line-add-foreign-key = Refererende sleutel { $name } → { $table }({ $columns }) toevoegen
ddl-line-drop-foreign-key = Refererende sleutel { $name } verwijderen
ddl-line-add-check = Controle { $name } ({ $expression }) toevoegen
ddl-line-drop-check = Controle { $name } verwijderen
ddl-line-table-options = Tabel instellen: { $parts }
ddl-table-option-engine = engine op { $engine }
ddl-table-option-collation = sortering op { $collation }
ddl-table-option-comment = opmerking
ddl-table-option-comment-cleared = opmerking (gewist)
ddl-line-create-view = View { $name } maken
ddl-line-create-view-index = View { $name } en de index { $index } ervan maken
ddl-line-create-view-indexes = View { $name } en de indexen { $indexes } ervan maken
ddl-line-redefine-view = View { $name } opnieuw definiëren
ddl-line-recreate-view = View { $name } verwijderen en opnieuw maken
ddl-line-rename-view = De view hernoemen naar { $to }
ddl-line-drop-view = De view verwijderen
ddl-line-drop-materialized-view = De materialized view verwijderen
ddl-line-refresh-view = De rijen van de materialized view opnieuw opbouwen — lezers worden geblokkeerd tot dat klaar is
ddl-line-refresh-view-concurrently = De rijen van de materialized view opnieuw opbouwen zonder lezers te blokkeren
ddl-line-create-trigger = Trigger { $name } maken
ddl-line-redefine-trigger = Trigger { $name } opnieuw definiëren
ddl-line-recreate-trigger = Trigger { $name } opnieuw maken
ddl-line-recreate-trigger-as = Trigger { $from } opnieuw maken als { $name }
ddl-line-drop-trigger = Trigger { $name } verwijderen
ddl-line-create-routine = Maken: { $what } { $name }
ddl-line-redefine-routine = Opnieuw definiëren: { $what } { $name }
ddl-line-redefine-routine-as = Opnieuw definiëren: { $what } { $from } als { $name }
ddl-line-recreate-routine = Opnieuw maken: { $what } { $name }
ddl-line-recreate-routine-as = Opnieuw maken: { $what } { $from } als { $name }
ddl-line-rename-routine = Hernoemen: { $what } { $from } naar { $to }
ddl-line-drop-routine = Verwijderen: { $what } { $name }
ddl-line-create-event = Event { $name } maken
ddl-line-alter-event = Event { $name } wijzigen
ddl-line-alter-event-edits = Event { $name } wijzigen: { $edits }
ddl-line-drop-event = Event { $name } verwijderen
ddl-event-edit-rename = hernoemen naar { $to }
ddl-event-edit-schedule = planning op { $schedule }
ddl-event-edit-status-enabled = status op ingeschakeld
ddl-event-edit-status-disabled = status op uitgeschakeld
ddl-event-edit-status-replica = status op uitgeschakeld op replica
ddl-event-edit-preserve = bewaren na voltooiing
ddl-event-edit-no-preserve = verwijderen na voltooiing
ddl-event-edit-definer = definer op { $definer }
ddl-event-edit-comment = opmerking
ddl-event-edit-body = inhoud
ddl-line-create-enum = { $values ->
    [one] Type { $name } maken met { $values } waarde
   *[other] Type { $name } maken met { $values } waarden
}
ddl-line-add-enum-value = Waarde { $value } toevoegen
ddl-line-add-enum-value-after = Waarde { $value } toevoegen na { $after }
ddl-line-add-enum-value-before = Waarde { $value } toevoegen vóór { $before }
ddl-line-rename-enum-value = Waarde { $from } hernoemen naar { $to }
ddl-line-recreate-enum = Type { $name } opnieuw maken ({ $values ->
    [one] { $values } waarde
   *[other] { $values } waarden
}, met hercasten van { $columns ->
    [one] { $columns } kolom
   *[other] { $columns } kolommen
})
ddl-line-create-domain = Domein { $name } maken als { $datatype }
ddl-line-set-domain-default = De standaardwaarde van het domein instellen op { $default }
ddl-line-drop-domain-default = De standaardwaarde van het domein verwijderen
ddl-line-domain-not-null = Het domein NOT NULL maken
ddl-line-domain-nullable = Het domein nullable maken
ddl-line-recreate-domain = Domein { $name } opnieuw maken als { $datatype } (was { $was }; met hercasten van { $columns ->
    [one] { $columns } kolom
   *[other] { $columns } kolommen
})
ddl-line-create-sequence = Reeks { $name } maken
ddl-line-alter-sequence = Reeks instellen: { $edits }
ddl-sequence-edit-type = type op { $datatype }
ddl-sequence-edit-increment = stapgrootte op { $value }
ddl-sequence-edit-minimum = minimum op { $value }
ddl-sequence-edit-maximum = maximum op { $value }
ddl-sequence-edit-start = begin op { $value }
ddl-sequence-edit-cache = cache op { $value }
ddl-sequence-edit-cycling-on = cyclisch aan
ddl-sequence-edit-cycling-off = cyclisch uit
ddl-sequence-edit-owner = eigenaar op { $owner }
ddl-sequence-edit-no-owner = eigenaar op geen
ddl-line-restart-sequence = De reeks herstarten op { $value }
ddl-line-create-object = Maken: { $what } { $name }
ddl-line-replace-object = Verwijderen en opnieuw maken: { $what } { $name }
ddl-line-replace-object-as = Verwijderen en opnieuw maken als { $to }: { $what } { $name }
ddl-line-alter-tsql-sequence = De reeks wijzigen: { $clauses }
ddl-line-rename-object = Hernoemen naar { $to }: { $what }
ddl-line-drop-object = Verwijderen: { $what }
ddl-line-set-object-comment = Opmerking instellen: { $what }
ddl-line-clear-object-comment = Opmerking wissen: { $what }
ddl-line-create-database = Database { $name } maken
ddl-line-drop-database = Database { $name } verwijderen
ddl-line-create-schema = Schema { $name } maken
ddl-line-drop-schema = Schema { $name } verwijderen
ddl-line-create-extension = Extensie { $name } maken
ddl-line-create-user = Gebruiker { $who } maken
ddl-line-create-role = Rol { $who } maken
ddl-line-create-login = Aanmelding { $who } maken
ddl-line-drop-user = Gebruiker { $who } verwijderen
ddl-line-drop-role = Rol { $who } verwijderen
ddl-line-drop-login = Aanmelding { $who } verwijderen
ddl-line-set-password = Wachtwoord instellen voor { $who }
ddl-line-grant-privileges = { $privileges } toekennen aan { $who }
ddl-line-revoke-privileges = { $privileges } intrekken van { $who }
ddl-privileges-none = geen bevoegdheden
ddl-privileges-count = { $count } bevoegdheden
ddl-line-grant-role = { $role } toekennen aan { $member }
ddl-line-revoke-role = { $role } intrekken van { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Verwijdert { $name } en maakt de tabel opnieuw, kopieert elke rij over en zet de sleutels, indexen, controles, standaardwaarden en triggers terug, de eigen opmerkingen en die van de kolommen, en de refererende sleutels die andere tabellen erop hebben. Er wordt gestopt voordat er iets begint als de tabel nog iets anders heeft wat Schemaic niet herhaalt — machtigingen, een eigen eigenaar, een schemagebonden afhankelijk object, systeemversiebeheer, een ledger, replicatie, compressie, indexopties, een uitgeschakelde index, een gebonden regel of standaardwaarde, een gevoeligheidsclassificatie, een beschrijving op een sleutel, index, standaardwaarde, controle of trigger. Met de hand gemaakte statistieken worden niet overgenomen.
       *[other] Verwijdert { $name } en maakt de tabel opnieuw, en kopieert elke rij over. Alles aan de tabel wat Schemaic niet heeft gelezen, wordt niet overgenomen.
    }
ddl-risk-drop-table = Verwijdert de tabel en elke rij erin.
ddl-risk-drop-foreign-table = Verwijdert de foreign table: de definitie verdwijnt, en de rijen die ze leest blijven waar ze zijn, op de externe server of in het bestand; een view of routine die haar noemt, mislukt vanaf dan.
ddl-risk-truncate-table = Verwijdert elke rij in de tabel. Dit kan niet ongedaan worden gemaakt.
ddl-risk-drop-column = Verwijdert kolom { $name } en alle gegevens erin.
ddl-risk-rebuild-computed-column = Verwijdert berekende kolom { $name } en voegt haar weer toe, zodat ze naar het eind van de tabel verhuist.
ddl-risk-column-not-null = Kolom { $name } wordt NOT NULL — de instructie mislukt als een rij NULL bevat.
ddl-risk-column-stops-generated = Kolom { $name } wordt niet meer gegenereerd. De huidige waarden blijven behouden, maar niets berekent ze vanaf nu opnieuw.
ddl-risk-column-becomes-generated = Kolom { $name } wordt gegenereerd uit { $expression }. Elke waarde erin wordt vervangen door wat de expressie berekent.
ddl-risk-column-expression-changes = De expressie van kolom { $name } verandert van { $from } in { $to }. Op PostgreSQL wordt de kolom daarvoor verwijderd en weer toegevoegd, dus elke waarde wordt opnieuw berekend en de kolom verhuist naar het eind van de tabel.
ddl-risk-type-narrows = Het versmallen van { $subject } van { $from } naar { $to } { $loss ->
        [truncates] kapt waarden af die niet meer passen
        [rounds] rondt elke waarde in de kolom af
       *[both] kapt waarden af die niet meer passen en rondt elke waarde in de kolom af
    }.
ddl-risk-type-rewrites = Het wijzigen van { $subject } van { $from } in { $to } herschrijft elke waarde; het kan mislukken of precisie verliezen.
ddl-risk-domain-subject = domein { $name }
ddl-risk-primary-key-dropped = Laat de tabel zonder primaire sleutel achter — rijen kunnen niet meer vanuit het raster worden bewerkt.
ddl-risk-primary-key-rebuilt = Verwijdert de primaire sleutel en voegt hem weer toe rond de kolomwijziging. Als het de geclusterde index is, wordt daarmee de hele tabel herschreven, en een refererende sleutel in een andere tabel die ernaar verwijst, stopt het plan.
ddl-risk-primary-key-replaced = Vervangt de primaire sleutel: { $count ->
        [one] { $columns } is
       *[other] { $columns } zijn
    } niet langer uniek, en duplicaten worden vanaf nu geaccepteerd. Op MySQL wordt hiermee de geclusterde index opnieuw opgebouwd, wat de hele tabel herschrijft.
ddl-risk-drop-foreign-key = Verwijdert refererende sleutel { $name }. Rijen zonder bijbehorende ouder worden vanaf nu geaccepteerd, en bestaande gegevens worden niet opnieuw gecontroleerd als de sleutel weer wordt toegevoegd.
ddl-risk-drop-unique-index = Verwijdert unieke index { $name }. Dubbele waarden worden vanaf nu geaccepteerd, en het raster kan de sleutel kwijtraken waarmee het rijen bewerkte.
ddl-risk-drop-view = { $materialized ->
        [yes] Verwijdert de materialized view. { $cost }
       *[no] Verwijdert de view. { $cost }
    }
ddl-risk-view-cost-sqlite = Views die eruit selecteren, worden niet meer opgelost tot hij terug is, en zijn INSTEAD OF-triggers worden mee verwijderd en niet hersteld.
ddl-risk-view-cost-sqlite-restored = Views die eruit selecteren, worden niet meer opgelost tot hij terug is, en zijn INSTEAD OF-triggers worden mee verwijderd.
ddl-risk-view-cost-sqlserver = Rechten erop, de beschrijving en de INSTEAD OF-triggers worden mee verwijderd en niet hersteld.
ddl-risk-view-cost-sqlserver-restored = Rechten erop en de beschrijving worden mee verwijderd en niet hersteld; de INSTEAD OF-triggers worden ook verwijderd.
ddl-risk-view-cost-other = Afhankelijke views, regels, rechten en de INSTEAD OF-triggers worden mee verwijderd en niet hersteld.
ddl-risk-view-cost-other-restored = Afhankelijke views, regels en rechten worden mee verwijderd en niet hersteld; de INSTEAD OF-triggers worden ook verwijderd.
ddl-risk-recreate-view = Het opnieuw maken van { $name } verwijdert hem eerst. { $cost } { $why ->
        [sqlite] SQLite kan een view niet ter plekke vervangen, dus elke bewerking ervan — ook een hernoeming — gaat via deze route.
        [sqlserver] Met sp_rename van SQL Server zou de opgeslagen definitie van de view nog de oude naam noemen, dus een hernoeming gaat via deze route.
       *[postgres] PostgreSQL kan geen view vervangen waarvan de kolommen van naam, type of volgorde zijn veranderd, dus dit is de enige manier om de bewerking toe te passen.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } instructie die mee is verwijderd, wordt daarna teruggezet, in dezelfde transactie.
       *[other] { $count } instructies die mee zijn verwijderd, worden daarna teruggezet, in dezelfde transactie.
    }
ddl-risk-view-indexes-dropped = Het opnieuw definiëren van view { $view } verwijdert { $dropped_count ->
        [one] zijn index
       *[other] zijn indexen
    } ({ $dropped }) — ALTER VIEW van SQL Server doet dat altijd — { $case ->
        [all] en dit plan bouwt { $again_count ->
            [one] hem
           *[other] ze
        } daarna opnieuw op, in dezelfde transactie. Bij een grote view duurt dat net zo lang als het oorspronkelijke opbouwen van { $again_count ->
            [one] die index
           *[other] die indexen
        }. Een index met een fill factor, compressie, padding, vergrendelingsopties, een bestandsgroep of een beschrijving stopt het plan voordat er iets verandert — Schemaic kan die niet opnieuw opbouwen. Met de hand gemaakte statistieken op de view worden niet overgenomen.
        [none] en dit plan bouwt { $gone_count ->
            [one] hem
           *[other] ze
        } niet opnieuw op: de view waaraan hij gelijk wordt gemaakt, heeft zo'n index niet. Met de hand gemaakte statistieken op de view worden niet overgenomen.
       *[some] en dit plan bouwt { $again } daarna opnieuw op, in dezelfde transactie, maar niet { $gone }, dat de view waaraan hij gelijk wordt gemaakt niet heeft. Een index met een fill factor, compressie, padding, vergrendelingsopties, een bestandsgroep of een beschrijving stopt het plan voordat er iets verandert — Schemaic kan die niet opnieuw opbouwen. Met de hand gemaakte statistieken op de view worden niet overgenomen.
    }
ddl-risk-add-not-null-column = Kolom { $name } is NOT NULL zonder standaardwaarde. Als de tabel al rijen heeft, is er geen waarde om ze te geven, en de engine weigert het hele plan.
ddl-risk-drop-check = Verwijdert controle { $name }. Rijen die de beperking weigerde, worden vanaf nu geaccepteerd, en bestaande gegevens worden niet opnieuw gecontroleerd als ze weer wordt toegevoegd.
ddl-risk-keep-lossy-index = Je bewerking van index { $name } is niet opgenomen. Hij gebruikt iets wat Schemaic niet kan teruglezen — een expressie als sleutelkolom, een operatorklasse of een NULLS-ordening — en de bewerking toepassen zou betekenen dat de index zonder dat opnieuw wordt gemaakt. Bewerk deze index in plaats daarvan in SQL.
ddl-risk-drop-trigger = Verwijdert trigger { $name }. Wat hij bij elke schrijfactie deed, gebeurt niet meer, en op rijen die vanaf nu worden geschreven, wordt het niet toegepast.
ddl-risk-redefine-trigger-in-place = Definieert trigger { $name } ter plekke opnieuw — hij wordt niet verwijderd, dus zijn machtigingen blijven — en wat hij bij elke schrijfactie doet, verandert vanaf nu.
ddl-risk-recreate-trigger = Het opnieuw maken van trigger { $name } verwijdert hem eerst. Waar DDL niet transactioneel is (MySQL), laat een nieuwe definitie die de server weigert de tabel zonder trigger achter.
ddl-risk-signature-lost = { $kind ->
        [trigger] Trigger
        [function] Functie
       *[procedure] Procedure
    } { $name } is ondertekend (ADD SIGNATURE), en elke wijziging verwijdert de handtekening — Schemaic kan opnieuw ondertekenen niet zonder de sleutel van het certificaat. Wat het certificaat toestond, werkt niet meer tot je ADD SIGNATURE opnieuw uitvoert.
ddl-risk-redefines-routine = Definieert { $name } opnieuw. Alles wat deze routine aanroept, voert vanaf nu de nieuwe inhoud uit, ook triggers op andere tabellen die deze bewerking nooit noemde.
ddl-risk-routine-parameters-change = Wijzigt de parameterlijst van { $name }. Aanroepers die de oude argumenten doorgeven, komen er niet meer bij uit.
ddl-risk-recreate-routine = Het opnieuw maken van { $held } verwijdert het eerst. Waar DDL niet transactioneel is (MySQL), laat een nieuwe definitie die de server weigert helemaal geen { $what } achter.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Bevoegdheden die op { $held } zijn toegekend en de opmerking ervan overleven het verwijderen niet, en dit plan herstelt ze niet — pas daarna elke GRANT en COMMENT opnieuw toe.
       *[no] Bevoegdheden die op { $held } zijn toegekend, overleven het verwijderen niet, en dit plan herstelt ze niet — pas daarna elke GRANT opnieuw toe.
    }
ddl-risk-drop-numbered-group = Verwijdert { $what } { $name } en daarmee elk lid van de genummerde groep: { $members }.
ddl-risk-drop-routine = Verwijdert { $what } { $name }. PostgreSQL weigert dit zolang een trigger deze routine nog gebruikt, dus zulke triggers moeten eerst worden verwijderd.
ddl-risk-drop-event = Verwijdert event { $name }. De planning en de inhoud verdwijnen mee — niets anders in de database bewaart een kopie.
ddl-risk-one-shot-event = Dit event wordt één keer uitgevoerd en daarna door de server verwijderd — dat is ON COMPLETION NOT PRESERVE. Zet Bewaren aan om het te behouden.
ddl-risk-one-shot-event-altered = Event { $name } wordt één keer uitgevoerd en daarna door de server verwijderd — dat is ON COMPLETION NOT PRESERVE. Zet Bewaren aan om het te behouden.
ddl-risk-add-enum-value = Het toevoegen van { $value } kan niet ter plekke ongedaan worden gemaakt — PostgreSQL kan geen enumwaarde verwijderen, dus terugdraaien betekent het type opnieuw maken. De waarde kan ook pas worden gebruikt nadat dit plan is toegepast.
ddl-risk-rename-enum-value = Elke rij met { $from } leest vanaf nu { $to }. Als je { $from } wilde verwijderen in plaats van hernoemen, verwijder het dan en pas dat apart toe.
ddl-risk-recreate = Het opnieuw maken van { $kind ->
        [domain] domein
       *[type] type
    } { $name } verwijdert het en bouwt het opnieuw op. Elke kolom hieronder wordt via tekst gehercast; een waarde die de nieuwe definitie niet accepteert, laat het hele plan mislukken, en er wordt niets toegepast{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}en nog { $more }
        }. Een view of functie die erop is gebouwd, kan hier niet worden getoond en zou de server laten weigeren.
       *[no] . Niets gebruikt het nu, maar een view of functie die erop is gebouwd, kan hier niet worden getoond en zou de server laten weigeren.
    }
ddl-risk-drop-domain-check = Verwijdert controle { $name }. Geen enkele kolom van dit domein wordt nog gecontroleerd, en bestaande gegevens worden niet opnieuw gecontroleerd als ze weer wordt toegevoegd.
ddl-risk-domain-nullable = Elke kolom van dit domein gaat NULL accepteren, ook kolommen in tabellen die deze bewerking nooit noemde.
ddl-risk-domain-not-null = De instructie mislukt als een kolom van dit domein al NULL bevat.
ddl-risk-drop-sequence = Verwijdert de reeks en de positie die ze had bereikt. De server weigert dit zolang de standaardwaarde van een kolom er nog uit put, dus die standaardwaarden moeten eerst weg.
ddl-risk-drop-synonym = Verwijdert het synoniem. Niets weigert dit: een view, routine of query die het noemt, mislukt vanaf dan wanneer die weer wordt uitgevoerd.
ddl-risk-drop-extension = Verwijdert de extensie en de functies, typen en operatoren die ze heeft geïnstalleerd. De server weigert dit zolang iets van jou er nog van afhangt — een kolom van haar type, een index of een server op haar wrapper — en noemt wat dat is.
ddl-risk-drop-used-object = Verwijdert het object ({ $what }). De server weigert dit zolang een kolom het nog gebruikt, dus die kolommen moeten eerst van type veranderen.
ddl-risk-restart-sequence = Herstart de teller op { $value }. Al uitgegeven waarden worden niet gewijzigd, dus een sleutel die hij opnieuw bereikt, botst.
ddl-risk-replace-synonym = Verwijdert het synoniem en maakt het opnieuw. Niets weigert het verwijderen: de twee instructies worden in één transactie uitgevoerd.
ddl-risk-replace-sequence = Verwijdert de reeks en maakt haar opnieuw: de bereikte positie gaat verloren, en ze begint opnieuw bij { $start }. De server weigert het verwijderen zolang de standaardwaarde van een kolom er nog uit put.
ddl-risk-start-value = haar beginwaarde
ddl-risk-replace-object = Verwijdert het object ({ $what }) en maakt het opnieuw. De server weigert het verwijderen zolang een kolom het nog gebruikt, dus die kolommen moeten eerst van type veranderen.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Geweigerd zolang er machtigingen op staan, het een eigen eigenaar heeft, uitgebreide eigenschappen of een gebonden regel of standaardwaarde: het opnieuw maken zou die niet herstellen.
       *[no] Geweigerd zolang er machtigingen op staan, het een eigen eigenaar heeft of uitgebreide eigenschappen: het opnieuw maken zou die niet herstellen.
    }
ddl-risk-drop-database = Verwijdert de database { $name } en alles erin — elke tabel en al haar rijen, elke view, index, routine en trigger. Er blijft niets bewaard en er is geen ongedaan maken: de enige weg terug is een back-up die is gemaakt voordat dit wordt uitgevoerd.
ddl-risk-drop-schema = Verwijdert het schema { $name }. De server weigert dit zolang het nog tabellen, views of routines bevat, dus dit verwijdert een leeg schema of mislukt{ $cascade ->
        [yes] {" "}— Schemaic verstuurt nooit CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Verwijdert de aanmelding { $who }, en daarmee elke servermachtiging en elk serverrollidmaatschap dat ze heeft — nergens anders vastgelegd, dus een aanmelding die opnieuw onder deze naam wordt gemaakt, begint zonder. De server weigert dit zolang er iets als deze aanmelding verbonden is, dus haar sessies moeten eerst eindigen. Haar gebruikers in elke database worden niet verwijderd: ze blijven, met hun machtigingen, aan geen aanmelding gekoppeld, en een nieuwe aanmelding met deze naam koppelt zich niet opnieuw aan hen zonder ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Verwijdert { $who } en elke bevoegdheid die het account heeft. De rechten zijn nergens anders vastgelegd, dus het account terugzetten betekent ze allemaal uit het hoofd opnieuw toekennen. { $cuts_off ->
        [yes] Alles wat als dit account verbonden is, mislukt bij de volgende instructie in deze database.
       *[no] Alles wat nog als dit account verbonden is, blijft draaien tot de verbinding wordt verbroken.
    }
ddl-risk-set-password = Wijzigt het wachtwoord van { $who }. Het oude kan niet worden hersteld, dus alles wat er nog mee is ingesteld — een applicatie, een taak, de client van een andere ontwikkelaar — kan geen verbinding maken tot het met de hand is bijgewerkt. Al geopende sessies blijven draaien.
ddl-risk-revoke-privileges = Neemt { $privileges } af van { $who }. Alles wat ervan afhangt — een applicatie, een taak, een view van iemand anders — mislukt bij de volgende instructie.{ $cascades ->
        [yes] {" "}De intrekking heeft CASCADE, dus ze wordt ook afgenomen van elk account waaraan { $who } haar heeft toegekend, en wat als die accounts draait, mislukt op dezelfde manier.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Waar { $who } haar in plaats daarvan is geweigerd (DENY), heft de intrekking de weigering op — wat de toegang teruggeeft als een rol waartoe het account behoort haar toekent.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}Op een tabel verwijdert ze ook elke toekenning en DENY van { $cleared } op kolomniveau die { $who } op de kolommen van de tabel heeft: een kolom die { $who } was geweigerd, is niet langer geweigerd, en een kolom die apart was toegekend, is niet langer toegekend.
       *[no] {""}
    }
ddl-risk-revoke-role = Neemt de rol { $role } af van { $member }, en daarmee elke bevoegdheid die de rol meebrengt.
ddl-risk-blank-password = { $who } krijgt geen wachtwoord: iedereen die de server kan bereiken, kan zich als dit account aanmelden. Stel er hier een in, of later met ALTER USER.
ddl-risk-host-is-everywhere = De host is % — elke computer op het netwerk, niet alleen deze.
ddl-risk-grant-server-wide = Kent { $privileges } toe aan { $who } op de hele server — de instantie zelf, niet de database die wordt bekeken.
ddl-risk-grant-every-database = Kent { $privileges } toe aan { $who } op elke database op deze server — ook databases die nog niet bestaan, niet alleen de database die wordt bekeken.
ddl-risk-grant-keeps-column-denies = Een toekenning van { $privileges } op tabelniveau verwijdert elke DENY ervan die { $who } op een van de kolommen van de tabel heeft, dus de instructie weigert die kolommen direct na het toekennen opnieuw aan { $who }: een kolom die { $who } was geweigerd, blijft geweigerd. Waar { $who } ook de grant option op de tabel heeft, weigert de server opnieuw te weigeren zonder CASCADE, en wordt er niets toegepast.
ddl-risk-star-refresh = Vernieuwt elke view en inline functie in deze database die select * uit { $table } doet, rechtstreeks of via een synoniem: SQL Server bindt die op positie aan de kolommen van de tabel, en zou anders de verkeerde kolommen lezen en schrijven. Het plan mislukt als een ervan niet meer compileert tegen de nieuwe kolommen. Een view in een andere database die select * uit { $table } doet, wordt vernieuwd nadat het plan is doorgevoerd, buiten de transactie: tot dan leest hij de oude posities, en als het vernieuwen mislukt, blijft het plan toegepast en wordt de view hier genoemd.
ddl-risk-piecewise = SQL Server maakt, wijzigt of verwijdert geen systeemeigen gecompileerde module binnen een transactie, dus dit plan voert één instructie tegelijk uit en kan niet worden gestopt: als een instructie mislukt, blijven de voorgaande toegepast.
ddl-risk-drop-sequence-table = Verwijdert de reeks: de bereikte waarde gaat verloren, en alles wat er nog uit put, mislukt.
ddl-risk-drop-scale = Volgens de statistieken van de schemaboom zijn dat { $rows } rijen.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = het voorstel is voor tabel `{ $proposed }`, maar `{ $actual }` is geopend
propose-no-ops = het voorstel vraagt om geen wijzigingen
propose-no-such-column = `{ $table }` heeft geen kolom met de naam `{ $name }`
propose-column-exists = `{ $table }` heeft al een kolom met de naam `{ $name }`
propose-no-such-index = `{ $table }` heeft geen index met de naam `{ $name }`
propose-index-exists = `{ $table }` heeft al een index met de naam `{ $name }`
propose-no-such-foreign-key = `{ $table }` heeft geen refererende sleutel met de naam `{ $name }`
propose-no-such-check = `{ $table }` heeft geen controlebeperking met de naam `{ $name }`
propose-key-width-mismatch = een refererende sleutel heeft evenveel kolommen nodig waarnaar wordt verwezen als verwijzende kolommen
propose-empty-index = een index heeft minstens één kolom nodig
propose-invalid = de voorgestelde tabel zou niet geldig zijn: { $problem }
propose-not-a-table = `{ $name }` is een view — een voorstel kan alleen een tabel wijzigen
propose-no-such-table = geen tabel met de naam `{ $name }` — roep list_schema aan om te zien wat er bestaat
propose-not-an-expression = { $field ->
        [default] de standaardwaarde `{ $text }` is geen enkele SQL-expressie
        [generated] de gegenereerde expressie `{ $text }` is geen enkele SQL-expressie
       *[check] de controle-expressie `{ $text }` is geen enkele SQL-expressie
    }
propose-not-a-type = `{ $text }` is geen kolomtype — geef alleen het type, zonder standaardwaarde, zonder NOT NULL en zonder iets erachter
propose-unknown-action = `{ $text }` is geen actie voor een refererende sleutel
propose-not-loaded = { $table } is nu niet geladen in { $database } — open de database in de schemaboom, of wacht tot een vernieuwing klaar is, en probeer het opnieuw.
propose-nothing-to-change = { $subject } ziet er al zo uit — er valt niets te wijzigen.
propose-no-database = Voor dit tabblad is geen database geselecteerd, dus een schemawijziging die hiervandaan wordt toegepast, zou nergens heen kunnen. Kies er eerst een voor deze verbinding — als elke database verborgen is, haal je er een terug via het oog bij SCHEMA.
propose-other-connection = Deze wijziging gaat over een tabblad op een andere verbinding — schakel daarnaar over, of selecteer de verbinding ervan in de boom, voordat je een schemawijziging toepast.
ai-run-no-database = Voor dit tabblad is geen database geselecteerd, dus een instructie die hiervandaan wordt uitgevoerd, zou nergens heen kunnen. Kies er eerst een voor deze verbinding — als elke database verborgen is, haal je er een terug via het oog bij SCHEMA.
ai-run-other-connection = Deze chat gaat over een tabblad op een andere verbinding. Een nieuw tabblad zou openen op de verbinding die in de boom is geselecteerd, dus de instructie zou ergens anders worden uitgevoerd — schakel eerst over naar dat tabblad, of selecteer de verbinding ervan in de boom.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, die dit plan hernoemt naar { $to }
ddl-moved-dropped = { $name }, die dit plan verwijdert
ddl-withheld-unreplayable-rename = { $count ->
        [one] Het hernoemen van { $was } naar { $to } kan zijn { $count } trigger niet meenemen: de SQL ervan noemt { $was }, en die hier herschrijven zou veranderen wat jij hebt geschreven. Hernoem de view apart nadat je de trigger opnieuw hebt gemaakt met de nieuwe naam.
       *[other] Het hernoemen van { $was } naar { $to } kan zijn { $count } triggers niet meenemen: hun SQL noemt { $was }, en die hier herschrijven zou veranderen wat jij hebt geschreven. Hernoem de view apart nadat je de triggers opnieuw hebt gemaakt met de nieuwe naam.
    }
ddl-withheld-numbered-group = { $name } staat aan het hoofd van een genummerde proceduregroep ({ $members }), en dit plan verwijdert hem eerst, wat elk lid van de groep mee verwijdert. Bewerk hem onder zijn eigen naam en soort; dan wordt hij ter plekke gewijzigd en blijven ze behouden.
ddl-withheld-view-index-create = { $count ->
        [one] Bij het maken van view { $view } kan de index { $indexes } niet worden gemaakt — van een soort die Schemaic niet kan herhalen. Maak deze view in plaats daarvan in SQL.
       *[other] Bij het maken van view { $view } kunnen de indexen { $indexes } niet worden gemaakt — van een soort die Schemaic niet kan herhalen. Maak deze view in plaats daarvan in SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Het bewerken van view { $view } verwijdert de index { $indexes }, die Schemaic niet opnieuw kan maken — van een soort die Schemaic niet kan herhalen. Bewerk deze view in plaats daarvan in SQL.
       *[other] Het bewerken van view { $view } verwijdert de indexen { $indexes }, die Schemaic niet opnieuw kan maken — van een soort die Schemaic niet kan herhalen. Bewerk deze view in plaats daarvan in SQL.
    }
ddl-withheld-tsql-lossy-index = Het opnieuw opbouwen van { $table } zou de index { $index } verwijderen: die is van een soort die Schemaic niet volledig leest
ddl-withheld-tsql-encrypted-trigger = Het opnieuw opbouwen van { $table } zou de trigger { $trigger } verwijderen: de tekst ervan is versleuteld
ddl-withheld-tsql-signed-trigger = Het opnieuw opbouwen van { $table } zou de handtekening op de trigger { $trigger } verwijderen, en die kan niet worden teruggezet zonder de privésleutel van het certificaat
ddl-withheld-tsql-renamed-trigger = Het opnieuw opbouwen van { $table } onder een nieuwe naam zou de trigger { $trigger } de oude naam laten noemen
ddl-withheld-partly-read = Index { $index } is maar deels gelezen — SQLite bewaart het predicaat van een partiële index, en een expressiesleutel, alleen in de eigen CREATE-tekst van de index{ $why ->
        [may-name] , en die tekst kan { $moved } noemen. Verwijder de index om zonder verder te gaan.
        [not-found] , dus hij kan niet worden gemaakt uit wat is gelezen. Verwijder de index om zonder verder te gaan.
        [edited] , dus deze bewerking ervan kan niet worden geschreven — wat is gelezen, is niet de hele index. Maak de wijziging ongedaan om de index te houden zoals hij is, of verwijder hem.
       *[no-text] {" "}— en voor deze is zo'n tekst er niet, dus hem opnieuw maken zou veranderen wat hij indexeert. Verwijder de index om zonder verder te gaan.
    }
ddl-withheld-condition-names-dropped = De voorwaarde van index { $index } noemt { $column }, die dit plan verwijdert. Bewerk de voorwaarde of verwijder eerst de index.
ddl-withheld-strands-triggers = Deze tabel heeft { $count ->
        [one] { $count } trigger
       *[other] { $count } triggers
    }, en hun SQL wordt teruggespeeld precies zoals SQLite die heeft opgeslagen — en die zou nog steeds { $moved } noemen. SQLite accepteert zo'n trigger en weigert daarna elke schrijfactie naar de tabel. Verwijder of bewerk eerst de trigger, of hernoem de kolom zonder andere wijziging, zodat de eigen ALTER TABLE van SQLite hem kan herschrijven.
ddl-withheld-strands-other-trigger = { $owner } noemt { $moved }. Een herbouw hernoemt deze tabel onder die trigger door en laat de SQL ervan precies zoals hij was, dus de trigger zou een kolom blijven noemen die weg is — SQLite accepteert zo'n trigger en weigert daarna elke schrijfactie naar *die* tabel. Verwijder of bewerk eerst de trigger, of hernoem de kolom zonder andere wijziging, zodat de eigen ALTER TABLE van SQLite hem kan herschrijven.
ddl-withheld-trigger-on = Trigger { $name } op { $table }
ddl-withheld-some-trigger = Een trigger op een andere tabel
ddl-withheld-strands-generated-column = Gegenereerde kolom { $column } rekent met { $moved }. Een herbouw schrijft de tabel uit het model, en niets herschrijft die expressie — SQLite weigert de nieuwe tabel met ‘no such column’ en het hele plan wordt teruggedraaid. Bewerk of verwijder eerst { $column }.
ddl-withheld-cannot-restate = De declaratie van deze tabel bevat { $clauses }, waarvoor het model van Schemaic geen veld heeft. Een herbouw schrijft de tabel terug uit dat model, dus de clausule zou verdwijnen uit een tabel die er nog goed uitzag — en elke schrijfactie die ervan afhangt, zou zich vanaf dan anders gedragen. Wijzig deze tabel in plaats daarvan met een script (DDL kopiëren is daar het begin van), of haal de clausule er eerst af.
ddl-sqlite-clauses = { $first } en { $last }
ddl-sqlite-clause-deferrable = de DEFERRABLE-clausule van een refererende sleutel
ddl-sqlite-clause-on-conflict = een ON CONFLICT-clausule
ddl-sqlite-clause-desc-key = een DESC-kolom in de primaire sleutel
ddl-sqlite-clause-collate-key = een COLLATE op een kolom van de primaire sleutel
ddl-withheld-virtual-table = `{ $name }` is een virtuele tabel. De kolommen zijn de interface van een module in plaats van een opslagdeclaratie, en de echte gegevens staan in schaduwtabellen die een herbouw zou verwijderen — een FTS5-index zou verdwijnen uit een tabel die de tekst nog bevatte. Wijzig hem in plaats daarvan met een script (DROP en CREATE VIRTUAL TABLE), of bewerk de tabellen die hij indexeert.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } view
   *[other] { $count } views
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } routine
   *[other] { $count } routines
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } trigger
   *[other] { $count } triggers
}
ddl-used-by-count-event = { $count ->
    [one] { $count } event
   *[other] { $count } events
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } refererende sleutel
   *[other] { $count } refererende sleutels
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } tabelexpressie
   *[other] { $count } tabelexpressies
}
ddl-used-by-counts = { $first } en { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}en nog { $more }
    }
ddl-used-by-drop = Gebruikt door { $listing }.
ddl-used-by-change = { $subject } wordt gebruikt door { $listing }. { $then ->
        [renamed] Die noemen na de hernoeming nog steeds { $name }.
        [dropped] Die breken wanneer het wordt verwijderd.
       *[retyped] Controleer ze tegen het nieuwe type.
    }
ddl-used-by-column = Kolom { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = De refererende sleutels tussen deze tabellen vormen een kringloop, dus geen volgorde van aanmaken voldoet aan allemaal. Eén instructie wordt geweigerd omdat ze verwijst naar een tabel die nog niet bestaat, en MySQL noch MariaDB draait DDL terug.
ddl-risk-clash = { $holder } bevat nog { $names }, wat nodig is om { $claimer } te maken — { $why ->
        [not-dropped] en dit plan verwijdert het niet
        [default] en het kan niet eerst worden verwijderd, omdat de standaardwaarde van een kolom eruit put
       *[referenced] en het kan niet eerst worden verwijderd, omdat een andere tabel ernaar verwijst
    }. { $cost ->
        [no-rollback] Eén instructie wordt geweigerd, en MySQL noch MariaDB draait DDL terug.
        [rolls-back] Eén instructie wordt geweigerd, wat de hele migratie terugdraait.
       *[refused] Eén instructie wordt geweigerd.
    }

preview-title = Wijzigingen toepassen op { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } wijziging
   *[other] { $count } wijzigingen
}
preview-used-by = Gebruikt door
preview-withheld = Deze engine kan een deel van dit plan niet uitdrukken
preview-withheld-hint = Er wordt niets toegepast zolang dit hier staat.
preview-omitted = Niet opgenomen in deze migratie
preview-omitted-hint = Toepassen laat deze objecten zoals ze zijn.
preview-copy = Kopiëren
preview-open-in-editor = Openen in editor
preview-back = Terug
preview-apply = Toepassen
preview-applying = Toepassen…
preview-applied = { $count ->
    [one] { $count } instructie toegepast op { $subject }.
   *[other] { $count } instructies toegepast op { $subject }.
}
preview-refreshed = Het schema is vernieuwd.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Verbindingen importeren
conn-import-url = Verbindings-URL
conn-import-add = Toevoegen
conn-import-url-hint = Een verbindings-URL, een JDBC-URL of een DATABASE_URL-regel uit een .env — één per regel.
conn-import-choose-file = Kies een bestand…
conn-import-scan = Geïnstalleerde clients scannen
conn-import-scanning = Scannen…
conn-import-docker = Docker-containers
conn-import-asking-docker = Docker vragen…
conn-import-looking = Verbindingen zoeken op deze computer…
conn-import-none-found = Geen verbindingen gevonden. DBeaver, de JetBrains-IDE's en de opdrachtregelclients bewaren hun bestanden op bekende plekken; staat het jouwe ergens anders, kies het dan hierboven.
conn-import-nothing-yet = Nog niets gevonden — plak een URL, lees een bestand of scan naar clients die je al gebruikt.
conn-import-found = { $count ->
    [one] { $count } verbinding gevonden
   *[other] { $count } verbindingen gevonden
}
conn-import-select-all = Alles selecteren
conn-import-select-none = Niets
conn-import-note-no-password = Geen wachtwoord in de bron
conn-import-note-already-saved = Al opgeslagen
conn-import-note-unexpanded-path = Pad bevat een niet-uitgevouwen macro
conn-import-note-pgpass = Wachtwoord uit je eigen .pgpass gehaald
conn-import-note-port-assumed = De bron noemde geen poort; dit is de standaard
conn-import-note-external-login = Meldt aan op een manier die Schemaic niet kan: stel een SQL-aanmelding in
conn-import-skipped = { $count ->
    [one] { $count } item is niet geïmporteerd: { $names }.
   *[other] { $count } items zijn niet geïmporteerd: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } item is niet geïmporteerd: { $names } en nog { $rest }.
   *[other] { $count } items zijn niet geïmporteerd: { $names } en nog { $rest }.
}
conn-import-reason-unsupported-engine = niet-ondersteunde engine ({ $driver })
conn-import-reason-no-server = geen server om mee te verbinden
conn-import-reason-not-published = de poort is niet naar deze computer gepubliceerd
conn-import-reason-unreadable = kon niet worden gelezen ({ $error })
conn-import-reason-not-an-export = kon niet worden gelezen (geen export van Schemaic-verbindingen: { $error })
conn-import-reason-newer-export = kon niet worden gelezen (geëxporteerd door een nieuwere Schemaic (versie { $version }) — werk bij om het te importeren)
conn-import-url-empty = Plak een verbindings-URL.
conn-import-url-no-scheme = Geen schema — een URL begint met mysql://, postgresql://, sqlserver:// of sqlite:, een SQL Server-verbindingsreeks met Server=.
conn-import-url-unknown-scheme = Onbekend schema ‘{ $scheme }’ — verwacht werd mysql, mariadb, postgresql, sqlserver of sqlite.
conn-import-url-no-host = Geen host in de URL.
conn-import-url-bad-port = ‘{ $port }’ is geen poortnummer.
conn-import-url-no-path = Geen databasebestand na sqlite:.
conn-import-url-transport = ‘{ $server }’ wordt bereikt via named pipes of LocalDB — Schemaic verbindt alleen via TCP.
conn-import-url-userinfo = De servernaam bevat een @ — een gebruikersnaam of wachtwoord hoort in een eigen veld, niet in de server.
conn-import-url-not-sql-server = Deze verbindingsreeks zegt niet dat ze van SQL Server is — de .NET-stuurprogramma's van MySQL en PostgreSQL gebruiken dezelfde trefwoorden. Plak in plaats daarvan de URL van de server (mysql://, postgresql://, sqlserver://), of schrijf voor SQL Server de server als tcp:host.
conn-import-row-overflow = De eerste { $shown } van { $total } worden getoond. De overige { $hidden } worden nog steeds geïmporteerd als ze zijn aangevinkt — deze lijst stopt hier zodat het venster reageert.
conn-import-import = Importeren
conn-import-import-count = { $count } importeren

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = PNG-afbeelding
blob-kind-jpeg = JPEG-afbeelding
blob-kind-gif = GIF-afbeelding
blob-kind-bmp = BMP-afbeelding
blob-kind-webp = WebP-afbeelding
blob-kind-ico = Pictogram
blob-kind-opaque = Binaire gegevens
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · de eerste { $shown } worden getoond
blob-pane-preview = Voorbeeld
blob-pane-hex = Hex
blob-reading = Lezen…
blob-empty = Deze cel is NULL, of de rij ervan bestaat niet meer.
blob-preview-too-large = Deze afbeelding is { $width } × { $height } — te groot om hier te tonen. De bytes zijn intact: lees ze als Hex, of sla ze op in een bestand.
blob-preview-truncated = Alleen de eerste { $shown } van { $key ->
    [png] deze PNG-afbeelding
    [jpeg] deze JPEG-afbeelding
    [gif] deze GIF-afbeelding
    [bmp] deze BMP-afbeelding
    [webp] deze WebP-afbeelding
    [ico] dit pictogram
   *[other] deze waarde
} zijn gelezen, dus het kan niet worden getekend. Lees de bytes als Hex.
blob-preview-unreadable = Deze bytes beginnen als { $key ->
    [png] een PNG-afbeelding
    [jpeg] een JPEG-afbeelding
    [gif] een GIF-afbeelding
    [bmp] een BMP-afbeelding
    [webp] een WebP-afbeelding
    [ico] een pictogram
   *[other] een afbeelding
}, maar kunnen niet als zodanig worden gelezen. Lees ze als Hex, of sla ze op in een bestand.
blob-save-truncated = Alleen de eerste { $shown } van { $size } konden worden gelezen, dus dit zou het begin van de waarde opslaan.
blob-load = Laden uit bestand
blob-save = Opslaan in bestand
blob-load-dialog = Binaire waarde laden uit bestand
blob-save-dialog = Binaire waarde opslaan
blob-saved-to = Opgeslagen in { $path }
blob-loaded-file = Bestand geladen. Voer door in het raster om het te schrijven.
blob-not-writable = Deze cel kan niet worden geschreven.
blob-file-too-large = Dat bestand is { $got } — deze kolom bevat maximaal { $most }.
blob-not-accepting = Die cel accepteert geen waarde meer — er is niets klaargezet.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = ER-diagram
erd-scope = Bereik
erd-chip-tables = { $count ->
    [one] { $count } tabel
   *[other] { $count } tabellen
}
erd-chip-relationships = { $count ->
    [one] { $count } relatie
   *[other] { $count } relaties
}
erd-chip-hidden = { $count } niet-gerelateerde verborgen
erd-find-placeholder = Tabel of kolom zoeken
erd-find-none = Geen overeenkomsten
erd-find-matches = { $hits ->
    [one] { $hits } overeenkomst
   *[other] { $hits } overeenkomsten
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } overeenkomst in { $tables } tabellen
   *[other] { $hits } overeenkomsten in { $tables } tabellen
}
erd-more-columns = +{ $count } meer
erd-show-less = minder tonen
erd-not-loaded = Het schema is voor deze database nog niet geladen.
erd-no-relationships = Geen relaties via refererende sleutels om in een diagram te zetten.
erd-export-tip = Het diagram exporteren…
erd-copy-as = Kopiëren als
erd-export-dialog = Diagram exporteren
erd-nothing-to-export = Niets om te exporteren.
erd-nothing-to-copy = Niets om te kopiëren.
erd-copied-as = Gekopieerd als { $format }
erd-clipboard-unavailable = Het klembord is niet beschikbaar.
erd-copy-failed = Kon niet kopiëren: { $error }
erd-saved = { $name } opgeslagen
erd-unnamed-file = het bestand

## Server variables modal (server_vars_view.rs)

server-vars-title = Servervariabelen — { $connection }
server-vars-search = Instellingen doorzoeken
server-vars-variables = Variabelen
server-vars-status = Status
server-vars-loading = Laden…
server-vars-unsupported = De engine van deze verbinding publiceert geen lijst van deze soort.
server-vars-no-match = Geen overeenkomende instellingen.
server-vars-more = { $count ->
    [one] Nog { $count } instelling komt overeen — verfijn de zoekopdracht
   *[other] Nog { $count } instellingen komen overeen — verfijn de zoekopdracht
}
server-vars-count = { $count ->
    [one] { $count } instelling
   *[other] { $count } instellingen
}
server-vars-count-filtered = { $shown } van { $count } instellingen
server-vars-refresh = Vernieuwen

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Fragment bewerken — { $name }
snippet-edit-name = Naam
snippet-edit-name-placeholder = Naam van fragment
snippet-edit-shortcut = Uitvouwafkorting
snippet-edit-shortcut-none = geen
snippet-edit-query = Query
snippet-edit-save = Opslaan

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Queryplan
plan-analyze-unavailable = Analyze niet beschikbaar (instructie schrijft gegevens)
plan-analyze-hint = Voert de instructie uit om echte tijden te meten
plan-explaining = Plan opvragen
plan-potential-issues = Mogelijke problemen
plan-detail-no-index = geen index gebruikt
plan-detail-key-unused = index niet gebruikt voor opzoeken
plan-my-full-scan = Volledige tabelscan op `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Volledige tabelscan op het resultaat ({ $detail }, type = ALL)
plan-my-filesort = Filesort op `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort op het resultaat (Extra: Using filesort)
plan-my-temporary = Tijdelijke tabel voor `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Tijdelijke tabel voor het resultaat (Extra: Using temporary)
plan-pg-seq-scan = Sequentiële scan op `{ $table }` (geen index gebruikt)
plan-pg-seq-scan-unnamed = Sequentiële scan op een tabel (geen index gebruikt)
plan-pg-sort = Sort-knooppunt — de query sorteert rijen ter plekke
plan-ms-full-scan = Volledige scan van `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Volledige scan van een tabel ({ $op })
plan-ms-sort = Sort — de query sorteert rijen ter plekke
plan-ms-spool = { $op } — een tussentijdse werktabel in tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = geen join-predicaat — elke rij wordt met elke rij gecombineerd
plan-server-no-statistics = een kolom die hij leest, heeft geen statistieken
plan-server-spill = uitgeweken naar tempdb — de geheugentoekenning was te klein
plan-server-convert = een typeconversie beïnvloedt het plan ({ $issue }): { $expression }
plan-server-unmatched-indexes = een gefilterde index kon niet worden gebruikt
plan-server-memory-grant = de geheugentoekenning was verkeerd ingeschat
plan-server-wait = wachtte op { $wait }
plan-server-other = de server waarschuwt: { $name }
plan-missing-index = Ontbrekende index op `{ $table }` ({ $keys })
plan-missing-index-include = Ontbrekende index op `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Ontbrekende index op `{ $table }` ({ $keys }) — de server verwachtte een verbetering van { $impact }%
plan-missing-index-include-impact = Ontbrekende index op `{ $table }` ({ $keys }; include { $include }) — de server verwachtte een verbetering van { $impact }%
plan-ask-ai = AI vragen
plan-ms-operation = Bewerking
plan-ms-object = Object
plan-ms-estimated-rows = Geschatte rijen
plan-ms-estimated-cost = Geschatte kosten
plan-ms-actual-rows = Werkelijke rijen
plan-ms-executions = Uitvoeringen
plan-ms-statement = Instructie { $n }: { $kind }
plan-ms-statement-in-procedure = Instructie { $n }: { $kind } in procedure { $name }
plan-ms-statement-in-function = Instructie { $n }: { $kind } in functie { $name }
plan-ms-statement-in-a-procedure = Instructie { $n }: { $kind } in een procedure
plan-ms-statement-in-a-function = Instructie { $n }: { $kind } in een functie
plan-ms-plan = Plan
plan-ms-no-plan = Geen instructie in deze batch heeft een plan.
plan-ms-unread = Kon het plan niet lezen: { $reason }
plan-ms-unread-not-xml = geen XML-document
plan-ms-unread-not-showplan = ShowPlanXML verwacht, { $found } gevonden
plan-ms-unread-empty = een leeg document

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Gebruikt door — { $object }
used-by-copy = Kopiëren
used-by-not-loaded = Het schema van { $database } is nog niet geladen — vouw het uit in de boom en vraag het opnieuw.
used-by-nothing-in = Niets in { $database } gebruikt het.
used-by-nothing = Niets gebruikt het.
used-by-limits = Gevonden door definities in deze database te lezen: SQL die tijdens het uitvoeren wordt opgebouwd en andere databases worden niet gezien.
used-by-dropped-with-it = { $detail } · wordt mee verwijderd
used-by-heading-view = { $count ->
    [one] View ({ $count })
   *[other] Views ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Routine ({ $count })
   *[other] Routines ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Triggers ({ $count })
}
used-by-heading-event = { $count ->
    [one] Event ({ $count })
   *[other] Events ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Refererende sleutel ({ $count })
   *[other] Refererende sleutels ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Tabelexpressie ({ $count })
   *[other] Tabelexpressies ({ $count })
}
used-by-noun-view = view
used-by-noun-routine = routine
used-by-noun-trigger = trigger
used-by-noun-event = event
used-by-noun-foreign-key = refererende sleutel
used-by-noun-expression = tabelexpressie

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Donker
theme-light = Licht

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = CODE
md-code-copy = Kopiëren
md-code-insert = Invoegen
md-code-run = Uitvoeren
md-proposal-title = Voorgestelde wijziging van { $table }
md-proposal-count = { $count ->
    [one] { $count } wijziging
   *[other] { $count } wijzigingen
}
md-proposal-review = Wijziging bekijken…
md-proposal-unreadable = De assistent stelde een wijziging voor die niet kon worden gelezen — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Kiezen…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Nu
cell-calendar-today = Vandaag
cell-month-1 = januari
cell-month-2 = februari
cell-month-3 = maart
cell-month-4 = april
cell-month-5 = mei
cell-month-6 = juni
cell-month-7 = juli
cell-month-8 = augustus
cell-month-9 = september
cell-month-10 = oktober
cell-month-11 = november
cell-month-12 = december
cell-weekday-mon = ma
cell-weekday-tue = di
cell-weekday-wed = wo
cell-weekday-thu = do
cell-weekday-fri = vr
cell-weekday-sat = za
cell-weekday-sun = zo

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Alias hernoemen
alias-rename-title-cte = CTE hernoemen
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = CTE-naam
alias-rename-new-name = Nieuwe naam
alias-rename-rename = Hernoemen

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Er is hier niets meer om te hernoemen.
alias-rename-empty = Geef het een naam.
alias-rename-conflict = ‘{ $name }’ is al een naam in deze query: ernaar hernoemen zou veranderen welke tabel een verwijzing leest.
alias-rename-unsure = Een verwijzing hier kan een andere kolom met deze naam lezen, of deze lezen waar de hernoeming niet kan volgen, en het geladen schema kan dat niet zeggen — dus er wordt niets bewerkt. Laad het schema van de tabellen, of hernoem met de hand.
alias-rename-changed = De query is gewijzigd terwijl dit open stond. Sluit het en hernoem opnieuw.
alias-rename-read-only = De editor is alleen-lezen totdat de AI-suggestie is geaccepteerd of afgewezen.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } vindplaats van { $name }
   *[other] { $count } vindplaatsen van { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = Ongeldige SVG: { $error }
raster-too-large = Afbeelding is te groot om te rasteren ({ $width }×{ $height })
raster-png-failed = PNG-codering mislukt: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Kopiëren
grid-menu-copy-value = Waarde kopiëren
grid-menu-copy-formatted = Opgemaakt kopiëren
grid-menu-copy-as = Kopiëren als
grid-menu-paste = Plakken
grid-menu-fill-down = Omlaag doorvoeren
grid-menu-fill-up = Omhoog doorvoeren
grid-menu-edit-field = Veld bewerken
grid-menu-edit-row = Rij bewerken
grid-menu-view-row = Rij bekijken
grid-menu-edit-binary = Binair bewerken
grid-menu-view-binary = Binair bekijken
grid-menu-filter-by-value = Filteren op deze waarde
grid-menu-exclude-value = Deze waarde uitsluiten
grid-menu-follow-relation = Relatie volgen
grid-menu-referenced-by = Verwezen door
grid-menu-set-null = Instellen op NULL
grid-menu-set-default = Instellen op DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Rij dupliceren
   *[other] { $count } rijen dupliceren
}
grid-menu-delete-rows = { $count ->
    [one] Rij verwijderen
   *[other] { $count } rijen verwijderen
}
grid-menu-undo-delete = Verwijderen ongedaan maken
grid-menu-ai-summary = AI-samenvatting
grid-attach-to-chat = { $scope } aan chat toevoegen
grid-attach-scope-rows = { $rows ->
    [one] { $rows } rij
   *[other] { $rows } rijen
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } kolom
   *[other] { $cols } kolommen
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } rij
   *[other] { $rows } rijen
} en { $cols ->
    [one] { $cols } kolom
   *[other] { $cols } kolommen
}
grid-menu-freeze = Vastzetten
grid-menu-unfreeze = Losmaken
grid-menu-hide-column = Kolom verbergen
grid-menu-show-hidden-columns = Verborgen kolommen tonen ({ $count })
grid-menu-show-all = Alles tonen
grid-menu-format-as = Opmaken als
grid-format-none = Geen
grid-format-timestamp = Tijdstempel
grid-format-grouped = Cijfergroepering
grid-format-bytes = Bestandsgrootte
grid-format-bool = Booleaans
grid-menu-copy-name = Naam kopiëren
grid-menu-copy-qualified-name = Gekwalificeerde naam kopiëren
grid-menu-copy-values = Waarden kopiëren
grid-menu-sql-in-list = SQL IN-lijst

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
    [one] { $read } rij
   *[other] { $read } rijen
}
grid-rows-read-capped = { $n ->
    [one] { $read } rij
   *[other] { $read } rijen
} (afgekapt)
grid-rows-read-of-total = { $n ->
    [one] { $read } van { $total } rij
   *[other] { $read } van { $total } rijen
}
grid-kept-note = · bewaard — alleen-lezen
grid-arena-note = · { $columns } te groot om volledig vast te houden — latere rijen blijven leeg
grid-sorted-subset = · gesorteerde deelverzameling (afgekapt) — niet de volledige volgorde
grid-read-all = alle rijen lezen
grid-read-more = { $rows } rijen lezen
grid-reading = lezen…
grid-committing = Doorvoeren…
grid-commit-tip = { $count ->
    [one] { $count } wijziging doorvoeren ({ $keys })
   *[other] { $count } wijzigingen doorvoeren ({ $keys })
}
grid-discard-tip = Alle openstaande wijzigingen verwerpen
grid-copy-sql-tip = De wijzigingen als SQL kopiëren
grid-add-row-tip = Een rij toevoegen
grid-delete-row-tip = De geselecteerde rij markeren voor verwijdering ({ $key })
grid-clone-row-tip = De geselecteerde rij klonen
grid-copy-results-tip = De resultaten kopiëren…
grid-copy-results-empty = Niets om te kopiëren — geen rijen
grid-save-results-tip = De resultaten in een bestand opslaan…
grid-save-results-empty = Niets om op te slaan — geen rijen
grid-ai-tip = Rijen genereren met AI…
grid-ai-generating = Genereren…
grid-ai-off = AI-acties versturen gegevens — uit voor deze verbinding
grid-ai-fill-value = Waarde invullen met AI
grid-ai-insert-row = Rij invoegen met AI
grid-ai-seed-table = Tabel vullen met AI
grid-chart-tip = De resultaten in een grafiek zetten
grid-chart-empty = Niets voor een grafiek — geen rijen
grid-chart-this-result = dit resultaat

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Bekijken
grid-rollback-tab = { $tab } terugdraaien
grid-find-placeholder = Zoeken in resultaten
grid-figure-copied = Gekopieerd
grid-figure-rows = { $n ->
    [one] { $n }{ $more } rij
   *[other] { $n }{ $more } rijen
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } uniek
grid-figure-sum = Som { $value }
grid-figure-avg = Gem. { $value }
grid-figure-median = Mediaan { $value }
grid-figure-min = Min. { $value }
grid-figure-max = Max. { $value }
grid-figure-length = { $n ->
    [one] Lengte { $n } teken
   *[other] Lengte { $n } tekens
}
grid-figure-length-range = Lengte { $lo }–{ $hi } tekens
grid-goto-label = Ga naar rij:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = het klembord
export-dest-file = het bestand
export-note-rows = { $n ->
    [one] { $rows } rij geëxporteerd naar { $name }
   *[other] { $rows } rijen geëxporteerd naar { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } bevat bytes die dit formaat niet kan dragen: die cellen zijn leeg
   *[other] { $cols } bevatten bytes die dit formaat niet kan dragen: die cellen zijn leeg
}
export-note-blanked = { $cols } te groot om volledig vast te houden: latere rijen zijn leeg
export-note-cut = { $cols } ingekort tot de limiet van { $limit } tekens per cel van Excel
export-failure-partial = { $message } — { $name } is niet gewijzigd; de rijen die zijn geschreven, staan in { $part }
export-cancelled-partial = Exporteren geannuleerd — { $name } is niet gewijzigd; de rijen die zijn geschreven, staan in { $part }
export-cancelled-nothing = Exporteren geannuleerd — { $name } is niet gewijzigd.
copy-sql-as-null = { $n ->
    [one] Gekopieerd naar het klembord — { $cols } bevat bytes die SQL-tekst niet kan dragen: geschreven als NULL
   *[other] Gekopieerd naar het klembord — { $cols } bevatten bytes die SQL-tekst niet kan dragen: geschreven als NULL
}
copy-sql-nothing = { $n ->
    [one] Niets gekopieerd — { $cols } bevat bytes die SQL-tekst niet kan dragen
   *[other] Niets gekopieerd — { $cols } bevatten bytes die SQL-tekst niet kan dragen
}
copy-sql-left-out = { $n ->
    [one] Gekopieerd naar het klembord — { $cols } bevat bytes die SQL-tekst niet kan dragen: die waarden zijn weggelaten
   *[other] Gekopieerd naar het klembord — { $cols } bevatten bytes die SQL-tekst niet kan dragen: die waarden zijn weggelaten
}

grid-fill-nothing = Niets om te vullen: de geselecteerde kolommen kunnen niet worden geschreven, of de cel om te kopiëren is een niet-ingestelde waarde van een nieuwe rij of een klaargezette DEFAULT.
grid-paste-outside = { $n } buiten het raster
grid-paste-read-only = { $n } in alleen-lezen kolommen
grid-paste-capped = { $n } boven de plaklimiet van { $cap } cellen
grid-paste-deleted = { $n } in rijen gemarkeerd voor verwijdering
grid-paste-nothing = Niets geplakt: { $notes }.
grid-paste-partial = { $count ->
        [one] { $cells } cel geplakt, overgeslagen: { $notes }.
       *[other] { $cells } cellen geplakt, overgeslagen: { $notes }.
    }
grid-row-not-editable = Kolom `{ $column }` is niet bewerkbaar.
grid-row-not-nullable = Kolom `{ $column }` mag niet NULL zijn.
grid-fill-skipped = { $count ->
    [one] { $filled } cel gevuld, { $skipped } overgeslagen in rijen gemarkeerd voor verwijdering.
   *[other] { $filled } cellen gevuld, { $skipped } overgeslagen in rijen gemarkeerd voor verwijdering.
}
grid-attach-summary = { $rows ->
    [one] { $rows } rij
   *[other] { $rows } rijen
} × { $cols ->
    [one] { $cols } kolom
   *[other] { $cols } kolommen
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } rij
   *[other] { $rows } rijen
} × { $cols ->
    [one] { $cols } kolom
   *[other] { $cols } kolommen
} uit { $table }
grid-attach-summary-capped = { $summary } (de eerste { $taken } van { $total } geselecteerd)
grid-copy-sql-unset = Niets om als SQL te kopiëren — de geselecteerde cellen van de nieuwe rij hebben nog geen waarde en krijgen de standaardwaarde van de server.
grid-copy-sql-joined = Niets om als SQL te kopiëren: deze rijen komen uit een query over meer dan één tabel.
grid-copy-sql-no-keyed-table = Niets om als SQL te kopiëren: de rijen moeten uit één tabel met een sleutel komen.
grid-copy-sql-key-too-large = Niets om als SQL te kopiëren: de sleutel was te groot om volledig in dit resultaat te passen, dus sommige rijen kunnen er niet mee worden aangeduid.
grid-copy-sql-pinned = Niets om als SQL te kopiëren: dit resultaat is vastgemaakt, dus de rijen zijn een momentopname.
grid-copy-sql-read-only = Niets om als SQL te kopiëren: deze verbinding is alleen-lezen.
grid-copy-update-too-large = Niets om als UPDATE te kopiëren: de geselecteerde kolommen waren te groot om volledig in dit resultaat te passen, en sommige van hun cellen zijn leeg.
grid-copy-update-nothing-writable = Niets om als UPDATE te kopiëren: geen van de geselecteerde kolommen kan worden geschreven.
grid-copy-where-in-no-key = Niets om als WHERE IN te kopiëren: deze tabel heeft geen primaire of unieke sleutel om de rijen mee aan te duiden.
grid-copy-new-row-no-key = Niets om te kopiëren: een nieuwe rij heeft pas een sleutel als ze is opgeslagen.
grid-export-fetched-rows = Opgehaalde rijen ({ $count })
grid-export-dialog-title = Resultaten exporteren
grid-export-already-running = Er loopt al een export — er is niets geschreven.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Rij { $row }
grid-row-title-in-table = Rij { $row }  ·  { $table }
grid-row-set-value = Waarde instellen
grid-row-set-null = NULL instellen
grid-row-edit-binary = Bewerken
grid-row-view-binary = Bekijken
grid-row-invalid-json = Ongeldige JSON: { $error }
grid-row-saving = Opslaan

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } rij beïnvloed · { $ms } ms
   *[other] { $count } rijen beïnvloed · { $ms } ms
}
grid-rerun-would-write = Kan deze instructie niet opnieuw uitvoeren — ze zou naar de database schrijven
grid-filter-not-simple = Kan deze query niet filteren — geen eenvoudige SELECT op één tabel
grid-filter-bracket-in-name = een naam met `]` kan hier niet worden gefilterd
grid-filter-trailing-input = onverwachte invoer na de voorwaarde
grid-staged-requery = Voer je wijzigingen eerst door of verwerp ze — de rijen opnieuw lezen zou ze weggooien
grid-staged-landing = Er zijn wijzigingen klaargezet terwijl de tabel opnieuw werd gelezen, dus ze zijn behouden en de tabel is niet vernieuwd — voer ze door of verwerp ze en vernieuw daarna.
grid-reread-without-filter = Het filter roept een functie aan die Schemaic niet ongevraagd opnieuw uitvoert, dus na het doorvoeren is de tabel opnieuw gelezen zonder filter en sortering.
grid-hidden-columns-tip = { $count ->
    [one] { $count } verborgen kolom — klik om alles te tonen, of klik met rechts op een kop om te kiezen
   *[other] { $count } verborgen kolommen — klik om alles te tonen, of klik met rechts op een kop om te kiezen
}
grid-copied-staged-sql = De klaargezette wijzigingen als SQL gekopieerd.
grid-copy-split = { $n ->
        [one] { $n } gekopieerde cel bevat een tab of een regeleinde — bij plakken wordt die als meer dan één cel gelezen.
       *[other] { $n } gekopieerde cellen bevatten een tab of een regeleinde — bij plakken wordt elk ervan als meer dan één cel gelezen.
    }
grid-duplicate-title = Rijen dupliceren
grid-duplicate-question = { $n ->
    [one] { $rows } rij dupliceren? Ze wordt klaargezet als openstaande rij en er wordt niets geschreven tot je doorvoert.
   *[other] { $rows } rijen dupliceren? Ze worden klaargezet als openstaande rijen en er wordt niets geschreven tot je doorvoert.
}
grid-nullable-tip = Nullable
grid-cell-auto = <auto>
grid-cell-required = <verplicht>
grid-seed-title = Rijen genereren
grid-seed-generate = Genereren

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Schema
footer-panel-ai = AI-assistent
footer-panel-history = Querygeschiedenis
footer-panel-snippets = Fragmentenbibliotheek
footer-panel-activity = Serveractiviteit
footer-panel-terminal = Terminal
footer-panel-narrow = Het venster is te smal voor dit paneel — maak het breder om het paneel terug te halen.
footer-cursor = Rg { $line }, Kol { $col }
footer-indent-spaces = Spaties: { $width }
footer-indent-tabs = Tabs: { $width }
footer-spaces = Spaties
footer-tabs = Tabs
footer-wrap = Terugloop
footer-no-wrap = Geen terugloop
footer-read-only = Alleen-lezen
footer-write-mode = Schrijfmodus
footer-tx-auto = Auto-commit
footer-tx-manual = Handmatig
footer-tx-open = { $count } open
footer-tx-aborted = Tx afgebroken — draai terug om verder te gaan
footer-tx-lost = Transactie verloren
footer-commit = Doorvoeren
footer-rollback = Terugdraaien
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Query { $n }
lib-result-default-label = Resultaat { $n }
lib-ai-off-note = De AI-assistent staat uit. Kies een agent-CLI in de AI-instellingen om hem aan te zetten.
lib-test-testing = Testen…
lib-test-connected = Verbonden
lib-test-failed = Verbinding mislukt

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Minimaal
lib-effort-low = Laag
lib-effort-medium = Gemiddeld
lib-effort-high = Hoog
lib-effort-extra = Extra
lib-effort-max = Maximaal
lib-schema-scope-active = Alleen actieve database
lib-schema-scope-all = Alle databases
lib-schema-scope-none = Geen
lib-cursor-block = Blok
lib-cursor-bar = Streep
lib-cursor-underline = Onderstreping

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Geen verbinding
header-new-connection = Nieuwe verbinding
header-find-anywhere-tip = Overal zoeken ({ $keys })
header-shortcuts-tip = Sneltoetsen
header-settings-tip = Instellingen
header-disconnected = Verbinding verbroken
header-retry = Opnieuw proberen
colour-red = Rood
colour-orange = Oranje
colour-amber = Amber
colour-green = Groen
colour-teal = Blauwgroen
colour-blue = Blauw
colour-purple = Paars
colour-pink = Roze

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = ‘{ $name }’ is op schijf gewijzigd.
disk-reload = Opnieuw laden
disk-keep-mine = Mijn versie behouden
results-run-hint = Voer een query uit  ({ $keys })
results-failed = Instructie mislukt.
results-cancelled = Query geannuleerd.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = { $count } vastgemaakt · { $size }
results-pinned-tip = Vastgemaakte resultaten blijven in het geheugen tot je ze sluit. Sluit de resultaten die je niet meer nodig hebt om het geheugen terug te geven.
results-chip-nothing-run = In dit tabblad is nog niets uitgevoerd.
results-chip-kept = bewaard — alleen-lezen
results-menu-pin = Vastmaken
results-menu-unpin = Losmaken
results-menu-close = Sluiten
results-menu-compare-with = Vergelijken met { $result }
results-menu-close-others = Andere resultaten sluiten
results-menu-close-all = Alle resultaten sluiten

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Overal zoeken

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Niets om uit te kiezen

widgets-spinner-verbs = Brouwen, Peinzen, Mijmeren, Kuieren, Prutsen, Knutselen, Puzzelen, Broeden, Sudderen, Pruttelen, Kneden, Rijzen, Marineren, Gisten, Fermenteren, Kokkerellen, Roosteren, Karamelliseren, Flamberen, Garneren, Kruiden, Roeren, Opkloppen, Destilleren, Overpeinzen, Filosoferen, Dagdromen, Fantaseren, Uitvogelen, Ontcijferen, Ontrafelen, Berekenen, Rekenen, Ordenen, Componeren, Orkestreren, Dirigeren, Improviseren, Jongleren, Goochelen, Toveren, Betoveren, Zweven, Levitatie oefenen, Dwarrelen, Fladderen, Wervelen, Kolken, Golven, Kabbelen, Druppelen, Motregenen, Borrelen, Bruisen, Sprankelen, Glinsteren, Schitteren, Zoemen, Snorren, Neuriën, Swingen, Moonwalken, Huppelen, Slenteren, Dwalen, Zwerven, Dartelen, Ravotten, Stoeien, Scharrelen, Snuffelen, Speuren, Wroeten, Schuifelen, Waggelen, Kronkelen, Zigzaggen, Tollen, Spinnen, Weven, Breien, Smeden, Boetseren, Schaven, Polijsten, Kiemen, Bloeien, Bestuiven, Nestelen, Uitbroeden, Lanterfanten, Treuzelen, Teuten, Lummelen, Klungelen, Rommelen, Frunniken, Friemelen, Fröbelen, Kliederen, Kwebbelen, Prakkiseren, Piekeren, Herkauwen, Overwegen, Bespiegelen, Clauden, Gitificeren, Hyperspacen, Kwantumdenken, Doorgronden

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Vraag de AI-assistent om hulp.
cmdk-accept = Accepteren
cmdk-reject = Afwijzen
cmdk-no-changes = Geen wijzigingen voorgesteld
cmdk-diff-summary = { $hunks ->
        [one] { $n } blok · { $removed } − / { $added } +
       *[other] { $n } blokken · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = waarde
params-kind-tip = Tekst, getal, NULL of ruwe SQL — klik om te wijzigen
params-missing = Geen waarde voor { $names }
params-not-a-number = :{ $name } is een getal, maar bevat ‘{ $text }’

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Bekijken
editor-run-anyway = Toch uitvoeren
guard-read-only = Alleen-lezen verbinding.
guard-read-only-refused = Alleen-lezen verbinding: { $reason }
read-only-several = er is maar één instructie toegestaan
read-only-not-a-read = alleen alleen-lezen query's ({ $heads }) zijn toegestaan
read-only-denied = `{ $word }` is niet toegestaan in een alleen-lezen query
read-only-advances-sequence = `NEXT VALUE FOR` verhoogt een reeks, en dat doet een alleen-lezen query niet
read-only-unlisted-call = `{ $name }` is geen functie waarvan bekend is dat ze alleen leest, dus ze is niet toegestaan in een alleen-lezen query; functies uit extensies of gedefinieerd in de database worden ook geweigerd
read-only-locks = `{ $clause }` vergrendelt de rijen die het leest, en dat doet een alleen-lezen query niet; laat de vergrendelingsclausule weg
guard-no-database = Geen database geselecteerd.
guard-writes-one = Deze instructie wijzigt gegevens.
guard-writes-several = Deze instructies wijzigen gegevens.
guard-script = Elke instructie in { $file } uitvoeren? Een script kan objecten maken, wijzigen en verwijderen, en wat het wijzigt, kan hier niet ongedaan worden gemaakt.
guard-staged = { $n ->
    [one] Deze uitvoering vervangt het resultaat met { $n } klaargezette wijziging — uitvoeren zou die verwerpen. Voer haar eerst door of verwerp haar, of voer toch uit.
   *[other] Deze uitvoering vervangt het resultaat met { $n } klaargezette wijzigingen — uitvoeren zou die verwerpen. Voer ze eerst door of verwerp ze, of voer toch uit.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL verwijdert elke rij in de tabel.
guard-truncate = TRUNCATE verwijdert elke rij in de tabel.
guard-no-where = { $kind }-instructie zonder WHERE-clausule gevonden.
guard-drop-table = DROP TABLE verwijdert de tabel en elke rij erin.
guard-drop-database = DROP DATABASE verwijdert de database en elke tabel erin.
guard-drop-schema = DROP SCHEMA verwijdert het schema en elke tabel erin.
guard-drop-owned = DROP OWNED verwijdert alles wat de rol bezit, met elke tabel en haar rijen.
guard-drop-cascade = DROP { $obj } … CASCADE verwijdert elke kolom die ervan afhangt, met haar waarden.
guard-replace-table = CREATE OR REPLACE TABLE verwijdert de bestaande tabel en elke rij erin.
guard-replace-database = CREATE OR REPLACE DATABASE verwijdert de bestaande database en elke tabel erin.
guard-replace-schema = CREATE OR REPLACE SCHEMA verwijdert het bestaande schema en elke tabel erin.
guard-truncate-partition = TRUNCATE PARTITION verwijdert elke rij in de partities die het noemt.
guard-drop-partition = DROP PARTITION verwijdert de partitie en elke rij erin.
quickinfo-table = tabel { $name } · { $database }
quickinfo-view = view { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } kolom
       *[other] { $n } kolommen
    }
quickinfo-primary-key = primaire sleutel
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto-increment
quickinfo-default = standaard { $value }
quickinfo-generated = gegenereerd als { $expression }
quickinfo-function = functie { $name }({ $arguments })
quickinfo-function-returns = functie { $name }({ $arguments }) retourneert { $returns }
quickinfo-procedure = procedure { $name }({ $arguments })
quickinfo-procedure-returns = procedure { $name }({ $arguments }) retourneert { $returns }
completion-foreign-key = refererende sleutel
completion-expand-star = * uitvouwen
completion-columns = { $count ->
        [one] { $n } kolom
       *[other] { $n } kolommen
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Syntaxisfout
diag-syntax-detail = Syntaxisfout: { $detail }
diag-reserved-alias-backticks = `{ $word }` is een gereserveerd trefwoord en kan niet als alias worden gebruikt (zet het tussen backticks)
diag-reserved-alias-double-quotes = `{ $word }` is een gereserveerd trefwoord en kan niet als alias worden gebruikt (zet het tussen dubbele aanhalingstekens)
diag-reserved-alias-brackets = `{ $word }` is een gereserveerd trefwoord en kan niet als alias worden gebruikt (zet het tussen vierkante haken)
diag-table-not-found = Tabel `{ $table }` niet gevonden{ $within ->
        [some] {" "}in `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— bedoelde je `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = Kolom `{ $column }` niet gevonden{ $within ->
        [some] {" "}in `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— bedoelde je `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` heeft hier de alias `{ $alias }` — kwalificeer met `{ $alias }`
diag-ambiguous-column = Kolom `{ $column }` is dubbelzinnig
diag-join-without-condition = JOIN heeft geen ON/USING-voorwaarde — dit is een cross join; gebruik CROSS JOIN als dat de bedoeling is
diag-comma-join-without-where = Kommajoin zonder WHERE — dit is een cross join; voeg een WHERE toe, of gebruik CROSS JOIN als dat de bedoeling is
diag-not-grouped = Kolom `{ $column }` moet in GROUP BY staan of in een aggregaat worden gebruikt
diag-insert-row-width = INSERT noemt { $columns } { $columns ->
        [one] kolom
       *[other] kolommen
    }, maar deze rij heeft { $values } { $values ->
        [one] waarde
       *[other] waarden
    }
diag-row-width-without-list = `{ $table }` verwacht zonder kolomlijst { $needed } { $needed ->
        [one] waarde
       *[other] waarden
    }, maar deze rij heeft er { $values }
diag-row-width-of-table = `{ $table }` heeft { $columns } { $columns ->
        [one] kolom
       *[other] kolommen
    }, maar deze rij heeft { $values } { $values ->
        [one] waarde
       *[other] waarden
    }
diag-nullable-not-in = `{ $column }` kan NULL zijn, en `NOT IN` komt met geen enkele rij overeen zodra de subquery er een retourneert — voeg er `WHERE { $column } IS NOT NULL` aan toe, of gebruik NOT EXISTS
diag-update-without-where = UPDATE zonder WHERE wijzigt elke rij in de tabel
diag-delete-without-where = DELETE zonder WHERE verwijdert elke rij in de tabel
diag-null-comparison = Een vergelijking met NULL is nooit waar — gebruik { $fix }
diag-misspelled-keyword = `{ $word }` lijkt op een verkeerd gespeld trefwoord
diag-misspelled-function = `{ $word }` lijkt op een verkeerd gespelde functie
diag-flow-merge-needs-semicolon = `MERGE` moet eindigen op `;`
diag-flow-if-without-statement = `IF` heeft geen instructie om uit te voeren
diag-flow-else-without-statement = `ELSE` heeft geen instructie om uit te voeren
diag-flow-while-without-statement = `WHILE` heeft geen instructie om uit te voeren
diag-flow-end-try-without-begin-try = `END TRY` zonder bijbehorende `BEGIN TRY`
diag-flow-end-catch-without-begin-catch = `END CATCH` zonder bijbehorende `BEGIN CATCH`
diag-flow-end-without-begin = `END` zonder bijbehorende `BEGIN`
diag-flow-throw-needs-semicolon = De instructie vóór `THROW` moet eindigen op `;`
diag-flow-end-try-without-catch = `END TRY` moet worden gevolgd door `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` heeft geen instructie vóór de `ELSE`
diag-flow-else-without-if = `ELSE` zonder `IF` ervoor
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` moet volgen op een `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` heeft geen bijbehorende `END TRY`
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` heeft geen bijbehorende `END CATCH`
diag-flow-begin-without-end = `BEGIN` heeft geen bijbehorende `END`

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = AI vragen
editor-menu-explain = Uitleggen
editor-menu-optimise = Optimaliseren
editor-menu-plan = Plan
editor-menu-copy-single-line = Kopiëren als één regel
editor-menu-make-single-line = Op één regel zetten
editor-menu-format-statement = Instructie opmaken
editor-menu-find-usages = Vindplaatsen zoeken
editor-menu-create-view = View maken
cmdk-optimise-label = Deze query optimaliseren
editor-several-carets = Meerdere cursors en niets geselecteerd: selecteer wat moet worden uitgevoerd, of laat één cursor over

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Huidige uitvoeren
editor-run-everything = Alles uitvoeren

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Zoeken
find-invalid = Ongeldig
find-bad-pattern = Geen geldig patroon
find-bad-pattern-why = Geen geldig patroon: { $why }
find-match-case = Hoofdlettergevoelig
find-whole-word = Heel woord
find-regex = Reguliere expressie
find-scope = Sel
find-in-selection-tip = Zoeken in selectie
find-replace-placeholder = Vervangen
find-replace = Vervangen
find-replace-all = Alle
goto-line-label = Ga naar:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Geen { $client }-client gevonden (PATH of WSL).
cli-no-client-on-path = Geen { $client }-client gevonden op PATH.
cli-no-tool = Geen { $tool } gevonden op PATH.
cli-no-either-tool = Geen { $first } of { $second } gevonden op PATH.
cli-no-either-client = Geen { $first }- of { $second }-client gevonden op PATH.
cli-sqlcmd-windows-in-wsl = Aanmelden met Windows vereist een Windows-sqlcmd, en de gevonden sqlcmd staat in WSL. Installeer sqlcmd op Windows, of open de verbinding in een querytabblad.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = De client die op je PATH is gevonden, is een .cmd/.bat-shim, die Windows via cmd.exe uitvoert — waar een databasenaam met '&' of '|' een tweede opdracht zou worden. Laat Instellingen naar het echte uitvoerbare bestand wijzen, of open in plaats daarvan een querytabblad.
cli-refused-name-equals = De naam van deze database bevat '=', wat psql als een verbindingsreeks zou lezen en zou volgen naar een andere server. Open hem in plaats daarvan in een querytabblad.
cli-refused-name-uri = De naam van deze database begint als een verbindings-URI, die psql zou volgen naar een andere server. Open hem in plaats daarvan in een querytabblad.
cli-refused-file-dash = De naam van dit databasebestand begint met '-', wat de sqlite3-client als een optie zou lezen in plaats van als het bestand. Hernoem het, of geef het een pad (./name.db), en open het opnieuw.
cli-refused-sqlcmd-entra = sqlcmd kan het aanmeldtoken van de Azure CLI niet krijgen, dus deze verbinding kan er niet in worden geopend. Open haar in plaats daarvan in een querytabblad.
cli-refused-sqlcmd-host = De host van deze verbinding bevat een teken dat sqlcmd als deel van het adres leest (',', ';', een tweede '\' of een ':'-voorvoegsel). Stel de poort in het eigen veld in en open haar opnieuw.
cli-refused-sqlcmd-ca = sqlcmd kan niet verifiëren tegen een CA-bestand. Voeg de CA toe aan het vertrouwensarchief van het systeem en maak het CA-veld van de verbinding leeg, of open de verbinding in een querytabblad.
cli-refused-sqlcmd-client-cert = SQL Server meldt niet aan met een clientcertificaat, dus sqlcmd kan dat van deze verbinding nergens heen sturen. Maak het clientcertificaat en de sleutel leeg en open haar opnieuw.
cli-refused-wsl-paths = De TLS-certificaatpaden van deze verbinding zijn Windows-paden, die de WSL-client niet kan openen. Installeer een systeemeigen client, of verplaats de certificaten naar WSL.
cli-refused-tunnelled-verify = Deze verbinding verifieert het certificaat van de server en bereikt die via een SSH-tunnel, dus de client zou worden verteld 127.0.0.1 te bellen en het certificaat daartegen te controleren — waaraan het eigen certificaat van de server niet kan voldoen. Open in plaats daarvan een querytabblad, of verlaag de TLS-modus voor de CLI-sessie.
cli-refused-not-container-id = docker ps gaf iets weer dat geen container-id is.
cli-refused-file-line-break = Die bestandsnaam bevat een regeleinde, wat de client als twee argumenten zou lezen. Kies een andere naam.
cli-refused-role = De rol in de init-SQL van deze verbinding kan niet ongewijzigd aan de tool worden doorgegeven.
cli-refused-mysql-source-path = De mysql-client zou het pad van dat bestand als eigen opdrachten lezen (er staat een ';' of een '\' in, of een spatie aan een uiteinde). Verplaats of hernoem het en herstel het daarna.
cli-refused-cloud-token = Deze verbinding meldt aan via het token van een cloud-CLI, en dat krijgt een externe client niet. Open haar in plaats daarvan in een querytabblad.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Kan niet bepalen van waaruit Schemaic draait: { $error }
install-registry-open = Kon je gebruikersomgeving in het register niet openen: { $error }
install-path-read = Kon je gebruikers-PATH niet lezen: { $error }
install-path-write = Kon je gebruikers-PATH niet schrijven: { $error }
install-look-at = Kon { $path } niet bekijken: { $error }
install-read-link = Kon de koppeling op { $path } niet lezen: { $error }
install-create = Kon { $path } niet maken: { $error }
install-remove-old = Kon de oude { $path } niet verwijderen: { $error }
install-remove = Kon { $path } niet verwijderen: { $error }
install-path-is-windows = Een vermelding in de gebruikers-PATH is een installatie voor Windows; dit platform gebruikt een koppeling.
install-link-is-unix = De opdracht koppelen is een installatie voor macOS en Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Kan niet bepalen in welke map { $exe } staat.
install-refuse-no-shim = Er staat geen schemaic.com naast deze kopie van Schemaic, in { $dir }, dus er is geen consoleopdracht om op PATH te zetten. De geïnstalleerde app heeft er een; een ontwikkelversie niet.
install-refuse-semicolon = Deze kopie van Schemaic staat in { $dir }, waarvan de naam een ';' bevat — PATH zou dat als twee verschillende mappen lezen. Verplaats haar naar een plek zonder en probeer het opnieuw.
install-refuse-exe-replaced = { $exe } is vervangen terwijl Schemaic draaide, dus een koppeling ernaar zou nergens heen wijzen. Start Schemaic opnieuw en kies daarna opnieuw Installeren.
install-refuse-no-home = Er is geen thuismap om de koppeling in te zetten, dus er is nergens om te kijken.
install-refuse-path-not-text = Je gebruikers-PATH is niet als tekst in het register opgeslagen, dus die is ongemoeid gelaten.
install-refuse-path-not-unicode = Je gebruikers-PATH bevat tekens die niet veilig kunnen worden teruggelezen, dus die is ongemoeid gelaten.
install-refuse-portable = Dit is een draagbare kopie, dus Verwijderen kan niet bepalen of Installeren { $dir } op je PATH heeft gezet of dat de map er al stond met andere programma's erin. Haal haar weg onder Omgevingsvariabelen als Installeren haar heeft toegevoegd.
install-refuse-spot-taken = { $link } bestaat al en is geen koppeling, dus Installeren heeft het ongemoeid gelaten. Zet het opzij en probeer het opnieuw.
install-refuse-points-elsewhere = { $link } wijst al naar { $to }. Verwijder het als dat een oude kopie van Schemaic is en probeer het opnieuw.
install-refuse-unlink-elsewhere = { $link } wijst naar { $to }, niet naar deze kopie van Schemaic, dus Verwijderen heeft het ongemoeid gelaten.
install-refuse-unlink-not-ours = { $link } is geen koppeling die Schemaic heeft gemaakt, dus Verwijderen heeft het ongemoeid gelaten.
install-report-already = De opdracht schemaic staat al op je PATH, in { $dir }.
install-report-added = { $dir } is toegevoegd aan je gebruikers-PATH. Open een nieuwe terminal om schemaic te gebruiken — een terminal die al open is, houdt de oude PATH.
install-report-linked = { $link } gekoppeld aan { $target }.
install-report-linked-if-not-found = { $link } gekoppeld aan { $target }. Als een nieuwe terminal schemaic niet kan vinden, voeg { $dir } dan toe aan je PATH in je shellprofiel.
install-report-linked-not-on-path = { $link } gekoppeld aan { $target }. { $dir } staat nog niet op je PATH — voeg het toe in je shellprofiel en open daarna een nieuwe terminal.
install-report-removed = { $dir } is verwijderd uit je gebruikers-PATH. Een terminal die al open is, houdt de oude PATH.
install-report-path-absent = { $dir } staat niet op je gebruikers-PATH, dus er was niets te verwijderen.
install-report-unlinked = { $link } verwijderd.
install-report-link-absent = Er is geen { $link }, dus er was niets te verwijderen.
install-report-package = De opdracht schemaic in { $dir } kwam mee met het geïnstalleerde pakket, niet via Installeren — verwijder het pakket om haar te verwijderen.
install-hint-adds-folder = Schemaic vanuit een terminal op naam starten. Voegt de map toe aan je gebruikers-PATH.
install-hint-links = Schemaic vanuit een terminal op naam starten. Koppelt het in ~/.local/bin.
install-hint-folder-on-path = Schemaic vanuit een terminal op naam starten. De map, { $dir }, staat al op je gebruikers-PATH.
install-hint-comes-with-app = Schemaic vanuit een terminal op naam starten. De opdracht schemaic komt mee met de app, op { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = SSH-tunnel is nog niet opgezet
tunnel-could-not-open = De SSH-tunnel kon niet worden geopend: { $error }. Vernieuw het schema om het opnieuw te proberen.
tunnel-server-failed = Kon geen verbinding maken via de SSH-tunnel: { $error }. Vernieuw het schema om het opnieuw te proberen.

gate-plan-not-connected = Niet verbonden — het plan is niet uitgevoerd. De server antwoordde niet op de statuscontrole, dus er is niets naartoe gestuurd.
gate-plan-connection-changed = Je hebt van verbinding gewisseld terwijl Schemaic controleerde of ze bereikbaar was, dus het plan is niet uitgevoerd. Wissel terug en vraag het opnieuw.
gate-plan-tab-moved-on = Je hebt van tabblad gewisseld terwijl de verbinding opnieuw werd gecontroleerd, dus het plan is niet uitgevoerd. Het beschrijft het tabblad waaruit het werd gevraagd — ga terug naar dat tabblad en vraag het opnieuw.
gate-connection-changed = Niet uitgevoerd op { $name } — je hebt van verbinding gewisseld terwijl Schemaic controleerde of ze bereikbaar was. Wissel terug en probeer het opnieuw.
gate-not-connected = Niet verbonden met { $name }. De server antwoordde niet — controleer of hij draait en of de instellingen van deze verbinding kloppen.
gate-this-connection = deze verbinding
gate-run-tab-moved-on = Je hebt van tabblad gewisseld terwijl de verbinding opnieuw werd gecontroleerd, dus de instructie is niet uitgevoerd. Ze is gecontroleerd tegen het tabblad waarin ze werd getypt, en haar hier uitvoeren zou haar ergens anders uitvoeren. Ga terug naar dat tabblad en voer haar opnieuw uit.
gate-commit-not-connected = Niet verbonden — er is niet geprobeerd door te voeren. Klaargezette wijzigingen blijven behouden.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = Kon { $program } niet starten: { $error }. Kies een andere shell in Instellingen, Terminal.
term-cli-read-only = Deze verbinding is alleen-lezen in Schemaic. De opdrachtregelclient ervan zou dat niet zijn, dus die wordt hier niet geopend.
backup-read-only-server = Deze verbinding is alleen-lezen in Schemaic, en de back-up van deze engine wordt door de server geschreven, dus er is geen back-up gemaakt.
backup-dialog-title = Back-up maken van database
backup-kind-pg-dump = PostgreSQL-dump in aangepast formaat
backup-kind-sql = SQL-script
backup-kind-sqlite = SQLite-database
backup-not-started = De back-up is niet gestart: { $error }
backup-done = Back-up gemaakt naar { $path }.
backup-failed = De back-up is mislukt: { $error }
restore-title = Herstellen uit bestand
restore-confirm = { $path } uitvoeren in { $database } op { $connection }? Wat het maakt of vervangt, kan niet ongedaan worden gemaakt. Het wordt uitgevoerd in de terminal, waar Ctrl+C het stopt.
restore-read-only = Deze verbinding is alleen-lezen in Schemaic, dus er wordt niets in hersteld.
restore-engine-not-here = Back-ups van deze engine worden niet vanaf hier hersteld.
restore-unreadable = { $path } kon voor het herstellen niet worden gelezen, dus er is niets uitgevoerd: { $error }
restore-refused = { $path } stapt tijdens het uitvoeren over naar een andere database — { $switches } — dus als het in { $database } wordt hersteld, zou het daar schrijven. Er is niets uitgevoerd. Herstel het in de database die het noemt, of verwijder die regels uit een kopie van het bestand.
restore-refused-more = { $path } stapt tijdens het uitvoeren over naar een andere database — { $switches } en nog { $more } — dus als het in { $database } wordt hersteld, zou het daar schrijven. Er is niets uitgevoerd. Herstel het in de database die het noemt, of verwijder die regels uit een kopie van het bestand.
restore-switch = { $how } `{ $name }` op regel { $line }
restore-switch-unnamed = een { $how } op regel { $line } die geen database noemt die Schemaic kan lezen

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = ‘{ $name }’ sluiten
tab-close-unsaved = ‘{ $name }’ heeft niet-opgeslagen wijzigingen. Sluiten verwerpt ze; het bestand op schijf blijft zoals het is. Toch sluiten?
tab-close-all-title = Alle tabbladen sluiten
tab-close-all-question = Weet je zeker dat je alle tabbladen wilt sluiten?
tab-close-others-title = Andere tabbladen sluiten
tab-close-others-question = Weet je zeker dat je alle andere tabbladen wilt sluiten?
file-open-title = SQL-bestand openen
file-save-title = SQL-bestand opslaan
file-sql-filter = SQL-script
file-read-failed = Kon { $path } niet lezen: { $error }
file-save-failed = Kon { $path } niet opslaan: { $error }
file-too-large = { $path } is { $size } — te groot om in een editortabblad te openen. Schemaic zou het grootste deel van de tijd bezig zijn het opnieuw te analyseren. Voer het uit vanuit een querytabblad, of gebruik Importeren voor een gegevensbestand.
file-too-large-short = Het bestand is te groot om te openen.
file-large-title = Een groot bestand openen?
file-large-question = ‘{ $name }’ is { $size }. Schemaic analyseert het hele document opnieuw kort na elke pauze in het typen, dus een bestand van deze grootte maakt de editor traag zolang het tabblad open is. Toch openen?
file-changed-on-disk = { $path } is op schijf gewijzigd sinds het werd geopend. Nu opslaan zou die wijzigingen verwerpen — laad het bestand opnieuw (of gebruik Opslaan als met een andere naam).
file-lossy-title = Onleesbare bytes vervangen?
file-lossy-question = Schemaic kon niet elke byte van ‘{ $name }’ als tekst lezen en toonde die bytes als ‘�’. Opslaan schrijft wat je ziet, dus elk ervan wordt permanent vervangen — in het hele bestand, niet alleen in de regels die je hebt bewerkt. Toch opslaan?
file-replace-title = Bestand vervangen?
file-replace-question = ‘{ $name }’ bestaat al — ‘{ $typed }’ is opgeslagen met de extensie .sql toegevoegd. Vervangen?
file-reload-title = Opnieuw laden van schijf
file-reload-unsaved = ‘{ $name }’ heeft niet-opgeslagen wijzigingen. Opnieuw laden verwerpt ze. Toch opnieuw laden?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Er loopt al een export. Annuleer hem of wacht tot hij klaar is.
export-failed = Exporteren mislukt: { $error }
export-worker-died = Exporteren mislukt: worker gestopt: { $error }
export-not-published = Exporteren mislukt: { $path } kon niet worden gepubliceerd: { $error }
export-not-renamed = De export schreef { $part }, maar kon het niet hernoemen naar { $path }: { $error }
export-nothing-matched = Niets om te exporteren — geen tabel kwam overeen met de selectie.
export-stopped = Exporteren geannuleerd.
dump-failed = Dump mislukt: { $error }
blob-load-too-large = Dat bestand is { $size } — er kan maximaal { $most } worden geladen.
blob-load-failed = Laden mislukt: { $error }
blob-save-failed = Opslaan mislukt: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = Kon { $path } niet schrijven: { $error }
settings-export-title = Instellingen exporteren
settings-import-title = Instellingen importeren
settings-file-filter = Schemaic-instellingen
settings-import-nothing = Niets om te importeren: elke instelling in { $path } komt al overeen met de jouwe, en er worden geen fragmenten toegevoegd.
settings-import-question = { $path } wijzigt { $changed ->
        [one] { $changed } instelling
       *[other] { $changed } instellingen
    } en voegt { $snippets ->
        [one] { $snippets } fragment
       *[other] { $snippets } fragmenten
    } toe. De instellingen die het wijzigt, vervangen je huidige waarden; de fragmenten worden naast de jouwe toegevoegd.
settings-import-unreadable = kon het niet lezen: { $error }
connections-export-title = Verbindingen exporteren
connections-file-filter = Schemaic-verbindingen
connections-replace-title = ‘{ $file }’ vervangen
connections-replace-question = Er staat al een bestand met de naam ‘{ $file }’ in die map. De export voegt die extensie toe aan de naam die je typte, dus het opslagvenster heeft er niet naar gevraagd. Vervangen kan niet ongedaan worden gemaakt.
import-choose-file-title = Kies een verbindingenbestand
import-no-containers = Er draaien geen databasecontainers.
import-added = { $count ->
        [one] { $count } verbinding toegevoegd.
       *[other] { $count } verbindingen toegevoegd.
    }
import-added-foreign = { $count ->
        [one] { $count } verbinding toegevoegd.
       *[other] { $count } verbindingen toegevoegd.
    } Alleen-lezen en het omgevingslabel worden niet overgenomen uit een andere client — stel ze in via Verbindingen beheren voor alles wat ertoe doet.
conn-delete-title = ‘{ $name }’ verwijderen
conn-delete-question = Dit verwijdert de verbinding, het opgeslagen wachtwoord, het opgeslagen AI-gesprek, de querygeschiedenis en de tabbladen ervan. Dit kan niet ongedaan worden gemaakt.
conn-delete-keyring-kept = Schemaic kon de sleutelhanger van het besturingssysteem niet bereiken, dus het opgeslagen wachtwoord en de SSH-geheimen van de verwijderde verbinding staan **er nog in**.
conn-delete-keyring-later = Ze worden verwijderd de volgende keer dat een verbinding wordt verwijderd of opgeslagen terwijl de sleutelhanger bereikbaar is. Intussen kan geen nieuwe verbinding ze krijgen — verbindings-id's worden nooit hergebruikt.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = { $name } kon niet worden gelezen ({ $error }).
notice-unreadable-kept = Het onleesbare bestand is bewaard als { $name }.corrupt; Schemaic is teruggevallen op de back-up, of op de standaardwaarden als ook de back-up onleesbaar was.
notice-unreadable-swept = Die kopie kan wachtwoorden onversleuteld bevatten, dus ze wordt verwijderd de volgende keer dat je een verbinding verwijdert — haal er vóór die tijd uit wat je nodig hebt.
notice-missing = { $name } ontbrak.
notice-missing-restored = Schemaic heeft het hersteld uit { $name }{ $from }, dat er nog stond. Het wordt teruggeschreven de volgende keer dat deze instelling wijzigt — tot dan is dat zusterbestand de enige kopie.
notice-keyring-unread = { $n ->
    [one] Schemaic kon { $n } opgeslagen geheim niet lezen uit de sleutelhanger van het besturingssysteem.
   *[other] Schemaic kon { $n } opgeslagen geheimen niet lezen uit de sleutelhanger van het besturingssysteem.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic kon { $n } opgeslagen geheim niet lezen uit de sleutelhanger van het besturingssysteem ({ $error }).
   *[other] Schemaic kon { $n } opgeslagen geheimen niet lezen uit de sleutelhanger van het besturingssysteem ({ $error }).
}
notice-keyring-unread-kept = Die wachtwoorden zijn deze sessie niet beschikbaar en zijn **niet** verwijderd — laat de velden leeg en ze komen terug zodra de sleutelhanger bereikbaar is. Een nieuw wachtwoord typen overschrijft het opgeslagen geheim.
notice-in-the-clear = { $n ->
    [one] Schemaic kon de sleutelhanger van het besturingssysteem niet bereiken, dus { $n } wachtwoord is **als platte tekst** opgeslagen in `connections.json`.
   *[other] Schemaic kon de sleutelhanger van het besturingssysteem niet bereiken, dus { $n } wachtwoorden zijn **als platte tekst** opgeslagen in `connections.json`.
}
notice-in-the-clear-where = Dat bestand staat in de map die de knop *Map openen* onder Instellingen → Algemeen opent, naast het logbestand — controleer het voordat je het deelt. Opnieuw opslaan zodra de sleutelhanger bereikbaar is, zet ze er weer in.
notice-undeleted = { $n ->
    [one] { $n } gewist wachtwoord staat nog in de sleutelhanger van het besturingssysteem — Schemaic kon het niet verwijderen.
   *[other] { $n } gewiste wachtwoorden staan nog in de sleutelhanger van het besturingssysteem — Schemaic kon ze niet verwijderen.
}
notice-undeleted-after = Het veld is leeg op schijf, maar bij de volgende start wordt het weer ingevuld. Sla opnieuw op zodra de sleutelhanger bereikbaar is.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Geen SQL geretourneerd
inline-ai-not-sql = Het model gaf geen SQL terug
inline-ai-failed = genereren mislukt
ai-fill-no-value = De AI gaf geen waarde terug.
conn-gone = verbinding bestaat niet meer
tx-session-not-ready = de transactieverbinding is niet gereed — schakel over naar Auto-commit en terug
tx-session-open-failed = kon geen transactieverbinding openen: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = De instructie is geannuleerd.
tx-committed-before-ddl = De transactie waarin dit werd uitgevoerd, bestaat niet meer: MySQL voert door voordat het een DDL-instructie uitvoert, dus de instructies die er al in stonden, zijn permanent en Terugdraaien maakt ze niet ongedaan.
tx-rolled-back-by-server = De transactie waarin dit werd uitgevoerd, bestaat niet meer: SQL Server heeft haar teruggedraaid, dus elke instructie erin sinds het begin is ongedaan gemaakt.
tx-doomed = De transactie waarin dit werd uitgevoerd, kan niet meer worden doorgevoerd: SQL Server heeft haar als niet-doorvoerbaar gemarkeerd, dus Terugdraaien is de enige weg verder, en dat maakt elke instructie erin ongedaan.
tx-after-own-commit = De transactie waarin dit werd uitgevoerd, is beëindigd, en dit voerde een eigen COMMIT uit voordat het mislukte, dus wat vóór die COMMIT kwam, is waarschijnlijk doorgevoerd in plaats van ongedaan gemaakt. Controleer de gegevens voordat je iets ervan opnieuw uitvoert.
tx-lost-on-commit = De verbinding ging verloren met een open transactie. Ze ging verloren tijdens een COMMIT, dus of { $stmts ->
        [0] de transactie is
        [one] de { $stmts } instructie die er sinds het begin in werd uitgevoerd, is
       *[other] de { $stmts } instructies die er sinds het begin in werden uitgevoerd, zijn
    } doorgevoerd of teruggedraaid, is onbekend: de server kan hebben doorgevoerd voordat de verbinding wegviel. Controleer de gegevens voordat je iets ervan opnieuw uitvoert.
tx-lost-on-implicit-commit = De verbinding ging verloren met een open transactie. MySQL voert een open transactie door voordat het deze instructie uitvoert, dus { $stmts ->
        [0] er was nog niets in uitgevoerd, en er viel niets door te voeren
        [one] de { $stmts } instructie die er sinds het begin in werd uitgevoerd, is toen zeer waarschijnlijk doorgevoerd en is permanent
       *[other] de { $stmts } instructies die er sinds het begin in werden uitgevoerd, zijn toen zeer waarschijnlijk doorgevoerd en zijn permanent
    }. Controleer de gegevens voordat je iets ervan opnieuw uitvoert.
tx-lost-rolled-back = De verbinding ging verloren met een open transactie. De server draait een onvoltooide transactie terug wanneer haar verbinding eindigt, dus { $stmts ->
        [0] er was nog niets in uitgevoerd
        [one] de { $stmts } instructie die er sinds het begin in werd uitgevoerd, is ongedaan gemaakt
       *[other] de { $stmts } instructies die er sinds het begin in werden uitgevoerd, zijn ongedaan gemaakt
    }.
tx-lost-rolled-back-mysql = De verbinding ging verloren met een open transactie. De server draait een onvoltooide transactie terug wanneer haar verbinding eindigt, dus { $stmts ->
        [0] er was nog niets in uitgevoerd
        [one] de { $stmts } instructie die er sinds het begin in werd uitgevoerd, is ongedaan gemaakt — behalve een schrijfactie naar een tabel die geen transacties ondersteunt (MyISAM, MEMORY), die geen terugdraaiing ongedaan maakt
       *[other] de { $stmts } instructies die er sinds het begin in werden uitgevoerd, zijn ongedaan gemaakt — behalve een schrijfactie naar een tabel die geen transacties ondersteunt (MyISAM, MEMORY), die geen terugdraaiing ongedaan maakt
    }.
tx-not-sent-timeout = De time-out voor instructies ging af terwijl deze uitvoering nog wachtte op de verbinding van het tabblad. Een eerdere bewerking erop — een doorvoering, of het opnieuw ophalen daarna — was nog niet klaar, dus de instructie is nooit naar de server gestuurd. Er is niets uitgevoerd, en de transactie staat precies waar ze stond.
tx-not-sent-cancelled = Deze uitvoering is geannuleerd terwijl ze nog wachtte op de verbinding van het tabblad. Een eerdere bewerking erop — een doorvoering, of het opnieuw ophalen daarna — was nog niet klaar, dus de instructie is nooit naar de server gestuurd. Er is niets uitgevoerd, en de transactie staat precies waar ze stond.
snippet-default-name = Fragment
snippet-delete-title = Fragment verwijderen
snippet-delete-question = ‘{ $name }’ verwijderen? Dit kan niet ongedaan worden gemaakt.
copy-target-read-only = De doelverbinding is alleen-lezen, dus er zijn geen rijen gekopieerd.
copy-target-gone = De doelverbinding bestaat niet meer, dus er zijn geen rijen gekopieerd.
copy-not-started = Er zijn geen rijen gekopieerd: { $error }
copy-counter-not-moved = { $count ->
        [one] { $count } rij is gekopieerd, maar de teller van de tabel is er niet voorbij gezet, dus de volgende insert kan botsen: { $error }
       *[other] { $count } rijen zijn gekopieerd, maar de teller van de tabel is er niet voorbij gezet, dus de volgende insert kan botsen: { $error }
    }
copy-stopped = Het kopiëren is gestopt door een fout: { $error }
compare-reading-failed = { $database } lezen: { $error }
monitor-no-row-key = Geen rijsleutel voor deze tabel — wijzigingen kunnen niet worden gevolgd.
monitor-restarted = De kolommen van de tabel zijn gewijzigd, dus het volgen van wijzigingen is vanaf hier opnieuw gestart.
timeout-cancelled-app = Geannuleerd: de instructie liep langer dan de time-out voor instructies van { $timeout }. Wijzig hem of zet hem uit in Instellingen → Query.
timeout-cancelled-connection = Geannuleerd: de instructie liep langer dan de time-out voor instructies van { $timeout }. Wijzig hem of zet hem uit in de instellingen van deze verbinding.
ai-chat-no-database = Kan de database niet bereiken, dus de assistent kan voor deze verbinding niet worden gestart. Controleer de verbinding en probeer het opnieuw.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **De assistent kon niet starten.** Schemaic geeft elk agentproces een eigen privéwerkmap, en noch de tijdelijke map van het systeem noch de configuratiemap kon er een bevatten — dus er is geen plek om hem uit te voeren die niet van iemand anders is. Controleer of er naar de tijdelijke map kan worden geschreven en probeer het opnieuw.
ai-no-reply-file = Kon geen tijdelijk bestand voor het antwoord maken.
ai-inline-no-opencode-config = Kon de configuratie van OpenCode niet schrijven, dus het genereren is niet gestart (zonder die configuratie zou de CLI draaien met zijn eigen tools ingeschakeld).
ai-inline-no-copilot-home = Kon de privéthuismap van GitHub Copilot niet maken, dus het genereren is niet gestart (zonder die map zou de CLI je eigen Copilot-configuratie laden, inclusief de MCP-servers en hooks daarvan).
ai-inline-no-env-writer = { $cli } is gemarkeerd als geïsoleerd door zijn omgeving, maar Schemaic kan die omgeving niet opzetten, dus het genereren is niet gestart.
ai-inline-no-cursor-rules = Kon het machtigingenbestand van Cursor niet schrijven, dus het genereren is niet gestart (zonder dat bestand schrijft de CLI ongevraagd bestanden).
ai-inline-no-rules-writer = { $cli } is gemarkeerd als beperkt door een machtigingenbestand, maar Schemaic kan er geen voor schrijven, dus het genereren is niet gestart.
ai-no-tools = **Deze sessie heeft geen databasetools.** { $cause }, dus de assistent kan niets opzoeken — hij kan alleen antwoorden op basis van het schemaoverzicht dat al in zijn prompt staat. Het paneel sluiten en opnieuw openen probeert het nog eens.
ai-no-endpoint-file = Schemaic kon het privébestand niet maken dat de assistent vertelt hoe je database te bereiken is
ai-cursor-not-approved = Schemaic kon zijn databasetools niet laten goedkeuren door Cursor ({ $error })
ai-antigravity-not-registered = Schemaic kon zijn databasetools niet registreren bij Antigravity
ai-antigravity-shared = Een ander Schemaic-venster gebruikt Antigravity al. Antigravity houdt één MCP-registratie voor de hele computer bij, die maar naar één verbinding kan wijzen, dus de assistent van dit venster heeft geen databasetools
ai-antigravity-not-claimed = Schemaic kon de configuratie van Antigravity niet voor deze sessie claimen
ai-refused-no-opencode-config = Schemaic kon de configuratie die OpenCode beperkt niet schrijven, dus de assistent is uitgeschakeld — hem zonder dat bestand uitvoeren zou de sessie een shell geven. Controleer of er naar de gegevensmap van de app kan worden geschreven.
ai-refused-no-copilot-home = Schemaic kon de privéthuismap van GitHub Copilot niet maken, dus de assistent is uitgeschakeld — zonder die map zou de CLI je eigen Copilot-configuratie laden, inclusief de MCP-servers en hooks daarvan. Controleer of er naar de gegevensmap van de app kan worden geschreven.
ai-refused-no-cursor-rules = Schemaic kon het machtigingenbestand niet schrijven dat Cursor ervan weerhoudt bestanden te schrijven en opdrachten uit te voeren, dus de assistent is uitgeschakeld — zonder dat bestand schrijft Cursor ongevraagd bestanden. Controleer of er naar de gegevensmap van de app kan worden geschreven.
ai-launch-failed-turn = Kon de CLI `{ $bin }` niet starten ({ $error }). Zorg dat { $cli } is geïnstalleerd, of geef een pad op in Instellingen → AI.
ai-launch-failed-session = Kon de CLI `{ $bin }` niet starten ({ $error }). Zorg dat { $cli } is geïnstalleerd (of stel het pad in via Instellingen → AI).
ai-turn-ended = De beurt van { $cli } is onverwacht beëindigd: { $error }
ai-session-ended-badly = De AI-sessie is onverwacht beëindigd: { $error }
ai-session-ended = De sessie van { $cli } is beëindigd. Stel opnieuw een vraag om een nieuwe te starten.
ai-session-restart-failed = De sessie van { $cli } kon na het stoppen niet opnieuw worden gestart ({ $error }), dus ze is beëindigd. Stel opnieuw een vraag om een nieuwe te starten.
ai-session-not-accepting = De sessie van { $cli } accepteert geen vragen meer. Stel opnieuw een vraag om een nieuwe te starten.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = De context die naar { $cli } wordt gestuurd, is te groot voor één opdrachtregel ({ $total } { $unit }; dit platform staat ongeveer { $limit } toe). { $levers }
ai-unit-characters = tekens
ai-unit-bytes = bytes
ai-levers-schema-and-query = Beperk Instellingen → AI → Schemacontext tot de actieve database (of Geen), of maak de query in de editor korter.
ai-levers-sample = Zet de AI-toegang tot gegevens van deze verbinding lager dan ‘Gegevens laten lezen’, waardoor de voorbeeldrijen niet meer worden gestuurd, of beperk Instellingen → AI → Schemacontext.
ai-batch-shim = { $cli } verwees naar een batch-shim ({ $bin }), die geen prompt van meerdere regels kan krijgen — Windows voert een .cmd uit via cmd.exe, en dat weigert het argument voordat { $cli } wordt gestart. Stel Instellingen → AI → CLI-pad in op het echte uitvoerbare bestand (een npm-installatie heeft er meestal een onder node_modules, naast de shim).
ai-cli-exited = de CLI van { $cli } is afgesloten met status { $code }
ai-cli-signalled = de CLI van { $cli } is beëindigd door een signaal
ai-constraint-sandboxed = { $cli } draait in een sandbox: de eigen ingebouwde tools kunnen geen bestanden schrijven of opdrachten uitvoeren, al kunnen ze deze computer nog wel lezen.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}MCP-servers die je ermee hebt geregistreerd, zijn ook beschikbaar voor deze sessie, en Schemaic kan ze niet beperken.
    }
ai-constraint-workspace-rules = { $cli } wordt tot antwoorden beperkt door een machtigingenbestand dat Schemaic voor deze sessie schrijft, niet door een sandbox: de eigen tools mogen geen bestanden schrijven of opdrachten uitvoeren, maar kunnen nog wel bestanden op deze computer lezen.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}MCP-servers die je ermee hebt geregistreerd, starten ook met deze sessie; hun tools worden geweigerd, tenzij je eigen instellingen ze toestaan.
    }
ai-constraint-denylist = Deze versie van { $cli } accepteert de vlag niet die de ingebouwde tools leegmaakt, dus die worden in plaats daarvan tegengehouden door een blokkeerlijst — zwakker, en niet iets wat Schemaic kan garanderen. Een update van de CLI herstelt de volledige afscherming.
ai-constraint-unrestricted = Schemaic kon de ingebouwde tools van { $cli } voor deze sessie niet volledig beperken, dus het kan mogelijk meer doen dan vragen over deze database beantwoorden.
ai-constraint-unknown = Schemaic kon niet bevestigen dat dit uitvoerbare bestand van { $cli } de vlaggen accepteert die het beperken, dus de assistent is uitgeschakeld. Controleer het pad in Instellingen → AI.
ai-seed-empty = het model gaf geen inhoud terug
ai-seed-not-json = het antwoord was geen geldige JSON: { $error }
ai-seed-not-rows = het antwoord was geen JSON-array van rij-objecten

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } is geen bestand.
import-source-too-big = { $path } is { $size }, meer dan de { $most } die hiermee wordt gelezen.
import-source-unreadable = { $path } kon niet worden gelezen: { $error }
docker-not-found = Docker is niet gevonden — niet op PATH en niet in WSL.
docker-not-started = Docker kon niet worden gestart: { $error }
docker-timed-out = Docker antwoordde niet binnen { $secs } seconden.
docker-not-waited = Er kon niet op Docker worden gewacht: { $error }
docker-answered = Docker antwoordde: { $error }
docker-exited = Docker is afgesloten met { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = de bestandslezer is onverwacht gestopt: { $error }
script-changed-since-read = Dit bestand is op schijf gewijzigd nadat het werd gelezen, dus de samenvatting hierboven is niet wat zou worden uitgevoerd. Kies het opnieuw.
script-no-terminator = Dit bestand bevat geen instructiescheidingsteken, dus het is geen SQL-script dat Schemaic kan uitvoeren. Controleer of je het bedoelde bestand hebt gekozen.
ai-cursor-enable-unfinished = `mcp enable` is niet voltooid
