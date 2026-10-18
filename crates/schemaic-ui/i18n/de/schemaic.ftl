# Schemaic — German catalogue.
#
# Machine-drafted and unreviewed (docs/roadmap.md); a message missing here
# falls back to the en-GB catalogue. Not offered in Settings until complete —
# reach it with SCHEMAIC_LOCALE=de.

## Settings › Appearance › Language

settings-language = Sprache
settings-language-system = System ({ $language })
settings-language-restart = Die geänderte Sprache gilt ab dem nächsten Start von Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Schriftgröße
settings-term-cursor-style = Cursorform
settings-term-copy-on-select = Beim Markieren kopieren
settings-term-copy-on-select-hint = Markierten Text in die Zwischenablage kopieren, sobald die Auswahl endet.
settings-term-blink = Cursor blinkt
settings-term-blink-hint = Den Cursor blinken lassen, solange das Terminal den Fokus hat.

## Settings › AI

settings-ai-agent = Agent-CLI
settings-ai-off-hint = Nichts wird an einen Agenten gesendet, keine Agent-CLI wird gestartet und keine KI-Aktion angeboten. Wähle eine CLI, um den Assistenten einzuschalten.
settings-ai-cli-path = Pfad zu { $cli }
settings-ai-cli-path-placeholder = Leer lassen für automatische Erkennung
settings-ai-detected = Automatisch erkannt: { $path }
settings-ai-detect-failed = Automatische Erkennung fehlgeschlagen. { $cli } nicht gefunden – gib oben einen Pfad an.
settings-ai-cli-path-missing = Die Datei existiert nicht.
settings-ai-model = Modell
settings-ai-model-placeholder = Leer lassen für die Vorgabe der CLI
settings-ai-model-hint = Jede ID, die diese CLI akzeptiert – ein Alias oder ein datierter Snapshot, um ein Modell über Updates hinweg festzuhalten.
settings-ai-model-hint-with-chips = Jede ID, die diese CLI akzeptiert – ein Alias oder ein datierter Snapshot, um ein Modell über Updates hinweg festzuhalten. Die Schaltflächen sind Abkürzungen, nicht die vollständige Liste.
settings-ai-effort = Aufwand
settings-ai-instructions = Eigene Anweisungen
settings-ai-instructions-placeholder = Dialekt, Konventionen, Hausregeln…
settings-ai-schema-context = Schemakontext
settings-ai-schema-context-hint = Wie viel Datenbankstruktur jede Nachricht mitführt. „Keiner“ sperrt auch die Schema-Werkzeuge, sodass der Assistent dich nach Namen fragt, statt den Katalog selbst zu lesen.
settings-ai-data-access = Datenzugriff
settings-ai-data-access-where = Pro Verbindung festgelegt, in den Einstellungen der Verbindung.
settings-ai-gutter = Akzentlinie an Antworten
settings-ai-gutter-hint = Die Antworten des Assistenten mit einer farbigen Linie am rechten Rand markieren. Aus gibt ihnen auf beiden Seiten denselben Rand.

## Settings › section list

settings-section-general = Allgemein
settings-section-editor = Editor
settings-section-formatting = Code-Formatierung
settings-section-query = Abfrage
settings-section-terminal = Terminal
settings-section-ai = KI-Assistent
settings-section-appearance = Darstellung

## Settings › shared

settings-off = Aus

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } Zeichen
   *[other] { $n } Zeichen
}

## Settings › General

settings-log = Protokolldatei
settings-log-hint = Diagnosedaten, auch zu Abstürzen, werden nach { $path } geschrieben.
settings-log-hint-none = Auf diesem Rechner ließ sich keine Protokolldatei öffnen, daher wird in keine geschrieben.
settings-log-open-folder = Ordner öffnen
settings-bundle = Einstellungsdatei
settings-bundle-hint = Deine Einstellungen und Snippets in einer Datei, um sie auf einen anderen Rechner mitzunehmen. Verbindungen, Fensteraufteilung, die Shell des Terminals und verbindungsgebundene Snippets bleiben hier.
settings-bundle-export = Exportieren
settings-bundle-import = Importieren
settings-cli = Kommandozeile
settings-cli-install = Installieren
settings-cli-installing = Wird installiert
settings-cli-remove = Entfernen
settings-cli-removing = Wird entfernt

## Settings › modal

settings-title = Einstellungen
settings-restore-tabs = Tabs beim Start wiederherstellen
settings-restore-tabs-hint = Die Abfrage-Tabs der letzten Sitzung beim Start der App wieder öffnen.

## Settings › Editor

settings-editor-font-size = Schriftgröße
settings-editor-predictions = Inline-Vorschläge
settings-editor-predictions-hint = Die wahrscheinlich nächsten Wörter hinter dem Cursor zeigen – das Gegenstück eines Schlüsselworts, den Fremdschlüssel eines Joins, die Spalten zum Gruppieren oder Einfügen. Tab oder → am Zeilenende übernimmt sie ganz, Strg+→ (⌘→ unter macOS) wortweise, und Escape lehnt sie ab.
settings-editor-remembered = Vorschläge aus früheren Anweisungen
settings-editor-remembered-hint = Auch den Rest einer Zeile aus einer Anweisung anbieten, die du ausgeführt, in einem anderen Tab geschrieben oder als Snippet gespeichert hast. Ausschalten, um frühere Anweisungen vom Bildschirm fernzuhalten.
settings-editor-aliases = Tabellen-Aliasse
settings-editor-aliases-hint = Wenn die Vervollständigung eine Tabelle nach FROM oder JOIN schreibt, einen kurzen Alias aus ihren Anfangsbuchstaben anhängen – orders o, order_items oi – und den Alias in der ON-Bedingung eines Joins verwenden.
settings-editor-rainbow = Regenbogen-Klammern
settings-editor-rainbow-hint = Jedes Klammerpaar nach seiner Verschachtelungstiefe einfärben, damit sich eine schließende Klammer auf einen Blick ihrer öffnenden zuordnen lässt.
settings-editor-whitespace = Leerraum anzeigen
settings-editor-whitespace-hint = Leerzeichen und Tabs als blasse Markierungen zeichnen, damit sich ein Tab von Leerzeichen unterscheiden lässt.
settings-editor-guides = Einrückungshilfen
settings-editor-guides-hint = Auf jeder Einrückungsebene eine blasse senkrechte Linie zeichnen.

## Settings › Code formatting

settings-format-keyword-case = Schreibweise der Schlüsselwörter
settings-format-keyword-case-hint = Wie „Code formatieren“ Schlüsselwörter wie SELECT und FROM schreibt. Namen, Zeichenketten und Kommentare behalten ihre Schreibweise.
settings-format-function-case = Schreibweise der Funktionen
settings-format-function-case-hint = Wie „Code formatieren“ die eingebauten Funktionen der Datenbank wie COUNT und COALESCE schreibt, wo sie aufgerufen werden. Eigene Funktionen behalten ihre Schreibweise.
settings-format-type-case = Schreibweise der Typen
settings-format-type-case-hint = Wie „Code formatieren“ die eingebauten Datentypen der Datenbank wie INT und VARCHAR schreibt. Selbst angelegte Typen behalten ihre Schreibweise.
settings-format-commas = Kommas
settings-format-commas-hint = Wo „Code formatieren“ das Komma einer Liste setzt, die es auf mehrere Zeilen umbricht. Listen auf einer Zeile, etwa die Argumente einer Funktion, bleiben unverändert, sofern die Umbruchbreite sie nicht umbricht.
settings-format-statement-gap = Zwischen Anweisungen
settings-format-statement-gap-hint = Die Leerzeilen, die „Code formatieren“ zwischen zwei Anweisungen lässt. „Wie eingegeben“ behält eine Leerzeile überall dort, wo eine war.
settings-format-wrap = Umbruchbreite
settings-format-wrap-hint = Eine Liste, die „Code formatieren“ auf einer Zeile hält, etwa die Argumente einer Funktion, bekommt ein Element pro Zeile, sobald ihre Zeile über diese Breite hinausginge.
settings-format-river = Klausel-Schlüsselwörter rechtsbündig
settings-format-river-hint = SELECT, FROM, WHERE und die übrigen an ihrer rechten Kante ausrichten, den Rumpf jeder Klausel neben ihrem Schlüsselwort, statt eines Schlüsselworts auf eigener Zeile.
settings-format-align = Spalten ausrichten
settings-format-align-hint = Das AS der Aliasse einer Select-Liste, das = der Zuweisungen eines UPDATE und die Typen der Spalten einer Tabelle untereinander ausrichten.
settings-format-join-on = Join-Bedingung auf eigener Zeile
settings-format-join-on-hint = Das ON eines Joins und das AND und OR seiner Bedingung jeweils auf eine eigene Zeile unter den Join setzen.

## Settings › Query

settings-query-row-limit = Standard-Zeilenlimit
settings-query-timeout = Zeitlimit für Anweisungen
settings-query-timeout-hint = Eine Anweisung abbrechen, die länger als diese Zeit läuft. Standardmäßig aus – ein Import oder ein Bericht darf durchaus länger dauern.
settings-query-confirm-writes = Vor schreibenden Anweisungen nachfragen
settings-query-confirm-writes-hint = Vor jeder Anweisung nachfragen, die Daten oder das Schema ändert.
settings-query-validate = Live-Prüfung gegen die Datenbank
settings-query-validate-hint = Die Anweisung unter dem Cursor beim Tippen gegen die Datenbank prüfen (ein PREPARE ohne Ausführung), um genaue Fehler anzuzeigen. Kostet bei jeder Pause eine Anfrage an die Datenbank.
settings-query-notify = Benachrichtigen, wenn ein Lauf im Hintergrund endet
settings-query-notify-hint = Eine Desktop-Benachrichtigung zeigen, wenn eine Abfrage, ein Skript, ein Export oder ein Dump, der eine Sekunde oder länger lief, endet, während Schemaic nicht das aktive Fenster ist. Die Taskleisten-Schaltfläche wird in jedem Fall hervorgehoben. Die Benachrichtigung enthält nie das SQL.
settings-query-write-log = Werte im Schreibprotokoll behalten
settings-query-write-log-hint = Jede schreibende Anweisung, die Schemaic oder schemaic exec ausführt, wird an write_log.jsonl im Einstellungsordner angehängt und nie gelöscht. Werte in ihrem SQL werden maskiert, solange dies aus ist; ein Passwort wird in jedem Fall maskiert.

## Settings › Appearance

settings-appearance-ui-theme = Oberflächen-Design
settings-appearance-editor-theme = Editor-Design
settings-appearance-scale = Oberflächengröße

## Shortcuts modal

shortcuts-title = Tastenkürzel

## Shortcuts modal › group headings

shortcuts-group-global = Global
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Ergebnistabelle
shortcuts-group-schema-tree = Schemabaum
shortcuts-group-ai-panel = KI-Bereich
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = ER-Diagramm

## Shortcuts modal › rows

shortcuts-find-anywhere = Überall suchen
shortcuts-command-palette = Befehlspalette
shortcuts-new-tab = Neuer Abfrage-Tab
shortcuts-close-tab = Abfrage-Tab schließen
shortcuts-next-tab = Nächster Tab
shortcuts-previous-tab = Vorheriger Tab
shortcuts-jump-to-tab = Zu Tab springen
shortcuts-reopen-tab = Zuletzt geschlossenen Tab wieder öffnen
shortcuts-open-file = SQL-Datei öffnen
shortcuts-save-file = SQL-Datei speichern
shortcuts-save-file-as = SQL-Datei speichern unter
shortcuts-toggle-schema = Schemabereich ein-/ausblenden
shortcuts-toggle-ai = KI-Bereich ein-/ausblenden
shortcuts-toggle-terminal = Terminal ein-/ausblenden
shortcuts-run-query = Abfrage ausführen
shortcuts-autocomplete = Autovervollständigung
shortcuts-inline-ai = KI-Bearbeitung im Editor
shortcuts-find-in-editor = Im Editor suchen
shortcuts-find-replace = Suchen und ersetzen
shortcuts-go-to-line = Gehe zu Zeile
shortcuts-matching-bracket = Zur passenden Klammer oder zu BEGIN/END springen
shortcuts-line-comment = Zeilenkommentar umschalten
shortcuts-block-comment = Blockkommentar umschalten
shortcuts-toggle-case = Groß-/Kleinschreibung umschalten
shortcuts-join-lines = Zeilen verbinden
shortcuts-duplicate = Zeile / Auswahl duplizieren
shortcuts-cut = Zeile / Auswahl ausschneiden
shortcuts-format-code = Code formatieren
shortcuts-format-statement = Anweisung formatieren
shortcuts-single-line = In eine Zeile umwandeln
shortcuts-move-line = Zeile nach oben / unten verschieben
shortcuts-quick-fix = Schnellkorrektur am Cursor anwenden
shortcuts-indent = Einrücken / Ausrücken
shortcuts-take-prediction = Inline-Vorschlag übernehmen (am Zeilenende auch →) / sein nächstes Wort
shortcuts-find-in-results = In Ergebnissen suchen
shortcuts-go-to-row = Gehe zu Zeile
shortcuts-copy = Kopieren
shortcuts-paste-staged = Als vorgemerkte Änderungen einfügen
shortcuts-select-all = Alles auswählen
shortcuts-first-last-cell = Erste / letzte Zelle
shortcuts-edit-cell = Zelle bearbeiten
shortcuts-next-cell = Nächste / vorherige Zelle beim Bearbeiten
shortcuts-commit-edits = Änderungen übernehmen
shortcuts-mark-deletion = Zeile zum Löschen vormerken
shortcuts-results-toolbar = Zur Werkzeugleiste der Ergebnisse
shortcuts-along-toolbar = In der Werkzeugleiste bewegen (Esc kehrt zur Tabelle zurück)
shortcuts-open-selected = Ausgewählte Tabelle, Spalte oder Objekt öffnen
shortcuts-context-menu = Kontextmenü der ausgewählten Zeile
shortcuts-send-message = Nachricht senden
shortcuts-new-line = Neue Zeile
shortcuts-recall-prompt = Vorherige / nächste Eingabe zurückholen
shortcuts-copy-paste = Kopieren / Einfügen
shortcuts-find-in-diagram = Im Diagramm suchen

## Key names in shortcut labels — the German keyboard's legends

key-ctrl = Strg
key-shift = Umschalt
key-alt = Alt
key-enter = Eingabe
key-del = Entf
key-home = Pos1
key-end = Ende
key-space = Leertaste

## Command palette › commands

palette-toggle-panel = Bereich umschalten
palette-toggle-panel-hint = Schema · KI · Terminal · Abfrageverlauf · Serveraktivität
palette-panel-schema = Schema
palette-panel-ai = KI
palette-panel-terminal = Terminal
palette-panel-history = Abfrageverlauf
palette-panel-activity = Serveraktivität
palette-new-tab = Neuer Tab
palette-export-settings = Einstellungen exportieren
palette-import-settings = Einstellungen importieren
palette-duplicate-tab = Tab duplizieren
palette-close-tab = Tab schließen
palette-reopen-closed-tab = Geschlossenen Tab wieder öffnen
palette-reopen-closed-tab-hint = <Tab>
palette-open-file = Datei öffnen
palette-open-file-hint = ein .sql-Skript
palette-save-file = Datei speichern
palette-save-file-as = Datei speichern unter
palette-next-tab = Nächster Tab
palette-previous-tab = Vorheriger Tab
palette-format-code = Code formatieren
palette-format-statement = Anweisung formatieren
palette-format-statement-hint = die Anweisung am Cursor
palette-make-single-line = In eine Zeile umwandeln
palette-make-single-line-hint = die Auswahl oder Anweisung, auf einer Zeile
palette-run = Ausführen
palette-run-hint = alle Anweisungen ausführen
palette-go-to-line = Gehe zu Zeile
palette-go-to-line-hint = <Nummer>
palette-go-to-statement = Gehe zu Anweisung
palette-search-hint = <Suche>
palette-history = Verlauf
palette-clear-history = Abfrageverlauf leeren
palette-clear-history-hint = aktuelle Verbindung
palette-ask-ai = KI fragen
palette-ask-ai-hint = <Frage>
palette-ask-ai-empty = Frage eingeben…
palette-terminal = Terminal
palette-terminal-hint = <Befehl>
palette-run-in-terminal = Im Terminal ausführen
palette-terminal-empty = Befehl eingeben…
palette-ui-theme = Oberflächen-Design
palette-ui-theme-hint = hell · dunkel
palette-editor-theme = Editor-Design
palette-editor-theme-hint = <Design>
palette-font-size = Schriftgröße
palette-increase-font-size = Schrift vergrößern
palette-decrease-font-size = Schrift verkleinern
palette-indent-style = Einrückungsart
palette-indent-style-hint = Tabs · Leerzeichen
palette-indent-spaces = Leerzeichen
palette-indent-tabs = Tabs
palette-indent-width = Einrückungsbreite
palette-toggle-word-wrap = Zeilenumbruch umschalten
palette-toggle-whitespace = Leerraum umschalten
palette-toggle-whitespace-hint = Leerzeichen und Tabs anzeigen
palette-toggle-indent-guides = Einrückungshilfen umschalten
palette-switch-connection = Verbindung wechseln
palette-switch-connection-hint = <Verbindung>
palette-server-variables = Servervariablen
palette-server-variables-hint = die Einstellungen der aktiven Verbindung
palette-enter-number = Eine Zahl eingeben
palette-nothing-found = Nichts gefunden

## Command palette › what a result row is

palette-row-snippet = Snippet
palette-row-open-tab = offener Tab
palette-row-history = Verlauf
palette-row-view-body = Rumpf der Sicht
palette-row-body = Rumpf ({ $kind })
palette-row-trigger-on = Trigger auf { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu

tree-open-in-cli = In der CLI öffnen
tree-open = Öffnen
tree-open-in-new-tab = In neuem Tab öffnen
tree-copy-name = Namen kopieren
tree-copy-qualified-name = Qualifizierten Namen kopieren
tree-properties = Eigenschaften
tree-used-by = Verwendet von
tree-live-monitor = Live-Überwachung
tree-er-diagram = ER-Diagramm
tree-compare-with = Vergleichen mit
tree-generate = Generieren
tree-generate-ddl = DDL generieren
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Aktualisieren
tree-collapse-all = Alle einklappen
tree-favourite = Zu Favoriten
tree-unfavourite = Aus Favoriten entfernen
tree-colour = Farbe
tree-colour-none = Keine
tree-hide = Ausblenden
tree-import = Importieren
tree-export = Exportieren
tree-back-up = Sichern
tree-restore-from-file = Aus Datei wiederherstellen
tree-copy-to = Kopieren nach
tree-create = Erstellen
tree-create-table = Tabelle
tree-create-view = Sicht
tree-create-type = Typ
tree-create-domain = Domäne
tree-create-sequence = Sequenz
tree-create-synonym = Synonym
tree-create-alias-type = Aliastyp
tree-create-xml-schema-collection = XML-Schemaauflistung
tree-create-function = Funktion
tree-create-procedure = Prozedur
tree-create-event = Ereignis
tree-create-extension = Erweiterung
tree-create-database = Datenbank
tree-create-schema = Schema
tree-create-kind = Erstellen: { $kind }
tree-edit-kind = Bearbeiten: { $kind }
tree-folder-create-extension = Erweiterung erstellen
tree-edit-table = Tabelle bearbeiten
tree-edit-view = Sicht bearbeiten
tree-edit-column = Spalte bearbeiten
tree-edit-index = Index bearbeiten
tree-edit-primary-key = Primärschlüssel bearbeiten
tree-edit-foreign-key = Fremdschlüssel bearbeiten
tree-triggers = Trigger
tree-triggers-count = Trigger ({ $n })
tree-refresh-view = Sicht aktualisieren
tree-truncate = Leeren
tree-truncate-title = Tabelle leeren
tree-drop = Löschen
tree-drop-index = Index löschen
tree-drop-foreign-key = Fremdschlüssel löschen
tree-ai-explain = Mit KI erklären
tree-users = Benutzer und Rechte
tree-server-variables = Servervariablen
tree-new-database = Datenbank erstellen
tree-show-table-sizes = Tabellengrößen anzeigen

## Connection switcher

conn-menu-manage = Verbindungen verwalten

## Shared dialog buttons

dialog-cancel = Abbrechen
dialog-yes = Ja
dialog-no = Nein

## Open-transaction prompt

tx-prompt-title = Offene Transaktion
tx-prompt-body = { $tab } hat eine offene Transaktion mit { $n ->
    [one] einer Anweisung
   *[other] { $n } Anweisungen
}. Du kannst sie committen, um die Änderungen zu behalten, oder per Rollback verwerfen.
tx-prompt-body-aborted = { $tab } hat eine Transaktion mit { $n ->
    [one] einer Anweisung
   *[other] { $n } Anweisungen
}, die eine fehlgeschlagene Anweisung abgebrochen hat. Sie kann nur zurückgerollt werden, wodurch die Änderungen verworfen werden.
tx-prompt-rollback = Rollback
tx-prompt-commit = Commit

## Error modal

error-modal-title = Fehler
error-modal-none = Kein Fehler.
error-modal-ai-fix = KI-Korrektur
error-modal-explain = Erklären

## Schema tree › the panel

tree-panel-title = SCHEMA
tree-tip-databases = Datenbanken ein- oder ausblenden
tree-tip-options = Schema-Optionen
tree-search-placeholder = Suchen…
tree-loading = Wird geladen
tree-no-tables = Keine Tabellen
tree-failed-to-connect = Verbindung fehlgeschlagen
tree-failed-view = Anzeigen
tree-single-user-unread = Einzelbenutzer – nicht gelesen
tree-single-user-read = Lesen
tree-count-tables = { $n ->
    [one] { $n } Tabelle
   *[other] { $n } Tabellen
}
tree-count-cols = { $n ->
    [one] { $n } Spalte
   *[other] { $n } Spalten
}
tree-count-keys = { $n ->
    [one] { $n } Schlüssel
   *[other] { $n } Schlüssel
}
tree-count-partitions = { $n ->
    [one] { $n } Partition
   *[other] { $n } Partitionen
}

## Object kinds

kind-type = Typ
kind-domain = Domäne
kind-sequence = Sequenz
kind-function = Funktion
kind-procedure = Prozedur
kind-event = Ereignis
kind-synonym = Synonym
kind-alias-type = Aliastyp
kind-xml-schema-collection = XML-Schemaauflistung
kind-extension = Erweiterung
kind-group-type = Typen
kind-group-domain = Domänen
kind-group-sequence = Sequenzen
kind-group-function = Funktionen
kind-group-procedure = Prozeduren
kind-group-event = Ereignisse
kind-group-synonym = Synonyme
kind-group-alias-type = Aliastypen
kind-group-xml-schema-collection = XML-Schemaauflistungen
kind-group-extension = Erweiterungen

## Query history panel

history-title = ABFRAGEVERLAUF
history-clear-tip = Verlauf dieser Verbindung leeren…
history-clear-title = Abfrageverlauf leeren
history-clear-message = { $n ->
    [one] { $n } gespeicherte Abfrage
   *[other] { $n } gespeicherte Abfragen
} dieser Verbindung löschen? Das lässt sich nicht rückgängig machen.
history-scope-current = Aktuell
history-scope-all = Alle
history-cut-note = Die neuesten { $shown } von { $matched } – suchen, um einzugrenzen.
history-empty = Noch keine Abfragen.
history-no-match = Keine passenden Abfragen.
history-open-in-new-tab = In neuem Tab öffnen
history-delete = Löschen
history-failed = Fehlgeschlagen

## Snippet library panel

snippets-title = SNIPPET-BIBLIOTHEK
snippets-save-tip = Das SQL im Editor als Snippet speichern
snippets-no-match = Kein passendes Snippet.
snippets-band-this-connection = DIESE VERBINDUNG
snippets-band-all-connections = ALLE VERBINDUNGEN
snippets-builtin = Mitgeliefert
snippets-name-placeholder = Name des Snippets
snippets-insert = In den Editor einfügen
snippets-open-in-new-tab = In neuem Tab öffnen
snippets-edit = Bearbeiten
snippets-duplicate = Duplizieren
snippets-delete = Löschen
snippets-show-in = Anzeigen in
snippets-scope-this-connection = Diese Verbindung
snippets-scope-engine = Jede { $engine }-Verbindung
snippets-scope-all-connections = Alle Verbindungen

## Query history panel › recency bands

history-band-today = HEUTE
history-band-this-week = DIESE WOCHE
history-band-earlier = FRÜHER

## Query tab strip

tabs-tip-unsaved = { $path } – ungespeicherte Änderungen
tabs-pin = Anheften
tabs-unpin = Lösen
tabs-rename = Umbenennen
tabs-duplicate = Duplizieren
tabs-close = Schließen
tabs-open-file = Datei öffnen
tabs-save = Speichern
tabs-save-as = Speichern unter
tabs-reload = Von der Festplatte neu laden
tabs-reopen-last = Letzten Tab wieder öffnen
tabs-close-others = Andere Tabs schließen
tabs-close-all = Alle Tabs schließen

## Server Activity panel

activity-title = SERVERAKTIVITÄT
activity-refresh-tip = Jetzt aktualisieren
activity-auto-refresh-off = Automatische Aktualisierung aus
activity-auto-refresh-every = Automatisch alle { $secs } s aktualisieren
activity-view-sessions = Sitzungen
activity-view-locks = Sperren
activity-search-sessions = Sitzungen durchsuchen…
activity-search-locks = Sperren durchsuchen…
activity-loading = Wird geladen…
activity-sqlite-no-sessions = SQLite hat keine Serversitzungen – es läuft in diesem Prozess, daher ist nichts anderes verbunden, das sich anzeigen ließe.
activity-sessions-no-match = Keine passenden Sitzungen.
activity-sessions-none = Keine anderen Sitzungen auf diesem Server.
activity-sessions-undrawn = { $n ->
    [one] { $n } weitere passende Sitzung – Suche eingrenzen, um sie zu sehen.
   *[other] { $n } weitere passende Sitzungen – Suche eingrenzen, um sie zu sehen.
}
activity-locks-no-match = Keine passenden Sperren.
activity-locks-no-waits = Keine Sperrwartezeiten auf diesem Server.
activity-locks-none = Keine andere Sitzung hält eine Sperre.
activity-locks-undrawn = { $n ->
    [one] { $n } weitere passende Sperre – Suche eingrenzen, um sie zu sehen.
   *[other] { $n } weitere passende Sperren – Suche eingrenzen, um sie zu sehen.
}
activity-list-capped = Liste gekürzt
activity-lock-wait = Sperrwartezeit
activity-kill-holder = { $id } beenden
activity-terminate-tip = Sitzung { $id } beenden
activity-read-only-tip = Diese Verbindung ist als schreibgeschützt markiert.
activity-lock-holds = hält
activity-lock-waits-for = wartet auf
activity-lock-held-by = gehalten von { $ids }
activity-copy-statement = Anweisung kopieren
activity-cancel-query = Abfrage abbrechen
activity-kill-session = Sitzung beenden

## AI assistant panel

ai-title = KI-ASSISTENT
ai-new-chat-tip = Neuer Chat
ai-settings-tip = KI-Einstellungen
ai-empty-ask = Frag etwas zu deinem SQL…
ai-empty-not-connected = { $cli } ist nicht verbunden.
ai-input-placeholder = Nachricht…
ai-input-off = Der KI-Assistent ist aus
ai-input-db-down = Nicht mit der Datenbank verbunden
ai-speaker-you = DU
ai-result-more-lines = … (+{ $n } weitere Zeilen)

## Shared dialog buttons (continued)

dialog-close = Schließen

## Chart modal

chart-title = Diagramm – { $tab }
chart-caption-kind = Diagramm
chart-caption-values = Werte
chart-caption-labels = Beschriftungen
chart-row-number = Zeilennummer
chart-kind-bar = Balken
chart-kind-line = Linie
chart-kind-histogram = Histogramm
chart-save-tip = Diagramm speichern…
chart-save-dialog = Diagramm speichern
chart-file-stem = { $name } Diagramm
chart-nothing-to-save = Nichts zu speichern.
chart-no-numbers = In diesem Ergebnis gibt es keine Zahlenspalte für ein Diagramm.
chart-all-null = Jeder Wert dieser Spalte ist NULL.

## Schema editors — shared

editor-preview-sql = SQL-Vorschau
editor-change-count = { $n ->
    [0] Keine Änderungen
    [one] 1 Änderung
   *[other] { $n } Änderungen
}

## View editor

view-editor-title-edit = Sicht { $view } bearbeiten
view-editor-title-create = Sicht in { $location } erstellen
view-editor-section-view = Sicht
view-editor-section-definition = Definition
view-editor-section-options = Optionen
view-editor-name = Name
view-editor-select = SELECT-Anweisung
view-editor-check-option = Check-Option
view-editor-column-names = Spaltennamen
view-editor-column-names-placeholder = optional – aus dem SELECT übernommen, wenn leer
view-editor-sql-security = SQL-Sicherheit
view-editor-definer = Definer
view-editor-recreate = Neu erstellen statt ersetzen
view-editor-recreate-hint = PostgreSQL kann einer bestehenden Sicht nur Spalten hinzufügen. Eine umzubenennen, ihren Typ zu ändern oder sie umzuordnen braucht ein DROP und ein CREATE, das abhängige Sichten und Rechte mitnimmt.

## Table properties panel

props-title = Eigenschaften – { $table }
props-reading = Statistiken werden gelesen
props-no-stats-view = Eine Sicht hat keinen eigenen Speicher, daher meldet der Server keine Statistiken für sie.
props-no-stats-table = Der Server hat für diese Tabelle keine Statistiken gemeldet.
props-on-disk = auf der Festplatte
props-counting = Wird gezählt
props-count-rows = Zeilen zählen
props-count-slow = Ein vollständiger Durchlauf der Tabelle. Bei einer großen langsam.
props-storage-data = Daten
props-storage-indexes = Indizes
props-storage-free = Frei
props-section-structure = Struktur
props-columns = Spalten
props-indexes = Indizes
props-foreign-keys = Fremdschlüssel
props-count-columns = { $n ->
    [one] { $n } Spalte
   *[other] { $n } Spalten
}
props-count-indexes = { $n ->
    [one] { $n } Index
   *[other] { $n } Indizes
}
props-count-keys = { $n ->
    [one] { $n } Schlüssel
   *[other] { $n } Schlüssel
}
props-type = Typ
props-type-view = Sicht
props-type-table = Tabelle
props-section-options = Optionen
props-engine = Engine
props-row-format = Zeilenformat
props-collation = Sortierung
props-next-auto-increment = Nächster Auto-Increment
props-created = Erstellt
props-updated = Aktualisiert
props-comment = Kommentar
props-section-partitioning = Partitionierung
props-section-indexes = Indizes
props-vacuum-note = { $pct } % der Zeilenversionen dieser Tabelle sind tot und noch nicht freigegeben. Ein VACUUM würde diesen Platz zurückgeben.
props-copy = Kopieren

## Shared panel search box

panel-search-placeholder = Suchen…

## How long ago

ago-just-now = gerade eben
ago-minutes = vor { $n } Min.
ago-hours = vor { $n } Std.
ago-days = { $n ->
    [one] vor { $n } Tag
   *[other] vor { $n } Tagen
}
ago-weeks = { $n ->
    [one] vor { $n } Woche
   *[other] vor { $n } Wochen
}

## A history row's outcome line

outcome-rows = { $n ->
    [one] { $n } Zeile
   *[other] { $n } Zeilen
}
outcome-rows-capped = { $n }+ Zeilen

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = Datenbank-CLI öffnen
term-db-cli-tip-read-only = Datenbank-CLI öffnen – diese Verbindung ist als schreibgeschützt markiert
term-restart-tip = Terminal neu starten
term-settings-tip = Terminal-Einstellungen

## Results panel header

results-title = ERGEBNISSE
results-properties-tip = Tabelleneigenschaften…
results-monitor-tip = Live-Überwachung…
results-restore-editor-tip = Editor wiederherstellen
results-collapse-editor-tip = Editor einklappen

## Query panel header

query-title = ABFRAGE
query-no-database = Keine Datenbank

## Shared dialog buttons (continued)

dialog-stop = Anhalten

## Copy table modal

copy-table-title = { $table } kopieren
copy-table-into = Kopieren nach
copy-table-schema = Schema
copy-table-name = Name
copy-table-back = ‹ zurück
copy-table-data = Daten kopieren
copy-table-data-hint = Die Zeilen kopieren, sobald die Tabelle erstellt ist. Aus kopiert nur die Struktur.
copy-table-what-with-rows = Die Struktur und die Zeilen. Fremdschlüssel und Trigger werden nicht kopiert, und eine generierte Spalte wird neu berechnet.
copy-table-what-structure = Nur die Struktur. Fremdschlüssel und Trigger werden nicht kopiert.
copy-table-choose-database = Wähle die Datenbank, in die kopiert werden soll.
copy-table-connection-changed = Die aktive Verbindung hat sich geändert – öffne „Kopieren nach“ erneut von der Tabelle aus.
copy-table-schema-not-loaded = Das Schema von { $database } ist nicht geladen – klappe es im Baum auf und versuche es dann erneut.
copy-table-preview-subject = Kopie von { $table }
copy-table-copying = Zeilen werden kopiert… { $n }
copy-table-copied = { $n ->
    [one] { $n } Zeile kopiert.
   *[other] { $n } Zeilen kopiert.
}
copy-table-stopped = Angehalten, bevor alle Zeilen kopiert waren. Die Tabelle wurde erstellt.

## Database, schema and extension editor

database-editor-title-database = Datenbank erstellen
database-editor-title-schema = Schema erstellen
database-editor-title-extension = Erweiterung erstellen
database-editor-title-schema-in = Schema in { $database } erstellen
database-editor-title-extension-in = Erweiterung in { $database } erstellen
database-editor-section-database = Datenbank
database-editor-section-schema = Schema
database-editor-section-extension = Erweiterung
database-editor-name = Name
database-editor-charset = Zeichensatz
database-editor-collation = Sortierung
database-editor-owner = Eigentümer
database-editor-server-default = Vorgabe des Servers
database-editor-owner-you = du
database-editor-no-extensions = Keine Erweiterungen mehr zu installieren
database-editor-no-charsets = Keine Zeichensätze vorzuschlagen
database-editor-no-collations = Keine Sortierungen vorzuschlagen
database-editor-no-roles = Keine Rollen auf diesem Server

## SQL script import modal

script-title = Import in { $database }
script-dialog-title = SQL-Skript ausführen
script-section = SQL-Skript
script-choose-file = Datei wählen…
script-no-file = Keine Datei gewählt
script-reading = Datei wird gelesen
script-hint = Das Skript kann eine andere Datenbank als die, in die du importierst, anlegen und auswählen. Es kann auch mehr als eine Datenbank anlegen. Prüfe das Skript, damit es tut, was du erwartest.
script-hint-namespace = Das Skript kann eine andere Datenbank als die, in die du importierst, anlegen und auswählen. Es kann auch mehr als eine Datenbank anlegen. Das Öffnen aus { $namespace } legt das Skript auch nicht dort hinein: Alles, was die Datei nicht qualifiziert, landet dort, wohin der search_path des Servers zeigt. Prüfe das Skript, damit es tut, was du erwartest.
script-what-it-does = Was diese Datei tut
script-probe-nothing = Diese Datei enthält keine Anweisungen, die Schemaic ausführen kann.
script-probe-cut-off = Die ersten { $size } dieser Datei sind eine einzige unvollständige Anweisung, daher gibt es noch nichts zusammenzufassen. Beim Ausführen wird die Datei vollständig gesendet.
script-probe-counted = { $count } { $n ->
    [one] Anweisung
   *[other] Anweisungen
} aus den ersten { $size } gelesen.
script-destroys = { $count } { $n ->
    [one] Anweisung in dieser Datei zerstört
   *[other] Anweisungen in dieser Datei zerstören
} Daten – DROP, TRUNCATE oder DELETE. Das lässt sich von hier aus nicht rückgängig machen.
script-unread = Nur die ersten { $size } dieser Datei wurden gelesen, und nichts darin zerstört Daten – aber der Rest wurde nicht angesehen. Er kann DROP, TRUNCATE oder DELETE enthalten, und das lässt sich von hier aus nicht rückgängig machen.
script-unqualified = { $count } { $n ->
    [one] Anweisung in dieser Datei wirkt
   *[other] Anweisungen in dieser Datei wirken
} auf jede Zeile einer Tabelle – ein DELETE, UPDATE oder TRUNCATE ohne WHERE. Das lässt sich von hier aus nicht rückgängig machen.
script-tx-own = Diese Datei öffnet ihre eigene Transaktion, sie landet also ganz oder gar nicht. Schemaic fügt keine eigene hinzu.
script-tx-none = Diese Datei öffnet keine Transaktion, ein Fehler mittendrin lässt also stehen, was davor lief.
script-connecting = Verbindung wird hergestellt
script-progress-of = { $done } von { $total }
script-progress-read = { $done } gelesen
script-run = Ausführen
script-done = { $n ->
    [one] { $n } Anweisung
   *[other] { $n } Anweisungen
} aus { $file } ausgeführt.
script-stopped = Nach { $n ->
    [one] { $n } Anweisung
   *[other] { $n } Anweisungen
} angehalten.
script-failed = Fehlgeschlagen: { $message } – davor { $n ->
    [one] lief { $n } Anweisung
   *[other] liefen { $n } Anweisungen
}.
script-failed-at = Fehlgeschlagen in Zeile { $line } von { $file }: { $message } – davor { $n ->
    [one] lief { $n } Anweisung
   *[other] liefen { $n } Anweisungen
}.
script-durability-rolled-back = { $file } hat eine eigene Transaktion geöffnet, daher hat der Server den ganzen Lauf zurückgerollt – nichts wurde übernommen, auch nicht die erfolgreichen Anweisungen.
script-durability-applied = { $file } hat keine Transaktion geöffnet, daher ist jede Anweisung, die lief, weiterhin übernommen.
script-durability-applied-before = Jede Anweisung, die lief, bevor { $file } eine eigene Transaktion öffnete, ist weiterhin übernommen; was innerhalb der Transaktion lief, wurde zurückgerollt.
script-durability-unknown = Jede bereits gelaufene Anweisung ist weiterhin übernommen, es sei denn, { $file } hat eine eigene Transaktion geöffnet.
