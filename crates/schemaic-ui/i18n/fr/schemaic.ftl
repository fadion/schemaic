# Schemaic — French catalogue.
#
# Machine-drafted and unreviewed (docs/architecture.md, The string catalogue); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to French.

## Settings › Appearance › Language

settings-language = Langue
settings-language-system = Système ({ $language })
settings-language-restart = Le changement de langue prendra effet au prochain démarrage de Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Taille de police
settings-term-cursor-style = Style du curseur
settings-term-copy-on-select = Copier à la sélection
settings-term-copy-on-select-hint = Copier le texte sélectionné dans le presse-papiers dès la fin de la sélection.
settings-term-blink = Curseur clignotant
settings-term-blink-hint = Faire clignoter le curseur quand le terminal a le focus.

## Settings › AI

settings-ai-agent = CLI d'agent
settings-ai-disabled = Désactivé
settings-ai-off-hint = Rien n'est envoyé à aucun agent, aucune CLI d'agent n'est lancée et aucune action d'IA n'est proposée. Choisissez une CLI pour activer l'assistant.
settings-ai-cli-path = Chemin de { $cli }
settings-ai-cli-path-placeholder = Laisser vide pour la détection automatique
settings-ai-detected = Détecté automatiquement : { $path }
settings-ai-detect-failed = La détection automatique a échoué. { $cli } introuvable — indiquez son chemin ci-dessus.
settings-ai-cli-path-missing = Le fichier n'existe pas.
settings-ai-model = Modèle
settings-ai-model-placeholder = Laisser vide pour le modèle par défaut de l'outil d'agent
settings-ai-model-hint = Tout identifiant accepté par cette CLI — un alias, ou un instantané daté pour en garder un d'une mise à jour à l'autre.
settings-ai-model-hint-with-chips = Tout identifiant accepté par cette CLI — un alias, ou un instantané daté pour en garder un d'une mise à jour à l'autre. Les boutons sont des raccourcis, pas la liste complète.
settings-ai-effort = Effort
settings-ai-instructions = Instructions personnalisées
settings-ai-instructions-placeholder = Dialecte, conventions, règles maison…
settings-ai-schema-context = Contexte du schéma
settings-ai-schema-context-hint = La part de la structure de la base de données jointe à chaque message. Aucun retire aussi les outils de schéma : l'assistant vous demande alors les noms au lieu de lire lui-même le catalogue.
settings-ai-data-access = Accès aux données
settings-ai-data-access-where = Se règle par connexion, dans les paramètres de la connexion.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name } : { $level } — { $hint }
ai-data-schema-only = Schéma uniquement
ai-data-on-request = Seulement ce que je joins
ai-data-full = Lui laisser lire les données
ai-data-schema-only-hint = La structure uniquement — noms, types et définitions. Aucune ligne ne quitte jamais cette machine, et les pièces jointes sont refusées.
ai-data-on-request-hint = L'assistant ne lit aucune donnée de lui-même. Les lignes que vous joignez depuis un résultat quittent cette machine avec la question.
ai-data-full-hint = L'assistant peut exécuter des requêtes en lecture seule et lire des lignes d'exemple de lui-même. Tout ce qu'il lit quitte cette machine.
settings-ai-gutter = Filet de couleur sur les réponses
settings-ai-gutter-hint = Marquer les réponses de l'assistant d'un filet coloré le long de leur bord droit. Désactivé, elles ont la même marge des deux côtés.

## Settings › section list

settings-section-general = Général
settings-section-editor = Éditeur
settings-section-formatting = Mise en forme du code
settings-section-query = Requête
settings-section-terminal = Terminal
settings-section-ai = Assistant IA
settings-section-appearance = Apparence

## Settings › shared

settings-off = Désactivé

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } caractère
   *[other] { $n } caractères
}

## Settings › General

settings-log = Fichier journal
settings-log-hint = Les diagnostics, y compris les plantages, sont écrits dans { $path }.
settings-log-hint-none = Aucun fichier journal n'a pu être ouvert sur cette machine ; rien n'est donc écrit dans un journal.
settings-log-open-folder = Ouvrir le dossier
settings-bundle = Fichier de paramètres
settings-bundle-hint = Vos préférences et vos extraits dans un seul fichier, à emporter sur une autre machine. Les connexions, la disposition des fenêtres, le shell du terminal et les extraits propres à une connexion restent ici.
settings-bundle-export = Exporter
settings-bundle-import = Importer
settings-cli = Ligne de commande
settings-cli-install = Installer
settings-cli-installing = Installation
settings-cli-remove = Supprimer
settings-cli-removing = Suppression

## Settings › modal

settings-title = Paramètres
settings-restore-tabs = Restaurer les onglets au démarrage
settings-restore-tabs-hint = Rouvrir les onglets de requête de votre dernière session au démarrage de l'application.

## Settings › Editor

settings-editor-font-size = Taille de police
settings-editor-predictions = Prédictions en ligne
settings-editor-predictions-hint = Afficher les mots suivants probables après le curseur — le complément d'un mot-clé, la clé étrangère d'une jointure, les colonnes à regrouper ou à insérer. Tab, ou → en fin de ligne, les accepte en entier, Ctrl+→ (⌘→ sous macOS) mot par mot, et Échap les refuse.
settings-editor-remembered = Prédictions tirées des instructions passées
settings-editor-remembered-hint = Proposer aussi la suite d'une ligne tirée d'une instruction que vous avez exécutée, écrite dans un autre onglet ou enregistrée comme extrait. Désactivez pour ne pas afficher les instructions passées.
settings-editor-aliases = Alias de table
settings-editor-aliases-hint = Quand la complétion écrit une table après FROM ou JOIN, la faire suivre d'un alias court formé de ses initiales — orders o, order_items oi — et utiliser cet alias dans la condition ON d'une jointure.
settings-editor-rainbow = Parenthèses arc-en-ciel
settings-editor-rainbow-hint = Colorer chaque paire de parenthèses selon sa profondeur d'imbrication, pour associer d'un coup d'œil une parenthèse fermante à son ouvrante.
settings-editor-whitespace = Afficher les espaces
settings-editor-whitespace-hint = Dessiner les espaces et les tabulations sous forme de marques discrètes, pour distinguer une tabulation d'espaces.
settings-editor-guides = Guides d'indentation
settings-editor-guides-hint = Dessiner une ligne verticale discrète à chaque niveau d'indentation.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Casse des mots-clés
settings-format-case-keep = Telle que saisie
settings-format-case-upper = Majuscules
settings-format-case-lower = Minuscules
settings-format-commas-trailing = En fin de ligne
settings-format-commas-leading = En début de ligne
settings-format-gap-one = Une ligne vide
settings-format-gap-none = Aucune ligne vide
settings-format-keyword-case-hint = Comment Mettre en forme le code écrit les mots-clés tels que SELECT et FROM. Les noms, les chaînes et les commentaires gardent leur casse.
settings-format-function-case = Casse des fonctions
settings-format-function-case-hint = Comment Mettre en forme le code écrit les fonctions propres au moteur, telles que COUNT et COALESCE, là où elles sont appelées. Vos propres fonctions gardent leur casse.
settings-format-type-case = Casse des types
settings-format-type-case-hint = Comment Mettre en forme le code écrit les types de données propres au moteur, tels que INT et VARCHAR. Les types que vous avez créés gardent leur casse.
settings-format-commas = Virgules
settings-format-commas-hint = Où Mettre en forme le code place la virgule d'une liste qu'il répartit sur plusieurs lignes. Les listes gardées sur une ligne, comme les arguments d'une fonction, ne sont pas touchées, sauf si la largeur de retour à la ligne les coupe.
settings-format-statement-gap = Entre les instructions
settings-format-statement-gap-hint = Les lignes vides que Mettre en forme le code laisse entre deux instructions. Telle que saisie garde une ligne vide partout où il y en avait une.
settings-format-wrap = Largeur de retour à la ligne
settings-format-wrap-hint = Une liste que Mettre en forme le code garde sur une ligne, comme les arguments d'une fonction, passe à un élément par ligne dès que sa ligne dépasserait cette largeur.
settings-format-river = Aligner à droite les mots-clés des clauses
settings-format-river-hint = Aligner SELECT, FROM, WHERE et les autres sur leur bord droit, le corps de chaque clause à côté de son mot-clé, au lieu de placer un mot-clé seul sur sa ligne.
settings-format-align = Aligner les colonnes
settings-format-align-hint = Aligner les AS des alias d'une liste de sélection, les = des affectations d'un UPDATE et les types des colonnes d'une table.
settings-format-join-on = Condition de jointure sur sa propre ligne
settings-format-join-on-hint = Placer le ON d'une jointure, ainsi que les AND et OR de sa condition, sur des lignes à part sous la jointure.

## Settings › Query

settings-query-row-limit = Limite de lignes par défaut
settings-query-timeout = Délai d'expiration des instructions
settings-query-timeout-hint = Annuler une instruction qui s'exécute plus longtemps que cela. Désactivé par défaut — un import ou un rapport peut légitimement prendre du temps.
timeout-off = Aucun délai
timeout-hours = { $n ->
    [one] { $n } heure
   *[other] { $n } heures
}
timeout-minutes = { $n ->
    [one] { $n } minute
   *[other] { $n } minutes
}
timeout-seconds = { $n ->
    [one] { $n } seconde
   *[other] { $n } secondes
}
settings-query-confirm-writes = Confirmer avant d'exécuter des écritures
settings-query-confirm-writes-hint = Demander avant d'exécuter toute instruction qui modifie les données ou le schéma.
settings-query-validate = Validation en direct par la base de données
settings-query-validate-hint = Vérifier l'instruction sous le curseur auprès de la base de données pendant la saisie (un PREPARE sans exécution) pour faire apparaître les erreurs exactes. Ajoute un aller-retour vers la base à chaque pause.
settings-query-notify = Avertir quand une exécution se termine en arrière-plan
settings-query-notify-hint = Afficher une notification de bureau quand une requête, un script, un export ou un dump ayant duré une seconde ou plus se termine alors que Schemaic n'est pas la fenêtre active. Le bouton de la barre des tâches est mis en évidence dans tous les cas. La notification ne contient jamais le SQL.
notify-query-finished = Requête terminée
notify-query-failed = Échec de la requête
notify-script-finished = Script terminé
notify-script-failed = Échec du script
notify-export-finished = Export terminé
notify-export-failed = Échec de l'export
notify-dump-finished = Dump terminé
notify-dump-failed = Échec du dump
notify-took = Durée : { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Conserver les valeurs dans le journal des écritures
settings-query-write-log-hint = Chaque écriture exécutée par Schemaic ou schemaic exec est ajoutée à write_log.jsonl dans le dossier des paramètres, et n'est jamais effacée. Les valeurs de son SQL sont masquées sauf si cette option est activée ; un mot de passe est masqué dans tous les cas.

## Settings › Appearance

settings-appearance-ui-theme = Thème de l'interface
settings-appearance-editor-theme = Thème de l'éditeur
settings-appearance-scale = Échelle de l'interface

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Raccourcis

## Shortcuts modal › group headings

shortcuts-group-global = Global
shortcuts-group-editor = Éditeur
shortcuts-group-results-grid = Grille de résultats
shortcuts-group-schema-tree = Arborescence du schéma
shortcuts-group-ai-panel = Panneau IA
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = Diagramme ER

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Rechercher partout
shortcuts-command-palette = Palette de commandes
shortcuts-new-tab = Nouvel onglet de requête
shortcuts-close-tab = Fermer l'onglet de requête
shortcuts-next-tab = Onglet suivant
shortcuts-previous-tab = Onglet précédent
shortcuts-jump-to-tab = Aller à l'onglet
shortcuts-reopen-tab = Rouvrir le dernier onglet fermé
shortcuts-open-file = Ouvrir un fichier SQL
shortcuts-save-file = Enregistrer le fichier SQL
shortcuts-save-file-as = Enregistrer le fichier SQL sous
shortcuts-toggle-schema = Afficher/masquer le panneau du schéma
shortcuts-toggle-ai = Afficher/masquer le panneau IA
shortcuts-toggle-terminal = Afficher/masquer le terminal
shortcuts-run-query = Exécuter la requête
shortcuts-autocomplete = Saisie semi-automatique
shortcuts-inline-ai = Modification IA en ligne
shortcuts-find-in-editor = Rechercher dans l'éditeur
shortcuts-find-replace = Rechercher et remplacer
shortcuts-go-to-line = Aller à la ligne
shortcuts-matching-bracket = Aller à la parenthèse ou au BEGIN/END correspondant
shortcuts-line-comment = Commenter/décommenter la ligne
shortcuts-block-comment = Commenter/décommenter le bloc
shortcuts-toggle-case = Inverser la casse
shortcuts-join-lines = Joindre les lignes
shortcuts-duplicate = Dupliquer la ligne / la sélection
shortcuts-cut = Couper la ligne / la sélection
shortcuts-format-code = Mettre en forme le code
shortcuts-format-statement = Mettre en forme l'instruction
shortcuts-single-line = Mettre sur une seule ligne
shortcuts-move-line = Déplacer la ligne vers le haut / le bas
shortcuts-quick-fix = Appliquer la correction rapide sous le curseur
shortcuts-indent = Indenter / désindenter
shortcuts-take-prediction = Accepter la prédiction en ligne (→ aussi, en fin de ligne) / son mot suivant
shortcuts-find-in-results = Rechercher dans les résultats
shortcuts-go-to-row = Aller à la ligne de résultat
shortcuts-copy = Copier
shortcuts-paste-staged = Coller comme modifications en attente
shortcuts-select-all = Tout sélectionner
shortcuts-first-last-cell = Première / dernière cellule
shortcuts-edit-cell = Modifier la cellule
shortcuts-next-cell = Cellule suivante / précédente pendant la modification
shortcuts-commit-edits = Valider les modifications
shortcuts-mark-deletion = Marquer la ligne pour suppression
shortcuts-results-toolbar = Aller à la barre d'outils des résultats
shortcuts-along-toolbar = Se déplacer dans la barre d'outils (Échap revient à la grille)
shortcuts-open-selected = Ouvrir la table, la colonne ou l'objet sélectionné
shortcuts-context-menu = Menu contextuel de la ligne sélectionnée
shortcuts-send-message = Envoyer le message
shortcuts-new-line = Nouvelle ligne
shortcuts-recall-prompt = Rappeler l'invite précédente / suivante
shortcuts-copy-paste = Copier / coller
shortcuts-find-in-diagram = Rechercher dans le diagramme

## Key names in shortcut labels — a keyboard's own legends, so a language
## whose keyboards print another word uses it (German: Strg, Umschalt, Entf).
## Keys written the same everywhere (letters, arrows, Tab, Esc, F6) and macOS's
## Cmd are not here. Read key by key by `shortcuts::keys_label`.

key-ctrl = Ctrl
key-shift = Maj
key-alt = Alt
key-enter = Entrée
key-del = Suppr
key-home = Origine
key-end = Fin
key-space = Espace

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Afficher/masquer un panneau
palette-toggle-panel-hint = schéma · ia · terminal · historique des requêtes · activité du serveur
palette-panel-schema = Schéma
palette-panel-ai = IA
palette-panel-terminal = Terminal
palette-panel-history = Historique des requêtes
palette-panel-activity = Activité du serveur
palette-new-tab = Nouvel onglet
palette-export-settings = Exporter les paramètres
palette-import-settings = Importer les paramètres
palette-duplicate-tab = Dupliquer l'onglet
palette-close-tab = Fermer l'onglet
palette-reopen-closed-tab = Rouvrir un onglet fermé
palette-reopen-closed-tab-hint = <onglet>
palette-open-file = Ouvrir un fichier
palette-open-file-hint = un script .sql
palette-save-file = Enregistrer le fichier
palette-save-file-as = Enregistrer le fichier sous
palette-next-tab = Onglet suivant
palette-previous-tab = Onglet précédent
palette-format-code = Mettre en forme le code
palette-format-statement = Mettre en forme l'instruction
palette-format-statement-hint = l'instruction au curseur
palette-make-single-line = Mettre sur une seule ligne
palette-make-single-line-hint = la sélection ou l'instruction, sur une ligne
palette-run = Exécuter
palette-run-hint = exécuter toutes les instructions
palette-go-to-line = Aller à la ligne
palette-go-to-line-hint = <numéro>
palette-go-to-statement = Aller à l'instruction
palette-search-hint = <recherche>
palette-history = Historique
palette-clear-history = Effacer l'historique
palette-clear-history-hint = connexion actuelle
palette-ask-ai = Demander à l'IA
palette-ask-ai-hint = <invite>
palette-ask-ai-empty = Saisissez une invite…
palette-terminal = Terminal
palette-terminal-hint = <commande>
palette-run-in-terminal = Exécuter dans le terminal
palette-terminal-empty = Saisissez une commande…
palette-ui-theme = Thème de l'interface
palette-ui-theme-hint = clair · sombre
palette-editor-theme = Thème de l'éditeur
palette-editor-theme-hint = <thème>
palette-font-size = Taille de police
palette-increase-font-size = Augmenter la taille de police
palette-decrease-font-size = Réduire la taille de police
palette-indent-style = Style d'indentation
palette-indent-style-hint = tabulations · espaces
palette-indent-spaces = Espaces
palette-indent-tabs = Tabulations
palette-indent-width = Largeur d'indentation
palette-toggle-word-wrap = Activer/désactiver le retour à la ligne
palette-toggle-whitespace = Afficher/masquer les espaces
palette-toggle-whitespace-hint = afficher les espaces et les tabulations
palette-toggle-indent-guides = Afficher/masquer les guides d'indentation
palette-switch-connection = Changer de connexion
palette-switch-connection-hint = <connexion>
palette-server-variables = Variables du serveur
palette-server-variables-hint = les paramètres de la connexion active
palette-enter-number = Saisissez un nombre
palette-nothing-found = Aucun résultat

## Command palette › what a result row is

palette-row-snippet = extrait
palette-row-open-tab = onglet ouvert
palette-row-history = historique
palette-row-view-body = corps de la vue
palette-row-body = corps ({ $kind })
palette-row-trigger-on = déclencheur sur { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Ouvrir dans la CLI
tree-open = Ouvrir
tree-open-in-new-tab = Ouvrir dans un nouvel onglet
tree-copy-name = Copier le nom
tree-copy-qualified-name = Copier le nom qualifié
tree-properties = Propriétés
tree-used-by = Utilisé par
tree-live-monitor = Moniteur en direct
tree-er-diagram = Diagramme ER
tree-compare-with = Comparer avec
tree-generate = Générer
tree-generate-ddl = Générer le DDL
tree-generate-create = Create
tree-generate-insert = Insert
tree-generate-update = Update
tree-generate-delete = Delete
tree-refresh = Actualiser
tree-collapse-all = Tout réduire
tree-favourite = Ajouter aux favoris
tree-unfavourite = Retirer des favoris
tree-colour = Couleur
tree-colour-none = Aucune
tree-hide = Masquer
tree-import = Importer
tree-export = Exporter
tree-back-up = Sauvegarder
tree-restore-from-file = Restaurer depuis un fichier
tree-copy-to = Copier vers
tree-create = Créer
tree-create-table = Table
tree-create-view = Vue
tree-create-type = Type
tree-create-domain = Domaine
tree-create-sequence = Séquence
tree-create-synonym = Synonyme
tree-create-alias-type = Type alias
tree-create-xml-schema-collection = Collection de schémas XML
tree-create-function = Fonction
tree-create-procedure = Procédure
tree-create-event = Événement
tree-create-extension = Extension
tree-create-database = Base de données
tree-create-schema = Schéma
tree-create-kind = Créer : { $kind }
tree-edit-kind = Modifier : { $kind }
tree-folder-create-extension = Créer une extension
tree-edit-table = Modifier la table
tree-edit-view = Modifier la vue
tree-edit-column = Modifier la colonne
tree-edit-index = Modifier l'index
tree-edit-primary-key = Modifier la clé primaire
tree-edit-foreign-key = Modifier la clé étrangère
tree-triggers = Déclencheurs
tree-triggers-count = Déclencheurs ({ $n })
tree-refresh-view = Actualiser la vue
tree-truncate = Vider
tree-truncate-title = Vider la table
tree-truncate-rows = { $n ->
    [one] Supprimer { $rows } ligne de { $table } ? Cette action est irréversible.
   *[other] Supprimer les { $rows } lignes de { $table } ? Cette action est irréversible.
}
tree-truncate-every-row = Supprimer toutes les lignes de { $table } ? Cette action est irréversible.
tree-drop = Supprimer
tree-drop-index = Supprimer l'index
tree-drop-foreign-key = Supprimer la clé étrangère
tree-ai-explain = Explication par l'IA
tree-users = Utilisateurs et privilèges
tree-server-variables = Variables du serveur
tree-new-database = Créer une base de données
tree-show-table-sizes = Afficher la taille des tables

## Connection switcher

conn-menu-manage = Gérer les connexions

## Shared dialog buttons

dialog-cancel = Annuler
dialog-yes = Oui
dialog-no = Non

## Open-transaction prompt

tx-prompt-title = Transaction ouverte
tx-prompt-body = { $tab } a une transaction ouverte contenant { $n ->
    [one] { $n } instruction
   *[other] { $n } instructions
}. Vous pouvez valider pour conserver les modifications, ou annuler la transaction pour les abandonner.
tx-prompt-body-aborted = { $tab } a une transaction interrompue par l'échec d'une instruction, contenant { $n ->
    [one] { $n } instruction
   *[other] { $n } instructions
}. Elle ne peut qu'être annulée, ce qui abandonne les modifications.
tx-prompt-rollback = Annuler la transaction
tx-prompt-commit = Valider

## Error modal

error-modal-title = Erreur
error-modal-none = Aucune erreur.
error-modal-ai-fix = Correction par l'IA
error-modal-explain = Expliquer

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = échec de la requête : { $error }
db-connection-failed = échec de la connexion : { $error }
db-query-cancelled = requête annulée

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Non connecté — le SQL d'initialisation de cette connexion a été refusé : { $why }
db-refused-transport-entra = L'authentification Microsoft Entra exige SSL / TLS en mode Verify full : son jeton ouvre toutes les bases Azure SQL auxquelles le compte a accès, le serveur doit donc prouver son nom avant de le recevoir.
db-refused-transport-aws = L'authentification AWS IAM exige SSL / TLS au moins en mode Require : son jeton est un mot de passe pour ce serveur, et ne doit pas traverser le réseau en clair.
db-refused-transport-gcp = L'authentification Google Cloud IAM exige SSL / TLS en mode Verify CA, avec le certificat CA du serveur de l'instance comme CA racine : son jeton ouvre toutes les instances Cloud SQL auxquelles le compte a accès.
db-refused-transport = Cette authentification exige un mode SSL / TLS plus strict que celui de la connexion.
db-refused-backup-in-terminal = Une base de données serveur se sauvegarde avec ses propres outils, dans le terminal.
db-refused-commit-aborted = la transaction était déjà interrompue — une de ses instructions a échoué ou a été arrêtée, et la valider abandonnerait le travail au lieu de l'enregistrer. Rien n'a été validé.
db-refused-sqlite-no-manual = Les connexions SQLite ne prennent pas encore en charge le mode de transaction manuel — les instructions s'exécutent et sont validées dès leur envoi
db-refused-mssql-read-only-auto = Une connexion SQL Server en lecture seule s'exécute en mode Auto : SQL Server n'a pas de transaction en lecture seule où tenir un onglet Manuel, et chaque instruction exécutée en mode Auto est annulée
db-refused-client-charset = refusé : le jeu de caractères client de la session ({ $charset }) peut cacher un guillemet à l'intérieur d'un caractère, la vérification de l'instruction ne peut donc pas la lire comme le ferait le serveur
db-refused-sql-mode = refusé : le sql_mode du serveur ({ $mode }) lit les guillemets autrement que la vérification de l'instruction, et n'a pas pu être modifié pour cette session
db-refused-writing-plan = Ici, les plans s'exécutent dans une transaction en lecture seule, et MySQL et MariaDB refusent d'y planifier une instruction qui écrit. Planifiez plutôt la forme SELECT de l'instruction.
db-refused-writing-plan-function = Ici, les plans s'exécutent dans une transaction en lecture seule, et MySQL et MariaDB refusent d'y planifier une instruction qui écrit. Une fonction qu'elle appelle écrit, ce qu'un plan n'a pas le droit de faire ici.
db-refused-windows-build-only = L'authentification Windows n'est disponible que dans la version Windows de Schemaic.
db-refused-cloud-iam-engine = L'authentification Cloud IAM est réservée aux connexions PostgreSQL et MySQL.
db-refused-keyring-unreadable = Schemaic n'a pas pu lire les identifiants enregistrés de cette connexion dans le trousseau du système ({ $error }). Rien n'a été supprimé : déverrouillez le trousseau et réessayez.
db-refused-mssql-cancel-needs-kill = SQL Server ne peut pas annuler l'instruction d'une autre session sans mettre fin à la session — utilisez Terminer la session.
db-refused-shadowed-function = Refusé avant l'exécution de votre instruction : { $schema }.{ $name } est une fonction propre à cette base de données portant le nom d'une fonction intégrée de PostgreSQL que les requêtes en lecture seule peuvent appeler, et PostgreSQL peut l'appeler à sa place ; la vérification de lecture seule ne peut donc pas garantir ce qu'une requête exécuterait ici. Retirez { $schema } du chemin de recherche de la connexion, ou demandez au propriétaire de la base de données de la supprimer ou de la renommer.
db-refused-shadowed-operator = Refusé avant l'exécution de votre instruction : { $schema } contient l'opérateur { $name }, et PostgreSQL peut l'utiliser à la place d'un opérateur intégré ; la vérification de lecture seule ne peut donc pas garantir ce qu'une requête exécuterait ici. Retirez { $schema } du chemin de recherche de la connexion, ou demandez au propriétaire de la base de données de le supprimer.
db-refused-blank-value = { $has_row ->
        [some] La ligne { $row } a une valeur vide ou blanche pour { $column } ({ $sql_type }) : SQL Server l'enregistrerait comme 0 ou 1900-01-01 au lieu de la refuser. Importez les champs vides comme NULL, ou remplissez-les.
       *[none] Une valeur vide ou blanche ne peut pas être écrite dans { $column } ({ $sql_type }) : SQL Server l'enregistrerait comme 0 ou 1900-01-01 au lieu de la refuser. Mettez la cellule à NULL, ou saisissez une valeur.
    }
db-refused-code-page = { $has_row ->
        [some] La ligne { $row } a une valeur pour { $column } ({ $sql_type }) qui contient
       *[none] Une valeur pour { $column } ({ $sql_type }) contient
    } { $has_lost ->
        [some] `{ $character }`, que la page de codes { $code_page } ne peut pas représenter
       *[none] un caractère que sa page de codes ne peut pas représenter
    } : SQL Server l'enregistrerait comme `?` ou un caractère ressemblant au lieu de la refuser. Stockez la colonne en nvarchar, ou retirez le caractère.
db-refused-sqlite-no-file = aucun fichier de base de données n'est défini
db-refused-sqlite-memory = :memory: n'est pas un fichier de base de données — SQLite ne garde une base en mémoire que tant que vit la connexion qui l'a créée, et cette application ouvre une nouvelle connexion par opération : chaque écriture serait donc signalée comme enregistrée, puis abandonnée. Indiquez le chemin d'un fichier.
db-refused-sqlite-uri = { $file } : ceci est une URI, pas un fichier de base de données — SQLite lirait la partie après `?` comme des options d'ouverture, et `?mode=memory` ouvre une base temporaire qui accepte toutes les écritures sans en garder aucune. Indiquez le chemin du fichier lui-même.
db-refused-sqlite-worker = le processus SQLite a échoué : { $error }
db-refused-sqlite-no-column = { $table }.{ $column } n'est pas une colonne de cette table, elle n'a donc pas de valeur par défaut à écrire
db-refused-sqlite-generated = { $column } est une colonne générée : elle n'a pas de valeur par défaut à laquelle revenir, et SQLite la calcule à partir de la ligne
db-refused-sqlite-sizes-too-large = La taille des tables n'est lue que pour une base de données d'au plus { $cap } : SQLite la mesure en lisant chaque page du fichier, et celui-ci fait { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = délai dépassé
db-connect-init-failed = le SQL d'initialisation `{ $statement }` a échoué : { $error }
db-connect-session-retired = la connexion de cet onglet a été fermée après un arrêt que le serveur n'a pas confirmé, et une nouvelle n'a pas pu être ouverte ; sa transaction a été annulée. Passez l'onglet en Auto puis de nouveau en Manuel pour en ouvrir une autre.
db-connect-begin-failed = impossible de démarrer la transaction : { $error }
db-connect-prepare-session = impossible de préparer la session : { $error }
db-connect-read-only-setup = impossible de passer la session en lecture seule : { $error }
db-connect-guard-session = impossible de protéger la session : { $error }
db-connect-plan-setup = impossible de préparer le plan : { $error }
db-connect-no-maintenance-db = aucune base de maintenance accessible
db-connect-browser-silent = SQL Server Browser sur { $server } n'a donné aucun port pour l'instance { $instance } : le service Browser ne tourne pas, le port UDP 1434 est bloqué en chemin, ou aucune instance ne porte ce nom. Si l'instance écoute sur un port fixe, définissez l'hôte sur { $server } sans \{ $instance } et le port sur celui-ci.
db-connect-browser-without-tcp = SQL Server Browser sur { $server } a répondu pour l'instance { $instance } mais n'a donné aucun port TCP : l'instance n'écoute pas en TCP. Activez TCP/IP pour elle dans le Gestionnaire de configuration SQL Server et redémarrez-la.
db-connect-azure-timed-out = Azure SQL n'a pas terminé l'authentification en { $secs } secondes. Une base serverless en pause met environ une minute à reprendre, et cette tentative l'a réveillée — reconnectez-vous dans un instant.
db-connect-azure-unavailable = { $server } Une base serverless en pause met environ une minute à reprendre, et cette tentative l'a réveillée — reconnectez-vous dans un instant.
db-ssh-connect-no-answer = Connexion SSH avec { $host } : aucune réponse en { $secs } s
db-ssh-auth-no-answer = Authentification SSH avec { $host } : aucune réponse en { $secs } s
db-ssh-auth-error = Erreur d'authentification SSH : { $error }
db-ssh-auth-failed = Échec de l'authentification SSH
db-ssh-key-load = Échec du chargement de la clé SSH : { $error }
db-ssh-key-auth = Erreur d'authentification par clé SSH : { $error }
db-ssh-agent = Erreur de l'agent SSH : { $error }
db-ssh-agent-empty = L'agent SSH n'a aucune identité chargée (exécutez `ssh-add`)
db-ssh-agent-auth = Erreur d'authentification par l'agent SSH : { $error }
db-ssh-no-agent = aucun agent SSH trouvé (démarrez le service OpenSSH Authentication Agent, ou lancez Pageant)
db-ssh-no-agent-socket = aucun agent SSH ($SSH_AUTH_SOCK) : { $error }
db-ssh-jump-unreachable = L'hôte de rebond SSH n'a pas pu joindre { $host } : { $error }
db-ssh-connect-to = Échec de la connexion SSH à { $host } : { $error }
db-ssh-connect = Échec de la connexion SSH : { $error }
db-ssh-no-host = Le tunnel SSH n'a pas d'hôte
db-ssh-bind = échec de la liaison du tunnel local : { $error }
db-ssh-named-instance = Une instance nommée ne peut pas être jointe par un tunnel SSH : SQL Server Browser indique son port via UDP, que le tunnel ne transporte pas. Définissez l'hôte sur { $server } sans \{ $instance } et le port sur celui où l'instance écoute.
db-ssh-host-key-changed = La clé d'hôte SSH de { $host } a CHANGÉ depuis que Schemaic lui a fait confiance pour la première fois.
db-ssh-host-key-expected = Attendue { $key }
db-ssh-host-key-offered = Reçue    { $key }
db-ssh-host-key-none = (aucune clé enregistrée)
db-ssh-host-key-changed-advice = Cela peut signifier que le serveur a été reconstruit ou que sa clé a été renouvelée — ou que quelque chose intercepte la connexion. Rien ne lui a été envoyé. Vérifiez la nouvelle empreinte par un autre canal (sur la console du serveur lui-même, ou auprès de son administrateur) avant de lui faire confiance. Ensuite, retirez l'entrée de cet hôte de { $store } dans le dossier de configuration de Schemaic pour enregistrer la nouvelle clé.
db-ssh-host-key-store = Schemaic n'a pas pu lire son registre des clés d'hôte SSH approuvées ({ $store }) ; il ne peut donc pas savoir si la clé de { $host } ({ $key }) est celle que vous aviez approuvée.
db-ssh-host-key-store-advice = La connexion a été refusée plutôt que de faire confiance à la clé d'emblée. Réparez ou supprimez { $store } dans le dossier de configuration de Schemaic — le supprimer signifie que chaque hôte SSH sera approuvé à nouveau à sa prochaine connexion : ne le faites que si vous pouvez vérifier les empreintes.
db-tls-setup = Échec de la configuration TLS : { $error }
db-tls-client-cert = certificat client refusé : { $error }
db-tls-trust-store = le magasin de confiance TLS est inutilisable : { $error }
db-tls-bad-name = { $name } n'est pas un nom qu'un certificat peut porter : { $error }
db-tls-ca-empty = le fichier CA { $path } ne contient aucun certificat
db-tls-ca-unusable = le fichier CA { $path } contient un certificat inutilisable comme ancre de confiance
db-tls-cert-unreadable = impossible de lire le fichier de certificat { $path } : { $error }
db-tls-cert-not-pem = { $path } n'est pas un fichier de certificat PEM : { $error }
db-tls-cert-is-der = { $path } ne contient aucun certificat PEM — un fichier DER doit d'abord être converti (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = impossible de lire le fichier de clé { $path } : { $error }
db-tls-key-encrypted = { $path } est une clé privée protégée par une phrase secrète, que Schemaic ne peut pas ouvrir. Déchiffrez-la d'abord : openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } n'est pas une clé privée PEM : { $error }
db-sign-in-no-value = { $field ->
        [host] La connexion n'a pas d'hôte avec lequel s'authentifier.
       *[user] La connexion n'a pas d'utilisateur avec lequel s'authentifier.
    }
db-sign-in-leading-dash = { $field ->
        [host] L'hôte de la connexion commence par « - », que la CLI cloud lirait comme une option et non comme l'hôte.
       *[user] L'utilisateur de la connexion commence par « - », que la CLI cloud lirait comme une option et non comme l'utilisateur.
    }
db-sign-in-control = { $field ->
        [host] L'hôte de la connexion contient un caractère de contrôle.
       *[user] L'utilisateur de la connexion contient un caractère de contrôle.
    }
db-sign-in-shim-aws = L'AWS CLI trouvée est un shim .cmd (la version 1 installée par pip), et Schemaic n'exécute pas de shim via cmd.exe. Installez la version 2 de l'AWS CLI, ou authentifiez-vous avec un mot de passe.
db-sign-in-shim-gcloud = Le gcloud trouvé est un shim .cmd sans Python intégré à côté, et Schemaic n'exécute pas de shim via cmd.exe. Réinstallez la Google Cloud CLI avec son programme d'installation, ou authentifiez-vous avec un mot de passe.
db-sign-in-shim-azure = L'Azure CLI trouvée est un shim .cmd/.bat sans Python à côté, et Schemaic n'exécute pas de shim via cmd.exe. Réinstallez l'Azure CLI avec le programme d'installation de Microsoft, ou authentifiez-vous avec un login SQL.
db-sign-in-no-token = La CLI cloud a répondu sans jeton.
db-sign-in-more-than-token = La CLI cloud a répondu avec plus qu'un jeton.
db-sign-in-azure-bad-answer = La réponse de l'Azure CLI n'était pas le jeton qu'elle aurait dû afficher.
db-sign-in-azure-no-access-token = L'Azure CLI a répondu sans jeton d'accès.
db-sign-in-signed-out-aws = L'AWS CLI n'a aucune information d'identification. Exécutez `aws configure` ou `aws sso login`, puis reconnectez-vous.
db-sign-in-signed-out-gcloud = La Google Cloud CLI n'est pas authentifiée. Exécutez `gcloud auth login`, puis reconnectez-vous.
db-sign-in-signed-out-azure = L'Azure CLI n'est pas authentifiée. Exécutez `az login`, puis reconnectez-vous.
db-sign-in-no-token-from = { $cli } n'a pas pu obtenir de jeton : { $line }
db-sign-in-no-token-from-silent = { $cli } n'a pas pu obtenir de jeton.
db-sign-in-no-token-from-azure-silent = L'Azure CLI n'a pas pu obtenir de jeton : elle n'a donné aucune raison
db-sign-in-not-found = { $cli } est introuvable. Installez-la et authentifiez-vous avec, ou authentifiez-vous avec un mot de passe.
db-sign-in-not-found-azure = L'Azure CLI (az) est introuvable. Installez-la et exécutez `az login`, ou authentifiez-vous avec un login SQL.
db-sign-in-no-answer = { $cli } n'a pas répondu en { $secs } secondes.
db-sign-in-not-started = Impossible de lancer { $cli } : { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = l'export a cessé de lire
db-query-no-rows-to-export = cette instruction ne renvoie aucune ligne à exporter
db-query-no-server-settings = Ce moteur n'a aucun paramètre serveur à afficher.
db-query-no-status-list = Ce moteur ne publie aucune liste d'état — seulement ses variables.
db-query-no-import-columns = Aucune colonne dans laquelle importer
db-query-read-stopped = La lecture s'est arrêtée : { $error }
db-query-batch-continued = SQL Server poursuit un lot après l'erreur d'une instruction, sauf si l'erreur met fin au lot : les instructions qui suivent celle en échec ont donc pu s'exécuter.
db-query-sqlite-locked = Sous SQLite, une lecture longue bloque l'écriture : un export ou un import de table entière garde un verrou de lecture sur le fichier jusqu'à la fin, et aucune écriture ne peut commencer avant (MySQL et PostgreSQL ne fonctionnent pas ainsi). Attendez la fin ou annulez, puis réessayez. Pour que lectures et écritures puissent se chevaucher durablement sur ce fichier, passez-le en mode de journal WAL — `PRAGMA journal_mode = WAL` — ce qui modifie le fichier lui-même : Schemaic ne le fera donc pas à votre place.
db-query-rollback-failed-too = { $error } — et l'annulation a également échoué : { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = l'instruction { $n } a échoué : { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } instruction précédente déjà appliquée ne peut pas être annulée
   *[other] { $failed } — { $n } instructions précédentes déjà appliquées ne peuvent pas être annulées
}
ddl-cancelled = le plan a été annulé
ddl-dangling-foreign-key = le plan laisse une clé étrangère qui ne pointe vers rien : { $row }
ddl-breaks-view = le plan casserait la vue { $view }, qui lit une colonne qu'il renomme ou supprime ({ $why }). Rien n'a été modifié. Modifiez ou supprimez d'abord la vue.
ddl-no-databases = SQLite n'a pas de bases de données à créer ou supprimer — une base y est un fichier, que Schemaic ne crée ni ne supprime pour vous.
ddl-elsewhere-failed = Les vues d'autres bases de données qui font select * from { $table } n'ont pas été actualisées ({ $error }), et lisent et écrivent les mauvaises colonnes tant qu'elles ne le sont pas (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } vue d'une autre base de données qui fait select * from { $table } n'a pas pu être actualisée, et lit et écrit les mauvaises colonnes tant qu'elle ne l'est pas (sp_refreshview) : { $views }.
       *[other] { $count } vues d'une autre base de données qui font select * from { $table } n'ont pas pu être actualisées, et lisent et écrivent les mauvaises colonnes tant qu'elles ne le sont pas (sp_refreshview) : { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } base de données n'a pas été examinée à la recherche d'une vue qui fait select * from { $table } : { $databases }.
       *[other] { $count } bases de données n'ont pas été examinées à la recherche d'une vue qui fait select * from { $table } : { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , et { $more } de plus
    }
ddl-rollback-failed = { $error } — et l'annulation a également échoué : { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = la suppression dans { $table } a touché { $affected } lignes (exactement 1 attendue)
write-one-row-update = la mise à jour de { $table } a touché { $affected } lignes (exactement 1 attendue)
write-one-row-insert = l'insertion dans { $table } a touché { $affected } lignes (exactement 1 attendue)
write-batch-inserted = un lot de { $rows } lignes a inséré { $wrote }
write-batch-affected = un lot de { $rows } lignes a touché { $wrote }
write-row-issue = ligne { $line }, colonne { $column } : { $problem } ({ $text })
write-row-issue-empty = ligne { $line }, colonne { $column } : { $problem } (vide)
write-copy-too-long = { $column } contient plus de texte qu'une lecture ne peut en transporter : la copie s'est donc arrêtée avant de l'écrire
write-copy-too-long-unnamed = une colonne contient plus de texte qu'une lecture ne peut en transporter : la copie s'est donc arrêtée avant de l'écrire
write-copy-bytes = { $column } contient des octets que la copie ne peut pas lire tels quels : elle s'est donc arrêtée avant de les écrire
write-copy-not-its-form = la valeur lue pour { $column } n'est pas sous la forme demandée par la copie : rien n'a été écrit
write-copy-not-its-form-unnamed = la valeur lue pour une colonne n'est pas sous la forme demandée par la copie : rien n'a été écrit
write-commit-cancelled = Validation annulée
write-import-cancelled = Import annulé
write-rollback-complete = {" "}— toutes les modifications ont été annulées
write-rollback-incomplete = {" "}— l'annulation ne les a PAS défaites : le moteur de stockage de cette table n'est pas transactionnel, les lignes déjà écrites restent donc en place. Vérifiez la table avant de réessayer.
write-rollback-unknown = {" "}— et l'annulation n'a pas pu être confirmée : la connexion a échoué avant la réponse du serveur. Des lignes peuvent subsister ou non. Vérifiez la table avant de réessayer.

## Schema tree › the panel

tree-panel-title = SCHÉMA
tree-tip-databases = Afficher ou masquer les bases de données
tree-tip-options = Options du schéma
tree-search-placeholder = Rechercher…
tree-loading = Chargement
tree-no-tables = Aucune table
tree-failed-to-connect = Échec de la connexion
tree-failed-view = Afficher
tree-single-user-unread = Mono-utilisateur — non lue
tree-single-user-read = Lire
tree-count-tables = { $n ->
    [one] { $n } table
   *[other] { $n } tables
}
tree-count-cols = { $n ->
    [one] { $n } col.
   *[other] { $n } col.
}
tree-count-keys = { $n ->
    [one] { $n } clé
   *[other] { $n } clés
}
tree-count-partitions = { $n ->
    [one] { $n } partition
   *[other] { $n } partitions
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = type
kind-domain = domaine
kind-sequence = séquence
kind-function = fonction
kind-procedure = procédure
kind-event = événement
kind-synonym = synonyme
kind-alias-type = type alias
kind-xml-schema-collection = collection de schémas XML
kind-extension = extension
kind-group-type = Types
kind-group-domain = Domaines
kind-group-sequence = Séquences
kind-group-function = Fonctions
kind-group-procedure = Procédures
kind-group-event = Événements
kind-group-synonym = Synonymes
kind-group-alias-type = Types alias
kind-group-xml-schema-collection = Collections de schémas XML
kind-group-extension = Extensions

## Query history panel

history-title = HISTORIQUE DES REQUÊTES
history-clear-tip = Effacer l'historique de cette connexion…
history-clear-title = Effacer l'historique des requêtes
history-clear-message = Supprimer { $n } { $n ->
    [one] requête enregistrée
   *[other] requêtes enregistrées
} pour cette connexion ? Cette action est irréversible.
history-scope-current = Actuelle
history-scope-all = Toutes
history-cut-note = Affichage des { $shown } plus récentes sur { $matched } — lancez une recherche pour affiner.
history-empty = Aucune requête pour l'instant.
history-no-match = Aucune requête correspondante.
history-open-in-new-tab = Ouvrir dans un nouvel onglet
history-delete = Supprimer
history-failed = Échec

## Snippet library panel

snippets-title = BIBLIOTHÈQUE D'EXTRAITS
snippets-save-tip = Enregistrer le SQL de l'éditeur comme extrait
snippets-no-match = Aucun extrait ne correspond.
snippets-band-this-connection = CETTE CONNEXION
snippets-band-all-connections = TOUTES LES CONNEXIONS
snippets-builtin = Intégré
snippets-name-placeholder = Nom de l'extrait
snippets-insert = Insérer dans l'éditeur
snippets-open-in-new-tab = Ouvrir dans un nouvel onglet
snippets-edit = Modifier
snippets-duplicate = Dupliquer
snippets-delete = Supprimer
snippets-show-in = Afficher dans
snippets-scope-this-connection = Cette connexion
snippets-scope-engine = Toutes les connexions { $engine }
snippets-scope-all-connections = Toutes les connexions

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = AUJOURD'HUI
history-band-this-week = CETTE SEMAINE
history-band-earlier = PLUS ANCIEN

## Query tab strip

tabs-tip-unsaved = { $path } — modifications non enregistrées
tabs-pin = Épingler
tabs-unpin = Désépingler
tabs-rename = Renommer
tabs-duplicate = Dupliquer
tabs-close = Fermer
tabs-open-file = Ouvrir un fichier
tabs-save = Enregistrer
tabs-save-as = Enregistrer sous
tabs-reload = Recharger depuis le disque
tabs-reopen-last = Rouvrir le dernier onglet
tabs-close-others = Fermer les autres onglets
tabs-close-all = Fermer tous les onglets

## Server Activity panel

activity-title = ACTIVITÉ DU SERVEUR
activity-refresh-tip = Actualiser maintenant
activity-auto-refresh-off = Actualisation automatique désactivée
activity-auto-refresh-every = Actualisation automatique toutes les { $secs } s
activity-interval-off = Désactivée
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } h { $m } min
activity-age-days = { $d } j { $h } h
activity-view-sessions = Sessions
activity-view-locks = Verrous
activity-search-sessions = Rechercher des sessions…
activity-search-locks = Rechercher des verrous…
activity-loading = Chargement…
activity-sqlite-no-sessions = SQLite n'a pas de sessions serveur — il s'exécute dans ce processus, il n'y a donc rien d'autre de connecté à afficher.
activity-sessions-no-match = Aucune session correspondante.
activity-sessions-none = Aucune autre session sur ce serveur.
activity-sessions-undrawn = { $n ->
    [one] { $n } session de plus correspond — affinez la recherche pour la voir.
   *[other] { $n } sessions de plus correspondent — affinez la recherche pour les voir.
}
activity-locks-no-match = Aucun verrou correspondant.
activity-locks-no-waits = Aucune attente de verrou sur ce serveur.
activity-locks-none = Aucune autre session ne détient de verrou.
activity-locks-contended-only = Ce serveur ne liste que les verrous attendus, ou qui bloquent une attente — pas tous les verrous d'une session.
activity-locks-undrawn = { $n ->
    [one] { $n } verrou de plus correspond — affinez la recherche pour le voir.
   *[other] { $n } verrous de plus correspondent — affinez la recherche pour les voir.
}
activity-list-capped = liste tronquée
activity-locks-total = { $n ->
    [one] { $shown } verrou
   *[other] { $shown } verrous
}
activity-locks-waiting = { $n ->
    [one] { $shown } en attente
   *[other] { $shown } en attente
}
activity-state-blocked = Bloquée
activity-state-idle-in-tx = Inactive en transaction
activity-state-running = En cours
activity-state-idle = Inactive
activity-count-sessions = { $n ->
    [one] { $figure } session
   *[other] { $figure } sessions
}
activity-count-running = { $figure } en cours
activity-count-blocked = { $figure } bloquée(s)
activity-count-idle-in-tx = { $figure } inactive(s) en transaction
activity-lock-wait = Attente de verrou
activity-lock-wait-sentence = La session { $waiter } ({ $waiter_who }) { $waited ->
        [none] attend
       *[other] attend depuis { $waited }
    } un verrou détenu par la session { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] inactive dans une transaction
        [blocked] elle-même bloquée
        [running] en cours d'exécution
       *[idle] inactive
    }{ $held ->
        [none] {""}
       *[other] {" "}depuis { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}{ $others } autre session détient aussi un verrou dont elle a besoin.
       *[other] {" "}{ $others } autres sessions détiennent aussi un verrou dont elle a besoin.
    }
activity-kill-holder = Terminer { $id }
activity-terminate-tip = Terminer la session { $id }
activity-read-only-tip = Cette connexion est en lecture seule.
activity-lock-holds = détient
activity-lock-waits-for = attend
activity-lock-held-by = détenu par { $ids }
activity-copy-statement = Copier l'instruction
activity-cancel-query = Annuler la requête
activity-kill-session = Terminer la session
activity-waiting-on = attend un verrou détenu par { $ids }
activity-blocks = { $n ->
        [one] bloque { $n } session
       *[other] bloque { $n } sessions
    }
activity-read-only-cancel = Cette connexion est en lecture seule : Schemaic n'y annulera donc pas de requêtes. Désactivez d'abord la lecture seule dans les paramètres de la connexion.
activity-read-only-terminate = Cette connexion est en lecture seule : Schemaic n'y terminera donc pas de sessions. Désactivez d'abord la lecture seule dans les paramètres de la connexion.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ASSISTANT IA
ai-new-chat-tip = Nouvelle conversation
ai-settings-tip = Paramètres de l'IA
ai-empty-ask = Posez une question sur votre SQL…
ai-empty-not-connected = { $cli } n'est pas connecté.
ai-input-placeholder = Message…
ai-input-off = L'assistant IA est désactivé
ai-input-db-down = Non connecté à la base de données
ai-speaker-you = VOUS
ai-result-more-lines = … (+{ $n } lignes de plus)

## Shared dialog buttons (continued)

dialog-close = Fermer

## Chart modal

chart-title = Graphique — { $tab }
chart-caption-kind = Graphique
chart-caption-values = Valeurs
chart-caption-labels = Libellés
chart-row-number = Numéro de ligne
chart-kind-bar = Barres
chart-kind-line = Courbe
chart-kind-histogram = Histogramme
chart-save-tip = Enregistrer le graphique…
chart-save-dialog = Enregistrer le graphique
chart-file-stem = Graphique { $name }
chart-nothing-to-save = Rien à enregistrer.
chart-no-numbers = Aucune colonne numérique à représenter dans ce résultat.
chart-all-null = Toutes les valeurs de cette colonne sont NULL.
chart-too-large = Ces valeurs sont trop grandes pour être dessinées.
chart-caption-rows = { $n ->
    [one] { $n } ligne
   *[other] { $n } lignes
}
chart-caption-skipped-null = { $skipped } NULL, non dessinées
chart-caption-skipped = { $skipped } non dessinées (NULL, ou au-delà de { $max })
chart-caption-truncated = les lignes lues, pas le résultat complet

## Schema editors — shared

editor-preview-sql = Aperçu du SQL
editor-change-count = { $n ->
    [0] Aucune modification
    [one] { $n } modification
   *[other] { $n } modifications
}

## View editor

view-editor-title-edit = Modifier la vue { $view }
view-problem-no-name = La vue a besoin d'un nom.
view-problem-no-select = Une vue a besoin d'un SELECT pour la définir.
view-problem-not-a-query = Le corps d'une vue doit être une requête — il commence par SELECT, WITH, VALUES ou TABLE.
view-problem-materialized = Schemaic ne peut pas modifier une vue matérialisée — PostgreSQL n'a pas de CREATE OR REPLACE pour elles.
view-problem-hidden = La définition de cette vue est chiffrée ou masquée pour ce login : elle peut être supprimée ici, mais pas modifiée.
view-problem-verbatim = L'en-tête de cette vue contient quelque chose que Schemaic ne sait pas réécrire : elle peut être supprimée ici, mais pas modifiée.
view-editor-title-create = Créer une vue dans { $location }
view-editor-section-view = Vue
view-editor-section-definition = Définition
view-editor-section-options = Options
view-editor-name = Nom
view-editor-select = Instruction SELECT
view-editor-check-option = Check option
view-editor-column-names = Noms des colonnes
view-editor-column-names-placeholder = facultatif — tirés du SELECT si vide
view-editor-sql-security = SQL security
view-editor-definer = Definer
view-editor-recreate = Recréer au lieu de remplacer
view-editor-recreate-hint = PostgreSQL ne peut qu'ajouter des colonnes à une vue existante. En renommer, en changer le type ou l'ordre exige un DROP et un CREATE, qui emportent les vues dépendantes et les droits accordés.

## Table properties panel

props-title = Propriétés — { $table }
props-reading = Lecture des statistiques
props-no-stats-view = Une vue n'a pas de stockage propre : le serveur ne renvoie donc aucune statistique pour elle.
props-no-stats-table = Le serveur n'a renvoyé aucune statistique pour cette table.
props-on-disk = sur disque
props-rows-estimated = lignes (estimation)
props-rows-counted = lignes (comptées)
props-rows-not-reported = lignes — non communiqué
props-counting = Comptage
props-count-rows = Compter les lignes
props-count-slow = Un parcours complet de la table. Lent sur une grande table.
props-storage-data = Données
props-storage-indexes = Index
props-storage-free = Libre
props-section-structure = Structure
props-columns = Colonnes
props-indexes = Index
props-foreign-keys = Clés étrangères
props-count-columns = { $n ->
    [one] { $n } colonne
   *[other] { $n } colonnes
}
props-count-indexes = { $n ->
    [one] { $n } index
   *[other] { $n } index
}
props-count-keys = { $n ->
    [one] { $n } clé
   *[other] { $n } clés
}
props-type = Type
props-type-view = Vue
props-type-table = Table
props-section-options = Options
props-engine = Moteur
props-row-format = Format de ligne
props-collation = Collation
props-next-auto-increment = Prochain auto-incrément
props-created = Créée
props-updated = Modifiée
props-comment = Commentaire
props-section-partitioning = Partitionnement
props-partition-of = Partition de
props-partition-bound = Limite
props-partitioned-by = Partitionnée par
props-partitions-more = { $n ->
    [one] et { $n } partition de plus
   *[other] et { $n } partitions de plus
}
props-section-indexes = Index
props-index-cardinality = cardinalité { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } parcours
       *[other] { $shown } parcours
    }
props-index-not-counted = utilisation non comptée
props-index-unused = jamais utilisé depuis la réinitialisation des compteurs du serveur
props-never-analysed = Cette table n'a jamais été analysée : le serveur n'a donc aucune estimation du nombre de lignes. Exécutez ANALYZE, ou comptez les lignes.
props-analysed-at = Estimation au dernier ANALYZE, { $when }.
props-read-live = Lu en direct — ce serveur a information_schema_stats_expiry = 0.
props-cached-up-to = Mis en cache par le serveur : ces chiffres peuvent dater de { $age } au plus (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } h
props-age-days = { $n } j
props-vacuum-note = { $pct } % des versions de lignes de cette table sont mortes et pas encore récupérées. Un VACUUM rendrait cet espace.
props-copy = Copier

## Shared panel search box

panel-search-placeholder = Rechercher…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = à l'instant
ago-minutes = il y a { $n } min
ago-hours = il y a { $n } h
ago-days = il y a { $n } j
ago-weeks = il y a { $n } sem.

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } ligne
   *[other] { $n } lignes
}
outcome-rows-capped = { $n }+ lignes

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = Ouvrir la CLI de la base de données
term-db-cli-tip-read-only = Ouvrir la CLI de la base de données — cette connexion est en lecture seule
term-restart-tip = Redémarrer le terminal
term-settings-tip = Paramètres du terminal

## Results panel header

results-title = RÉSULTATS
results-properties-tip = Propriétés de la table…
results-monitor-tip = Moniteur en direct…
results-restore-editor-tip = Restaurer l'éditeur
results-collapse-editor-tip = Réduire l'éditeur

## Query panel header

query-title = REQUÊTE
query-no-database = Aucune base de données

## Shared dialog buttons (continued)

dialog-stop = Arrêter

## Copy table modal

copy-table-title = Copier { $table }
copy-table-rows-line = Puis y copier toutes les lignes de { $table } (non affiché en SQL)
copy-table-into = Copier dans
copy-table-schema = Schéma
copy-table-name = Nom
copy-table-back = ‹ retour
copy-table-data = Copier les données
copy-table-data-hint = Copier les lignes une fois la table créée. Désactivé, seule la structure est copiée.
copy-table-what-with-rows = La structure et les lignes. Les clés étrangères et les déclencheurs ne sont pas copiés, et une colonne générée est recalculée.
copy-table-what-structure = La structure uniquement. Les clés étrangères et les déclencheurs ne sont pas copiés.
copy-table-choose-database = Choisissez la base de données de destination.
copy-table-unnamed = Nommez la copie.
copy-table-onto-itself = Une table ne peut pas être copiée sur elle-même — donnez un autre nom à la copie.
copy-table-connection-changed = La connexion active a changé — rouvrez Copier vers depuis la table.
copy-table-schema-not-loaded = Le schéma de { $database } n'est pas chargé — développez-le dans l'arborescence, puis réessayez.
copy-table-preview-subject = Copie de { $table }
copy-table-copying = Copie des lignes… { $n }
copy-table-copied = { $n } { $n ->
    [one] ligne copiée
   *[other] lignes copiées
}.
copy-table-stopped = Arrêté avant que toutes les lignes soient copiées. La table a été créée.

## Database, schema and extension editor

database-editor-title-database = Créer une base de données
database-problem-no-name = Le nom ne peut pas être vide.
database-problem-bad-collation = Un nom de collation ne contient que des lettres, des chiffres et des traits de soulignement.
database-editor-title-schema = Créer un schéma
database-editor-title-extension = Créer une extension
database-editor-title-schema-in = Créer un schéma dans { $database }
database-editor-title-extension-in = Créer une extension dans { $database }
database-editor-section-database = Base de données
database-editor-section-schema = Schéma
database-editor-section-extension = Extension
database-editor-name = Nom
database-editor-charset = Jeu de caractères
database-editor-collation = Collation
database-editor-owner = Propriétaire
database-editor-server-default = valeur par défaut du serveur
database-editor-owner-you = vous
database-editor-no-extensions = Plus aucune extension à installer
database-editor-no-charsets = Aucun jeu de caractères à suggérer
database-editor-no-collations = Aucune collation à suggérer
database-editor-no-roles = Aucun rôle sur ce serveur

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importer dans { $database }
script-dialog-title = Exécuter un script SQL
script-section = Script SQL
script-choose-file = Choisir un fichier…
script-no-file = Aucun fichier choisi
script-reading = Lecture du fichier
script-hint = Le script peut créer et sélectionner une autre base de données que celle dans laquelle vous importez. Il peut aussi créer plusieurs bases de données. Vérifiez que le script fait bien ce que vous attendez.
script-hint-namespace = Le script peut créer et sélectionner une autre base de données que celle dans laquelle vous importez. Il peut aussi créer plusieurs bases de données. L'ouvrir depuis { $namespace } n'y place pas non plus le script : tout ce que le fichier ne qualifie pas va là où l'indique le search_path du serveur. Vérifiez que le script fait bien ce que vous attendez.
script-what-it-does = Ce que fait ce fichier
script-probe-nothing = Ce fichier ne contient aucune instruction que Schemaic puisse exécuter.
script-probe-cut-off = Les premiers { $size } de ce fichier forment une seule instruction inachevée : il n'y a donc encore rien à résumer. L'exécution envoie le fichier entier.
script-probe-counted = { $count } { $n ->
    [one] instruction lue
   *[other] instructions lues
} dans les premiers { $size }.
script-destroys = { $count } { $n ->
    [one] instruction
   *[other] instructions
} de ce fichier { $n ->
    [one] détruit
   *[other] détruisent
} des données — DROP, TRUNCATE ou DELETE. Cela ne peut pas être annulé d'ici.
script-unread = Seuls les premiers { $size } de ce fichier ont été lus, et rien n'y détruit de données — mais le reste n'a pas été examiné. Il peut faire DROP, TRUNCATE ou DELETE, et cela ne peut pas être annulé d'ici.
script-unqualified = { $count } { $n ->
    [one] instruction
   *[other] instructions
} de ce fichier { $n ->
    [one] agit
   *[other] agissent
} sur toutes les lignes d'une table — un DELETE, UPDATE ou TRUNCATE sans WHERE. Cela ne peut pas être annulé d'ici.
script-tx-own = Ce fichier ouvre sa propre transaction : il s'applique donc entièrement ou pas du tout. Schemaic n'en ajoute aucune.
script-tx-none = Ce fichier n'ouvre aucune transaction : un échec en cours de route laisse en place ce qui a été exécuté avant.
script-connecting = Connexion
script-progress-of = { $done } sur { $total }
script-progress-read = { $done } lus
script-run = Exécuter
script-done = { $n } { $n ->
    [one] instruction exécutée
   *[other] instructions exécutées
} depuis { $file }.
script-stopped = Arrêté après { $n } { $n ->
    [one] instruction
   *[other] instructions
}.
script-failed = Échec : { $message } — { $n } { $n ->
    [one] instruction exécutée
   *[other] instructions exécutées
} auparavant.
script-failed-at = Échec à la ligne { $line } de { $file } : { $message } — { $n } { $n ->
    [one] instruction exécutée
   *[other] instructions exécutées
} auparavant.
script-durability-rolled-back = { $file } a ouvert sa propre transaction : le serveur a donc annulé toute l'exécution — rien n'a été appliqué, y compris les instructions qui avaient réussi.
script-durability-applied = { $file } n'a ouvert aucune transaction : toutes les instructions exécutées restent donc appliquées.
script-durability-applied-before = Toute instruction exécutée avant que { $file } n'ouvre sa propre transaction reste appliquée ; ce qui a été exécuté dans la transaction a été annulé.
script-durability-unknown = Toute instruction déjà exécutée reste appliquée, sauf si { $file } a ouvert sa propre transaction.

## Schema comparison modal

compare-title = Comparer des schémas
compare-choose-database = Choisir une base de données
compare-no-connection = Aucune connexion de ce moteur avec laquelle comparer.
compare-will-change = sera modifiée
compare-source-of-truth = référence
compare-idle = Choisissez une base de données avec laquelle comparer.
compare-loading = Lecture des deux schémas
compare-select-object = Sélectionnez un objet pour voir ses deux versions.
compare-fk-cycle-note = Les clés étrangères de ces tables forment un cycle — aucun ordre de création ne les satisfait toutes : une migration générée peut donc être refusée en cours de route.
compare-same-but-stored-differently = Ces objets concordent structurellement — la différence ci-dessous tient à la façon dont le moteur stocke l'instruction, et il n'y a rien à migrer.
compare-summary = { $differ } différents · { $only_here } seulement ici · { $only_there } seulement là-bas · { $same } identiques
compare-summary-uncertain = { $n } { $n ->
    [one] correspondance
   *[other] correspondances
} que la comparaison ne peut pas garantir
compare-filter-placeholder = Filtrer par nom
compare-include-identical = Inclure les identiques
compare-select-all = Tout sélectionner
compare-select-none = Aucun
compare-hint-held-back = exclu du plan ; la note ci-dessus explique pourquoi
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = son corps doit être relu depuis le serveur avant de pouvoir être appliqué
compare-omitted-unread = cette comparaison n'a aucune instruction pour lui — sa définition n'a pas pu être lue
compare-omitted-no-statement = cette comparaison n'a aucune instruction pour la différence trouvée
compare-held-partition = c'est une { $sub ->
        [yes] sous-partition
       *[no] partition
    } de { $parent }, qui est exclue — recréez-les ensemble, à la main
compare-held-partitioning-differs = son partitionnement diffère, et cette comparaison n'écrit aucune instruction qui repartitionne une table — recréez-la à la main
compare-held-sequence-unwritten = cette comparaison n'écrit aucune instruction pour une séquence — copiez-la depuis `SHOW CREATE SEQUENCE`, que son volet affiche
compare-held-partitioned-unrestated = elle est partitionnée, et aucun CREATE TABLE ici ne réécrit son schéma de partition — créez à la main la fonction et le schéma de partition, puis la table
compare-held-sequence-range = le compteur de la cible a atteint { $at }, hors de la nouvelle plage { $min } à { $max }, et le serveur refuse de restreindre une séquence en deçà d'une valeur déjà distribuée. Redémarrez-la sur la cible (ALTER SEQUENCE … RESTART WITH) à une valeur qu'aucune table ne contient, puis relancez la comparaison
compare-note-set-aside = { $aside } — la { $side ->
        [target] cible
       *[source] source
    } contient aussi { $kept }, et les noms de la cible ignorent la casse : elle ne peut donc contenir que l'un des deux ; { $kept } est celui qui est comparé
compare-note-binding = Type alias { $alias } : { $bound ->
        [rule] la règle { $rule } qui lui est liée sur la source n'est ni comparée ni reportée — liez-la
        [default] la valeur par défaut { $default } qui lui est liée sur la source n'est ni comparée ni reportée — liez-la
       *[both] la règle { $rule } et la valeur par défaut { $default } qui lui sont liées sur la source ne sont ni comparées ni reportées — liez-les
    } à la main (sp_bindrule / sp_bindefault).
compare-hint-blocked = le corps doit être relu avant de pouvoir être appliqué
compare-hint-uncertain = un index que ce modèle ne lit qu'en partie — une correspondance ici n'est pas certaine
compare-preview-migration = Aperçu de la migration
compare-plan-subject = { $n ->
    [one] { $n } objet dans { $database }
   *[other] { $n } objets dans { $database }
}
compare-selected = { $n ->
        [0] Rien de sélectionné.
        [one] { $n } objet sélectionné
       *[other] { $n } objets sélectionnés
    }
compare-incomparable = { $left } et { $right } ne peuvent pas être comparés. Les noms de types, les valeurs par défaut et la forme des index ne se transposent pas d'un moteur à l'autre : toute migration générée à partir de la différence serait fausse.
compare-group-table = Tables
compare-group-view = Vues
compare-group-trigger = Déclencheurs
compare-kind-enum = énumération
compare-kind-domain = domaine
compare-kind-xml-schema-collection = collection de schémas XML
compare-kind-alias-type = type alias
compare-kind-sequence = séquence
compare-kind-table = table
compare-kind-view = vue
compare-kind-function = fonction
compare-kind-procedure = procédure
compare-kind-trigger = déclencheur
compare-kind-event = événement
compare-kind-synonym = synonyme

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Aucune des deux bases de données ne contient quoi que ce soit à comparer.
compare-empty-agrees = Ces deux schémas concordent, objet par objet.
compare-empty-agrees-noted = Tous les objets concordent, mais tout n'a pas pu être comparé — voir les notes ci-dessus.
compare-empty-no-match = Rien ne correspond à ce filtre.
compare-empty-only-identical = Tout ce qui correspond à ce filtre est identique — activez Inclure les identiques pour le voir.

## Schema editors — shared (continued)

editor-comment-placeholder = ce qu'il fait

## Event editor

event-editor-title-edit = Modifier l'événement { $event }
event-editor-title-create = Créer un événement dans { $location }
event-editor-section-event = Événement
event-editor-section-schedule = Planification
event-editor-section-body = Corps
event-editor-section-options = Options
event-editor-name = Nom
event-editor-body = Corps
event-editor-runs = Exécution
event-editor-runs-repeating = Récurrente
event-editor-runs-once = Une seule fois
event-editor-at = À
event-editor-every = Toutes les
event-editor-starts = Début
event-editor-ends = Fin
event-editor-status = État
event-editor-status-enabled = Activé
event-editor-status-disabled = Désactivé
event-editor-status-replica = Désactivé sur le réplica
event-editor-preserve = Conserver une fois terminé
event-editor-preserve-hint = ON COMPLETION PRESERVE. Désactivé, c'est le comportement par défaut de MySQL : le serveur supprime l'événement une fois sa dernière exécution passée — ce qui, pour un événement ponctuel, a lieu juste après son exécution.
event-editor-definer = Definer
event-editor-comment = Commentaire
event-editor-reading-source = Lecture du source de l'événement…
event-editor-source-stale = Le véritable source de l'événement est arrivé après que vous avez commencé à saisir : ce corps repose donc sur la copie du catalogue — qui n'est pas ce que contient le serveur. Fermez et rouvrez pour le modifier.
event-problem-no-name = L'événement a besoin d'un nom.
event-problem-no-time = Un événement ponctuel a besoin d'une heure d'exécution.
event-problem-no-interval = Un événement récurrent a besoin d'un intervalle.
event-problem-no-unit = Un événement récurrent a besoin d'une unité d'intervalle (DAY, HOUR…).
event-problem-no-body = L'événement a besoin d'un corps.
event-name-clash = Un événement nommé { $name } existe déjà ici.

## Routine editor

routine-editor-title-edit-function = Modifier la fonction { $routine }
routine-editor-title-edit-procedure = Modifier la procédure { $routine }
routine-editor-title-create-function = Créer une fonction dans { $location }
routine-editor-title-create-procedure = Créer une procédure dans { $location }
routine-editor-section-function = Fonction
routine-editor-section-procedure = Procédure
routine-editor-section-body = Corps
routine-editor-section-options = Options
routine-editor-name = Nom
routine-editor-parameters = Paramètres
routine-editor-returns = Renvoie
routine-editor-returns-placeholder-mssql = int, ou TABLE
routine-editor-language = Langage
routine-editor-body = Corps
routine-editor-recompile = Recompiler
routine-editor-recompile-hint = WITH RECOMPILE — un nouveau plan à chaque appel, jamais un plan en cache.
routine-editor-schema-binding = Liaison au schéma
routine-editor-schema-binding-hint = WITH SCHEMABINDING — les tables et les vues qu'elle lit ne peuvent pas être modifiées à son insu.
routine-editor-execute-as = Exécuter en tant que
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER ou un utilisateur
routine-editor-null-input = Renvoie NULL sur une entrée NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — le corps ne s'exécute pas quand un argument est NULL.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatique
routine-editor-comment = Commentaire
routine-editor-volatility = Volatilité
routine-editor-strict = Stricte
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = S'exécute avec les droits du propriétaire plutôt que ceux de l'appelant. Fixez un search_path ci-dessous si vous l'utilisez.
routine-editor-security-definer-hint-mysql = S'exécute avec les droits du definer plutôt que ceux de l'appelant. C'est le comportement par défaut de MySQL — le désactiver correspond à SQL SECURITY INVOKER.
routine-editor-settings = Paramètres
routine-editor-add-setting = Ajouter un paramètre
routine-editor-deterministic = Déterministe
routine-editor-deterministic-hint = Garantit le même résultat pour les mêmes arguments. Un serveur avec journalisation binaire refuse une routine non déterministe, sauf s'il fait confiance aux créateurs.
routine-editor-data-access = Accès aux données
routine-editor-definer = Definer
routine-editor-reading-source = Lecture du source de la routine…
routine-editor-source-stale = Le véritable source de la routine est arrivé après que vous avez commencé à saisir : ce corps repose donc sur la copie du catalogue — qui n'est pas ce que contient le serveur. Fermez et rouvrez pour le modifier.
routine-problem-function-no-name = La fonction a besoin d'un nom.
routine-problem-procedure-no-name = La procédure a besoin d'un nom.
routine-problem-function-no-language = La fonction a besoin d'un langage (plpgsql, sql).
routine-problem-procedure-no-language = La procédure a besoin d'un langage (plpgsql, sql).
routine-problem-no-return-type = La fonction a besoin d'un type de retour.
routine-problem-procedure-returns = Une procédure ne renvoie rien — videz le type de retour.
routine-problem-function-no-body = La fonction a besoin d'un corps.
routine-problem-procedure-no-body = La procédure a besoin d'un corps.
routine-problem-bare-parameter = SQL Server nomme chaque paramètre avec @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Une procédure n'accepte pas WITH { $option }.
routine-problem-scalar-refuses = Une fonction scalaire n'accepte pas WITH { $option }.
routine-problem-inline-refuses = Une fonction table inline n'accepte pas WITH { $option }.
routine-problem-multi-refuses = Une fonction table à instructions multiples n'accepte pas WITH { $option }.
routine-problem-procedure-refuses-native = Une procédure n'accepte pas WITH { $option } une fois compilée en mode natif.
routine-problem-scalar-refuses-native = Une fonction scalaire n'accepte pas WITH { $option } une fois compilée en mode natif.
routine-problem-inline-refuses-native = Une fonction table inline n'accepte pas WITH { $option } une fois compilée en mode natif.
routine-problem-multi-refuses-native = Une fonction table à instructions multiples n'accepte pas WITH { $option } une fois compilée en mode natif.
routine-problem-native-needs-schemabinding = Une routine compilée en mode natif exige WITH SCHEMABINDING — SQL Server impose les deux ensemble.
routine-problem-inline-with-execute-as = INLINE = ON ne peut pas être combiné avec EXECUTE AS — une fonction qui s'exécute sous l'identité de quelqu'un d'autre n'est jamais inlinée.
routine-problem-numbered-rename = { $was } est en tête d'un groupe de procédures numérotées ({ $group }), et le renommer les supprimerait toutes. Gardez le nom pour la modifier.
routine-problem-trigger-function-args = Une fonction de déclencheur ne prend aucun argument déclaré — ils arrivent dans TG_ARGV.
routine-clash-function-refused = Une fonction nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : le serveur refuse donc celui-ci et rien ne change — choisissez un autre nom, ou supprimez d'abord { $name }.
routine-clash-procedure-refused = Une procédure nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : le serveur refuse donc celui-ci et rien ne change — choisissez un autre nom, ou supprimez d'abord { $name }.
routine-clash-function-destroys = Une fonction nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : l'appliquer détruirait donc { $original } et laisserait { $name } tel quel.
routine-clash-procedure-destroys = Une procédure nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : l'appliquer détruirait donc { $original } et laisserait { $name } tel quel.
routine-clash-function-destroys-new = Une fonction nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : l'appliquer détruirait donc l'original et laisserait { $name } tel quel.
routine-clash-procedure-destroys-new = Une procédure nommée { $name } existe déjà ici. Sur ce moteur, un renommage est une suppression suivie d'une création : l'appliquer détruirait donc l'original et laisserait { $name } tel quel.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Rechercher des comptes
users-loading-accounts = Chargement des comptes
users-unsupported = Le moteur de cette connexion n'a pas de comptes utilisateur.
users-none = Ce serveur ne signale aucun compte.
users-no-match = Aucun compte ne correspond à « { $needle } ».
users-pick-account = Choisissez un compte pour voir ce qu'il peut faire.
users-section-attributes = Attributs
users-section-privileges = Privilèges
users-attr-authentication = Authentification
users-attr-locked = Verrouillé
users-attr-password-expired = Mot de passe expiré
users-attr-superuser = Superutilisateur
users-attr-create-role = Créer des rôles
users-attr-create-database = Créer des bases de données
users-attr-replication = Réplication
users-attr-bypass-rls = Contourner la RLS
users-attr-inherit = Hériter
users-attr-connection-limit = Limite de connexions
users-attr-valid-until = Valide jusqu'au
users-attr-type = Type
users-attr-disabled = Désactivé
users-attr-default-database = Base de données par défaut
users-attr-server-roles = Rôles serveur
users-attr-login = Login
users-attr-default-schema = Schéma par défaut
users-attr-member-of = Membre de
users-attr-yes = Oui
users-attr-no = Non
users-login-type-sql = Login SQL
users-login-type-windows = Login Windows
users-login-type-windows-group = Groupe Windows
users-kind-user = Utilisateur
users-kind-role = Rôle
users-kind-login = Login
users-maintained-by-server = géré par le serveur
users-loading-privileges = Chargement des privilèges
users-no-privileges = Ce compte ne détient aucun privilège.
users-note-my-own-account = Cette connexion ne peut pas lire mysql.user : ceci n'est donc pas la liste des comptes du serveur — c'est le seul compte que cette connexion peut voir, le sien. Connectez-vous avec un compte disposant de SELECT sur la base mysql pour parcourir les autres.
users-note-ms-logins-elsewhere = Azure SQL Database conserve ses logins dans master, et ils ne sont pas gérés ici : la liste montre les utilisateurs et les rôles de la base de données.
users-note-ms-logins-only = Seuls les logins du serveur sont listés : choisissez une base de données pour voir ses utilisateurs et ses rôles.
users-note-ms-short = Ce login n'a pas VIEW ANY DEFINITION : le serveur ne lui montre donc que les principaux qu'il peut voir — la liste peut être incomplète.
users-note-my-scope = Les privilèges détenus via un rôle accordé ne sont pas développés : SHOW GRANTS liste ce qui est accordé directement au compte, et les rôles dont il est membre, mais pas ce que ces rôles détiennent eux-mêmes. Ouvrez les privilèges du rôle pour les voir.
users-note-pg-scope = Les privilèges sur les schémas, les tables et les séquences sont ceux de { $database } — PostgreSQL les conserve dans le catalogue propre à chaque base : les privilèges des autres bases ne sont donc pas listés. EXECUTE sur les fonctions et les procédures, et les privilèges accordés sur une seule colonne, ne sont pas listés non plus. Les privilèges détenus via l'appartenance à un rôle ou accordés à PUBLIC ne sont pas développés, pas plus que ceux qu'un rôle détient en étant propriétaire d'un objet ou superutilisateur.
users-note-pg-no-database = Les privilèges sur les schémas, les tables et les séquences ne sont pas listés : PostgreSQL les conserve dans le catalogue propre à chaque base, et cette connexion n'a aucune base de données sélectionnée. Ouvrez un onglet de requête sur une base de données pour les voir. EXECUTE sur les fonctions et les procédures, et les privilèges accordés sur une seule colonne, ne sont pas listés non plus. Les privilèges détenus via l'appartenance à un rôle ou accordés à PUBLIC ne sont pas développés, pas plus que ceux qu'un rôle détient en étant propriétaire d'un objet ou superutilisateur.
users-note-pg-superuser = Ce rôle est SUPERUSER : il contourne toutes les vérifications d'autorisation, et détient donc tous les privilèges sur tous les objets, que la liste ci-dessous le dise ou non.
users-note-pg-predefined = { $role } est l'un des rôles prédéfinis de PostgreSQL : ce qu'il peut faire est intégré au serveur au lieu d'être accordé, et n'est donc enregistré dans aucun catalogue lu ici.
users-note-pg-owns = { $n ->
    [one] Ce rôle est propriétaire de { $n } objet dans { $database } (schémas, tables, séquences ou la base elle-même). Un propriétaire détient tous les privilèges sur ce qu'il possède, et PostgreSQL n'enregistre aucun droit pour cela : rien de tout cela n'est donc listé ci-dessous.
   *[other] Ce rôle est propriétaire de { $n } objets dans { $database } (schémas, tables, séquences ou la base elle-même). Un propriétaire détient tous les privilèges sur ce qu'il possède, et PostgreSQL n'enregistre aucun droit pour cela : rien de tout cela n'est donc listé ci-dessous.
}
users-note-pg-owns-cluster = { $n ->
    [one] Ce rôle est propriétaire de { $n } objet dans ce cluster (schémas, tables, séquences ou la base elle-même). Un propriétaire détient tous les privilèges sur ce qu'il possède, et PostgreSQL n'enregistre aucun droit pour cela : rien de tout cela n'est donc listé ci-dessous.
   *[other] Ce rôle est propriétaire de { $n } objets dans ce cluster (schémas, tables, séquences ou la base elle-même). Un propriétaire détient tous les privilèges sur ce qu'il possède, et PostgreSQL n'enregistre aucun droit pour cela : rien de tout cela n'est donc listé ci-dessous.
}
users-note-ms-unshown = { $n ->
    [one] { $n } autorisation n'est pas affichée ici, sur { $classes } : le serveur la signale sur un élément sécurisable que cette liste ne sait pas nommer.
   *[other] { $n } autorisations ne sont pas affichées ici, sur { $classes } : le serveur les signale sur des éléments sécurisables que cette liste ne sait pas nommer.
}
users-note-ms-pick-database = Les autorisations d'un utilisateur de base de données sont celles de cette base — choisissez-en une pour les voir.
users-statements-capped = Affichage des { $shown } premières instructions sur { $total }. Copier les privilèges les prend toutes.
users-new-account = Nouveau compte
users-system-account = Ce compte appartient au serveur : Schemaic ne le modifiera donc pas.
users-reset-password = Réinitialiser le mot de passe
users-gate-no-database = Ouvrez un onglet de requête sur une base de données pour modifier les privilèges — l'instruction doit s'exécuter dans l'une d'elles.
users-gate-read-only = Cette connexion est en lecture seule.
users-count = { $n } { $n ->
    [one] compte
   *[other] comptes
}
users-count-filtered = { $shown } comptes sur { $n }
users-copy-privileges = Copier les privilèges

## Schema editors — shared (continued)

editor-row-remove = Retirer

## Trigger editor

trigger-editor-title = Déclencheurs sur { $table }
trigger-problem-no-name = Le déclencheur a besoin d'un nom.
trigger-problem-no-table = Un déclencheur a besoin d'une table sur laquelle se déclencher.
trigger-problem-no-event = Un déclencheur a besoin d'au moins un événement sur lequel se déclencher.
trigger-problem-pg-no-function = Un déclencheur PostgreSQL a besoin d'une fonction à exécuter.
trigger-problem-pg-body = Un déclencheur PostgreSQL exécute une fonction, pas un corps — choisissez-en une ou écrivez-en une à exécuter.
trigger-problem-truncate-row-level = Un déclencheur TRUNCATE doit être FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Un déclencheur INSTEAD OF doit être FOR EACH ROW.
trigger-problem-instead-of-view-only = Seule une vue peut avoir un déclencheur INSTEAD OF — une table prend BEFORE ou AFTER.
trigger-problem-pg-view-row-timing = Le déclencheur BEFORE ou AFTER d'une vue doit être FOR EACH STATEMENT — utilisez INSTEAD OF pour agir sur les lignes.
trigger-problem-update-of-without-update = UPDATE OF nomme des colonnes sur un déclencheur UPDATE.
trigger-problem-pg-instead-of-columns = Un déclencheur INSTEAD OF ne prend pas de liste de colonnes — PostgreSQL le déclenche sur tout UPDATE de la vue.
trigger-problem-one-event = { $engine } déclenche un déclencheur sur un seul événement — créez un déclencheur distinct par événement.
trigger-problem-no-truncate = { $engine } n'a pas de déclencheur TRUNCATE.
trigger-problem-sqlite-view-timing = Le déclencheur d'une vue doit être INSTEAD OF — SQLite n'a pas de déclencheur BEFORE ou AFTER sur une vue.
trigger-problem-sqlite-row-only = SQLite n'a que des déclencheurs FOR EACH ROW — il n'y en a pas au niveau de l'instruction.
trigger-problem-no-body = Un déclencheur { $engine } a besoin d'un corps.
trigger-problem-sqlite-not-block = Le corps d'un déclencheur SQLite doit être un bloc BEGIN … END contenant au moins une instruction.
trigger-problem-runs-body = Un déclencheur { $engine } exécute un corps, pas une fonction — écrivez les instructions à exécuter.
trigger-problem-ms-no-before = SQL Server n'a pas de déclencheur BEFORE — utilisez INSTEAD OF pour agir avant l'écriture.
trigger-problem-ms-view-timing = Sous SQL Server, le déclencheur d'une vue doit être INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server déclenche un déclencheur une fois par instruction — lisez les lignes depuis inserted et deleted.
trigger-problem-ms-instead-of-rank = Un déclencheur INSTEAD OF ne peut pas avoir d'ordre de déclenchement — remettez chaque événement sur N'importe quel ordre.
trigger-problem-first-without-event = Il se déclenche en premier sur { $event }, un événement pour lequel il n'est pas défini.
trigger-problem-last-without-event = Il se déclenche en dernier sur { $event }, un événement pour lequel il n'est pas défini.
trigger-problem-no-when = { $engine } n'a pas de condition WHEN — placez le test dans le corps avec IF.
trigger-problem-ms-no-update-of = SQL Server n'a pas de UPDATE OF — testez UPDATE(colonne) dans le corps.
trigger-problem-my-no-view = MySQL n'accepte pas de déclencheur sur une vue.
trigger-problem-my-no-instead-of = MySQL n'a pas de déclencheur INSTEAD OF — ils sont propres à PostgreSQL.
trigger-problem-my-no-update-of = MySQL n'a pas de UPDATE OF — un déclencheur voit toutes les colonnes.
trigger-problem-constraint-trigger = Schemaic ne peut pas modifier le déclencheur de contrainte { $name } — il ne modélise pas les paramètres de report qu'il porte.
trigger-problem-not-editable = Schemaic ne peut pas modifier le déclencheur { $name } — sa définition est chiffrée, masquée pour ce login, ou utilise une option que Schemaic ne modélise pas. Modifiez-le plutôt en SQL.
trigger-problem-twice = Deux déclencheurs s'appellent tous deux { $name }.
trigger-problem-first-clash = { $n ->
    [2] Un seul déclencheur peut se déclencher en premier sur { $event } — { $names } et { $last } y sont tous deux réglés.
   *[other] Un seul déclencheur peut se déclencher en premier sur { $event } — { $names } et { $last } y sont tous réglés.
}
trigger-problem-last-clash = { $n ->
    [2] Un seul déclencheur peut se déclencher en dernier sur { $event } — { $names } et { $last } y sont tous deux réglés.
   *[other] Un seul déclencheur peut se déclencher en dernier sur { $event } — { $names } et { $last } y sont tous réglés.
}
trigger-editor-preview-subject = déclencheurs sur { $table }
trigger-editor-none-selected = Aucun déclencheur sélectionné.
trigger-editor-readonly-constraint = Ceci est un déclencheur de contrainte. Schemaic ne modélise pas les paramètres de report qu'il porte : il peut être supprimé ici, mais pas modifié — modifiez-le plutôt en SQL.
trigger-editor-readonly-hidden = La définition de ce déclencheur est chiffrée ou masquée pour ce login : il peut être supprimé ici, mais pas modifié.
trigger-editor-readonly-unmodelled = Ce déclencheur utilise une option que Schemaic ne modélise pas : il peut être supprimé ici, mais pas modifié — modifiez-le plutôt en SQL.
trigger-editor-section-trigger = Déclencheur
trigger-editor-section-condition = Condition
trigger-editor-section-action = Action
trigger-editor-name = Nom
trigger-editor-timing = Moment
trigger-editor-event = Événement
trigger-editor-fires = Se déclenche
trigger-editor-truncate-hint = Au niveau de l'instruction uniquement.
trigger-editor-not-for-replication-hint = Inactif pendant qu'un agent de réplication écrit dans la table.
trigger-editor-rank-any = N'importe quel ordre
trigger-editor-rank-first = En premier
trigger-editor-rank-last = En dernier
trigger-editor-when = When
trigger-editor-of-columns = Sur les colonnes
trigger-editor-of-columns-placeholder = facultatif — toutes les colonnes si vide
trigger-editor-body = Corps
trigger-editor-function = Fonction
trigger-editor-edit-function = Modifier
trigger-editor-new-function = Nouvelle fonction
trigger-editor-arguments = Arguments
trigger-editor-add-argument = Ajouter un argument

## Schema editors — shared (continued)

editor-row-move-up = Monter
editor-row-move-down = Descendre

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Modifier : { $kind } { $name }
object-editor-title-create = Créer : { $kind } dans { $location }
object-editor-name = Nom
object-editor-comment = Commentaire
object-editor-type = Type
object-editor-default = Valeur par défaut
object-editor-section-type = Type
object-editor-section-values = Valeurs
object-editor-section-domain = Domaine
object-editor-section-constraints = Contraintes
object-editor-section-sequence = Séquence
object-editor-section-position = Position
object-editor-section-synonym = Synonyme
object-editor-section-alias-type = Type alias
object-editor-section-xml-schema-collection = Collection de schémas XML
object-editor-section-stands-for = Désigne
object-editor-value-placeholder = valeur
object-editor-add-value = Ajouter une valeur
object-editor-values-order = Dans l'ordre de comparaison — c'est l'ordre qu'utilise ORDER BY
object-editor-add-constraint = Ajouter une contrainte
object-editor-no-base-types = Ce moteur ne suggère aucun type de base
object-editor-not-null-hint = Toute colonne de ce domaine refuse NULL. L'activation échoue si l'une d'elles en contient déjà un.
object-editor-stores = Stocke
object-editor-increment = Incrément
object-editor-start = Début
object-editor-cache = Cache
object-editor-minimum = Minimum
object-editor-maximum = Maximum
object-editor-not-whole-number = { $field } doit être un nombre entier.
object-problem-type-no-name = Le type a besoin d'un nom.
object-problem-duplicate-value = La valeur { $value } figure plusieurs fois.
object-problem-empty-value = Une valeur ne peut pas être vide — PostgreSQL ne pourra jamais la retirer.
object-problem-domain-no-name = Le domaine a besoin d'un nom.
object-problem-domain-no-base = Un domaine a besoin d'un type sur lequel se baser.
object-problem-constraint-no-name = Chaque contrainte a besoin d'un nom.
object-problem-constraint-no-predicate = La contrainte { $name } n'a pas de prédicat.
object-problem-constraint-twice = La contrainte { $name } est nommée deux fois.
object-problem-sequence-no-name = La séquence a besoin d'un nom.
object-problem-zero-increment = Une séquence ne peut pas s'incrémenter de 0.
object-problem-min-above-max = Le minimum ({ $min }) est supérieur au maximum ({ $max }).
object-problem-start-outside = La valeur de début ({ $start }) est hors de { $min }..{ $max }.
object-problem-cache-below-one = Le cache doit être d'au moins 1.
object-problem-does-not-fit = { $min }..{ $max } ne tient pas dans { $data_type }.
object-problem-restart-outside = Redémarrer à { $restart } est hors de { $min }..{ $max }.
object-problem-counter-outside = La séquence en est à { $last }, hors de la nouvelle plage { $min }..{ $max }. Donnez-lui une valeur de redémarrage dans la plage.
object-problem-no-name = Le nom ne peut pas être vide.
object-problem-tsql-no-type = Une séquence a besoin d'un type.
object-problem-start-not-whole = Le début doit être un nombre entier.
object-problem-increment-not-whole = L'incrément doit être un nombre entier.
object-problem-minimum-not-whole = Le minimum doit être un nombre entier.
object-problem-maximum-not-whole = Le maximum doit être un nombre entier.
object-problem-tsql-zero-increment = L'incrément ne peut pas être 0.
object-problem-min-not-below-max = Le minimum doit être inférieur au maximum.
object-problem-started-outside = Elle a démarré à { $start }, hors de la nouvelle plage : redémarrez-la à une valeur comprise entre le minimum et le maximum.
object-problem-tsql-start-outside = Le début doit être compris entre le minimum et le maximum.
object-problem-cache-not-positive = La taille du cache doit être un nombre entier supérieur à 0.
object-problem-restart-not-whole = La valeur de redémarrage doit être un nombre entier.
object-problem-tsql-restart-outside = La valeur de redémarrage doit être comprise entre le minimum et le maximum.
object-problem-synonym-no-target = Un synonyme a besoin de l'objet qu'il désigne.
object-problem-alias-no-base = Un type alias a besoin d'un type de base.
object-editor-cycle = Cycle
object-editor-cycle-hint = Repartir à l'autre extrémité de la plage au lieu d'échouer quand la séquence est épuisée.
object-editor-restart-at = Redémarrer à — dernière valeur distribuée par le compteur : { $last }
object-editor-restart-at-unused = Redémarrer à — le compteur n'a pas été utilisé
object-editor-restart-not-whole = Redémarrer à doit être un nombre entier.
object-editor-restart-placeholder = laisser vide pour garder la position
object-editor-tsql-restart-placeholder = vide la conserve, sauf si le type change
object-editor-owned-by = Appartient à
object-editor-detach = Détacher de sa colonne
object-editor-detach-hint = La séquence n'est plus supprimée avec la colonne, et lui survit.
object-editor-cache-hint = Distribuer les valeurs depuis la mémoire, plus rapidement, au prix d'un trou après un redémarrage du serveur.
object-editor-cache-size = Taille du cache
object-editor-cache-size-placeholder = valeur par défaut du serveur
object-editor-synonym-server = Serveur
object-editor-synonym-server-placeholder = ce serveur
object-editor-synonym-database = Base de données
object-editor-synonym-database-placeholder = cette base de données
object-editor-synonym-schema = Schéma
object-editor-synonym-schema-placeholder = le schéma par défaut
object-editor-synonym-object = Objet
object-editor-synonym-object-placeholder = table, vue ou routine
object-editor-base-type = Type de base
object-editor-nullable = Accepte NULL
object-editor-nullable-hint = Une colonne de ce type accepte NULL, sauf indication contraire de la colonne.
object-editor-schemas = Schémas

## Account and grant editors (Users and privileges)

account-editor-title-create = Créer un compte
account-editor-title-reset = Réinitialiser le mot de passe
account-editor-title-privileges = Privilèges — { $account }
account-editor-account = Compte
account-editor-kind = Genre
account-editor-name = Nom
account-editor-host = Hôte
account-editor-host-placeholder = % (tout hôte)
account-editor-for-login = Pour le login
account-editor-for-login-placeholder = login — vide pour un utilisateur sans login
account-editor-also-user = Aussi utilisateur dans { $database }
account-editor-password = Mot de passe
account-editor-confirm = Confirmation
account-editor-action = Action
account-editor-action-grant = Accorder
account-editor-action-revoke = Révoquer
account-editor-subject = Objet
account-editor-subject-privileges = Privilèges
account-editor-subject-role = Rôle
account-editor-role = Rôle
account-editor-no-roles = Ce serveur n'a aucun rôle
account-editor-with-admin-option = Avec l'option d'administration
account-editor-level = Niveau
account-editor-level-global = Serveur entier
account-editor-level-database = Base de données
account-editor-level-schema = Schéma
account-editor-level-table = Table
account-editor-level-sequence = Séquence
account-editor-no-suggestions = Aucune suggestion
account-editor-section-privileges = Privilèges
account-editor-with-grant-option = Avec l'option d'attribution
account-editor-needs-role = Nommez un rôle.
account-editor-needs-privileges = Choisissez un niveau, nommez ce à quoi il s'applique et cochez un privilège.
account-editor-password-required = Un mot de passe est requis.
account-editor-name-required = Un nom est requis.
account-editor-entra-only-user = Seul un utilisateur de base de données peut être créé à partir de Microsoft Entra.
account-editor-entra-no-password = Un utilisateur Entra n'a ni login ni mot de passe ici — c'est Entra qui l'authentifie.
account-editor-login-needs-password = Un login a besoin d'un mot de passe.
account-editor-user-for-login-password = Un utilisateur lié à un login s'authentifie avec le mot de passe du login — laissez ce champ vide.
account-editor-confirm-password = Saisissez de nouveau le mot de passe pour le confirmer.
account-editor-passwords-differ = Les deux mots de passe diffèrent.
account-editor-password-hashed = Le SQL de l'aperçu, qui est l'instruction exécutée, contient un hachage du mot de passe sous la forme stockée par ce serveur, et non le mot de passe lui-même — sauf s'il contient des caractères que le login normaliserait (lettres pleine chasse, ligatures, espaces insécables), ou si le serveur utilise une extension de vérification des mots de passe : il est alors envoyé tel que saisi.
account-editor-password-as-typed = Le mot de passe apparaît dans le SQL de l'aperçu, qui est l'instruction exécutée.

## Table designer

designer-title-edit = Modifier { $table }
designer-title-create = Créer une table dans { $location }
designer-tab-table = Table
designer-tab-columns = Colonnes
designer-tab-indexes = Index
designer-tab-foreign-keys = Clés étrangères
designer-tab-checks = Contraintes CHECK
designer-list-add = Ajouter
designer-no-suggestions = Aucune suggestion
designer-name = Nom
designer-comment = Commentaire
designer-table-comment-placeholder = À quoi sert cette table
designer-engine = Moteur
designer-collation = Collation
designer-no-column = Aucune colonne sélectionnée.
designer-type = Type
designer-type-hint = Écrit tel quel — c'est le serveur qui décide de sa signification.
designer-nullable = Accepte NULL
designer-nullable-hint = Autoriser NULL dans cette colonne.
designer-primary-key = Clé primaire
designer-primary-key-hint = Fait partie de la clé primaire de la table, dans l'ordre des colonnes.
designer-identity = Identité
designer-auto-increment = Auto-incrément
designer-identity-hint = Le serveur attribue la valeur ({ $keyword }).
designer-default = Valeur par défaut
designer-default-hint = Du texte SQL, tel que vous l'écririez : 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Générée à partir de
designer-generated-hint = Une expression, p. ex. qty * price. Une colonne générée n'accepte pas de valeur par défaut.
designer-on-update = On update
designer-index-unique-detail = unique
designer-index-redundant-detail = redondant
designer-hint-redundant = L'index { $index } est couvert par { $cover } : toute recherche qu'il sert, celui-ci la sert aussi.
table-problem-no-name = La table a besoin d'un nom.
table-problem-no-columns = Une table a besoin d'au moins une colonne.
table-problem-column-no-name = Chaque colonne a besoin d'un nom.
table-problem-column-twice = Deux colonnes s'appellent toutes deux { $name }.
table-problem-column-no-type = La colonne { $name } a besoin d'un type.
table-problem-key-not-column = La clé primaire nomme { $column }, qui n'est pas une colonne.
table-problem-index-no-name = Chaque index a besoin d'un nom.
table-problem-index-twice = Deux index s'appellent tous deux { $name }.
table-problem-index-no-columns = L'index { $index } n'a aucune colonne.
table-problem-xml-one-column = L'index { $index } doit nommer exactement une colonne.
table-problem-xml-no-options = L'index { $index } ne peut pas être unique, inclure des colonnes ni être filtré : un index XML ou spatial n'accepte rien de tout cela.
table-problem-xml-no-using = L'index { $index } a besoin de sa clause USING XML INDEX … FOR … : un index XML secondaire est construit sur un index primaire ou sélectif.
table-problem-selective-no-paths = L'index { $index } a besoin de ses chemins FOR (…) : un index XML sélectif se résume aux chemins qu'il promeut.
table-problem-xml-parent-gone = L'index { $index } est construit sur { $parent }, qui n'est pas un index XML primaire ou sélectif de cette table : il disparaît avec son parent.
table-problem-not-orderable = L'index { $index } ne peut pas être ordonné : seul un index columnstore a un ORDER.
table-problem-order-not-column = L'index { $index } est ordonné par { $column }, qui n'est pas une colonne.
table-problem-order-not-stored = L'index { $index } est ordonné par { $column }, qu'il ne stocke pas.
table-problem-columnstore-unique = L'index { $index } ne peut pas être unique : un index columnstore stocke des colonnes, pas des clés.
table-problem-columnstore-include = L'index { $index } ne peut pas inclure de colonnes : un index columnstore liste toutes les colonnes qu'il stocke.
table-problem-columnstore-sorted = L'index { $index } ne peut pas trier ses colonnes : une colonne columnstore n'a pas de sens de tri.
table-problem-clustered-columnstore-filtered = L'index { $index } ne peut pas être filtré : un index columnstore cluster stocke toutes les lignes.
table-problem-clustered-include = L'index { $index } ne peut pas inclure de colonnes : un index cluster contient déjà toutes les colonnes.
table-problem-unique-include = L'index { $index } ne peut pas inclure de colonnes : une contrainte unique n'a pas d'INCLUDE ici.
table-problem-clustered-filtered = L'index { $index } ne peut pas être filtré : un index cluster couvre toutes les lignes.
table-problem-constraint-filtered = L'index { $index } ne peut pas être filtré : il soutient une contrainte UNIQUE, qui n'accepte pas de condition ici. Supprimez-le et ajoutez un index unique avec la condition.
table-problem-not-partial = L'index { $index } ne peut pas être filtré : cet index ne peut pas être partiel ici.
table-problem-index-not-column = L'index { $index } nomme { $column }, qui n'est pas une colonne.
table-problem-include-not-column = L'index { $index } inclut { $column }, qui n'est pas une colonne.
table-problem-cluster-clash = Une table ne peut avoir qu'un seul index cluster, or { $clustered } sont tous déclarés cluster - rendez-les tous non cluster sauf un.
table-problem-cluster-key = la clé primaire
table-problem-cluster-index = l'index { $index }
table-problem-cluster-and = et
table-problem-fk-no-name = Chaque clé étrangère a besoin d'un nom.
table-problem-fk-twice = Deux clés étrangères s'appellent toutes deux { $name }.
table-problem-fk-no-table = La clé étrangère { $fk } ne référence aucune table.
table-problem-fk-unpaired = La clé étrangère { $fk } doit associer chaque colonne à une colonne référencée.
table-problem-fk-not-column = La clé étrangère { $fk } nomme { $column }, qui n'est pas une colonne.
table-problem-check-no-name = Chaque contrainte CHECK a besoin d'un nom.
table-problem-check-twice = Deux contraintes CHECK s'appellent toutes deux { $name }.
table-problem-check-no-predicate = La contrainte CHECK { $name } n'a pas de prédicat.
table-problem-without-rowid-no-key = Une table WITHOUT ROWID a besoin d'une clé primaire.
table-problem-strict-type = Les tables STRICT n'acceptent que INT, INTEGER, REAL, TEXT, BLOB ou ANY — { $column } est { $data_type }.
designer-hint-unindexed-fk = La clé étrangère { $fk } n'a aucun index commençant par ses colonnes : une suppression ou une mise à jour de clé dans la table qu'elle référence parcourt donc celle-ci.
designer-no-index = Aucun index sélectionné. La clé primaire se définit sur les colonnes.
designer-columns = Colonnes
designer-index-columns-hint-single = L'unique colonne xml, geometry ou geography qu'il indexe.
designer-index-columns-hint-columnstore = Colonnes qu'il stocke, séparées par des virgules.
designer-index-columns-hint-prefix = Séparées par des virgules, dans l'ordre de la clé. bio(20) est une longueur de préfixe ; ajoutez DESC pour un tri décroissant.
designer-index-columns-hint = Séparées par des virgules, dans l'ordre de la clé. Ajoutez DESC pour une colonne décroissante.
designer-index-method = Méthode
designer-index-method-hint = Laisser vide pour la méthode par défaut (btree).
designer-index-filter = Seulement les lignes où
designer-index-filter-hint = La condition d'un index partiel, sans le WHERE.
designer-index-storage = Stockage
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Columnstore cluster
designer-storage-primary-xml = XML primaire
designer-storage-selective-xml = XML sélectif
designer-storage-secondary-xml = XML secondaire
designer-storage-spatial = Spatial
designer-index-using = Using
designer-index-using-hint = La clause après la colonne : le USING XML INDEX … FOR PATH, VALUE ou PROPERTY d'un index XML secondaire (ou FOR (path) sur un index sélectif), le [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …) d'un index XML sélectif, ou le USING … WITH (…) d'un index spatial.
designer-index-order = Ordre
designer-index-order-hint = Colonnes, séparées par des virgules, selon lesquelles les segments sont triés — à partir de SQL Server 2022, et de 2025 pour un index non cluster, qui ne trie que selon les colonnes qu'il stocke.
designer-index-include = Inclure
designer-index-include-hint = Colonnes, séparées par des virgules, portées par l'index sans faire partie de sa clé.
designer-index-unique = Unique
designer-index-unique-hint = Refuser les valeurs en double sur ces colonnes.
designer-fk-detail-unindexed = → { $table } · aucun index
designer-no-fk = Aucune clé étrangère sélectionnée.
designer-fk-ref-table = Table référencée
designer-fk-on-delete = On delete
designer-fk-columns-hint = Séparées par des virgules, associées dans l'ordre aux colonnes référencées.
designer-fk-ref-columns = Colonnes référencées
designer-no-check = Aucune contrainte CHECK sélectionnée.
designer-check-expression = Expression
designer-check-expression-hint = La condition que chaque ligne doit satisfaire. Écrite sans CHECK (…).
designer-check-enforced = Appliquée
designer-check-enforced-hint = Désactivée, la contrainte est enregistrée sans être appliquée — les lignes existantes comme les nouvelles sont acceptées.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Gérer les connexions
connections-new = Nouvelle connexion
connections-import = Importer des connexions
connections-export = Exporter les connexions
connections-duplicate = Dupliquer
connections-delete = Supprimer
connections-test = Tester
connections-save = Enregistrer
connections-test-failed = Échec de la connexion
connections-browse = Parcourir…
connections-section-general = Général
connections-section-safeguards = Garde-fous
connections-section-database = Base de données
connections-section-network = Réseau
connections-name = Nom
connections-colour = Couleur
connections-folder = Dossier
connections-no-folders = Aucun dossier pour l'instant
connections-folder-hint = Facultatif. Les connexions d'un même dossier sont listées ensemble.
connections-prominent-colour = Couleur visible dans l'éditeur
connections-prominent-colour-hint = Encadrer l'éditeur de requêtes de la couleur de cette connexion.
connections-read-only = Lecture seule
connections-read-only-hint = Désactiver la modification des cellules et bloquer les requêtes d'écriture et DDL sur cette connexion.
connections-read-only-mssql = SQL Server n'a pas de session en lecture seule : les lectures s'exécutent donc dans une transaction que Schemaic annule. L'effet d'une procédure hors de la base de données y survit ; pour une garantie, connectez-vous avec un login qui ne peut que lire.
connections-cli-access = Accès CLI
connections-cli-access-hint = Permettre à la ligne de commande schemaic, et aux agents IA via schemaic mcp, d'utiliser cette connexion.
connections-ai-data-access = Accès de l'IA aux données
connections-statement-timeout = Délai d'expiration des instructions
connections-statement-timeout-hint = Annule une instruction de cette connexion qui dure plus longtemps.
connections-statement-timeout-same = Comme l'application
connections-environment = Environnement
env-none = Aucun
env-local = Local
env-development = Développement
env-testing = Test
env-staging = Préproduction
env-production = Production
connections-type = Type
connections-init-sql = SQL d'initialisation
connections-init-sql-pragma-hint = Facultatif. Paramètres PRAGMA exécutés sur chaque connexion qu'ouvre Schemaic.
connections-init-sql-set-hint = Facultatif. Instructions SET exécutées sur chaque connexion qu'ouvre Schemaic — un search path, un fuseau horaire, un rôle.
connections-init-refused = « { $statement } » : { $why }
connections-init-only-pragmas = seuls des paramètres PRAGMA peuvent s'exécuter à la connexion
connections-init-only-sets = seules des instructions SET peuvent s'exécuter à la connexion
connections-init-calls-function = un paramètre ne peut pas appeler de fonction à la connexion
connections-init-quoted-name = le nom d'un paramètre ne peut pas être entre guillemets à la connexion
connections-init-names-no-setting = cela ne nomme aucun paramètre
connections-init-not-listed = { $name } ne peut pas être défini à la connexion — ce n'est pas l'un des paramètres de session dont Schemaic sait qu'ils laissent ses protections intactes
connections-init-not-utf8 = le jeu de caractères client doit rester UTF-8 — la vérification de lecture seule de Schemaic lit chaque instruction en UTF-8
connections-init-one-value = un paramètre prend une seule valeur à la connexion, et rien ne peut la suivre
connections-init-one-value-after-equals = un paramètre prend une seule valeur à la connexion, après un =
connections-init-transaction = les paramètres de transaction ne peuvent pas être définis à la connexion — la protection de lecture seule les définit
connections-init-pragma = ce PRAGMA ne peut pas s'exécuter à la connexion — il modifie le fichier ou la protection de lecture seule
connections-init-server-scope = les paramètres { $scope } modifient le serveur, pas la session
connections-init-variable = une variable n'est pas un paramètre de session — elle prend fin avec son lot
connections-init-on-only = { $name } ne peut qu'être activé à la connexion — Schemaic en dépend
connections-host = Hôte
connections-port = Port
connections-named-instance-port = Une instance nommée : SQL Server Browser fournit son port, Port n'est donc pas utilisé. Pour vous passer du Browser, indiquez l'hôte sans l'instance et le port sur lequel elle écoute.
connections-database = Base de données
connections-database-hint = Facultatif : la base de données dans laquelle la connexion s'ouvre. Certains hébergeurs n'autorisent que la leur et refusent une connexion qui ne la nomme pas.
connections-authentication = Authentification
ssh-auth-password = Mot de passe
ssh-auth-key-pair = Paire de clés
ssh-auth-agent = Agent SSH
auth-password = Utilisateur et mot de passe
auth-windows = Windows (ce compte)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Utilisateur
connections-password = Mot de passe
connections-azure-cli-hint = S'authentifie sous l'identité connectée à l'Azure CLI (exécutez d'abord `az login`). Le jeton est obtenu quand une connexion en a besoin et n'est jamais enregistré.
connections-aws-iam-hint = S'authentifie en tant que l'utilisateur ci-dessous avec un jeton tiré des informations d'identification de l'AWS CLI (d'abord `aws configure` ou `aws sso login`). L'utilisateur doit avoir le rôle rds_iam (PostgreSQL) ou l'AWSAuthenticationPlugin (MySQL). Le jeton est obtenu quand une connexion en a besoin et n'est jamais enregistré.
connections-gcp-iam-hint = S'authentifie en tant que l'utilisateur ci-dessous — l'utilisateur IAM ou le compte de service connu de Cloud SQL — avec un jeton de la Google Cloud CLI (d'abord `gcloud auth login`). Le jeton est obtenu quand une connexion en a besoin et n'est jamais enregistré.
connections-windows-hint-named = S'authentifie avec le compte Windows sous lequel s'exécute Schemaic ({ $account }). Le serveur doit connaître ce compte comme login Windows.
connections-windows-hint = S'authentifie avec le compte Windows sous lequel s'exécute Schemaic. Le serveur doit connaître ce compte comme login Windows.
connections-database-file = Fichier de base de données
connections-database-file-dialog = Sélectionner une base de données SQLite
connections-sqlite-hint = Une base de données SQLite est un simple fichier : il n'y a donc aucun serveur à joindre — ni hôte, ni utilisateur, ni mot de passe, ni tunnel. Les transactions manuelles n'y sont pas encore disponibles : les instructions sont validées dès leur envoi.
connections-ssh-tunnel = Tunnel SSH
connections-ssh-tunnel-hint = Joindre le serveur par un tunnel SSH.
connections-ssh-host = Hôte SSH
connections-ssh-port = Port SSH
connections-ssh-user = Utilisateur SSH
connections-ssh-password = Mot de passe SSH
connections-ssh-agent-hint = La signature est déléguée à votre agent SSH en cours d'exécution (agent OpenSSH / Pageant sous Windows). Schemaic ne stocke aucune clé.
connections-jump-hosts = Hôtes de rebond
connections-jump-hosts-hint = Facultatif. Hôtes à traverser avant l'hôte SSH, du plus proche au plus lointain — chacun s'authentifie de la manière définie ci-dessous.
connections-route-config = ~/.ssh/config : { $address }{ $key ->
        [some] , clé { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , via { $jump }
       *[none] {""}
    }
connections-route-jump-host = hôte de rebond { $host }
connections-route-proxy-command = ~/.ssh/config joint { $host } via une ProxyCommand, que Schemaic ne peut pas exécuter. Indiquez plutôt la route dans Hôtes de rebond.
connections-route-match = ~/.ssh/config peut acheminer { $host } via un bloc Match, que Schemaic ne lit pas. Indiquez plutôt la route dans Hôtes de rebond.
connections-route-match-destination = ~/.ssh/config peut rediriger { $host } vers un autre hôte ou port via un bloc Match, que Schemaic ne lit pas. Indiquez plutôt l'adresse elle-même comme hôte SSH.
connections-route-include = ~/.ssh/config inclut un fichier que Schemaic n'a pas pu lire, là où il s'applique à { $host }. Indiquez l'adresse elle-même comme hôte SSH, et la route dans Hôtes de rebond.
connections-route-config-jump-password = ~/.ssh/config joint { $host } via { $jump }, et un mot de passe n'est envoyé qu'aux hôtes que nomme cette connexion. Ajoutez l'hôte de rebond à Hôtes de rebond s'il accepte le même mot de passe, ou authentifiez-vous avec une clé ou l'agent SSH.
connections-route-jump-own-proxy-jump = ~/.ssh/config joint l'hôte de rebond { $host } via { $jump }, et Schemaic ne suit pas le ProxyJump propre à un hôte de rebond. Indiquez plutôt la route dans Hôtes de rebond, { $jump } en premier.
connections-jump-host-error = Hôte de rebond { $number } : { $why }
connections-jump-empty = vide — retirez la virgule en trop
connections-jump-space = « { $entry } » contient une espace
connections-jump-unclosed = « { $entry } » ouvre un [ qu'il ne ferme pas
connections-jump-after-bracket = « { $entry } » a du texte après son ]
connections-jump-no-host = « { $entry } » ne nomme aucun hôte
connections-jump-not-a-port = « { $entry } » n'est pas un port
connections-private-key = Clé privée
connections-private-key-dialog = Sélectionner une clé privée
connections-passphrase = Phrase secrète
connections-tls = SSL / TLS
tls-disable = Disable
tls-prefer = Prefer
tls-require = Require
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Se connecter en clair.
tls-prefer-description = Chiffrer si le serveur le propose. Aucune vérification.
tls-require-description = Refuser de se connecter sans chiffrement. Aucune vérification.
tls-verify-ca-description = Chiffrer, et vérifier le certificat auprès de la CA.
tls-verify-full-description = Chiffrer, vérifier le certificat et vérifier le nom d'hôte.
tls-verify-ca-checks-host = Sous MySQL et MariaDB, ce mode vérifie aussi le nom d'hôte et se comporte donc comme Verify full — une limite du pilote, pas un réglage.
tls-floor-azure-cli = L'authentification Microsoft Entra se connecte en Verify full quel que soit le choix fait ici : son jeton ouvre toutes les bases Azure SQL auxquelles le compte a accès.
tls-floor-aws-iam = L'authentification AWS IAM se connecte au moins en Require : son jeton ne doit pas traverser le réseau en clair. Verify full exige le bundle CA RDS comme CA racine.
tls-floor-gcp-iam = L'authentification Google Cloud IAM se connecte au moins en Verify CA — définissez la CA racine sur le certificat CA du serveur de l'instance : son jeton ouvre toutes les instances Cloud SQL auxquelles le compte a accès.
tls-floor-other = Cette authentification se connecte dans un mode plus strict que celui choisi ici.
connections-ca-certificate = Certificat CA
connections-ca-certificate-dialog = Sélectionner un certificat CA
connections-ca-certificate-hint = Laisser vide pour faire confiance aux autorités de certification approuvées par votre système d'exploitation, y compris celles installées par votre organisation. Indiquez un fichier pour faire plutôt confiance à celui-ci.
connections-client-certificate = Certificat client
connections-client-certificate-dialog = Sélectionner un certificat client
connections-client-key = Clé client
connections-client-key-dialog = Sélectionner une clé client
connections-client-identity-hint = Un certificat et une clé client ne sont nécessaires que si le serveur vous demande de prouver votre identité (TLS mutuel). La clé ne doit pas être protégée par une phrase secrète.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importer dans { $table }
import-section-source = Source
import-section-reading = Lecture
import-section-columns = Colonnes
import-section-preview = Aperçu
import-file = Fichier
import-choose-file = Choisir un fichier…
import-file-dialog = Importer dans la table
import-no-file = Aucun fichier choisi
import-unnamed-file = (fichier)
import-format = Format
import-sheet = Feuille
import-delimiter = Délimiteur
import-header-row = La première ligne est un en-tête
import-header-row-hint = Utiliser la première ligne comme noms de colonnes plutôt que comme données.
import-trim = Supprimer les espaces
import-trim-hint = Retirer les espaces autour de chaque valeur — y compris entre guillemets.
import-empty-is-null = Champ vide = NULL
import-empty-is-null-hint = Un champ vide signifie NULL plutôt qu'une chaîne vide.
import-null-tokens = Autres valeurs signifiant NULL
import-null-tokens-hint = Séparées par des virgules, p. ex. NULL, \N, NA.
import-skip = Ignorer
import-nothing-mapped = Rien n'est mis en correspondance : il n'y a donc rien à importer.
import-values-withheld = Le premier enregistrement de ce fichier est trop volumineux pour l'aperçu : seuls les noms de colonnes ont été lus. La mise en correspondance et l'import lisent tout le fichier.
import-missing-required = Non mises en correspondance, et ne pouvant pas être vides : { $columns }. L'import échouera sauf si la base de données les remplit.
import-engine-not-transactional = Le moteur de stockage de cette table ({ $engine }) n'est pas transactionnel : un import en échec ne peut donc pas être annulé — les lignes chargées avant l'échec resteront.
import-problems = { $count ->
    [one] { $count } problème dans le fichier — rien n'a été importé.
   *[other] { $count } problèmes dans le fichier — rien n'a été importé.
}
import-problem-at-line = ligne { $line } : { $message }
import-problem-at-column = ligne { $line }, { $column } : { $message }
import-issue-not-an-integer = pas un nombre entier
import-issue-not-a-number = pas un nombre
import-issue-not-a-boolean = pas une valeur vrai/faux
import-issue-cell-error = la feuille n'a pas pu évaluer cette cellule
import-issue-null-in-not-null = vide, mais la colonne ne peut pas être NULL
import-issue-field-count = a { $found } champs, { $expected } attendus
import-tail-cut = Affichage de { $shown } sur { $total }.
import-tail-uncounted = Affichage des { $total } — la vérification a cessé de compter là : le fichier en contient davantage.
import-tail-cut-and-uncounted = Affichage de { $shown } sur les { $total } premiers — la vérification a cessé de compter là : le fichier en contient davantage.
import-memory-json = Ce fichier JSON fait { $size }, et un import JSON est gardé en mémoire pendant le chargement — prévoyez qu'il ait besoin d'environ { $estimate }. Un CSV des mêmes données se charge en mémoire constante.
import-memory-xlsx = Ce classeur fait { $size }, et un import Excel est gardé en mémoire pendant le chargement — prévoyez qu'il ait besoin d'environ { $estimate }, car le fichier est compressé. Un CSV de la même feuille se charge en mémoire constante.
import-read-failed = Impossible de lire le fichier : { $error }
import-no-columns-mapped = Aucune colonne du fichier n'est mise en correspondance avec une colonne de la table
import-preview-record-too-large = son premier enregistrement JSON dépasse les { $mib } Mio lus par l'aperçu : il n'y a donc rien à afficher
import-refuse-not-objects = objets JSON attendus (un tableau d'objets, ou un par ligne)
import-refuse-oversize = Ce classeur fait { $size }, et un fichier Excel doit être lu en entier avant de pouvoir en afficher quoi que ce soit — { $max } est le maximum que cette fonction peut ouvrir. Exportez la feuille en CSV et importez-la plutôt ; un CSV se charge en mémoire constante.
import-refuse-oversize-unsized = Ce classeur dépasse { $max }, et un fichier Excel doit être lu en entier avant de pouvoir en afficher quoi que ce soit — { $max } est le maximum que cette fonction peut ouvrir. Exportez la feuille en CSV et importez-la plutôt ; un CSV se charge en mémoire constante.
import-refuse-inflated = Ce classeur fait { $size } sur disque mais se décompresse en au moins { $unpacked }, et un fichier Excel doit être lu en entier avant de pouvoir en afficher quoi que ce soit — { $ceiling } de contenu décompressé est le maximum que cette fonction ouvre, et il faut plus de mémoire que cela pour le contenir. Exportez la feuille en CSV et importez-la plutôt ; un CSV se charge en mémoire constante.
import-refuse-no-such-sheet = ce classeur n'a pas de feuille nommée « { $sheet } » — il contient { $sheets }
import-refuse-no-sheets = ce classeur n'a aucune feuille
import-refuse-too-wide = cette feuille déclare { $width } colonnes ; une feuille de calcul Excel en contient { $max }. Exportez-la en CSV et importez-la plutôt.
import-cancelled = Import annulé — la transaction a été annulée : rien n'a été écrit.
import-done = { $count ->
    [one] { $count } ligne importée dans { $table }.
   *[other] { $count } lignes importées dans { $table }.
}
import-next = Suivant
import-reading = Lecture…
import-back = Retour
import-import = Importer
import-importing = Import en cours…
dialog-read-only = Cette connexion est en lecture seule.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Exporter { $database }.{ $schema } en { $format }
dump-title = Exporter { $database } en { $format }
dump-sql-dialog = Exporter en SQL
dump-folder-dialog = Exporter en { $format }
dump-folder-hint = Un fichier { $format } par table, écrit dans un dossier de votre choix.
dump-all-selected = Les { $total } sélectionnées
dump-some-selected = { $chosen } sur { $total } sélectionnées
dump-select-all = Tout sélectionner
dump-select-none = Aucune
dump-reading-tables = Lecture de la liste des tables…
dump-no-tables = Cette base de données n'a aucune table.
dump-tables-unreadable = La liste des tables n'a pas pu être lue.
dump-section-contents = Contenu
dump-structure = Structure
dump-structure-hint = CREATE TABLE, ses déclencheurs, et les clés étrangères, rétablies après les données.
dump-data = Données
dump-data-hint = Toutes les lignes, sous forme d'instructions INSERT.
dump-other-objects = Autres objets
dump-other-objects-hint = Types, séquences, routines et événements dont dépendent ces tables.
dump-section-replaying = Rejeu
dump-drop-before-create = Supprimer avant de créer
dump-drop-hint-up-front = Supprimer chaque table et chaque vue avant son CREATE, pour que le fichier se charge sur une base qui les contient déjà. Avec Une seule transaction, il remplace aussi toutes les routines de leurs schémas, et s'arrête avant de supprimer quoi que ce soit qu'il ne peut pas rétablir à l'identique. Sans Une seule transaction, il laisse les routines telles quelles : un rejeu sur une base qui les contient encore s'arrête donc à la première. Le propriétaire et les autorisations de chaque table sont rétablis à la fin.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS avant chaque CREATE, pour que le fichier se charge sur une base qui contient déjà ces tables. Il remplace aussi toutes les routines de leurs schémas. Le propriétaire, les droits et la sécurité au niveau des lignes de chaque table sont rétablis à la fin.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS avant chaque CREATE, pour que le fichier se charge sur une base qui contient déjà ces tables. Il remplace aussi toutes les routines et tous les événements de leurs schémas : sous MySQL, chacun est d'abord supprimé, ce qui révoque tous les droits accordés dessus, et le fichier les accorde de nouveau à la fin si votre compte peut lire mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS avant chaque CREATE, pour que le fichier se charge sur une base qui contient déjà ces tables.
dump-one-transaction = Une seule transaction
dump-one-transaction-hint = Englober le chargement, pour qu'un échec ne laisse rien derrière lui.
dump-disable-fk-checks = Désactiver la vérification des clés étrangères
dump-fk-hint = Désactivée pendant le chargement, pour que l'ordre d'arrivée des lignes ne puisse pas les faire refuser.
dump-fk-hint-drops = Désactivée pendant le chargement, pour que l'ordre d'arrivée des lignes ne puisse pas les faire refuser. Un fichier qui supprime des tables la désactive de toute façon : une suppression avec la vérification active efface les lignes de tables hors du fichier via leurs actions ON DELETE.
dump-no-fk-guard-hint = Ce moteur n'a pas d'interrupteur de session pour la vérification des clés étrangères ; le fichier ajoute plutôt les contraintes après les données.
dump-export = Exporter
dump-progress = { $count ->
    [one] Écriture de { $table } — { $index } sur { $total }, { $rows } ligne jusqu'ici
   *[other] Écriture de { $table } — { $index } sur { $total }, { $rows } lignes jusqu'ici
}
dump-reading-schema = Lecture du schéma
dump-replace-files = Remplacer les fichiers
dump-replace-prompt = { $n ->
    [one] { $n } fichier de { $folder } sera remplacé, et il ne pourra pas être récupéré : { $names }.
   *[other] { $n } fichiers de { $folder } seront remplacés, et ils ne pourront pas être récupérés : { $names }.
}
dump-replace-prompt-more = { $n } fichiers de { $folder } seront remplacés, et ils ne pourront pas être récupérés : { $names }, et { $more } de plus.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] { $n } table écrite.
   *[other] { $n } tables écrites.
}
dump-done-missing = { $n ->
    [one] { $n } table cochée est introuvable et ne figure pas dans le fichier : { $names }.
   *[other] { $n } tables cochées sont introuvables et ne figurent pas dans le fichier : { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } objet demandé par l'export ne figure pas dans le fichier : { $names }.
   *[other] { $n } objets demandés par l'export ne figurent pas dans le fichier : { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Les lignes de la table d'arêtes de graphe { $names } ne figurent pas dans le fichier ; elle est créée vide.
   *[other] Les lignes des tables d'arêtes de graphe { $names } ne figurent pas dans le fichier ; elles sont créées vides.
}
dump-done-refused = { $n ->
    [one] Un rejeu sur une base qui contient déjà cet objet s'arrête avant de supprimer quoi que ce soit, car le fichier ne peut pas le rétablir à l'identique : { $names }.
   *[other] Un rejeu sur une base qui contient déjà l'un de ces objets s'arrête avant de supprimer quoi que ce soit, car le fichier ne peut pas les rétablir à l'identique : { $names }.
}
dump-cancelled-partial = Export annulé — { $name } n'a pas été modifié ; ce qui avait été écrit se trouve dans { $part }
files-done = { $n ->
    [one] { $n } fichier écrit.
   *[other] { $n } fichiers écrits.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Export annulé — aucun fichier n'a été terminé : rien n'a donc été écrit dans { $folder }.
files-cancelled = { $n ->
    [one] Export annulé — { $n } fichier a été terminé dans { $folder } ; la table en cours n'a pas été écrite.
   *[other] Export annulé — { $n } fichiers ont été terminés dans { $folder } ; la table en cours n'a pas été écrite.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } fichier déjà écrit dans { $folder } est conservé.
   *[other] { $message } — { $n } fichiers déjà écrits dans { $folder } sont conservés.
}
files-replaced = { $n ->
    [one] { $n } fichier existant a été remplacé : { $names }.
   *[other] { $n } fichiers existants ont été remplacés : { $names }.
}
files-missing = { $n ->
    [one] { $n } table cochée est introuvable et n'a donné aucun fichier : { $names }.
   *[other] { $n } tables cochées sont introuvables et n'ont donné aucun fichier : { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } sur ~{ $total } ligne
   *[other] { $rows } sur ~{ $total } lignes
}
export-progress-of = { $count ->
    [one] { $rows } sur { $total } ligne
   *[other] { $rows } sur { $total } lignes
}
export-progress-rows = { $count ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
}
export-progress-starting = Démarrage
export-progress-file = Fichier : { $name }
export-progress-writing = Écriture de { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Moniteur en direct
monitor-badge-watching = Surveillance
monitor-badge-paused = En pause
monitor-badge-stopped = Arrêté
monitor-filter-all = Tout
monitor-filter-inserts = Insertions
monitor-filter-updates = Mises à jour
monitor-filter-deletes = Suppressions
monitor-resume = Reprendre l'interrogation
monitor-pause = Suspendre l'interrogation
monitor-clear-tip = Effacer le journal
monitor-export-tip = Exporter le journal…
monitor-interval = { $secs } s
monitor-clear-title = Effacer le journal ?
monitor-clear-message = { $count ->
    [one] { $count } modification serait abandonnée. Le journal est la seule trace de ce que contenait une ligne supprimée — l'interrogation ne signalera plus cette modification — et cette action est irréversible. Exportez-le d'abord si vous voulez le conserver.
   *[other] { $count } modifications seraient abandonnées. Le journal est la seule trace de ce que contenait une ligne supprimée — l'interrogation ne signalera plus ces modifications — et cette action est irréversible. Exportez-le d'abord si vous voulez le conserver.
}
monitor-col-time = Heure
monitor-col-action = Action
monitor-no-inserts = Aucune insertion pour l'instant.
monitor-no-updates = Aucune mise à jour pour l'instant.
monitor-no-deletes = Aucune suppression pour l'instant.
monitor-empty-stopped = Arrêté.
monitor-empty-paused = En pause.
monitor-waiting = En attente
monitor-events-since = { $count ->
    [one] { $count } événement depuis { $since }
   *[other] { $count } événements depuis { $since }
}
monitor-no-changes = Aucune modification pour l'instant
monitor-partial = seules les { $cap } premières lignes par clé primaire sont surveillées
monitor-capped = les plus anciennes sont abandonnées au-delà de { $cap } modifications
monitor-length-only = { $count ->
    [one] { $columns } est binaire : ses modifications ne sont vues que par leur longueur
   *[other] { $columns } sont binaires : leurs modifications ne sont vues que par leur longueur
}
monitor-newest-at-bottom = les plus récentes en bas
monitor-export-dialog = Exporter le journal des modifications

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Comparer les résultats
diff-filter-all = Tout
diff-filter-changed = Modifiées
diff-filter-added = Ajoutées
diff-filter-removed = Retirées
diff-match-rows-by = Associer les lignes par
diff-whole-row = Ligne entière
diff-export-tip = Exporter la comparaison…
diff-nothing-to-export = Rien à exporter.
diff-export-dialog = Exporter la comparaison
diff-unnamed-file = la comparaison
diff-ran-at = Exécutée à { $time }
diff-side-facts = { $count ->
    [one] { $count } ligne · { $ms } ms
   *[other] { $count } lignes · { $ms } ms
}
diff-only-in = Seulement dans { $side } : { $columns }
diff-uncompared = Binaire, non comparé : { $columns }
diff-missing-key = les deux résultats ont besoin d'une colonne nommée { $column } pour associer les lignes
diff-binary-key = { $column } contient des valeurs binaires, qui ne sont pas comparées : elle ne peut donc pas associer les lignes
diff-summary-identical = { $n ->
        [one] Aucune différence · { $n } ligne inchangée
       *[other] Aucune différence · { $n } lignes inchangées
    }
diff-summary-changed = { $n } modifiées
diff-summary-added = { $n } ajoutées
diff-summary-removed = { $n } retirées
diff-summary-unchanged = { $n } inchangées
diff-summary-not-compared = { $columns } non comparées
diff-repeated-keys = { $count ->
    [one] { $count } ligne répète une clé déjà prise de son côté, et est écartée.
   *[other] { $count } lignes répètent une clé déjà prise de leur côté, et sont écartées.
}
diff-capped = Un résultat a été tronqué : les lignes au-delà de sa limite ne sont pas comparées.
diff-col-change = Modification
diff-no-differences = Aucune différence.
diff-no-changed = Aucune ligne modifiée.
diff-no-added = Aucune ligne ajoutée.
diff-no-removed = Aucune ligne retirée.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Avant d'appliquer
preview-risk-irreversible = Cette action est irréversible

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Créer la table { $name } avec { $columns } colonne
   *[other] Créer la table { $name } avec { $columns } colonnes
}
ddl-line-rebuild-table = Reconstruire { $name } — impossible à modifier sur place : les lignes sont donc copiées dans une nouvelle table qui prend la place de l'ancienne
ddl-line-drop-table = Supprimer la table
ddl-line-drop-foreign-table = Supprimer la table étrangère
ddl-line-truncate-table = Supprimer toutes les lignes
ddl-line-rename-table = Renommer la table en { $to }
ddl-line-add-column = Ajouter la colonne { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Supprimer la colonne { $name }
ddl-line-rebuild-computed-column = Reconstruire la colonne calculée { $name } comme ({ $expression }) autour de la modification de colonne
ddl-line-rename-column = Renommer la colonne { $from } en { $to }{ $retyped ->
        [yes] , et changer son type de { $old_type } en { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Placer la colonne { $name } en premier
ddl-line-move-column-after = Placer la colonne { $name } après { $after }
ddl-line-move-column = Déplacer la colonne { $name }
ddl-line-retype-column = Changer la colonne { $name } de { $from } en { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Modifier la colonne { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (aucun)
ddl-line-add-primary-key = Ajouter la clé primaire ({ $columns })
ddl-line-drop-primary-key = Supprimer la clé primaire ({ $columns })
ddl-line-rebuild-primary-key = Reconstruire la clé primaire ({ $columns }) autour de la modification de colonne
ddl-line-change-primary-key = Changer la clé primaire de ({ $from }) en ({ $to })
ddl-line-add-index = Ajouter l'index { $has_kind ->
        [yes] { $kind }{" "}
       *[no] {""}
    }{ $unique ->
        [yes] unique{" "}
       *[no] {""}
    }{ $name }{ $has_method ->
        [yes] {" "}avec { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}sur ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}incluant ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}ordonné par ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}où { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = columnstore cluster
ddl-index-kind-primary-xml = XML primaire
ddl-index-kind-selective-xml = XML sélectif
ddl-index-kind-secondary-xml = XML secondaire
ddl-index-kind-spatial = spatial
ddl-line-drop-index = Supprimer l'index { $name }
ddl-line-rename-index = Renommer l'index { $from } en { $to }
ddl-line-keep-lossy-index = Laisser l'index { $name } inchangé — Schemaic ne peut pas le lire en entier
ddl-line-add-foreign-key = Ajouter la clé étrangère { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Supprimer la clé étrangère { $name }
ddl-line-add-check = Ajouter la contrainte CHECK { $name } ({ $expression })
ddl-line-drop-check = Supprimer la contrainte CHECK { $name }
ddl-line-table-options = Définir pour la table : { $parts }
ddl-table-option-engine = moteur { $engine }
ddl-table-option-collation = collation { $collation }
ddl-table-option-comment = commentaire
ddl-table-option-comment-cleared = commentaire (effacé)
ddl-line-create-view = Créer la vue { $name }
ddl-line-create-view-index = Créer la vue { $name } et son index { $index }
ddl-line-create-view-indexes = Créer la vue { $name } et ses index { $indexes }
ddl-line-redefine-view = Redéfinir la vue { $name }
ddl-line-recreate-view = Supprimer et recréer la vue { $name }
ddl-line-rename-view = Renommer la vue en { $to }
ddl-line-drop-view = Supprimer la vue
ddl-line-drop-materialized-view = Supprimer la vue matérialisée
ddl-line-refresh-view = Reconstruire les lignes de la vue matérialisée — les lecteurs sont bloqués jusqu'à la fin
ddl-line-refresh-view-concurrently = Reconstruire les lignes de la vue matérialisée sans bloquer les lecteurs
ddl-line-create-trigger = Créer le déclencheur { $name }
ddl-line-redefine-trigger = Redéfinir le déclencheur { $name }
ddl-line-recreate-trigger = Recréer le déclencheur { $name }
ddl-line-recreate-trigger-as = Recréer le déclencheur { $from } sous le nom { $name }
ddl-line-drop-trigger = Supprimer le déclencheur { $name }
ddl-line-create-routine = Créer la { $what } { $name }
ddl-line-redefine-routine = Redéfinir la { $what } { $name }
ddl-line-redefine-routine-as = Redéfinir la { $what } { $from } sous le nom { $name }
ddl-line-recreate-routine = Recréer la { $what } { $name }
ddl-line-recreate-routine-as = Recréer la { $what } { $from } sous le nom { $name }
ddl-line-rename-routine = Renommer la { $what } { $from } en { $to }
ddl-line-drop-routine = Supprimer la { $what } { $name }
ddl-line-create-event = Créer l'événement { $name }
ddl-line-alter-event = Modifier l'événement { $name }
ddl-line-alter-event-edits = Modifier l'événement { $name } : { $edits }
ddl-line-drop-event = Supprimer l'événement { $name }
ddl-event-edit-rename = renommer en { $to }
ddl-event-edit-schedule = planification : { $schedule }
ddl-event-edit-status-enabled = état : activé
ddl-event-edit-status-disabled = état : désactivé
ddl-event-edit-status-replica = état : désactivé sur le réplica
ddl-event-edit-preserve = le conserver une fois terminé
ddl-event-edit-no-preserve = le supprimer une fois terminé
ddl-event-edit-definer = definer : { $definer }
ddl-event-edit-comment = commentaire
ddl-event-edit-body = corps
ddl-line-create-enum = { $values ->
    [one] Créer le type { $name } avec { $values } valeur
   *[other] Créer le type { $name } avec { $values } valeurs
}
ddl-line-add-enum-value = Ajouter la valeur { $value }
ddl-line-add-enum-value-after = Ajouter la valeur { $value } après { $after }
ddl-line-add-enum-value-before = Ajouter la valeur { $value } avant { $before }
ddl-line-rename-enum-value = Renommer la valeur { $from } en { $to }
ddl-line-recreate-enum = Recréer le type { $name } ({ $values ->
    [one] { $values } valeur
   *[other] { $values } valeurs
}, conversion de { $columns ->
    [one] { $columns } colonne
   *[other] { $columns } colonnes
})
ddl-line-create-domain = Créer le domaine { $name } comme { $datatype }
ddl-line-set-domain-default = Définir la valeur par défaut du domaine à { $default }
ddl-line-drop-domain-default = Supprimer la valeur par défaut du domaine
ddl-line-domain-not-null = Rendre le domaine NOT NULL
ddl-line-domain-nullable = Rendre le domaine nullable
ddl-line-recreate-domain = Recréer le domaine { $name } comme { $datatype } (auparavant { $was } ; conversion de { $columns ->
    [one] { $columns } colonne
   *[other] { $columns } colonnes
})
ddl-line-create-sequence = Créer la séquence { $name }
ddl-line-alter-sequence = Définir pour la séquence : { $edits }
ddl-sequence-edit-type = type { $datatype }
ddl-sequence-edit-increment = incrément { $value }
ddl-sequence-edit-minimum = minimum { $value }
ddl-sequence-edit-maximum = maximum { $value }
ddl-sequence-edit-start = début { $value }
ddl-sequence-edit-cache = cache { $value }
ddl-sequence-edit-cycling-on = cycle activé
ddl-sequence-edit-cycling-off = cycle désactivé
ddl-sequence-edit-owner = propriétaire { $owner }
ddl-sequence-edit-no-owner = aucun propriétaire
ddl-line-restart-sequence = Redémarrer la séquence à { $value }
ddl-line-create-object = Créer : { $what } { $name }
ddl-line-replace-object = Supprimer puis recréer : { $what } { $name }
ddl-line-replace-object-as = Supprimer puis recréer : { $what } { $name }, sous le nom { $to }
ddl-line-alter-tsql-sequence = Modifier la séquence : { $clauses }
ddl-line-rename-object = Renommer ({ $what }) en { $to }
ddl-line-drop-object = Supprimer ({ $what })
ddl-line-set-object-comment = Définir le commentaire ({ $what })
ddl-line-clear-object-comment = Effacer le commentaire ({ $what })
ddl-line-create-database = Créer la base de données { $name }
ddl-line-drop-database = Supprimer la base de données { $name }
ddl-line-create-schema = Créer le schéma { $name }
ddl-line-drop-schema = Supprimer le schéma { $name }
ddl-line-create-extension = Créer l'extension { $name }
ddl-line-create-user = Créer l'utilisateur { $who }
ddl-line-create-role = Créer le rôle { $who }
ddl-line-create-login = Créer le login { $who }
ddl-line-drop-user = Supprimer l'utilisateur { $who }
ddl-line-drop-role = Supprimer le rôle { $who }
ddl-line-drop-login = Supprimer le login { $who }
ddl-line-set-password = Définir le mot de passe de { $who }
ddl-line-grant-privileges = Accorder { $privileges } à { $who }
ddl-line-revoke-privileges = Révoquer { $privileges } de { $who }
ddl-privileges-none = aucun privilège
ddl-privileges-count = { $count } privilèges
ddl-line-grant-role = Accorder { $role } à { $member }
ddl-line-revoke-role = Révoquer { $role } de { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Supprime { $name } et la recrée en copiant toutes les lignes, puis rétablit ses clés, index, contraintes CHECK, valeurs par défaut et déclencheurs, ses commentaires et ceux de ses colonnes, ainsi que les clés étrangères d'autres tables qui pointent vers elle. L'opération s'arrête avant de commencer si la table a quoi que ce soit d'autre que Schemaic ne réécrit pas — des autorisations, un propriétaire propre, un dépendant lié au schéma, le versionnage système, un registre (ledger), la réplication, la compression, des options d'index, un index désactivé, une règle ou une valeur par défaut liée, une classification de sensibilité, une description sur une clé, un index, une valeur par défaut, une contrainte CHECK ou un déclencheur. Les statistiques créées à la main ne sont pas reportées.
       *[other] Supprime { $name } et la recrée en copiant toutes les lignes. Tout ce que Schemaic n'a pas lu de la table n'est pas reporté.
    }
ddl-risk-drop-table = Supprime la table et toutes ses lignes.
ddl-risk-drop-foreign-table = Supprime la table étrangère : sa définition disparaît, et les lignes qu'elle lit restent où elles sont, sur le serveur étranger ou dans le fichier ; une vue ou une routine qui la nomme échoue dès lors.
ddl-risk-truncate-table = Supprime toutes les lignes de la table. Cette action est irréversible.
ddl-risk-drop-column = Supprime la colonne { $name } et toutes ses données.
ddl-risk-rebuild-computed-column = Supprime la colonne calculée { $name } puis la rajoute : elle passe donc en fin de table.
ddl-risk-column-not-null = La colonne { $name } devient NOT NULL — l'instruction échoue si une ligne contient NULL.
ddl-risk-column-stops-generated = La colonne { $name } cesse d'être générée. Ses valeurs actuelles sont conservées, mais plus rien ne les recalcule désormais.
ddl-risk-column-becomes-generated = La colonne { $name } devient générée à partir de { $expression }. Chacune de ses valeurs est remplacée par le résultat de l'expression.
ddl-risk-column-expression-changes = L'expression de la colonne { $name } passe de { $from } à { $to }. Sous PostgreSQL, cela supprime puis rajoute la colonne : chaque valeur est donc recalculée et la colonne passe en fin de table.
ddl-risk-type-narrows = Restreindre { $subject } de { $from } à { $to } { $loss ->
        [truncates] tronque les valeurs qui ne tiennent plus
        [rounds] arrondit toutes les valeurs de la colonne
       *[both] tronque les valeurs qui ne tiennent plus et arrondit toutes les valeurs de la colonne
    }.
ddl-risk-type-rewrites = Changer { $subject } de { $from } à { $to } réécrit chaque valeur ; cela peut échouer ou perdre en précision.
ddl-risk-domain-subject = le domaine { $name }
ddl-risk-primary-key-dropped = Laisse la table sans clé primaire — les lignes ne peuvent plus être modifiées depuis la grille.
ddl-risk-primary-key-rebuilt = Supprime puis rajoute la clé primaire autour de la modification de colonne. Si c'est l'index cluster, cela réécrit toute la table, et une clé étrangère d'une autre table qui la référence arrête le plan.
ddl-risk-primary-key-replaced = Remplace la clé primaire : { $count ->
        [one] { $columns } n'est plus unique
       *[other] { $columns } ne sont plus uniques
    }, et les doublons sont désormais acceptés. Sous MySQL, cela reconstruit l'index cluster, ce qui réécrit toute la table.
ddl-risk-drop-foreign-key = Supprime la clé étrangère { $name }. Les lignes sans parent correspondant sont désormais acceptées, et les données existantes ne sont pas réexaminées si elle est rajoutée.
ddl-risk-drop-unique-index = Supprime l'index unique { $name }. Les valeurs en double sont désormais acceptées, et la grille peut perdre la clé par laquelle elle modifiait les lignes.
ddl-risk-drop-view = { $materialized ->
        [yes] Supprime la vue matérialisée. { $cost }
       *[no] Supprime la vue. { $cost }
    }
ddl-risk-view-cost-sqlite = Les vues qui la sélectionnent ne se résolvent plus tant qu'elle n'est pas rétablie, et ses déclencheurs INSTEAD OF sont supprimés avec elle et ne sont pas rétablis.
ddl-risk-view-cost-sqlite-restored = Les vues qui la sélectionnent ne se résolvent plus tant qu'elle n'est pas rétablie, et ses déclencheurs INSTEAD OF sont supprimés avec elle.
ddl-risk-view-cost-sqlserver = Les droits accordés dessus, sa description et ses déclencheurs INSTEAD OF sont supprimés avec elle et ne sont pas rétablis.
ddl-risk-view-cost-sqlserver-restored = Les droits accordés dessus et sa description sont supprimés avec elle et ne sont pas rétablis ; ses déclencheurs INSTEAD OF sont aussi supprimés.
ddl-risk-view-cost-other = Les vues dépendantes, les règles, les droits accordés et ses déclencheurs INSTEAD OF sont supprimés avec elle et ne sont pas rétablis.
ddl-risk-view-cost-other-restored = Les vues dépendantes, les règles et les droits accordés sont supprimés avec elle et ne sont pas rétablis ; ses déclencheurs INSTEAD OF sont aussi supprimés.
ddl-risk-recreate-view = Recréer { $name } commence par la supprimer. { $cost } { $why ->
        [sqlite] SQLite ne peut pas remplacer une vue sur place : toute modification — renommage compris — passe donc par cette voie.
        [sqlserver] Le sp_rename de SQL Server laisserait la définition stockée de la vue nommer l'ancien nom : un renommage passe donc par cette voie.
       *[postgres] PostgreSQL ne peut pas remplacer une vue dont les colonnes ont changé de nom, de type ou d'ordre : c'est donc la seule façon d'appliquer la modification.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } instruction supprimée avec elle est rétablie ensuite, dans la même transaction.
       *[other] { $count } instructions supprimées avec elle sont rétablies ensuite, dans la même transaction.
    }
ddl-risk-view-indexes-dropped = Redéfinir la vue { $view } supprime ses { $dropped_count ->
        [one] index
       *[other] index
    } ({ $dropped }) — l'ALTER VIEW de SQL Server le fait toujours — { $case ->
        [all] et ce plan { $again_count ->
            [one] le
           *[other] les
        } reconstruit ensuite, dans la même transaction. Sur une grande vue, cela prend autant de temps que { $again_count ->
            [one] sa
           *[other] leur
        } construction initiale. Un index avec un facteur de remplissage, une compression, un remplissage (padding), des options de verrouillage, un groupe de fichiers ou une description arrête le plan avant toute modification — Schemaic ne sait pas les reconstruire. Les statistiques créées à la main sur la vue ne sont pas reportées.
        [none] et ce plan ne { $gone_count ->
            [one] le
           *[other] les
        } reconstruit pas : la vue à laquelle il doit correspondre n'a pas d'index de ce type. Les statistiques créées à la main sur la vue ne sont pas reportées.
       *[some] et ce plan reconstruit ensuite { $again }, dans la même transaction, mais pas { $gone }, que la vue à laquelle il doit correspondre n'a pas. Un index avec un facteur de remplissage, une compression, un remplissage (padding), des options de verrouillage, un groupe de fichiers ou une description arrête le plan avant toute modification — Schemaic ne sait pas les reconstruire. Les statistiques créées à la main sur la vue ne sont pas reportées.
    }
ddl-risk-add-not-null-column = La colonne { $name } est NOT NULL sans valeur par défaut. Si la table contient déjà des lignes, il n'y a aucune valeur à leur donner, et le moteur refusera tout le plan.
ddl-risk-drop-check = Supprime la contrainte CHECK { $name }. Les lignes que la contrainte refusait sont désormais acceptées, et les données existantes ne sont pas réexaminées si elle est rajoutée.
ddl-risk-keep-lossy-index = Votre modification de l'index { $name } n'est pas incluse. Il utilise quelque chose que Schemaic ne sait pas relire — une colonne clé à expression, une classe d'opérateurs ou un ordre NULLS — et appliquer la modification reviendrait à recréer l'index sans cela. Modifiez plutôt cet index en SQL.
ddl-risk-drop-trigger = Supprime le déclencheur { $name }. Ce qu'il faisait à chaque écriture cesse, et les lignes écrites à partir de maintenant ne le subiront pas.
ddl-risk-redefine-trigger-in-place = Redéfinit le déclencheur { $name } sur place — il n'est pas supprimé, ses autorisations restent donc — et ce qu'il fait à chaque écriture change dès maintenant.
ddl-risk-recreate-trigger = Recréer le déclencheur { $name } commence par le supprimer. Là où le DDL n'est pas transactionnel (MySQL), une nouvelle définition rejetée par le serveur laisse la table sans déclencheur.
ddl-risk-signature-lost = { $kind ->
        [trigger] Le déclencheur
        [function] La fonction
       *[procedure] La procédure
    } { $name } est signé(e) (ADD SIGNATURE), et toute modification retire la signature — Schemaic ne peut pas la signer de nouveau sans la clé du certificat. Ce que le certificat lui permettait de faire cesse de fonctionner jusqu'à ce que vous exécutiez de nouveau ADD SIGNATURE.
ddl-risk-redefines-routine = Redéfinit { $name }. Tout ce qui l'appelle exécute désormais le nouveau corps, y compris des déclencheurs sur d'autres tables que cette modification ne mentionne pas.
ddl-risk-routine-parameters-change = Modifie la liste des paramètres de { $name }. Les appelants qui passent les anciens arguments ne la résolvent plus.
ddl-risk-recreate-routine = Recréer { $held } commence par la supprimer. Là où le DDL n'est pas transactionnel (MySQL), une nouvelle définition rejetée par le serveur ne laisse plus aucune { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Les privilèges accordés sur { $held } et son commentaire ne survivent pas à la suppression, et ce plan ne les rétablit pas — réappliquez ensuite tout GRANT et tout COMMENT.
       *[no] Les privilèges accordés sur { $held } ne survivent pas à la suppression, et ce plan ne les rétablit pas — réappliquez ensuite tout GRANT.
    }
ddl-risk-drop-numbered-group = Supprime la { $what } { $name } et, avec elle, chaque membre de son groupe numéroté : { $members }.
ddl-risk-drop-routine = Supprime la { $what } { $name }. PostgreSQL refuse tant qu'un déclencheur l'utilise encore : ceux qui le font doivent donc être supprimés d'abord.
ddl-risk-drop-event = Supprime l'événement { $name }. Sa planification et son corps disparaissent avec lui — rien d'autre dans la base de données n'en garde de copie.
ddl-risk-one-shot-event = Cet événement s'exécute une fois, puis le serveur le supprime — c'est ON COMPLETION NOT PRESERVE. Activez Conserver pour le garder.
ddl-risk-one-shot-event-altered = L'événement { $name } s'exécutera une fois, puis le serveur le supprimera — c'est ON COMPLETION NOT PRESERVE. Activez Conserver pour le garder.
ddl-risk-add-enum-value = L'ajout de { $value } ne peut pas être défait sur place — PostgreSQL ne peut pas retirer une valeur d'énumération : revenir en arrière implique de recréer le type. La valeur ne peut pas non plus être utilisée avant l'application de ce plan.
ddl-risk-rename-enum-value = Chaque ligne contenant { $from } lit désormais { $to }. Si vous vouliez retirer { $from } plutôt que le renommer, supprimez-la et appliquez cela séparément.
ddl-risk-recreate = Recréer le { $kind ->
        [domain] domaine
       *[type] type
    } { $name } le supprime et le reconstruit. Chaque colonne ci-dessous est reconvertie via du texte ; une valeur que la nouvelle définition n'accepte pas fait échouer tout le plan, et rien n'est appliqué{ $has_columns ->
        [yes] {" "}: { $columns }{ $more ->
            [0] {""}
           *[other] {" "}et { $more } de plus
        }. Une vue ou une fonction construite dessus ne peut pas être listée ici et ferait refuser le serveur.
       *[no] . Rien ne l'utilise aujourd'hui, mais une vue ou une fonction construite dessus ne peut pas être listée ici et ferait refuser le serveur.
    }
ddl-risk-drop-domain-check = Supprime la contrainte CHECK { $name }. Plus aucune colonne de ce domaine n'est vérifiée, et les données existantes ne sont pas réexaminées si elle est rajoutée.
ddl-risk-domain-nullable = Toutes les colonnes de ce domaine se mettent à accepter NULL, y compris dans des tables que cette modification ne mentionne pas.
ddl-risk-domain-not-null = L'instruction échoue si une colonne de ce domaine contient déjà NULL.
ddl-risk-drop-sequence = Supprime la séquence et la position qu'elle avait atteinte. Le serveur refuse tant que la valeur par défaut d'une colonne y puise encore : ces valeurs par défaut doivent donc disparaître d'abord.
ddl-risk-drop-synonym = Supprime le synonyme. Rien ne s'y oppose : une vue, une routine ou une requête qui le nomme échoue dès lors, à sa prochaine exécution.
ddl-risk-drop-extension = Supprime l'extension et les fonctions, types et opérateurs qu'elle a installés. Le serveur refuse tant que quelque chose vous appartenant en dépend encore — une colonne de son type, un index ou un serveur sur son wrapper — et indique quoi.
ddl-risk-drop-used-object = Supprime l'objet ({ $what }). Le serveur refuse tant qu'une colonne l'utilise encore : ces colonnes doivent donc d'abord changer de type.
ddl-risk-restart-sequence = Redémarre le compteur à { $value }. Les valeurs déjà distribuées ne changent pas : une clé qu'il atteint de nouveau entre donc en collision.
ddl-risk-replace-synonym = Supprime le synonyme et le recrée. Rien ne s'oppose à la suppression : les deux instructions s'exécutent dans une seule transaction.
ddl-risk-replace-sequence = Supprime la séquence et la recrée : la position qu'elle avait atteinte est perdue, et elle repart de { $start }. Le serveur refuse la suppression tant que la valeur par défaut d'une colonne y puise encore.
ddl-risk-start-value = sa valeur de début
ddl-risk-replace-object = Supprime l'objet ({ $what }) et le recrée. Le serveur refuse la suppression tant qu'une colonne l'utilise encore : ces colonnes doivent donc d'abord changer de type.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Refusé tant qu'il a des autorisations, un propriétaire propre, des propriétés étendues ou une règle ou une valeur par défaut liée : la recréation ne les rétablirait pas.
       *[no] Refusé tant qu'il a des autorisations, un propriétaire propre ou des propriétés étendues : la recréation ne les rétablirait pas.
    }
ddl-risk-drop-database = Supprime la base de données { $name } et tout ce qu'elle contient — toutes les tables et toutes leurs lignes, chaque vue, index, routine et déclencheur. Rien n'est conservé et rien ne peut être annulé : le seul retour possible est une sauvegarde faite avant cette exécution.
ddl-risk-drop-schema = Supprime le schéma { $name }. Le serveur refuse tant qu'il contient encore des tables, des vues ou des routines : cette opération prend donc un schéma vide ou échoue{ $cascade ->
        [yes] {" "}— Schemaic n'envoie jamais CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Supprime le login { $who }, et avec lui toutes les autorisations serveur et appartenances à des rôles serveur qu'il détient — enregistrées nulle part ailleurs : un login recréé sous ce nom démarre donc sans elles. Le serveur refuse tant que quelque chose est connecté sous ce login : ses sessions doivent donc se terminer d'abord. Ses utilisateurs dans chaque base ne sont pas supprimés : ils restent, avec leurs autorisations, associés à aucun login, et un nouveau login de ce nom ne s'y rattache pas sans ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Supprime { $who } et tous les privilèges qu'il détient. Les droits ne sont enregistrés nulle part ailleurs : rétablir le compte implique de tous les accorder de nouveau de mémoire. { $cuts_off ->
        [yes] Tout ce qui est connecté sous ce compte échoue à sa prochaine instruction dans cette base de données.
       *[no] Tout ce qui est encore connecté sous ce compte continue de s'exécuter jusqu'à sa déconnexion.
    }
ddl-risk-set-password = Change le mot de passe de { $who }. L'ancien ne peut pas être récupéré : tout ce qui est encore configuré avec — une application, une tâche, le client d'un autre développeur — ne pourra plus se connecter tant qu'il n'aura pas été mis à jour à la main. Les sessions déjà ouvertes continuent.
ddl-risk-revoke-privileges = Retire { $privileges } à { $who }. Tout ce qui en dépend — une application, une tâche, une vue appartenant à quelqu'un d'autre — commence à échouer à sa prochaine instruction.{ $cascades ->
        [yes] {" "}La révocation porte CASCADE : le privilège est donc aussi retiré à chaque compte à qui { $who } l'a accordé, et ce qui s'exécute sous ces comptes échoue de la même façon.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Là où { $who } se le voit refuser (DENY), la révocation lève le refus — ce qui rend l'accès si un rôle auquel il appartient l'accorde.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}Sur une table, elle supprime aussi chaque droit et DENY au niveau colonne de { $cleared } que { $who } a sur les colonnes de la table : une colonne refusée à { $who } ne l'est plus, et une colonne accordée séparément ne l'est plus.
       *[no] {""}
    }
ddl-risk-revoke-role = Retire le rôle { $role } à { $member }, et avec lui tous les privilèges que porte le rôle.
ddl-risk-blank-password = { $who } n'aura pas de mot de passe : quiconque peut joindre le serveur peut se connecter sous ce compte. Définissez-en un ici, ou ensuite avec ALTER USER.
ddl-risk-host-is-everywhere = Son hôte est % — toutes les machines du réseau, pas seulement celle-ci.
ddl-risk-grant-server-wide = Accorde { $privileges } à { $who } sur tout le serveur — l'instance elle-même, pas la base de données parcourue.
ddl-risk-grant-every-database = Accorde { $privileges } à { $who } sur toutes les bases de données de ce serveur — y compris celles qui n'existent pas encore, pas seulement celle parcourue.
ddl-risk-grant-keeps-column-denies = Un droit de niveau table sur { $privileges } supprime tout DENY correspondant que { $who } a sur l'une des colonnes de la table : l'instruction refuse donc de nouveau ces colonnes à { $who } juste après l'octroi — une colonne refusée à { $who } le reste. Si { $who } détient aussi l'option d'attribution sur la table, le serveur refuse de la refuser de nouveau sans CASCADE, et rien n'est appliqué.
ddl-risk-star-refresh = Actualise chaque vue et chaque fonction inline de cette base de données qui fait select * from { $table }, directement ou via un synonyme : SQL Server les lie aux colonnes de la table par position, et elles liraient et écriraient sinon les mauvaises colonnes. Le plan échoue si l'une d'elles ne compile plus avec les nouvelles colonnes. Une vue d'une autre base de données qui fait select * from { $table } est actualisée après la validation du plan, hors de sa transaction : d'ici là, elle lit les anciennes positions, et si son actualisation échoue, le plan reste appliqué et la vue est nommée ici.
ddl-risk-piecewise = SQL Server ne crée, ne modifie ni ne supprime un module compilé en mode natif à l'intérieur d'une transaction : ce plan s'exécute donc une instruction à la fois et ne peut pas être arrêté — si une instruction échoue, celles qui la précèdent restent appliquées.
ddl-risk-drop-sequence-table = Supprime la séquence : la valeur qu'elle a atteinte est perdue, et tout ce qui y puise encore échoue.
ddl-risk-drop-scale = D'après les statistiques de l'arborescence du schéma, cela représente { $rows } lignes.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = la proposition concerne la table `{ $proposed }`, mais c'est `{ $actual }` qui est ouverte
propose-no-ops = la proposition ne demande aucune modification
propose-no-such-column = `{ $table }` n'a pas de colonne nommée `{ $name }`
propose-column-exists = `{ $table }` a déjà une colonne nommée `{ $name }`
propose-no-such-index = `{ $table }` n'a pas d'index nommé `{ $name }`
propose-index-exists = `{ $table }` a déjà un index nommé `{ $name }`
propose-no-such-foreign-key = `{ $table }` n'a pas de clé étrangère nommée `{ $name }`
propose-no-such-check = `{ $table }` n'a pas de contrainte CHECK nommée `{ $name }`
propose-key-width-mismatch = une clé étrangère a besoin d'autant de colonnes référencées que de colonnes référençantes
propose-empty-index = un index a besoin d'au moins une colonne
propose-invalid = la table proposée ne serait pas valide : { $problem }
propose-not-a-table = `{ $name }` est une vue — une proposition ne peut modifier qu'une table
propose-no-such-table = aucune table nommée `{ $name }` — appelez list_schema pour voir ce qui existe
propose-not-an-expression = { $field ->
        [default] la valeur par défaut `{ $text }` n'est pas une expression SQL unique
        [generated] l'expression générée `{ $text }` n'est pas une expression SQL unique
       *[check] l'expression CHECK `{ $text }` n'est pas une expression SQL unique
    }
propose-not-a-type = `{ $text }` n'est pas un type de colonne — indiquez le type seul, sans valeur par défaut, sans NOT NULL et sans rien après
propose-unknown-action = `{ $text }` n'est pas une action de clé étrangère
propose-not-loaded = { $table } n'est pas chargée dans { $database } pour l'instant — ouvrez la base de données dans l'arborescence du schéma, ou attendez la fin d'une actualisation, puis réessayez.
propose-nothing-to-change = { $subject } a déjà cette forme — il n'y a rien à modifier.
propose-no-database = Cet onglet n'a aucune base de données sélectionnée : une modification de schéma appliquée d'ici n'aurait donc nulle part où aller. Choisissez-en d'abord une pour cette connexion — si toutes les bases sont masquées, c'est l'œil de SCHÉMA qui permet d'en faire réapparaître une.
propose-other-connection = Cette modification concerne un onglet d'une autre connexion — passez-y, ou sélectionnez sa connexion dans l'arborescence, avant d'appliquer une modification de schéma.
ai-run-no-database = Cet onglet n'a aucune base de données sélectionnée : une instruction exécutée d'ici n'aurait donc nulle part où aller. Choisissez-en d'abord une pour cette connexion — si toutes les bases sont masquées, c'est l'œil de SCHÉMA qui permet d'en faire réapparaître une.
ai-run-other-connection = Cette conversation concerne un onglet d'une autre connexion. Un nouvel onglet s'ouvrirait sur la connexion sélectionnée dans l'arborescence, et l'instruction s'exécuterait donc ailleurs — passez d'abord à cet onglet, ou sélectionnez sa connexion dans l'arborescence.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, que ce plan renomme en { $to }
ddl-moved-dropped = { $name }, que ce plan supprime
ddl-withheld-unreplayable-rename = { $count ->
        [one] Renommer { $was } en { $to } ne peut pas emporter son { $count } déclencheur : son SQL nomme { $was }, et le réécrire ici modifierait ce que vous avez écrit. Renommez la vue séparément après avoir recréé les déclencheurs avec le nouveau nom.
       *[other] Renommer { $was } en { $to } ne peut pas emporter ses { $count } déclencheurs : leur SQL nomme { $was }, et le réécrire ici modifierait ce que vous avez écrit. Renommez la vue séparément après avoir recréé les déclencheurs avec le nouveau nom.
    }
ddl-withheld-numbered-group = { $name } est en tête d'un groupe de procédures numérotées ({ $members }), et ce plan commence par la supprimer, ce qui supprime avec elle chaque membre du groupe. Modifiez-la sous son propre nom et son propre type, ce qui la modifie sur place et les conserve.
ddl-withheld-view-index-create = { $count ->
        [one] La création de la vue { $view } ne peut pas créer son index { $indexes } — d'un type que Schemaic ne sait pas réécrire. Créez plutôt cette vue en SQL.
       *[other] La création de la vue { $view } ne peut pas créer ses index { $indexes } — d'un type que Schemaic ne sait pas réécrire. Créez plutôt cette vue en SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] La modification de la vue { $view } supprime son index { $indexes }, que Schemaic ne sait pas recréer — d'un type que Schemaic ne sait pas réécrire. Modifiez plutôt cette vue en SQL.
       *[other] La modification de la vue { $view } supprime ses index { $indexes }, que Schemaic ne sait pas recréer — d'un type que Schemaic ne sait pas réécrire. Modifiez plutôt cette vue en SQL.
    }
ddl-withheld-tsql-lossy-index = Reconstruire { $table } supprimerait l'index { $index } : il est d'un type que Schemaic ne lit pas en entier
ddl-withheld-tsql-encrypted-trigger = Reconstruire { $table } supprimerait le déclencheur { $trigger } : son texte est chiffré
ddl-withheld-tsql-signed-trigger = Reconstruire { $table } supprimerait la signature du déclencheur { $trigger }, qui ne peut pas être rétablie sans la clé privée du certificat
ddl-withheld-tsql-renamed-trigger = Reconstruire { $table } sous un nouveau nom laisserait le déclencheur { $trigger } nommer l'ancien
ddl-withheld-partly-read = L'index { $index } n'a été lu qu'en partie — SQLite ne garde le prédicat d'un index partiel, et une clé à expression, que dans le texte CREATE de l'index lui-même{ $why ->
        [may-name] , et ce texte peut nommer { $moved }. Supprimez l'index pour continuer sans lui.
        [not-found] {" "}: il ne peut donc pas être créé à partir de ce qui a été lu. Supprimez l'index pour continuer sans lui.
        [edited] {" "}: cette modification ne peut donc pas être écrite — ce qui a été lu n'est pas l'index entier. Annulez la modification pour garder l'index tel quel, ou supprimez-le.
       *[no-text] {" "}— et ce texte n'existe pas pour celui-ci : le recréer changerait donc ce qu'il indexe. Supprimez l'index pour continuer sans lui.
    }
ddl-withheld-condition-names-dropped = La condition de l'index { $index } nomme { $column }, que ce plan supprime. Modifiez la condition ou supprimez d'abord l'index.
ddl-withheld-strands-triggers = Cette table porte { $count ->
        [one] { $count } déclencheur
       *[other] { $count } déclencheurs
    }, dont le SQL est rejoué exactement tel que SQLite l'a stocké — et nommerait encore { $moved }. SQLite accepte un tel déclencheur, puis refuse toute écriture dans la table. Supprimez ou modifiez d'abord le déclencheur, ou renommez la colonne sans autre modification pour que l'ALTER TABLE de SQLite puisse le réécrire.
ddl-withheld-strands-other-trigger = { $owner } nomme { $moved }. Une reconstruction renomme cette table sous ce déclencheur et laisse son SQL exactement tel quel : le déclencheur continuerait donc de nommer une colonne disparue — SQLite accepte un tel déclencheur, puis refuse toute écriture dans *cette* table. Supprimez ou modifiez d'abord le déclencheur, ou renommez la colonne sans autre modification pour que l'ALTER TABLE de SQLite puisse le réécrire.
ddl-withheld-trigger-on = Le déclencheur { $name } sur { $table }
ddl-withheld-some-trigger = Un déclencheur sur une autre table
ddl-withheld-strands-generated-column = La colonne générée { $column } est calculée à partir de { $moved }. Une reconstruction écrit la table à partir du modèle, et rien ne réécrit cette expression — SQLite refuse la nouvelle table avec « no such column » et tout le plan est annulé. Modifiez ou retirez d'abord { $column }.
ddl-withheld-cannot-restate = La déclaration de cette table porte { $clauses }, pour lesquels le modèle de Schemaic n'a pas de champ. Une reconstruction réécrit la table à partir de ce modèle : la clause disparaîtrait donc d'une table d'apparence correcte — et toute écriture qui en dépend se comporterait différemment dès lors. Modifiez plutôt cette table avec un script (Copier le DDL en est le point de départ), ou retirez d'abord la clause.
ddl-sqlite-clauses = { $first } et { $last }
ddl-sqlite-clause-deferrable = la clause DEFERRABLE d'une clé étrangère
ddl-sqlite-clause-on-conflict = une clause ON CONFLICT
ddl-sqlite-clause-desc-key = une colonne de clé primaire DESC
ddl-sqlite-clause-collate-key = un COLLATE sur une colonne de clé primaire
ddl-withheld-virtual-table = `{ $name }` est une table virtuelle. Ses colonnes sont l'interface d'un module plutôt qu'une déclaration de stockage, et ses vraies données vivent dans des tables fantômes qu'une reconstruction supprimerait — un index FTS5 disparaîtrait d'une table qui contiendrait encore le texte. Modifiez-la plutôt avec un script (DROP et CREATE VIRTUAL TABLE), ou modifiez les tables qu'elle indexe.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } vue
   *[other] { $count } vues
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } routine
   *[other] { $count } routines
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } déclencheur
   *[other] { $count } déclencheurs
}
ddl-used-by-count-event = { $count ->
    [one] { $count } événement
   *[other] { $count } événements
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } clé étrangère
   *[other] { $count } clés étrangères
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } expression de table
   *[other] { $count } expressions de table
}
ddl-used-by-counts = { $first } et { $last }
ddl-used-by-listing = { $counts } : { $names }{ $more ->
        [0] {""}
       *[other] {" "}et { $more } de plus
    }
ddl-used-by-drop = Utilisé par { $listing }.
ddl-used-by-change = { $subject } — utilisé par { $listing }. { $then ->
        [renamed] Ces dépendants nomment encore { $name } après le renommage.
        [dropped] Ces dépendants cessent de fonctionner à sa suppression.
       *[retyped] Vérifiez-les au regard de son nouveau type.
    }
ddl-used-by-column = La colonne { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Les clés étrangères entre ces tables forment un cycle : aucun ordre de création ne les satisfait toutes. Une instruction sera refusée pour avoir référencé une table qui n'existe pas encore, et ni MySQL ni MariaDB n'annulent le DDL.
ddl-risk-clash = { $holder } détient encore { $names }, dont la création de { $claimer } a besoin — { $why ->
        [not-dropped] et ce plan ne le supprime pas
        [default] et il ne peut pas être supprimé d'abord, car la valeur par défaut d'une colonne y puise
       *[referenced] et il ne peut pas être supprimé d'abord, car une autre table le référence
    }. { $cost ->
        [no-rollback] Une instruction sera refusée, et ni MySQL ni MariaDB n'annulent le DDL.
        [rolls-back] Une instruction sera refusée, ce qui annule toute la migration.
       *[refused] Une instruction sera refusée.
    }

preview-title = Appliquer les modifications à { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } modification
   *[other] { $count } modifications
}
preview-used-by = Utilisé par
preview-withheld = Ce moteur ne peut pas exprimer une partie de ce plan
preview-withheld-hint = Rien n'est appliqué tant que ceci est listé.
preview-omitted = Non inclus dans cette migration
preview-omitted-hint = L'application laisse ces objets tels quels.
preview-copy = Copier
preview-open-in-editor = Ouvrir dans l'éditeur
preview-back = Retour
preview-apply = Appliquer
preview-applying = Application…
preview-applied = { $count ->
    [one] { $count } instruction appliquée à { $subject }.
   *[other] { $count } instructions appliquées à { $subject }.
}
preview-refreshed = Le schéma a été actualisé.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Importer des connexions
conn-import-url = URL de connexion
conn-import-add = Ajouter
conn-import-url-hint = Une URL de connexion, une URL JDBC ou une ligne DATABASE_URL tirée d'un .env — une par ligne.
conn-import-choose-file = Choisir un fichier…
conn-import-scan = Analyser les clients installés
conn-import-scanning = Analyse…
conn-import-docker = Conteneurs Docker
conn-import-asking-docker = Interrogation de Docker…
conn-import-looking = Recherche de connexions sur cette machine…
conn-import-none-found = Aucune connexion trouvée. DBeaver, les IDE JetBrains et les clients en ligne de commande gardent leurs fichiers à des emplacements connus ; si le vôtre est ailleurs, choisissez-le ci-dessus.
conn-import-nothing-yet = Rien de trouvé pour l'instant — collez une URL, lisez un fichier, ou recherchez les clients que vous utilisez déjà.
conn-import-found = { $count ->
    [one] { $count } connexion trouvée
   *[other] { $count } connexions trouvées
}
conn-import-select-all = Tout sélectionner
conn-import-select-none = Aucune
conn-import-note-no-password = Aucun mot de passe dans la source
conn-import-note-already-saved = Déjà enregistrée
conn-import-note-unexpanded-path = Le chemin contient une macro non développée
conn-import-note-pgpass = Mot de passe tiré de votre propre .pgpass
conn-import-note-port-assumed = La source n'indiquait aucun port ; voici celui par défaut
conn-import-note-external-login = S'authentifie d'une manière que Schemaic ne gère pas : définissez un login SQL
conn-import-skipped = { $count ->
    [one] { $count } entrée n'a pas été importée : { $names }.
   *[other] { $count } entrées n'ont pas été importées : { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } entrée n'a pas été importée : { $names }, et { $rest } de plus.
   *[other] { $count } entrées n'ont pas été importées : { $names }, et { $rest } de plus.
}
conn-import-reason-unsupported-engine = moteur non pris en charge ({ $driver })
conn-import-reason-no-server = aucun serveur auquel se connecter
conn-import-reason-not-published = son port n'est pas publié sur cette machine
conn-import-reason-unreadable = illisible ({ $error })
conn-import-reason-not-an-export = illisible (pas un export de connexions Schemaic : { $error })
conn-import-reason-newer-export = illisible (exporté par une version plus récente de Schemaic (version { $version }) — mettez à jour pour l'importer)
conn-import-url-empty = Collez une URL de connexion.
conn-import-url-no-scheme = Aucun schéma d'URL — une URL commence par mysql://, postgresql://, sqlserver:// ou sqlite:, une chaîne de connexion SQL Server par Server=.
conn-import-url-unknown-scheme = Schéma d'URL inconnu « { $scheme } » — attendu : mysql, mariadb, postgresql, sqlserver ou sqlite.
conn-import-url-no-host = Aucun hôte dans l'URL.
conn-import-url-bad-port = « { $port } » n'est pas un numéro de port.
conn-import-url-no-path = Aucun fichier de base de données après sqlite:.
conn-import-url-transport = « { $server } » se joint par canaux nommés ou LocalDB — Schemaic ne se connecte qu'en TCP.
conn-import-url-userinfo = Le nom du serveur contient un @ — un nom d'utilisateur ou un mot de passe va dans son propre champ, pas dans le serveur.
conn-import-url-not-sql-server = Cette chaîne de connexion n'indique pas qu'elle est destinée à SQL Server — les pilotes .NET de MySQL et de PostgreSQL utilisent les mêmes mots-clés. Collez plutôt l'URL du serveur (mysql://, postgresql://, sqlserver://) ou, pour SQL Server, écrivez le serveur sous la forme tcp:host.
conn-import-row-overflow = Affichage des { $shown } premières sur { $total }. Les { $hidden } autres sont tout de même importées si elles sont cochées — cette liste s'arrête ici pour que la fenêtre reste réactive.
conn-import-import = Importer
conn-import-import-count = Importer { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = Image PNG
blob-kind-jpeg = Image JPEG
blob-kind-gif = Image GIF
blob-kind-bmp = Image BMP
blob-kind-webp = Image WebP
blob-kind-ico = Icône
blob-kind-opaque = Données binaires
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · affichage du début ({ $shown })
blob-pane-preview = Aperçu
blob-pane-hex = Hexa
blob-reading = Lecture…
blob-empty = Cette cellule est NULL, ou sa ligne n'existe plus.
blob-preview-too-large = Cette image fait { $width } × { $height } — trop grande pour un aperçu ici. Les octets sont intacts : lisez-les en hexadécimal, ou enregistrez-les dans un fichier.
blob-preview-truncated = Seul le début { $key ->
    [png] de cette image PNG
    [jpeg] de cette image JPEG
    [gif] de cette image GIF
    [bmp] de cette image BMP
    [webp] de cette image WebP
    [ico] de cette icône
   *[other] de cette valeur
} a été lu ({ $shown }) : elle ne peut donc pas être dessinée. Lisez les octets en hexadécimal.
blob-preview-unreadable = Ces octets commencent comme { $key ->
    [png] une image PNG
    [jpeg] une image JPEG
    [gif] une image GIF
    [bmp] une image BMP
    [webp] une image WebP
    [ico] une icône
   *[other] une image
} mais ne peuvent pas être lus comme telle. Lisez-les en hexadécimal, ou enregistrez-les dans un fichier.
blob-save-truncated = Seul le début ({ $shown } sur { $size }) a pu être lu : l'enregistrement ne sauverait donc que le début de la valeur.
blob-load = Charger depuis un fichier
blob-save = Enregistrer dans un fichier
blob-load-dialog = Charger une valeur binaire depuis un fichier
blob-save-dialog = Enregistrer la valeur binaire
blob-saved-to = Enregistré dans { $path }
blob-loaded-file = Fichier chargé. Validez dans la grille pour l'écrire.
blob-not-writable = Cette cellule ne peut pas être écrite.
blob-file-too-large = Ce fichier fait { $got } — cette colonne contient au plus { $most }.
blob-not-accepting = Cette cellule n'accepte plus de valeur — rien n'a été mis en attente.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = Diagramme ER
erd-scope = Portée
erd-chip-tables = { $count ->
    [one] { $count } table
   *[other] { $count } tables
}
erd-chip-relationships = { $count ->
    [one] { $count } relation
   *[other] { $count } relations
}
erd-chip-hidden = { $count ->
    [one] { $count } non liée masquée
   *[other] { $count } non liées masquées
}
erd-find-placeholder = Rechercher une table ou une colonne
erd-find-none = Aucun résultat
erd-find-matches = { $hits ->
    [one] { $hits } résultat
   *[other] { $hits } résultats
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } résultat dans { $tables } tables
   *[other] { $hits } résultats dans { $tables } tables
}
erd-more-columns = +{ $count } de plus
erd-show-less = afficher moins
erd-not-loaded = Le schéma de cette base de données n'est pas encore chargé.
erd-no-relationships = Aucune relation de clé étrangère à représenter.
erd-export-tip = Exporter le diagramme…
erd-copy-as = Copier en tant que
erd-export-dialog = Exporter le diagramme
erd-nothing-to-export = Rien à exporter.
erd-nothing-to-copy = Rien à copier.
erd-copied-as = Copié en tant que { $format }
erd-clipboard-unavailable = Le presse-papiers n'est pas disponible.
erd-copy-failed = Impossible de copier : { $error }
erd-saved = { $name } enregistré
erd-unnamed-file = le fichier

## Server variables modal (server_vars_view.rs)

server-vars-title = Variables du serveur — { $connection }
server-vars-search = Rechercher des paramètres
server-vars-variables = Variables
server-vars-status = État
server-vars-loading = Chargement…
server-vars-unsupported = Le moteur de cette connexion ne publie aucune liste de ce type.
server-vars-no-match = Aucun paramètre correspondant.
server-vars-more = { $count ->
    [one] { $count } paramètre de plus correspond — affinez la recherche
   *[other] { $count } paramètres de plus correspondent — affinez la recherche
}
server-vars-count = { $count ->
    [one] { $count } paramètre
   *[other] { $count } paramètres
}
server-vars-count-filtered = { $shown } paramètres sur { $count }
server-vars-refresh = Actualiser

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Modifier l'extrait — { $name }
snippet-edit-name = Nom
snippet-edit-name-placeholder = Nom de l'extrait
snippet-edit-shortcut = Raccourci d'expansion
snippet-edit-shortcut-none = aucun
snippet-edit-query = Requête
snippet-edit-save = Enregistrer

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Plan de la requête
plan-analyze-unavailable = Analyze indisponible (l'instruction écrit des données)
plan-analyze-hint = Exécute l'instruction pour mesurer les durées réelles
plan-explaining = Analyse du plan
plan-potential-issues = Problèmes potentiels
plan-detail-no-index = aucun index utilisé
plan-detail-key-unused = index non utilisé pour la recherche
plan-my-full-scan = Parcours complet de la table `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Parcours complet du résultat ({ $detail }, type = ALL)
plan-my-filesort = Filesort sur `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort sur le résultat (Extra: Using filesort)
plan-my-temporary = Table temporaire pour `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Table temporaire pour le résultat (Extra: Using temporary)
plan-pg-seq-scan = Parcours séquentiel de `{ $table }` (aucun index utilisé)
plan-pg-seq-scan-unnamed = Parcours séquentiel d'une table (aucun index utilisé)
plan-pg-sort = Nœud Sort — la requête trie les lignes à la volée
plan-ms-full-scan = Parcours complet de `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Parcours complet d'une table ({ $op })
plan-ms-sort = Sort — la requête trie les lignes à la volée
plan-ms-spool = { $op } — une table de travail intermédiaire dans tempdb
plan-ms-operator = { $op } : { $warning }
plan-server-no-join-predicate = aucun prédicat de jointure — chaque ligne est associée à chaque ligne
plan-server-no-statistics = une colonne qu'il lit n'a pas de statistiques
plan-server-spill = débordement dans tempdb — son allocation de mémoire était trop faible
plan-server-convert = une conversion de type affecte le plan ({ $issue }) : { $expression }
plan-server-unmatched-indexes = un index filtré n'a pas pu être utilisé
plan-server-memory-grant = son allocation de mémoire a été mal évaluée
plan-server-wait = a attendu { $wait }
plan-server-other = le serveur avertit : { $name }
plan-missing-index = Index manquant sur `{ $table }` ({ $keys })
plan-missing-index-include = Index manquant sur `{ $table }` ({ $keys } ; include { $include })
plan-missing-index-impact = Index manquant sur `{ $table }` ({ $keys }) — le serveur prévoyait une amélioration de { $impact } %
plan-missing-index-include-impact = Index manquant sur `{ $table }` ({ $keys } ; include { $include }) — le serveur prévoyait une amélioration de { $impact } %
plan-ask-ai = Demander à l'IA
plan-ms-operation = Opération
plan-ms-object = Objet
plan-ms-estimated-rows = Lignes estimées
plan-ms-estimated-cost = Coût estimé
plan-ms-actual-rows = Lignes réelles
plan-ms-executions = Exécutions
plan-ms-statement = Instruction { $n } : { $kind }
plan-ms-statement-in-procedure = Instruction { $n } : { $kind } dans la procédure { $name }
plan-ms-statement-in-function = Instruction { $n } : { $kind } dans la fonction { $name }
plan-ms-statement-in-a-procedure = Instruction { $n } : { $kind } dans une procédure
plan-ms-statement-in-a-function = Instruction { $n } : { $kind } dans une fonction
plan-ms-plan = Plan
plan-ms-no-plan = Aucune instruction de ce lot n'a de plan.
plan-ms-unread = Impossible de lire le plan : { $reason }
plan-ms-unread-not-xml = pas un document XML
plan-ms-unread-not-showplan = ShowPlanXML attendu, { $found } trouvé
plan-ms-unread-empty = un document vide

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Utilisé par — { $object }
used-by-copy = Copier
used-by-not-loaded = Le schéma de { $database } n'est pas encore chargé — développez-le dans l'arborescence, puis relancez la demande.
used-by-nothing-in = Rien dans { $database } ne l'utilise.
used-by-nothing = Rien ne l'utilise.
used-by-limits = Trouvé en lisant les définitions de cette base de données : le SQL construit à l'exécution et les autres bases de données ne sont pas vus.
used-by-dropped-with-it = { $detail } · supprimé avec lui
used-by-heading-view = { $count ->
    [one] Vue ({ $count })
   *[other] Vues ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Routine ({ $count })
   *[other] Routines ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Déclencheur ({ $count })
   *[other] Déclencheurs ({ $count })
}
used-by-heading-event = { $count ->
    [one] Événement ({ $count })
   *[other] Événements ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Clé étrangère ({ $count })
   *[other] Clés étrangères ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Expression de table ({ $count })
   *[other] Expressions de table ({ $count })
}
used-by-noun-view = vue
used-by-noun-routine = routine
used-by-noun-trigger = déclencheur
used-by-noun-event = événement
used-by-noun-foreign-key = clé étrangère
used-by-noun-expression = expression de table

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Sombre
theme-light = Clair

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = CODE
md-code-copy = Copier
md-code-insert = Insérer
md-code-run = Exécuter
md-proposal-title = Modification proposée pour { $table }
md-proposal-count = { $count ->
    [one] { $count } modification
   *[other] { $count } modifications
}
md-proposal-review = Examiner la modification…
md-proposal-unreadable = L'assistant a proposé une modification illisible — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Choisir…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Maintenant
cell-calendar-today = Aujourd'hui
cell-month-1 = janvier
cell-month-2 = février
cell-month-3 = mars
cell-month-4 = avril
cell-month-5 = mai
cell-month-6 = juin
cell-month-7 = juillet
cell-month-8 = août
cell-month-9 = septembre
cell-month-10 = octobre
cell-month-11 = novembre
cell-month-12 = décembre
cell-weekday-mon = Lu
cell-weekday-tue = Ma
cell-weekday-wed = Me
cell-weekday-thu = Je
cell-weekday-fri = Ve
cell-weekday-sat = Sa
cell-weekday-sun = Di

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Renommer l'alias
alias-rename-title-cte = Renommer la CTE
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = Nom de la CTE
alias-rename-new-name = Nouveau nom
alias-rename-rename = Renommer

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Il n'y a plus rien à renommer ici.
alias-rename-empty = Donnez-lui un nom.
alias-rename-conflict = « { $name } » est déjà un nom de cette requête : renommer avec ce nom changerait la table que lit une référence.
alias-rename-unsure = Une référence ici peut lire une autre colonne de ce nom, ou lire celle-ci là où le renommage ne peut pas suivre, et le schéma chargé ne permet pas de le savoir — aucune modification n'est donc faite. Chargez le schéma des tables, ou renommez à la main.
alias-rename-changed = La requête a changé pendant que ceci était ouvert. Fermez et renommez de nouveau.
alias-rename-read-only = L'éditeur est en lecture seule jusqu'à ce que la suggestion de l'IA soit acceptée ou rejetée.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } utilisation de { $name }
   *[other] { $count } utilisations de { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = SVG non valide : { $error }
raster-too-large = L'image est trop grande pour être rastérisée ({ $width }×{ $height })
raster-png-failed = Échec de l'encodage PNG : { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Copier
grid-menu-copy-value = Copier la valeur
grid-menu-copy-formatted = Copier avec la mise en forme
grid-menu-copy-as = Copier en tant que
grid-menu-paste = Coller
grid-menu-fill-down = Recopier vers le bas
grid-menu-fill-up = Recopier vers le haut
grid-menu-edit-field = Modifier le champ
grid-menu-edit-row = Modifier la ligne
grid-menu-view-row = Afficher la ligne
grid-menu-edit-binary = Modifier le binaire
grid-menu-view-binary = Afficher le binaire
grid-menu-filter-by-value = Filtrer par cette valeur
grid-menu-exclude-value = Exclure cette valeur
grid-menu-follow-relation = Suivre la relation
grid-menu-referenced-by = Référencé par
grid-menu-set-null = Mettre à NULL
grid-menu-set-default = Mettre à DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Dupliquer la ligne
   *[other] Dupliquer { $count } lignes
}
grid-menu-delete-rows = { $count ->
    [one] Supprimer la ligne
   *[other] Supprimer { $count } lignes
}
grid-menu-undo-delete = Annuler la suppression
grid-menu-ai-summary = Résumé par l'IA
grid-attach-to-chat = Joindre { $scope } à la conversation
grid-attach-scope-rows = { $rows ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } colonne
   *[other] { $cols } colonnes
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
} et { $cols ->
    [one] { $cols } colonne
   *[other] { $cols } colonnes
}
grid-menu-freeze = Figer
grid-menu-unfreeze = Libérer
grid-menu-hide-column = Masquer la colonne
grid-menu-show-hidden-columns = Afficher les colonnes masquées ({ $count })
grid-menu-show-all = Tout afficher
grid-menu-format-as = Formater comme
grid-format-none = Aucun
grid-format-timestamp = Horodatage
grid-format-grouped = Séparateur de milliers
grid-format-bytes = Taille de fichier
grid-format-bool = Booléen
grid-menu-copy-name = Copier le nom
grid-menu-copy-qualified-name = Copier le nom qualifié
grid-menu-copy-values = Copier les valeurs
grid-menu-sql-in-list = Liste SQL IN

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
    [one] { $read } ligne
   *[other] { $read } lignes
}
grid-rows-read-capped = { $n ->
    [one] { $read } ligne
   *[other] { $read } lignes
} (limité)
grid-rows-read-of-total = { $n ->
    [one] { $read } sur { $total } ligne
   *[other] { $read } sur { $total } lignes
}
grid-kept-note = · conservé — lecture seule
grid-arena-note = · { $columns } trop volumineux pour être conservés en entier — les lignes suivantes s'affichent vides
grid-sorted-subset = · sous-ensemble trié (limité) — pas l'ordre complet
grid-read-all = lire toutes les lignes
grid-read-more = lire { $rows } lignes
grid-reading = lecture…
grid-committing = Validation…
grid-commit-tip = { $count ->
    [one] Valider { $count } modification ({ $keys })
   *[other] Valider { $count } modifications ({ $keys })
}
grid-discard-tip = Abandonner toutes les modifications en attente
grid-copy-sql-tip = Copier les modifications en SQL
grid-add-row-tip = Ajouter une ligne
grid-delete-row-tip = Marquer la ligne sélectionnée pour suppression ({ $key })
grid-clone-row-tip = Cloner la ligne sélectionnée
grid-copy-results-tip = Copier les résultats…
grid-copy-results-empty = Rien à copier — aucune ligne
grid-save-results-tip = Enregistrer les résultats dans un fichier…
grid-save-results-empty = Rien à enregistrer — aucune ligne
grid-ai-tip = Générer des lignes avec l'IA…
grid-ai-generating = Génération…
grid-ai-off = Les actions d'IA envoient des données — désactivées pour cette connexion
grid-ai-fill-value = Remplir la valeur par l'IA
grid-ai-insert-row = Insérer une ligne par l'IA
grid-ai-seed-table = Peupler la table par l'IA
grid-chart-tip = Représenter les résultats en graphique
grid-chart-empty = Rien à représenter — aucune ligne
grid-chart-this-result = ce résultat

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Afficher
grid-rollback-tab = Annuler la transaction de { $tab }
grid-find-placeholder = Rechercher dans les résultats
grid-figure-copied = Copié
grid-figure-rows = { $n ->
    [one] { $n }{ $more } ligne
   *[other] { $n }{ $more } lignes
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } distinctes
grid-figure-sum = Somme { $value }
grid-figure-avg = Moy. { $value }
grid-figure-median = Médiane { $value }
grid-figure-min = Min { $value }
grid-figure-max = Max { $value }
grid-figure-length = { $n ->
    [one] Longueur { $n } car.
   *[other] Longueur { $n } car.
}
grid-figure-length-range = Longueur { $lo }–{ $hi } car.
grid-goto-label = Aller à la ligne :

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = le presse-papiers
export-dest-file = le fichier
export-note-rows = { $n ->
    [one] { $rows } ligne exportée vers { $name }
   *[other] { $rows } lignes exportées vers { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } contient des octets que ce format ne peut pas transporter : ces cellules sont vides
   *[other] { $cols } contiennent des octets que ce format ne peut pas transporter : ces cellules sont vides
}
export-note-blanked = { $cols } trop volumineux pour être conservés en entier : les lignes suivantes sont vides
export-note-cut = { $cols } tronqué à la limite de { $limit } caractères par cellule d'Excel
export-failure-partial = { $message } — { $name } n'a pas été modifié ; les lignes écrites se trouvent dans { $part }
export-cancelled-partial = Export annulé — { $name } n'a pas été modifié ; les lignes écrites se trouvent dans { $part }
export-cancelled-nothing = Export annulé — { $name } n'a pas été modifié.
copy-sql-as-null = { $n ->
    [one] Copié dans le presse-papiers — { $cols } contient des octets que le texte SQL ne peut pas transporter : écrits comme NULL
   *[other] Copié dans le presse-papiers — { $cols } contiennent des octets que le texte SQL ne peut pas transporter : écrits comme NULL
}
copy-sql-nothing = { $n ->
    [one] Rien n'a été copié — { $cols } contient des octets que le texte SQL ne peut pas transporter
   *[other] Rien n'a été copié — { $cols } contiennent des octets que le texte SQL ne peut pas transporter
}
copy-sql-left-out = { $n ->
    [one] Copié dans le presse-papiers — { $cols } contient des octets que le texte SQL ne peut pas transporter : ces valeurs sont omises
   *[other] Copié dans le presse-papiers — { $cols } contiennent des octets que le texte SQL ne peut pas transporter : ces valeurs sont omises
}

grid-fill-nothing = Rien à remplir : les colonnes sélectionnées ne peuvent pas être écrites, ou la cellule à copier est une valeur non définie d'une nouvelle ligne ou un DEFAULT en attente.
grid-paste-outside = { $n } hors de la grille
grid-paste-read-only = { $n } dans des colonnes en lecture seule
grid-paste-capped = { $n } au-delà de la limite de collage de { $cap } cellules
grid-paste-deleted = { $n } dans des lignes marquées pour suppression
grid-paste-nothing = Rien n'a été collé : { $notes }.
grid-paste-partial = { $count ->
        [one] { $cells } cellule collée, en ignorant { $notes }.
       *[other] { $cells } cellules collées, en ignorant { $notes }.
    }
grid-row-not-editable = La colonne `{ $column }` n'est pas modifiable.
grid-row-not-nullable = La colonne `{ $column }` ne peut pas être NULL.
grid-fill-skipped = { $count ->
    [one] { $filled } cellule remplie, { $skipped } ignorées dans des lignes marquées pour suppression.
   *[other] { $filled } cellules remplies, { $skipped } ignorées dans des lignes marquées pour suppression.
}
grid-attach-summary = { $rows ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
} × { $cols ->
    [one] { $cols } colonne
   *[other] { $cols } colonnes
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
} × { $cols ->
    [one] { $cols } colonne
   *[other] { $cols } colonnes
} de { $table }
grid-attach-summary-capped = { $summary } (les { $taken } premières sur { $total } sélectionnées)
grid-copy-sql-unset = Rien à copier en SQL — les cellules sélectionnées de la nouvelle ligne n'ont pas encore de valeur et prennent la valeur par défaut du serveur.
grid-copy-sql-joined = Rien à copier en SQL : ces lignes proviennent d'une requête portant sur plusieurs tables.
grid-copy-sql-no-keyed-table = Rien à copier en SQL : les lignes doivent provenir d'une seule table dotée d'une clé.
grid-copy-sql-key-too-large = Rien à copier en SQL : la clé était trop volumineuse pour que ce résultat la conserve en entier, certaines lignes ne peuvent donc pas être désignées par elle.
grid-copy-sql-pinned = Rien à copier en SQL : ce résultat est épinglé, ses lignes sont donc un instantané.
grid-copy-sql-read-only = Rien à copier en SQL : cette connexion est en lecture seule.
grid-copy-update-too-large = Rien à copier en UPDATE : les colonnes sélectionnées étaient trop volumineuses pour que ce résultat les conserve en entier, et certaines de leurs cellules s'affichent vides.
grid-copy-update-nothing-writable = Rien à copier en UPDATE : aucune des colonnes sélectionnées ne peut être écrite.
grid-copy-where-in-no-key = Rien à copier en WHERE IN : cette table n'a pas de clé primaire ou unique pour désigner ses lignes.
grid-copy-new-row-no-key = Rien à copier : une nouvelle ligne n'a pas de clé tant qu'elle n'est pas enregistrée.
grid-export-fetched-rows = Lignes lues ({ $count })
grid-export-dialog-title = Exporter les résultats
grid-export-already-running = Un export est déjà en cours — rien n'a été écrit.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Ligne { $row }
grid-row-title-in-table = Ligne { $row }  ·  { $table }
grid-row-set-value = Définir la valeur
grid-row-set-null = Mettre à NULL
grid-row-edit-binary = Modifier
grid-row-view-binary = Afficher
grid-row-invalid-json = JSON non valide : { $error }
grid-row-saving = Enregistrement

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } ligne affectée · { $ms } ms
   *[other] { $count } lignes affectées · { $ms } ms
}
grid-rerun-would-write = Impossible de réexécuter cette instruction — elle écrirait dans la base de données
grid-filter-not-simple = Impossible de filtrer cette requête — ce n'est pas un simple SELECT sur une seule table
grid-filter-bracket-in-name = un nom contenant `]` ne peut pas être filtré ici
grid-filter-trailing-input = saisie inattendue après la condition
grid-staged-requery = Validez ou abandonnez d'abord vos modifications — relire les lignes les effacerait
grid-staged-landing = Des modifications ont été mises en attente pendant la relecture de la table : elles ont été conservées et la table n'a pas été actualisée — validez-les ou abandonnez-les, puis actualisez.
grid-reread-without-filter = Le filtre appelle une fonction que Schemaic ne réexécute pas sans qu'on le lui demande : après la validation, la table a donc été relue sans le filtre ni le tri.
grid-hidden-columns-tip = { $count ->
    [one] { $count } colonne masquée — cliquez pour tout afficher, ou faites un clic droit sur un en-tête pour choisir
   *[other] { $count } colonnes masquées — cliquez pour tout afficher, ou faites un clic droit sur un en-tête pour choisir
}
grid-copied-staged-sql = Modifications en attente copiées en SQL.
grid-copy-split = { $n ->
        [one] { $n } cellule copiée contient une tabulation ou un saut de ligne — un collage la lira comme plusieurs cellules.
       *[other] { $n } cellules copiées contiennent une tabulation ou un saut de ligne — un collage lira chacune d'elles comme plusieurs cellules.
    }
grid-duplicate-title = Dupliquer des lignes
grid-duplicate-question = { $n ->
    [one] Dupliquer { $rows } ligne ? Elle est mise en attente comme ligne en suspens, et rien n'est écrit avant la validation.
   *[other] Dupliquer { $rows } lignes ? Elles sont mises en attente comme lignes en suspens, et rien n'est écrit avant la validation.
}
grid-nullable-tip = Accepte NULL
grid-cell-auto = <auto>
grid-cell-required = <obligatoire>
grid-seed-title = Peupler avec des lignes
grid-seed-generate = Générer

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Schéma
footer-panel-ai = Assistant IA
footer-panel-history = Historique des requêtes
footer-panel-snippets = Bibliothèque d'extraits
footer-panel-activity = Activité du serveur
footer-panel-terminal = Terminal
footer-panel-narrow = La fenêtre est trop étroite pour ce panneau — élargissez-la pour le faire réapparaître.
footer-cursor = Ligne { $line }, col. { $col }
footer-indent-spaces = Espaces : { $width }
footer-indent-tabs = Tabulations : { $width }
footer-spaces = Espaces
footer-tabs = Tabulations
footer-wrap = Retour à la ligne
footer-no-wrap = Sans retour à la ligne
footer-read-only = Lecture seule
footer-write-mode = Mode écriture
footer-tx-auto = Validation auto
footer-tx-manual = Manuel
footer-tx-open = Ouverte ({ $count })
footer-tx-aborted = Transaction interrompue — annulez-la pour continuer
footer-tx-lost = Transaction perdue
footer-commit = Valider
footer-rollback = Annuler
footer-cpu = CPU : { $value }
footer-ram = RAM : { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Requête { $n }
lib-result-default-label = Résultat { $n }
lib-ai-off-note = L'assistant IA est désactivé. Choisissez une CLI d'agent dans les paramètres de l'IA pour l'activer.
lib-test-testing = Test…
lib-test-connected = Connecté
lib-test-failed = Échec de la connexion

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Minimal
lib-effort-low = Faible
lib-effort-medium = Moyen
lib-effort-high = Élevé
lib-effort-extra = Très élevé
lib-effort-max = Max
lib-schema-scope-active = Base de données active uniquement
lib-schema-scope-all = Toutes les bases de données
lib-schema-scope-none = Aucun
lib-cursor-block = Bloc
lib-cursor-bar = Barre
lib-cursor-underline = Souligné

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Aucune connexion
header-new-connection = Nouvelle connexion
header-find-anywhere-tip = Rechercher partout ({ $keys })
header-shortcuts-tip = Raccourcis clavier
header-settings-tip = Paramètres
header-disconnected = Déconnecté
header-retry = Réessayer
colour-red = Rouge
colour-orange = Orange
colour-amber = Ambre
colour-green = Vert
colour-teal = Sarcelle
colour-blue = Bleu
colour-purple = Violet
colour-pink = Rose

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = « { $name } » a été modifié sur le disque.
disk-reload = Recharger
disk-keep-mine = Garder ma version
results-run-hint = Exécuter une requête  ({ $keys })
results-failed = Échec de l'instruction.
results-cancelled = Requête annulée.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = { $count } épinglés · { $size }
results-pinned-tip = Les résultats épinglés restent en mémoire jusqu'à ce que vous les fermiez. Fermez ceux dont vous n'avez plus besoin pour libérer la mémoire.
results-chip-nothing-run = Rien n'a encore été exécuté dans cet onglet.
results-chip-kept = conservé — lecture seule
results-menu-pin = Épingler
results-menu-unpin = Désépingler
results-menu-close = Fermer
results-menu-compare-with = Comparer avec { $result }
results-menu-close-others = Fermer les autres résultats
results-menu-close-all = Fermer tous les résultats

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Rechercher partout

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Rien à choisir

widgets-spinner-verbs = Mijoter, Mitonner, Cogiter, Ruminer, Gamberger, Phosphorer, Méditer, Rêvasser, Songer, Supputer, Conjecturer, Extrapoler, Élucubrer, Philosopher, Pontifier, Tergiverser, Turbiner, Carburer, Mouliner, Bidouiller, Bricoler, Fignoler, Peaufiner, Tripatouiller, Trifouiller, Farfouiller, Fureter, Concocter, Cuisiner, Touiller, Pétrir, Infuser, Distiller, Fermenter, Macérer, Mariner, Caraméliser, Flamber, Napper, Saupoudrer, Émulsionner, Assaisonner, Gratiner, Décanter, Tamiser, Ciseler, Pocher, Mûrir, Batifoler, Gambader, Folâtrer, Virevolter, Tournoyer, Louvoyer, Zigzaguer, Serpenter, Onduler, Ondoyer, Tourbillonner, Pétiller, Scintiller, Chatoyer, Germer, Bourgeonner, Éclore, Polliniser, Couver, Incuber, Cristalliser, Sublimer, Transmuter, Métamorphoser, Léviter, Graviter, Orbiter, Planer, Flotter, Voltiger, Papillonner, Vagabonder, Flâner, Musarder, Lambiner, Fredonner, Chantonner, Siffloter, Gazouiller, Tricoter, Broder, Tisser, Manigancer, Jongler, Barboter, Clapoter, Bouillonner, Ressasser, Imaginer, Orchestrer, Chorégraphier, Improviser, Composer, Calculer, Déchiffrer, Échafauder, Ourdir, Clauder, Tambouriner, Pianoter, Griffonner, Crayonner, Gribouiller, Soupeser, Dénouer, Démêler, Façonner, Forger, Sculpter

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Demandez de l'aide à l'assistant IA.
cmdk-accept = Accepter
cmdk-reject = Rejeter
cmdk-no-changes = Aucune modification suggérée
cmdk-diff-summary = { $hunks ->
        [one] { $n } bloc · { $removed } − / { $added } +
       *[other] { $n } blocs · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = valeur
params-kind-tip = Texte, nombre, NULL ou SQL brut — cliquez pour changer
params-missing = Aucune valeur pour { $names }
params-not-a-number = :{ $name } est un nombre, mais contient « { $text } »

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Afficher
editor-run-anyway = Exécuter quand même
guard-read-only = Connexion en lecture seule.
guard-read-only-refused = Connexion en lecture seule : { $reason }
read-only-several = une seule instruction est autorisée
read-only-not-a-read = seules les requêtes en lecture seule ({ $heads }) sont autorisées
read-only-denied = `{ $word }` n'est pas autorisé dans une requête en lecture seule
read-only-advances-sequence = `NEXT VALUE FOR` fait avancer une séquence, ce que ne fait pas une requête en lecture seule
read-only-unlisted-call = `{ $name }` n'est pas une fonction connue pour être en lecture seule : elle n'est donc pas autorisée dans une requête en lecture seule ; les fonctions issues d'extensions ou définies dans la base de données sont refusées aussi
read-only-locks = `{ $clause }` verrouille les lignes qu'il lit, ce que ne fait pas une requête en lecture seule ; retirez la clause de verrouillage
guard-no-database = Aucune base de données sélectionnée.
guard-writes-one = Cette instruction modifie des données.
guard-writes-several = Ces instructions modifient des données.
guard-script = Exécuter toutes les instructions de { $file } ? Un script peut créer, modifier et supprimer des objets, et ce qu'il modifie ne peut pas être annulé d'ici.
guard-staged = { $n ->
    [one] Cette exécution remplace le résultat qui contient { $n } modification en attente — exécuter l'abandonnerait. Validez-la ou abandonnez-la d'abord, ou exécutez quand même.
   *[other] Cette exécution remplace le résultat qui contient { $n } modifications en attente — exécuter les abandonnerait. Validez-les ou abandonnez-les d'abord, ou exécutez quand même.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL supprime toutes les lignes de la table.
guard-truncate = TRUNCATE supprime toutes les lignes de la table.
guard-no-where = Instruction { $kind } sans clause WHERE détectée.
guard-drop-table = DROP TABLE supprime la table et toutes ses lignes.
guard-drop-database = DROP DATABASE supprime la base de données et toutes ses tables.
guard-drop-schema = DROP SCHEMA supprime le schéma et toutes ses tables.
guard-drop-owned = DROP OWNED supprime tout ce que possède le rôle, y compris chaque table et ses lignes.
guard-drop-cascade = DROP { $obj } … CASCADE supprime chaque colonne qui en dépend, avec ses valeurs.
guard-replace-table = CREATE OR REPLACE TABLE supprime la table existante et toutes ses lignes.
guard-replace-database = CREATE OR REPLACE DATABASE supprime la base de données existante et toutes ses tables.
guard-replace-schema = CREATE OR REPLACE SCHEMA supprime le schéma existant et toutes ses tables.
guard-truncate-partition = TRUNCATE PARTITION supprime toutes les lignes des partitions qu'il nomme.
guard-drop-partition = DROP PARTITION supprime la partition et toutes ses lignes.
quickinfo-table = table { $name } · { $database }
quickinfo-view = vue { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } colonne
       *[other] { $n } colonnes
    }
quickinfo-primary-key = clé primaire
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto-incrément
quickinfo-default = défaut { $value }
quickinfo-generated = générée comme { $expression }
quickinfo-function = fonction { $name }({ $arguments })
quickinfo-function-returns = fonction { $name }({ $arguments }) renvoie { $returns }
quickinfo-procedure = procédure { $name }({ $arguments })
quickinfo-procedure-returns = procédure { $name }({ $arguments }) renvoie { $returns }
completion-foreign-key = clé étrangère
completion-expand-star = développer *
completion-columns = { $count ->
        [one] { $n } colonne
       *[other] { $n } colonnes
    }
editor-diag-fix = { $keys } : { $fix }
diag-syntax = Erreur de syntaxe
diag-syntax-detail = Erreur de syntaxe : { $detail }
diag-reserved-alias-backticks = `{ $word }` est un mot-clé réservé et ne peut pas servir d'alias (entourez-le d'accents graves)
diag-reserved-alias-double-quotes = `{ $word }` est un mot-clé réservé et ne peut pas servir d'alias (entourez-le de guillemets doubles)
diag-reserved-alias-brackets = `{ $word }` est un mot-clé réservé et ne peut pas servir d'alias (entourez-le de crochets)
diag-table-not-found = Table `{ $table }` introuvable{ $within ->
        [some] {" "}dans `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— vouliez-vous dire `{ $suggestion }` ?
       *[none] {""}
    }
diag-column-not-found = Colonne `{ $column }` introuvable{ $within ->
        [some] {" "}dans `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— vouliez-vous dire `{ $suggestion }` ?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` a ici l'alias `{ $alias }` — qualifiez avec `{ $alias }`
diag-ambiguous-column = La colonne `{ $column }` est ambiguë
diag-join-without-condition = JOIN sans condition ON/USING — c'est une jointure croisée ; utilisez CROSS JOIN si c'est voulu
diag-comma-join-without-where = Jointure par virgule sans WHERE — c'est une jointure croisée ; ajoutez un WHERE, ou utilisez CROSS JOIN si c'est voulu
diag-not-grouped = La colonne `{ $column }` doit figurer dans le GROUP BY ou être utilisée dans un agrégat
diag-insert-row-width = INSERT nomme { $columns } { $columns ->
        [one] colonne
       *[other] colonnes
    } mais cette ligne a { $values } { $values ->
        [one] valeur
       *[other] valeurs
    }
diag-row-width-without-list = `{ $table }` prend { $needed } { $needed ->
        [one] valeur
       *[other] valeurs
    } sans liste de colonnes, mais cette ligne en a { $values }
diag-row-width-of-table = `{ $table }` a { $columns } { $columns ->
        [one] colonne
       *[other] colonnes
    } mais cette ligne a { $values } { $values ->
        [one] valeur
       *[other] valeurs
    }
diag-nullable-not-in = `{ $column }` peut être NULL, et `NOT IN` ne correspond plus à aucune ligne dès que la sous-requête en renvoie un — ajoutez-y `WHERE { $column } IS NOT NULL`, ou utilisez NOT EXISTS
diag-update-without-where = UPDATE sans WHERE modifie toutes les lignes de la table
diag-delete-without-where = DELETE sans WHERE supprime toutes les lignes de la table
diag-null-comparison = Une comparaison avec NULL n'est jamais vraie — utilisez { $fix }
diag-misspelled-keyword = `{ $word }` ressemble à un mot-clé mal orthographié
diag-misspelled-function = `{ $word }` ressemble à une fonction mal orthographiée
diag-flow-merge-needs-semicolon = `MERGE` doit se terminer par `;`
diag-flow-if-without-statement = `IF` n'a aucune instruction à exécuter
diag-flow-else-without-statement = `ELSE` n'a aucune instruction à exécuter
diag-flow-while-without-statement = `WHILE` n'a aucune instruction à exécuter
diag-flow-end-try-without-begin-try = `END TRY` sans `BEGIN TRY` correspondant
diag-flow-end-catch-without-begin-catch = `END CATCH` sans `BEGIN CATCH` correspondant
diag-flow-end-without-begin = `END` sans `BEGIN` correspondant
diag-flow-throw-needs-semicolon = L'instruction qui précède `THROW` doit se terminer par `;`
diag-flow-end-try-without-catch = `END TRY` doit être suivi de `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` n'a aucune instruction avant son `ELSE`
diag-flow-else-without-if = `ELSE` sans `IF` le précédant
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` doit suivre un `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` n'a pas de `END TRY` correspondant
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` n'a pas de `END CATCH` correspondant
diag-flow-begin-without-end = `BEGIN` n'a pas de `END` correspondant

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Demander à l'IA
editor-menu-explain = Expliquer
editor-menu-optimise = Optimiser
editor-menu-plan = Plan
editor-menu-copy-single-line = Copier sur une seule ligne
editor-menu-make-single-line = Mettre sur une seule ligne
editor-menu-format-statement = Mettre en forme l'instruction
editor-menu-find-usages = Rechercher les utilisations
editor-menu-create-view = Créer une vue
cmdk-optimise-label = Optimiser cette requête
editor-several-carets = Plusieurs curseurs et rien de sélectionné : sélectionnez ce qu'il faut exécuter, ou ne gardez qu'un curseur

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Exécuter l'instruction courante
editor-run-everything = Tout exécuter

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Rechercher
find-invalid = Non valide
find-bad-pattern = Motif non valide
find-bad-pattern-why = Motif non valide : { $why }
find-match-case = Respecter la casse
find-whole-word = Mot entier
find-regex = Expression régulière
find-scope = Sél
find-in-selection-tip = Rechercher dans la sélection
find-replace-placeholder = Remplacer
find-replace = Remplacer
find-replace-all = Tout
goto-line-label = Aller à :

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Aucun client { $client } trouvé (PATH ou WSL).
cli-no-client-on-path = Aucun client { $client } trouvé dans le PATH.
cli-no-tool = Aucun { $tool } trouvé dans le PATH.
cli-no-either-tool = Ni { $first } ni { $second } trouvé dans le PATH.
cli-no-either-client = Aucun client { $first } ou { $second } trouvé dans le PATH.
cli-sqlcmd-windows-in-wsl = L'authentification Windows exige un sqlcmd Windows, et celui trouvé est dans WSL. Installez sqlcmd sous Windows, ou ouvrez la connexion dans un onglet de requête.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = Le client trouvé dans votre PATH est un shim .cmd/.bat, que Windows exécute via cmd.exe — où un nom de base de données contenant '&' ou '|' deviendrait une seconde commande. Pointez les paramètres vers le véritable exécutable, ou ouvrez plutôt un onglet de requête.
cli-refused-name-equals = Le nom de cette base de données contient '=', que psql lirait comme une chaîne de connexion et suivrait vers un autre serveur. Ouvrez-la plutôt dans un onglet de requête.
cli-refused-name-uri = Le nom de cette base de données commence comme une URI de connexion, que psql suivrait vers un autre serveur. Ouvrez-la plutôt dans un onglet de requête.
cli-refused-file-dash = Le nom de ce fichier de base de données commence par '-', que le client sqlite3 lirait comme une option et non comme le fichier. Renommez-le, ou indiquez-le avec un chemin (./name.db), puis rouvrez-le.
cli-refused-sqlcmd-entra = sqlcmd ne peut pas recevoir le jeton d'authentification de l'Azure CLI : cette connexion ne peut donc pas s'y ouvrir. Ouvrez-la plutôt dans un onglet de requête.
cli-refused-sqlcmd-host = L'hôte de cette connexion contient un caractère que sqlcmd lit comme faisant partie de l'adresse (',', ';', un second '\' ou un préfixe ':'). Indiquez le port dans son propre champ, puis rouvrez-la.
cli-refused-sqlcmd-ca = sqlcmd ne peut pas vérifier avec un fichier CA. Ajoutez la CA au magasin de confiance du système et videz le champ CA de la connexion, ou ouvrez la connexion dans un onglet de requête.
cli-refused-sqlcmd-client-cert = SQL Server n'authentifie pas avec un certificat client : sqlcmd n'a donc nulle part où envoyer celui de cette connexion. Videz le certificat et la clé client, puis rouvrez-la.
cli-refused-wsl-paths = Les chemins des certificats TLS de cette connexion sont des chemins Windows, que le client WSL ne peut pas ouvrir. Installez un client natif, ou déplacez les certificats dans WSL.
cli-refused-tunnelled-verify = Cette connexion vérifie le certificat du serveur et le joint par un tunnel SSH : le client recevrait donc l'ordre de joindre 127.0.0.1 et d'y vérifier le certificat — ce que le propre certificat du serveur ne peut pas satisfaire. Ouvrez plutôt un onglet de requête, ou abaissez le mode TLS pour la session CLI.
cli-refused-not-container-id = docker ps a affiché quelque chose qui n'est pas un identifiant de conteneur.
cli-refused-file-line-break = Ce nom de fichier contient un saut de ligne, que le client lirait comme deux arguments. Choisissez un autre nom.
cli-refused-role = Le rôle du SQL d'initialisation de cette connexion ne peut pas être transmis tel quel à l'outil.
cli-refused-mysql-source-path = Le client mysql lirait le chemin de ce fichier comme ses propres commandes (un ';' ou un '\' dedans, ou une espace à une extrémité). Déplacez-le ou renommez-le, puis restaurez-le.
cli-refused-cloud-token = Cette connexion s'authentifie avec le jeton d'une CLI cloud, qui n'est pas transmis à un client externe. Ouvrez-la plutôt dans un onglet de requête.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Impossible de savoir d'où s'exécute Schemaic : { $error }
install-registry-open = Impossible d'ouvrir votre environnement utilisateur dans le registre : { $error }
install-path-read = Impossible de lire votre PATH utilisateur : { $error }
install-path-write = Impossible d'écrire votre PATH utilisateur : { $error }
install-look-at = Impossible d'examiner { $path } : { $error }
install-read-link = Impossible de lire le lien { $path } : { $error }
install-create = Impossible de créer { $path } : { $error }
install-remove-old = Impossible de supprimer l'ancien { $path } : { $error }
install-remove = Impossible de supprimer { $path } : { $error }
install-path-is-windows = Une entrée dans le PATH utilisateur est une installation Windows ; cette plateforme crée un lien à la place.
install-link-is-unix = La création d'un lien vers la commande est une installation macOS et Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Impossible de savoir dans quel dossier se trouve { $exe }.
install-refuse-no-shim = Il n'y a pas de schemaic.com à côté de cette copie de Schemaic, dans { $dir } : il n'y a donc aucune commande console à mettre dans le PATH. L'application installée en a une ; une version de développement non.
install-refuse-semicolon = Cette copie de Schemaic se trouve dans { $dir }, dont le nom contient un ';' — le PATH le lirait comme deux dossiers différents. Déplacez-la ailleurs, dans un dossier sans ce caractère, et réessayez.
install-refuse-exe-replaced = { $exe } a été remplacé pendant l'exécution de Schemaic : un lien vers lui ne pointerait donc vers rien. Redémarrez Schemaic, puis relancez Installer.
install-refuse-no-home = Il n'y a pas de dossier personnel pour accueillir le lien, donc nulle part où chercher.
install-refuse-path-not-text = Votre PATH utilisateur n'est pas stocké sous forme de texte dans le registre : il n'a donc pas été modifié.
install-refuse-path-not-unicode = Votre PATH utilisateur contient des caractères qui ne peuvent pas être relus sans risque : il n'a donc pas été modifié.
install-refuse-portable = Ceci est une copie portable : Supprimer ne peut donc pas savoir si Installer a ajouté { $dir } à votre PATH ou s'il y figurait déjà avec d'autres programmes. Retirez-le dans Variables d'environnement si c'est Installer qui l'a ajouté.
install-refuse-spot-taken = { $link } existe déjà et n'est pas un lien : Installer ne l'a donc pas modifié. Déplacez-le et réessayez.
install-refuse-points-elsewhere = { $link } pointe déjà vers { $to }. Supprimez-le s'il s'agit d'une ancienne copie de Schemaic, puis réessayez.
install-refuse-unlink-elsewhere = { $link } pointe vers { $to }, et non vers cette copie de Schemaic : Supprimer ne l'a donc pas modifié.
install-refuse-unlink-not-ours = { $link } n'est pas un lien créé par Schemaic : Supprimer ne l'a donc pas modifié.
install-report-already = La commande schemaic est déjà dans votre PATH, dans { $dir }.
install-report-added = { $dir } a été ajouté à votre PATH utilisateur. Ouvrez un nouveau terminal pour utiliser schemaic — un terminal déjà ouvert garde l'ancien PATH.
install-report-linked = Lien créé de { $link } vers { $target }.
install-report-linked-if-not-found = Lien créé de { $link } vers { $target }. Si un nouveau terminal ne trouve pas schemaic, ajoutez { $dir } à votre PATH dans le profil de votre shell.
install-report-linked-not-on-path = Lien créé de { $link } vers { $target }. { $dir } n'est pas encore dans votre PATH — ajoutez-le dans le profil de votre shell, puis ouvrez un nouveau terminal.
install-report-removed = { $dir } a été retiré de votre PATH utilisateur. Un terminal déjà ouvert garde l'ancien PATH.
install-report-path-absent = { $dir } n'est pas dans votre PATH utilisateur : il n'y avait donc rien à supprimer.
install-report-unlinked = { $link } a été supprimé.
install-report-link-absent = { $link } n'existe pas : il n'y avait donc rien à supprimer.
install-report-package = La commande schemaic de { $dir } a été fournie par le paquet installé, et non par Installer — supprimez le paquet pour la supprimer.
install-hint-adds-folder = Lancer Schemaic par son nom depuis un terminal. Ajoute son dossier à votre PATH utilisateur.
install-hint-links = Lancer Schemaic par son nom depuis un terminal. Crée un lien dans ~/.local/bin.
install-hint-folder-on-path = Lancer Schemaic par son nom depuis un terminal. Son dossier, { $dir }, est déjà dans votre PATH utilisateur.
install-hint-comes-with-app = Lancer Schemaic par son nom depuis un terminal. La commande schemaic est fournie avec l'application, dans { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = Le tunnel SSH n'est pas encore établi
tunnel-could-not-open = Le tunnel SSH n'a pas pu être ouvert : { $error }. Actualisez le schéma pour réessayer.
tunnel-server-failed = Impossible de se connecter par le tunnel SSH : { $error }. Actualisez le schéma pour réessayer.

gate-plan-not-connected = Non connecté — le plan n'a pas été exécuté. Le serveur n'a pas répondu à la vérification d'état : rien ne lui a donc été envoyé.
gate-plan-connection-changed = Vous avez changé de connexion pendant que Schemaic vérifiait qu'elle était joignable : le plan n'a donc pas été exécuté. Revenez-y et relancez la demande.
gate-plan-tab-moved-on = Vous avez changé d'onglet pendant la nouvelle vérification de la connexion : le plan n'a donc pas été exécuté. Il décrit l'onglet depuis lequel il a été demandé — retournez dans cet onglet et relancez la demande.
gate-connection-changed = Non exécuté sur { $name } — vous avez changé de connexion pendant que Schemaic vérifiait qu'elle était joignable. Revenez-y et réessayez.
gate-not-connected = Non connecté à { $name }. Le serveur n'a pas répondu — vérifiez qu'il fonctionne et que les paramètres de cette connexion sont corrects.
gate-this-connection = cette connexion
gate-run-tab-moved-on = Vous avez changé d'onglet pendant la nouvelle vérification de la connexion : l'instruction n'a donc pas été exécutée. Elle a été vérifiée pour l'onglet où elle a été saisie, et l'exécuter ici l'exécuterait ailleurs. Retournez dans cet onglet et exécutez-la de nouveau.
gate-commit-not-connected = Non connecté — la validation n'a pas été tentée. Les modifications en attente sont conservées.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = Impossible de lancer { $program } : { $error }. Choisissez un autre shell dans Paramètres, Terminal.
term-cli-read-only = Cette connexion est en lecture seule dans Schemaic. Son client en ligne de commande ne le serait pas : il n'est donc pas ouvert ici.
backup-read-only-server = Cette connexion est en lecture seule dans Schemaic, et la sauvegarde de ce moteur est écrite par le serveur : aucune n'a donc été faite.
backup-dialog-title = Sauvegarder la base de données
backup-kind-pg-dump = Dump PostgreSQL au format personnalisé
backup-kind-sql = Script SQL
backup-kind-sqlite = Base de données SQLite
backup-not-started = La sauvegarde n'a pas démarré : { $error }
backup-done = Sauvegarde effectuée dans { $path }.
backup-failed = La sauvegarde a échoué : { $error }
restore-title = Restaurer depuis un fichier
restore-confirm = Exécuter { $path } dans { $database } sur { $connection } ? Ce qu'il crée ou remplace ne peut pas être annulé. Il s'exécute dans le terminal, où Ctrl+C l'arrête.
restore-read-only = Cette connexion est en lecture seule dans Schemaic : rien n'y est donc restauré.
restore-engine-not-here = Les sauvegardes de ce moteur ne se restaurent pas d'ici.
restore-unreadable = { $path } n'a pas pu être lu avant la restauration ({ $error }) : rien n'a donc été exécuté.
restore-refused = { $path } passe à une autre base de données en cours d'exécution — { $switches } — et, restauré dans { $database }, il écrirait donc ailleurs. Rien n'a été exécuté. Restaurez-le dans la base de données qu'il nomme, ou retirez ces lignes d'une copie du fichier.
restore-refused-more = { $path } passe à une autre base de données en cours d'exécution — { $switches }, et { $more } de plus — et, restauré dans { $database }, il écrirait donc ailleurs. Rien n'a été exécuté. Restaurez-le dans la base de données qu'il nomme, ou retirez ces lignes d'une copie du fichier.
restore-switch = { $how } `{ $name }` à la ligne { $line }
restore-switch-unnamed = un { $how } à la ligne { $line } qui ne nomme aucune base de données lisible par Schemaic

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Fermer « { $name } »
tab-close-unsaved = « { $name } » contient des modifications non enregistrées. La fermeture les abandonne ; le fichier sur le disque reste tel quel. Fermer quand même ?
tab-close-all-title = Fermer tous les onglets
tab-close-all-question = Voulez-vous vraiment fermer tous les onglets ?
tab-close-others-title = Fermer les autres onglets
tab-close-others-question = Voulez-vous vraiment fermer tous les autres onglets ?
file-open-title = Ouvrir un fichier SQL
file-save-title = Enregistrer le fichier SQL
file-sql-filter = Script SQL
file-read-failed = Impossible de lire { $path } : { $error }
file-save-failed = Impossible d'enregistrer { $path } : { $error }
file-too-large = { $path } fait { $size } — trop volumineux pour être ouvert dans un onglet d'éditeur. Schemaic passerait l'essentiel de son temps à le réanalyser. Exécutez-le depuis un onglet de requête, ou utilisez Importer pour un fichier de données.
file-too-large-short = Le fichier est trop volumineux pour être ouvert.
file-large-title = Ouvrir un fichier volumineux ?
file-large-question = « { $name } » fait { $size }. Schemaic réanalyse tout le document peu après chaque pause dans la saisie : un fichier de cette taille ralentit donc l'éditeur tant que l'onglet est ouvert. L'ouvrir quand même ?
file-changed-on-disk = { $path } a été modifié sur le disque depuis son ouverture. Enregistrer maintenant abandonnerait ces modifications — rechargez plutôt le fichier (ou enregistrez-le sous un autre nom).
file-lossy-title = Remplacer les octets illisibles ?
file-lossy-question = Schemaic n'a pas pu lire tous les octets de « { $name } » comme du texte et les a affichés sous la forme « � ». L'enregistrement écrit ce que vous voyez : chacun d'eux est donc remplacé définitivement — dans tout le fichier, pas seulement dans les lignes modifiées. Enregistrer quand même ?
file-replace-title = Remplacer le fichier ?
file-replace-question = « { $name } » existe déjà — « { $typed } » a été enregistré avec l'extension .sql ajoutée. Le remplacer ?
file-reload-title = Recharger depuis le disque
file-reload-unsaved = « { $name } » contient des modifications non enregistrées. Le rechargement les abandonne. Recharger quand même ?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Un export est déjà en cours. Annulez-le ou attendez qu'il se termine.
export-failed = Échec de l'export : { $error }
export-worker-died = Échec de l'export : le processus de travail s'est arrêté : { $error }
export-not-published = Échec de l'export : { $path } n'a pas pu être publié : { $error }
export-not-renamed = L'export a écrit { $part } mais n'a pas pu le renommer en { $path } : { $error }
export-nothing-matched = Rien à exporter — aucune table ne correspond à la sélection.
export-stopped = Export annulé.
dump-failed = Échec du dump : { $error }
blob-load-too-large = Ce fichier fait { $size } — le maximum chargeable est { $most }.
blob-load-failed = Échec du chargement : { $error }
blob-save-failed = Échec de l'enregistrement : { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = Impossible d'écrire { $path } : { $error }
settings-export-title = Exporter les paramètres
settings-import-title = Importer les paramètres
settings-file-filter = Paramètres Schemaic
settings-import-nothing = Rien à importer : chaque paramètre de { $path } correspond déjà aux vôtres, et le fichier n'ajoute aucun extrait.
settings-import-question = { $path } modifie { $changed ->
        [one] { $changed } paramètre
       *[other] { $changed } paramètres
    } et ajoute { $snippets ->
        [one] { $snippets } extrait
       *[other] { $snippets } extraits
    }. Les paramètres modifiés remplacent vos valeurs actuelles ; les extraits sont ajoutés à côté des vôtres.
settings-import-unreadable = impossible de le lire : { $error }
connections-export-title = Exporter les connexions
connections-file-filter = Connexions Schemaic
connections-replace-title = Remplacer « { $file } »
connections-replace-question = Un fichier nommé « { $file } » existe déjà dans ce dossier. L'export ajoute cette terminaison au nom saisi : la boîte de dialogue d'enregistrement ne l'a donc pas signalé. Le remplacement est irréversible.
import-choose-file-title = Choisir un fichier de connexions
import-no-containers = Aucun conteneur de base de données n'est en cours d'exécution.
import-added = { $count ->
        [one] { $count } connexion ajoutée.
       *[other] { $count } connexions ajoutées.
    }
import-added-foreign = { $count ->
        [one] { $count } connexion ajoutée.
       *[other] { $count } connexions ajoutées.
    } La lecture seule et le badge d'environnement ne sont pas repris d'un autre client — définissez-les dans Gérer les connexions pour tout ce qui compte.
conn-delete-title = Supprimer « { $name } »
conn-delete-question = Cela supprime la connexion, son mot de passe enregistré, sa conversation IA enregistrée, son historique de requêtes et ses onglets. Cette action est irréversible.
conn-delete-keyring-kept = Schemaic n'a pas pu joindre le trousseau du système : le mot de passe et les secrets SSH enregistrés de la connexion supprimée **y sont toujours**.
conn-delete-keyring-later = Ils seront supprimés la prochaine fois qu'une connexion sera supprimée ou enregistrée alors que le trousseau est joignable. Aucune nouvelle connexion ne peut les recevoir d'ici là — les identifiants de connexion ne sont jamais réutilisés.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = { $name } n'a pas pu être lu ({ $error }).
notice-unreadable-kept = Le fichier illisible a été conservé sous le nom { $name }.corrupt ; Schemaic s'est rabattu sur sa sauvegarde, ou sur les valeurs par défaut si la sauvegarde était illisible elle aussi.
notice-unreadable-swept = Cette copie peut contenir des mots de passe en clair : elle est donc supprimée la prochaine fois que vous supprimez une connexion — récupérez-en d'ici là tout ce dont vous avez besoin.
notice-missing = { $name } était manquant.
notice-missing-restored = Schemaic l'a récupéré depuis { $name }{ $from }, qui était encore présent. Il sera réécrit la prochaine fois que ce paramètre changera — d'ici là, cette copie voisine est la seule.
notice-keyring-unread = { $n ->
    [one] Schemaic n'a pas pu lire { $n } secret enregistré dans le trousseau du système.
   *[other] Schemaic n'a pas pu lire { $n } secrets enregistrés dans le trousseau du système.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic n'a pas pu lire { $n } secret enregistré dans le trousseau du système ({ $error }).
   *[other] Schemaic n'a pas pu lire { $n } secrets enregistrés dans le trousseau du système ({ $error }).
}
notice-keyring-unread-kept = Ces mots de passe sont indisponibles pour cette session et n'ont **pas** été supprimés — laissez les champs vides et ils reviendront dès que le trousseau sera joignable. En saisir un nouveau remplace le secret enregistré.
notice-in-the-clear = { $n ->
    [one] Schemaic n'a pas pu joindre le trousseau du système : { $n } mot de passe a donc été enregistré **en clair** dans `connections.json`.
   *[other] Schemaic n'a pas pu joindre le trousseau du système : { $n } mots de passe ont donc été enregistrés **en clair** dans `connections.json`.
}
notice-in-the-clear-where = Ce fichier se trouve dans le dossier qu'ouvre le bouton *Ouvrir le dossier* de Paramètres → Général, à côté du journal — vérifiez-le avant de le partager. Enregistrer de nouveau une fois le trousseau joignable les y replace.
notice-undeleted = { $n ->
    [one] { $n } mot de passe effacé se trouve encore dans le trousseau du système — Schemaic n'a pas pu le supprimer.
   *[other] { $n } mots de passe effacés se trouvent encore dans le trousseau du système — Schemaic n'a pas pu les supprimer.
}
notice-undeleted-after = Le champ est vide sur le disque, mais le prochain lancement le remplira de nouveau. Enregistrez de nouveau une fois le trousseau joignable.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Aucun SQL renvoyé
inline-ai-not-sql = Le modèle n'a pas renvoyé de SQL
inline-ai-failed = la génération a échoué
ai-fill-no-value = L'IA n'a renvoyé aucune valeur.
conn-gone = la connexion n'existe plus
tx-session-not-ready = la connexion de transaction n'est pas prête — passez en Validation auto puis revenez
tx-session-open-failed = impossible d'ouvrir une connexion de transaction : { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = L'instruction a été annulée.
tx-committed-before-ddl = La transaction dans laquelle ceci s'exécutait n'existe plus : MySQL valide avant d'exécuter une instruction DDL, les instructions qu'elle contenait sont donc définitives et Annuler ne les défera pas.
tx-rolled-back-by-server = La transaction dans laquelle ceci s'exécutait n'existe plus : SQL Server l'a annulée, et toutes ses instructions depuis son début sont donc défaites.
tx-doomed = La transaction dans laquelle ceci s'exécutait ne peut plus être validée : SQL Server l'a marquée comme non validable, Annuler est donc la seule issue, et cela défait toutes ses instructions.
tx-after-own-commit = La transaction dans laquelle ceci s'exécutait a pris fin, et ceci a exécuté son propre COMMIT avant d'échouer : ce qui précédait ce COMMIT est donc probablement validé plutôt que défait. Vérifiez les données avant d'en réexécuter quoi que ce soit.
tx-lost-on-commit = La connexion a été perdue avec une transaction ouverte. Elle a été perdue sur un COMMIT : on ne sait donc pas si { $stmts ->
        [0] la transaction a été validée ou annulée
        [one] l'instruction exécutée dans la transaction depuis son début ({ $stmts }) a été validée ou annulée
       *[other] les { $stmts } instructions exécutées dans la transaction depuis son début ont été validées ou annulées
    } : le serveur a pu valider avant la coupure de la connexion. Vérifiez les données avant d'en réexécuter quoi que ce soit.
tx-lost-on-implicit-commit = La connexion a été perdue avec une transaction ouverte. MySQL valide une transaction ouverte avant d'exécuter cette instruction : { $stmts ->
        [0] rien n'y avait encore été exécuté, il n'y avait donc rien à valider
        [one] l'instruction exécutée dans la transaction depuis son début ({ $stmts }) a donc très probablement été validée à ce moment-là, et elle est définitive
       *[other] les { $stmts } instructions exécutées dans la transaction depuis son début ont donc très probablement été validées à ce moment-là, et elles sont définitives
    }. Vérifiez les données avant d'en réexécuter quoi que ce soit.
tx-lost-rolled-back = La connexion a été perdue avec une transaction ouverte. Le serveur annule une transaction inachevée quand sa connexion se termine : { $stmts ->
        [0] rien n'y avait encore été exécuté
        [one] l'instruction exécutée dans la transaction depuis son début ({ $stmts }) est donc défaite
       *[other] les { $stmts } instructions exécutées dans la transaction depuis son début sont donc défaites
    }.
tx-lost-rolled-back-mysql = La connexion a été perdue avec une transaction ouverte. Le serveur annule une transaction inachevée quand sa connexion se termine : { $stmts ->
        [0] rien n'y avait encore été exécuté
        [one] l'instruction exécutée dans la transaction depuis son début ({ $stmts }) est donc défaite — sauf une écriture dans une table qui ne prend pas en charge les transactions (MyISAM, MEMORY), qu'aucune annulation ne défait
       *[other] les { $stmts } instructions exécutées dans la transaction depuis son début sont donc défaites — sauf une écriture dans une table qui ne prend pas en charge les transactions (MyISAM, MEMORY), qu'aucune annulation ne défait
    }.
tx-not-sent-timeout = Le délai d'expiration de l'instruction a été atteint alors que cette exécution attendait encore la connexion de l'onglet. Une opération antérieure sur celle-ci — une validation, ou la relecture qui la suit — n'était pas terminée : l'instruction n'a donc jamais été envoyée au serveur. Rien ne s'est exécuté, et la transaction est exactement dans le même état.
tx-not-sent-cancelled = Cette exécution a été annulée alors qu'elle attendait encore la connexion de l'onglet. Une opération antérieure sur celle-ci — une validation, ou la relecture qui la suit — n'était pas terminée : l'instruction n'a donc jamais été envoyée au serveur. Rien ne s'est exécuté, et la transaction est exactement dans le même état.
snippet-default-name = Extrait
snippet-delete-title = Supprimer l'extrait
snippet-delete-question = Supprimer « { $name } » ? Cette action est irréversible.
copy-target-read-only = La connexion cible est en lecture seule : aucune ligne n'a donc été copiée.
copy-target-gone = La connexion cible n'existe plus : aucune ligne n'a donc été copiée.
copy-not-started = Aucune ligne n'a été copiée : { $error }
copy-counter-not-moved = { $count ->
        [one] { $count } ligne a été copiée, mais le compteur de la table n'a pas été avancé au-delà : la prochaine insertion risque donc une collision : { $error }
       *[other] { $count } lignes ont été copiées, mais le compteur de la table n'a pas été avancé au-delà : la prochaine insertion risque donc une collision : { $error }
    }
copy-stopped = La copie s'est arrêtée sur une erreur : { $error }
compare-reading-failed = Lecture de { $database } : { $error }
monitor-no-row-key = Aucune clé de ligne pour cette table — les modifications ne peuvent pas être suivies.
monitor-restarted = Les colonnes de la table ont changé : le suivi des modifications a donc repris à partir d'ici.
timeout-cancelled-app = Annulée : l'instruction a dépassé le délai d'expiration de { $timeout }. Modifiez-le ou désactivez-le dans Paramètres → Requête.
timeout-cancelled-connection = Annulée : l'instruction a dépassé le délai d'expiration de { $timeout }. Modifiez-le ou désactivez-le dans les paramètres de cette connexion.
ai-chat-no-database = Impossible de joindre la base de données : l'assistant ne peut donc pas être lancé pour cette connexion. Vérifiez la connexion et réessayez.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **L'assistant n'a pas pu démarrer.** Schemaic donne à chaque processus d'agent son propre répertoire de travail privé, et ni le répertoire temporaire du système ni le répertoire de configuration n'ont pu en accueillir un — il n'y a donc nulle part où l'exécuter qui ne soit pas le répertoire de quelqu'un d'autre. Vérifiez que le répertoire temporaire est accessible en écriture et réessayez.
ai-no-reply-file = Impossible de créer un fichier temporaire pour la réponse.
ai-inline-no-opencode-config = Impossible d'écrire la configuration d'OpenCode : la génération n'a donc pas été lancée (sans elle, la CLI s'exécuterait avec ses propres outils activés).
ai-inline-no-copilot-home = Impossible de créer le dossier personnel privé de GitHub Copilot : la génération n'a donc pas été lancée (sans lui, la CLI chargerait votre propre configuration Copilot, y compris ses serveurs MCP et ses hooks).
ai-inline-no-env-writer = { $cli } est marqué comme isolé par son environnement, mais Schemaic n'a aucun moyen de configurer cet environnement : la génération n'a donc pas été lancée.
ai-inline-no-cursor-rules = Impossible d'écrire le fichier d'autorisations de Cursor : la génération n'a donc pas été lancée (sans lui, la CLI écrit des fichiers sans demander).
ai-inline-no-rules-writer = { $cli } est marqué comme restreint par un fichier d'autorisations, mais Schemaic n'a aucun moyen d'en écrire un pour lui : la génération n'a donc pas été lancée.
ai-no-tools = **Cette session n'a pas d'outils de base de données.** { $cause } : l'assistant ne peut donc rien consulter — il ne peut répondre qu'à partir du plan du schéma déjà présent dans son invite. Fermer et rouvrir le panneau fera une nouvelle tentative.
ai-no-endpoint-file = Schemaic n'a pas pu créer le fichier privé qui indique à l'assistant comment joindre votre base de données
ai-cursor-not-approved = Schemaic n'a pas pu faire approuver ses outils de base de données par Cursor ({ $error })
ai-antigravity-not-registered = Schemaic n'a pas pu enregistrer ses outils de base de données auprès d'Antigravity
ai-antigravity-shared = Une autre fenêtre de Schemaic utilise déjà Antigravity. Antigravity conserve un seul enregistrement MCP pour toute la machine, qui ne peut pointer que vers une seule connexion : l'assistant de cette fenêtre n'a donc pas d'outils de base de données
ai-antigravity-not-claimed = Schemaic n'a pas pu s'approprier la configuration d'Antigravity pour cette session
ai-refused-no-opencode-config = Schemaic n'a pas pu écrire la configuration qui restreint OpenCode : l'assistant est donc désactivé — l'exécuter sans ce fichier donnerait un shell à la session. Vérifiez que le répertoire de données de l'application est accessible en écriture.
ai-refused-no-copilot-home = Schemaic n'a pas pu créer le dossier personnel privé de GitHub Copilot : l'assistant est donc désactivé — sans lui, la CLI chargerait votre propre configuration Copilot, y compris ses serveurs MCP et ses hooks. Vérifiez que le répertoire de données de l'application est accessible en écriture.
ai-refused-no-cursor-rules = Schemaic n'a pas pu écrire le fichier d'autorisations qui empêche Cursor d'écrire des fichiers et d'exécuter des commandes : l'assistant est donc désactivé — sans lui, Cursor écrit des fichiers sans demander. Vérifiez que le répertoire de données de l'application est accessible en écriture.
ai-launch-failed-turn = Impossible de lancer la CLI `{ $bin }` ({ $error }). Vérifiez que { $cli } est installé, ou indiquez son chemin dans Paramètres → IA.
ai-launch-failed-session = Impossible de lancer la CLI `{ $bin }` ({ $error }). Vérifiez que { $cli } est installé (ou définissez son chemin dans Paramètres → IA).
ai-turn-ended = Le tour de { $cli } s'est terminé de façon inattendue : { $error }
ai-session-ended-badly = La session IA s'est terminée de façon inattendue : { $error }
ai-session-ended = La session { $cli } est terminée. Posez de nouveau votre question pour en démarrer une nouvelle.
ai-session-restart-failed = La session { $cli } n'a pas pu être redémarrée après son arrêt ({ $error }) : elle est donc terminée. Posez de nouveau votre question pour en démarrer une nouvelle.
ai-session-not-accepting = La session { $cli } n'accepte plus de questions. Posez de nouveau votre question pour en démarrer une nouvelle.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = Le contexte envoyé à { $cli } est trop volumineux pour une seule ligne de commande ({ $total } { $unit } ; cette plateforme en autorise environ { $limit }). { $levers }
ai-unit-characters = caractères
ai-unit-bytes = octets
ai-levers-schema-and-query = Restreignez Paramètres → IA → Contexte du schéma à la base de données active (ou à Aucun), ou raccourcissez la requête dans l'éditeur.
ai-levers-sample = Abaissez l'accès de l'IA aux données de cette connexion sous « Lui laisser lire les données », ce qui cesse d'envoyer l'échantillon de lignes, ou restreignez Paramètres → IA → Contexte du schéma.
ai-batch-shim = { $cli } correspond à un shim batch ({ $bin }), qui ne peut pas recevoir d'invite sur plusieurs lignes — Windows exécute un .cmd via cmd.exe, qui refuse l'argument avant le lancement de { $cli }. Définissez Paramètres → IA → Chemin de la CLI sur le véritable exécutable (une installation npm en a généralement un sous node_modules, à côté du shim).
ai-cli-exited = la CLI { $cli } s'est terminée avec le code { $code }
ai-cli-signalled = la CLI { $cli } a été arrêtée par un signal
ai-constraint-sandboxed = { $cli } s'exécute dans un bac à sable : ses propres outils intégrés ne peuvent ni écrire de fichiers ni exécuter de commandes, mais ils peuvent toujours lire cette machine.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Tous les serveurs MCP que vous y avez enregistrés sont aussi disponibles dans cette session, et Schemaic ne peut pas les restreindre.
    }
ai-constraint-workspace-rules = { $cli } est cantonné à répondre par un fichier d'autorisations que Schemaic écrit pour cette session, et non par un bac à sable : ses propres outils se voient refuser l'écriture de fichiers et l'exécution de commandes, mais ils peuvent toujours lire les fichiers de cette machine.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Les serveurs MCP que vous y avez enregistrés démarrent aussi avec cette session ; leurs outils sont refusés sauf si vos propres paramètres les autorisent.
    }
ai-constraint-denylist = Cette version de { $cli } n'accepte pas l'option qui vide ses outils intégrés : ils sont donc retenus par une liste de refus — plus faible, et que Schemaic ne peut pas garantir. Mettre à jour la CLI rétablit le verrouillage complet.
ai-constraint-unrestricted = Schemaic n'a pas pu restreindre entièrement les outils intégrés de { $cli } pour cette session : il pourrait donc faire plus que répondre à des questions sur cette base de données.
ai-constraint-unknown = Schemaic n'a pas pu confirmer que ce binaire { $cli } accepte les options qui le restreignent : l'assistant est donc désactivé. Vérifiez le chemin dans Paramètres → IA.
ai-seed-empty = le modèle n'a renvoyé aucun contenu
ai-seed-not-json = la réponse n'était pas du JSON valide : { $error }
ai-seed-not-rows = la réponse n'était pas un tableau JSON d'objets ligne

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } n'est pas un fichier.
import-source-too-big = { $path } fait { $size }, au-delà des { $most } lus ici.
import-source-unreadable = { $path } n'a pas pu être lu : { $error }
docker-not-found = Docker est introuvable — ni dans le PATH ni dans WSL.
docker-not-started = Docker n'a pas pu être lancé : { $error }
docker-timed-out = Docker n'a pas répondu en { $secs } secondes.
docker-not-waited = Impossible d'attendre Docker : { $error }
docker-answered = Docker a répondu : { $error }
docker-exited = Docker s'est terminé avec { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = le lecteur de fichier s'est arrêté de façon inattendue : { $error }
script-changed-since-read = Ce fichier a été modifié sur le disque après sa lecture : le résumé ci-dessus ne correspond donc pas à ce qui serait exécuté. Choisissez-le de nouveau.
script-no-terminator = Ce fichier ne contient aucun terminateur d'instruction : ce n'est donc pas un script SQL que Schemaic peut exécuter. Vérifiez que vous avez choisi le bon fichier.
ai-cursor-enable-unfinished = `mcp enable` ne s'est pas terminé
