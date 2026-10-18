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
