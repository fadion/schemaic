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
settings-ai-disabled = Deaktiviert
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
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Nur Schema
ai-data-on-request = Nur was ich anhänge
ai-data-full = Daten lesen lassen
ai-data-schema-only-hint = Nur Struktur — Namen, Typen und Definitionen. Keine Zeile verlässt je diesen Rechner, und Anhängen wird abgelehnt.
ai-data-on-request-hint = Der Assistent liest von sich aus keine Daten. Zeilen, die du aus einem Ergebnis anhängst, verlassen diesen Rechner mit dieser Frage.
ai-data-full-hint = Der Assistent darf von sich aus schreibgeschützte Abfragen ausführen und Beispielzeilen lesen. Was er liest, verlässt diesen Rechner.
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
settings-format-case-keep = Wie eingegeben
settings-format-case-upper = Großbuchstaben
settings-format-case-lower = Kleinbuchstaben
settings-format-commas-trailing = Am Zeilenende
settings-format-commas-leading = Am Zeilenanfang
settings-format-gap-one = Eine Leerzeile
settings-format-gap-none = Keine Leerzeile
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
timeout-off = Kein Timeout
timeout-hours = { $n ->
    [one] { $n } Stunde
   *[other] { $n } Stunden
}
timeout-minutes = { $n ->
    [one] { $n } Minute
   *[other] { $n } Minuten
}
timeout-seconds = { $n ->
    [one] { $n } Sekunde
   *[other] { $n } Sekunden
}
settings-query-confirm-writes = Vor schreibenden Anweisungen nachfragen
settings-query-confirm-writes-hint = Vor jeder Anweisung nachfragen, die Daten oder das Schema ändert.
settings-query-validate = Live-Prüfung gegen die Datenbank
settings-query-validate-hint = Die Anweisung unter dem Cursor beim Tippen gegen die Datenbank prüfen (ein PREPARE ohne Ausführung), um genaue Fehler anzuzeigen. Kostet bei jeder Pause eine Anfrage an die Datenbank.
settings-query-notify = Benachrichtigen, wenn ein Lauf im Hintergrund endet
notify-query-finished = Abfrage abgeschlossen
notify-query-failed = Abfrage fehlgeschlagen
notify-script-finished = Skript abgeschlossen
notify-script-failed = Skript fehlgeschlagen
notify-export-finished = Export abgeschlossen
notify-export-failed = Export fehlgeschlagen
notify-dump-finished = Dump abgeschlossen
notify-dump-failed = Dump fehlgeschlagen
notify-took = Dauer: { $duration }.
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
tree-truncate-rows = Alle { $rows } Zeilen in { $table } löschen? Das lässt sich nicht rückgängig machen.
tree-truncate-every-row = Jede Zeile in { $table } löschen? Das lässt sich nicht rückgängig machen.
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
activity-state-blocked = Blockiert
activity-state-idle-in-tx = Leerlauf in Txn
activity-state-running = Läuft
activity-state-idle = Leerlauf
activity-count-sessions = { $n ->
    [one] { $figure } Sitzung
   *[other] { $figure } Sitzungen
}
activity-count-running = { $figure } laufend
activity-count-blocked = { $figure } blockiert
activity-count-idle-in-tx = { $figure } Leerlauf in Txn
activity-lock-wait = Sperrwartezeit
activity-lock-wait-sentence = Sitzung { $waiter } ({ $waiter_who }) { $waited ->
        [none] wartet
       *[other] wartet seit { $waited }
    } auf eine Sperre, die Sitzung { $holder } ({ $holder_who }) hält, { $doing ->
        [idle-in-tx] im Leerlauf in einer Transaktion
        [blocked] selbst blockiert
        [running] laufend
       *[idle] im Leerlauf
    }{ $held ->
        [none] {""}
       *[other] {" "}seit { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}{ $others } weitere Sitzung hält ebenfalls eine Sperre, die sie braucht.
       *[other] {" "}{ $others } weitere Sitzungen halten ebenfalls eine Sperre, die sie braucht.
    }
activity-kill-holder = { $id } beenden
activity-terminate-tip = Sitzung { $id } beenden
activity-read-only-tip = Diese Verbindung ist als schreibgeschützt markiert.
activity-lock-holds = hält
activity-lock-waits-for = wartet auf
activity-lock-held-by = gehalten von { $ids }
activity-copy-statement = Anweisung kopieren
activity-cancel-query = Abfrage abbrechen
activity-kill-session = Sitzung beenden
activity-waiting-on = wartet auf eine Sperre von { $ids }
activity-blocks = { $n ->
        [one] blockiert { $n } Sitzung
       *[other] blockiert { $n } Sitzungen
    }
activity-read-only-cancel = Diese Verbindung ist als schreibgeschützt markiert, daher bricht Schemaic darauf keine Abfragen ab. Hebe den Schreibschutz zuerst in den Einstellungen der Verbindung auf.
activity-read-only-terminate = Diese Verbindung ist als schreibgeschützt markiert, daher beendet Schemaic darauf keine Sitzungen. Hebe den Schreibschutz zuerst in den Einstellungen der Verbindung auf.

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
props-rows-estimated = Zeilen (geschätzt)
props-rows-counted = Zeilen (gezählt)
props-rows-not-reported = Zeilen — nicht gemeldet
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
props-index-cardinality = Kardinalität { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } Scan
       *[other] { $shown } Scans
    }
props-index-not-counted = Nutzung nicht gezählt
props-index-unused = seit dem Zurücksetzen der Serverzähler nie verwendet
props-never-analysed = Diese Tabelle wurde nie analysiert, daher hat der Server keine Zeilenschätzung für sie. Führe ANALYZE aus, oder zähle die Zeilen.
props-analysed-at = Geschätzt zum Stand des letzten ANALYZE, { $when }.
props-read-live = Live gelesen — dieser Server hat information_schema_stats_expiry = 0.
props-cached-up-to = Vom Server zwischengespeichert: diese Werte können bis zu { $age } alt sein (information_schema_stats_expiry = { $secs }).
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
copy-table-unnamed = Gib der Kopie einen Namen.
copy-table-onto-itself = Eine Tabelle kann nicht auf sich selbst kopiert werden — gib der Kopie einen anderen Namen.
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

## Schema comparison modal

compare-title = Schemas vergleichen
compare-choose-database = Datenbank wählen
compare-no-connection = Keine Verbindung dieser Engine zum Vergleichen.
compare-will-change = wird geändert
compare-source-of-truth = maßgebliche Quelle
compare-idle = Wähle eine Datenbank zum Vergleichen.
compare-loading = Beide Schemas werden gelesen
compare-select-object = Wähle ein Objekt, um beide Seiten zu sehen.
compare-summary = { $differ } abweichend · { $only_here } nur hier · { $only_there } nur dort · { $same } identisch
compare-summary-uncertain = { $n ->
    [one] { $n } Übereinstimmung
   *[other] { $n } Übereinstimmungen
}, für die der Vergleich nicht bürgen kann
compare-filter-placeholder = Nach Namen filtern
compare-include-identical = Identische einbeziehen
compare-select-all = Alle auswählen
compare-select-none = Keine
compare-hint-held-back = aus dem Plan zurückgehalten; der Hinweis oben sagt, warum
compare-hint-blocked = der Rumpf muss neu gelesen werden, bevor dies angewendet werden kann
compare-hint-uncertain = ein Index, den dieses Modell nur teilweise liest – eine Übereinstimmung hier ist nicht sicher
compare-preview-migration = Migrationsvorschau
compare-selected = { $n ->
        [0] Nichts ausgewählt.
        [one] { $n } Objekt ausgewählt
       *[other] { $n } Objekte ausgewählt
    }
compare-incomparable = { $left } und { $right } lassen sich nicht vergleichen. Typnamen, Standardwerte und Indexformen lassen sich nicht zwischen Engines übertragen, daher wäre jede aus dem Unterschied erzeugte Migration falsch.
compare-group-table = Tabellen
compare-group-view = Sichten
compare-group-trigger = Trigger
compare-kind-enum = Enum
compare-kind-domain = Domäne
compare-kind-xml-schema-collection = XML-Schemaauflistung
compare-kind-alias-type = Aliastyp
compare-kind-sequence = Sequenz
compare-kind-table = Tabelle
compare-kind-view = Sicht
compare-kind-function = Funktion
compare-kind-procedure = Prozedur
compare-kind-trigger = Trigger
compare-kind-event = Ereignis
compare-kind-synonym = Synonym

## Schema comparison modal › why the list is empty

compare-empty-nothing = Keine der beiden Datenbanken enthält etwas zum Vergleichen.
compare-empty-agrees = Diese beiden Schemas stimmen Objekt für Objekt überein.
compare-empty-agrees-noted = Jedes Objekt stimmt überein, aber nicht alles ließ sich vergleichen – siehe die Hinweise oben.
compare-empty-no-match = Nichts passt zu diesem Filter.
compare-empty-only-identical = Alles, was zu diesem Filter passt, ist identisch – schalte „Identische einbeziehen“ ein, um es zu sehen.

## Schema editors — shared (continued)

editor-comment-placeholder = wozu es dient

## Event editor

event-editor-title-edit = Ereignis { $event } bearbeiten
event-editor-title-create = Ereignis in { $location } erstellen
event-editor-section-event = Ereignis
event-editor-section-schedule = Zeitplan
event-editor-section-body = Rumpf
event-editor-section-options = Optionen
event-editor-name = Name
event-editor-body = Rumpf
event-editor-runs = Läuft
event-editor-runs-repeating = Wiederholt
event-editor-runs-once = Einmal
event-editor-at = Am
event-editor-every = Alle
event-editor-starts = Beginnt
event-editor-ends = Endet
event-editor-status = Status
event-editor-status-enabled = Aktiviert
event-editor-status-disabled = Deaktiviert
event-editor-status-replica = Auf dem Replikat deaktiviert
event-editor-preserve = Nach Abschluss behalten
event-editor-preserve-hint = ON COMPLETION PRESERVE. Aus ist MySQLs Vorgabe und bedeutet, dass der Server das Ereignis löscht, sobald sein letzter Lauf vorbei ist – bei einem einmaligen also gleich nach dem Lauf.
event-editor-definer = Definer
event-editor-comment = Kommentar
event-editor-reading-source = Quelltext des Ereignisses wird gelesen…
event-editor-source-stale = Der echte Quelltext des Ereignisses kam erst an, nachdem du zu tippen begonnen hattest, daher beruht dieser Rumpf auf der Kopie des Katalogs – die nicht dem entspricht, was der Server hält. Schließe und öffne erneut, um ihn zu bearbeiten.

## Routine editor

routine-editor-title-edit-function = Funktion { $routine } bearbeiten
routine-editor-title-edit-procedure = Prozedur { $routine } bearbeiten
routine-editor-title-create-function = Funktion in { $location } erstellen
routine-editor-title-create-procedure = Prozedur in { $location } erstellen
routine-editor-section-function = Funktion
routine-editor-section-procedure = Prozedur
routine-editor-section-body = Rumpf
routine-editor-section-options = Optionen
routine-editor-name = Name
routine-editor-parameters = Parameter
routine-editor-returns = Rückgabe
routine-editor-returns-placeholder-mssql = int oder TABLE
routine-editor-language = Sprache
routine-editor-body = Rumpf
routine-editor-recompile = Neu kompilieren
routine-editor-recompile-hint = WITH RECOMPILE – bei jedem Aufruf ein frischer Plan, nie ein zwischengespeicherter.
routine-editor-schema-binding = Schemabindung
routine-editor-schema-binding-hint = WITH SCHEMABINDING – die Tabellen und Sichten, die sie liest, können nicht unter ihr geändert werden.
routine-editor-execute-as = Ausführen als
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER oder ein Benutzer
routine-editor-null-input = Gibt bei NULL-Eingabe NULL zurück
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT – der Rumpf läuft nicht, wenn ein Argument NULL ist.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatisch
routine-editor-comment = Kommentar
routine-editor-volatility = Volatilität
routine-editor-strict = Strikt
routine-editor-security-definer = Security Definer
routine-editor-security-definer-hint-pg = Läuft mit den Rechten des Eigentümers statt des Aufrufers. Lege unten einen search_path fest, wenn du das nutzt.
routine-editor-security-definer-hint-mysql = Läuft mit den Rechten des Definers statt des Aufrufers. Das ist MySQLs Vorgabe – ausschalten bedeutet SQL SECURITY INVOKER.
routine-editor-settings = Einstellungen
routine-editor-add-setting = Einstellung hinzufügen
routine-editor-deterministic = Deterministisch
routine-editor-deterministic-hint = Verspricht für dieselben Argumente dasselbe Ergebnis. Ein Server mit Binärprotokoll lehnt eine nicht deterministische Routine ab, sofern er ihren Erstellern nicht vertraut.
routine-editor-data-access = Datenzugriff
routine-editor-definer = Definer
routine-editor-reading-source = Quelltext der Routine wird gelesen…
routine-editor-source-stale = Der echte Quelltext der Routine kam erst an, nachdem du zu tippen begonnen hattest, daher beruht dieser Rumpf auf der Kopie des Katalogs – die nicht dem entspricht, was der Server hält. Schließe und öffne erneut, um ihn zu bearbeiten.

## Users and privileges browser

users-search-placeholder = Konten durchsuchen
users-loading-accounts = Konten werden geladen
users-unsupported = Die Engine dieser Verbindung hat keine Benutzerkonten.
users-none = Dieser Server meldet keine Konten.
users-no-match = Kein Konto passt zu „{ $needle }“.
users-pick-account = Wähle ein Konto, um zu sehen, was es darf.
users-section-attributes = Attribute
users-section-privileges = Rechte
users-kind-user = Benutzer
users-kind-role = Rolle
users-kind-login = Anmeldung
users-maintained-by-server = vom Server verwaltet
users-loading-privileges = Rechte werden geladen
users-no-privileges = Dieses Konto hat keine Rechte.
users-statements-capped = Die ersten { $shown } von { $total } Anweisungen werden angezeigt. „Rechte kopieren“ nimmt alle.
users-new-account = Neues Konto
users-system-account = Dieses Konto gehört dem Server, daher ändert Schemaic es nicht.
users-reset-password = Passwort zurücksetzen
users-gate-no-database = Öffne einen Abfrage-Tab auf einer Datenbank, um Rechte zu ändern – die Anweisung muss in einer laufen.
users-gate-read-only = Diese Verbindung ist schreibgeschützt.
users-count = { $n ->
    [one] { $n } Konto
   *[other] { $n } Konten
}
users-count-filtered = { $shown } von { $n } Konten
users-copy-privileges = Rechte kopieren

## Schema editors — shared (continued)

editor-row-remove = Entfernen

## Trigger editor

trigger-editor-title = Trigger auf { $table }
trigger-editor-preview-subject = Trigger auf { $table }
trigger-editor-none-selected = Kein Trigger ausgewählt.
trigger-editor-readonly-constraint = Dies ist ein Constraint-Trigger. Schemaic bildet dessen Verzögerungseinstellungen nicht ab, daher kann er hier entfernt, aber nicht bearbeitet werden – ändere ihn stattdessen in SQL.
trigger-editor-readonly-hidden = Die Definition dieses Triggers ist verschlüsselt oder für diese Anmeldung verborgen, daher kann er hier entfernt, aber nicht bearbeitet werden.
trigger-editor-readonly-unmodelled = Dieser Trigger nutzt eine Option, die Schemaic nicht abbildet, daher kann er hier entfernt, aber nicht bearbeitet werden – ändere ihn stattdessen in SQL.
trigger-editor-section-trigger = Trigger
trigger-editor-section-condition = Bedingung
trigger-editor-section-action = Aktion
trigger-editor-name = Name
trigger-editor-timing = Zeitpunkt
trigger-editor-event = Ereignis
trigger-editor-fires = Feuert
trigger-editor-truncate-hint = Nur auf Anweisungsebene.
trigger-editor-not-for-replication-hint = Schweigt, während ein Replikationsagent die Tabelle schreibt.
trigger-editor-rank-any = Beliebige Reihenfolge
trigger-editor-rank-first = Feuert zuerst
trigger-editor-rank-last = Feuert zuletzt
trigger-editor-when = Wenn
trigger-editor-of-columns = Von Spalten
trigger-editor-of-columns-placeholder = optional – jede Spalte, wenn leer
trigger-editor-body = Rumpf
trigger-editor-function = Funktion
trigger-editor-edit-function = Bearbeiten
trigger-editor-new-function = Neue Funktion
trigger-editor-arguments = Argumente
trigger-editor-add-argument = Argument hinzufügen

## Schema editors — shared (continued)

editor-row-move-up = Nach oben
editor-row-move-down = Nach unten

## Object editor

object-editor-title-edit = { $kind } { $name } bearbeiten
object-editor-title-create = { $kind } in { $location } erstellen
object-editor-name = Name
object-editor-comment = Kommentar
object-editor-type = Typ
object-editor-default = Standardwert
object-editor-section-type = Typ
object-editor-section-values = Werte
object-editor-section-domain = Domäne
object-editor-section-constraints = Constraints
object-editor-section-sequence = Sequenz
object-editor-section-position = Position
object-editor-section-synonym = Synonym
object-editor-section-alias-type = Aliastyp
object-editor-section-xml-schema-collection = XML-Schemaauflistung
object-editor-section-stands-for = Steht für
object-editor-value-placeholder = Wert
object-editor-add-value = Wert hinzufügen
object-editor-values-order = In Vergleichsreihenfolge – diese Reihenfolge verwendet ORDER BY
object-editor-add-constraint = Constraint hinzufügen
object-editor-no-base-types = Diese Engine schlägt keine Basistypen vor
object-editor-not-null-hint = Jede Spalte dieser Domäne lehnt NULL ab. Das Einschalten schlägt fehl, wenn eine bereits NULL enthält.
object-editor-stores = Speichert
object-editor-increment = Schrittweite
object-editor-start = Start
object-editor-cache = Cache
object-editor-minimum = Minimum
object-editor-maximum = Maximum
object-editor-not-whole-number = { $field } muss eine ganze Zahl sein.
object-editor-cycle = Zyklisch
object-editor-cycle-hint = Am anderen Ende des Bereichs neu beginnen, statt fehlzuschlagen, wenn die Sequenz erschöpft ist.
object-editor-restart-at = Neu starten bei – der Zähler hat zuletzt { $last } ausgegeben
object-editor-restart-at-unused = Neu starten bei – der Zähler wurde noch nicht verwendet
object-editor-restart-not-whole = „Neu starten bei“ muss eine ganze Zahl sein.
object-editor-restart-placeholder = leer lassen, um die Position zu behalten
object-editor-tsql-restart-placeholder = leer behält sie, außer der Typ ändert sich
object-editor-owned-by = Gehört zu
object-editor-detach = Von der Spalte lösen
object-editor-detach-hint = Die Sequenz wird nicht mehr mit der Spalte entfernt und überdauert sie.
object-editor-cache-hint = Werte aus dem Speicher ausgeben – schneller, um den Preis einer Lücke nach einem Neustart des Servers.
object-editor-cache-size = Cache-Größe
object-editor-cache-size-placeholder = Serverstandard
object-editor-synonym-server = Server
object-editor-synonym-server-placeholder = dieser Server
object-editor-synonym-database = Datenbank
object-editor-synonym-database-placeholder = diese Datenbank
object-editor-synonym-schema = Schema
object-editor-synonym-schema-placeholder = das Standardschema
object-editor-synonym-object = Objekt
object-editor-synonym-object-placeholder = Tabelle, Sicht oder Routine
object-editor-base-type = Basistyp
object-editor-nullable = Nullable
object-editor-nullable-hint = Eine Spalte dieses Typs akzeptiert NULL, sofern die Spalte nichts anderes angibt.
object-editor-schemas = Schemas

## Account and grant editors

account-editor-title-create = Konto erstellen
account-editor-title-reset = Passwort zurücksetzen
account-editor-title-privileges = Berechtigungen – { $account }
account-editor-account = Konto
account-editor-kind = Art
account-editor-name = Name
account-editor-host = Host
account-editor-host-placeholder = % (jeder Host)
account-editor-for-login = Für Anmeldung
account-editor-for-login-placeholder = Anmeldung – leer für einen Benutzer ohne
account-editor-also-user = Auch Benutzer in { $database }
account-editor-password = Passwort
account-editor-confirm = Bestätigen
account-editor-action = Aktion
account-editor-action-grant = Gewähren
account-editor-action-revoke = Entziehen
account-editor-subject = Gegenstand
account-editor-subject-privileges = Berechtigungen
account-editor-subject-role = Rolle
account-editor-role = Rolle
account-editor-no-roles = Dieser Server hat keine Rollen
account-editor-with-admin-option = Mit Admin-Option
account-editor-level = Ebene
account-editor-level-global = Ganzer Server
account-editor-level-database = Datenbank
account-editor-level-schema = Schema
account-editor-level-table = Tabelle
account-editor-level-sequence = Sequenz
account-editor-no-suggestions = Keine Vorschläge
account-editor-section-privileges = Berechtigungen
account-editor-with-grant-option = Mit Grant-Option
account-editor-needs-role = Nenne eine Rolle.
account-editor-needs-privileges = Wähle eine Ebene, nenne, worauf sie sich bezieht, und markiere eine Berechtigung.
account-editor-password-required = Ein Passwort ist erforderlich.
account-editor-name-required = Ein Name ist erforderlich.
account-editor-entra-only-user = Aus Microsoft Entra wird nur ein Datenbankbenutzer erstellt.
account-editor-entra-no-password = Ein Entra-Benutzer hat hier kein Login und kein Passwort — Entra meldet ihn an.
account-editor-login-needs-password = Ein Login braucht ein Passwort.
account-editor-user-for-login-password = Ein Benutzer für ein Login meldet sich mit dem Passwort des Logins an — lass dies leer.
account-editor-confirm-password = Gib das Passwort zur Bestätigung erneut ein.
account-editor-passwords-differ = Die beiden Passwörter stimmen nicht überein.
account-editor-password-hashed = Das Vorschau-SQL, also die Anweisung, die ausgeführt wird, enthält einen Hash des Passworts in der Form, die dieser Server speichert, nicht das Passwort selbst — es sei denn, es enthält Zeichen, die die Anmeldung normalisieren würde (Vollbreitenbuchstaben, Ligaturen, geschützte Leerzeichen), oder der Server verwendet eine Passwortprüfungserweiterung; dann wird es wie eingegeben gesendet.
account-editor-password-as-typed = Das Passwort erscheint im Vorschau-SQL, also in der Anweisung, die ausgeführt wird.

## Table designer

designer-title-edit = { $table } bearbeiten
designer-title-create = Tabelle in { $location } erstellen
designer-tab-table = Tabelle
designer-tab-columns = Spalten
designer-tab-indexes = Indizes
designer-tab-foreign-keys = Fremdschlüssel
designer-tab-checks = Checks
designer-list-add = Hinzufügen
designer-no-suggestions = Keine Vorschläge
designer-name = Name
designer-comment = Kommentar
designer-table-comment-placeholder = Wofür diese Tabelle da ist
designer-engine = Engine
designer-collation = Sortierung
designer-no-column = Keine Spalte ausgewählt.
designer-type = Typ
designer-type-hint = Wird unverändert übernommen – der Server entscheidet, was er bedeutet.
designer-nullable = Nullable
designer-nullable-hint = NULL in dieser Spalte zulassen.
designer-primary-key = Primärschlüssel
designer-primary-key-hint = Teil des Primärschlüssels der Tabelle, in Spaltenreihenfolge.
designer-identity = Identity
designer-auto-increment = Auto-Inkrement
designer-identity-hint = Der Server vergibt den Wert ({ $keyword }).
designer-default = Standardwert
designer-default-hint = SQL-Text, wie du ihn schreiben würdest: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Berechnet aus
designer-generated-hint = Ein Ausdruck, z. B. qty * price. Eine berechnete Spalte hat keinen Standardwert.
designer-on-update = Bei Aktualisierung
designer-index-unique-detail = eindeutig
designer-index-redundant-detail = redundant
designer-hint-redundant = Index { $index } wird von { $cover } abgedeckt: Jede Suche, die er bedient, bedient auch dieser.
designer-hint-unindexed-fk = Fremdschlüssel { $fk } hat keinen Index, der mit seinen Spalten beginnt, daher durchsucht ein Löschen oder Schlüssel-Update in der referenzierten Tabelle diese hier vollständig.
designer-no-index = Kein Index ausgewählt. Der Primärschlüssel liegt bei den Spalten.
designer-columns = Spalten
designer-index-columns-hint-single = Die eine xml-, geometry- oder geography-Spalte, die er indiziert.
designer-index-columns-hint-columnstore = Kommagetrennte Spalten, die er speichert.
designer-index-columns-hint-prefix = Kommagetrennt, in Schlüsselreihenfolge. bio(20) ist eine Präfixlänge; DESC sortiert absteigend.
designer-index-columns-hint = Kommagetrennt, in Schlüsselreihenfolge. DESC für eine absteigende Spalte.
designer-index-method = Methode
designer-index-method-hint = Leer lassen für den Standard (btree).
designer-index-filter = Nur Zeilen, wo
designer-index-filter-hint = Die Bedingung eines partiellen Index, ohne das WHERE.
designer-index-storage = Speicherung
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Gruppierter Columnstore
designer-storage-primary-xml = Primärer XML
designer-storage-selective-xml = Selektiver XML
designer-storage-secondary-xml = Sekundärer XML
designer-storage-spatial = Räumlich
designer-index-using = Using
designer-index-using-hint = Die Klausel nach der Spalte: USING XML INDEX … FOR PATH, VALUE oder PROPERTY eines sekundären XML-Index (oder FOR (path) bei einem selektiven), [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …) eines selektiven XML-Index oder USING … WITH (…) eines räumlichen Index.
designer-index-order = Reihenfolge
designer-index-order-hint = Kommagetrennte Spalten, nach denen die Segmente sortiert werden – ab SQL Server 2022, und ab 2025 für einen nicht gruppierten Index, der nur nach Spalten sortiert, die er speichert.
designer-index-include = Einschließen
designer-index-include-hint = Kommagetrennte Spalten, die im Index mitgeführt werden, aber nicht Teil seines Schlüssels sind.
designer-index-unique = Eindeutig
designer-index-unique-hint = Doppelte Werte über diese Spalten ablehnen.
designer-fk-detail-unindexed = → { $table } · kein Index
designer-no-fk = Kein Fremdschlüssel ausgewählt.
designer-fk-ref-table = Referenziert Tabelle
designer-fk-on-delete = Beim Löschen
designer-fk-columns-hint = Kommagetrennt, der Reihe nach den referenzierten Spalten zugeordnet.
designer-fk-ref-columns = Referenziert Spalten
designer-no-check = Kein Check ausgewählt.
designer-check-expression = Ausdruck
designer-check-expression-hint = Die Bedingung, die jede Zeile erfüllen muss. Ohne CHECK (…) geschrieben.
designer-check-enforced = Erzwungen
designer-check-enforced-hint = Aus hält den Constraint fest, ohne ihn anzuwenden – bestehende und neue Zeilen werden beide angenommen.
connections-title = Verbindungen verwalten
connections-new = Neue Verbindung
connections-import = Verbindungen importieren
connections-export = Verbindungen exportieren
connections-duplicate = Duplizieren
connections-delete = Löschen
connections-test = Testen
connections-save = Speichern
connections-test-failed = Verbindung fehlgeschlagen
connections-browse = Durchsuchen…
connections-section-general = Allgemein
connections-section-safeguards = Schutzmaßnahmen
connections-section-database = Datenbank
connections-section-network = Netzwerk
connections-name = Name
connections-colour = Farbe
connections-folder = Ordner
connections-no-folders = Noch keine Ordner
connections-folder-hint = Optional. Verbindungen mit demselben Ordner werden zusammen aufgeführt.
connections-prominent-colour = Auffällige Farbe im Editor
connections-prominent-colour-hint = Den Abfrageeditor in der Farbe dieser Verbindung einrahmen.
connections-read-only = Schreibgeschützt
connections-read-only-hint = Zellenbearbeitung deaktivieren und schreibende/DDL-Abfragen auf dieser Verbindung blockieren.
connections-read-only-mssql = SQL Server hat keine schreibgeschützte Sitzung, daher laufen Lesevorgänge in einer Transaktion, die Schemaic zurückrollt. Die Wirkung einer Prozedur außerhalb der Datenbank übersteht das; für eine Garantie verbinde dich mit einem Login, das nur lesen kann.
connections-cli-access = CLI-Zugriff
connections-cli-access-hint = Der Kommandozeile schemaic und KI-Agenten über schemaic mcp erlauben, diese Verbindung zu nutzen.
connections-ai-data-access = KI-Datenzugriff
connections-statement-timeout = Zeitlimit für Anweisungen
connections-statement-timeout-hint = Bricht eine Anweisung auf dieser Verbindung ab, die länger läuft.
connections-statement-timeout-same = Wie in der App
connections-environment = Umgebung
env-none = Keine
env-local = Lokal
env-development = Entwicklung
env-testing = Test
env-staging = Staging
env-production = Produktion
connections-type = Typ
connections-init-sql = Init-SQL
connections-init-sql-pragma-hint = Optional. PRAGMA-Einstellungen, die bei jeder Verbindung laufen, die Schemaic öffnet.
connections-init-sql-set-hint = Optional. SET-Anweisungen, die bei jeder Verbindung laufen, die Schemaic öffnet – ein Suchpfad, eine Zeitzone, eine Rolle.
connections-host = Host
connections-port = Port
connections-named-instance-port = Eine benannte Instanz: SQL Server Browser liefert ihren Port, daher wird Port nicht verwendet. Um den Browser zu umgehen, gib den Host ohne die Instanz und den Port an, auf dem sie lauscht.
connections-database = Datenbank
connections-database-hint = Optional, und die Datenbank, in der die Verbindung geöffnet wird. Manche Hosting-Anbieter erlauben nur ihre eigene und lehnen eine Verbindung ab, die sie nicht nennt.
connections-authentication = Authentifizierung
ssh-auth-password = Passwort
ssh-auth-key-pair = Schlüsselpaar
ssh-auth-agent = SSH-Agent
auth-password = Benutzer und Passwort
auth-windows = Windows (dieses Konto)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Benutzer
connections-password = Passwort
connections-azure-cli-hint = Meldet sich als die Person an, die in der Azure CLI angemeldet ist (vorher `az login` ausführen). Das Token wird abgerufen, wenn eine Verbindung eines braucht, und nie gespeichert.
connections-aws-iam-hint = Meldet sich als der unten angegebene Benutzer mit einem Token aus den Anmeldedaten der AWS CLI an (vorher `aws configure` oder `aws sso login`). Der Benutzer braucht die Rolle rds_iam (PostgreSQL) oder das AWSAuthenticationPlugin (MySQL). Das Token wird abgerufen, wenn eine Verbindung eines braucht, und nie gespeichert.
connections-gcp-iam-hint = Meldet sich als der unten angegebene Benutzer an – der IAM-Benutzer oder das Dienstkonto, das Cloud SQL kennt – mit einem Token aus der Google Cloud CLI (vorher `gcloud auth login`). Das Token wird abgerufen, wenn eine Verbindung eines braucht, und nie gespeichert.
connections-windows-hint-named = Meldet sich mit dem Windows-Konto an, unter dem Schemaic läuft ({ $account }). Der Server muss dieses Konto als Windows-Anmeldung kennen.
connections-windows-hint = Meldet sich mit dem Windows-Konto an, unter dem Schemaic läuft. Der Server muss dieses Konto als Windows-Anmeldung kennen.
connections-database-file = Datenbankdatei
connections-database-file-dialog = SQLite-Datenbank auswählen
connections-sqlite-hint = Eine SQLite-Datenbank ist eine einzelne Datei, es gibt also keinen Server zu erreichen – keinen Host, keinen Benutzer, kein Passwort und keinen Tunnel. Manuelle Transaktionen gibt es dort noch nicht: Anweisungen werden beim Senden festgeschrieben.
connections-ssh-tunnel = SSH-Tunnel
connections-ssh-tunnel-hint = Den Server über einen SSH-Tunnel erreichen.
connections-ssh-host = SSH-Host
connections-ssh-port = SSH-Port
connections-ssh-user = SSH-Benutzer
connections-ssh-password = SSH-Passwort
connections-ssh-agent-hint = Das Signieren übernimmt dein laufender SSH-Agent (OpenSSH-Agent / Pageant unter Windows). Schemaic speichert keinen Schlüssel.
connections-jump-hosts = Jump-Hosts
connections-jump-hosts-hint = Optional. Hosts, die vor dem SSH-Host durchlaufen werden, der nächste zuerst – jeder meldet sich wie unten eingestellt an.
connections-private-key = Privater Schlüssel
connections-private-key-dialog = Privaten Schlüssel auswählen
connections-passphrase = Passphrase
connections-tls = SSL / TLS
tls-disable = Deaktiviert
tls-prefer = Bevorzugen
tls-require = Erzwingen
tls-verify-ca = CA prüfen
tls-verify-full = Vollständig prüfen
tls-disable-description = Unverschlüsselt verbinden.
tls-prefer-description = Verschlüsseln, wenn der Server es anbietet. Keine Prüfung.
tls-require-description = Ohne Verschlüsselung nicht verbinden. Keine Prüfung.
tls-verify-ca-description = Verschlüsseln und das Zertifikat gegen die CA prüfen.
tls-verify-full-description = Verschlüsseln, das Zertifikat prüfen und den Hostnamen prüfen.
tls-verify-ca-checks-host = Unter MySQL und MariaDB prüft dies auch den Hostnamen und verhält sich daher wie „Vollständig prüfen“ — eine Einschränkung des Treibers, keine Einstellung.
tls-floor-azure-cli = Die Anmeldung mit Microsoft Entra verbindet mit „Vollständig prüfen“, egal was hier gewählt ist: Ihr Token öffnet jede Azure-SQL-Datenbank, die das Konto erreichen kann.
tls-floor-aws-iam = Die Anmeldung mit AWS IAM verbindet mindestens mit „Erzwingen“: Ihr Token darf das Netz nicht unverschlüsselt durchqueren. „Vollständig prüfen“ braucht das RDS-CA-Bundle als Root-CA.
tls-floor-gcp-iam = Die Anmeldung mit Google Cloud IAM verbindet mindestens mit „CA prüfen“ — setze Root-CA auf die Server-CA der Instanz: Ihr Token öffnet jede Cloud-SQL-Instanz, die das Konto erreichen kann.
tls-floor-other = Diese Anmeldung verbindet mit einem stärkeren Modus als dem hier gewählten.
connections-ca-certificate = CA-Zertifikat
connections-ca-certificate-dialog = CA-Zertifikat auswählen
connections-ca-certificate-hint = Leer lassen, um den Zertifizierungsstellen zu vertrauen, denen dein Betriebssystem vertraut, einschließlich der von deiner Organisation installierten. Gib eine Datei an, um stattdessen dieser zu vertrauen.
connections-client-certificate = Client-Zertifikat
connections-client-certificate-dialog = Client-Zertifikat auswählen
connections-client-key = Client-Schlüssel
connections-client-key-dialog = Client-Schlüssel auswählen
connections-client-identity-hint = Ein Client-Zertifikat und ein Schlüssel werden nur gebraucht, wo der Server verlangt, dass du dich ausweist (gegenseitiges TLS). Der Schlüssel darf nicht durch eine Passphrase geschützt sein.
import-title = Import in { $table }
import-section-source = Quelle
import-section-reading = Einlesen
import-section-columns = Spalten
import-section-preview = Vorschau
import-file = Datei
import-choose-file = Datei auswählen…
import-file-dialog = In Tabelle importieren
import-no-file = Keine Datei ausgewählt
import-unnamed-file = (Datei)
import-format = Format
import-sheet = Tabellenblatt
import-delimiter = Trennzeichen
import-header-row = Erste Zeile ist eine Kopfzeile
import-header-row-hint = Die erste Zeile als Spaltennamen statt als Daten verwenden.
import-trim = Leerraum entfernen
import-trim-hint = Leerzeichen um jeden Wert entfernen – auch innerhalb von Anführungszeichen.
import-empty-is-null = Leeres Feld ist NULL
import-empty-is-null-hint = Ein leeres Feld bedeutet NULL statt einer leeren Zeichenkette.
import-null-tokens = Weitere Werte, die NULL bedeuten
import-null-tokens-hint = Kommagetrennt, z. B. NULL, \N, NA.
import-skip = Überspringen
import-nothing-mapped = Nichts ist zugeordnet, also gibt es nichts zu importieren.
import-values-withheld = Der erste Datensatz dieser Datei ist zu groß für die Vorschau, daher wurden nur die Spaltennamen gelesen. Zuordnung und Import lesen die ganze Datei.
import-missing-required = Nicht zugeordnet und dürfen nicht leer sein: { $columns }. Der Import schlägt fehl, sofern die Datenbank sie nicht selbst füllt.
import-engine-not-transactional = Die Speicher-Engine dieser Tabelle ({ $engine }) ist nicht transaktional, ein fehlgeschlagener Import lässt sich also nicht rückgängig machen – die vor dem Fehler geladenen Zeilen bleiben erhalten.
import-problems = { $count ->
    [one] { $count } Problem in der Datei – nichts wurde importiert.
   *[other] { $count } Probleme in der Datei – nichts wurde importiert.
}
import-problem-at-line = Zeile { $line }: { $message }
import-problem-at-column = Zeile { $line }, { $column }: { $message }
import-issue-not-an-integer = keine ganze Zahl
import-issue-not-a-number = keine Zahl
import-issue-not-a-boolean = kein Wahr/Falsch-Wert
import-issue-cell-error = das Tabellenblatt konnte diese Zelle nicht auswerten
import-issue-null-in-not-null = leer, aber die Spalte darf nicht NULL sein
import-issue-field-count = hat { $found } Felder, erwartet { $expected }
import-tail-cut = { $shown } von { $total } werden angezeigt.
import-tail-uncounted = Alle { $total } werden angezeigt — die Prüfung hat dort aufgehört zu zählen, die Datei enthält also mehr.
import-tail-cut-and-uncounted = { $shown } der ersten { $total } werden angezeigt — die Prüfung hat dort aufgehört zu zählen, die Datei enthält also mehr.
import-memory-json = Diese JSON-Datei ist { $size } groß, und ein JSON-Import wird während des Ladens im Speicher gehalten — rechne mit etwa { $estimate }. Eine CSV mit denselben Daten lädt mit konstantem Speicherbedarf.
import-memory-xlsx = Diese Arbeitsmappe ist { $size } groß, und ein Excel-Import wird während des Ladens im Speicher gehalten — rechne mit ungefähr { $estimate }, da die Datei komprimiert ist. Eine CSV desselben Blatts lädt mit konstantem Speicherbedarf.
import-read-failed = Die Datei konnte nicht gelesen werden: { $error }
import-no-columns-mapped = Keine Dateispalte ist einer Tabellenspalte zugeordnet
import-preview-record-too-large = ihr erster JSON-Datensatz ist größer als die { $mib } MiB, die die Vorschau liest, daher gibt es nichts anzuzeigen
import-refuse-not-objects = JSON-Objekte erwartet (ein Array davon oder eines pro Zeile)
import-refuse-oversize = Diese Arbeitsmappe ist { $size } groß, und eine Excel-Datei muss vollständig gelesen werden, bevor etwas davon angezeigt werden kann — { $max } ist das Maximum, das hier geöffnet wird. Exportiere das Blatt als CSV und importiere stattdessen diese; eine CSV lädt mit konstantem Speicherbedarf.
import-refuse-oversize-unsized = Diese Arbeitsmappe ist größer als { $max }, und eine Excel-Datei muss vollständig gelesen werden, bevor etwas davon angezeigt werden kann — { $max } ist das Maximum, das hier geöffnet wird. Exportiere das Blatt als CSV und importiere stattdessen diese; eine CSV lädt mit konstantem Speicherbedarf.
import-refuse-inflated = Diese Arbeitsmappe ist auf der Festplatte { $size } groß, entpackt aber mindestens { $unpacked }, und eine Excel-Datei muss vollständig gelesen werden, bevor etwas davon angezeigt werden kann — { $ceiling } entpackter Inhalt ist das Maximum, das hier geöffnet wird, und es braucht mehr Speicher als das, um ihn zu halten. Exportiere das Blatt als CSV und importiere stattdessen diese; eine CSV lädt mit konstantem Speicherbedarf.
import-refuse-no-such-sheet = diese Arbeitsmappe hat kein Blatt namens „{ $sheet }“ — sie hat { $sheets }
import-refuse-no-sheets = diese Arbeitsmappe hat keine Blätter
import-refuse-too-wide = dieses Blatt gibt { $width } Spalten an; ein Excel-Arbeitsblatt fasst { $max }. Exportiere es als CSV und importiere stattdessen diese.
import-cancelled = Import abgebrochen – die Transaktion wurde zurückgesetzt, also wurde nichts geschrieben.
import-done = { $count ->
    [one] { $count } Zeile in { $table } importiert.
   *[other] { $count } Zeilen in { $table } importiert.
}
import-next = Weiter
import-reading = Wird gelesen…
import-back = Zurück
import-import = Importieren
import-importing = Wird importiert…
dialog-read-only = Diese Verbindung ist schreibgeschützt.
dump-title-schema = { $database }.{ $schema } als { $format } exportieren
dump-title = { $database } als { $format } exportieren
dump-sql-dialog = Als SQL exportieren
dump-folder-dialog = Als { $format } exportieren
dump-folder-hint = Eine { $format }-Datei pro Tabelle, geschrieben in einen Ordner deiner Wahl.
dump-all-selected = Alle { $total } ausgewählt
dump-some-selected = { $chosen } von { $total } ausgewählt
dump-select-all = Alle auswählen
dump-select-none = Keine
dump-reading-tables = Tabellenliste wird gelesen…
dump-no-tables = Diese Datenbank hat keine Tabellen.
dump-tables-unreadable = Die Tabellenliste konnte nicht gelesen werden.
dump-section-contents = Inhalt
dump-structure = Struktur
dump-structure-hint = CREATE TABLE, seine Trigger und die Fremdschlüssel, nach den Daten erneut angegeben.
dump-data = Daten
dump-data-hint = Jede Zeile, als INSERT-Anweisungen.
dump-other-objects = Weitere Objekte
dump-other-objects-hint = Typen, Sequenzen, Routinen und Events, auf die sich diese Tabellen stützen.
dump-section-replaying = Einspielen
dump-drop-before-create = Vor dem Anlegen löschen
dump-drop-hint-up-front = Jede Tabelle und Sicht vor ihrem CREATE löschen, damit die Datei in eine Datenbank geladen werden kann, die sie bereits enthält. Mit „Eine Transaktion“ ersetzt sie außerdem jede Routine in deren Schemas und bricht ab, bevor sie etwas löscht, das sie nicht unverändert zurücksetzen kann. Ohne „Eine Transaktion“ lässt sie Routinen unverändert, sodass ein erneutes Einspielen in eine Datenbank, die sie noch enthält, bei der ersten abbricht. Eigentümer und Berechtigungen jeder Tabelle werden am Ende wiederhergestellt.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS vor jedem CREATE, damit die Datei in eine Datenbank geladen werden kann, die diese Tabellen bereits enthält. Sie ersetzt außerdem jede Routine in deren Schemas. Eigentümer, Rechte und Zeilensicherheit jeder Tabelle werden am Ende wiederhergestellt.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS vor jedem CREATE, damit die Datei in eine Datenbank geladen werden kann, die diese Tabellen bereits enthält. Sie ersetzt außerdem jede Routine und jedes Ereignis in deren Schemas: Unter MySQL wird jede zuerst gelöscht, was alle Rechte darauf entzieht, und die Datei vergibt sie an ihrem Ende wieder, sofern dein Konto mysql.procs_priv lesen kann.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS vor jedem CREATE, damit die Datei in eine Datenbank geladen werden kann, die diese Tabellen bereits enthält.
dump-one-transaction = Eine Transaktion
dump-one-transaction-hint = Das Laden umschließen, damit ein Fehler nichts zurücklässt.
dump-disable-fk-checks = Fremdschlüsselprüfungen deaktivieren
dump-fk-hint = Für die Dauer des Ladens aus, damit die Reihenfolge, in der Zeilen ankommen, sie nicht abweisen kann.
dump-fk-hint-drops = Für die Dauer des Ladens aus, damit die Reihenfolge, in der Zeilen ankommen, sie nicht abweisen kann. Eine Datei, die Tabellen löscht, schaltet sie ohnehin aus: Ein Löschen bei eingeschalteten Prüfungen löscht über ihre ON-DELETE-Aktionen die Zeilen von Tabellen außerhalb der Datei.
dump-no-fk-guard-hint = Diese Engine hat keinen Sitzungsschalter für Fremdschlüsselprüfungen; die Datei fügt die Constraints stattdessen nach den Daten hinzu.
dump-export = Exportieren
dump-progress = Schreibe { $table } – { $index } von { $total }, bisher { $rows } Zeilen
dump-reading-schema = Schema wird gelesen
dump-replace-files = Dateien ersetzen
dump-replace-prompt = { $n ->
    [one] { $n } Datei in { $folder } wird ersetzt und kann nicht wiederhergestellt werden: { $names }.
   *[other] { $n } Dateien in { $folder } werden ersetzt und können nicht wiederhergestellt werden: { $names }.
}
dump-replace-prompt-more = { $n } Dateien in { $folder } werden ersetzt und können nicht wiederhergestellt werden: { $names } und { $more } weitere.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] { $n } Tabelle geschrieben.
   *[other] { $n } Tabellen geschrieben.
}
dump-done-missing = { $n ->
    [one] { $n } ausgewählte Tabelle wurde nicht gefunden und ist nicht in der Datei: { $names }.
   *[other] { $n } ausgewählte Tabellen wurden nicht gefunden und sind nicht in der Datei: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } angefordertes Objekt ist nicht in der Datei: { $names }.
   *[other] { $n } angeforderte Objekte sind nicht in der Datei: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Die Zeilen der Graph-Kantentabelle { $names } sind nicht in der Datei; sie wird leer angelegt.
   *[other] Die Zeilen der Graph-Kantentabellen { $names } sind nicht in der Datei; sie werden leer angelegt.
}
dump-done-refused = { $n ->
    [one] Ein erneutes Einspielen in eine Datenbank, die dies bereits enthält, bricht ab, bevor es etwas löscht: Die Datei kann es nicht so zurücksetzen, wie es ist: { $names }.
   *[other] Ein erneutes Einspielen in eine Datenbank, die eines davon bereits enthält, bricht ab, bevor es etwas löscht: Die Datei kann sie nicht so zurücksetzen, wie sie sind: { $names }.
}
dump-cancelled-partial = Export abgebrochen — { $name } wurde nicht verändert; was geschrieben war, steht in { $part }
files-done = { $n ->
    [one] { $n } Datei geschrieben.
   *[other] { $n } Dateien geschrieben.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Export abgebrochen — keine Datei wurde fertiggestellt, daher wurde nichts in { $folder } geschrieben.
files-cancelled = { $n ->
    [one] Export abgebrochen — { $n } Datei wurde in { $folder } fertiggestellt; die Tabelle in Arbeit blieb ungeschrieben.
   *[other] Export abgebrochen — { $n } Dateien wurden in { $folder } fertiggestellt; die Tabelle in Arbeit blieb ungeschrieben.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } bereits in { $folder } geschriebene Datei bleibt erhalten.
   *[other] { $message } — { $n } bereits in { $folder } geschriebene Dateien bleiben erhalten.
}
files-replaced = { $n ->
    [one] { $n } vorhandene Datei wurde ersetzt: { $names }.
   *[other] { $n } vorhandene Dateien wurden ersetzt: { $names }.
}
files-missing = { $n ->
    [one] { $n } ausgewählte Tabelle wurde nicht gefunden und ergab keine Datei: { $names }.
   *[other] { $n } ausgewählte Tabellen wurden nicht gefunden und ergaben keine Datei: { $names }.
}
export-progress-of-about = { $rows } von ~{ $total } Zeilen
export-progress-of = { $rows } von { $total } Zeilen
export-progress-rows = { $count ->
    [one] { $rows } Zeile
   *[other] { $rows } Zeilen
}
export-progress-starting = Wird gestartet
export-progress-file = Datei: { $name }
export-progress-writing = Schreibe { $name }
monitor-title = Live-Monitor
monitor-badge-watching = Beobachtet
monitor-badge-paused = Pausiert
monitor-badge-stopped = Angehalten
monitor-filter-all = Alle
monitor-filter-inserts = Einfügungen
monitor-filter-updates = Aktualisierungen
monitor-filter-deletes = Löschungen
monitor-resume = Abfragen fortsetzen
monitor-pause = Abfragen pausieren
monitor-clear-tip = Protokoll leeren
monitor-export-tip = Protokoll exportieren…
monitor-interval = { $secs } s
monitor-clear-title = Protokoll leeren?
monitor-clear-message = { $count ->
    [one] { $count } Änderung würde verworfen. Das Protokoll ist die einzige Aufzeichnung dessen, was eine gelöschte Zeile enthielt – die Abfrage meldet eine Änderung nicht noch einmal –, und das lässt sich nicht rückgängig machen. Exportiere es vorher, wenn du es behalten willst.
   *[other] { $count } Änderungen würden verworfen. Das Protokoll ist die einzige Aufzeichnung dessen, was eine gelöschte Zeile enthielt – die Abfrage meldet eine Änderung nicht noch einmal –, und das lässt sich nicht rückgängig machen. Exportiere es vorher, wenn du es behalten willst.
}
monitor-col-time = Zeit
monitor-col-action = Aktion
monitor-no-inserts = Noch keine Einfügungen.
monitor-no-updates = Noch keine Aktualisierungen.
monitor-no-deletes = Noch keine Löschungen.
monitor-empty-stopped = Angehalten.
monitor-empty-paused = Pausiert.
monitor-waiting = Warte
monitor-events-since = { $count ->
    [one] { $count } Ereignis seit { $since }
   *[other] { $count } Ereignisse seit { $since }
}
monitor-no-changes = Noch keine Änderungen
monitor-partial = nur die ersten { $cap } Zeilen nach Primärschlüssel werden beobachtet
monitor-capped = die ältesten fallen bei der Obergrenze von { $cap } Änderungen weg
monitor-length-only = { $count ->
    [one] { $columns } ist binär, daher werden seine Änderungen nur an der Länge erkannt
   *[other] { $columns } sind binär, daher werden ihre Änderungen nur an der Länge erkannt
}
monitor-newest-at-bottom = neueste unten
monitor-export-dialog = Änderungsprotokoll exportieren
diff-title = Ergebnisse vergleichen
diff-filter-all = Alle
diff-filter-changed = Geändert
diff-filter-added = Hinzugefügt
diff-filter-removed = Entfernt
diff-match-rows-by = Zeilen zuordnen nach
diff-whole-row = Ganze Zeile
diff-export-tip = Vergleich exportieren…
diff-nothing-to-export = Nichts zu exportieren.
diff-export-dialog = Vergleich exportieren
diff-unnamed-file = den Vergleich
diff-ran-at = Ausgeführt { $time }
diff-side-facts = { $count ->
    [one] { $count } Zeile · { $ms } ms
   *[other] { $count } Zeilen · { $ms } ms
}
diff-only-in = Nur in { $side }: { $columns }
diff-uncompared = Binär, nicht verglichen: { $columns }
diff-summary-identical = { $n ->
        [one] Keine Unterschiede · { $n } Zeile unverändert
       *[other] Keine Unterschiede · { $n } Zeilen unverändert
    }
diff-summary-changed = { $n } geändert
diff-summary-added = { $n } hinzugefügt
diff-summary-removed = { $n } entfernt
diff-summary-unchanged = { $n } unverändert
diff-summary-not-compared = { $columns } nicht verglichen
diff-repeated-keys = { $count ->
    [one] { $count } Zeile wiederholt einen auf ihrer Seite schon vergebenen Schlüssel und wird ausgelassen.
   *[other] { $count } Zeilen wiederholen einen auf ihrer Seite schon vergebenen Schlüssel und werden ausgelassen.
}
diff-capped = Ein Ergebnis wurde begrenzt: Zeilen jenseits der Grenze werden nicht verglichen.
diff-col-change = Änderung
diff-no-differences = Keine Unterschiede.
diff-no-changed = Keine geänderten Zeilen.
diff-no-added = Keine hinzugefügten Zeilen.
diff-no-removed = Keine entfernten Zeilen.
preview-title = Änderungen auf { $connection } anwenden · { $subject }
preview-changes = { $count ->
    [one] { $count } Änderung
   *[other] { $count } Änderungen
}
preview-used-by = Verwendet von
preview-withheld = Diese Engine kann einen Teil dieses Plans nicht ausdrücken
preview-withheld-hint = Nichts wird angewendet, solange dies aufgeführt ist.
preview-omitted = Nicht in dieser Migration enthalten
preview-omitted-hint = Das Anwenden lässt diese Objekte, wie sie sind.
preview-copy = Kopieren
preview-open-in-editor = Im Editor öffnen
preview-back = Zurück
preview-apply = Anwenden
preview-applying = Wird angewendet…
preview-applied = { $count ->
    [one] { $count } Anweisung auf { $subject } angewendet.
   *[other] { $count } Anweisungen auf { $subject } angewendet.
}
preview-refreshed = Das Schema wurde aktualisiert.
conn-import-title = Verbindungen importieren
conn-import-url = Verbindungs-URL
conn-import-add = Hinzufügen
conn-import-url-hint = Eine Verbindungs-URL, eine JDBC-URL oder eine DATABASE_URL-Zeile aus einer .env – eine pro Zeile.
conn-import-choose-file = Datei auswählen…
conn-import-scan = Installierte Clients durchsuchen
conn-import-scanning = Wird durchsucht…
conn-import-docker = Docker-Container
conn-import-asking-docker = Docker wird gefragt…
conn-import-looking = Suche nach Verbindungen auf diesem Rechner…
conn-import-none-found = Keine Verbindungen gefunden. DBeaver, die JetBrains-IDEs und die Kommandozeilen-Clients legen ihre Dateien an bekannten Orten ab; liegt deine woanders, wähle sie oben aus.
conn-import-nothing-yet = Noch nichts gefunden – füge eine URL ein, lies eine Datei oder durchsuche Clients, die du schon nutzt.
conn-import-found = { $count ->
    [one] { $count } Verbindung gefunden
   *[other] { $count } Verbindungen gefunden
}
conn-import-select-all = Alle auswählen
conn-import-select-none = Keine
conn-import-note-no-password = Kein Passwort in der Quelle
conn-import-note-already-saved = Bereits gespeichert
conn-import-note-unexpanded-path = Pfad enthält ein nicht aufgelöstes Makro
conn-import-note-pgpass = Passwort aus deiner eigenen .pgpass übernommen
conn-import-note-port-assumed = Die Quelle nannte keinen Port; dies ist der Standard
conn-import-note-external-login = Meldet sich auf eine Weise an, die Schemaic nicht kann: SQL-Anmeldung festlegen
conn-import-skipped = { $count ->
    [one] { $count } Eintrag wurde nicht importiert: { $names }.
   *[other] { $count } Einträge wurden nicht importiert: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } Eintrag wurde nicht importiert: { $names } und { $rest } weitere.
   *[other] { $count } Einträge wurden nicht importiert: { $names } und { $rest } weitere.
}
conn-import-reason-unsupported-engine = nicht unterstützte Engine ({ $driver })
conn-import-reason-no-server = kein Server, mit dem verbunden werden kann
conn-import-reason-not-published = sein Port ist für diesen Rechner nicht veröffentlicht
conn-import-reason-unreadable = konnte nicht gelesen werden ({ $error })
conn-import-reason-not-an-export = konnte nicht gelesen werden (kein Verbindungsexport von Schemaic: { $error })
conn-import-reason-newer-export = konnte nicht gelesen werden (von einem neueren Schemaic exportiert (Version { $version }) – aktualisiere, um es zu importieren)
conn-import-url-empty = Füge eine Verbindungs-URL ein.
conn-import-url-no-scheme = Kein Schema – eine URL beginnt mit mysql://, postgresql://, sqlserver:// oder sqlite:, eine SQL-Server-Verbindungszeichenfolge mit Server=.
conn-import-url-unknown-scheme = Unbekanntes Schema „{ $scheme }“ – erwartet wird mysql, mariadb, postgresql, sqlserver oder sqlite.
conn-import-url-no-host = Kein Host in der URL.
conn-import-url-bad-port = „{ $port }“ ist keine Portnummer.
conn-import-url-no-path = Keine Datenbankdatei nach sqlite:.
conn-import-url-transport = „{ $server }“ wird über Named Pipes oder LocalDB erreicht – Schemaic verbindet sich nur über TCP.
conn-import-url-userinfo = Der Servername enthält ein @ – ein Benutzername oder Passwort gehört in sein eigenes Feld, nicht in den Server.
conn-import-url-not-sql-server = Diese Verbindungszeichenfolge sagt nicht, dass sie zu SQL Server gehört – die .NET-Treiber von MySQL und PostgreSQL schreiben dieselben Schlüsselwörter. Füge stattdessen die URL des Servers ein (mysql://, postgresql://, sqlserver://), oder schreib für SQL Server den Server als tcp:host.
conn-import-row-overflow = Die ersten { $shown } von { $total } werden angezeigt. Die übrigen { $hidden } werden trotzdem importiert, wenn sie angehakt sind – die Liste endet hier, damit das Fenster reaktionsschnell bleibt.
conn-import-import = Importieren
conn-import-import-count = { $count } importieren
blob-kind-png = PNG-Bild
blob-kind-jpeg = JPEG-Bild
blob-kind-gif = GIF-Bild
blob-kind-bmp = BMP-Bild
blob-kind-webp = WebP-Bild
blob-kind-ico = Symbol
blob-kind-opaque = Binärdaten
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · angezeigt werden die ersten { $shown }
blob-pane-preview = Vorschau
blob-pane-hex = Hex
blob-reading = Wird gelesen…
blob-empty = Diese Zelle ist NULL, oder ihre Zeile existiert nicht mehr.
blob-preview-too-large = Dieses Bild ist { $width } × { $height } groß – zu groß für eine Vorschau hier. Die Bytes sind unversehrt: lies sie als Hex oder speichere sie in eine Datei.
blob-preview-truncated = Nur die ersten { $shown } dieses Werts ({ $kind }) wurden gelesen, daher lässt er sich nicht darstellen. Lies die Bytes als Hex.
blob-preview-unreadable = Diese Bytes beginnen wie ein Wert vom Typ { $kind }, lassen sich aber nicht so lesen. Lies sie als Hex oder speichere sie in eine Datei.
blob-save-truncated = Nur die ersten { $shown } von { $size } konnten gelesen werden, dies würde also nur den Anfang des Werts speichern.
blob-load = Aus Datei laden
blob-save = In Datei speichern
blob-load-dialog = Binärwert aus Datei laden
blob-save-dialog = Binärwert speichern
blob-saved-to = Gespeichert unter { $path }
blob-loaded-file = Datei geladen. Übernimm die Änderungen im Ergebnisraster, um sie zu schreiben.
blob-not-writable = Diese Zelle kann nicht geschrieben werden.
blob-file-too-large = Diese Datei ist { $got } groß – diese Spalte fasst höchstens { $most }.
blob-not-accepting = Diese Zelle nimmt keinen Wert mehr an – nichts wurde vorgemerkt.
erd-title = ER-Diagramm
erd-scope = Umfang
erd-chip-tables = { $count ->
    [one] { $count } Tabelle
   *[other] { $count } Tabellen
}
erd-chip-relationships = { $count ->
    [one] { $count } Beziehung
   *[other] { $count } Beziehungen
}
erd-chip-hidden = { $count } unverbundene ausgeblendet
erd-find-placeholder = Tabelle oder Spalte suchen
erd-more-columns = +{ $count } weitere
erd-show-less = weniger anzeigen
erd-not-loaded = Das Schema dieser Datenbank ist noch nicht geladen.
erd-no-relationships = Keine Fremdschlüsselbeziehungen für ein Diagramm.
erd-export-tip = Diagramm exportieren…
erd-copy-as = Kopieren als
erd-export-dialog = Diagramm exportieren
erd-nothing-to-export = Nichts zu exportieren.
erd-nothing-to-copy = Nichts zu kopieren.
erd-copied-as = Als { $format } kopiert
erd-clipboard-unavailable = Die Zwischenablage ist nicht verfügbar.
erd-copy-failed = Kopieren fehlgeschlagen: { $error }
erd-saved = { $name } gespeichert
erd-unnamed-file = die Datei
server-vars-title = Servervariablen — { $connection }
server-vars-search = Einstellungen suchen
server-vars-variables = Variablen
server-vars-status = Status
server-vars-loading = Wird geladen…
server-vars-unsupported = Die Engine dieser Verbindung veröffentlicht keine Liste dieser Art.
server-vars-no-match = Keine passenden Einstellungen.
server-vars-more = { $count ->
    [one] { $count } weitere Einstellung passt – Suche eingrenzen
   *[other] { $count } weitere Einstellungen passen – Suche eingrenzen
}
server-vars-count = { $count ->
    [one] { $count } Einstellung
   *[other] { $count } Einstellungen
}
server-vars-count-filtered = { $shown } von { $count } Einstellungen
server-vars-refresh = Aktualisieren
snippet-edit-title = Snippet bearbeiten — { $name }
snippet-edit-name = Name
snippet-edit-name-placeholder = Name des Snippets
snippet-edit-shortcut = Kürzel zum Erweitern
snippet-edit-shortcut-none = keins
snippet-edit-query = Abfrage
snippet-edit-save = Speichern
plan-title = Abfrageplan
plan-analyze-unavailable = Analyze nicht verfügbar (Anweisung schreibt Daten)
plan-analyze-hint = Führt die Anweisung aus, um echte Laufzeiten zu messen
plan-explaining = Wird erklärt
plan-potential-issues = Mögliche Probleme
plan-detail-no-index = kein Index verwendet
plan-detail-key-unused = Index nicht für die Suche verwendet
plan-my-full-scan = Vollständiger Tabellenscan auf `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Vollständiger Tabellenscan auf dem Ergebnis ({ $detail }, type = ALL)
plan-my-filesort = Filesort auf `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort auf dem Ergebnis (Extra: Using filesort)
plan-my-temporary = Temporäre Tabelle für `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Temporäre Tabelle für das Ergebnis (Extra: Using temporary)
plan-pg-seq-scan = Sequenzieller Scan auf `{ $table }` (kein Index verwendet)
plan-pg-seq-scan-unnamed = Sequenzieller Scan auf einer Tabelle (kein Index verwendet)
plan-pg-sort = Sort-Knoten — die Abfrage sortiert Zeilen während der Ausführung
plan-ms-full-scan = Vollständiger Scan von `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Vollständiger Scan einer Tabelle ({ $op })
plan-ms-sort = Sort — die Abfrage sortiert Zeilen während der Ausführung
plan-ms-spool = { $op } — eine Zwischen-Arbeitstabelle in tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = kein Join-Prädikat — jede Zeile wird mit jeder Zeile gepaart
plan-server-no-statistics = eine gelesene Spalte hat keine Statistiken
plan-server-spill = in tempdb ausgelagert — die Speicherzuteilung war zu klein
plan-server-convert = eine Typumwandlung beeinflusst den Plan ({ $issue }): { $expression }
plan-server-unmatched-indexes = ein gefilterter Index konnte nicht verwendet werden
plan-server-memory-grant = die Speicherzuteilung wurde falsch eingeschätzt
plan-server-wait = hat auf { $wait } gewartet
plan-server-other = der Server warnt: { $name }
plan-missing-index = Fehlender Index auf `{ $table }` ({ $keys })
plan-missing-index-include = Fehlender Index auf `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Fehlender Index auf `{ $table }` ({ $keys }) — der Server erwartete eine Verbesserung um { $impact } %
plan-missing-index-include-impact = Fehlender Index auf `{ $table }` ({ $keys }; include { $include }) — der Server erwartete eine Verbesserung um { $impact } %
plan-ask-ai = KI fragen
used-by-title = Verwendet von — { $object }
used-by-copy = Kopieren
used-by-not-loaded = Das Schema von { $database } ist noch nicht geladen – klappe es im Baum auf und frage dann erneut.
used-by-nothing-in = Nichts in { $database } verwendet es.
used-by-nothing = Nichts verwendet es.
used-by-limits = Gefunden durch Lesen der Definitionen in dieser Datenbank: zur Laufzeit erzeugtes SQL und andere Datenbanken werden nicht erfasst.
used-by-dropped-with-it = { $detail } · wird mit gelöscht
used-by-heading-view = { $count ->
    [one] Sicht ({ $count })
   *[other] Sichten ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Routine ({ $count })
   *[other] Routinen ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Trigger ({ $count })
}
used-by-heading-event = { $count ->
    [one] Ereignis ({ $count })
   *[other] Ereignisse ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Fremdschlüssel ({ $count })
   *[other] Fremdschlüssel ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Tabellenausdruck ({ $count })
   *[other] Tabellenausdrücke ({ $count })
}
used-by-noun-view = Sicht
used-by-noun-routine = Routine
used-by-noun-trigger = Trigger
used-by-noun-event = Ereignis
used-by-noun-foreign-key = Fremdschlüssel
used-by-noun-expression = Tabellenausdruck
theme-dark = Dunkel
theme-light = Hell
md-code-block = CODE
md-code-copy = Kopieren
md-code-insert = Einfügen
md-code-run = Ausführen
md-proposal-title = Vorgeschlagene Änderung an { $table }
md-proposal-count = { $count ->
    [one] { $count } Änderung
   *[other] { $count } Änderungen
}
md-proposal-review = Änderung prüfen…
md-proposal-unreadable = Der Assistent hat eine Änderung vorgeschlagen, die sich nicht lesen ließ — { $error }
cell-choose = Auswählen…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Jetzt
cell-calendar-today = Heute
cell-month-1 = Januar
cell-month-2 = Februar
cell-month-3 = März
cell-month-4 = April
cell-month-5 = Mai
cell-month-6 = Juni
cell-month-7 = Juli
cell-month-8 = August
cell-month-9 = September
cell-month-10 = Oktober
cell-month-11 = November
cell-month-12 = Dezember
cell-weekday-mon = Mo
cell-weekday-tue = Di
cell-weekday-wed = Mi
cell-weekday-thu = Do
cell-weekday-fri = Fr
cell-weekday-sat = Sa
cell-weekday-sun = So
alias-rename-title-alias = Alias umbenennen
alias-rename-title-cte = CTE umbenennen
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = Name der CTE
alias-rename-new-name = Neuer Name
alias-rename-rename = Umbenennen
alias-rename-no-alias = Hier gibt es nichts mehr umzubenennen.
alias-rename-empty = Gib einen Namen ein.
alias-rename-conflict = „{ $name }“ ist in dieser Abfrage schon ein Name: Eine Umbenennung darauf würde ändern, welche Tabelle eine Referenz liest.
alias-rename-unsure = Eine Referenz hier liest vielleicht eine andere Spalte dieses Namens oder liest diese dort, wo die Umbenennung nicht folgen kann, und das geladene Schema kann es nicht entscheiden — daher wird nichts geändert. Lade das Schema der Tabellen oder benenne von Hand um.
alias-rename-changed = Die Abfrage hat sich geändert, während dies offen war. Schließe es und benenne erneut um.
alias-rename-read-only = Der Editor ist schreibgeschützt, bis der KI-Vorschlag angenommen oder abgelehnt ist.
usages-title = { $count ->
    [one] { $count } Verwendung von { $name }
   *[other] { $count } Verwendungen von { $name }
}
raster-invalid-svg = Ungültiges SVG: { $error }
raster-too-large = Das Bild ist zu groß zum Rastern ({ $width }×{ $height })
raster-png-failed = PNG-Kodierung fehlgeschlagen: { $error }
grid-menu-copy = Kopieren
grid-menu-copy-value = Wert kopieren
grid-menu-copy-formatted = Formatiert kopieren
grid-menu-copy-as = Kopieren als
grid-menu-paste = Einfügen
grid-menu-fill-down = Nach unten ausfüllen
grid-menu-fill-up = Nach oben ausfüllen
grid-menu-edit-field = Feld bearbeiten
grid-menu-edit-row = Zeile bearbeiten
grid-menu-view-row = Zeile anzeigen
grid-menu-edit-binary = Binärwert bearbeiten
grid-menu-view-binary = Binärwert anzeigen
grid-menu-filter-by-value = Nach diesem Wert filtern
grid-menu-exclude-value = Diesen Wert ausschließen
grid-menu-follow-relation = Beziehung folgen
grid-menu-referenced-by = Referenziert von
grid-menu-set-null = Auf NULL setzen
grid-menu-set-default = Auf DEFAULT setzen
grid-menu-duplicate-rows = { $count ->
    [one] Zeile duplizieren
   *[other] { $count } Zeilen duplizieren
}
grid-menu-delete-rows = { $count ->
    [one] Zeile löschen
   *[other] { $count } Zeilen löschen
}
grid-menu-undo-delete = Löschen rückgängig machen
grid-menu-ai-summary = KI-Zusammenfassung
grid-attach-to-chat = { $scope } an den Chat anhängen
grid-attach-scope-rows = { $rows ->
    [one] { $rows } Zeile
   *[other] { $rows } Zeilen
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } Zeile
   *[other] { $rows } Zeilen
} und { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
}
grid-menu-freeze = Fixieren
grid-menu-unfreeze = Fixierung aufheben
grid-menu-hide-column = Spalte ausblenden
grid-menu-show-hidden-columns = Ausgeblendete Spalten anzeigen ({ $count })
grid-menu-show-all = Alle anzeigen
grid-menu-format-as = Formatieren als
grid-menu-copy-name = Namen kopieren
grid-menu-copy-qualified-name = Qualifizierten Namen kopieren
grid-menu-copy-values = Werte kopieren
grid-menu-sql-in-list = SQL-IN-Liste
grid-stats = { $rows } · { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
} · { $ms } ms
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
} · { $ms } ms
grid-rows-read = { $n ->
    [one] { $read } Zeile
   *[other] { $read } Zeilen
}
grid-rows-read-capped = { $n ->
    [one] { $read } Zeile
   *[other] { $read } Zeilen
} (begrenzt)
grid-rows-read-of-total = { $n ->
    [one] { $read } von { $total } Zeile
   *[other] { $read } von { $total } Zeilen
}
grid-kept-note = · behalten — schreibgeschützt
grid-arena-note = · { $columns } zu groß, um vollständig gehalten zu werden — spätere Zeilen bleiben leer
grid-sorted-subset = · sortierte Teilmenge (begrenzt) — nicht die vollständige Reihenfolge
grid-reading = wird gelesen…
grid-committing = Wird committet…
grid-commit-tip = { $count ->
    [one] { $count } Änderung committen ({ $keys })
   *[other] { $count } Änderungen committen ({ $keys })
}
grid-discard-tip = Alle ausstehenden Änderungen verwerfen
grid-copy-sql-tip = Die Änderungen als SQL kopieren
grid-add-row-tip = Zeile hinzufügen
grid-delete-row-tip = Die ausgewählte Zeile zum Löschen markieren ({ $key })
grid-clone-row-tip = Die ausgewählte Zeile klonen
grid-copy-results-tip = Die Ergebnisse kopieren…
grid-copy-results-empty = Nichts zu kopieren — keine Zeilen
grid-save-results-tip = Die Ergebnisse in eine Datei speichern…
grid-save-results-empty = Nichts zu speichern — keine Zeilen
grid-ai-tip = Zeilen mit KI erzeugen…
grid-ai-generating = Wird erzeugt…
grid-ai-off = KI-Aktionen senden Daten — für diese Verbindung ausgeschaltet
grid-ai-fill-value = Wert mit KI ausfüllen
grid-ai-insert-row = Zeile mit KI einfügen
grid-ai-seed-table = Tabelle mit KI befüllen
grid-chart-tip = Die Ergebnisse als Diagramm darstellen
grid-chart-empty = Nichts darzustellen — keine Zeilen
grid-chart-this-result = dieses Ergebnis
grid-error-view = Anzeigen
grid-rollback-tab = { $tab } zurückrollen
grid-find-placeholder = In Ergebnissen suchen
grid-figure-copied = Kopiert
grid-goto-label = Gehe zu Zeile:
export-dest-clipboard = die Zwischenablage
export-dest-file = die Datei
export-note-rows = { $n ->
    [one] { $rows } Zeile in { $name } exportiert
   *[other] { $rows } Zeilen in { $name } exportiert
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } enthält Bytes, die dieses Format nicht tragen kann: Diese Zellen sind leer
   *[other] { $cols } enthalten Bytes, die dieses Format nicht tragen kann: Diese Zellen sind leer
}
export-note-blanked = { $cols } zu groß, um vollständig gehalten zu werden: spätere Zeilen sind leer
export-note-cut = { $cols } auf Excels Zellgrenze von { $limit } Zeichen gekürzt
export-failure-partial = { $message } — { $name } wurde nicht verändert; die geschriebenen Zeilen stehen in { $part }
export-cancelled-partial = Export abgebrochen — { $name } wurde nicht verändert; die geschriebenen Zeilen stehen in { $part }
export-cancelled-nothing = Export abgebrochen — { $name } wurde nicht verändert.
copy-sql-as-null = { $n ->
    [one] In die Zwischenablage kopiert — { $cols } enthält Bytes, die SQL-Text nicht tragen kann: als NULL geschrieben
   *[other] In die Zwischenablage kopiert — { $cols } enthalten Bytes, die SQL-Text nicht tragen kann: als NULL geschrieben
}
copy-sql-nothing = { $n ->
    [one] Nichts kopiert — { $cols } enthält Bytes, die SQL-Text nicht tragen kann
   *[other] Nichts kopiert — { $cols } enthalten Bytes, die SQL-Text nicht tragen kann
}
copy-sql-left-out = { $n ->
    [one] In die Zwischenablage kopiert — { $cols } enthält Bytes, die SQL-Text nicht tragen kann: diese Werte sind ausgelassen
   *[other] In die Zwischenablage kopiert — { $cols } enthalten Bytes, die SQL-Text nicht tragen kann: diese Werte sind ausgelassen
}
grid-fill-nothing = Nichts auszufüllen: Die ausgewählten Spalten sind nicht beschreibbar, oder die zu kopierende Zelle ist der nicht gesetzte Wert einer neuen Zeile oder ein vorgemerktes DEFAULT.
grid-fill-skipped = { $count ->
    [one] { $filled } Zelle ausgefüllt, { $skipped } in zum Löschen markierten Zeilen übersprungen.
   *[other] { $filled } Zellen ausgefüllt, { $skipped } in zum Löschen markierten Zeilen übersprungen.
}
grid-attach-summary = { $rows ->
    [one] { $rows } Zeile
   *[other] { $rows } Zeilen
} × { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } Zeile
   *[other] { $rows } Zeilen
} × { $cols ->
    [one] { $cols } Spalte
   *[other] { $cols } Spalten
} aus { $table }
grid-attach-summary-capped = { $summary } (die ersten { $taken } von { $total } ausgewählten)
grid-copy-sql-unset = Nichts als SQL zu kopieren — die ausgewählten Zellen der neuen Zeile haben noch keinen Wert und erhalten den Standardwert des Servers.
grid-copy-sql-joined = Nichts als SQL zu kopieren: Diese Zeilen stammen aus einer Abfrage über mehr als eine Tabelle.
grid-export-fetched-rows = Abgerufene Zeilen ({ $count })
grid-export-dialog-title = Ergebnisse exportieren
grid-export-already-running = Es läuft bereits ein Export — es wurde nichts geschrieben.
grid-row-title = Zeile { $row }
grid-row-title-in-table = Zeile { $row }  ·  { $table }
grid-row-set-value = Wert setzen
grid-row-set-null = Auf NULL setzen
grid-row-edit-binary = Bearbeiten
grid-row-view-binary = Anzeigen
grid-row-invalid-json = Ungültiges JSON: { $error }
grid-row-saving = Wird gespeichert
grid-rows-affected = { $count ->
    [one] { $count } Zeile betroffen · { $ms } ms
   *[other] { $count } Zeilen betroffen · { $ms } ms
}
grid-rerun-would-write = Diese Anweisung kann nicht erneut ausgeführt werden — sie würde in die Datenbank schreiben
grid-filter-not-simple = Diese Abfrage kann nicht gefiltert werden — kein einfaches SELECT auf eine Tabelle
grid-staged-requery = Übernimm oder verwirf zuerst deine Änderungen — ein erneutes Lesen der Zeilen würde sie verwerfen
grid-staged-landing = Während die Tabelle neu gelesen wurde, wurden Änderungen vorgemerkt; sie wurden behalten und die Tabelle nicht aktualisiert — übernimm oder verwirf sie, dann aktualisiere.
grid-reread-without-filter = Der Filter ruft eine Funktion auf, die Schemaic nicht ungefragt erneut ausführt, daher wurde die Tabelle nach dem Commit ohne Filter und Sortierung neu gelesen.
grid-hidden-columns-tip = { $count ->
    [one] { $count } ausgeblendete Spalte — klicken, um alle anzuzeigen, oder Rechtsklick auf eine Spaltenüberschrift zur Auswahl
   *[other] { $count } ausgeblendete Spalten — klicken, um alle anzuzeigen, oder Rechtsklick auf eine Spaltenüberschrift zur Auswahl
}
grid-copied-staged-sql = Die vorgemerkten Änderungen wurden als SQL kopiert.
grid-copy-split = { $n ->
        [one] { $n } kopierte Zelle enthält einen Tabulator oder Zeilenumbruch — beim Einfügen wird sie als mehr als eine Zelle gelesen.
       *[other] { $n } kopierte Zellen enthalten einen Tabulator oder Zeilenumbruch — beim Einfügen wird jede davon als mehr als eine Zelle gelesen.
    }
grid-duplicate-title = Zeilen duplizieren
grid-duplicate-question = { $n ->
    [one] { $rows } Zeile duplizieren? Sie wird als ausstehende Zeile vorgemerkt, und nichts wird geschrieben, bis du committest.
   *[other] { $rows } Zeilen duplizieren? Sie werden als ausstehende Zeilen vorgemerkt, und nichts wird geschrieben, bis du committest.
}
grid-nullable-tip = Nullable
grid-cell-auto = <auto>
grid-cell-required = <Pflicht>
grid-seed-title = Zeilen erzeugen
grid-seed-generate = Erzeugen
footer-panel-schema = Schema
footer-panel-ai = KI-Assistent
footer-panel-history = Abfrageverlauf
footer-panel-snippets = Snippet-Bibliothek
footer-panel-activity = Serveraktivität
footer-panel-terminal = Terminal
footer-panel-narrow = Das Fenster ist für dieses Panel zu schmal — verbreitere es, um das Panel zurückzuholen.
footer-cursor = Z. { $line }, Sp. { $col }
footer-indent-spaces = Leerzeichen: { $width }
footer-indent-tabs = Tabs: { $width }
footer-spaces = Leerzeichen
footer-tabs = Tabs
footer-wrap = Umbruch
footer-no-wrap = Kein Umbruch
footer-read-only = Schreibgeschützt
footer-write-mode = Schreibmodus
footer-tx-auto = Auto-Commit
footer-tx-manual = Manuell
footer-tx-open = { $count } offen
footer-tx-aborted = Transaktion abgebrochen — Rollback zum Fortfahren
footer-tx-lost = Transaktion verloren
footer-commit = Commit
footer-rollback = Rollback
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }
lib-tab-default-title = Abfrage { $n }
lib-result-default-label = Ergebnis { $n }
lib-ai-off-note = Der KI-Assistent ist ausgeschaltet. Wähle in den KI-Einstellungen eine Agent-CLI, um ihn einzuschalten.
lib-test-testing = Wird getestet…
lib-test-connected = Verbunden
lib-test-failed = Verbindung fehlgeschlagen
lib-effort-minimal = Minimal
lib-effort-low = Niedrig
lib-effort-medium = Mittel
lib-effort-high = Hoch
lib-effort-extra = Extra
lib-effort-max = Maximal
lib-schema-scope-active = Nur aktive Datenbank
lib-schema-scope-all = Alle Datenbanken
lib-schema-scope-none = Keiner
lib-cursor-block = Block
lib-cursor-bar = Balken
lib-cursor-underline = Unterstrich
header-no-connection = Keine Verbindung
header-new-connection = Neue Verbindung
header-find-anywhere-tip = Überall suchen ({ $keys })
header-shortcuts-tip = Tastenkürzel
header-settings-tip = Einstellungen
header-disconnected = Getrennt
header-retry = Erneut versuchen
colour-red = Rot
colour-orange = Orange
colour-amber = Bernstein
colour-green = Grün
colour-teal = Türkis
colour-blue = Blau
colour-purple = Lila
colour-pink = Rosa
disk-changed = „{ $name }“ wurde auf dem Datenträger geändert.
disk-reload = Neu laden
disk-keep-mine = Meine behalten
results-run-hint = Abfrage ausführen  ({ $keys })
results-failed = Anweisung fehlgeschlagen.
results-cancelled = Abfrage abgebrochen.
results-pinned-total = { $count } angeheftet · { $size }
results-pinned-tip = Angeheftete Ergebnisse bleiben im Speicher, bis du sie schließt. Schließe die, die du nicht mehr brauchst, um ihn freizugeben.
results-chip-nothing-run = In diesem Tab wurde noch nichts ausgeführt.
results-chip-kept = behalten — schreibgeschützt
results-menu-pin = Anheften
results-menu-unpin = Lösen
results-menu-close = Schließen
results-menu-compare-with = Vergleichen mit { $result }
results-menu-close-others = Andere Ergebnisse schließen
results-menu-close-all = Alle Ergebnisse schließen
search-everywhere-placeholder = Überall suchen
widgets-nothing-to-choose = Nichts zur Auswahl
widgets-spinner-verbs = Abwägen, Ausbrüten, Backen, Basteln, Berechnen, Bummeln, Brauen, Brodeln, Brüten, Destillieren, Entschlüsseln, Entwirren, Ergründen, Erkunden, Fabulieren, Flitzen, Formen, Garen, Gären, Graben, Grübeln, Hexen, Huschen, Improvisieren, Jonglieren, Keimen, Kneten, Knobeln, Köcheln, Kombinieren, Komponieren, Kreiseln, Kristallisieren, Marinieren, Meditieren, Nachdenken, Ordnen, Orchestrieren, Philosophieren, Rechnen, Reifen, Rühren, Schlendern, Schmieden, Schmoren, Schürfen, Schweben, Sichten, Sinnieren, Sortieren, Sprießen, Sprudeln, Stöbern, Tänzeln, Trödeln, Tüfteln, Verdichten, Werkeln, Wirbeln, Wühlen, Würzen, Wuseln, Zaubern, Zwirbeln
cmdk-placeholder = Bitte den KI-Assistenten um Hilfe.
cmdk-accept = Übernehmen
cmdk-reject = Verwerfen
params-value-placeholder = Wert
params-kind-tip = Text, Zahl, NULL oder rohes SQL — zum Ändern klicken
params-missing = Kein Wert für { $names }
params-not-a-number = :{ $name } ist eine Zahl, lautet aber „{ $text }“
editor-error-view = Anzeigen
editor-run-anyway = Trotzdem ausführen
guard-read-only = Schreibgeschützte Verbindung.
guard-read-only-refused = Schreibgeschützte Verbindung: { $reason }
guard-no-database = Keine Datenbank ausgewählt.
guard-writes-one = Diese Anweisung ändert Daten.
guard-writes-several = Diese Anweisungen ändern Daten.
guard-script = Jede Anweisung in { $file } ausführen? Ein Skript kann Objekte anlegen, ändern und löschen, und was es ändert, lässt sich von hier aus nicht rückgängig machen.
guard-staged = { $n ->
    [one] Diese Ausführung ersetzt das Ergebnis mit { $n } vorgemerkten Änderung — die Ausführung würde sie verwerfen. Übernimm oder verwirf sie zuerst, oder führe trotzdem aus.
   *[other] Diese Ausführung ersetzt das Ergebnis mit { $n } vorgemerkten Änderungen — die Ausführung würde sie verwerfen. Übernimm oder verwirf sie zuerst, oder führe trotzdem aus.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL entfernt jede Zeile der Tabelle.
guard-truncate = TRUNCATE entfernt jede Zeile der Tabelle.
guard-no-where = { $kind }-Anweisung ohne WHERE-Klausel erkannt.
guard-drop-table = DROP TABLE löscht die Tabelle und jede Zeile darin.
guard-drop-database = DROP DATABASE löscht die Datenbank und jede Tabelle darin.
guard-drop-schema = DROP SCHEMA löscht das Schema und jede Tabelle darin.
guard-drop-owned = DROP OWNED löscht alles, was der Rolle gehört, jede Tabelle samt ihren Zeilen.
guard-drop-cascade = DROP { $obj } … CASCADE löscht jede davon abhängige Spalte samt ihren Werten.
guard-replace-table = CREATE OR REPLACE TABLE löscht die vorhandene Tabelle und jede Zeile darin.
guard-replace-database = CREATE OR REPLACE DATABASE löscht die vorhandene Datenbank und jede Tabelle darin.
guard-replace-schema = CREATE OR REPLACE SCHEMA löscht das vorhandene Schema und jede Tabelle darin.
guard-truncate-partition = TRUNCATE PARTITION entfernt jede Zeile in den genannten Partitionen.
guard-drop-partition = DROP PARTITION löscht die Partition und jede Zeile darin.
editor-diag-fix = { $keys }: { $fix }
editor-menu-ask-ai = KI fragen
editor-menu-explain = Erklären
editor-menu-optimise = Optimieren
editor-menu-plan = Plan
editor-menu-copy-single-line = Als einzelne Zeile kopieren
editor-menu-make-single-line = In eine Zeile umwandeln
editor-menu-format-statement = Anweisung formatieren
editor-menu-find-usages = Verwendungen finden
editor-menu-create-view = Sicht erstellen
cmdk-optimise-label = Diese Abfrage optimieren
editor-several-carets = Mehrere Cursor und keine Auswahl: Wähle aus, was ausgeführt werden soll, oder lass nur einen Cursor übrig
editor-run-current = Aktuelle ausführen
editor-run-everything = Alle ausführen
find-placeholder = Suchen
find-invalid = Ungültig
find-match-case = Groß-/Kleinschreibung beachten
find-whole-word = Ganzes Wort
find-regex = Regulärer Ausdruck
find-scope = Ausw
find-in-selection-tip = In Auswahl suchen
find-replace-placeholder = Ersetzen
find-replace = Ersetzen
find-replace-all = Alle
goto-line-label = Gehe zu:
cli-no-client = Kein { $client }-Client gefunden (PATH oder WSL).
cli-no-client-on-path = Kein { $client }-Client im PATH gefunden.
cli-no-tool = { $tool } wurde im PATH nicht gefunden.
cli-no-either-tool = Weder { $first } noch { $second } im PATH gefunden.
cli-no-either-client = Weder ein { $first }- noch ein { $second }-Client im PATH gefunden.
cli-sqlcmd-windows-in-wsl = Die Windows-Anmeldung braucht ein sqlcmd für Windows, und das gefundene liegt in WSL. Installiere sqlcmd unter Windows, oder öffne die Verbindung in einem Abfrage-Tab.
cli-refused-shim = Der im PATH gefundene Client ist ein .cmd/.bat-Shim, den Windows über cmd.exe ausführt – wo ein Datenbankname mit '&' oder '|' ein zweiter Befehl wäre. Verweise in den Einstellungen auf die echte ausführbare Datei, oder öffne stattdessen einen Abfrage-Tab.
cli-refused-name-equals = Der Name dieser Datenbank enthält '=', was psql als Verbindungszeichenfolge lesen und zu einem anderen Server folgen würde. Öffne sie stattdessen in einem Abfrage-Tab.
cli-refused-name-uri = Der Name dieser Datenbank beginnt wie eine Verbindungs-URI, der psql zu einem anderen Server folgen würde. Öffne sie stattdessen in einem Abfrage-Tab.
cli-refused-file-dash = Der Name dieser Datenbankdatei beginnt mit '-', was der sqlite3-Client als Option statt als Datei lesen würde. Benenne sie um oder gib einen Pfad an (./name.db), und öffne sie erneut.
cli-refused-sqlcmd-entra = sqlcmd kann das Anmeldetoken der Azure CLI nicht übergeben werden, daher lässt sich diese Verbindung darin nicht öffnen. Öffne sie stattdessen in einem Abfrage-Tab.
cli-refused-sqlcmd-host = Der Host dieser Verbindung enthält ein Zeichen, das sqlcmd als Teil der Adresse liest (',', ';', ein zweites '\' oder ein ':'-Präfix). Gib den Port in seinem eigenen Feld an, und öffne sie erneut.
cli-refused-sqlcmd-ca = sqlcmd kann nicht gegen eine CA-Datei prüfen. Füge die CA dem Zertifikatspeicher des Systems hinzu und leere das CA-Feld der Verbindung, oder öffne die Verbindung in einem Abfrage-Tab.
cli-refused-sqlcmd-client-cert = SQL Server meldet niemanden mit einem Client-Zertifikat an, daher kann sqlcmd das dieser Verbindung nirgendwohin senden. Leere Client-Zertifikat und Schlüssel, und öffne sie erneut.
cli-refused-wsl-paths = Die TLS-Zertifikatspfade dieser Verbindung sind Windows-Pfade, die der WSL-Client nicht öffnen kann. Installiere einen nativen Client, oder verschiebe die Zertifikate in WSL.
cli-refused-tunnelled-verify = Diese Verbindung prüft das Zertifikat des Servers und erreicht ihn über einen SSH-Tunnel, daher würde der Client angewiesen, 127.0.0.1 anzuwählen und das Zertifikat dagegen zu prüfen – was das eigene Zertifikat des Servers nicht erfüllen kann. Öffne stattdessen einen Abfrage-Tab, oder senke den TLS-Modus für die CLI-Sitzung.
cli-refused-not-container-id = docker ps hat etwas ausgegeben, das keine Container-ID ist.
cli-refused-file-line-break = Dieser Dateiname enthält einen Zeilenumbruch, den der Client als zwei Argumente lesen würde. Wähle einen anderen Namen.
cli-refused-role = Die Rolle im Init-SQL dieser Verbindung kann dem Werkzeug so nicht übergeben werden.
cli-refused-mysql-source-path = Der mysql-Client würde den Pfad dieser Datei als eigene Befehle lesen (ein ';' oder ein '\' darin, oder ein Leerzeichen am Anfang oder Ende). Verschiebe oder benenne sie um, und stelle sie dann wieder her.
cli-refused-cloud-token = Diese Verbindung meldet sich über das Token einer Cloud-CLI an, das einem externen Client nicht übergeben wird. Öffne sie stattdessen in einem Abfrage-Tab.
install-no-exe-path = Es lässt sich nicht feststellen, von wo Schemaic läuft: { $error }
install-registry-open = Deine Benutzerumgebung in der Registry konnte nicht geöffnet werden: { $error }
install-path-read = Dein Benutzer-PATH konnte nicht gelesen werden: { $error }
install-path-write = Dein Benutzer-PATH konnte nicht geschrieben werden: { $error }
install-look-at = { $path } konnte nicht geprüft werden: { $error }
install-read-link = Der Link unter { $path } konnte nicht gelesen werden: { $error }
install-create = { $path } konnte nicht angelegt werden: { $error }
install-remove-old = Das alte { $path } konnte nicht entfernt werden: { $error }
install-remove = { $path } konnte nicht entfernt werden: { $error }
install-path-is-windows = Ein Eintrag im Benutzer-PATH ist eine Windows-Installation; auf dieser Plattform wird stattdessen verlinkt.
install-link-is-unix = Das Verlinken des Befehls ist eine Installation für macOS und Linux.
install-refuse-no-folder = Es lässt sich nicht feststellen, in welchem Ordner { $exe } liegt.
install-refuse-no-shim = Neben dieser Kopie von Schemaic, in { $dir }, liegt kein schemaic.com, also gibt es keinen Konsolenbefehl, der in den PATH könnte. Die installierte App hat einen; ein Entwicklungs-Build nicht.
install-refuse-semicolon = Diese Kopie von Schemaic liegt in { $dir }, dessen Name ein ';' enthält — PATH würde ihn als zwei verschiedene Ordner lesen. Verschiebe sie an einen Ort ohne und versuche es erneut.
install-refuse-exe-replaced = { $exe } wurde ersetzt, während Schemaic lief, daher würde ein Link darauf ins Leere zeigen. Starte Schemaic neu und installiere dann erneut.
install-refuse-no-home = Es gibt kein Home-Verzeichnis für den Link, also gibt es keinen Ort zum Nachsehen.
install-refuse-path-not-text = Dein Benutzer-PATH ist in der Registry nicht als Text gespeichert, daher wurde er nicht angetastet.
install-refuse-path-not-unicode = Dein Benutzer-PATH enthält Zeichen, die sich nicht sicher zurückschreiben lassen, daher wurde er nicht angetastet.
install-refuse-portable = Dies ist eine portable Kopie, daher kann Entfernen nicht erkennen, ob Installieren { $dir } in deinen PATH gesetzt hat oder ob der Ordner schon mit anderen Programmen darin war. Entferne ihn unter Umgebungsvariablen, falls Installieren ihn hinzugefügt hat.
install-refuse-spot-taken = { $link } existiert bereits und ist kein Link, daher hat Installieren es nicht angetastet. Verschiebe es und versuche es erneut.
install-refuse-points-elsewhere = { $link } zeigt bereits auf { $to }. Entferne ihn, falls das eine alte Kopie von Schemaic ist, und versuche es erneut.
install-refuse-unlink-elsewhere = { $link } zeigt auf { $to }, nicht auf diese Kopie von Schemaic, daher hat Entfernen ihn nicht angetastet.
install-refuse-unlink-not-ours = { $link } ist kein Link, den Schemaic angelegt hat, daher hat Entfernen es nicht angetastet.
install-report-already = Der Befehl schemaic ist bereits in deinem PATH, in { $dir }.
install-report-added = { $dir } wurde deinem Benutzer-PATH hinzugefügt. Öffne ein neues Terminal, um schemaic zu verwenden — ein bereits offenes behält den alten PATH.
install-report-linked = { $link } wurde mit { $target } verlinkt.
install-report-linked-if-not-found = { $link } wurde mit { $target } verlinkt. Falls ein neues Terminal schemaic nicht findet, füge { $dir } in deinem Shell-Profil zum PATH hinzu.
install-report-linked-not-on-path = { $link } wurde mit { $target } verlinkt. { $dir } ist noch nicht in deinem PATH — füge es in deinem Shell-Profil hinzu und öffne dann ein neues Terminal.
install-report-removed = { $dir } wurde aus deinem Benutzer-PATH entfernt. Ein bereits offenes Terminal behält den alten PATH.
install-report-path-absent = { $dir } ist nicht in deinem Benutzer-PATH, es gab also nichts zu entfernen.
install-report-unlinked = { $link } wurde entfernt.
install-report-link-absent = Es gibt kein { $link }, es gab also nichts zu entfernen.
install-report-package = Der Befehl schemaic in { $dir } kam mit dem installierten Paket, nicht über Installieren — entferne das Paket, um ihn zu entfernen.
install-hint-adds-folder = Schemaic im Terminal über seinen Namen starten. Fügt seinen Ordner deinem Benutzer-PATH hinzu.
install-hint-links = Schemaic im Terminal über seinen Namen starten. Verlinkt es nach ~/.local/bin.
install-hint-folder-on-path = Schemaic im Terminal über seinen Namen starten. Sein Ordner, { $dir }, ist bereits in deinem Benutzer-PATH.
install-hint-comes-with-app = Schemaic im Terminal über seinen Namen starten. Der Befehl schemaic kommt mit der App, unter { $path }.
tunnel-still-opening = Der SSH-Tunnel ist noch nicht aufgebaut
tunnel-could-not-open = Der SSH-Tunnel konnte nicht geöffnet werden: { $error }. Aktualisiere das Schema, um es erneut zu versuchen.
tunnel-server-failed = Die Verbindung durch den SSH-Tunnel ist fehlgeschlagen: { $error }. Aktualisiere das Schema, um es erneut zu versuchen.
gate-plan-not-connected = Nicht verbunden – der Plan wurde nicht ausgeführt. Der Server hat auf die Verbindungsprüfung nicht geantwortet, daher wurde nichts an ihn gesendet.
gate-plan-connection-changed = Du hast die Verbindung gewechselt, während Schemaic geprüft hat, ob sie erreichbar ist, daher wurde der Plan nicht ausgeführt. Wechsle zurück und frag erneut.
gate-plan-tab-moved-on = Du hast den Tab gewechselt, während die Verbindung erneut geprüft wurde, daher wurde der Plan nicht ausgeführt. Er beschreibt den Tab, in dem er angefordert wurde – geh zu diesem Tab zurück und frag erneut.
gate-connection-changed = Nicht auf { $name } ausgeführt – du hast die Verbindung gewechselt, während Schemaic geprüft hat, ob sie erreichbar ist. Wechsle zurück und versuch es erneut.
gate-not-connected = Nicht mit { $name } verbunden. Der Server hat nicht geantwortet – prüfe, ob er läuft und ob die Einstellungen dieser Verbindung stimmen.
gate-this-connection = diese Verbindung
gate-run-tab-moved-on = Du hast den Tab gewechselt, während die Verbindung erneut geprüft wurde, daher wurde die Anweisung nicht ausgeführt. Sie wurde für den Tab geprüft, in dem sie geschrieben wurde, und hier würde sie woanders laufen. Geh zu diesem Tab zurück und führe sie erneut aus.
gate-commit-not-connected = Nicht verbunden – das Commit wurde nicht versucht. Die vorgemerkten Änderungen bleiben erhalten.
term-shell-failed = { $program } konnte nicht gestartet werden: { $error }. Wähle eine andere Shell unter Einstellungen, Terminal.
term-cli-read-only = Diese Verbindung ist in Schemaic schreibgeschützt. Ihr Kommandozeilen-Client wäre es nicht, daher wird er hier nicht geöffnet.
backup-read-only-server = Diese Verbindung ist in Schemaic schreibgeschützt, und das Backup dieser Engine schreibt der Server, daher wurde keins erstellt.
backup-dialog-title = Datenbank sichern
backup-kind-pg-dump = PostgreSQL-Dump im Custom-Format
backup-kind-sql = SQL-Skript
backup-kind-sqlite = SQLite-Datenbank
backup-not-started = Das Backup wurde nicht gestartet: { $error }
backup-done = Gesichert nach { $path }.
backup-failed = Das Backup ist fehlgeschlagen: { $error }
restore-title = Aus Datei wiederherstellen
restore-confirm = { $path } in { $database } auf { $connection } ausführen? Was dabei angelegt oder ersetzt wird, lässt sich nicht rückgängig machen. Es läuft im Terminal, wo Strg+C es abbricht.
restore-read-only = Diese Verbindung ist in Schemaic schreibgeschützt, daher wird nichts in sie wiederhergestellt.
restore-engine-not-here = Die Backups dieser Engine werden nicht von hier aus wiederhergestellt.
restore-unreadable = { $path } konnte vor dem Wiederherstellen nicht gelesen werden, daher wurde nichts ausgeführt: { $error }
restore-refused = { $path } wechselt beim Ausführen in eine andere Datenbank — { $switches } —, in { $database } wiederhergestellt würde es also dorthin schreiben. Es wurde nichts ausgeführt. Stelle es in der Datenbank wieder her, die es nennt, oder entferne diese Zeilen aus einer Kopie der Datei.
restore-refused-more = { $path } wechselt beim Ausführen in eine andere Datenbank — { $switches } und { $more } weitere —, in { $database } wiederhergestellt würde es also dorthin schreiben. Es wurde nichts ausgeführt. Stelle es in der Datenbank wieder her, die es nennt, oder entferne diese Zeilen aus einer Kopie der Datei.
restore-switch = { $how } `{ $name }` in Zeile { $line }
restore-switch-unnamed = ein { $how } in Zeile { $line }, das keine Datenbank nennt, die Schemaic lesen kann
tab-close-title = „{ $name }“ schließen
tab-close-unsaved = „{ $name }“ hat ungespeicherte Änderungen. Beim Schließen gehen sie verloren; die Datei auf der Festplatte bleibt, wie sie ist. Trotzdem schließen?
tab-close-all-title = Alle Tabs schließen
tab-close-all-question = Möchtest du wirklich alle Tabs schließen?
tab-close-others-title = Andere Tabs schließen
tab-close-others-question = Möchtest du wirklich alle anderen Tabs schließen?
file-open-title = SQL-Datei öffnen
file-save-title = SQL-Datei speichern
file-sql-filter = SQL-Skript
file-read-failed = { $path } konnte nicht gelesen werden: { $error }
file-save-failed = { $path } konnte nicht gespeichert werden: { $error }
file-too-large = { $path } ist { $size } groß – zu groß für einen Editor-Tab. Schemaic wäre die meiste Zeit damit beschäftigt, sie neu zu analysieren. Führe sie aus einem Abfrage-Tab aus, oder nutze Import für eine Datendatei.
file-too-large-short = Die Datei ist zu groß zum Öffnen.
file-large-title = Große Datei öffnen?
file-large-question = „{ $name }“ ist { $size } groß. Schemaic analysiert das ganze Dokument kurz nach jeder Tipppause neu, daher reagiert der Editor bei einer Datei dieser Größe träge, solange der Tab offen ist. Trotzdem öffnen?
file-changed-on-disk = { $path } wurde auf der Festplatte geändert, seit sie geöffnet wurde. Jetzt zu speichern würde diese Änderungen verwerfen – lade die Datei stattdessen neu (oder nutze Speichern unter mit einem anderen Namen).
file-lossy-title = Unlesbare Bytes ersetzen?
file-lossy-question = Schemaic konnte nicht jedes Byte von „{ $name }“ als Text lesen und hat diese Bytes als „�“ angezeigt. Beim Speichern wird geschrieben, was du siehst, daher wird jedes davon dauerhaft ersetzt – in der ganzen Datei, nicht nur in den Zeilen, die du bearbeitet hast. Trotzdem speichern?
file-replace-title = Datei ersetzen?
file-replace-question = „{ $name }“ existiert bereits – „{ $typed }“ wurde mit angehängter Endung .sql gespeichert. Ersetzen?
file-reload-title = Von der Festplatte neu laden
file-reload-unsaved = „{ $name }“ hat ungespeicherte Änderungen. Beim Neuladen gehen sie verloren. Trotzdem neu laden?
export-already-running = Es läuft bereits ein Export. Brich ihn ab oder warte, bis er fertig ist.
export-failed = Export fehlgeschlagen: { $error }
export-worker-died = Export fehlgeschlagen: Hintergrundprozess abgestürzt: { $error }
export-not-published = Export fehlgeschlagen: { $path } konnte nicht veröffentlicht werden: { $error }
export-not-renamed = Der Export hat { $part } geschrieben, konnte es aber nicht in { $path } umbenennen: { $error }
export-nothing-matched = Nichts zu exportieren — keine Tabelle entsprach der Auswahl.
export-stopped = Export abgebrochen.
dump-failed = Dump fehlgeschlagen: { $error }
blob-load-too-large = Diese Datei ist { $size } groß – höchstens { $most } lassen sich laden.
blob-load-failed = Laden fehlgeschlagen: { $error }
blob-save-failed = Speichern fehlgeschlagen: { $error }
file-write-failed = { $path } konnte nicht geschrieben werden: { $error }
settings-export-title = Einstellungen exportieren
settings-import-title = Einstellungen importieren
settings-file-filter = Schemaic-Einstellungen
settings-import-nothing = Nichts zu importieren: Jede Einstellung in { $path } stimmt bereits mit deiner überein, und es kommen keine Snippets hinzu.
settings-import-question = { $path } ändert { $changed ->
        [one] { $changed } Einstellung
       *[other] { $changed } Einstellungen
    } und fügt { $snippets ->
        [one] { $snippets } Snippet
       *[other] { $snippets } Snippets
    } hinzu. Die geänderten Einstellungen ersetzen deine aktuellen Werte; die Snippets kommen neben deine.
settings-import-unreadable = konnte nicht gelesen werden: { $error }
connections-export-title = Verbindungen exportieren
connections-file-filter = Schemaic-Verbindungen
connections-replace-title = „{ $file }“ ersetzen
connections-replace-question = Eine Datei namens „{ $file }“ liegt bereits in diesem Ordner. Der Export hängt diese Endung an den eingegebenen Namen an, daher hat der Speicherdialog nicht danach gefragt. Das Ersetzen lässt sich nicht rückgängig machen.
import-choose-file-title = Verbindungsdatei wählen
import-no-containers = Es laufen keine Datenbank-Container.
import-added = { $count ->
        [one] { $count } Verbindung hinzugefügt.
       *[other] { $count } Verbindungen hinzugefügt.
    }
import-added-foreign = { $count ->
        [one] { $count } Verbindung hinzugefügt.
       *[other] { $count } Verbindungen hinzugefügt.
    } Schreibschutz und Umgebungs-Badge werden aus einem anderen Client nicht übernommen – lege sie unter Verbindungen verwalten für alles fest, worauf es ankommt.
conn-delete-title = „{ $name }“ löschen
conn-delete-question = Dies löscht die Verbindung, ihr gespeichertes Passwort, ihre gespeicherte KI-Unterhaltung, ihren Abfrageverlauf und ihre Tabs. Das lässt sich nicht rückgängig machen.
conn-delete-keyring-kept = Schemaic konnte den Schlüsselbund des Betriebssystems nicht erreichen, daher sind das gespeicherte Passwort und die SSH-Geheimnisse der gelöschten Verbindung **noch darin**.
conn-delete-keyring-later = Sie werden entfernt, sobald das nächste Mal eine Verbindung gelöscht oder gespeichert wird, während der Schlüsselbund erreichbar ist. Bis dahin kann keine neue Verbindung sie erhalten – Verbindungs-IDs werden nie wiederverwendet.
notice-unreadable = { $name } konnte nicht gelesen werden ({ $error }).
notice-unreadable-kept = Die unlesbare Datei wurde als { $name }.corrupt aufbewahrt; Schemaic ist auf ihre Sicherung zurückgefallen, oder auf die Standardwerte, falls auch die Sicherung unlesbar war.
notice-unreadable-swept = Diese Kopie kann Passwörter im Klartext enthalten und wird daher entfernt, wenn du das nächste Mal eine Verbindung löschst – hol dir vorher alles heraus, was du daraus brauchst.
notice-missing = { $name } fehlte.
notice-missing-restored = Schemaic hat sie aus { $name }{ $from } wiederhergestellt, das noch vorhanden war. Sie wird zurückgeschrieben, sobald sich diese Einstellung das nächste Mal ändert – bis dahin ist diese Kopie die einzige.
notice-keyring-unread = { $n ->
    [one] Schemaic konnte { $n } gespeichertes Geheimnis nicht aus dem Schlüsselbund des Betriebssystems lesen.
   *[other] Schemaic konnte { $n } gespeicherte Geheimnisse nicht aus dem Schlüsselbund des Betriebssystems lesen.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic konnte { $n } gespeichertes Geheimnis nicht aus dem Schlüsselbund des Betriebssystems lesen ({ $error }).
   *[other] Schemaic konnte { $n } gespeicherte Geheimnisse nicht aus dem Schlüsselbund des Betriebssystems lesen ({ $error }).
}
notice-keyring-unread-kept = Diese Passwörter sind in dieser Sitzung nicht verfügbar und wurden **nicht** gelöscht – lass die Felder leer, dann kommen sie zurück, sobald der Schlüsselbund erreichbar ist. Ein neu eingegebenes überschreibt das gespeicherte Geheimnis.
notice-in-the-clear = { $n ->
    [one] Schemaic konnte den Schlüsselbund des Betriebssystems nicht erreichen, daher wurde { $n } Passwort **im Klartext** in `connections.json` gespeichert.
   *[other] Schemaic konnte den Schlüsselbund des Betriebssystems nicht erreichen, daher wurden { $n } Passwörter **im Klartext** in `connections.json` gespeichert.
}
notice-in-the-clear-where = Diese Datei liegt in dem Ordner, den die Schaltfläche *Ordner öffnen* unter Einstellungen → Allgemein öffnet, neben dem Protokoll – prüfe sie, bevor du sie weitergibst. Erneutes Speichern, sobald der Schlüsselbund erreichbar ist, verschiebt sie wieder dorthin.
notice-undeleted = { $n ->
    [one] { $n } geleertes Passwort ist noch im Schlüsselbund des Betriebssystems – Schemaic konnte es nicht löschen.
   *[other] { $n } geleerte Passwörter sind noch im Schlüsselbund des Betriebssystems – Schemaic konnte sie nicht löschen.
}
notice-undeleted-after = Das Feld ist auf der Festplatte leer, aber der nächste Start füllt es wieder. Speichere erneut, sobald der Schlüsselbund erreichbar ist.
inline-ai-no-sql = Kein SQL zurückgegeben
inline-ai-not-sql = Das Modell hat kein SQL zurückgegeben
inline-ai-failed = Generierung fehlgeschlagen
ai-fill-no-value = Die KI hat keinen Wert zurückgegeben.
conn-gone = Die Verbindung existiert nicht mehr
tx-session-not-ready = Die Transaktionsverbindung ist nicht bereit – wechsle zu Auto-Commit und zurück
tx-session-open-failed = Die Transaktionsverbindung konnte nicht geöffnet werden: { $error }
tx-cancelled = Die Anweisung wurde abgebrochen.
tx-committed-before-ddl = Die Transaktion, in der dies lief, ist beendet: MySQL schreibt fest, bevor es eine DDL-Anweisung ausführt, daher sind die Anweisungen darin dauerhaft, und Rollback macht sie nicht rückgängig.
tx-rolled-back-by-server = Die Transaktion, in der dies lief, ist beendet: SQL Server hat sie zurückgerollt, daher ist jede Anweisung darin seit ihrem Beginn rückgängig gemacht.
tx-doomed = Die Transaktion, in der dies lief, kann nicht mehr festgeschrieben werden: SQL Server hat sie als nicht festschreibbar markiert, daher bleibt nur Rollback, und das macht jede Anweisung darin rückgängig.
tx-after-own-commit = Die Transaktion, in der dies lief, ist beendet, und dies hat vor dem Fehler ein eigenes COMMIT ausgeführt, daher ist, was vor diesem COMMIT kam, wahrscheinlich festgeschrieben statt rückgängig gemacht. Prüfe die Daten, bevor du etwas davon erneut ausführst.
tx-lost-on-commit = Die Verbindung ging bei offener Transaktion verloren. Sie ging bei einem COMMIT verloren, daher ist unbekannt, ob { $stmts ->
        [0] die Transaktion
        [one] die { $stmts } Anweisung, die seit ihrem Beginn darin lief,
       *[other] die { $stmts } Anweisungen, die seit ihrem Beginn darin liefen,
    } festgeschrieben oder zurückgerollt wurde: Der Server hat möglicherweise festgeschrieben, bevor die Verbindung abbrach. Prüfe die Daten, bevor du etwas davon erneut ausführst.
tx-lost-on-implicit-commit = Die Verbindung ging bei offener Transaktion verloren. MySQL schreibt eine offene Transaktion fest, bevor es diese Anweisung ausführt, daher { $stmts ->
        [0] lief darin noch nichts, und es gab nichts festzuschreiben
        [one] wurde die { $stmts } Anweisung, die seit ihrem Beginn darin lief, höchstwahrscheinlich dabei festgeschrieben und ist dauerhaft
       *[other] wurden die { $stmts } Anweisungen, die seit ihrem Beginn darin liefen, höchstwahrscheinlich dabei festgeschrieben und sind dauerhaft
    }. Prüfe die Daten, bevor du etwas davon erneut ausführst.
tx-lost-rolled-back = Die Verbindung ging bei offener Transaktion verloren. Der Server rollt eine unvollendete Transaktion zurück, wenn ihre Verbindung endet, daher { $stmts ->
        [0] lief darin noch nichts
        [one] ist die { $stmts } Anweisung, die seit ihrem Beginn darin lief, rückgängig gemacht
       *[other] sind die { $stmts } Anweisungen, die seit ihrem Beginn darin liefen, rückgängig gemacht
    }.
tx-lost-rolled-back-mysql = Die Verbindung ging bei offener Transaktion verloren. Der Server rollt eine unvollendete Transaktion zurück, wenn ihre Verbindung endet, daher { $stmts ->
        [0] lief darin noch nichts
        [one] ist die { $stmts } Anweisung, die seit ihrem Beginn darin lief, rückgängig gemacht — außer einem Schreibzugriff auf eine Tabelle ohne Transaktionen (MyISAM, MEMORY), den kein Rollback rückgängig macht
       *[other] sind die { $stmts } Anweisungen, die seit ihrem Beginn darin liefen, rückgängig gemacht — außer einem Schreibzugriff auf eine Tabelle ohne Transaktionen (MyISAM, MEMORY), den kein Rollback rückgängig macht
    }.
tx-not-sent-timeout = Das Anweisungs-Timeout lief ab, während dieser Lauf noch auf die Verbindung des Tabs wartete. Ein früherer Vorgang darauf — ein Commit oder das erneute Abrufen danach — war nicht beendet, daher wurde die Anweisung nie an den Server gesendet. Nichts lief, und die Transaktion ist genau, wo sie war.
tx-not-sent-cancelled = Dieser Lauf wurde abgebrochen, während er noch auf die Verbindung des Tabs wartete. Ein früherer Vorgang darauf — ein Commit oder das erneute Abrufen danach — war nicht beendet, daher wurde die Anweisung nie an den Server gesendet. Nichts lief, und die Transaktion ist genau, wo sie war.
snippet-default-name = Snippet
snippet-delete-title = Snippet löschen
snippet-delete-question = „{ $name }“ löschen? Das lässt sich nicht rückgängig machen.
copy-target-read-only = Die Zielverbindung ist schreibgeschützt, daher wurden keine Zeilen kopiert.
copy-target-gone = Die Zielverbindung existiert nicht mehr, daher wurden keine Zeilen kopiert.
copy-not-started = Es wurden keine Zeilen kopiert: { $error }
copy-counter-not-moved = { $count ->
        [one] { $count } Zeile wurde kopiert, aber der Zähler der Tabelle wurde nicht über sie hinaus verschoben, daher kann die nächste Einfügung kollidieren: { $error }
       *[other] { $count } Zeilen wurden kopiert, aber der Zähler der Tabelle wurde nicht über sie hinaus verschoben, daher kann die nächste Einfügung kollidieren: { $error }
    }
copy-stopped = Das Kopieren wurde wegen eines Fehlers abgebrochen: { $error }
compare-reading-failed = { $database } lesen: { $error }
monitor-no-row-key = Kein Zeilenschlüssel für diese Tabelle – Änderungen lassen sich nicht verfolgen.
monitor-restarted = Die Spalten der Tabelle haben sich geändert, daher wurde die Änderungsverfolgung ab hier neu gestartet.
timeout-cancelled-app = Abgebrochen: Die Anweisung lief länger als das Anweisungs-Timeout von { $timeout }. Ändere oder deaktiviere es unter Einstellungen → Abfrage.
timeout-cancelled-connection = Abgebrochen: Die Anweisung lief länger als das Anweisungs-Timeout von { $timeout }. Ändere oder deaktiviere es in den Einstellungen dieser Verbindung.
ai-chat-no-database = Die Datenbank ist nicht erreichbar, daher kann der Assistent für diese Verbindung nicht gestartet werden. Prüfe die Verbindung und versuch es erneut.
ai-no-cwd = **Der Assistent konnte nicht starten.** Schemaic gibt jedem Agentenprozess ein eigenes privates Arbeitsverzeichnis, aber weder das temporäre Verzeichnis des Systems noch das Konfigurationsverzeichnis hat eins aufgenommen – es gibt also keinen Ort, an dem er laufen könnte, der nicht das Verzeichnis von jemand anderem ist. Prüfe, ob das temporäre Verzeichnis beschreibbar ist, und versuch es erneut.
ai-no-reply-file = Es konnte keine temporäre Datei für die Antwort angelegt werden.
ai-inline-no-opencode-config = Die Konfiguration von OpenCode konnte nicht geschrieben werden, daher wurde die Generierung nicht gestartet (ohne sie liefe die CLI mit ihren eigenen Werkzeugen).
ai-inline-no-copilot-home = Das private Home-Verzeichnis für GitHub Copilot konnte nicht angelegt werden, daher wurde die Generierung nicht gestartet (ohne es würde die CLI deine eigene Copilot-Konfiguration laden, samt ihrer MCP-Server und Hooks).
ai-inline-no-env-writer = { $cli } gilt als durch seine Umgebung isoliert, aber Schemaic kann diese Umgebung nicht einrichten, daher wurde die Generierung nicht gestartet.
ai-inline-no-cursor-rules = Die Berechtigungsdatei von Cursor konnte nicht geschrieben werden, daher wurde die Generierung nicht gestartet (ohne sie schreibt die CLI ungefragt Dateien).
ai-inline-no-rules-writer = { $cli } gilt als durch eine Berechtigungsdatei eingeschränkt, aber Schemaic kann keine dafür schreiben, daher wurde die Generierung nicht gestartet.
ai-no-tools = **Diese Sitzung hat keine Datenbank-Werkzeuge.** { $cause }, daher kann der Assistent nichts nachschlagen – er kann nur aus der Schema-Übersicht antworten, die schon in seinem Prompt steht. Schließen und erneutes Öffnen des Bereichs versucht es noch einmal.
ai-no-endpoint-file = Schemaic konnte die private Datei nicht anlegen, die dem Assistenten sagt, wie er deine Datenbank erreicht
ai-cursor-not-approved = Schemaic konnte seine Datenbank-Werkzeuge bei Cursor nicht freigeben ({ $error })
ai-antigravity-not-registered = Schemaic konnte seine Datenbank-Werkzeuge bei Antigravity nicht registrieren
ai-antigravity-shared = Ein anderes Schemaic-Fenster nutzt Antigravity bereits. Antigravity hat eine einzige MCP-Registrierung für den ganzen Rechner, die nur auf eine Verbindung zeigen kann, daher hat der Assistent dieses Fensters keine Datenbank-Werkzeuge
ai-antigravity-not-claimed = Schemaic konnte die Konfiguration von Antigravity für diese Sitzung nicht übernehmen
ai-refused-no-opencode-config = Schemaic konnte die Konfiguration, die OpenCode einschränkt, nicht schreiben, daher ist der Assistent deaktiviert – ohne diese Datei hätte die Sitzung eine Shell. Prüfe, ob das Datenverzeichnis der App beschreibbar ist.
ai-refused-no-copilot-home = Schemaic konnte das private Home-Verzeichnis für GitHub Copilot nicht anlegen, daher ist der Assistent deaktiviert – ohne es würde die CLI deine eigene Copilot-Konfiguration laden, samt ihrer MCP-Server und Hooks. Prüfe, ob das Datenverzeichnis der App beschreibbar ist.
ai-refused-no-cursor-rules = Schemaic konnte die Berechtigungsdatei nicht schreiben, die Cursor daran hindert, Dateien zu schreiben und Befehle auszuführen, daher ist der Assistent deaktiviert – ohne sie schreibt Cursor ungefragt Dateien. Prüfe, ob das Datenverzeichnis der App beschreibbar ist.
ai-launch-failed-turn = Die CLI `{ $bin }` konnte nicht gestartet werden ({ $error }). Stelle sicher, dass { $cli } installiert ist, oder gib unter Einstellungen → KI einen Pfad an.
ai-launch-failed-session = Die CLI `{ $bin }` konnte nicht gestartet werden ({ $error }). Stelle sicher, dass { $cli } installiert ist (oder lege ihren Pfad unter Einstellungen → KI fest).
ai-turn-ended = Der Durchgang von { $cli } endete unerwartet: { $error }
ai-session-ended-badly = Die KI-Sitzung endete unerwartet: { $error }
ai-session-ended = Die Sitzung von { $cli } ist beendet. Frag erneut, um eine neue zu starten.
ai-session-restart-failed = Die Sitzung von { $cli } konnte nach dem Anhalten nicht neu gestartet werden ({ $error }), daher ist sie beendet. Frag erneut, um eine neue zu starten.
ai-session-not-accepting = Die Sitzung von { $cli } nimmt keine Fragen mehr an. Frag erneut, um eine neue zu starten.
import-source-not-a-file = { $path } ist keine Datei.
import-source-too-big = { $path } ist { $size } groß, mehr als die { $most }, die hier gelesen werden.
import-source-unreadable = { $path } konnte nicht gelesen werden: { $error }
docker-not-found = Docker wurde nicht gefunden – weder im PATH noch in WSL.
docker-not-started = Docker konnte nicht gestartet werden: { $error }
docker-timed-out = Docker hat nicht innerhalb von { $secs } Sekunden geantwortet.
docker-not-waited = Auf Docker konnte nicht gewartet werden: { $error }
docker-answered = Docker hat geantwortet: { $error }
docker-exited = Docker wurde mit { $status } beendet.
script-reader-stopped = Das Lesen der Datei brach unerwartet ab: { $error }
script-changed-since-read = Diese Datei wurde nach dem Lesen auf der Festplatte geändert, daher ist die Zusammenfassung oben nicht das, was laufen würde. Wähle sie erneut.
ai-cursor-enable-unfinished = `mcp enable` wurde nicht abgeschlossen
