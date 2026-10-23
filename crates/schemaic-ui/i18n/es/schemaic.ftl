# Schemaic — Spanish catalogue.
#
# Machine-drafted and unreviewed (docs/architecture.md, The string catalogue); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Spanish.

## Settings › Appearance › Language

settings-language = Idioma
settings-language-system = Sistema ({ $language })
settings-language-restart = El cambio de idioma se aplicará la próxima vez que se inicie Schemaic.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Tamaño de fuente
settings-term-cursor-style = Estilo del cursor
settings-term-copy-on-select = Copiar al seleccionar
settings-term-copy-on-select-hint = Copia el texto seleccionado al portapapeles en cuanto termina la selección.
settings-term-blink = Cursor parpadeante
settings-term-blink-hint = Hace parpadear el cursor mientras el terminal tiene el foco.

## Settings › AI

settings-ai-agent = CLI del agente
settings-ai-disabled = Desactivado
settings-ai-off-hint = No se envía nada a ningún agente, no se inicia ninguna CLI de agente y no se ofrece ninguna acción de IA. Elige una CLI para activar el asistente.
settings-ai-cli-path = Ruta de { $cli }
settings-ai-cli-path-placeholder = Déjalo vacío para detectarla automáticamente
settings-ai-detected = Detectado automáticamente: { $path }
settings-ai-detect-failed = La detección automática ha fallado. No se ha encontrado { $cli }: indica su ruta arriba.
settings-ai-cli-path-missing = El archivo no existe.
settings-ai-model = Modelo
settings-ai-model-placeholder = Déjalo vacío para usar el predeterminado de la CLI del agente
settings-ai-model-hint = Cualquier id que acepte esta CLI: un alias, o una versión fechada para fijar un modelo entre actualizaciones.
settings-ai-model-hint-with-chips = Cualquier id que acepte esta CLI: un alias, o una versión fechada para fijar un modelo entre actualizaciones. Los botones son atajos, no la lista completa.
settings-ai-effort = Esfuerzo
settings-ai-instructions = Instrucciones personalizadas
settings-ai-instructions-placeholder = Dialecto, convenciones, normas de la casa…
settings-ai-schema-context = Contexto del esquema
settings-ai-schema-context-hint = Cuánta estructura de la base de datos acompaña a cada mensaje. Ninguno también retira las herramientas de esquema, así que el asistente te pide los nombres en lugar de leer el catálogo por sí mismo.
settings-ai-data-access = Acceso a datos
settings-ai-data-access-where = Se configura por conexión, en los ajustes de la conexión.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Solo el esquema
ai-data-on-request = Solo lo que yo adjunte
ai-data-full = Dejar que lea datos
ai-data-schema-only-hint = Solo la estructura: nombres, tipos y definiciones. Ninguna fila sale nunca de este equipo, y adjuntar está bloqueado.
ai-data-on-request-hint = El asistente no lee datos por su cuenta. Las filas que adjuntes de un resultado salen de este equipo junto con esa pregunta.
ai-data-full-hint = El asistente puede ejecutar consultas de solo lectura y leer filas de muestra por sí mismo. Todo lo que lea sale de este equipo.
settings-ai-gutter = Línea de acento en las respuestas
settings-ai-gutter-hint = Marca las respuestas del asistente con una línea de color en su borde derecho. Si se desactiva, tienen el mismo margen a ambos lados.

## Settings › section list

settings-section-general = General
settings-section-editor = Editor
settings-section-formatting = Formato de código
settings-section-query = Consulta
settings-section-terminal = Terminal
settings-section-ai = Asistente de IA
settings-section-appearance = Apariencia

## Settings › shared

settings-off = Desactivado

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } carácter
   *[other] { $n } caracteres
}

## Settings › General

settings-log = Archivo de registro
settings-log-hint = Los diagnósticos, incluidos los bloqueos, se escriben en { $path }.
settings-log-hint-none = No se ha podido abrir ningún archivo de registro en este equipo, así que no se está escribiendo en ninguno.
settings-log-open-folder = Abrir carpeta
settings-bundle = Archivo de ajustes
settings-bundle-hint = Tus preferencias y fragmentos en un solo archivo, para llevarlos a otro equipo. Las conexiones, la disposición de la ventana, el shell del terminal y los fragmentos propios de una conexión se quedan aquí.
settings-bundle-export = Exportar
settings-bundle-import = Importar
settings-cli = Línea de comandos
settings-cli-install = Instalar
settings-cli-installing = Instalando
settings-cli-remove = Quitar
settings-cli-removing = Quitando

## Settings › modal

settings-title = Ajustes
settings-restore-tabs = Restaurar pestañas al iniciar
settings-restore-tabs-hint = Vuelve a abrir las pestañas de consulta de tu última sesión al iniciar la aplicación.

## Settings › Editor

settings-editor-font-size = Tamaño de fuente
settings-editor-predictions = Predicciones en línea
settings-editor-predictions-hint = Muestra las palabras siguientes probables tras el cursor: la pareja de una palabra clave, la clave foránea de un join, las columnas por las que agrupar o que insertar. Tab, o → al final de una línea, las acepta enteras; Ctrl+→ (⌘→ en macOS), palabra a palabra; y Escape las descarta.
settings-editor-remembered = Predicciones de sentencias anteriores
settings-editor-remembered-hint = Ofrece también el resto de una línea a partir de una sentencia que ejecutaste, escribiste en otra pestaña o guardaste como fragmento. Desactívalo para que las sentencias anteriores no aparezcan en pantalla.
settings-editor-aliases = Alias de tabla
settings-editor-aliases-hint = Cuando el autocompletado escribe una tabla tras FROM o JOIN, la sigue de un alias corto formado por sus iniciales —orders o, order_items oi— y usa el alias en la condición ON de un join.
settings-editor-rainbow = Paréntesis de colores
settings-editor-rainbow-hint = Colorea cada par de paréntesis según su nivel de anidamiento, para emparejar de un vistazo un paréntesis de cierre con el de apertura.
settings-editor-whitespace = Mostrar espacios en blanco
settings-editor-whitespace-hint = Dibuja los espacios y tabuladores como marcas tenues, para distinguir un tabulador de los espacios.
settings-editor-guides = Guías de sangría
settings-editor-guides-hint = Dibuja una línea vertical tenue en cada nivel de sangría.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Mayúsculas de palabras clave
settings-format-case-keep = Como se escribió
settings-format-case-upper = Mayúsculas
settings-format-case-lower = Minúsculas
settings-format-commas-trailing = Al final de la línea
settings-format-commas-leading = Al principio de la línea
settings-format-gap-one = Una línea en blanco
settings-format-gap-none = Sin línea en blanco
settings-format-keyword-case-hint = Cómo escribe Formatear código las palabras clave como SELECT y FROM. Los nombres, las cadenas y los comentarios conservan sus mayúsculas y minúsculas.
settings-format-function-case = Mayúsculas de funciones
settings-format-function-case-hint = Cómo escribe Formatear código las funciones propias del motor, como COUNT y COALESCE, donde se llaman. Tus propias funciones conservan sus mayúsculas y minúsculas.
settings-format-type-case = Mayúsculas de tipos
settings-format-type-case-hint = Cómo escribe Formatear código los tipos de datos propios del motor, como INT y VARCHAR. Los tipos que hayas creado conservan sus mayúsculas y minúsculas.
settings-format-commas = Comas
settings-format-commas-hint = Dónde pone Formatear código la coma de una lista que reparte en varias líneas. Las listas que quedan en una sola línea, como los argumentos de una función, no se tocan salvo que el ancho de ajuste las divida.
settings-format-statement-gap = Entre sentencias
settings-format-statement-gap-hint = Las líneas en blanco que deja Formatear código entre dos sentencias. Como se escribió conserva una línea en blanco allí donde la hubiera.
settings-format-wrap = Ancho de ajuste
settings-format-wrap-hint = Una lista que Formatear código mantiene en una línea, como los argumentos de una función, pasa a un elemento por línea en cuanto su línea supere este ancho.
settings-format-river = Alinear a la derecha las palabras clave de cláusula
settings-format-river-hint = Alinea SELECT, FROM, WHERE y las demás por su borde derecho, con el cuerpo de cada cláusula junto a su palabra clave, en lugar de poner cada palabra clave en su propia línea.
settings-format-align = Alinear columnas
settings-format-align-hint = Alinea el AS de los alias de una lista SELECT, el = de las asignaciones de un UPDATE y los tipos de las columnas de una tabla.
settings-format-join-on = Condición del join en su propia línea
settings-format-join-on-hint = Pone el ON de un join, y los AND y OR de su condición, en líneas propias bajo el join.

## Settings › Query

settings-query-row-limit = Límite de filas predeterminado
settings-query-timeout = Tiempo de espera de la sentencia
settings-query-timeout-hint = Cancela una sentencia que tarde más que esto. Desactivado de forma predeterminada: una importación o un informe pueden tardar bastante con razón.
timeout-off = Sin tiempo de espera
timeout-hours = { $n ->
    [one] { $n } hora
   *[other] { $n } horas
}
timeout-minutes = { $n ->
    [one] { $n } minuto
   *[other] { $n } minutos
}
timeout-seconds = { $n ->
    [one] { $n } segundo
   *[other] { $n } segundos
}
settings-query-confirm-writes = Confirmar antes de ejecutar escrituras
settings-query-confirm-writes-hint = Pregunta antes de ejecutar cualquier sentencia que modifique datos o el esquema.
settings-query-validate = Validación en vivo con la base de datos
settings-query-validate-hint = Comprueba la sentencia bajo el cursor contra la base de datos mientras escribes (un PREPARE que no ejecuta nada) para mostrar los errores exactos. Añade un viaje de ida y vuelta a la base de datos en cada pausa.
settings-query-notify = Avisar cuando termine una ejecución en segundo plano
settings-query-notify-hint = Muestra una notificación de escritorio cuando una consulta, un script, una exportación o un volcado que haya tardado un segundo o más termine mientras Schemaic no es la ventana activa. El botón de la barra de tareas se resalta en cualquier caso. La notificación nunca incluye el SQL.
notify-query-finished = Consulta terminada
notify-query-failed = La consulta ha fallado
notify-script-finished = Script terminado
notify-script-failed = El script ha fallado
notify-export-finished = Exportación terminada
notify-export-failed = La exportación ha fallado
notify-dump-finished = Volcado terminado
notify-dump-failed = El volcado ha fallado
notify-took = Ha tardado { $duration }.
took-millis = { $ms } ms
took-seconds = { $s } s
took-minutes = { $m } min { $s } s
settings-query-write-log = Conservar los valores en el registro de escrituras
settings-query-write-log-hint = Cada escritura que ejecutan Schemaic o schemaic exec se añade a write_log.jsonl en la carpeta de ajustes, y nunca se borra. Los valores de su SQL se enmascaran salvo que esto esté activado; una contraseña se enmascara en cualquier caso.

## Settings › Appearance

settings-appearance-ui-theme = Tema de la interfaz
settings-appearance-editor-theme = Tema del editor
settings-appearance-scale = Escala de la interfaz
settings-appearance-resources = Mostrar CPU y RAM en la barra de estado
settings-appearance-resources-hint = Cuánto tiempo de procesador y memoria usa el propio Schemaic. Desactivado, deja de medirlos además de mostrarlos.

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Atajos

## Shortcuts modal › group headings

shortcuts-group-global = Globales
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Cuadrícula de resultados
shortcuts-group-schema-tree = Árbol del esquema
shortcuts-group-ai-panel = Panel de IA
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = Diagrama ER

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Buscar en todo
shortcuts-command-palette = Paleta de comandos
shortcuts-new-tab = Nueva pestaña de consulta
shortcuts-close-tab = Cerrar pestaña de consulta
shortcuts-next-tab = Pestaña siguiente
shortcuts-previous-tab = Pestaña anterior
shortcuts-jump-to-tab = Ir a una pestaña
shortcuts-reopen-tab = Reabrir la última pestaña cerrada
shortcuts-open-file = Abrir archivo SQL
shortcuts-save-file = Guardar archivo SQL
shortcuts-save-file-as = Guardar archivo SQL como
shortcuts-toggle-schema = Mostrar u ocultar el panel del esquema
shortcuts-toggle-ai = Mostrar u ocultar el panel de IA
shortcuts-toggle-terminal = Mostrar u ocultar el terminal
shortcuts-run-query = Ejecutar consulta
shortcuts-autocomplete = Autocompletar
shortcuts-inline-ai = Edición en línea con IA
shortcuts-find-in-editor = Buscar en el editor
shortcuts-find-replace = Buscar y reemplazar
shortcuts-go-to-line = Ir a la línea
shortcuts-matching-bracket = Saltar al paréntesis o BEGIN/END correspondiente
shortcuts-line-comment = Alternar comentario de línea
shortcuts-block-comment = Alternar comentario de bloque
shortcuts-toggle-case = Alternar mayúsculas y minúsculas
shortcuts-join-lines = Unir líneas
shortcuts-duplicate = Duplicar línea / selección
shortcuts-cut = Cortar línea / selección
shortcuts-format-code = Formatear código
shortcuts-format-statement = Formatear sentencia
shortcuts-single-line = Pasar a una sola línea
shortcuts-move-line = Mover línea arriba / abajo
shortcuts-quick-fix = Aplicar la corrección rápida bajo el cursor
shortcuts-indent = Aumentar / reducir sangría
shortcuts-take-prediction = Aceptar la predicción en línea (también →, al final de la línea) / su siguiente palabra
shortcuts-find-in-results = Buscar en los resultados
shortcuts-go-to-row = Ir a la fila
shortcuts-copy = Copiar
shortcuts-paste-staged = Pegar como cambios pendientes
shortcuts-select-all = Seleccionar todo
shortcuts-first-last-cell = Primera / última celda
shortcuts-edit-cell = Editar celda
shortcuts-next-cell = Celda siguiente / anterior durante la edición
shortcuts-commit-edits = Confirmar cambios
shortcuts-mark-deletion = Marcar la fila para eliminarla
shortcuts-results-toolbar = Ir a la barra de herramientas de resultados
shortcuts-along-toolbar = Moverse por la barra de herramientas (Esc vuelve a la cuadrícula)
shortcuts-open-selected = Abrir la tabla, columna u objeto seleccionado
shortcuts-context-menu = Menú contextual de la fila seleccionada
shortcuts-send-message = Enviar mensaje
shortcuts-new-line = Nueva línea
shortcuts-recall-prompt = Recuperar la instrucción anterior / siguiente
shortcuts-copy-paste = Copiar / pegar
shortcuts-find-in-diagram = Buscar en el diagrama

## Key names in shortcut labels — a keyboard's own legends, so a language
## whose keyboards print another word uses it (German: Strg, Umschalt, Entf).
## Keys written the same everywhere (letters, arrows, Tab, Esc, F6) and macOS's
## Cmd are not here. Read key by key by `shortcuts::keys_label`.

key-ctrl = Ctrl
key-shift = Mayús
key-alt = Alt
key-enter = Intro
key-del = Supr
key-home = Inicio
key-end = Fin
key-space = Espacio

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Mostrar u ocultar panel
palette-toggle-panel-hint = esquema · ia · terminal · historial de consultas · actividad del servidor
palette-panel-schema = Esquema
palette-panel-ai = IA
palette-panel-terminal = Terminal
palette-panel-history = Historial de consultas
palette-panel-activity = Actividad del servidor
palette-new-tab = Nueva pestaña
palette-export-settings = Exportar ajustes
palette-import-settings = Importar ajustes
palette-duplicate-tab = Duplicar pestaña
palette-close-tab = Cerrar pestaña
palette-reopen-closed-tab = Reabrir pestaña cerrada
palette-reopen-closed-tab-hint = <pestaña>
palette-open-file = Abrir archivo
palette-open-file-hint = un script .sql
palette-save-file = Guardar archivo
palette-save-file-as = Guardar archivo como
palette-next-tab = Pestaña siguiente
palette-previous-tab = Pestaña anterior
palette-format-code = Formatear código
palette-format-statement = Formatear sentencia
palette-format-statement-hint = la sentencia en el cursor
palette-make-single-line = Pasar a una sola línea
palette-make-single-line-hint = la selección o la sentencia, en una línea
palette-run = Ejecutar
palette-run-hint = ejecutar todas las sentencias
palette-go-to-line = Ir a la línea
palette-go-to-line-hint = <número>
palette-go-to-statement = Ir a la sentencia
palette-search-hint = <búsqueda>
palette-history = Historial
palette-clear-history = Borrar historial
palette-clear-history-hint = conexión actual
palette-ask-ai = Preguntar a la IA
palette-ask-ai-hint = <instrucción>
palette-ask-ai-empty = Escribe una instrucción…
palette-terminal = Terminal
palette-terminal-hint = <comando>
palette-run-in-terminal = Ejecutar en el terminal
palette-terminal-empty = Escribe un comando…
palette-ui-theme = Tema de la interfaz
palette-ui-theme-hint = claro · oscuro
palette-editor-theme = Tema del editor
palette-editor-theme-hint = <tema>
palette-font-size = Tamaño de fuente
palette-increase-font-size = Aumentar tamaño de fuente
palette-decrease-font-size = Reducir tamaño de fuente
palette-indent-style = Estilo de sangría
palette-indent-style-hint = tabuladores · espacios
palette-indent-spaces = Espacios
palette-indent-tabs = Tabuladores
palette-indent-width = Ancho de sangría
palette-toggle-word-wrap = Alternar ajuste de línea
palette-toggle-whitespace = Alternar espacios en blanco
palette-toggle-whitespace-hint = mostrar espacios y tabuladores
palette-toggle-indent-guides = Alternar guías de sangría
palette-switch-connection = Cambiar de conexión
palette-switch-connection-hint = <conexión>
palette-server-variables = Variables del servidor
palette-server-variables-hint = los ajustes de la conexión activa
palette-enter-number = Introduce un número
palette-nothing-found = No se ha encontrado nada

## Command palette › what a result row is

palette-row-snippet = fragmento
palette-row-open-tab = pestaña abierta
palette-row-history = historial
palette-row-view-body = cuerpo de la vista
palette-row-body = cuerpo de { $kind }
palette-row-trigger-on = disparador en { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Abrir en la CLI
tree-open = Abrir
tree-open-in-new-tab = Abrir en una pestaña nueva
tree-copy-name = Copiar nombre
tree-copy-qualified-name = Copiar nombre completo
tree-properties = Propiedades
tree-used-by = Usado por
tree-live-monitor = Monitor en vivo
tree-er-diagram = Diagrama ER
tree-compare-with = Comparar con
tree-generate = Generar
tree-generate-ddl = Generar DDL
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Actualizar
tree-collapse-all = Contraer todo
tree-favourite = Marcar como favorito
tree-unfavourite = Quitar de favoritos
tree-colour = Color
tree-colour-none = Ninguno
tree-hide = Ocultar
tree-import = Importar
tree-export = Exportar
tree-back-up = Hacer copia de seguridad
tree-restore-from-file = Restaurar desde archivo
tree-copy-to = Copiar a
tree-create = Crear
tree-create-table = Tabla
tree-create-view = Vista
tree-create-type = Tipo
tree-create-domain = Dominio
tree-create-sequence = Secuencia
tree-create-synonym = Sinónimo
tree-create-alias-type = Tipo alias
tree-create-xml-schema-collection = Colección de esquemas XML
tree-create-function = Función
tree-create-procedure = Procedimiento
tree-create-event = Evento
tree-create-extension = Extensión
tree-create-database = Base de datos
tree-create-schema = Esquema
tree-create-kind = Crear { $kind }
tree-edit-kind = Editar { $kind }
tree-folder-create-extension = Crear extensión
tree-edit-table = Editar tabla
tree-edit-view = Editar vista
tree-edit-column = Editar columna
tree-edit-index = Editar índice
tree-edit-primary-key = Editar clave primaria
tree-edit-foreign-key = Editar clave foránea
tree-triggers = Disparadores
tree-triggers-count = Disparadores ({ $n })
tree-refresh-view = Actualizar vista
tree-truncate = Vaciar
tree-truncate-title = Vaciar tabla
tree-truncate-rows = { $n ->
    [one] ¿Eliminar la única fila de { $table }? No se puede deshacer.
   *[other] ¿Eliminar las { $rows } filas de { $table }? No se puede deshacer.
}
tree-truncate-every-row = ¿Eliminar todas las filas de { $table }? No se puede deshacer.
tree-drop = Eliminar
tree-drop-index = Eliminar índice
tree-drop-foreign-key = Eliminar clave foránea
tree-ai-explain = Explicar con IA
tree-users = Usuarios y privilegios
tree-server-variables = Variables del servidor
tree-new-database = Crear base de datos
tree-show-table-sizes = Mostrar tamaños de tabla

## Connection switcher

conn-menu-manage = Gestionar conexiones

## Shared dialog buttons

dialog-cancel = Cancelar
dialog-yes = Sí
dialog-no = No

## Open-transaction prompt

tx-prompt-title = Transacción abierta
tx-prompt-body = { $tab } tiene una transacción abierta con { $n ->
    [one] 1 sentencia
   *[other] { $n } sentencias
}. Puedes confirmarla para conservar los cambios o revertirla para descartarlos.
tx-prompt-body-aborted = { $tab } tiene una transacción que una sentencia fallida ha abortado, con { $n ->
    [one] 1 sentencia
   *[other] { $n } sentencias
}. Solo se puede revertir, lo que descarta los cambios.
tx-prompt-rollback = Revertir
tx-prompt-commit = Confirmar

## Error modal

error-modal-title = Error
error-modal-none = No hay ningún error.
error-modal-ai-fix = Corregir con IA
error-modal-explain = Explicar

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = la consulta ha fallado: { $error }
db-connection-failed = la conexión ha fallado: { $error }
db-query-cancelled = consulta cancelada

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Sin conexión: se ha rechazado el SQL de inicio de esta conexión: { $why }
db-refused-transport-entra = El inicio de sesión con Microsoft Entra requiere SSL / TLS en Verify full: su token abre todas las bases de datos de Azure SQL a las que llega la cuenta, así que el servidor debe demostrar su nombre antes de recibirlo.
db-refused-transport-aws = El inicio de sesión con AWS IAM requiere SSL / TLS como mínimo en modo Obligatorio: su token es una contraseña para este servidor y no debe cruzar la red sin cifrar.
db-refused-transport-gcp = El inicio de sesión con Google Cloud IAM requiere SSL / TLS en Verify CA, con la CA del servidor de la instancia como CA raíz: su token abre todas las instancias de Cloud SQL a las que llega la cuenta.
db-refused-transport = Este inicio de sesión requiere un modo SSL / TLS más estricto que el de la conexión.
db-refused-backup-in-terminal = La copia de seguridad de una base de datos de servidor se hace con sus propias herramientas, en el terminal.
db-refused-commit-aborted = la transacción ya estaba abortada: una de sus sentencias falló o se detuvo, así que confirmar descarta el trabajo en lugar de guardarlo. No se ha confirmado nada.
db-refused-sqlite-no-manual = Las conexiones SQLite aún no admiten el modo de transacción manual: las sentencias se ejecutan y se confirman según se envían
db-refused-mssql-read-only-auto = Una conexión de SQL Server de solo lectura funciona en modo Auto: SQL Server no tiene una transacción de solo lectura en la que mantener una pestaña Manual, y cada sentencia que ejecuta el modo Auto se revierte
db-refused-client-charset = rechazada: el juego de caracteres del cliente de la sesión ({ $charset }) puede ocultar una comilla dentro de un carácter, así que la comprobación de sentencias no puede leerla como la leería el servidor
db-refused-sql-mode = rechazada: el sql_mode del servidor ({ $mode }) interpreta las comillas de forma distinta a la comprobación de sentencias, y no se ha podido cambiar para esta sesión
db-refused-writing-plan = Aquí los planes se ejecutan en una transacción de solo lectura, y MySQL y MariaDB no planifican dentro de ella una sentencia que escribe. Planifica en su lugar la forma SELECT de la sentencia.
db-refused-writing-plan-function = Aquí los planes se ejecutan en una transacción de solo lectura, y MySQL y MariaDB no planifican dentro de ella una sentencia que escribe. Una función a la que llama escribe, algo que un plan no puede hacer aquí.
db-refused-windows-build-only = El inicio de sesión de Windows solo está disponible en la versión de Schemaic para Windows.
db-refused-cloud-iam-engine = El inicio de sesión con Cloud IAM es para conexiones PostgreSQL y MySQL.
db-refused-keyring-unreadable = Schemaic no pudo leer las credenciales guardadas de esta conexión del llavero del sistema ({ $error }). No se eliminó nada: desbloquee el llavero y vuelva a intentarlo.
db-refused-mssql-cancel-needs-kill = SQL Server no puede cancelar la sentencia de otra sesión sin finalizar la sesión: usa Finalizar sesión.
db-refused-shadowed-function = Rechazado antes de ejecutar tu sentencia: { $schema }.{ $name } es una función propia de esta base de datos con el nombre de una función integrada de PostgreSQL que las consultas de solo lectura pueden llamar, y PostgreSQL puede llamarla en su lugar, así que la comprobación de solo lectura no puede responder de lo que ejecutaría aquí una consulta. Quita { $schema } de la ruta de búsqueda de la conexión o pide al propietario de la base de datos que la elimine o le cambie el nombre.
db-refused-shadowed-operator = Rechazado antes de ejecutar tu sentencia: { $schema } contiene el operador { $name }, y PostgreSQL puede usarlo en lugar de uno integrado, así que la comprobación de solo lectura no puede responder de lo que ejecutaría aquí una consulta. Quita { $schema } de la ruta de búsqueda de la conexión o pide al propietario de la base de datos que lo elimine.
db-refused-blank-value = { $has_row ->
        [some] La fila { $row } tiene un valor vacío o en blanco para { $column } ({ $sql_type }): SQL Server lo guardaría como 0 o 1900-01-01 en lugar de rechazarlo. Importa los campos vacíos como NULL, o rellénalos.
       *[none] No se puede escribir un valor vacío o en blanco en { $column } ({ $sql_type }): SQL Server lo guardaría como 0 o 1900-01-01 en lugar de rechazarlo. Pon la celda a NULL, o escribe un valor.
    }
db-refused-code-page = { $has_row ->
        [some] La fila { $row } tiene un valor para { $column } ({ $sql_type }) que
       *[none] Un valor para { $column } ({ $sql_type })
    } contiene { $has_lost ->
        [some] `{ $character }`, para el que la página de códigos { $code_page } no tiene carácter
       *[none] un carácter que su página de códigos no tiene
    }: SQL Server lo guardaría como `?` o como un carácter parecido en lugar de rechazarlo. Guarda la columna como nvarchar, o quita el carácter.
db-refused-sqlite-no-file = no se ha indicado ningún archivo de base de datos
db-refused-sqlite-memory = :memory: no es un archivo de base de datos: SQLite conserva una base de datos en memoria solo mientras dure la conexión que la creó, y esta aplicación abre una conexión nueva por operación, así que cada escritura se daría por guardada y luego se descartaría. Indica la ruta de un archivo.
db-refused-sqlite-uri = { $file }: esto es un URI, no un archivo de base de datos: SQLite leería lo que va tras `?` como opciones de apertura, y `?mode=memory` abre una base de datos temporal que acepta todas las escrituras y no conserva ninguna. Indica la ruta del propio archivo.
db-refused-sqlite-worker = el proceso de SQLite ha fallado: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } no es una columna de esta tabla, así que no tiene valor predeterminado que escribir
db-refused-sqlite-generated = { $column } es una columna generada: no tiene valor predeterminado al que volver, y SQLite la calcula a partir de la fila
db-refused-sqlite-sizes-too-large = Los tamaños de tabla solo se leen para bases de datos de hasta { $cap }: SQLite los mide leyendo todas las páginas del archivo, y este ocupa { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = se ha agotado el tiempo de espera
db-connect-init-failed = el SQL de inicio `{ $statement }` ha fallado: { $error }
db-connect-session-retired = la conexión de esta pestaña se cerró tras una detención que el servidor no confirmó, y no se ha podido abrir otra; su transacción se ha revertido. Cambia la pestaña a Auto y de nuevo a Manual para abrir otra.
db-connect-begin-failed = no se ha podido iniciar la transacción: { $error }
db-connect-prepare-session = no se ha podido preparar la sesión: { $error }
db-connect-read-only-setup = no se ha podido poner la sesión en solo lectura: { $error }
db-connect-guard-session = no se ha podido proteger la sesión: { $error }
db-connect-plan-setup = no se ha podido preparar el plan: { $error }
db-connect-no-maintenance-db = no se puede acceder a ninguna base de datos de mantenimiento
db-connect-browser-silent = SQL Server Browser en { $server } no ha devuelto ningún puerto para la instancia { $instance }: el servicio Browser no está en marcha, el puerto UDP 1434 está bloqueado por el camino o allí no hay ninguna instancia con ese nombre. Si la instancia escucha en un puerto fijo, pon como host { $server } sin \{ $instance } y como puerto ese.
db-connect-browser-without-tcp = SQL Server Browser en { $server } ha respondido por la instancia { $instance } pero no ha devuelto ningún puerto TCP: la instancia no escucha por TCP. Activa TCP/IP para ella en el Administrador de configuración de SQL Server y reiníciala.
db-connect-azure-timed-out = Azure SQL no ha terminado de iniciar sesión en { $secs } segundos. Una base de datos sin servidor que se ha pausado tarda alrededor de un minuto en reanudarse, y este intento la ha despertado: vuelve a conectar en breve.
db-connect-azure-unavailable = { $server } Una base de datos sin servidor que se ha pausado tarda alrededor de un minuto en reanudarse, y este intento la ha despertado: vuelve a conectar en breve.
db-ssh-connect-no-answer = Conexión SSH con { $host }: sin respuesta en { $secs } s
db-ssh-auth-no-answer = Autenticación SSH con { $host }: sin respuesta en { $secs } s
db-ssh-auth-error = Error de autenticación SSH: { $error }
db-ssh-auth-failed = La autenticación SSH ha fallado
db-ssh-key-load = No se ha podido cargar la clave SSH: { $error }
db-ssh-key-auth = Error de autenticación con clave SSH: { $error }
db-ssh-agent = Error del agente SSH: { $error }
db-ssh-agent-empty = El agente SSH no tiene identidades cargadas (ejecuta `ssh-add`)
db-ssh-agent-auth = Error de autenticación del agente SSH: { $error }
db-ssh-no-agent = no se ha encontrado ningún agente SSH (inicia el servicio OpenSSH Authentication Agent o ejecuta Pageant)
db-ssh-no-agent-socket = no hay agente SSH ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = El host de salto SSH no ha podido llegar a { $host }: { $error }
db-ssh-connect-to = La conexión SSH con { $host } ha fallado: { $error }
db-ssh-connect = La conexión SSH ha fallado: { $error }
db-ssh-no-host = El túnel SSH no tiene host
db-ssh-bind = No se ha podido enlazar el túnel local: { $error }
db-ssh-named-instance = No se puede llegar a una instancia con nombre a través de un túnel SSH: SQL Server Browser responde con su puerto por UDP, que el túnel no transporta. Pon como host { $server } sin \{ $instance } y como puerto aquel en el que escucha la instancia.
db-ssh-host-key-changed = La clave de host SSH de { $host } ha CAMBIADO desde que Schemaic confió en ella por primera vez.
db-ssh-host-key-expected = Esperada  { $key }
db-ssh-host-key-offered = Ofrecida  { $key }
db-ssh-host-key-none = (no hay clave registrada)
db-ssh-host-key-changed-advice = Puede significar que el servidor se ha reinstalado o que su clave se ha rotado, o que algo está interceptando la conexión. No se le ha enviado nada. Verifica la nueva huella por otra vía (en la propia consola del servidor, o con quien lo administre) antes de confiar en ella. Una vez hecho, quita la entrada de este host de { $store } en el directorio de configuración de Schemaic para registrar la nueva clave.
db-ssh-host-key-store = Schemaic no ha podido leer su registro de claves de host SSH de confianza ({ $store }), así que no puede saber si la clave de { $host } ({ $key }) es la misma en la que confiaste antes.
db-ssh-host-key-store-advice = Se ha rechazado la conexión en lugar de confiar en la clave a primera vista. Repara o elimina { $store } en el directorio de configuración de Schemaic; eliminarlo hace que cada host SSH vuelva a ser de confianza en su siguiente conexión, así que hazlo solo si puedes verificar las huellas.
db-tls-setup = No se ha podido configurar TLS: { $error }
db-tls-client-cert = certificado de cliente rechazado: { $error }
db-tls-trust-store = el almacén de confianza de TLS no se puede usar: { $error }
db-tls-bad-name = { $name } no es un nombre que pueda llevar un certificado: { $error }
db-tls-ca-empty = El archivo de CA { $path } no contiene certificados
db-tls-ca-unusable = El archivo de CA { $path } contiene un certificado que no se puede usar como ancla de confianza
db-tls-cert-unreadable = no se puede leer el archivo de certificado { $path }: { $error }
db-tls-cert-not-pem = { $path } no es un archivo de certificado PEM: { $error }
db-tls-cert-is-der = { $path } no contiene ningún certificado PEM: un archivo DER debe convertirse primero (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = no se puede leer el archivo de clave { $path }: { $error }
db-tls-key-encrypted = { $path } es una clave privada protegida con frase de contraseña, que Schemaic no puede abrir. Descífrala primero: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } no es una clave privada PEM: { $error }
db-sign-in-no-value = { $field ->
        [host] La conexión no tiene host con el que iniciar sesión.
       *[user] La conexión no tiene usuario con el que iniciar sesión.
    }
db-sign-in-leading-dash = { $field ->
        [host] El host de la conexión empieza por «-», que la CLI de la nube leería como una opción y no como el host.
       *[user] El usuario de la conexión empieza por «-», que la CLI de la nube leería como una opción y no como el usuario.
    }
db-sign-in-control = { $field ->
        [host] El host de la conexión contiene un carácter de control.
       *[user] El usuario de la conexión contiene un carácter de control.
    }
db-sign-in-shim-aws = La AWS CLI encontrada es un shim .cmd (la versión 1 instalada con pip), y Schemaic no ejecuta shims a través de cmd.exe. Instala la versión 2 de la AWS CLI, o inicia sesión con una contraseña.
db-sign-in-shim-gcloud = El gcloud encontrado es un shim .cmd sin Python incluido a su lado, y Schemaic no ejecuta shims a través de cmd.exe. Reinstala la Google Cloud CLI con su instalador, o inicia sesión con una contraseña.
db-sign-in-shim-azure = La Azure CLI encontrada es un shim .cmd/.bat sin Python a su lado, y Schemaic no ejecuta shims a través de cmd.exe. Reinstala la Azure CLI con el instalador de Microsoft, o inicia sesión con un inicio de sesión SQL.
db-sign-in-no-token = La CLI de la nube ha respondido sin token.
db-sign-in-more-than-token = La CLI de la nube ha respondido con algo más que un token.
db-sign-in-azure-bad-answer = La respuesta de la Azure CLI no era el token que debía imprimir.
db-sign-in-azure-no-access-token = La Azure CLI ha respondido sin token de acceso.
db-sign-in-signed-out-aws = La AWS CLI no tiene credenciales. Ejecuta `aws configure` o `aws sso login` y vuelve a conectar.
db-sign-in-signed-out-gcloud = La Google Cloud CLI no tiene la sesión iniciada. Ejecuta `gcloud auth login` y vuelve a conectar.
db-sign-in-signed-out-azure = La Azure CLI no tiene la sesión iniciada. Ejecuta `az login` y vuelve a conectar.
db-sign-in-no-token-from = { $cli } no ha podido obtener un token: { $line }
db-sign-in-no-token-from-silent = { $cli } no ha podido obtener un token.
db-sign-in-no-token-from-azure-silent = La Azure CLI no ha podido obtener un token: no ha dado ningún motivo
db-sign-in-not-found = No se ha encontrado { $cli }. Instálala e inicia sesión con ella, o inicia sesión con una contraseña.
db-sign-in-not-found-azure = No se ha encontrado la Azure CLI (az). Instálala y ejecuta `az login`, o inicia sesión con un inicio de sesión SQL.
db-sign-in-no-answer = { $cli } no ha respondido en { $secs } segundos.
db-sign-in-not-started = No se ha podido iniciar { $cli }: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = la exportación ha dejado de leer
db-query-no-rows-to-export = esa sentencia no devuelve filas que exportar
db-query-no-server-settings = Este motor no tiene ajustes de servidor que mostrar.
db-query-no-status-list = Este motor no publica ninguna lista de estado, solo sus variables.
db-query-no-import-columns = No hay columnas en las que importar
db-query-read-stopped = La lectura se ha detenido: { $error }
db-query-batch-continued = SQL Server sigue ejecutando un lote después del error de una sentencia salvo que el error termine el lote, así que las sentencias posteriores a la fallida pueden haberse ejecutado.
db-query-sqlite-locked = En SQLite una lectura larga bloquea las escrituras: una exportación o importación de una tabla entera mantiene un bloqueo de lectura sobre el archivo hasta que termina, y ninguna escritura puede empezar hasta entonces (MySQL y PostgreSQL no funcionan así). Espera a que termine o cancélala y vuelve a intentarlo. Para que en este archivo las lecturas y escrituras puedan solaparse de forma permanente, cámbialo al modo de diario WAL —`PRAGMA journal_mode = WAL`—, lo que modifica el propio archivo, así que Schemaic no lo hará por ti.
db-query-rollback-failed-too = { $error }; además, la reversión también ha fallado: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = la sentencia { $n } ha fallado: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } sentencia anterior ya se ha aplicado y no se puede revertir
   *[other] { $failed } — { $n } sentencias anteriores ya se han aplicado y no se pueden revertir
}
ddl-cancelled = se ha cancelado el plan
ddl-dangling-foreign-key = el plan deja una clave foránea que no apunta a nada: { $row }
ddl-breaks-view = el plan rompería la vista { $view }, que lee una columna que el plan renombra o elimina ({ $why }). No se ha cambiado nada. Edita o elimina primero la vista.
ddl-no-databases = SQLite no tiene bases de datos que crear o eliminar: allí una base de datos es un archivo, y Schemaic no lo crea ni lo borra por ti.
ddl-elsewhere-failed = No se han actualizado las vistas de otras bases de datos que hacen select * from { $table } ({ $error }), y leerán y escribirán las columnas equivocadas hasta que se actualicen (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] No se ha podido actualizar { $count } vista de otra base de datos que hace select * from { $table }, y lee y escribe las columnas equivocadas hasta que se actualice (sp_refreshview): { $views }.
       *[other] No se han podido actualizar { $count } vistas de otra base de datos que hacen select * from { $table }, y leen y escriben las columnas equivocadas hasta que se actualicen (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] No se ha buscado en { $count } base de datos una vista que haga select * from { $table }: { $databases }.
       *[other] No se ha buscado en { $count } bases de datos una vista que haga select * from { $table }: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , y { $more } más
    }
ddl-rollback-failed = { $error }; además, la reversión también ha fallado: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = el borrado en { $table } ha afectado a { $affected } filas (se esperaba exactamente 1)
write-one-row-update = la actualización en { $table } ha afectado a { $affected } filas (se esperaba exactamente 1)
write-one-row-insert = la inserción en { $table } ha afectado a { $affected } filas (se esperaba exactamente 1)
write-batch-inserted = un lote de { $rows } filas ha insertado { $wrote }
write-batch-affected = un lote de { $rows } filas ha afectado a { $wrote }
write-row-issue = línea { $line }, columna { $column }: { $problem } ({ $text })
write-row-issue-empty = línea { $line }, columna { $column }: { $problem } (vacío)
write-copy-too-long = { $column } contiene más texto del que cabe en una lectura, así que la copia se ha detenido antes de escribirlo
write-copy-too-long-unnamed = una columna contiene más texto del que cabe en una lectura, así que la copia se ha detenido antes de escribirlo
write-copy-bytes = { $column } contiene bytes que la copia no puede leer tal cual, así que se ha detenido antes de escribirlos
write-copy-not-its-form = el valor leído para { $column } no tiene la forma que pidió la copia, así que no se ha escrito nada
write-copy-not-its-form-unnamed = el valor leído para una columna no tiene la forma que pidió la copia, así que no se ha escrito nada
write-commit-cancelled = Confirmación cancelada
write-import-cancelled = Importación cancelada
write-rollback-complete = {" "}— se han revertido todos los cambios
write-rollback-incomplete = {" "}— la reversión NO los ha deshecho: el motor de almacenamiento de esta tabla no es transaccional, así que las filas ya escritas permanecen. Revisa la tabla antes de reintentarlo.
write-rollback-unknown = {" "}— y no se ha podido confirmar la reversión: la conexión falló antes de que respondiera el servidor. Puede que queden filas o no. Revisa la tabla antes de reintentarlo.

## Schema tree › the panel

tree-panel-title = ESQUEMA
tree-tip-databases = Mostrar u ocultar bases de datos
tree-tip-options = Opciones del esquema
tree-search-placeholder = Buscar…
tree-loading = Cargando
tree-no-tables = No hay tablas
tree-failed-to-connect = No se ha podido conectar
tree-failed-view = Ver
tree-single-user-unread = Monousuario: sin leer
tree-single-user-read = Leer
tree-count-tables = { $n ->
    [one] { $n } tabla
   *[other] { $n } tablas
}
tree-count-cols = { $n ->
    [one] { $n } col.
   *[other] { $n } cols.
}
tree-count-keys = { $n ->
    [one] { $n } clave
   *[other] { $n } claves
}
tree-count-partitions = { $n ->
    [one] { $n } partición
   *[other] { $n } particiones
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = tipo
kind-domain = dominio
kind-sequence = secuencia
kind-function = función
kind-procedure = procedimiento
kind-event = evento
kind-synonym = sinónimo
kind-alias-type = tipo alias
kind-xml-schema-collection = colección de esquemas XML
kind-extension = extensión
kind-group-type = Tipos
kind-group-domain = Dominios
kind-group-sequence = Secuencias
kind-group-function = Funciones
kind-group-procedure = Procedimientos
kind-group-event = Eventos
kind-group-synonym = Sinónimos
kind-group-alias-type = Tipos alias
kind-group-xml-schema-collection = Colecciones de esquemas XML
kind-group-extension = Extensiones

## Query history panel

history-title = HISTORIAL DE CONSULTAS
history-clear-tip = Borrar el historial de esta conexión…
history-clear-title = Borrar historial de consultas
history-clear-message = ¿Eliminar { $n } { $n ->
    [one] consulta registrada
   *[other] consultas registradas
} de esta conexión? No se puede deshacer.
history-scope-current = Actual
history-scope-all = Todas
history-cut-note = Se muestran las { $shown } más recientes de { $matched }: busca para acotar.
history-empty = Aún no hay consultas.
history-no-match = No hay consultas que coincidan.
history-open-in-new-tab = Abrir en una pestaña nueva
history-delete = Eliminar
history-failed = Fallida

## Snippet library panel

snippets-title = BIBLIOTECA DE FRAGMENTOS
snippets-save-tip = Guardar el SQL del editor como fragmento
snippets-no-match = Ningún fragmento coincide.
snippets-band-this-connection = ESTA CONEXIÓN
snippets-band-all-connections = TODAS LAS CONEXIONES
snippets-builtin = Integrado
snippets-name-placeholder = Nombre del fragmento
snippets-insert = Insertar en el editor
snippets-open-in-new-tab = Abrir en una pestaña nueva
snippets-edit = Editar
snippets-duplicate = Duplicar
snippets-delete = Eliminar
snippets-show-in = Mostrar en
snippets-scope-this-connection = Esta conexión
snippets-scope-engine = Todas las conexiones { $engine }
snippets-scope-all-connections = Todas las conexiones

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = HOY
history-band-this-week = ESTA SEMANA
history-band-earlier = ANTES

## Query tab strip

tabs-tip-unsaved = { $path }: cambios sin guardar
tabs-pin = Fijar
tabs-unpin = Desfijar
tabs-rename = Renombrar
tabs-duplicate = Duplicar
tabs-close = Cerrar
tabs-open-file = Abrir archivo
tabs-save = Guardar
tabs-save-as = Guardar como
tabs-reload = Recargar desde el disco
tabs-reopen-last = Reabrir la última pestaña
tabs-close-others = Cerrar las demás pestañas
tabs-close-all = Cerrar todas las pestañas

## Server Activity panel

activity-title = ACTIVIDAD DEL SERVIDOR
activity-refresh-tip = Actualizar ahora
activity-auto-refresh-off = Actualización automática desactivada
activity-auto-refresh-every = Actualización automática cada { $secs } s
activity-interval-off = Desactivada
activity-interval = { $secs } s
activity-age-seconds = { $s } s
activity-age-minutes = { $m } min { $s } s
activity-age-hours = { $h } h { $m } min
activity-age-days = { $d } d { $h } h
activity-view-sessions = Sesiones
activity-view-locks = Bloqueos
activity-search-sessions = Buscar sesiones…
activity-search-locks = Buscar bloqueos…
activity-loading = Cargando…
activity-sqlite-no-sessions = SQLite no tiene sesiones de servidor: se ejecuta dentro de este proceso, así que no hay nada más conectado que ver.
activity-sessions-no-match = No hay sesiones que coincidan.
activity-sessions-none = No hay otras sesiones en este servidor.
activity-sessions-undrawn = { $n ->
    [one] Coincide { $n } sesión más: acota la búsqueda para verla.
   *[other] Coinciden { $n } sesiones más: acota la búsqueda para verlas.
}
activity-locks-no-match = No hay bloqueos que coincidan.
activity-locks-no-waits = No hay esperas de bloqueo en este servidor.
activity-locks-none = Ninguna otra sesión tiene un bloqueo.
activity-locks-contended-only = Este servidor solo enumera los bloqueos por los que se espera, o que provocan una espera, no todos los bloqueos que tiene una sesión.
activity-locks-undrawn = { $n ->
    [one] Coincide { $n } bloqueo más: acota la búsqueda para verlo.
   *[other] Coinciden { $n } bloqueos más: acota la búsqueda para verlos.
}
activity-list-capped = lista limitada
activity-locks-total = { $n ->
    [one] { $shown } bloqueo
   *[other] { $shown } bloqueos
}
activity-locks-waiting = { $n ->
    [one] { $shown } en espera
   *[other] { $shown } en espera
}
activity-state-blocked = Bloqueada
activity-state-idle-in-tx = Inactiva en transacción
activity-state-running = En ejecución
activity-state-idle = Inactiva
activity-count-sessions = { $n ->
    [one] { $figure } sesión
   *[other] { $figure } sesiones
}
activity-count-running = { $figure } en ejecución
activity-count-blocked = { $figure } bloqueadas
activity-count-idle-in-tx = { $figure } inactivas en transacción
activity-lock-wait = Espera de bloqueo
activity-lock-wait-sentence = La sesión { $waiter } ({ $waiter_who }) { $waited ->
        [none] está esperando
       *[other] lleva { $waited } esperando
    } un bloqueo que tiene la sesión { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] inactiva en una transacción
        [blocked] bloqueada a su vez
        [running] en ejecución
       *[idle] inactiva
    }{ $held ->
        [none] {""}
       *[other] , que lo retiene desde hace { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}Otra sesión también tiene un bloqueo que necesita.
       *[other] {" "}Otras { $others } sesiones también tienen un bloqueo que necesita.
    }
activity-kill-holder = Finalizar { $id }
activity-terminate-tip = Finalizar la sesión { $id }
activity-read-only-tip = Esta conexión está marcada como de solo lectura.
activity-lock-holds = tiene
activity-lock-waits-for = espera
activity-lock-held-by = lo tiene { $ids }
activity-copy-statement = Copiar sentencia
activity-cancel-query = Cancelar consulta
activity-kill-session = Finalizar sesión
activity-waiting-on = esperando un bloqueo que tiene { $ids }
activity-blocks = { $n ->
        [one] bloquea { $n } sesión
       *[other] bloquea { $n } sesiones
    }
activity-read-only-cancel = Esta conexión está marcada como de solo lectura, así que Schemaic no cancelará consultas en ella. Desactiva primero la opción de solo lectura en los ajustes de la conexión.
activity-read-only-terminate = Esta conexión está marcada como de solo lectura, así que Schemaic no finalizará sesiones en ella. Desactiva primero la opción de solo lectura en los ajustes de la conexión.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ASISTENTE DE IA
ai-new-chat-tip = Nuevo chat
ai-settings-tip = Ajustes de IA
ai-empty-ask = Pregunta sobre tu SQL…
ai-empty-not-connected = { $cli } no está conectado.
ai-input-placeholder = Mensaje…
ai-input-off = El asistente de IA está desactivado
ai-input-db-down = Sin conexión con la base de datos
ai-speaker-you = TÚ
ai-result-more-lines = … (+{ $n } líneas más)

## Shared dialog buttons (continued)

dialog-close = Cerrar

## Chart modal

chart-title = Gráfico — { $tab }
chart-caption-kind = Gráfico
chart-caption-values = Valores
chart-caption-labels = Etiquetas
chart-row-number = Número de fila
chart-kind-bar = Barras
chart-kind-line = Líneas
chart-kind-histogram = Histograma
chart-save-tip = Guardar el gráfico…
chart-save-dialog = Guardar gráfico
chart-file-stem = Gráfico de { $name }
chart-nothing-to-save = No hay nada que guardar.
chart-no-numbers = En este resultado no hay ninguna columna numérica que representar.
chart-all-null = Todos los valores de esa columna son NULL.
chart-too-large = Estos valores son demasiado grandes para dibujarlos.
chart-caption-rows = { $n ->
    [one] { $n } fila
   *[other] { $n } filas
}
chart-caption-skipped-null = { $skipped } NULL, sin dibujar
chart-caption-skipped = { $skipped } sin dibujar (NULL, o más allá de { $max })
chart-caption-truncated = las filas leídas, no el resultado completo

## Schema editors — shared

editor-preview-sql = Vista previa del SQL
editor-change-count = { $n ->
    [0] Sin cambios
    [one] 1 cambio
   *[other] { $n } cambios
}

## View editor

view-editor-title-edit = Editar la vista { $view }
view-problem-no-name = La vista necesita un nombre.
view-problem-no-select = Una vista necesita un SELECT que la defina.
view-problem-not-a-query = El cuerpo de una vista tiene que ser una consulta: empieza por SELECT, WITH, VALUES o TABLE.
view-problem-materialized = Schemaic no puede editar una vista materializada: PostgreSQL no tiene CREATE OR REPLACE para ellas.
view-problem-hidden = La definición de esta vista está cifrada u oculta para este inicio de sesión, así que aquí se puede eliminar pero no editar.
view-problem-verbatim = La cabecera de esta vista dice algo que Schemaic no puede reformular, así que aquí se puede eliminar pero no editar.
view-editor-title-create = Crear vista en { $location }
view-editor-section-view = Vista
view-editor-section-definition = Definición
view-editor-section-options = Opciones
view-editor-name = Nombre
view-editor-select = Sentencia SELECT
view-editor-check-option = Opción CHECK
view-editor-column-names = Nombres de columna
view-editor-column-names-placeholder = opcional: si se deja vacío, se toman del SELECT
view-editor-sql-security = Seguridad SQL
view-editor-definer = Definidor
view-editor-recreate = Recrear en lugar de reemplazar
view-editor-recreate-hint = PostgreSQL solo puede añadir columnas a una vista existente. Renombrar, cambiar de tipo o reordenar una requiere un DROP y un CREATE, que se llevan por delante las vistas dependientes y los permisos concedidos.

## Table properties panel

props-title = Propiedades — { $table }
props-reading = Leyendo estadísticas
props-no-stats-view = Una vista no tiene almacenamiento propio, así que el servidor no da estadísticas sobre ella.
props-no-stats-table = El servidor no ha dado estadísticas para esta tabla.
props-on-disk = en disco
props-rows-estimated = filas (estimadas)
props-rows-counted = filas (contadas)
props-rows-not-reported = filas: sin datos
props-counting = Contando
props-count-rows = Contar filas
props-count-slow = Un recorrido completo de la tabla. Lento en una tabla grande.
props-storage-data = Datos
props-storage-indexes = Índices
props-storage-free = Libre
props-section-structure = Estructura
props-columns = Columnas
props-indexes = Índices
props-foreign-keys = Claves foráneas
props-count-columns = { $n ->
    [one] { $n } columna
   *[other] { $n } columnas
}
props-count-indexes = { $n ->
    [one] { $n } índice
   *[other] { $n } índices
}
props-count-keys = { $n ->
    [one] { $n } clave
   *[other] { $n } claves
}
props-type = Tipo
props-type-view = Vista
props-type-table = Tabla
props-section-options = Opciones
props-engine = Motor
props-row-format = Formato de fila
props-collation = Intercalación
props-next-auto-increment = Siguiente autoincremento
props-created = Creada
props-updated = Actualizada
props-comment = Comentario
props-section-partitioning = Particionado
props-partition-of = Partición de
props-partition-bound = Límite
props-partitioned-by = Particionada por
props-partitions-more = { $n ->
    [one] y { $n } partición más
   *[other] y { $n } particiones más
}
props-section-indexes = Índices
props-index-cardinality = cardinalidad { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } recorrido
       *[other] { $shown } recorridos
    }
props-index-not-counted = uso no contabilizado
props-index-unused = sin usar desde que se reiniciaron los contadores del servidor
props-never-analysed = Esta tabla nunca se ha analizado, así que el servidor no tiene una estimación de filas para ella. Ejecuta ANALYZE o cuenta las filas.
props-analysed-at = Estimado según el último ANALYZE, { $when }.
props-read-live = Leído en vivo: este servidor tiene information_schema_stats_expiry = 0.
props-cached-up-to = En caché del servidor: estas cifras pueden tener hasta { $age } de antigüedad (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } s
props-age-minutes = { $n } min
props-age-hours = { $n } h
props-age-days = { $n } d
props-vacuum-note = El { $pct } % de las versiones de fila de esta tabla están muertas y aún sin recuperar. Un VACUUM devolvería ese espacio.
props-copy = Copiar

## Shared panel search box

panel-search-placeholder = Buscar…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = ahora mismo
ago-minutes = hace { $n } min
ago-hours = hace { $n } h
ago-days = hace { $n } d
ago-weeks = hace { $n } sem.

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } fila
   *[other] { $n } filas
}
outcome-rows-capped = { $n }+ filas

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = Abrir la CLI de la base de datos
term-db-cli-tip-read-only = Abrir la CLI de la base de datos; esta conexión está marcada como de solo lectura
term-restart-tip = Reiniciar el terminal
term-settings-tip = Ajustes del terminal

## Results panel header

results-title = RESULTADOS
results-properties-tip = Propiedades de la tabla…
results-monitor-tip = Monitor en vivo…
results-restore-editor-tip = Restaurar el editor
results-collapse-editor-tip = Contraer el editor

## Query panel header

query-title = CONSULTA
query-no-database = Sin base de datos

## Shared dialog buttons (continued)

dialog-stop = Detener

## Copy table modal

copy-table-title = Copiar { $table }
copy-table-rows-line = Después, copiar en ella todas las filas de { $table } (no se muestra como SQL)
copy-table-into = Copiar en
copy-table-schema = Esquema
copy-table-name = Nombre
copy-table-back = ‹ atrás
copy-table-data = Copiar datos
copy-table-data-hint = Copia las filas una vez creada la tabla. Si se desactiva, solo se copia la estructura.
copy-table-what-with-rows = La estructura y las filas. Las claves foráneas y los disparadores no se copian, y las columnas generadas se vuelven a calcular.
copy-table-what-structure = Solo la estructura. Las claves foráneas y los disparadores no se copian.
copy-table-choose-database = Elige la base de datos en la que copiar.
copy-table-unnamed = Ponle un nombre a la copia.
copy-table-onto-itself = Una tabla no se puede copiar sobre sí misma: dale otro nombre a la copia.
copy-table-connection-changed = La conexión activa ha cambiado: vuelve a abrir Copiar a desde la tabla.
copy-table-schema-not-loaded = El esquema de { $database } no está cargado: despliégalo en el árbol y vuelve a intentarlo.
copy-table-preview-subject = Copia de { $table }
copy-table-copying = Copiando filas… { $n }
copy-table-copied = { $n ->
    [one] Se ha copiado { $n } fila.
   *[other] Se han copiado { $n } filas.
}
copy-table-stopped = Se ha detenido antes de copiar todas las filas. La tabla se ha creado.

## Database, schema and extension editor

database-editor-title-database = Crear base de datos
database-problem-no-name = El nombre no puede estar vacío.
database-problem-bad-collation = Un nombre de intercalación solo contiene letras, dígitos y guiones bajos.
database-editor-title-schema = Crear esquema
database-editor-title-extension = Crear extensión
database-editor-title-schema-in = Crear esquema en { $database }
database-editor-title-extension-in = Crear extensión en { $database }
database-editor-section-database = Base de datos
database-editor-section-schema = Esquema
database-editor-section-extension = Extensión
database-editor-name = Nombre
database-editor-charset = Juego de caracteres
database-editor-collation = Intercalación
database-editor-owner = Propietario
database-editor-server-default = predeterminado del servidor
database-editor-owner-you = tú
database-editor-no-extensions = No quedan extensiones por instalar
database-editor-no-charsets = No hay juegos de caracteres que sugerir
database-editor-no-collations = No hay intercalaciones que sugerir
database-editor-no-roles = No hay roles en este servidor

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Importar en { $database }
script-dialog-title = Ejecutar un script SQL
script-section = Script SQL
script-choose-file = Elegir un archivo…
script-no-file = No se ha elegido ningún archivo
script-reading = Leyendo el archivo
script-hint = El script puede crear y seleccionar una base de datos distinta de aquella en la que importas. También puede crear más de una base de datos. Revisa el script para asegurarte de que hace lo que esperas.
script-hint-namespace = El script puede crear y seleccionar una base de datos distinta de aquella en la que importas. También puede crear más de una base de datos. Abrir esto desde { $namespace } tampoco coloca el script en él: todo lo que el archivo no califique va adonde diga el search_path del servidor. Revisa el script para asegurarte de que hace lo que esperas.
script-what-it-does = Qué hace este archivo
script-probe-nothing = Este archivo no contiene ninguna sentencia que Schemaic pueda ejecutar.
script-probe-cut-off = Los primeros { $size } de este archivo son una única sentencia sin terminar, así que todavía no hay nada que resumir. Al ejecutarlo se envía el archivo completo.
script-probe-counted = { $n ->
    [one] { $count } sentencia leída de los primeros { $size }.
   *[other] { $count } sentencias leídas de los primeros { $size }.
}
script-destroys = { $n ->
    [one] { $count } sentencia de este archivo destruye datos
   *[other] { $count } sentencias de este archivo destruyen datos
} (DROP, TRUNCATE o DELETE). No se puede deshacer desde aquí.
script-unread = Solo se han leído los primeros { $size } de este archivo, y nada en ellos destruye datos, pero el resto no se ha examinado. Puede contener DROP, TRUNCATE o DELETE, y eso no se puede deshacer desde aquí.
script-unqualified = { $n ->
    [one] { $count } sentencia de este archivo actúa
   *[other] { $count } sentencias de este archivo actúan
} sobre todas las filas de una tabla: un DELETE, UPDATE o TRUNCATE sin WHERE. No se puede deshacer desde aquí.
script-tx-own = Este archivo abre su propia transacción, así que se aplica entero o no se aplica. Schemaic no añade ninguna propia.
script-tx-none = Este archivo no abre ninguna transacción, así que un fallo a mitad deja en su sitio lo que se ejecutó antes.
script-connecting = Conectando
script-progress-of = { $done } de { $total }
script-progress-read = { $done } leídos
script-run = Ejecutar
script-done = { $n ->
    [one] Se ha ejecutado { $n } sentencia de { $file }.
   *[other] Se han ejecutado { $n } sentencias de { $file }.
}
script-stopped = { $n ->
    [one] Detenido tras { $n } sentencia.
   *[other] Detenido tras { $n } sentencias.
}
script-failed = Error: { $message }. Antes se { $n ->
    [one] ejecutó { $n } sentencia.
   *[other] ejecutaron { $n } sentencias.
}
script-failed-at = Error en la línea { $line } de { $file }: { $message }. Antes se { $n ->
    [one] ejecutó { $n } sentencia.
   *[other] ejecutaron { $n } sentencias.
}
script-durability-rolled-back = { $file } abrió su propia transacción, así que el servidor revirtió toda la ejecución: no se ha aplicado nada, ni siquiera las sentencias que tuvieron éxito.
script-durability-applied = { $file } no abrió ninguna transacción, así que todas las sentencias que se ejecutaron siguen aplicadas.
script-durability-applied-before = Las sentencias que se ejecutaron antes de que { $file } abriera su propia transacción siguen aplicadas; lo que se ejecutó dentro de la transacción se ha revertido.
script-durability-unknown = Las sentencias que ya se ejecutaron siguen aplicadas, salvo que { $file } abriera su propia transacción.

## Schema comparison modal

compare-title = Comparar esquemas
compare-choose-database = Elige una base de datos
compare-no-connection = No hay ninguna conexión de este motor con la que comparar.
compare-will-change = se modificará
compare-source-of-truth = referencia
compare-idle = Elige una base de datos con la que comparar.
compare-loading = Leyendo ambos esquemas
compare-select-object = Selecciona un objeto para ver ambos lados.
compare-fk-cycle-note = Las claves foráneas de estas tablas forman un ciclo: ningún orden de creación las satisface todas, así que una migración generada puede rechazarse a mitad.
compare-same-but-stored-differently = Coinciden en estructura: la diferencia de abajo está en cómo guarda el motor la sentencia, y no hay nada que migrar.
compare-summary = { $differ } distintos · { $only_here } solo aquí · { $only_there } solo allí · { $same } idénticos
compare-summary-uncertain = { $n ->
    [one] { $n } coincidencia
   *[other] { $n } coincidencias
} que la comparación no puede garantizar
compare-filter-placeholder = Filtrar por nombre
compare-include-identical = Incluir idénticos
compare-select-all = Seleccionar todo
compare-select-none = Ninguno
compare-hint-held-back = excluido del plan; la nota de arriba explica por qué
compare-described = { $kind } { $name }
compare-omitted = { $what }: { $why }
compare-omitted-needs-source = hay que volver a leer su cuerpo del servidor antes de poder aplicarlo
compare-omitted-unread = esta comparación no tiene ninguna sentencia para ello: no se ha podido leer su definición
compare-omitted-no-statement = esta comparación no tiene ninguna sentencia para la diferencia que ha encontrado
compare-held-partition = es una { $sub ->
        [yes] subpartición
       *[no] partición
    } de { $parent }, que está excluida: recréalas juntas, a mano
compare-held-partitioning-differs = su particionado es distinto, y esta comparación no escribe ninguna sentencia que reparticione una tabla: recréala a mano
compare-held-sequence-unwritten = esta comparación no escribe ninguna sentencia para una secuencia: cópiala de `SHOW CREATE SEQUENCE`, que muestra su panel
compare-held-partitioned-unrestated = está particionada, y ningún CREATE TABLE de aquí reformula su esquema de partición: crea a mano la función y el esquema de partición y, después, la tabla
compare-held-sequence-range = el contador del destino ha llegado a { $at }, fuera del nuevo rango de { $min } a { $max }, y el servidor se niega a estrechar una secuencia por debajo de un valor que ya ha entregado. Reiníciala en el destino (ALTER SEQUENCE … RESTART WITH) en un valor que ninguna tabla contenga y vuelve a comparar
compare-note-set-aside = { $aside }: el { $side ->
        [target] destino
       *[source] origen
    } también contiene { $kept }, y los nombres del destino no distinguen mayúsculas de minúsculas, así que solo puede contener uno de los dos; se compara { $kept }
compare-note-binding = Tipo alias { $alias }: { $bound ->
        [rule] la regla { $rule } vinculada a él
        [default] el valor predeterminado { $default } vinculado a él
       *[both] la regla { $rule } y el valor predeterminado { $default } vinculados a él
    } en el origen no se comparan ni se trasladan: vincúlalos a mano (sp_bindrule / sp_bindefault).
compare-hint-blocked = hay que volver a leer el cuerpo antes de poder aplicar esto
compare-hint-uncertain = un índice que este modelo solo lee en parte: aquí la coincidencia no es segura
compare-preview-migration = Vista previa de la migración
compare-plan-subject = { $n ->
    [one] { $n } objeto en { $database }
   *[other] { $n } objetos en { $database }
}
compare-selected = { $n ->
        [0] No hay nada seleccionado.
        [one] { $n } objeto seleccionado
       *[other] { $n } objetos seleccionados
    }
compare-incomparable = { $left } y { $right } no se pueden comparar. Los nombres de tipo, los valores predeterminados y la forma de los índices no se trasladan entre motores, así que cualquier migración generada a partir de la diferencia sería errónea.
compare-group-table = Tablas
compare-group-view = Vistas
compare-group-trigger = Disparadores
compare-kind-enum = enum
compare-kind-domain = dominio
compare-kind-xml-schema-collection = colección de esquemas XML
compare-kind-alias-type = tipo alias
compare-kind-sequence = secuencia
compare-kind-table = tabla
compare-kind-view = vista
compare-kind-function = función
compare-kind-procedure = procedimiento
compare-kind-trigger = disparador
compare-kind-event = evento
compare-kind-synonym = sinónimo

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Ninguna de las dos bases de datos contiene nada que comparar.
compare-empty-agrees = Estos dos esquemas coinciden, objeto por objeto.
compare-empty-agrees-noted = Todos los objetos coinciden, pero no se ha podido comparar todo: consulta las notas de arriba.
compare-empty-no-match = Nada coincide con ese filtro.
compare-empty-only-identical = Todo lo que coincide con ese filtro es idéntico: activa Incluir idénticos para verlo.

## Schema editors — shared (continued)

editor-comment-placeholder = qué hace

## Event editor

event-editor-title-edit = Editar el evento { $event }
event-editor-title-create = Crear evento en { $location }
event-editor-section-event = Evento
event-editor-section-schedule = Programación
event-editor-section-body = Cuerpo
event-editor-section-options = Opciones
event-editor-name = Nombre
event-editor-body = Cuerpo
event-editor-runs = Se ejecuta
event-editor-runs-repeating = Periódicamente
event-editor-runs-once = Una vez
event-editor-at = A las
event-editor-every = Cada
event-editor-starts = Empieza
event-editor-ends = Termina
event-editor-status = Estado
event-editor-status-enabled = Activado
event-editor-status-disabled = Desactivado
event-editor-status-replica = Desactivado en la réplica
event-editor-preserve = Conservar al completarse
event-editor-preserve-hint = ON COMPLETION PRESERVE. Desactivado es el valor predeterminado de MySQL y significa que el servidor elimina el evento en cuanto pasa su última ejecución, lo que en un evento puntual es justo después de ejecutarse.
event-editor-definer = Definidor
event-editor-comment = Comentario
event-editor-reading-source = Leyendo el código fuente del evento…
event-editor-source-stale = El código fuente real del evento llegó después de que empezaras a escribir, así que este cuerpo se basa en la copia del catálogo, que no es lo que tiene el servidor. Cierra y vuelve a abrir para editarlo.
event-problem-no-name = El evento necesita un nombre.
event-problem-no-time = Un evento puntual necesita una hora a la que ejecutarse.
event-problem-no-interval = Un evento periódico necesita un intervalo.
event-problem-no-unit = Un evento periódico necesita una unidad de intervalo (DAY, HOUR…).
event-problem-no-body = El evento necesita un cuerpo.
event-name-clash = Ya hay aquí un evento llamado { $name }.

## Routine editor

routine-editor-title-edit-function = Editar la función { $routine }
routine-editor-title-edit-procedure = Editar el procedimiento { $routine }
routine-editor-title-create-function = Crear función en { $location }
routine-editor-title-create-procedure = Crear procedimiento en { $location }
routine-editor-section-function = Función
routine-editor-section-procedure = Procedimiento
routine-editor-section-body = Cuerpo
routine-editor-section-options = Opciones
routine-editor-name = Nombre
routine-editor-parameters = Parámetros
routine-editor-returns = Devuelve
routine-editor-returns-placeholder-mssql = int, o TABLE
routine-editor-language = Lenguaje
routine-editor-body = Cuerpo
routine-editor-recompile = Recompilar
routine-editor-recompile-hint = WITH RECOMPILE: un plan nuevo en cada llamada, nunca uno en caché.
routine-editor-schema-binding = Vinculación al esquema
routine-editor-schema-binding-hint = WITH SCHEMABINDING: las tablas y vistas que lee no se pueden cambiar sin que se entere.
routine-editor-execute-as = Ejecutar como
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER o un usuario
routine-editor-null-input = Devuelve NULL si recibe NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT: el cuerpo no se ejecuta si un argumento es NULL.
routine-editor-inlining = Inserción en línea
routine-editor-inlining-automatic = Automática
routine-editor-comment = Comentario
routine-editor-volatility = Volatilidad
routine-editor-strict = Estricta
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Se ejecuta con los derechos del propietario en lugar de los de quien la llama. Fija un search_path abajo si usas esto.
routine-editor-security-definer-hint-mysql = Se ejecuta con los derechos del definidor en lugar de los de quien la llama. Es el valor predeterminado de MySQL; desactivarlo equivale a SQL SECURITY INVOKER.
routine-editor-settings = Ajustes
routine-editor-add-setting = Añadir ajuste
routine-editor-deterministic = Determinista
routine-editor-deterministic-hint = Garantiza el mismo resultado para los mismos argumentos. Un servidor con registro binario rechaza una rutina no determinista salvo que confíe en sus creadores.
routine-editor-data-access = Acceso a datos
routine-editor-definer = Definidor
routine-editor-reading-source = Leyendo el código fuente de la rutina…
routine-editor-source-stale = El código fuente real de la rutina llegó después de que empezaras a escribir, así que este cuerpo se basa en la copia del catálogo, que no es lo que tiene el servidor. Cierra y vuelve a abrir para editarlo.
routine-problem-function-no-name = La función necesita un nombre.
routine-problem-procedure-no-name = El procedimiento necesita un nombre.
routine-problem-function-no-language = La función necesita un lenguaje (plpgsql, sql).
routine-problem-procedure-no-language = El procedimiento necesita un lenguaje (plpgsql, sql).
routine-problem-no-return-type = La función necesita un tipo de retorno.
routine-problem-procedure-returns = Un procedimiento no devuelve nada: borra el tipo de retorno.
routine-problem-function-no-body = La función necesita un cuerpo.
routine-problem-procedure-no-body = El procedimiento necesita un cuerpo.
routine-problem-bare-parameter = SQL Server nombra cada parámetro con @: @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Un procedimiento no admite WITH { $option }.
routine-problem-scalar-refuses = Una función escalar no admite WITH { $option }.
routine-problem-inline-refuses = Una función con valores de tabla en línea no admite WITH { $option }.
routine-problem-multi-refuses = Una función con valores de tabla de varias instrucciones no admite WITH { $option }.
routine-problem-procedure-refuses-native = Un procedimiento no admite WITH { $option } una vez compilado de forma nativa.
routine-problem-scalar-refuses-native = Una función escalar no admite WITH { $option } una vez compilada de forma nativa.
routine-problem-inline-refuses-native = Una función con valores de tabla en línea no admite WITH { $option } una vez compilada de forma nativa.
routine-problem-multi-refuses-native = Una función con valores de tabla de varias instrucciones no admite WITH { $option } una vez compilada de forma nativa.
routine-problem-native-needs-schemabinding = Una rutina compilada de forma nativa necesita WITH SCHEMABINDING: SQL Server exige ambas cosas juntas.
routine-problem-inline-with-execute-as = INLINE = ON no se puede combinar con EXECUTE AS: una función que se ejecuta como otra persona nunca se inserta en línea.
routine-problem-numbered-rename = { $was } encabeza un grupo de procedimientos numerados ({ $group }), y renombrarlo los eliminaría todos. Conserva el nombre para editarlo.
routine-problem-trigger-function-args = Una función de disparador no recibe argumentos declarados: llegan en TG_ARGV.
routine-clash-function-refused = Ya hay aquí una función llamada { $name }. En este motor, renombrar es eliminar y crear, así que el servidor rechaza esta y no cambia nada: elige otro nombre o elimina primero { $name }.
routine-clash-procedure-refused = Ya hay aquí un procedimiento llamado { $name }. En este motor, renombrar es eliminar y crear, así que el servidor rechaza este y no cambia nada: elige otro nombre o elimina primero { $name }.
routine-clash-function-destroys = Ya hay aquí una función llamada { $name }. En este motor, renombrar es eliminar y crear, así que aplicar esto destruiría { $original } y dejaría { $name } como estaba.
routine-clash-procedure-destroys = Ya hay aquí un procedimiento llamado { $name }. En este motor, renombrar es eliminar y crear, así que aplicar esto destruiría { $original } y dejaría { $name } como estaba.
routine-clash-function-destroys-new = Ya hay aquí una función llamada { $name }. En este motor, renombrar es eliminar y crear, así que aplicar esto destruiría el original y dejaría { $name } como estaba.
routine-clash-procedure-destroys-new = Ya hay aquí un procedimiento llamado { $name }. En este motor, renombrar es eliminar y crear, así que aplicar esto destruiría el original y dejaría { $name } como estaba.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Buscar cuentas
users-loading-accounts = Cargando cuentas
users-unsupported = El motor de esta conexión no tiene cuentas de usuario.
users-none = Este servidor no informa de ninguna cuenta.
users-no-match = Ninguna cuenta coincide con «{ $needle }».
users-pick-account = Elige una cuenta para ver qué puede hacer.
users-section-attributes = Atributos
users-section-privileges = Privilegios
users-attr-authentication = Autenticación
users-attr-locked = Bloqueada
users-attr-password-expired = Contraseña caducada
users-attr-superuser = Superusuario
users-attr-create-role = Crear roles
users-attr-create-database = Crear bases de datos
users-attr-replication = Replicación
users-attr-bypass-rls = Omitir RLS
users-attr-inherit = Heredar
users-attr-connection-limit = Límite de conexiones
users-attr-valid-until = Válido hasta
users-attr-type = Tipo
users-attr-disabled = Desactivado
users-attr-default-database = Base de datos predeterminada
users-attr-server-roles = Roles de servidor
users-attr-login = Inicio de sesión
users-attr-default-schema = Esquema predeterminado
users-attr-member-of = Miembro de
users-attr-yes = Sí
users-attr-no = No
users-login-type-sql = Inicio de sesión SQL
users-login-type-windows = Inicio de sesión de Windows
users-login-type-windows-group = Grupo de Windows
users-kind-user = Usuario
users-kind-role = Rol
users-kind-login = Inicio de sesión
users-maintained-by-server = gestionado por el servidor
users-loading-privileges = Cargando privilegios
users-no-privileges = Esta cuenta no tiene privilegios.
users-note-my-own-account = Esta conexión no puede leer mysql.user, así que esta no es la lista de cuentas del servidor: es la única cuenta que ve esta conexión, la suya. Conéctate con una cuenta que tenga SELECT sobre la base de datos mysql para ver el resto.
users-note-ms-logins-elsewhere = Azure SQL Database guarda sus inicios de sesión en master, y no se gestionan aquí: se muestran los usuarios y roles de la base de datos.
users-note-ms-logins-only = Solo se muestran los inicios de sesión del servidor: elige una base de datos para ver sus usuarios y roles.
users-note-ms-short = Este inicio de sesión no tiene VIEW ANY DEFINITION, así que el servidor solo le muestra las entidades de seguridad que puede ver: puede que la lista esté incompleta.
users-note-my-scope = Los privilegios obtenidos a través de un rol concedido no se desglosan: SHOW GRANTS enumera lo concedido directamente a la cuenta y los roles de los que es miembro, pero no lo que tienen esos roles. Abre los privilegios del propio rol para verlos.
users-note-pg-scope = Los privilegios sobre esquemas, tablas y secuencias son los de { $database }: PostgreSQL los guarda en el catálogo propio de cada base de datos, así que no se muestran los de otras bases de datos. Tampoco se muestran EXECUTE sobre funciones y procedimientos ni los privilegios concedidos sobre una sola columna. Los privilegios obtenidos por pertenencia a un rol o concedidos a PUBLIC no se desglosan, como tampoco los que tiene un rol por ser propietario de un objeto o superusuario.
users-note-pg-no-database = No se muestran los privilegios sobre esquemas, tablas y secuencias: PostgreSQL los guarda en el catálogo propio de cada base de datos, y esta conexión no tiene ninguna base de datos seleccionada. Abre una pestaña de consulta sobre una base de datos para verlos. Tampoco se muestran EXECUTE sobre funciones y procedimientos ni los privilegios concedidos sobre una sola columna. Los privilegios obtenidos por pertenencia a un rol o concedidos a PUBLIC no se desglosan, como tampoco los que tiene un rol por ser propietario de un objeto o superusuario.
users-note-pg-superuser = Este rol es SUPERUSER: se salta todas las comprobaciones de permisos, así que tiene todos los privilegios sobre todos los objetos, diga lo que diga la lista de abajo.
users-note-pg-predefined = { $role } es uno de los roles predefinidos de PostgreSQL: lo que puede hacer viene integrado en el servidor en lugar de concederse, así que no figura en ningún catálogo de los que se leen aquí.
users-note-pg-owns = { $n ->
    [one] Este rol es propietario de { $n } objeto en { $database } (esquemas, tablas, secuencias o la propia base de datos). Un propietario tiene todos los privilegios sobre lo que posee, y PostgreSQL no registra ninguna concesión por ello, así que nada de eso aparece abajo.
   *[other] Este rol es propietario de { $n } objetos en { $database } (esquemas, tablas, secuencias o la propia base de datos). Un propietario tiene todos los privilegios sobre lo que posee, y PostgreSQL no registra ninguna concesión por ello, así que nada de eso aparece abajo.
}
users-note-pg-owns-cluster = { $n ->
    [one] Este rol es propietario de { $n } objeto en este clúster (esquemas, tablas, secuencias o la propia base de datos). Un propietario tiene todos los privilegios sobre lo que posee, y PostgreSQL no registra ninguna concesión por ello, así que nada de eso aparece abajo.
   *[other] Este rol es propietario de { $n } objetos en este clúster (esquemas, tablas, secuencias o la propia base de datos). Un propietario tiene todos los privilegios sobre lo que posee, y PostgreSQL no registra ninguna concesión por ello, así que nada de eso aparece abajo.
}
users-note-ms-unshown = { $n ->
    [one] Aquí no se muestra { $n } permiso, sobre { $classes }: el servidor lo indica sobre un elemento protegible que esta lista no puede nombrar.
   *[other] Aquí no se muestran { $n } permisos, sobre { $classes }: el servidor los indica sobre elementos protegibles que esta lista no puede nombrar.
}
users-note-ms-pick-database = Los permisos de un usuario de base de datos son los de esa base de datos: elige una para verlos.
users-statements-capped = Se muestran las primeras { $shown } de { $total } sentencias. Copiar privilegios las copia todas.
users-new-account = Nueva cuenta
users-system-account = Esta cuenta pertenece al servidor, así que Schemaic no la modificará.
users-reset-password = Restablecer contraseña
users-gate-no-database = Abre una pestaña de consulta sobre una base de datos para cambiar privilegios: la sentencia tiene que ejecutarse en una.
users-gate-read-only = Esta conexión es de solo lectura.
users-count = { $n ->
    [one] { $n } cuenta
   *[other] { $n } cuentas
}
users-count-filtered = { $shown } de { $n } cuentas
users-copy-privileges = Copiar privilegios

## Schema editors — shared (continued)

editor-row-remove = Quitar

## Trigger editor

trigger-editor-title = Disparadores en { $table }
trigger-problem-no-name = El disparador necesita un nombre.
trigger-problem-no-table = Un disparador necesita una tabla sobre la que dispararse.
trigger-problem-no-event = Un disparador necesita al menos un evento con el que dispararse.
trigger-problem-pg-no-function = Un disparador de PostgreSQL necesita una función que ejecutar.
trigger-problem-pg-body = Un disparador de PostgreSQL ejecuta una función, no un cuerpo: elige o escribe una que ejecutar.
trigger-problem-truncate-row-level = Un disparador TRUNCATE tiene que ser FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Un disparador INSTEAD OF tiene que ser FOR EACH ROW.
trigger-problem-instead-of-view-only = Solo una vista puede tener un disparador INSTEAD OF: una tabla admite BEFORE o AFTER.
trigger-problem-pg-view-row-timing = Un disparador BEFORE o AFTER de una vista tiene que ser FOR EACH STATEMENT: usa INSTEAD OF para actuar sobre filas.
trigger-problem-update-of-without-update = UPDATE OF nombra columnas en un disparador UPDATE.
trigger-problem-pg-instead-of-columns = Un disparador INSTEAD OF no admite lista de columnas: PostgreSQL lo dispara con cualquier UPDATE de la vista.
trigger-problem-one-event = { $engine } dispara un disparador con un solo evento: crea un disparador distinto por evento.
trigger-problem-no-truncate = { $engine } no tiene disparadores TRUNCATE.
trigger-problem-sqlite-view-timing = El disparador de una vista tiene que ser INSTEAD OF: SQLite no tiene disparadores BEFORE ni AFTER sobre vistas.
trigger-problem-sqlite-row-only = SQLite solo tiene disparadores FOR EACH ROW: no los hay a nivel de sentencia.
trigger-problem-no-body = Un disparador de { $engine } necesita un cuerpo.
trigger-problem-sqlite-not-block = El cuerpo de un disparador de SQLite tiene que ser un bloque BEGIN … END con al menos una sentencia.
trigger-problem-runs-body = Un disparador de { $engine } ejecuta un cuerpo, no una función: escribe las sentencias que ejecutar.
trigger-problem-ms-no-before = SQL Server no tiene disparadores BEFORE: usa INSTEAD OF para actuar antes de la escritura.
trigger-problem-ms-view-timing = En SQL Server, el disparador de una vista tiene que ser INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server dispara un disparador una vez por sentencia: lee las filas de inserted y deleted.
trigger-problem-ms-instead-of-rank = Un disparador INSTEAD OF no puede tener orden de activación: devuelve cada evento a cualquier orden.
trigger-problem-first-without-event = Se dispara el primero con { $event }, y no es un disparador de ese evento.
trigger-problem-last-without-event = Se dispara el último con { $event }, y no es un disparador de ese evento.
trigger-problem-no-when = { $engine } no tiene condición WHEN: pon la comprobación dentro del cuerpo con IF.
trigger-problem-ms-no-update-of = SQL Server no tiene UPDATE OF: comprueba UPDATE(columna) dentro del cuerpo.
trigger-problem-my-no-view = MySQL no admite disparadores sobre vistas.
trigger-problem-my-no-instead-of = MySQL no tiene disparadores INSTEAD OF: son de PostgreSQL.
trigger-problem-my-no-update-of = MySQL no tiene UPDATE OF: un disparador ve todas las columnas.
trigger-problem-constraint-trigger = Schemaic no puede editar el disparador de restricción { $name }: no modela los ajustes de aplazamiento que lleva.
trigger-problem-not-editable = Schemaic no puede editar el disparador { $name }: su definición está cifrada, oculta para este inicio de sesión o usa una opción que Schemaic no modela. Cámbialo en SQL.
trigger-problem-twice = Hay dos disparadores llamados { $name }.
trigger-problem-first-clash = { $n ->
    [2] Solo un disparador puede dispararse el primero con { $event }, y tanto { $names } como { $last } están configurados así.
   *[other] Solo un disparador puede dispararse el primero con { $event }, y { $names } y { $last } están todos configurados así.
}
trigger-problem-last-clash = { $n ->
    [2] Solo un disparador puede dispararse el último con { $event }, y tanto { $names } como { $last } están configurados así.
   *[other] Solo un disparador puede dispararse el último con { $event }, y { $names } y { $last } están todos configurados así.
}
trigger-editor-preview-subject = disparadores en { $table }
trigger-editor-none-selected = No hay ningún disparador seleccionado.
trigger-editor-readonly-constraint = Este es un disparador de restricción. Schemaic no modela los ajustes de aplazamiento que lleva, así que aquí se puede eliminar pero no editar: cámbialo en SQL.
trigger-editor-readonly-hidden = La definición de este disparador está cifrada u oculta para este inicio de sesión, así que aquí se puede eliminar pero no editar.
trigger-editor-readonly-unmodelled = Este disparador usa una opción que Schemaic no modela, así que aquí se puede eliminar pero no editar: cámbialo en SQL.
trigger-editor-section-trigger = Disparador
trigger-editor-section-condition = Condición
trigger-editor-section-action = Acción
trigger-editor-name = Nombre
trigger-editor-timing = Momento
trigger-editor-event = Evento
trigger-editor-fires = Se dispara
trigger-editor-truncate-hint = Solo a nivel de sentencia.
trigger-editor-not-for-replication-hint = No actúa mientras un agente de replicación escribe en la tabla.
trigger-editor-rank-any = Cualquier orden
trigger-editor-rank-first = Se dispara el primero
trigger-editor-rank-last = Se dispara el último
trigger-editor-when = Cuando
trigger-editor-of-columns = De las columnas
trigger-editor-of-columns-placeholder = opcional: todas las columnas si se deja vacío
trigger-editor-body = Cuerpo
trigger-editor-function = Función
trigger-editor-edit-function = Editar
trigger-editor-new-function = Nueva función
trigger-editor-arguments = Argumentos
trigger-editor-add-argument = Añadir argumento

## Schema editors — shared (continued)

editor-row-move-up = Subir
editor-row-move-down = Bajar

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Editar { $kind } { $name }
object-editor-title-create = Crear { $kind } en { $location }
object-editor-name = Nombre
object-editor-comment = Comentario
object-editor-type = Tipo
object-editor-default = Valor predeterminado
object-editor-section-type = Tipo
object-editor-section-values = Valores
object-editor-section-domain = Dominio
object-editor-section-constraints = Restricciones
object-editor-section-sequence = Secuencia
object-editor-section-position = Posición
object-editor-section-synonym = Sinónimo
object-editor-section-alias-type = Tipo alias
object-editor-section-xml-schema-collection = Colección de esquemas XML
object-editor-section-stands-for = Representa a
object-editor-value-placeholder = valor
object-editor-add-value = Añadir valor
object-editor-values-order = En orden de comparación: es el orden que usa ORDER BY
object-editor-add-constraint = Añadir restricción
object-editor-no-base-types = Este motor no sugiere tipos base
object-editor-not-null-hint = Todas las columnas de este dominio rechazan NULL. Activarlo falla si alguna ya contiene uno.
object-editor-stores = Almacena
object-editor-increment = Incremento
object-editor-start = Inicio
object-editor-cache = Caché
object-editor-minimum = Mínimo
object-editor-maximum = Máximo
object-editor-not-whole-number = { $field } tiene que ser un número entero.
object-problem-type-no-name = El tipo necesita un nombre.
object-problem-duplicate-value = El valor { $value } aparece más de una vez.
object-problem-empty-value = Un valor no puede estar vacío: PostgreSQL nunca podrá quitarlo.
object-problem-domain-no-name = El dominio necesita un nombre.
object-problem-domain-no-base = Un dominio necesita un tipo en el que basarse.
object-problem-constraint-no-name = Todas las restricciones necesitan un nombre.
object-problem-constraint-no-predicate = La restricción { $name } no tiene predicado.
object-problem-constraint-twice = El nombre de restricción { $name } aparece dos veces.
object-problem-sequence-no-name = La secuencia necesita un nombre.
object-problem-zero-increment = Una secuencia no puede incrementarse en 0.
object-problem-min-above-max = El mínimo ({ $min }) es mayor que el máximo ({ $max }).
object-problem-start-outside = El valor inicial ({ $start }) está fuera de { $min }..{ $max }.
object-problem-cache-below-one = La caché tiene que ser al menos 1.
object-problem-does-not-fit = { $min }..{ $max } no cabe en { $data_type }.
object-problem-restart-outside = Reiniciar en { $restart } queda fuera de { $min }..{ $max }.
object-problem-counter-outside = La secuencia está en { $last }, fuera del nuevo { $min }..{ $max }. Dale un valor de reinicio dentro del rango.
object-problem-no-name = El nombre no puede estar vacío.
object-problem-tsql-no-type = Una secuencia necesita un tipo.
object-problem-start-not-whole = El inicio tiene que ser un número entero.
object-problem-increment-not-whole = El incremento tiene que ser un número entero.
object-problem-minimum-not-whole = El mínimo tiene que ser un número entero.
object-problem-maximum-not-whole = El máximo tiene que ser un número entero.
object-problem-tsql-zero-increment = El incremento no puede ser 0.
object-problem-min-not-below-max = El mínimo tiene que ser menor que el máximo.
object-problem-started-outside = Empezó en { $start }, fuera del nuevo rango: reiníciala en un valor entre el mínimo y el máximo.
object-problem-tsql-start-outside = El inicio tiene que estar entre el mínimo y el máximo.
object-problem-cache-not-positive = El tamaño de caché tiene que ser un número entero mayor que 0.
object-problem-restart-not-whole = El valor de reinicio tiene que ser un número entero.
object-problem-tsql-restart-outside = El valor de reinicio tiene que estar entre el mínimo y el máximo.
object-problem-synonym-no-target = Un sinónimo necesita el objeto al que representa.
object-problem-alias-no-base = Un tipo alias necesita un tipo base.
object-editor-cycle = Cíclica
object-editor-cycle-hint = Vuelve al otro extremo del rango en lugar de fallar cuando la secuencia se agota.
object-editor-restart-at = Reiniciar en (el último valor entregado por el contador fue { $last })
object-editor-restart-at-unused = Reiniciar en (el contador no se ha usado)
object-editor-restart-not-whole = Reiniciar en tiene que ser un número entero.
object-editor-restart-placeholder = déjalo vacío para conservar la posición
object-editor-tsql-restart-placeholder = vacío la conserva, salvo que cambie el tipo
object-editor-owned-by = Propiedad de
object-editor-detach = Desvincular de su columna
object-editor-detach-hint = La secuencia deja de eliminarse junto con la columna, y le sobrevive.
object-editor-cache-hint = Entrega valores desde la memoria, más rápido, a costa de un hueco tras un reinicio del servidor.
object-editor-cache-size = Tamaño de caché
object-editor-cache-size-placeholder = predeterminado del servidor
object-editor-synonym-server = Servidor
object-editor-synonym-server-placeholder = este servidor
object-editor-synonym-database = Base de datos
object-editor-synonym-database-placeholder = esta base de datos
object-editor-synonym-schema = Esquema
object-editor-synonym-schema-placeholder = el esquema predeterminado
object-editor-synonym-object = Objeto
object-editor-synonym-object-placeholder = tabla, vista o rutina
object-editor-base-type = Tipo base
object-editor-nullable = Admite NULL
object-editor-nullable-hint = Una columna de este tipo admite NULL salvo que la columna indique lo contrario.
object-editor-schemas = Esquemas

## Account and grant editors (Users and privileges)

account-editor-title-create = Crear cuenta
account-editor-title-reset = Restablecer contraseña
account-editor-title-privileges = Privilegios — { $account }
account-editor-account = Cuenta
account-editor-kind = Clase
account-editor-name = Nombre
account-editor-host = Host
account-editor-host-placeholder = % (cualquier host)
account-editor-for-login = Para el inicio de sesión
account-editor-for-login-placeholder = inicio de sesión: vacío para un usuario sin él
account-editor-also-user = También usuario en { $database }
account-editor-password = Contraseña
account-editor-confirm = Confirmar
account-editor-action = Acción
account-editor-action-grant = Conceder
account-editor-action-revoke = Revocar
account-editor-subject = Objeto
account-editor-subject-privileges = Privilegios
account-editor-subject-role = Rol
account-editor-role = Rol
account-editor-no-roles = Este servidor no tiene roles
account-editor-with-admin-option = Con opción de administración
account-editor-level = Nivel
account-editor-level-global = Todo el servidor
account-editor-level-database = Base de datos
account-editor-level-schema = Esquema
account-editor-level-table = Tabla
account-editor-level-sequence = Secuencia
account-editor-no-suggestions = No hay sugerencias
account-editor-section-privileges = Privilegios
account-editor-with-grant-option = Con opción de concesión
account-editor-needs-role = Indica un rol.
account-editor-needs-privileges = Elige un nivel, indica a qué se aplica y marca un privilegio.
account-editor-password-required = Se necesita una contraseña.
account-editor-name-required = Se necesita un nombre.
account-editor-entra-only-user = Con Microsoft Entra solo se crean usuarios de base de datos.
account-editor-entra-no-password = Un usuario de Entra no tiene aquí inicio de sesión ni contraseña: lo autentica Entra.
account-editor-login-needs-password = Un inicio de sesión necesita una contraseña.
account-editor-user-for-login-password = Un usuario para un inicio de sesión entra con la contraseña de ese inicio de sesión: deja esto en blanco.
account-editor-confirm-password = Vuelve a escribir la contraseña para confirmarla.
account-editor-passwords-differ = Las dos contraseñas no coinciden.
account-editor-password-hashed = El SQL de la vista previa, que es la sentencia que se ejecuta, lleva un hash de la contraseña en la forma en que la guarda este servidor, no la contraseña en sí, salvo que tenga caracteres que el inicio de sesión normalizaría (letras de ancho completo, ligaduras, espacios de no separación) o que el servidor use una extensión de comprobación de contraseñas; en esos casos se envía tal como se escribió.
account-editor-password-as-typed = La contraseña aparece en el SQL de la vista previa, que es la sentencia que se ejecuta.

## Table designer

designer-title-edit = Editar { $table }
designer-title-create = Crear tabla en { $location }
designer-tab-table = Tabla
designer-tab-columns = Columnas
designer-tab-indexes = Índices
designer-tab-foreign-keys = Claves foráneas
designer-tab-checks = Comprobaciones
designer-list-add = Añadir
designer-no-suggestions = No hay sugerencias
designer-name = Nombre
designer-comment = Comentario
designer-table-comment-placeholder = Para qué sirve esta tabla
designer-engine = Motor
designer-collation = Intercalación
designer-no-column = No hay ninguna columna seleccionada.
designer-type = Tipo
designer-type-hint = Se escribe tal cual: el servidor decide qué significa.
designer-nullable = Admite NULL
designer-nullable-hint = Permite NULL en esta columna.
designer-primary-key = Clave primaria
designer-primary-key-hint = Forma parte de la clave primaria de la tabla, en el orden de las columnas.
designer-identity = Identidad
designer-auto-increment = Autoincremento
designer-identity-hint = El servidor asigna el valor ({ $keyword }).
designer-default = Valor predeterminado
designer-default-hint = Texto SQL, tal como lo escribirías: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Generada a partir de
designer-generated-hint = Una expresión, p. ej. qty * price. Una columna generada no admite valor predeterminado.
designer-on-update = Al actualizar
designer-index-unique-detail = único
designer-index-redundant-detail = redundante
designer-hint-redundant = El índice { $index } está cubierto por { $cover }: toda búsqueda que atiende la atiende también ese otro.
table-problem-no-name = La tabla necesita un nombre.
table-problem-no-columns = Una tabla necesita al menos una columna.
table-problem-column-no-name = Todas las columnas necesitan un nombre.
table-problem-column-twice = Hay dos columnas llamadas { $name }.
table-problem-column-no-type = La columna { $name } necesita un tipo.
table-problem-key-not-column = La clave primaria nombra { $column }, que no es una columna.
table-problem-index-no-name = Todos los índices necesitan un nombre.
table-problem-index-twice = Hay dos índices llamados { $name }.
table-problem-index-no-columns = El índice { $index } no tiene columnas.
table-problem-xml-one-column = El índice { $index } debe nombrar exactamente una columna.
table-problem-xml-no-options = El índice { $index } no puede ser único, incluir columnas ni tener filtro: un índice XML o espacial no admite nada de eso.
table-problem-xml-no-using = El índice { $index } necesita su cláusula USING XML INDEX … FOR …: un índice XML secundario se construye sobre uno primario o selectivo.
table-problem-selective-no-paths = El índice { $index } necesita sus rutas FOR (…): un índice XML selectivo son las rutas que promueve.
table-problem-xml-parent-gone = El índice { $index } se construye sobre { $parent }, que no es un índice XML primario ni selectivo de esta tabla: desaparece cuando desaparece su índice padre.
table-problem-not-orderable = El índice { $index } no se puede ordenar: solo un índice de almacén de columnas tiene ORDER.
table-problem-order-not-column = El índice { $index } se ordena por { $column }, que no es una columna.
table-problem-order-not-stored = El índice { $index } se ordena por { $column }, que no almacena.
table-problem-columnstore-unique = El índice { $index } no puede ser único: un índice de almacén de columnas almacena columnas, no claves.
table-problem-columnstore-include = El índice { $index } no puede incluir columnas: un índice de almacén de columnas enumera todas las columnas que almacena.
table-problem-columnstore-sorted = El índice { $index } no puede ordenar sus columnas: una columna de almacén de columnas no tiene dirección.
table-problem-clustered-columnstore-filtered = El índice { $index } no puede tener filtro: un índice de almacén de columnas agrupado almacena todas las filas.
table-problem-clustered-include = El índice { $index } no puede incluir columnas: un índice agrupado ya contiene todas las columnas.
table-problem-unique-include = El índice { $index } no puede incluir columnas: aquí una restricción UNIQUE no tiene INCLUDE.
table-problem-clustered-filtered = El índice { $index } no puede tener filtro: un índice agrupado cubre todas las filas.
table-problem-constraint-filtered = El índice { $index } no puede tener filtro: respalda una restricción UNIQUE, que aquí no admite condición. Elimínalo y añade un índice único con la condición.
table-problem-not-partial = El índice { $index } no puede tener filtro: aquí este índice no puede ser parcial.
table-problem-index-not-column = El índice { $index } nombra { $column }, que no es una columna.
table-problem-include-not-column = El índice { $index } incluye { $column }, que no es una columna.
table-problem-cluster-clash = Una tabla solo admite un índice agrupado, y { $clustered } la agrupan cada uno: deja todos menos uno como no agrupados.
table-problem-cluster-key = la clave primaria
table-problem-cluster-index = el índice { $index }
table-problem-cluster-and = y
table-problem-fk-no-name = Todas las claves foráneas necesitan un nombre.
table-problem-fk-twice = Hay dos claves foráneas llamadas { $name }.
table-problem-fk-no-table = La clave foránea { $fk } no hace referencia a ninguna tabla.
table-problem-fk-unpaired = La clave foránea { $fk } debe emparejar cada columna con una a la que haga referencia.
table-problem-fk-not-column = La clave foránea { $fk } nombra { $column }, que no es una columna.
table-problem-check-no-name = Todas las restricciones CHECK necesitan un nombre.
table-problem-check-twice = Hay dos restricciones CHECK llamadas { $name }.
table-problem-check-no-predicate = La comprobación { $name } no tiene predicado.
table-problem-without-rowid-no-key = Una tabla WITHOUT ROWID necesita una clave primaria.
table-problem-strict-type = Las tablas STRICT solo admiten INT, INTEGER, REAL, TEXT, BLOB o ANY, y { $column } es { $data_type }.
designer-hint-unindexed-fk = La clave foránea { $fk } no tiene ningún índice que empiece por sus columnas, así que un borrado o una actualización de clave en la tabla a la que hace referencia recorre esta.
designer-no-index = No hay ningún índice seleccionado. La clave primaria se define en las columnas.
designer-columns = Columnas
designer-index-columns-hint-single = La única columna xml, geometry o geography que indexa.
designer-index-columns-hint-columnstore = Columnas que almacena, separadas por comas.
designer-index-columns-hint-prefix = Separadas por comas, en el orden de la clave. bio(20) es una longitud de prefijo; añade DESC para ordenar de forma descendente.
designer-index-columns-hint = Separadas por comas, en el orden de la clave. Añade DESC para una columna descendente.
designer-index-method = Método
designer-index-method-hint = Déjalo vacío para usar el predeterminado (btree).
designer-index-filter = Solo las filas donde
designer-index-filter-hint = La condición de un índice parcial, sin el WHERE.
designer-index-storage = Almacenamiento
designer-storage-rowstore = Almacén de filas
designer-storage-columnstore = Almacén de columnas
designer-storage-clustered-columnstore = Almacén de columnas agrupado
designer-storage-primary-xml = XML primario
designer-storage-selective-xml = XML selectivo
designer-storage-secondary-xml = XML secundario
designer-storage-spatial = Espacial
designer-index-using = Using
designer-index-using-hint = La cláusula que sigue a la columna: el USING XML INDEX … FOR PATH, VALUE o PROPERTY de un índice XML secundario (o FOR (ruta) en uno selectivo), el [WITH XMLNAMESPACES (…)] FOR (nombre = '/ruta' [AS …], …) de un índice XML selectivo, o el USING … WITH (…) de un índice espacial.
designer-index-order = Orden
designer-index-order-hint = Columnas, separadas por comas, por las que se ordenan los segmentos: desde SQL Server 2022, y desde 2025 para un índice no agrupado, que solo ordena por columnas que almacena.
designer-index-include = Incluir
designer-index-include-hint = Columnas, separadas por comas, que lleva el índice sin formar parte de su clave.
designer-index-unique = Único
designer-index-unique-hint = Rechaza valores duplicados en estas columnas.
designer-fk-detail-unindexed = → { $table } · sin índice
designer-no-fk = No hay ninguna clave foránea seleccionada.
designer-fk-ref-table = Tabla referenciada
designer-fk-on-delete = Al eliminar
designer-fk-columns-hint = Separadas por comas, emparejadas en orden con las columnas referenciadas.
designer-fk-ref-columns = Columnas referenciadas
designer-no-check = No hay ninguna comprobación seleccionada.
designer-check-expression = Expresión
designer-check-expression-hint = La condición que debe cumplir cada fila. Se escribe sin CHECK (…).
designer-check-enforced = Aplicada
designer-check-enforced-hint = Si se desactiva, la restricción se registra sin aplicarse: se aceptan tanto las filas existentes como las nuevas.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Gestionar conexiones
connections-new = Nueva conexión
connections-import = Importar conexiones
connections-export = Exportar conexiones
connections-duplicate = Duplicar
connections-delete = Eliminar
connections-test = Probar
connections-save = Guardar
connections-test-failed = La conexión ha fallado
connections-browse = Examinar…
connections-section-general = General
connections-section-safeguards = Protecciones
connections-section-database = Base de datos
connections-section-network = Red
connections-name = Nombre
connections-colour = Color
connections-folder = Carpeta
connections-no-folders = Aún no hay carpetas
connections-folder-hint = Opcional. Las conexiones con la misma carpeta se muestran juntas.
connections-prominent-colour = Color destacado en el editor
connections-prominent-colour-hint = Enmarca el editor de consultas con el color de esta conexión.
connections-read-only = Solo lectura
connections-read-only-hint = Desactiva la edición de celdas y bloquea las consultas de escritura y DDL en esta conexión.
connections-read-only-mssql = SQL Server no tiene sesiones de solo lectura, así que las lecturas se ejecutan en una transacción que Schemaic revierte. El efecto de un procedimiento fuera de la base de datos sobrevive a eso; para tener garantías, conéctate con un inicio de sesión que solo pueda leer.
connections-cli-access = Acceso desde la CLI
connections-cli-access-hint = Permite que la línea de comandos schemaic, y los agentes de IA a través de schemaic mcp, usen esta conexión.
connections-ai-data-access = Acceso de la IA a los datos
connections-statement-timeout = Tiempo de espera de la sentencia
connections-statement-timeout-hint = Cancela en esta conexión una sentencia que tarde más.
connections-statement-timeout-same = Igual que en la aplicación
connections-environment = Entorno
env-none = Ninguno
env-local = Local
env-development = Desarrollo
env-testing = Pruebas
env-staging = Preproducción
env-production = Producción
connections-type = Tipo
connections-init-sql = SQL de inicio
connections-init-sql-pragma-hint = Opcional. Ajustes PRAGMA que se ejecutan en cada conexión que abre Schemaic.
connections-init-sql-set-hint = Opcional. Sentencias SET que se ejecutan en cada conexión que abre Schemaic: un search path, una zona horaria, un rol.
connections-init-refused = «{ $statement }»: { $why }
connections-init-only-pragmas = al conectar solo se pueden ejecutar ajustes PRAGMA
connections-init-only-sets = al conectar solo se pueden ejecutar sentencias SET
connections-init-calls-function = al conectar, un ajuste no puede llamar a una función
connections-init-quoted-name = al conectar, el nombre de un ajuste no puede ir entre comillas
connections-init-names-no-setting = eso no nombra ningún ajuste
connections-init-not-listed = { $name } no se puede establecer al conectar: no es uno de los ajustes de sesión que Schemaic sabe que no afectan a sus protecciones
connections-init-not-utf8 = el juego de caracteres del cliente debe seguir siendo UTF-8: la comprobación de solo lectura de Schemaic lee cada sentencia como UTF-8
connections-init-one-value = al conectar, un ajuste admite un único valor, y no puede ir nada detrás
connections-init-one-value-after-equals = al conectar, un ajuste admite un único valor, después de un =
connections-init-transaction = los ajustes de transacción no se pueden establecer al conectar: los establece la protección de solo lectura
connections-init-pragma = ese PRAGMA no se puede ejecutar al conectar: cambia el archivo o la protección de solo lectura
connections-init-server-scope = los ajustes { $scope } cambian el servidor, no la sesión
connections-init-variable = una variable no es un ajuste de sesión: termina con su lote
connections-init-on-only = { $name } solo se puede activar al conectar: Schemaic depende de ello
connections-host = Host
connections-port = Puerto
connections-named-instance-port = Una instancia con nombre: SQL Server Browser proporciona su puerto, así que Puerto no se usa. Para prescindir de Browser, pon el host sin la instancia y el puerto en el que escucha.
connections-database = Base de datos
connections-database-hint = Opcional; es la base de datos en la que se abre la conexión. Algunos proveedores alojados solo permiten la suya y rechazan una conexión que no la nombre.
connections-authentication = Autenticación
ssh-auth-password = Contraseña
ssh-auth-key-pair = Par de claves
ssh-auth-agent = Agente SSH
auth-password = Usuario y contraseña
auth-windows = Windows (esta cuenta)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Usuario
connections-password = Contraseña
connections-azure-cli-hint = Inicia sesión como quien tenga la sesión iniciada en la Azure CLI (ejecuta antes `az login`). El token se obtiene cuando una conexión lo necesita y nunca se guarda.
connections-aws-iam-hint = Inicia sesión como el usuario de abajo con un token obtenido de las credenciales de la AWS CLI (antes, `aws configure` o `aws sso login`). El usuario necesita el rol rds_iam (PostgreSQL) o el AWSAuthenticationPlugin (MySQL). El token se obtiene cuando una conexión lo necesita y nunca se guarda.
connections-gcp-iam-hint = Inicia sesión como el usuario de abajo (el usuario de IAM o la cuenta de servicio que conoce Cloud SQL) con un token de la Google Cloud CLI (antes, `gcloud auth login`). El token se obtiene cuando una conexión lo necesita y nunca se guarda.
connections-windows-hint-named = Inicia sesión con la cuenta de Windows con la que se ejecuta Schemaic ({ $account }). El servidor debe conocer esa cuenta como inicio de sesión de Windows.
connections-windows-hint = Inicia sesión con la cuenta de Windows con la que se ejecuta Schemaic. El servidor debe conocer esa cuenta como inicio de sesión de Windows.
connections-database-file = Archivo de base de datos
connections-database-file-dialog = Seleccionar base de datos SQLite
connections-sqlite-hint = Una base de datos SQLite es un único archivo, así que no hay servidor al que llegar: ni host, ni usuario, ni contraseña, ni túnel. Aún no admite transacciones manuales: las sentencias se confirman según se envían.
connections-ssh-tunnel = Túnel SSH
connections-ssh-tunnel-hint = Llega al servidor a través de un túnel SSH.
connections-ssh-host = Host SSH
connections-ssh-port = Puerto SSH
connections-ssh-user = Usuario SSH
connections-ssh-password = Contraseña SSH
connections-ssh-agent-hint = La firma se delega en tu agente SSH en ejecución (agente de OpenSSH / Pageant en Windows). Schemaic no guarda ninguna clave.
connections-jump-hosts = Hosts de salto
connections-jump-hosts-hint = Opcional. Hosts por los que pasar antes del host SSH, del más cercano al más lejano; cada uno inicia sesión de la forma indicada abajo.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , clave { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , a través de { $jump }
       *[none] {""}
    }
connections-route-jump-host = host de salto { $host }
connections-route-proxy-command = ~/.ssh/config llega a { $host } a través de un ProxyCommand, que Schemaic no puede ejecutar. Indica la ruta en Hosts de salto.
connections-route-match = ~/.ssh/config puede encaminar { $host } a través de un bloque Match, que Schemaic no lee. Indica la ruta en Hosts de salto.
connections-route-match-destination = ~/.ssh/config puede dirigir { $host } a otro host o puerto mediante un bloque Match, que Schemaic no lee. Indica la propia dirección como host SSH.
connections-route-include = ~/.ssh/config incluye un archivo que Schemaic no ha podido leer donde se aplica a { $host }. Indica la propia dirección como host SSH, y la ruta en Hosts de salto.
connections-route-config-jump-password = ~/.ssh/config llega a { $host } a través de { $jump }, y la contraseña solo se envía a los hosts que nombra esta conexión. Añade el host de salto a Hosts de salto si usa la misma contraseña, o inicia sesión con una clave o con el agente SSH.
connections-route-jump-own-proxy-jump = ~/.ssh/config llega al host de salto { $host } a través de { $jump }, y Schemaic no sigue el ProxyJump propio de un host de salto. Indica la ruta en Hosts de salto, con { $jump } primero.
connections-jump-host-error = Host de salto { $number }: { $why }
connections-jump-empty = vacío: quita la coma sobrante
connections-jump-space = «{ $entry }» contiene un espacio
connections-jump-unclosed = «{ $entry }» abre un [ que no cierra
connections-jump-after-bracket = «{ $entry }» tiene texto después de su ]
connections-jump-no-host = «{ $entry }» no nombra ningún host
connections-jump-not-a-port = «{ $entry }» no es un puerto
connections-private-key = Clave privada
connections-private-key-dialog = Seleccionar clave privada
connections-passphrase = Frase de contraseña
connections-tls = SSL / TLS
tls-disable = Desactivado
tls-prefer = Preferir
tls-require = Obligatorio
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Conecta en texto sin cifrar.
tls-prefer-description = Cifra si el servidor lo ofrece. Sin verificación.
tls-require-description = Se niega a conectar sin cifrado. Sin verificación.
tls-verify-ca-description = Cifra y comprueba el certificado con la CA.
tls-verify-full-description = Cifra, comprueba el certificado y comprueba el nombre del host.
tls-verify-ca-checks-host = En MySQL y MariaDB también comprueba el nombre del host, así que se comporta como Verify full: es una limitación del controlador, no un ajuste.
tls-floor-azure-cli = El inicio de sesión con Microsoft Entra conecta en Verify full se elija lo que se elija aquí: su token abre todas las bases de datos de Azure SQL a las que llega la cuenta.
tls-floor-aws-iam = El inicio de sesión con AWS IAM conecta como mínimo en modo Obligatorio: su token no debe cruzar la red sin cifrar. Verify full necesita el paquete de CA de RDS como CA raíz.
tls-floor-gcp-iam = El inicio de sesión con Google Cloud IAM conecta al menos en Verify CA (pon como CA raíz la CA del servidor de la instancia): su token abre todas las instancias de Cloud SQL a las que llega la cuenta.
tls-floor-other = Este inicio de sesión conecta en un modo más estricto que el elegido aquí.
connections-ca-certificate = Certificado de CA
connections-ca-certificate-dialog = Seleccionar certificado de CA
connections-ca-certificate-hint = Déjalo vacío para confiar en las autoridades de certificación en las que confía tu sistema operativo, incluidas las que haya instalado tu organización. Indica un archivo para confiar en ese en su lugar.
connections-client-certificate = Certificado de cliente
connections-client-certificate-dialog = Seleccionar certificado de cliente
connections-client-key = Clave de cliente
connections-client-key-dialog = Seleccionar clave de cliente
connections-client-identity-hint = Solo se necesitan un certificado y una clave de cliente cuando el servidor te pide demostrar quién eres (TLS mutuo). La clave no debe estar protegida con frase de contraseña.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Importar en { $table }
import-section-source = Origen
import-section-reading = Lectura
import-section-columns = Columnas
import-section-preview = Vista previa
import-file = Archivo
import-choose-file = Elegir un archivo…
import-file-dialog = Importar en la tabla
import-no-file = No se ha elegido ningún archivo
import-unnamed-file = (archivo)
import-format = Formato
import-sheet = Hoja
import-delimiter = Delimitador
import-header-row = La primera fila es una cabecera
import-header-row-hint = Usa la primera fila como nombres de columna en lugar de como datos.
import-trim = Recortar espacios
import-trim-hint = Quita los espacios alrededor de cada valor, también dentro de las comillas.
import-empty-is-null = Un campo vacío es NULL
import-empty-is-null-hint = Un campo vacío significa NULL en lugar de una cadena vacía.
import-null-tokens = Otros valores que significan NULL
import-null-tokens-hint = Separados por comas, p. ej. NULL, \N, NA.
import-skip = Omitir
import-nothing-mapped = No hay nada asignado, así que no hay nada que importar.
import-values-withheld = El primer registro de este archivo es demasiado grande para la vista previa, así que solo se han leído los nombres de columna. La asignación y la importación leen el archivo completo.
import-missing-required = Sin asignar y no pueden estar vacías: { $columns }. La importación fallará salvo que la base de datos las rellene.
import-engine-not-transactional = El motor de almacenamiento de esta tabla ({ $engine }) no es transaccional, así que una importación fallida no se puede deshacer: las filas cargadas antes del fallo se quedarán.
import-problems = { $count ->
    [one] { $count } problema en el archivo: no se ha importado nada.
   *[other] { $count } problemas en el archivo: no se ha importado nada.
}
import-problem-at-line = línea { $line }: { $message }
import-problem-at-column = línea { $line }, { $column }: { $message }
import-issue-not-an-integer = no es un número entero
import-issue-not-a-number = no es un número
import-issue-not-a-boolean = no es un valor verdadero/falso
import-issue-cell-error = la hoja no ha podido evaluar esta celda
import-issue-null-in-not-null = vacío, pero la columna no puede ser NULL
import-issue-field-count = tiene { $found } campos; se esperaban { $expected }
import-tail-cut = Se muestran { $shown } de { $total }.
import-tail-uncounted = Se muestran los { $total }: la comprobación dejó de contar ahí, así que el archivo contiene más.
import-tail-cut-and-uncounted = Se muestran { $shown } de los primeros { $total }: la comprobación dejó de contar ahí, así que el archivo contiene más.
import-memory-json = Este archivo JSON ocupa { $size }, y una importación JSON se mantiene en memoria mientras se carga: cuenta con que necesite unos { $estimate }. Un CSV con los mismos datos se carga con memoria constante.
import-memory-xlsx = Este libro ocupa { $size }, y una importación de Excel se mantiene en memoria mientras se carga: cuenta con que necesite aproximadamente { $estimate }, porque el archivo está comprimido. Un CSV de la misma hoja se carga con memoria constante.
import-read-failed = No se ha podido leer el archivo: { $error }
import-no-columns-mapped = No hay columnas del archivo asignadas a columnas de la tabla
import-preview-record-too-large = su primer registro JSON supera los { $mib } MiB que lee la vista previa, así que no hay nada que mostrar
import-refuse-not-objects = se esperaban objetos JSON (una matriz de ellos, o uno por línea)
import-refuse-oversize = Este libro ocupa { $size }, y un archivo de Excel tiene que leerse entero antes de poder mostrar nada: { $max } es lo máximo que se puede abrir. Exporta la hoja como CSV e importa ese archivo; un CSV se carga con memoria constante.
import-refuse-oversize-unsized = Este libro ocupa más de { $max }, y un archivo de Excel tiene que leerse entero antes de poder mostrar nada: { $max } es lo máximo que se puede abrir. Exporta la hoja como CSV e importa ese archivo; un CSV se carga con memoria constante.
import-refuse-inflated = Este libro ocupa { $size } en disco pero se descomprime en al menos { $unpacked }, y un archivo de Excel tiene que leerse entero antes de poder mostrar nada: { $ceiling } de contenido descomprimido es lo máximo que se abrirá, y se necesita más memoria que eso para contenerlo. Exporta la hoja como CSV e importa ese archivo; un CSV se carga con memoria constante.
import-refuse-no-such-sheet = este libro no tiene ninguna hoja llamada «{ $sheet }»; tiene { $sheets }
import-refuse-no-sheets = este libro no tiene hojas
import-refuse-too-wide = esta hoja declara { $width } columnas; una hoja de cálculo de Excel admite { $max }. Expórtala como CSV e importa ese archivo.
import-cancelled = Importación cancelada: la transacción se ha revertido, así que no se ha escrito nada.
import-done = { $count ->
    [one] Se ha importado { $count } fila en { $table }.
   *[other] Se han importado { $count } filas en { $table }.
}
import-next = Siguiente
import-reading = Leyendo…
import-back = Atrás
import-import = Importar
import-importing = Importando…
dialog-read-only = Esta conexión es de solo lectura.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Exportar { $database }.{ $schema } a { $format }
dump-title = Exportar { $database } a { $format }
dump-sql-dialog = Exportar a SQL
dump-folder-dialog = Exportar a { $format }
dump-folder-hint = Un archivo { $format } por tabla, escrito en una carpeta que elijas.
dump-all-selected = Las { $total } seleccionadas
dump-some-selected = { $chosen } de { $total } seleccionadas
dump-select-all = Seleccionar todo
dump-select-none = Ninguna
dump-reading-tables = Leyendo la lista de tablas…
dump-no-tables = Esta base de datos no tiene tablas.
dump-tables-unreadable = No se ha podido leer la lista de tablas.
dump-section-contents = Contenido
dump-structure = Estructura
dump-structure-hint = CREATE TABLE, sus disparadores y las claves foráneas, reformuladas después de los datos.
dump-data = Datos
dump-data-hint = Todas las filas, como sentencias INSERT.
dump-other-objects = Otros objetos
dump-other-objects-hint = Tipos, secuencias, rutinas y eventos de los que dependen estas tablas.
dump-section-replaying = Reproducción
dump-drop-before-create = Eliminar antes de crear
dump-drop-hint-up-front = Elimina cada tabla y vista antes de su CREATE, para que el archivo se pueda cargar en una base de datos que ya las contenga. Con Una transacción también reemplaza todas las rutinas de sus esquemas, y se detiene antes de eliminar nada que no pueda restaurar tal como está. Sin Una transacción deja las rutinas como están, así que una reproducción en una base de datos que aún las tenga se detiene en la primera. El propietario y los permisos de cada tabla se restauran al final.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS antes de cada CREATE, para que el archivo se pueda cargar en una base de datos que ya contenga estas tablas. También reemplaza todas las rutinas de sus esquemas. El propietario, los permisos concedidos y la seguridad de filas de cada tabla se restauran al final.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS antes de cada CREATE, para que el archivo se pueda cargar en una base de datos que ya contenga estas tablas. También reemplaza todas las rutinas y eventos de sus esquemas: en MySQL cada uno se elimina primero, lo que revoca todos los permisos concedidos sobre él, y el archivo los vuelve a conceder al final si tu cuenta puede leer mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS antes de cada CREATE, para que el archivo se pueda cargar en una base de datos que ya contenga estas tablas.
dump-one-transaction = Una transacción
dump-one-transaction-hint = Envuelve la carga, para que un fallo no deje nada a medias.
dump-disable-fk-checks = Desactivar las comprobaciones de claves foráneas
dump-fk-hint = Desactivadas mientras dura la carga, para que el orden en que llegan las filas no haga que se rechacen.
dump-fk-hint-drops = Desactivadas mientras dura la carga, para que el orden en que llegan las filas no haga que se rechacen. Un archivo que elimina tablas las desactiva en cualquier caso: una eliminación con ellas activadas borra filas de tablas ajenas al archivo mediante sus acciones ON DELETE.
dump-no-fk-guard-hint = Este motor no tiene un interruptor de sesión para las comprobaciones de claves foráneas; en su lugar, el archivo añade las restricciones después de los datos.
dump-export = Exportar
dump-progress = { $count ->
    [one] Escribiendo { $table }: { $index } de { $total }, { $rows } fila hasta ahora
   *[other] Escribiendo { $table }: { $index } de { $total }, { $rows } filas hasta ahora
}
dump-reading-schema = Leyendo el esquema
dump-replace-files = Reemplazar archivos
dump-replace-prompt = { $n ->
    [one] Se reemplazará { $n } archivo de { $folder }, y no se podrá recuperar: { $names }.
   *[other] Se reemplazarán { $n } archivos de { $folder }, y no se podrán recuperar: { $names }.
}
dump-replace-prompt-more = Se reemplazarán { $n } archivos de { $folder }, y no se podrán recuperar: { $names } y { $more } más.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] Se ha escrito { $n } tabla.
   *[other] Se han escrito { $n } tablas.
}
dump-done-missing = { $n ->
    [one] No se ha encontrado { $n } tabla marcada y no está en el archivo: { $names }.
   *[other] No se han encontrado { $n } tablas marcadas y no están en el archivo: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } objeto que pedía la exportación no está en el archivo: { $names }.
   *[other] { $n } objetos que pedía la exportación no están en el archivo: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Las filas de la tabla de aristas de grafo { $names } no están en el archivo; se crea vacía.
   *[other] Las filas de las tablas de aristas de grafo { $names } no están en el archivo; se crean vacías.
}
dump-done-refused = { $n ->
    [one] Una reproducción en una base de datos que ya lo contenga se detiene antes de eliminar nada, porque el archivo no puede restaurarlo tal como está: { $names }.
   *[other] Una reproducción en una base de datos que ya contenga alguno de estos se detiene antes de eliminar nada, porque el archivo no puede restaurarlos tal como están: { $names }.
}
dump-cancelled-partial = Exportación cancelada: { $name } no se ha modificado; lo que se había escrito está en { $part }
files-done = { $n ->
    [one] Se ha escrito { $n } archivo.
   *[other] Se han escrito { $n } archivos.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Exportación cancelada: no se ha terminado ningún archivo, así que no se ha escrito nada en { $folder }.
files-cancelled = { $n ->
    [one] Exportación cancelada: se ha terminado { $n } archivo en { $folder }; la tabla en curso se ha quedado sin escribir.
   *[other] Exportación cancelada: se han terminado { $n } archivos en { $folder }; la tabla en curso se ha quedado sin escribir.
}
files-failed-kept = { $n ->
    [one] { $message }; se conserva { $n } archivo ya escrito en { $folder }.
   *[other] { $message }; se conservan { $n } archivos ya escritos en { $folder }.
}
files-replaced = { $n ->
    [one] Se ha reemplazado { $n } archivo existente: { $names }.
   *[other] Se han reemplazado { $n } archivos existentes: { $names }.
}
files-missing = { $n ->
    [one] No se ha encontrado { $n } tabla marcada y no se ha generado su archivo: { $names }.
   *[other] No se han encontrado { $n } tablas marcadas y no se han generado sus archivos: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } de ~{ $total } fila
   *[other] { $rows } de ~{ $total } filas
}
export-progress-of = { $count ->
    [one] { $rows } de { $total } fila
   *[other] { $rows } de { $total } filas
}
export-progress-rows = { $count ->
    [one] { $rows } fila
   *[other] { $rows } filas
}
export-progress-starting = Iniciando
export-progress-file = Archivo: { $name }
export-progress-writing = Escribiendo { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Monitor en vivo
monitor-badge-watching = Observando
monitor-badge-paused = En pausa
monitor-badge-stopped = Detenido
monitor-filter-all = Todo
monitor-filter-inserts = Inserciones
monitor-filter-updates = Actualizaciones
monitor-filter-deletes = Eliminaciones
monitor-resume = Reanudar el sondeo
monitor-pause = Pausar el sondeo
monitor-clear-tip = Borrar el registro
monitor-export-tip = Exportar el registro…
monitor-interval = { $secs } s
monitor-clear-title = ¿Borrar el registro?
monitor-clear-message = { $count ->
    [one] Se descartaría { $count } cambio. El registro es la única constancia de lo que contenía una fila eliminada (el sondeo no volverá a informar de un cambio) y esto no se puede deshacer. Expórtalo antes si quieres conservarlo.
   *[other] Se descartarían { $count } cambios. El registro es la única constancia de lo que contenía una fila eliminada (el sondeo no volverá a informar de un cambio) y esto no se puede deshacer. Expórtalo antes si quieres conservarlo.
}
monitor-col-time = Hora
monitor-col-action = Acción
monitor-no-inserts = Aún no hay inserciones.
monitor-no-updates = Aún no hay actualizaciones.
monitor-no-deletes = Aún no hay eliminaciones.
monitor-empty-stopped = Detenido.
monitor-empty-paused = En pausa.
monitor-waiting = Esperando
monitor-events-since = { $count ->
    [one] { $count } evento desde las { $since }
   *[other] { $count } eventos desde las { $since }
}
monitor-no-changes = Aún no hay cambios
monitor-partial = solo se observan las primeras { $cap } filas por clave primaria
monitor-capped = los más antiguos se descartan al llegar al límite de { $cap } cambios
monitor-length-only = { $count ->
    [one] { $columns } es binaria, así que sus cambios solo se detectan por la longitud
   *[other] { $columns } son binarias, así que sus cambios solo se detectan por la longitud
}
monitor-newest-at-bottom = lo más reciente, abajo
monitor-export-dialog = Exportar registro de cambios

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Comparar resultados
diff-filter-all = Todo
diff-filter-changed = Cambiadas
diff-filter-added = Añadidas
diff-filter-removed = Quitadas
diff-match-rows-by = Emparejar filas por
diff-whole-row = Fila completa
diff-export-tip = Exportar la comparación…
diff-nothing-to-export = No hay nada que exportar.
diff-export-dialog = Exportar comparación
diff-unnamed-file = la comparación
diff-ran-at = Ejecutada a las { $time }
diff-side-facts = { $count ->
    [one] { $count } fila · { $ms } ms
   *[other] { $count } filas · { $ms } ms
}
diff-only-in = Solo en { $side }: { $columns }
diff-uncompared = Binarias, sin comparar: { $columns }
diff-missing-key = ambos resultados necesitan una columna llamada { $column } por la que emparejar filas
diff-binary-key = { $column } contiene valores binarios, que no se comparan, así que no puede emparejar filas
diff-summary-identical = { $n ->
        [one] Sin diferencias · { $n } fila sin cambios
       *[other] Sin diferencias · { $n } filas sin cambios
    }
diff-summary-changed = { $n } cambiadas
diff-summary-added = { $n } añadidas
diff-summary-removed = { $n } quitadas
diff-summary-unchanged = { $n } sin cambios
diff-summary-not-compared = { $columns } sin comparar
diff-repeated-keys = { $count ->
    [one] { $count } fila repite una clave ya usada en su lado, y se deja fuera.
   *[other] { $count } filas repiten una clave ya usada en su lado, y se dejan fuera.
}
diff-capped = Un resultado estaba limitado: las filas que superan su límite no se comparan.
diff-col-change = Cambio
diff-no-differences = Sin diferencias.
diff-no-changed = No hay filas cambiadas.
diff-no-added = No hay filas añadidas.
diff-no-removed = No hay filas quitadas.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Antes de aplicar
preview-risk-irreversible = Esto no se puede deshacer

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Crear la tabla { $name } con { $columns } columna
   *[other] Crear la tabla { $name } con { $columns } columnas
}
ddl-line-rebuild-table = Reconstruir { $name }: no se puede modificar en su sitio, así que las filas se copian en una tabla nueva que ocupa el lugar de la antigua
ddl-line-drop-table = Eliminar la tabla
ddl-line-drop-foreign-table = Eliminar la tabla externa
ddl-line-truncate-table = Borrar todas las filas
ddl-line-rename-table = Renombrar la tabla a { $to }
ddl-line-add-column = Añadir la columna { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Eliminar la columna { $name }
ddl-line-rebuild-computed-column = Reconstruir la columna calculada { $name } como ({ $expression }) en torno al cambio de columna
ddl-line-rename-column = Renombrar la columna { $from } a { $to }{ $retyped ->
        [yes] , y cambiar su tipo de { $old_type } a { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Mover la columna { $name } al principio
ddl-line-move-column-after = Mover la columna { $name } detrás de { $after }
ddl-line-move-column = Mover la columna { $name }
ddl-line-retype-column = Cambiar la columna { $name } de { $from } a { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Cambiar la columna { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (ninguno)
ddl-line-add-primary-key = Añadir la clave primaria ({ $columns })
ddl-line-drop-primary-key = Eliminar la clave primaria ({ $columns })
ddl-line-rebuild-primary-key = Reconstruir la clave primaria ({ $columns }) en torno al cambio de columna
ddl-line-change-primary-key = Cambiar la clave primaria de ({ $from }) a ({ $to })
ddl-line-add-index = Añadir { $unique ->
        [yes] el índice único
       *[no] el índice
    }{ $has_kind ->
        [yes] {" "}{ $kind }
       *[no] {""}
    } { $name }{ $has_method ->
        [yes] {" "}con { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}sobre ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}que incluye ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}ordenado por ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}donde { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = de almacén de columnas
ddl-index-kind-clustered-columnstore = de almacén de columnas agrupado
ddl-index-kind-primary-xml = XML primario
ddl-index-kind-selective-xml = XML selectivo
ddl-index-kind-secondary-xml = XML secundario
ddl-index-kind-spatial = espacial
ddl-line-drop-index = Eliminar el índice { $name }
ddl-line-rename-index = Renombrar el índice { $from } a { $to }
ddl-line-keep-lossy-index = Dejar sin cambios el índice { $name }: Schemaic no puede leerlo entero
ddl-line-add-foreign-key = Añadir la clave foránea { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Eliminar la clave foránea { $name }
ddl-line-add-check = Añadir la comprobación { $name } ({ $expression })
ddl-line-drop-check = Eliminar la comprobación { $name }
ddl-line-table-options = Establecer en la tabla: { $parts }
ddl-table-option-engine = motor { $engine }
ddl-table-option-collation = intercalación { $collation }
ddl-table-option-comment = comentario
ddl-table-option-comment-cleared = comentario (borrado)
ddl-line-create-view = Crear la vista { $name }
ddl-line-create-view-index = Crear la vista { $name } y su índice { $index }
ddl-line-create-view-indexes = Crear la vista { $name } y sus índices { $indexes }
ddl-line-redefine-view = Redefinir la vista { $name }
ddl-line-recreate-view = Eliminar y volver a crear la vista { $name }
ddl-line-rename-view = Renombrar la vista a { $to }
ddl-line-drop-view = Eliminar la vista
ddl-line-drop-materialized-view = Eliminar la vista materializada
ddl-line-refresh-view = Reconstruir las filas de la vista materializada: los lectores quedan bloqueados hasta que termine
ddl-line-refresh-view-concurrently = Reconstruir las filas de la vista materializada sin bloquear a los lectores
ddl-line-create-trigger = Crear el disparador { $name }
ddl-line-redefine-trigger = Redefinir el disparador { $name }
ddl-line-recreate-trigger = Volver a crear el disparador { $name }
ddl-line-recreate-trigger-as = Volver a crear el disparador { $from } como { $name }
ddl-line-drop-trigger = Eliminar el disparador { $name }
ddl-line-create-routine = Crear { $what } { $name }
ddl-line-redefine-routine = Redefinir { $what } { $name }
ddl-line-redefine-routine-as = Redefinir { $what } { $from } como { $name }
ddl-line-recreate-routine = Volver a crear { $what } { $name }
ddl-line-recreate-routine-as = Volver a crear { $what } { $from } como { $name }
ddl-line-rename-routine = Renombrar { $what } { $from } a { $to }
ddl-line-drop-routine = Eliminar { $what } { $name }
ddl-line-create-event = Crear el evento { $name }
ddl-line-alter-event = Modificar el evento { $name }
ddl-line-alter-event-edits = Modificar el evento { $name }: { $edits }
ddl-line-drop-event = Eliminar el evento { $name }
ddl-event-edit-rename = renombrar a { $to }
ddl-event-edit-schedule = programación: { $schedule }
ddl-event-edit-status-enabled = estado: activado
ddl-event-edit-status-disabled = estado: desactivado
ddl-event-edit-status-replica = estado: desactivado en la réplica
ddl-event-edit-preserve = conservarlo al completarse
ddl-event-edit-no-preserve = eliminarlo al completarse
ddl-event-edit-definer = definidor: { $definer }
ddl-event-edit-comment = comentario
ddl-event-edit-body = cuerpo
ddl-line-create-enum = { $values ->
    [one] Crear el tipo { $name } con { $values } valor
   *[other] Crear el tipo { $name } con { $values } valores
}
ddl-line-add-enum-value = Añadir el valor { $value }
ddl-line-add-enum-value-after = Añadir el valor { $value } detrás de { $after }
ddl-line-add-enum-value-before = Añadir el valor { $value } delante de { $before }
ddl-line-rename-enum-value = Renombrar el valor { $from } a { $to }
ddl-line-recreate-enum = Volver a crear el tipo { $name } ({ $values ->
    [one] { $values } valor
   *[other] { $values } valores
}, reconvirtiendo { $columns ->
    [one] { $columns } columna
   *[other] { $columns } columnas
})
ddl-line-create-domain = Crear el dominio { $name } como { $datatype }
ddl-line-set-domain-default = Establecer el valor predeterminado del dominio en { $default }
ddl-line-drop-domain-default = Eliminar el valor predeterminado del dominio
ddl-line-domain-not-null = Hacer el dominio NOT NULL
ddl-line-domain-nullable = Hacer que el dominio admita NULL
ddl-line-recreate-domain = Volver a crear el dominio { $name } como { $datatype } (antes { $was }; reconvirtiendo { $columns ->
    [one] { $columns } columna
   *[other] { $columns } columnas
})
ddl-line-create-sequence = Crear la secuencia { $name }
ddl-line-alter-sequence = Establecer en la secuencia: { $edits }
ddl-sequence-edit-type = tipo { $datatype }
ddl-sequence-edit-increment = incremento { $value }
ddl-sequence-edit-minimum = mínimo { $value }
ddl-sequence-edit-maximum = máximo { $value }
ddl-sequence-edit-start = inicio { $value }
ddl-sequence-edit-cache = caché { $value }
ddl-sequence-edit-cycling-on = cíclica
ddl-sequence-edit-cycling-off = no cíclica
ddl-sequence-edit-owner = propietario { $owner }
ddl-sequence-edit-no-owner = sin propietario
ddl-line-restart-sequence = Reiniciar la secuencia en { $value }
ddl-line-create-object = Crear { $what } { $name }
ddl-line-replace-object = Eliminar { $what } { $name } y volver a crearlo
ddl-line-replace-object-as = Eliminar { $what } { $name } y volver a crearlo como { $to }
ddl-line-alter-tsql-sequence = Modificar la secuencia: { $clauses }
ddl-line-rename-object = Renombrar { $what } a { $to }
ddl-line-drop-object = Eliminar { $what }
ddl-line-set-object-comment = Establecer el comentario de { $what }
ddl-line-clear-object-comment = Borrar el comentario de { $what }
ddl-line-create-database = Crear la base de datos { $name }
ddl-line-drop-database = Eliminar la base de datos { $name }
ddl-line-create-schema = Crear el esquema { $name }
ddl-line-drop-schema = Eliminar el esquema { $name }
ddl-line-create-extension = Crear la extensión { $name }
ddl-line-create-user = Crear el usuario { $who }
ddl-line-create-role = Crear el rol { $who }
ddl-line-create-login = Crear el inicio de sesión { $who }
ddl-line-drop-user = Eliminar el usuario { $who }
ddl-line-drop-role = Eliminar el rol { $who }
ddl-line-drop-login = Eliminar el inicio de sesión { $who }
ddl-line-set-password = Establecer la contraseña de { $who }
ddl-line-grant-privileges = Conceder { $privileges } a { $who }
ddl-line-revoke-privileges = Revocar { $privileges } a { $who }
ddl-privileges-none = ningún privilegio
ddl-privileges-count = { $count } privilegios
ddl-line-grant-role = Conceder { $role } a { $member }
ddl-line-revoke-role = Revocar { $role } a { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Elimina { $name } y la vuelve a crear, copiando todas las filas, y restaura sus claves, índices, comprobaciones, valores predeterminados y disparadores, sus comentarios propios y los de sus columnas, y las claves foráneas que otras tablas tienen sobre ella. Se detiene antes de empezar si la tabla tiene cualquier otra cosa que Schemaic no reformula: permisos, un propietario propio, un dependiente vinculado al esquema, versiones del sistema, un libro de contabilidad, replicación, compresión, opciones de índice, un índice desactivado, una regla o un valor predeterminado vinculados, una clasificación de confidencialidad, o una descripción en una clave, índice, valor predeterminado, comprobación o disparador. Las estadísticas creadas a mano no se trasladan.
       *[other] Elimina { $name } y la vuelve a crear, copiando todas las filas. Lo que Schemaic no haya leído de la tabla no se traslada.
    }
ddl-risk-drop-table = Elimina la tabla y todas sus filas.
ddl-risk-drop-foreign-table = Elimina la tabla externa: su definición desaparece, y las filas que lee se quedan donde están, en el servidor externo o en el archivo; una vista o rutina que la nombre fallará a partir de entonces.
ddl-risk-truncate-table = Borra todas las filas de la tabla. Esto no se puede deshacer.
ddl-risk-drop-column = Elimina la columna { $name } y todos sus datos.
ddl-risk-rebuild-computed-column = Elimina la columna calculada { $name } y la vuelve a añadir, así que pasa al final de la tabla.
ddl-risk-column-not-null = La columna { $name } pasa a ser NOT NULL: la sentencia falla si alguna fila contiene NULL.
ddl-risk-column-stops-generated = La columna { $name } deja de ser generada. Sus valores actuales se conservan, pero a partir de ahora nada los vuelve a calcular.
ddl-risk-column-becomes-generated = La columna { $name } pasa a generarse a partir de { $expression }. Todos sus valores se sustituyen por lo que calcule la expresión.
ddl-risk-column-expression-changes = La expresión de la columna { $name } cambia de { $from } a { $to }. En PostgreSQL esto elimina y vuelve a añadir la columna, así que todos los valores se vuelven a calcular y la columna pasa al final de la tabla.
ddl-risk-type-narrows = Estrechar { $subject } de { $from } a { $to } { $loss ->
        [truncates] trunca los valores que ya no quepan
        [rounds] redondea todos los valores de la columna
       *[both] trunca los valores que ya no quepan y redondea todos los valores de la columna
    }.
ddl-risk-type-rewrites = Cambiar { $subject } de { $from } a { $to } reescribe todos los valores; puede fallar o perder precisión.
ddl-risk-domain-subject = el dominio { $name }
ddl-risk-primary-key-dropped = Deja la tabla sin clave primaria: las filas ya no se podrán editar desde la cuadrícula.
ddl-risk-primary-key-rebuilt = Elimina y vuelve a añadir la clave primaria en torno al cambio de columna. Si es el índice agrupado, esto reescribe toda la tabla, y una clave foránea de otra tabla que haga referencia a ella detiene el plan.
ddl-risk-primary-key-replaced = Reemplaza la clave primaria: { $count ->
        [one] { $columns } deja
       *[other] { $columns } dejan
    } de ser única, y a partir de ahora se aceptan duplicados. En MySQL esto reconstruye el índice agrupado, lo que reescribe toda la tabla.
ddl-risk-drop-foreign-key = Elimina la clave foránea { $name }. A partir de ahora se aceptan filas sin padre correspondiente, y los datos existentes no se vuelven a examinar si se añade de nuevo.
ddl-risk-drop-unique-index = Elimina el índice único { $name }. A partir de ahora se aceptan valores duplicados, y la cuadrícula puede perder la clave con la que editaba las filas.
ddl-risk-drop-view = { $materialized ->
        [yes] Elimina la vista materializada. { $cost }
       *[no] Elimina la vista. { $cost }
    }
ddl-risk-view-cost-sqlite = Las vistas que seleccionan de ella dejan de resolverse hasta que vuelva, y sus disparadores INSTEAD OF se eliminan con ella y no se restauran.
ddl-risk-view-cost-sqlite-restored = Las vistas que seleccionan de ella dejan de resolverse hasta que vuelva, y sus disparadores INSTEAD OF se eliminan con ella.
ddl-risk-view-cost-sqlserver = Los permisos concedidos sobre ella, su descripción y sus disparadores INSTEAD OF se eliminan con ella y no se restauran.
ddl-risk-view-cost-sqlserver-restored = Los permisos concedidos sobre ella y su descripción se eliminan con ella y no se restauran; sus disparadores INSTEAD OF también se eliminan.
ddl-risk-view-cost-other = Las vistas dependientes, las reglas, los permisos concedidos y sus disparadores INSTEAD OF se eliminan con ella y no se restauran.
ddl-risk-view-cost-other-restored = Las vistas dependientes, las reglas y los permisos concedidos se eliminan con ella y no se restauran; sus disparadores INSTEAD OF también se eliminan.
ddl-risk-recreate-view = Volver a crear { $name } la elimina primero. { $cost } { $why ->
        [sqlite] SQLite no puede reemplazar una vista en su sitio, así que cualquier edición de una vista, incluido un cambio de nombre, sigue este camino.
        [sqlserver] El sp_rename de SQL Server dejaría la definición guardada de la vista con el nombre antiguo, así que un cambio de nombre sigue este camino.
       *[postgres] PostgreSQL no puede reemplazar una vista cuyas columnas han cambiado de nombre, tipo u orden, así que esta es la única forma de aplicar la edición.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } sentencia eliminada con ella se restaura después, en la misma transacción.
       *[other] { $count } sentencias eliminadas con ella se restauran después, en la misma transacción.
    }
ddl-risk-view-indexes-dropped = Redefinir la vista { $view } elimina { $dropped_count ->
        [one] su índice
       *[other] sus índices
    } ({ $dropped }), como hace siempre el ALTER VIEW de SQL Server, { $case ->
        [all] y este plan { $again_count ->
            [one] lo vuelve
           *[other] los vuelve
        } a construir después, en la misma transacción. En una vista grande, eso tarda lo mismo que tardó construir{ $again_count ->
            [one] lo
           *[other] los
        } en su día. Un índice con factor de relleno, compresión, relleno, opciones de bloqueo, un grupo de archivos o una descripción detiene el plan antes de que cambie nada: Schemaic no puede volver a construirlos. Las estadísticas creadas a mano sobre la vista no se trasladan.
        [none] y este plan no { $gone_count ->
            [one] lo vuelve
           *[other] los vuelve
        } a construir: la vista con la que se quiere igualar no tiene ese índice. Las estadísticas creadas a mano sobre la vista no se trasladan.
       *[some] y este plan vuelve a construir { $again } después, en la misma transacción, pero no { $gone }, que la vista con la que se quiere igualar no tiene. Un índice con factor de relleno, compresión, relleno, opciones de bloqueo, un grupo de archivos o una descripción detiene el plan antes de que cambie nada: Schemaic no puede volver a construirlos. Las estadísticas creadas a mano sobre la vista no se trasladan.
    }
ddl-risk-add-not-null-column = La columna { $name } es NOT NULL y no tiene valor predeterminado. Si la tabla ya tiene filas, no hay valor que darles, y el motor rechazará el plan entero.
ddl-risk-drop-check = Elimina la comprobación { $name }. A partir de ahora se aceptan las filas que la restricción rechazaba, y los datos existentes no se vuelven a examinar si se añade de nuevo.
ddl-risk-keep-lossy-index = Tu edición del índice { $name } no se incluye. Usa algo que Schemaic no puede volver a leer (una columna de clave con expresión, una clase de operador o una ordenación NULLS), y aplicar la edición supondría volver a crear el índice sin ello. Edita este índice en SQL.
ddl-risk-drop-trigger = Elimina el disparador { $name }. Lo que hacía en cada escritura deja de ocurrir, y no se aplicará a las filas que se escriban a partir de ahora.
ddl-risk-redefine-trigger-in-place = Redefine el disparador { $name } en su sitio (no se elimina, así que conserva sus permisos), y lo que hace en cada escritura cambia a partir de ahora.
ddl-risk-recreate-trigger = Volver a crear el disparador { $name } lo elimina primero. Donde el DDL no es transaccional (MySQL), si el servidor rechaza la nueva definición, la tabla se queda sin disparador.
ddl-risk-signature-lost = { $kind ->
        [trigger] El disparador
        [function] La función
       *[procedure] El procedimiento
    } { $name } tiene una firma (ADD SIGNATURE), y cualquier cambio la elimina: Schemaic no puede volver a firmarlo sin la clave del certificado. Lo que el certificado le permitía hacer deja de funcionar hasta que vuelvas a ejecutar ADD SIGNATURE.
ddl-risk-redefines-routine = Redefine { $name }. Todo lo que la llama ejecuta el cuerpo nuevo a partir de ahora, incluidos disparadores de otras tablas que esta edición nunca menciona.
ddl-risk-routine-parameters-change = Cambia la lista de parámetros de { $name }. Las llamadas que pasan los argumentos antiguos ya no se resuelven a ella.
ddl-risk-recreate-routine = Volver a crear { $held } lo elimina primero. Donde el DDL no es transaccional (MySQL), si el servidor rechaza la nueva definición, no queda ningún { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Los privilegios concedidos sobre { $held } y su comentario no sobreviven a la eliminación, y este plan no los restaura: vuelve a aplicar después cualquier GRANT y COMMENT.
       *[no] Los privilegios concedidos sobre { $held } no sobreviven a la eliminación, y este plan no los restaura: vuelve a aplicar después cualquier GRANT.
    }
ddl-risk-drop-numbered-group = Elimina { $what } { $name } y, con él, todos los miembros de su grupo numerado: { $members }.
ddl-risk-drop-routine = Elimina { $what } { $name }. PostgreSQL se niega mientras algún disparador lo use, así que los que lo usen deben eliminarse primero.
ddl-risk-drop-event = Elimina el evento { $name }. Su programación y su cuerpo desaparecen con él: nada más en la base de datos guarda una copia.
ddl-risk-one-shot-event = Este evento se ejecuta una vez y después el servidor lo elimina: es ON COMPLETION NOT PRESERVE. Activa Conservar para mantenerlo.
ddl-risk-one-shot-event-altered = El evento { $name } se ejecutará una vez y después el servidor lo eliminará: es ON COMPLETION NOT PRESERVE. Activa Conservar para mantenerlo.
ddl-risk-add-enum-value = Añadir { $value } no se puede deshacer en su sitio: PostgreSQL no puede quitar un valor de enum, así que revertirlo implica volver a crear el tipo. Además, el valor no se puede usar hasta que se aplique este plan.
ddl-risk-rename-enum-value = A partir de ahora, todas las filas que contienen { $from } leerán { $to }. Si querías quitar { $from } en lugar de renombrarlo, elimínalo y aplica eso por separado.
ddl-risk-recreate = Volver a crear { $kind ->
        [domain] el dominio
       *[type] el tipo
    } { $name } lo elimina y lo reconstruye. Cada columna de abajo se reconvierte a través de texto; un valor que la nueva definición no acepte hace fallar el plan entero, y no se aplica nada{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}y { $more } más
        }. Una vista o función construida sobre él no se puede enumerar aquí y haría que el servidor se negara.
       *[no] . Hoy nada lo usa, pero una vista o función construida sobre él no se puede enumerar aquí y haría que el servidor se negara.
    }
ddl-risk-drop-domain-check = Elimina la comprobación { $name }. Ninguna columna de este dominio se comprobará ya, y los datos existentes no se vuelven a examinar si se añade de nuevo.
ddl-risk-domain-nullable = Todas las columnas de este dominio pasan a aceptar NULL, incluidas las de tablas que esta edición nunca menciona.
ddl-risk-domain-not-null = La sentencia falla si alguna columna de este dominio ya contiene NULL.
ddl-risk-drop-sequence = Elimina la secuencia y la posición que había alcanzado. El servidor se niega mientras el valor predeterminado de alguna columna siga tomando valores de ella, así que esos valores predeterminados deben quitarse primero.
ddl-risk-drop-synonym = Elimina el sinónimo. Nada lo impide: una vista, rutina o consulta que lo nombre fallará a partir de entonces, la próxima vez que se ejecute.
ddl-risk-drop-extension = Elimina la extensión y las funciones, tipos y operadores que instaló. El servidor se niega mientras algo tuyo dependa todavía de ellos (una columna de su tipo, un índice o un servidor sobre su contenedor) e indica qué.
ddl-risk-drop-used-object = Elimina { $what }. El servidor se niega mientras alguna columna lo use, así que esas columnas deben cambiar de tipo primero.
ddl-risk-restart-sequence = Reinicia el contador en { $value }. Los valores ya entregados no cambian, así que una clave a la que vuelva a llegar colisionará.
ddl-risk-replace-synonym = Elimina el sinónimo y lo vuelve a crear. Nada impide la eliminación: las dos sentencias se ejecutan en una sola transacción.
ddl-risk-replace-sequence = Elimina la secuencia y la vuelve a crear: se pierde la posición que había alcanzado, y empieza de nuevo en { $start }. El servidor rechaza la eliminación mientras el valor predeterminado de alguna columna siga tomando valores de ella.
ddl-risk-start-value = su valor inicial
ddl-risk-replace-object = Elimina { $what } y lo vuelve a crear. El servidor rechaza la eliminación mientras alguna columna lo use, así que esas columnas deben cambiar de tipo primero.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Se rechaza mientras tenga permisos, un propietario propio, propiedades extendidas o una regla o un valor predeterminado vinculados: la nueva creación no los restauraría.
       *[no] Se rechaza mientras tenga permisos, un propietario propio o propiedades extendidas: la nueva creación no los restauraría.
    }
ddl-risk-drop-database = Elimina la base de datos { $name } y todo lo que contiene: todas las tablas con todas sus filas, y todas las vistas, índices, rutinas y disparadores. No se conserva nada y no se puede deshacer: la única vuelta atrás es una copia de seguridad hecha antes de ejecutar esto.
ddl-risk-drop-schema = Elimina el esquema { $name }. El servidor se niega mientras contenga tablas, vistas o rutinas, así que esto o bien elimina un esquema vacío o bien falla{ $cascade ->
        [yes] . Schemaic nunca envía CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Elimina el inicio de sesión { $who } y, con él, todos los permisos de servidor y pertenencias a roles de servidor que tiene, que no constan en ningún otro sitio, así que un inicio de sesión que se vuelva a crear con este nombre empieza sin ellos. El servidor se niega mientras haya algo conectado con él, así que sus sesiones deben terminar primero. Sus usuarios en cada base de datos no se eliminan: se quedan, con sus permisos, sin asignar a ningún inicio de sesión, y un nuevo inicio de sesión con este nombre no se vuelve a vincular a ellos sin ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Elimina { $who } y todos los privilegios que tiene. Los permisos concedidos no constan en ningún otro sitio, así que restaurar la cuenta supone volver a concederlos todos de memoria. { $cuts_off ->
        [yes] Todo lo que esté conectado con ella fallará en su siguiente sentencia en esta base de datos.
       *[no] Todo lo que siga conectado con ella seguirá funcionando hasta que se desconecte.
    }
ddl-risk-set-password = Cambia la contraseña de { $who }. La antigua no se puede recuperar, así que todo lo que siga configurado con ella (una aplicación, un trabajo, el cliente de otro desarrollador) no podrá conectar hasta que se actualice a mano. Las sesiones ya abiertas siguen funcionando.
ddl-risk-revoke-privileges = Retira { $privileges } a { $who }. Todo lo que dependa de ello (una aplicación, un trabajo, una vista de otro propietario) empezará a fallar en su siguiente sentencia.{ $cascades ->
        [yes] {" "}La revocación lleva CASCADE, así que también se retira a todas las cuentas a las que { $who } lo concedió, y lo que se ejecute con esas cuentas fallará igual.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Donde a { $who } se le deniega (DENY), la revocación levanta la denegación, lo que devuelve el acceso si un rol al que pertenece lo concede.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}En una tabla también elimina todas las concesiones y DENY de { $cleared } a nivel de columna que { $who } tiene sobre las columnas de la tabla: una columna que se le denegaba a { $who } deja de estar denegada, y una que se le concedía por separado deja de estar concedida.
       *[no] {""}
    }
ddl-risk-revoke-role = Retira el rol { $role } a { $member } y, con él, todos los privilegios que lleva el rol.
ddl-risk-blank-password = { $who } no tendrá contraseña: cualquiera que llegue al servidor podrá iniciar sesión con esa cuenta. Establece una aquí, o después con ALTER USER.
ddl-risk-host-is-everywhere = Su host es %: cualquier equipo de la red, no solo este.
ddl-risk-grant-server-wide = Concede { $privileges } a { $who } en todo el servidor: la propia instancia, no la base de datos que se está examinando.
ddl-risk-grant-every-database = Concede { $privileges } a { $who } en todas las bases de datos de este servidor, incluidas las que aún no existen, no solo en la que se está examinando.
ddl-risk-grant-keeps-column-denies = Una concesión de { $privileges } a nivel de tabla elimina cualquier DENY de ello que { $who } tenga en una de las columnas de la tabla, así que la sentencia vuelve a denegar esas columnas a { $who } justo después de conceder: una columna que se le denegaba a { $who } sigue denegada. Si { $who } también tiene la opción de concesión sobre la tabla, el servidor se niega a volver a denegarlo sin CASCADE, y no se aplica nada.
ddl-risk-star-refresh = Actualiza todas las vistas y funciones en línea de esta base de datos que hacen select * from { $table }, directamente o a través de un sinónimo: SQL Server las vincula a las columnas de la tabla por posición, y de lo contrario leerían y escribirían las columnas equivocadas. El plan falla si alguna de ellas ya no compila con las nuevas columnas. Las de otra base de datos que hacen select * from { $table } se actualizan después de que se confirme el plan, fuera de su transacción: hasta entonces leen las posiciones antiguas, y si su actualización falla, el plan sigue aplicado y la vista se indica aquí.
ddl-risk-piecewise = SQL Server no crea, modifica ni elimina un módulo compilado de forma nativa dentro de una transacción, así que este plan ejecuta las sentencias de una en una y no se puede detener: si una sentencia falla, las anteriores siguen aplicadas.
ddl-risk-drop-sequence-table = Elimina la secuencia: se pierde el valor que había alcanzado, y todo lo que siga tomando valores de ella fallará.
ddl-risk-drop-scale = Según las estadísticas del árbol del esquema, son { $rows } filas.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = la propuesta es para la tabla `{ $proposed }`, pero está abierta `{ $actual }`
propose-no-ops = la propuesta no pide ningún cambio
propose-no-such-column = `{ $table }` no tiene ninguna columna llamada `{ $name }`
propose-column-exists = `{ $table }` ya tiene una columna llamada `{ $name }`
propose-no-such-index = `{ $table }` no tiene ningún índice llamado `{ $name }`
propose-index-exists = `{ $table }` ya tiene un índice llamado `{ $name }`
propose-no-such-foreign-key = `{ $table }` no tiene ninguna clave foránea llamada `{ $name }`
propose-no-such-check = `{ $table }` no tiene ninguna restricción CHECK llamada `{ $name }`
propose-key-width-mismatch = una clave foránea necesita tantas columnas referenciadas como columnas que hacen referencia
propose-empty-index = un índice necesita al menos una columna
propose-invalid = la tabla propuesta no sería válida: { $problem }
propose-not-a-table = `{ $name }` es una vista: una propuesta solo puede cambiar una tabla
propose-no-such-table = no hay ninguna tabla llamada `{ $name }`: llama a list_schema para ver qué existe
propose-not-an-expression = { $field ->
        [default] el valor predeterminado `{ $text }` no es una única expresión SQL
        [generated] la expresión generada `{ $text }` no es una única expresión SQL
       *[check] la expresión de comprobación `{ $text }` no es una única expresión SQL
    }
propose-not-a-type = `{ $text }` no es un tipo de columna: indica solo el tipo, sin valor predeterminado, sin NOT NULL y sin nada detrás
propose-unknown-action = `{ $text }` no es una acción de clave foránea
propose-not-loaded = { $table } no está cargada ahora mismo en { $database }: abre la base de datos en el árbol del esquema, o espera a que termine una actualización, y vuelve a intentarlo.
propose-nothing-to-change = { $subject } ya tiene ese aspecto: no hay nada que cambiar.
propose-no-database = Esta pestaña no tiene ninguna base de datos seleccionada, así que un cambio de esquema aplicado desde aquí no tendría adónde ir. Elige primero una para esta conexión; si todas las bases de datos están ocultas, el ojo de ESQUEMA es donde volver a mostrar una.
propose-other-connection = Este cambio se refiere a una pestaña de otra conexión: cambia a ella, o selecciona su conexión en el árbol, antes de aplicar un cambio de esquema.
ai-run-no-database = Esta pestaña no tiene ninguna base de datos seleccionada, así que una sentencia ejecutada desde aquí no tendría adónde ir. Elige primero una para esta conexión; si todas las bases de datos están ocultas, el ojo de ESQUEMA es donde volver a mostrar una.
ai-run-other-connection = Este chat se refiere a una pestaña de otra conexión. Una pestaña nueva se abriría en la conexión seleccionada en el árbol, así que la sentencia se ejecutaría en otro sitio: cambia antes a esa pestaña, o selecciona su conexión en el árbol.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, que este plan renombra a { $to }
ddl-moved-dropped = { $name }, que este plan elimina
ddl-withheld-unreplayable-rename = { $count ->
        [one] Renombrar { $was } a { $to } no puede llevarse consigo su { $count } disparador: su SQL nombra { $was }, y reescribirlo aquí cambiaría lo que escribiste. Renombra la vista por separado después de volver a crear los disparadores con el nuevo nombre.
       *[other] Renombrar { $was } a { $to } no puede llevarse consigo sus { $count } disparadores: su SQL nombra { $was }, y reescribirlo aquí cambiaría lo que escribiste. Renombra la vista por separado después de volver a crear los disparadores con el nuevo nombre.
    }
ddl-withheld-numbered-group = { $name } encabeza un grupo de procedimientos numerados ({ $members }), y este plan lo elimina primero, lo que elimina con él a todos los miembros del grupo. Edítalo con su propio nombre y tipo, lo que lo modifica en su sitio y los conserva.
ddl-withheld-view-index-create = { $count ->
        [one] Crear la vista { $view } no puede crear su índice { $indexes }, de un tipo que Schemaic no puede reformular. Crea esta vista en SQL.
       *[other] Crear la vista { $view } no puede crear sus índices { $indexes }, de un tipo que Schemaic no puede reformular. Crea esta vista en SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Editar la vista { $view } elimina su índice { $indexes }, que Schemaic no puede volver a crear, por ser de un tipo que Schemaic no puede reformular. Edita esta vista en SQL.
       *[other] Editar la vista { $view } elimina sus índices { $indexes }, que Schemaic no puede volver a crear, por ser de un tipo que Schemaic no puede reformular. Edita esta vista en SQL.
    }
ddl-withheld-tsql-lossy-index = Reconstruir { $table } eliminaría el índice { $index }: es de un tipo que Schemaic no lee entero
ddl-withheld-tsql-encrypted-trigger = Reconstruir { $table } eliminaría el disparador { $trigger }: su texto está cifrado
ddl-withheld-tsql-signed-trigger = Reconstruir { $table } eliminaría la firma del disparador { $trigger }, que no se puede restaurar sin la clave privada del certificado
ddl-withheld-tsql-renamed-trigger = Reconstruir { $table } con un nombre nuevo dejaría el disparador { $trigger } nombrando el antiguo
ddl-withheld-partly-read = El índice { $index } solo se ha leído en parte: SQLite guarda el predicado de un índice parcial, y una clave con expresión, solo en el propio texto CREATE del índice{ $why ->
        [may-name] , y ese texto puede nombrar { $moved }. Elimina el índice para continuar sin él.
        [not-found] , así que no se puede crear a partir de lo leído. Elimina el índice para continuar sin él.
        [edited] , así que esta edición no se puede escribir: lo leído no es el índice completo. Deshaz el cambio para conservar el índice tal como está, o elimínalo.
       *[no-text] , y este no tiene ese texto, así que volver a crearlo cambiaría lo que indexa. Elimina el índice para continuar sin él.
    }
ddl-withheld-condition-names-dropped = La condición del índice { $index } nombra { $column }, que este plan elimina. Edita la condición o elimina primero el índice.
ddl-withheld-strands-triggers = Esta tabla tiene { $count ->
        [one] { $count } disparador
       *[other] { $count } disparadores
    }, y su SQL se reproduce exactamente como lo guardó SQLite, que seguiría nombrando { $moved }. SQLite acepta un disparador así y después rechaza toda escritura en la tabla. Elimina o edita primero el disparador, o renombra la columna sin ningún otro cambio para que el propio ALTER TABLE de SQLite pueda reescribirlo.
ddl-withheld-strands-other-trigger = { $owner } nombra { $moved }. Una reconstrucción renombra esta tabla por debajo de ese disparador y deja su SQL exactamente como estaba, así que el disparador seguiría nombrando una columna que ya no existe: SQLite acepta un disparador así y después rechaza toda escritura en *esa* tabla. Elimina o edita primero el disparador, o renombra la columna sin ningún otro cambio para que el propio ALTER TABLE de SQLite pueda reescribirlo.
ddl-withheld-trigger-on = El disparador { $name } en { $table }
ddl-withheld-some-trigger = Un disparador de otra tabla
ddl-withheld-strands-generated-column = La columna generada { $column } se calcula a partir de { $moved }. Una reconstrucción escribe la tabla a partir del modelo, y nada reescribe esa expresión: SQLite rechaza la tabla nueva con «no such column» y todo el plan se revierte. Edita o quita primero { $column }.
ddl-withheld-cannot-restate = La declaración de esta tabla lleva { $clauses }, para lo que el modelo de Schemaic no tiene campo. Una reconstrucción vuelve a escribir la tabla a partir de ese modelo, así que la cláusula desaparecería de una tabla que seguiría pareciendo correcta, y toda escritura que dependa de ella se comportaría de otra forma a partir de entonces. Cambia esta tabla con un script (Copiar DDL es el comienzo de uno), o quita primero la cláusula.
ddl-sqlite-clauses = { $first } y { $last }
ddl-sqlite-clause-deferrable = la cláusula DEFERRABLE de una clave foránea
ddl-sqlite-clause-on-conflict = una cláusula ON CONFLICT
ddl-sqlite-clause-desc-key = una columna DESC de clave primaria
ddl-sqlite-clause-collate-key = un COLLATE en una columna de clave primaria
ddl-withheld-virtual-table = `{ $name }` es una tabla virtual. Sus columnas son la interfaz de un módulo y no una declaración de almacenamiento, y sus datos reales viven en tablas sombra que una reconstrucción eliminaría: un índice FTS5 desaparecería de una tabla que seguiría conteniendo el texto. Cámbiala con un script (DROP y CREATE VIRTUAL TABLE), o edita las tablas que indexa.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } vista
   *[other] { $count } vistas
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } rutina
   *[other] { $count } rutinas
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } disparador
   *[other] { $count } disparadores
}
ddl-used-by-count-event = { $count ->
    [one] { $count } evento
   *[other] { $count } eventos
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } clave foránea
   *[other] { $count } claves foráneas
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } expresión de tabla
   *[other] { $count } expresiones de tabla
}
ddl-used-by-counts = { $first } y { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}y { $more } más
    }
ddl-used-by-drop = Se usa en { $listing }.
ddl-used-by-change = { $subject } se usa en { $listing }. { $then ->
        [renamed] Tras el cambio de nombre seguirán nombrando { $name }.
        [dropped] Dejarán de funcionar cuando se elimine.
       *[retyped] Compruébalos con su nuevo tipo.
    }
ddl-used-by-column = La columna { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Las claves foráneas entre estas tablas forman un ciclo, así que ningún orden de creación las satisface todas. Se rechazará una sentencia por hacer referencia a una tabla que aún no existe, y ni MySQL ni MariaDB revierten el DDL.
ddl-risk-clash = { $holder } aún contiene { $names }, que necesita la creación de { $claimer }, { $why ->
        [not-dropped] y este plan no lo elimina
        [default] y no se puede eliminar primero porque el valor predeterminado de una columna toma valores de ello
       *[referenced] y no se puede eliminar primero porque otra tabla hace referencia a ello
    }. { $cost ->
        [no-rollback] Se rechazará una sentencia, y ni MySQL ni MariaDB revierten el DDL.
        [rolls-back] Se rechazará una sentencia, lo que revierte toda la migración.
       *[refused] Se rechazará una sentencia.
    }

preview-title = Aplicar cambios en { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } cambio
   *[other] { $count } cambios
}
preview-used-by = Usado por
preview-withheld = Este motor no puede expresar parte de este plan
preview-withheld-hint = No se aplica nada mientras esto aparezca.
preview-omitted = No incluido en esta migración
preview-omitted-hint = Al aplicar, estos objetos se quedan como están.
preview-copy = Copiar
preview-open-in-editor = Abrir en el editor
preview-back = Atrás
preview-apply = Aplicar
preview-applying = Aplicando…
preview-applied = { $count ->
    [one] Se ha aplicado { $count } sentencia en { $subject }.
   *[other] Se han aplicado { $count } sentencias en { $subject }.
}
preview-refreshed = Se ha actualizado el esquema.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Importar conexiones
conn-import-url = URL de conexión
conn-import-add = Añadir
conn-import-url-hint = Una URL de conexión, una URL JDBC o una línea DATABASE_URL de un .env, una por línea.
conn-import-choose-file = Elegir un archivo…
conn-import-scan = Buscar clientes instalados
conn-import-scanning = Buscando…
conn-import-docker = Contenedores de Docker
conn-import-asking-docker = Consultando a Docker…
conn-import-looking = Buscando conexiones en este equipo…
conn-import-none-found = No se han encontrado conexiones. DBeaver, los IDE de JetBrains y los clientes de línea de comandos guardan sus archivos en ubicaciones conocidas; si el tuyo está en otro sitio, elígelo arriba.
conn-import-nothing-yet = Aún no se ha encontrado nada: pega una URL, lee un archivo o busca los clientes que ya usas.
conn-import-found = { $count ->
    [one] Se ha encontrado { $count } conexión
   *[other] Se han encontrado { $count } conexiones
}
conn-import-select-all = Seleccionar todo
conn-import-select-none = Ninguna
conn-import-note-no-password = Sin contraseña en el origen
conn-import-note-already-saved = Ya guardada
conn-import-note-unexpanded-path = La ruta contiene una macro sin expandir
conn-import-note-pgpass = Contraseña tomada de tu propio .pgpass
conn-import-note-port-assumed = El origen no indicaba puerto; este es el predeterminado
conn-import-note-external-login = Inicia sesión de una forma que Schemaic no admite: configura un inicio de sesión SQL
conn-import-skipped = { $count ->
    [one] No se ha importado { $count } entrada: { $names }.
   *[other] No se han importado { $count } entradas: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] No se ha importado { $count } entrada: { $names } y { $rest } más.
   *[other] No se han importado { $count } entradas: { $names } y { $rest } más.
}
conn-import-reason-unsupported-engine = motor no compatible ({ $driver })
conn-import-reason-no-server = no hay servidor al que conectar
conn-import-reason-not-published = su puerto no está publicado en este equipo
conn-import-reason-unreadable = no se ha podido leer ({ $error })
conn-import-reason-not-an-export = no se ha podido leer (no es una exportación de conexiones de Schemaic: { $error })
conn-import-reason-newer-export = no se ha podido leer (exportada por un Schemaic más reciente, versión { $version }: actualiza para importarla)
conn-import-url-empty = Pega una URL de conexión.
conn-import-url-no-scheme = Falta el esquema: una URL empieza por mysql://, postgresql://, sqlserver:// o sqlite:, y una cadena de conexión de SQL Server, por Server=.
conn-import-url-unknown-scheme = Esquema desconocido «{ $scheme }»: se esperaba mysql, mariadb, postgresql, sqlserver o sqlite.
conn-import-url-no-host = La URL no tiene host.
conn-import-url-bad-port = «{ $port }» no es un número de puerto.
conn-import-url-no-path = No hay archivo de base de datos después de sqlite:.
conn-import-url-transport = A «{ $server }» se llega mediante canalizaciones con nombre o LocalDB, y Schemaic solo conecta por TCP.
conn-import-url-userinfo = El nombre del servidor contiene una @: el nombre de usuario o la contraseña van en su propio campo, no en el servidor.
conn-import-url-not-sql-server = Esta cadena de conexión no indica que sea de SQL Server: los controladores .NET de MySQL y PostgreSQL usan las mismas palabras clave. Pega en su lugar la URL del servidor (mysql://, postgresql://, sqlserver://) o, para SQL Server, escribe el servidor como tcp:host.
conn-import-row-overflow = Se muestran las primeras { $shown } de { $total }. Las otras { $hidden } también se importan si están marcadas: la lista se detiene aquí para que la ventana siga respondiendo.
conn-import-import = Importar
conn-import-import-count = Importar { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = Imagen PNG
blob-kind-jpeg = Imagen JPEG
blob-kind-gif = Imagen GIF
blob-kind-bmp = Imagen BMP
blob-kind-webp = Imagen WebP
blob-kind-ico = Icono
blob-kind-opaque = Datos binarios
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · se muestran los primeros { $shown }
blob-pane-preview = Vista previa
blob-pane-hex = Hex
blob-reading = Leyendo…
blob-empty = Esta celda es NULL, o su fila ya no existe.
blob-preview-too-large = Esta imagen mide { $width } × { $height }: es demasiado grande para mostrarla aquí. Los bytes están intactos: léelos en Hex o guárdalos en un archivo.
blob-preview-truncated = Solo se han leído los primeros { $shown } de { $key ->
    [png] esta imagen PNG
    [jpeg] esta imagen JPEG
    [gif] esta imagen GIF
    [bmp] esta imagen BMP
    [webp] esta imagen WebP
    [ico] este icono
   *[other] este valor
}, así que no se puede dibujar. Lee los bytes en Hex.
blob-preview-unreadable = Estos bytes empiezan como { $key ->
    [png] una imagen PNG
    [jpeg] una imagen JPEG
    [gif] una imagen GIF
    [bmp] una imagen BMP
    [webp] una imagen WebP
    [ico] un icono
   *[other] una imagen
}, pero no se pueden leer como tal. Léelos en Hex o guárdalos en un archivo.
blob-save-truncated = Solo se han podido leer los primeros { $shown } de { $size }, así que esto guardaría el principio del valor.
blob-load = Cargar desde archivo
blob-save = Guardar en archivo
blob-load-dialog = Cargar valor binario desde archivo
blob-save-dialog = Guardar valor binario
blob-saved-to = Guardado en { $path }
blob-loaded-file = Archivo cargado. Confirma en la cuadrícula para escribirlo.
blob-not-writable = No se puede escribir en esta celda.
blob-file-too-large = Ese archivo ocupa { $got }, y esta columna admite como máximo { $most }.
blob-not-accepting = Esa celda ya no acepta valores: no se ha dejado nada pendiente.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = Diagrama ER
erd-scope = Ámbito
erd-chip-tables = { $count ->
    [one] { $count } tabla
   *[other] { $count } tablas
}
erd-chip-relationships = { $count ->
    [one] { $count } relación
   *[other] { $count } relaciones
}
erd-chip-hidden = { $count ->
    [one] { $count } sin relación oculta
   *[other] { $count } sin relación ocultas
}
erd-find-placeholder = Buscar tabla o columna
erd-find-none = Sin coincidencias
erd-find-matches = { $hits ->
    [one] { $hits } coincidencia
   *[other] { $hits } coincidencias
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } coincidencia en { $tables } tablas
   *[other] { $hits } coincidencias en { $tables } tablas
}
erd-more-columns = +{ $count } más
erd-show-less = mostrar menos
erd-not-loaded = El esquema de esta base de datos aún no se ha cargado.
erd-no-relationships = No hay relaciones de clave foránea que representar.
erd-export-tip = Exportar el diagrama…
erd-copy-as = Copiar como
erd-export-dialog = Exportar diagrama
erd-nothing-to-export = No hay nada que exportar.
erd-nothing-to-copy = No hay nada que copiar.
erd-copied-as = Copiado como { $format }
erd-clipboard-unavailable = El portapapeles no está disponible.
erd-copy-failed = No se ha podido copiar: { $error }
erd-saved = Guardado { $name }
erd-unnamed-file = el archivo

## Server variables modal (server_vars_view.rs)

server-vars-title = Variables del servidor — { $connection }
server-vars-search = Buscar ajustes
server-vars-variables = Variables
server-vars-status = Estado
server-vars-loading = Cargando…
server-vars-unsupported = El motor de esta conexión no publica ninguna lista de este tipo.
server-vars-no-match = No hay ajustes que coincidan.
server-vars-more = { $count ->
    [one] Coincide { $count } ajuste más: acota la búsqueda
   *[other] Coinciden { $count } ajustes más: acota la búsqueda
}
server-vars-count = { $count ->
    [one] { $count } ajuste
   *[other] { $count } ajustes
}
server-vars-count-filtered = { $shown } de { $count } ajustes
server-vars-refresh = Actualizar

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Editar fragmento — { $name }
snippet-edit-name = Nombre
snippet-edit-name-placeholder = Nombre del fragmento
snippet-edit-shortcut = Atajo de expansión
snippet-edit-shortcut-none = ninguno
snippet-edit-query = Consulta
snippet-edit-save = Guardar

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Plan de consulta
plan-analyze-unavailable = Analyze no disponible (la sentencia escribe datos)
plan-analyze-hint = Ejecuta la sentencia para medir los tiempos reales
plan-explaining = Explicando
plan-potential-issues = Posibles problemas
plan-detail-no-index = no se usa ningún índice
plan-detail-key-unused = el índice no se usa para la búsqueda
plan-my-full-scan = Recorrido completo de la tabla `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Recorrido completo del resultado ({ $detail }, type = ALL)
plan-my-filesort = Filesort en `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort en el resultado (Extra: Using filesort)
plan-my-temporary = Tabla temporal para `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Tabla temporal para el resultado (Extra: Using temporary)
plan-pg-seq-scan = Recorrido secuencial de `{ $table }` (no se usa ningún índice)
plan-pg-seq-scan-unnamed = Recorrido secuencial de una tabla (no se usa ningún índice)
plan-pg-sort = Nodo Sort: la consulta ordena las filas sobre la marcha
plan-ms-full-scan = Recorrido completo de `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Recorrido completo de una tabla ({ $op })
plan-ms-sort = Sort: la consulta ordena las filas sobre la marcha
plan-ms-spool = { $op }: una tabla de trabajo intermedia en tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = sin predicado de join: cada fila se empareja con todas las filas
plan-server-no-statistics = una columna que lee no tiene estadísticas
plan-server-spill = se ha desbordado a tempdb: su concesión de memoria era demasiado pequeña
plan-server-convert = una conversión de tipo afecta al plan ({ $issue }): { $expression }
plan-server-unmatched-indexes = no se ha podido usar un índice filtrado
plan-server-memory-grant = su concesión de memoria se calculó mal
plan-server-wait = ha esperado en { $wait }
plan-server-other = el servidor advierte: { $name }
plan-missing-index = Falta un índice en `{ $table }` ({ $keys })
plan-missing-index-include = Falta un índice en `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Falta un índice en `{ $table }` ({ $keys }): el servidor esperaba una mejora del { $impact } %
plan-missing-index-include-impact = Falta un índice en `{ $table }` ({ $keys }; include { $include }): el servidor esperaba una mejora del { $impact } %
plan-ask-ai = Preguntar a la IA
plan-ms-operation = Operación
plan-ms-object = Objeto
plan-ms-estimated-rows = Filas estimadas
plan-ms-estimated-cost = Coste estimado
plan-ms-actual-rows = Filas reales
plan-ms-executions = Ejecuciones
plan-ms-statement = Sentencia { $n }: { $kind }
plan-ms-statement-in-procedure = Sentencia { $n }: { $kind } en el procedimiento { $name }
plan-ms-statement-in-function = Sentencia { $n }: { $kind } en la función { $name }
plan-ms-statement-in-a-procedure = Sentencia { $n }: { $kind } en un procedimiento
plan-ms-statement-in-a-function = Sentencia { $n }: { $kind } en una función
plan-ms-plan = Plan
plan-ms-no-plan = Ninguna sentencia de este lote tiene plan.
plan-ms-unread = No se ha podido leer el plan: { $reason }
plan-ms-unread-not-xml = no es un documento XML
plan-ms-unread-not-showplan = se esperaba ShowPlanXML y se ha encontrado { $found }
plan-ms-unread-empty = un documento vacío

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Usado por — { $object }
used-by-copy = Copiar
used-by-not-loaded = El esquema de { $database } aún no se ha cargado: despliégalo en el árbol y vuelve a preguntar.
used-by-nothing-in = Nada en { $database } lo usa.
used-by-nothing = Nada lo usa.
used-by-limits = Encontrado leyendo las definiciones de esta base de datos: no se ve el SQL construido en tiempo de ejecución ni otras bases de datos.
used-by-dropped-with-it = { $detail } · se elimina con él
used-by-heading-view = { $count ->
    [one] Vista ({ $count })
   *[other] Vistas ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Rutina ({ $count })
   *[other] Rutinas ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Disparador ({ $count })
   *[other] Disparadores ({ $count })
}
used-by-heading-event = { $count ->
    [one] Evento ({ $count })
   *[other] Eventos ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Clave foránea ({ $count })
   *[other] Claves foráneas ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Expresión de tabla ({ $count })
   *[other] Expresiones de tabla ({ $count })
}
used-by-noun-view = vista
used-by-noun-routine = rutina
used-by-noun-trigger = disparador
used-by-noun-event = evento
used-by-noun-foreign-key = clave foránea
used-by-noun-expression = expresión de tabla

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Oscuro
theme-light = Claro

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = CÓDIGO
md-code-copy = Copiar
md-code-insert = Insertar
md-code-run = Ejecutar
md-proposal-title = Cambio propuesto en { $table }
md-proposal-count = { $count ->
    [one] { $count } cambio
   *[other] { $count } cambios
}
md-proposal-review = Revisar el cambio…
md-proposal-unreadable = El asistente ha propuesto un cambio que no se ha podido leer: { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Elegir…
cell-calendar-month = { $month } de { $year }
cell-calendar-now = Ahora
cell-calendar-today = Hoy
cell-month-1 = enero
cell-month-2 = febrero
cell-month-3 = marzo
cell-month-4 = abril
cell-month-5 = mayo
cell-month-6 = junio
cell-month-7 = julio
cell-month-8 = agosto
cell-month-9 = septiembre
cell-month-10 = octubre
cell-month-11 = noviembre
cell-month-12 = diciembre
cell-weekday-mon = Lu
cell-weekday-tue = Ma
cell-weekday-wed = Mi
cell-weekday-thu = Ju
cell-weekday-fri = Vi
cell-weekday-sat = Sá
cell-weekday-sun = Do

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Renombrar alias
alias-rename-title-cte = Renombrar CTE
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = Nombre de la CTE
alias-rename-new-name = Nuevo nombre
alias-rename-rename = Renombrar

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Aquí ya no hay nada que renombrar.
alias-rename-empty = Ponle un nombre.
alias-rename-conflict = «{ $name }» ya es un nombre en esta consulta: renombrar a él cambiaría la tabla que lee una referencia.
alias-rename-unsure = Una referencia de aquí puede leer otra columna con este nombre, o leer esta donde el cambio de nombre no puede seguirla, y el esquema cargado no permite saberlo, así que no se hace ninguna edición. Carga el esquema de las tablas o renombra a mano.
alias-rename-changed = La consulta ha cambiado mientras esto estaba abierto. Ciérralo y vuelve a renombrar.
alias-rename-read-only = El editor es de solo lectura hasta que se acepte o se rechace la sugerencia de la IA.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } uso de { $name }
   *[other] { $count } usos de { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = SVG no válido: { $error }
raster-too-large = La imagen es demasiado grande para rasterizarla ({ $width }×{ $height })
raster-png-failed = Ha fallado la codificación PNG: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Copiar
grid-menu-copy-value = Copiar valor
grid-menu-copy-formatted = Copiar con formato
grid-menu-copy-as = Copiar como
grid-menu-paste = Pegar
grid-menu-fill-down = Rellenar hacia abajo
grid-menu-fill-up = Rellenar hacia arriba
grid-menu-edit-field = Editar campo
grid-menu-edit-row = Editar fila
grid-menu-view-row = Ver fila
grid-menu-edit-binary = Editar binario
grid-menu-view-binary = Ver binario
grid-menu-filter-by-value = Filtrar por este valor
grid-menu-exclude-value = Excluir este valor
grid-menu-follow-relation = Seguir la relación
grid-menu-referenced-by = Referenciada por
grid-menu-set-null = Poner a NULL
grid-menu-set-default = Poner a DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Duplicar fila
   *[other] Duplicar { $count } filas
}
grid-menu-delete-rows = { $count ->
    [one] Eliminar fila
   *[other] Eliminar { $count } filas
}
grid-menu-undo-delete = Deshacer eliminación
grid-menu-ai-summary = Resumen con IA
grid-attach-to-chat = Adjuntar { $scope } al chat
grid-attach-scope-rows = { $rows ->
    [one] { $rows } fila
   *[other] { $rows } filas
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } columna
   *[other] { $cols } columnas
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } fila
   *[other] { $rows } filas
} y { $cols ->
    [one] { $cols } columna
   *[other] { $cols } columnas
}
grid-menu-freeze = Inmovilizar
grid-menu-unfreeze = Movilizar
grid-menu-hide-column = Ocultar columna
grid-menu-show-hidden-columns = Mostrar columnas ocultas ({ $count })
grid-menu-show-all = Mostrar todo
grid-menu-format-as = Formato
grid-format-none = Ninguno
grid-format-timestamp = Marca de tiempo
grid-format-grouped = Separador de miles
grid-format-bytes = Tamaño de archivo
grid-format-bool = Booleano
grid-menu-copy-name = Copiar nombre
grid-menu-copy-qualified-name = Copiar nombre completo
grid-menu-copy-values = Copiar valores
grid-menu-sql-in-list = Lista SQL IN

## The results grid's toolbar (grid.rs) — the status line, the commit and row
## controls, and the copy, save, AI and chart icons.

grid-stats = { $rows } · { $cols ->
    [one] { $cols } col.
   *[other] { $cols } cols.
} · { $ms } ms
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } col.
   *[other] { $cols } cols.
} · { $ms } ms
grid-rows-read = { $n ->
    [one] { $read } fila
   *[other] { $read } filas
}
grid-rows-read-capped = { $n ->
    [one] { $read } fila
   *[other] { $read } filas
} (limitado)
grid-rows-read-of-total = { $n ->
    [one] { $read } de { $total } fila
   *[other] { $read } de { $total } filas
}
grid-kept-note = · conservado: solo lectura
grid-arena-note = · { $columns } demasiado grande para conservarlo entero: las filas posteriores aparecen en blanco
grid-sorted-subset = · subconjunto ordenado (limitado): no es el orden completo
grid-read-all = leer todas las filas
grid-read-more = leer { $rows } filas
grid-reading = leyendo…
grid-committing = Confirmando…
grid-commit-tip = { $count ->
    [one] Confirmar { $count } cambio ({ $keys })
   *[other] Confirmar { $count } cambios ({ $keys })
}
grid-discard-tip = Descartar todos los cambios pendientes
grid-copy-sql-tip = Copiar los cambios como SQL
grid-add-row-tip = Añadir una fila
grid-delete-row-tip = Marcar la fila seleccionada para eliminarla ({ $key })
grid-clone-row-tip = Clonar la fila seleccionada
grid-copy-results-tip = Copiar los resultados…
grid-copy-results-empty = No hay nada que copiar: no hay filas
grid-save-results-tip = Guardar los resultados en un archivo…
grid-save-results-empty = No hay nada que guardar: no hay filas
grid-ai-tip = Generar filas con IA…
grid-ai-generating = Generando…
grid-ai-off = Las acciones de IA envían datos: desactivadas en esta conexión
grid-ai-fill-value = Rellenar valor con IA
grid-ai-insert-row = Insertar fila con IA
grid-ai-seed-table = Poblar tabla con IA
grid-chart-tip = Representar los resultados en un gráfico
grid-chart-empty = No hay nada que representar: no hay filas
grid-chart-this-result = este resultado

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Ver
grid-rollback-tab = Revertir { $tab }
grid-find-placeholder = Buscar en los resultados
grid-figure-copied = Copiado
grid-figure-rows = { $n ->
    [one] { $n }{ $more } fila
   *[other] { $n }{ $more } filas
}
grid-figure-nulls = { $n }{ $more } nulos
grid-figure-distinct = { $n } distintos
grid-figure-sum = Suma { $value }
grid-figure-avg = Media { $value }
grid-figure-median = Mediana { $value }
grid-figure-min = Mín. { $value }
grid-figure-max = Máx. { $value }
grid-figure-length = { $n ->
    [one] Longitud { $n } carácter
   *[other] Longitud { $n } caracteres
}
grid-figure-length-range = Longitud { $lo }–{ $hi } caracteres
grid-goto-label = Ir a la fila:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = el portapapeles
export-dest-file = el archivo
export-note-rows = { $n ->
    [one] Se ha exportado { $rows } fila a { $name }
   *[other] Se han exportado { $rows } filas a { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } contiene bytes que este formato no puede transportar: esas celdas quedan vacías
   *[other] { $cols } contienen bytes que este formato no puede transportar: esas celdas quedan vacías
}
export-note-blanked = { $cols } demasiado grande para conservarlo entero: las filas posteriores quedan en blanco
export-note-cut = { $cols } recortado al límite de { $limit } caracteres por celda de Excel
export-failure-partial = { $message }; { $name } no se ha modificado, y las filas que se escribieron están en { $part }
export-cancelled-partial = Exportación cancelada: { $name } no se ha modificado, y las filas que se escribieron están en { $part }
export-cancelled-nothing = Exportación cancelada: { $name } no se ha modificado.
copy-sql-as-null = { $n ->
    [one] Copiado al portapapeles; { $cols } contiene bytes que el texto SQL no puede transportar: se escriben como NULL
   *[other] Copiado al portapapeles; { $cols } contienen bytes que el texto SQL no puede transportar: se escriben como NULL
}
copy-sql-nothing = { $n ->
    [one] No se ha copiado nada: { $cols } contiene bytes que el texto SQL no puede transportar
   *[other] No se ha copiado nada: { $cols } contienen bytes que el texto SQL no puede transportar
}
copy-sql-left-out = { $n ->
    [one] Copiado al portapapeles; { $cols } contiene bytes que el texto SQL no puede transportar: esos valores se omiten
   *[other] Copiado al portapapeles; { $cols } contienen bytes que el texto SQL no puede transportar: esos valores se omiten
}

grid-fill-nothing = No hay nada que rellenar: las columnas seleccionadas no se pueden escribir, o la celda que copiar es el valor sin asignar de una fila nueva o un DEFAULT pendiente.
grid-paste-outside = { $n } fuera de la cuadrícula
grid-paste-read-only = { $n } en columnas de solo lectura
grid-paste-capped = { $n } por encima del límite de pegado de { $cap } celdas
grid-paste-deleted = { $n } en filas marcadas para eliminar
grid-paste-nothing = No se ha pegado nada: { $notes }.
grid-paste-partial = { $count ->
        [one] Se ha pegado { $cells } celda; se han omitido { $notes }.
       *[other] Se han pegado { $cells } celdas; se han omitido { $notes }.
    }
grid-row-not-editable = La columna `{ $column }` no es editable.
grid-row-not-nullable = La columna `{ $column }` no puede ser NULL.
grid-fill-skipped = { $count ->
    [one] Se ha rellenado { $filled } celda; se han omitido { $skipped } en filas marcadas para eliminar.
   *[other] Se han rellenado { $filled } celdas; se han omitido { $skipped } en filas marcadas para eliminar.
}
grid-attach-summary = { $rows ->
    [one] { $rows } fila
   *[other] { $rows } filas
} × { $cols ->
    [one] { $cols } columna
   *[other] { $cols } columnas
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } fila
   *[other] { $rows } filas
} × { $cols ->
    [one] { $cols } columna
   *[other] { $cols } columnas
} de { $table }
grid-attach-summary-capped = { $summary } (las primeras { $taken } de { $total } seleccionadas)
grid-copy-sql-unset = No hay nada que copiar como SQL: las celdas seleccionadas de la fila nueva aún no tienen valor, y toman el predeterminado del servidor.
grid-copy-sql-joined = No hay nada que copiar como SQL: estas filas proceden de una consulta sobre más de una tabla.
grid-copy-sql-no-keyed-table = No hay nada que copiar como SQL: las filas deben proceder de una sola tabla con clave.
grid-copy-sql-key-too-large = No hay nada que copiar como SQL: la clave era demasiado grande para que este resultado la conservara entera, así que algunas filas no se pueden identificar con ella.
grid-copy-sql-pinned = No hay nada que copiar como SQL: este resultado está fijado, así que sus filas son una instantánea.
grid-copy-sql-read-only = No hay nada que copiar como SQL: esta conexión es de solo lectura.
grid-copy-update-too-large = No hay nada que copiar como UPDATE: las columnas seleccionadas eran demasiado grandes para que este resultado las conservara enteras, y algunas de sus celdas aparecen en blanco.
grid-copy-update-nothing-writable = No hay nada que copiar como UPDATE: ninguna de las columnas seleccionadas se puede escribir.
grid-copy-where-in-no-key = No hay nada que copiar como WHERE IN: esta tabla no tiene clave primaria ni única con la que identificar sus filas.
grid-copy-new-row-no-key = No hay nada que copiar: una fila nueva no tiene clave hasta que se guarda.
grid-export-fetched-rows = Filas obtenidas ({ $count })
grid-export-dialog-title = Exportar resultados
grid-export-already-running = Ya hay una exportación en curso: no se ha escrito nada.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Fila { $row }
grid-row-title-in-table = Fila { $row }  ·  { $table }
grid-row-set-value = Establecer valor
grid-row-set-null = Poner a NULL
grid-row-edit-binary = Editar
grid-row-view-binary = Ver
grid-row-invalid-json = JSON no válido: { $error }
grid-row-saving = Guardando

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } fila afectada · { $ms } ms
   *[other] { $count } filas afectadas · { $ms } ms
}
grid-rerun-would-write = No se puede volver a ejecutar esta sentencia: escribiría en la base de datos
grid-filter-not-simple = No se puede filtrar esta consulta: no es un SELECT simple de una sola tabla
grid-filter-bracket-in-name = aquí no se puede filtrar un nombre que contenga `]`
grid-filter-trailing-input = entrada inesperada después de la condición
grid-staged-requery = Confirma o descarta antes tus cambios: volver a leer las filas los perdería
grid-staged-landing = Se dejaron cambios pendientes mientras se volvía a leer la tabla, así que se han conservado y la tabla no se ha actualizado: confírmalos o descártalos y después actualiza.
grid-reread-without-filter = El filtro llama a una función que Schemaic no vuelve a ejecutar sin que se lo pidas, así que tras la confirmación la tabla se ha vuelto a leer sin el filtro ni el orden.
grid-hidden-columns-tip = { $count ->
    [one] { $count } columna oculta: haz clic para mostrar todas, o haz clic derecho en una cabecera para elegir
   *[other] { $count } columnas ocultas: haz clic para mostrar todas, o haz clic derecho en una cabecera para elegir
}
grid-copied-staged-sql = Se han copiado los cambios pendientes como SQL.
grid-copy-split = { $n ->
        [one] { $n } celda copiada contiene un tabulador o un salto de línea: al pegarla se leerá como más de una celda.
       *[other] { $n } celdas copiadas contienen un tabulador o un salto de línea: al pegarlas, cada una se leerá como más de una celda.
    }
grid-duplicate-title = Duplicar filas
grid-duplicate-question = { $n ->
    [one] ¿Duplicar { $rows } fila? Queda como fila pendiente y no se escribe nada hasta que confirmes.
   *[other] ¿Duplicar { $rows } filas? Quedan como filas pendientes y no se escribe nada hasta que confirmes.
}
grid-nullable-tip = Admite NULL
grid-cell-auto = <auto>
grid-cell-required = <obligatorio>
grid-seed-title = Poblar filas
grid-seed-generate = Generar

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Esquema
footer-panel-ai = Asistente de IA
footer-panel-history = Historial de consultas
footer-panel-snippets = Biblioteca de fragmentos
footer-panel-activity = Actividad del servidor
footer-panel-terminal = Terminal
footer-panel-narrow = La ventana es demasiado estrecha para este panel: ensánchala para que vuelva a aparecer.
footer-cursor = Lín. { $line }, col. { $col }
footer-indent-spaces = Espacios: { $width }
footer-indent-tabs = Tabuladores: { $width }
footer-spaces = Espacios
footer-tabs = Tabuladores
footer-wrap = Ajuste de línea
footer-no-wrap = Sin ajuste de línea
footer-read-only = Solo lectura
footer-write-mode = Modo escritura
footer-tx-auto = Confirmación automática
footer-tx-manual = Manual
footer-tx-open = Abierta: { $count }
footer-tx-aborted = Transacción abortada: revierte para continuar
footer-tx-lost = Transacción perdida
footer-commit = Confirmar
footer-rollback = Revertir
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Consulta { $n }
lib-result-default-label = Resultado { $n }
lib-ai-off-note = El asistente de IA está desactivado. Elige una CLI del agente en los ajustes de IA para activarlo.
lib-test-testing = Probando…
lib-test-connected = Conectado
lib-test-failed = La conexión ha fallado

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Mínimo
lib-effort-low = Bajo
lib-effort-medium = Medio
lib-effort-high = Alto
lib-effort-extra = Extra
lib-effort-max = Máximo
lib-schema-scope-active = Solo la base de datos activa
lib-schema-scope-all = Todas las bases de datos
lib-schema-scope-none = Ninguno
lib-cursor-block = Bloque
lib-cursor-bar = Barra
lib-cursor-underline = Subrayado

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Sin conexión
header-new-connection = Nueva conexión
header-find-anywhere-tip = Buscar en todo ({ $keys })
header-shortcuts-tip = Atajos de teclado
header-settings-tip = Ajustes
header-disconnected = Desconectado
header-retry = Reintentar
colour-red = Rojo
colour-orange = Naranja
colour-amber = Ámbar
colour-green = Verde
colour-teal = Verde azulado
colour-blue = Azul
colour-purple = Morado
colour-pink = Rosa

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = «{ $name }» ha cambiado en el disco.
disk-reload = Recargar
disk-keep-mine = Conservar la mía
results-run-hint = Ejecuta una consulta  ({ $keys })
results-failed = La sentencia ha fallado.
results-cancelled = Consulta cancelada.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = { $count } fijados · { $size }
results-pinned-tip = Los resultados fijados se mantienen en memoria hasta que los cierras. Cierra los que ya no necesites para liberarla.
results-chip-nothing-run = Aún no se ha ejecutado nada en esta pestaña.
results-chip-kept = conservado: solo lectura
results-menu-pin = Fijar
results-menu-unpin = Desfijar
results-menu-close = Cerrar
results-menu-compare-with = Comparar con { $result }
results-menu-close-others = Cerrar los demás resultados
results-menu-close-all = Cerrar todos los resultados

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Buscar en todo

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = No hay nada que elegir

widgets-spinner-verbs = Cavilando, Rumiando, Maquinando, Tramando, Hilvanando, Pergeñando, Barruntando, Elucubrando, Fermentando, Horneando, Destilando, Macerando, Marinando, Amasando, Sazonando, Aliñando, Sofriendo, Flambeando, Gratinando, Glaseando, Batiendo, Removiendo, Pululando, Deambulando, Merodeando, Zascandileando, Trasteando, Cacharreando, Enredando, Garabateando, Bosquejando, Cincelando, Forjando, Tejiendo, Urdiendo, Bordando, Zurciendo, Desenredando, Meditando, Ponderando, Sopesando, Calibrando, Calculando, Computando, Cuadrando, Deliberando, Dilucidando, Descifrando, Desentrañando, Conjeturando, Especulando, Filosofando, Divagando, Fantaseando, Imaginando, Improvisando, Ideando, Inventando, Orquestando, Componiendo, Armonizando, Afinando, Tarareando, Canturreando, Bailoteando, Correteando, Brincando, Trotando, Galopando, Revoloteando, Orbitando, Levitando, Flotando, Burbujeando, Chisporroteando, Centelleando, Germinando, Brotando, Floreciendo, Polinizando, Incubando, Eclosionando, Anidando, Pastoreando, Cristalizando, Sublimando, Condensando, Transmutando, Metamorfoseando, Materializando, Conjurando, Hechizando, Remolineando, Zigzagueando, Serpenteando, Ronroneando, Rebuscando, Husmeando, Escudriñando, Curioseando, Chapoteando, Procesando, Generando, Elaborando, Confeccionando, Ensamblando, Engrasando, Apañando, Reflexionando, Recapacitando, Lucubrando, Haciendo magia

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Pide ayuda al asistente de IA.
cmdk-accept = Aceptar
cmdk-reject = Rechazar
cmdk-no-changes = No se sugiere ningún cambio
cmdk-diff-summary = { $hunks ->
        [one] { $n } bloque · { $removed } − / { $added } +
       *[other] { $n } bloques · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = valor
params-kind-tip = Texto, número, NULL o SQL sin procesar: haz clic para cambiar
params-missing = Falta el valor de { $names }
params-not-a-number = :{ $name } es un número, pero contiene «{ $text }»

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Ver
editor-run-anyway = Ejecutar de todos modos
guard-read-only = Conexión de solo lectura.
guard-read-only-refused = Conexión de solo lectura: { $reason }
read-only-several = solo se permite una única sentencia
read-only-not-a-read = solo se permiten consultas de solo lectura ({ $heads })
read-only-denied = `{ $word }` no está permitido en una consulta de solo lectura
read-only-advances-sequence = `NEXT VALUE FOR` hace avanzar una secuencia, algo que una consulta de solo lectura no hace
read-only-unlisted-call = `{ $name }` no es una función que se sepa de solo lectura, así que no está permitida en una consulta de solo lectura; también se rechazan las funciones de extensiones o definidas en la base de datos
read-only-locks = `{ $clause }` bloquea las filas que lee, algo que una consulta de solo lectura no hace; quita la cláusula de bloqueo
guard-no-database = No hay ninguna base de datos seleccionada.
guard-writes-one = Esta sentencia modifica datos.
guard-writes-several = Estas sentencias modifican datos.
guard-script = ¿Ejecutar todas las sentencias de { $file }? Un script puede crear, modificar y eliminar objetos, y lo que cambie no se puede deshacer desde aquí.
guard-staged = { $n ->
    [one] Esta ejecución reemplaza el resultado que contiene { $n } cambio pendiente: ejecutar lo descartaría. Confírmalo o descártalo antes, o ejecuta de todos modos.
   *[other] Esta ejecución reemplaza el resultado que contiene { $n } cambios pendientes: ejecutar los descartaría. Confírmalos o descártalos antes, o ejecuta de todos modos.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL elimina todas las filas de la tabla.
guard-truncate = TRUNCATE elimina todas las filas de la tabla.
guard-no-where = Se ha detectado una sentencia { $kind } sin cláusula WHERE.
guard-drop-table = DROP TABLE elimina la tabla y todas sus filas.
guard-drop-database = DROP DATABASE elimina la base de datos y todas sus tablas.
guard-drop-schema = DROP SCHEMA elimina el esquema y todas sus tablas.
guard-drop-owned = DROP OWNED elimina todo lo que posee el rol, incluidas todas sus tablas con sus filas.
guard-drop-cascade = DROP { $obj } … CASCADE elimina todas las columnas que dependen de ello, con sus valores.
guard-replace-table = CREATE OR REPLACE TABLE elimina la tabla existente y todas sus filas.
guard-replace-database = CREATE OR REPLACE DATABASE elimina la base de datos existente y todas sus tablas.
guard-replace-schema = CREATE OR REPLACE SCHEMA elimina el esquema existente y todas sus tablas.
guard-truncate-partition = TRUNCATE PARTITION elimina todas las filas de las particiones que nombra.
guard-drop-partition = DROP PARTITION elimina la partición y todas sus filas.
quickinfo-table = tabla { $name } · { $database }
quickinfo-view = vista { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } columna
       *[other] { $n } columnas
    }
quickinfo-primary-key = clave primaria
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = autoincremento
quickinfo-default = predeterminado { $value }
quickinfo-generated = generada como { $expression }
quickinfo-function = función { $name }({ $arguments })
quickinfo-function-returns = función { $name }({ $arguments }) devuelve { $returns }
quickinfo-procedure = procedimiento { $name }({ $arguments })
quickinfo-procedure-returns = procedimiento { $name }({ $arguments }) devuelve { $returns }
completion-foreign-key = clave foránea
completion-expand-star = expandir *
completion-columns = { $count ->
        [one] { $n } columna
       *[other] { $n } columnas
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Error de sintaxis
diag-syntax-detail = Error de sintaxis: { $detail }
diag-reserved-alias-backticks = `{ $word }` es una palabra clave reservada y no se puede usar como alias (ponla entre comillas invertidas)
diag-reserved-alias-double-quotes = `{ $word }` es una palabra clave reservada y no se puede usar como alias (ponla entre comillas dobles)
diag-reserved-alias-brackets = `{ $word }` es una palabra clave reservada y no se puede usar como alias (ponla entre corchetes)
diag-table-not-found = No se ha encontrado la tabla `{ $table }`{ $within ->
        [some] {" "}en `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— ¿querías decir `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = No se ha encontrado la columna `{ $column }`{ $within ->
        [some] {" "}en `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— ¿querías decir `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = Aquí `{ $name }` tiene el alias `{ $alias }`: califícalo con `{ $alias }`
diag-ambiguous-column = La columna `{ $column }` es ambigua
diag-join-without-condition = El JOIN no tiene condición ON/USING: es un producto cartesiano; usa CROSS JOIN si es lo que pretendes
diag-comma-join-without-where = Join con coma sin WHERE: es un producto cartesiano; añade un WHERE, o usa CROSS JOIN si es lo que pretendes
diag-not-grouped = La columna `{ $column }` debe aparecer en GROUP BY o usarse en una función de agregado
diag-insert-row-width = INSERT nombra { $columns } { $columns ->
        [one] columna
       *[other] columnas
    }, pero esta fila tiene { $values } { $values ->
        [one] valor
       *[other] valores
    }
diag-row-width-without-list = `{ $table }` admite { $needed } { $needed ->
        [one] valor
       *[other] valores
    } sin lista de columnas, pero esta fila tiene { $values }
diag-row-width-of-table = `{ $table }` tiene { $columns } { $columns ->
        [one] columna
       *[other] columnas
    }, pero esta fila tiene { $values } { $values ->
        [one] valor
       *[other] valores
    }
diag-nullable-not-in = `{ $column }` puede ser NULL, y `NOT IN` no coincide con ninguna fila en cuanto la subconsulta devuelve uno: añádele `WHERE { $column } IS NOT NULL`, o usa NOT EXISTS
diag-update-without-where = UPDATE sin WHERE cambia todas las filas de la tabla
diag-delete-without-where = DELETE sin WHERE elimina todas las filas de la tabla
diag-null-comparison = Una comparación con NULL nunca es verdadera: usa { $fix }
diag-misspelled-keyword = `{ $word }` parece una palabra clave mal escrita
diag-misspelled-function = `{ $word }` parece una función mal escrita
diag-flow-merge-needs-semicolon = `MERGE` debe terminar en `;`
diag-flow-if-without-statement = `IF` no tiene ninguna sentencia que ejecutar
diag-flow-else-without-statement = `ELSE` no tiene ninguna sentencia que ejecutar
diag-flow-while-without-statement = `WHILE` no tiene ninguna sentencia que ejecutar
diag-flow-end-try-without-begin-try = `END TRY` sin su `BEGIN TRY` correspondiente
diag-flow-end-catch-without-begin-catch = `END CATCH` sin su `BEGIN CATCH` correspondiente
diag-flow-end-without-begin = `END` sin su `BEGIN` correspondiente
diag-flow-throw-needs-semicolon = La sentencia anterior a `THROW` debe terminar en `;`
diag-flow-end-try-without-catch = `END TRY` debe ir seguido de `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` no tiene ninguna sentencia antes de su `ELSE`
diag-flow-else-without-if = `ELSE` sin un `IF` delante
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` debe ir después de un `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` no tiene su `END TRY` correspondiente
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` no tiene su `END CATCH` correspondiente
diag-flow-begin-without-end = `BEGIN` no tiene su `END` correspondiente

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Preguntar a la IA
editor-menu-explain = Explicar
editor-menu-optimise = Optimizar
editor-menu-plan = Plan
editor-menu-copy-single-line = Copiar en una sola línea
editor-menu-make-single-line = Pasar a una sola línea
editor-menu-format-statement = Formatear sentencia
editor-menu-find-usages = Buscar usos
editor-menu-create-view = Crear vista
cmdk-optimise-label = Optimizar esta consulta
editor-several-carets = Varios cursores y nada seleccionado: selecciona lo que quieres ejecutar, o deja un solo cursor

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Ejecutar la actual
editor-run-everything = Ejecutar todo

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Buscar
find-invalid = No válida
find-bad-pattern = No es un patrón válido
find-bad-pattern-why = No es un patrón válido: { $why }
find-match-case = Coincidir mayúsculas y minúsculas
find-whole-word = Palabra completa
find-regex = Expresión regular
find-scope = Sel
find-in-selection-tip = Buscar en la selección
find-replace-placeholder = Reemplazar
find-replace = Reemplazar
find-replace-all = Todo
goto-line-label = Ir a:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = No se ha encontrado ningún cliente { $client } (PATH o WSL).
cli-no-client-on-path = No se ha encontrado ningún cliente { $client } en el PATH.
cli-no-tool = No se ha encontrado { $tool } en el PATH.
cli-no-either-tool = No se ha encontrado { $first } ni { $second } en el PATH.
cli-no-either-client = No se ha encontrado ningún cliente { $first } ni { $second } en el PATH.
cli-sqlcmd-windows-in-wsl = El inicio de sesión de Windows necesita un sqlcmd de Windows, y el encontrado está dentro de WSL. Instala sqlcmd en Windows, o abre la conexión en una pestaña de consulta.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = El cliente encontrado en tu PATH es un shim .cmd/.bat, que Windows ejecuta a través de cmd.exe, donde un nombre de base de datos con '&' o '|' se convertiría en un segundo comando. Indica en Ajustes el ejecutable real, o abre una pestaña de consulta.
cli-refused-name-equals = El nombre de esta base de datos contiene '=', que psql leería como una cadena de conexión y seguiría hasta otro servidor. Ábrela en una pestaña de consulta.
cli-refused-name-uri = El nombre de esta base de datos empieza como un URI de conexión, que psql seguiría hasta otro servidor. Ábrela en una pestaña de consulta.
cli-refused-file-dash = El nombre del archivo de esta base de datos empieza por '-', que el cliente sqlite3 leería como una opción y no como el archivo. Renómbralo, o indica una ruta (./name.db), y vuelve a abrirlo.
cli-refused-sqlcmd-entra = A sqlcmd no se le puede pasar el token de inicio de sesión de la Azure CLI, así que esta conexión no se puede abrir en él. Ábrela en una pestaña de consulta.
cli-refused-sqlcmd-host = El host de esta conexión contiene un carácter que sqlcmd lee como parte de la dirección (',', ';', una segunda '\' o un prefijo ':'). Indica el puerto en su propio campo y vuelve a abrirla.
cli-refused-sqlcmd-ca = sqlcmd no puede verificar con un archivo de CA. Añade la CA al almacén de confianza del sistema y vacía el campo de CA de la conexión, o abre la conexión en una pestaña de consulta.
cli-refused-sqlcmd-client-cert = SQL Server no inicia sesión con un certificado de cliente, así que sqlcmd no tiene adónde enviar el de esta conexión. Quita el certificado y la clave de cliente y vuelve a abrirla.
cli-refused-wsl-paths = Las rutas de los certificados TLS de esta conexión son rutas de Windows, que el cliente de WSL no puede abrir. Instala un cliente nativo, o mueve los certificados dentro de WSL.
cli-refused-tunnelled-verify = Esta conexión verifica el certificado del servidor y llega a él a través de un túnel SSH, así que al cliente se le diría que marcara 127.0.0.1 y comprobara el certificado contra esa dirección, algo que el propio certificado del servidor no puede cumplir. Abre una pestaña de consulta, o rebaja el modo TLS para la sesión de la CLI.
cli-refused-not-container-id = docker ps ha devuelto algo que no es un id de contenedor.
cli-refused-file-line-break = Ese nombre de archivo contiene un salto de línea, que el cliente leería como dos argumentos. Elige otro nombre.
cli-refused-role = El rol del SQL de inicio de esta conexión no se puede pasar tal cual a la herramienta.
cli-refused-mysql-source-path = El cliente mysql leería la ruta de ese archivo como comandos propios (contiene un ';' o una '\', o un espacio en un extremo). Muévelo o renómbralo y después restáuralo.
cli-refused-cloud-token = Esta conexión inicia sesión con el token de una CLI de la nube, que no se pasa a un cliente externo. Ábrela en una pestaña de consulta.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = No se puede saber desde dónde se ejecuta Schemaic: { $error }
install-registry-open = No se ha podido abrir tu entorno de usuario en el registro: { $error }
install-path-read = No se ha podido leer tu PATH de usuario: { $error }
install-path-write = No se ha podido escribir tu PATH de usuario: { $error }
install-look-at = No se ha podido examinar { $path }: { $error }
install-read-link = No se ha podido leer el enlace de { $path }: { $error }
install-create = No se ha podido crear { $path }: { $error }
install-remove-old = No se ha podido quitar el antiguo { $path }: { $error }
install-remove = No se ha podido quitar { $path }: { $error }
install-path-is-windows = Una entrada en el PATH de usuario es una instalación de Windows; esta plataforma usa un enlace.
install-link-is-unix = Enlazar el comando es una instalación de macOS y Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = No se puede saber en qué carpeta está { $exe }.
install-refuse-no-shim = No hay ningún schemaic.com junto a esta copia de Schemaic, en { $dir }, así que no hay ningún comando de consola que poner en el PATH. La aplicación instalada tiene uno; una compilación de desarrollo no.
install-refuse-semicolon = Esta copia de Schemaic está en { $dir }, cuyo nombre contiene un ';': el PATH lo leería como dos carpetas distintas. Muévela a un sitio sin ese carácter y vuelve a intentarlo.
install-refuse-exe-replaced = { $exe } se reemplazó mientras Schemaic estaba en ejecución, así que un enlace a él no apuntaría a nada. Reinicia Schemaic y vuelve a pulsar Instalar.
install-refuse-no-home = No hay ningún directorio personal que pueda contener el enlace, así que no hay dónde buscar.
install-refuse-path-not-text = Tu PATH de usuario no está guardado como texto en el registro, así que no se ha tocado.
install-refuse-path-not-unicode = Tu PATH de usuario contiene caracteres que no se pueden volver a leer con seguridad, así que no se ha tocado.
install-refuse-portable = Esta es una copia portátil, así que Quitar no puede saber si fue Instalar quien puso { $dir } en tu PATH o si ya estaba allí con otros programas. Quítalo en Variables de entorno si lo añadió Instalar.
install-refuse-spot-taken = { $link } ya existe y no es un enlace, así que Instalar no lo ha tocado. Apártalo y vuelve a intentarlo.
install-refuse-points-elsewhere = { $link } ya apunta a { $to }. Quítalo si es una copia antigua de Schemaic y vuelve a intentarlo.
install-refuse-unlink-elsewhere = { $link } apunta a { $to }, no a esta copia de Schemaic, así que Quitar no lo ha tocado.
install-refuse-unlink-not-ours = { $link } no es un enlace creado por Schemaic, así que Quitar no lo ha tocado.
install-report-already = El comando schemaic ya está en tu PATH, en { $dir }.
install-report-added = Se ha añadido { $dir } a tu PATH de usuario. Abre un terminal nuevo para usar schemaic: uno que ya esté abierto conserva el PATH antiguo.
install-report-linked = Se ha enlazado { $link } a { $target }.
install-report-linked-if-not-found = Se ha enlazado { $link } a { $target }. Si un terminal nuevo no encuentra schemaic, añade { $dir } a tu PATH en el perfil de tu shell.
install-report-linked-not-on-path = Se ha enlazado { $link } a { $target }. { $dir } aún no está en tu PATH: añádelo en el perfil de tu shell y abre un terminal nuevo.
install-report-removed = Se ha quitado { $dir } de tu PATH de usuario. Un terminal que ya esté abierto conserva el PATH antiguo.
install-report-path-absent = { $dir } no está en tu PATH de usuario, así que no había nada que quitar.
install-report-unlinked = Se ha quitado { $link }.
install-report-link-absent = No existe { $link }, así que no había nada que quitar.
install-report-package = El comando schemaic de { $dir } venía con el paquete instalado, no de Instalar: desinstala el paquete para quitarlo.
install-hint-adds-folder = Ejecuta Schemaic desde un terminal por su nombre. Añade su carpeta a tu PATH de usuario.
install-hint-links = Ejecuta Schemaic desde un terminal por su nombre. Lo enlaza en ~/.local/bin.
install-hint-folder-on-path = Ejecuta Schemaic desde un terminal por su nombre. Su carpeta, { $dir }, ya está en tu PATH de usuario.
install-hint-comes-with-app = Ejecuta Schemaic desde un terminal por su nombre. El comando schemaic viene con la aplicación, en { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = El túnel SSH aún no está establecido
tunnel-could-not-open = No se ha podido abrir el túnel SSH: { $error }. Actualiza el esquema para volver a intentarlo.
tunnel-server-failed = No se ha podido conectar a través del túnel SSH: { $error }. Actualiza el esquema para volver a intentarlo.

gate-plan-not-connected = Sin conexión: no se ha ejecutado el plan. El servidor no ha respondido a la comprobación de estado, así que no se le ha enviado nada.
gate-plan-connection-changed = Has cambiado de conexión mientras Schemaic comprobaba que se podía acceder a ella, así que no se ha ejecutado el plan. Vuelve a cambiar y pregunta de nuevo.
gate-plan-tab-moved-on = Has cambiado de pestaña mientras se volvía a comprobar la conexión, así que no se ha ejecutado el plan. Describe la pestaña desde la que se pidió: vuelve a esa pestaña y pregunta de nuevo.
gate-connection-changed = No se ha ejecutado en { $name }: has cambiado de conexión mientras Schemaic comprobaba que se podía acceder a ella. Vuelve a cambiar e inténtalo de nuevo.
gate-not-connected = Sin conexión con { $name }. El servidor no ha respondido: comprueba que esté en marcha y que los ajustes de esta conexión sean correctos.
gate-this-connection = esta conexión
gate-run-tab-moved-on = Has cambiado de pestaña mientras se volvía a comprobar la conexión, así que no se ha ejecutado la sentencia. Se comprobó con la pestaña en la que se escribió, y ejecutarla aquí la ejecutaría en otro sitio. Vuelve a esa pestaña y ejecútala de nuevo.
gate-commit-not-connected = Sin conexión: no se ha intentado la confirmación. Los cambios pendientes se conservan.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = No se ha podido iniciar { $program }: { $error }. Elige otro shell en Ajustes, Terminal.
term-cli-read-only = Esta conexión es de solo lectura en Schemaic. Su cliente de línea de comandos no lo sería, así que no se abre aquí.
backup-read-only-server = Esta conexión es de solo lectura en Schemaic, y la copia de seguridad de este motor la escribe el servidor, así que no se ha hecho ninguna.
backup-dialog-title = Hacer copia de seguridad de la base de datos
backup-kind-pg-dump = Volcado de PostgreSQL en formato personalizado
backup-kind-sql = Script SQL
backup-kind-sqlite = Base de datos SQLite
backup-not-started = La copia de seguridad no se ha iniciado: { $error }
backup-done = Copia de seguridad guardada en { $path }.
backup-failed = La copia de seguridad ha fallado: { $error }
restore-title = Restaurar desde archivo
restore-confirm = ¿Ejecutar { $path } en { $database } de { $connection }? Lo que cree o reemplace no se puede deshacer. Se ejecuta en el terminal, donde Ctrl+C lo detiene.
restore-read-only = Esta conexión es de solo lectura en Schemaic, así que no se restaura nada en ella.
restore-engine-not-here = Las copias de seguridad de este motor no se restauran desde aquí.
restore-unreadable = No se ha podido leer { $path } antes de restaurarlo, así que no se ha ejecutado nada: { $error }
restore-refused = { $path } cambia a otra base de datos mientras se ejecuta ({ $switches }), así que, restaurado en { $database }, escribiría en otra. No se ha ejecutado nada. Restáuralo en la base de datos que nombra, o quita esas líneas de una copia del archivo.
restore-refused-more = { $path } cambia a otra base de datos mientras se ejecuta ({ $switches } y { $more } más), así que, restaurado en { $database }, escribiría en otra. No se ha ejecutado nada. Restáuralo en la base de datos que nombra, o quita esas líneas de una copia del archivo.
restore-switch = { $how } `{ $name }` en la línea { $line }
restore-switch-unnamed = un { $how } en la línea { $line } que no nombra ninguna base de datos que Schemaic pueda leer

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Cerrar «{ $name }»
tab-close-unsaved = «{ $name }» tiene cambios sin guardar. Al cerrar se descartan; el archivo del disco se queda como está. ¿Cerrar de todos modos?
tab-close-all-title = Cerrar todas las pestañas
tab-close-all-question = ¿Seguro que quieres cerrar todas las pestañas?
tab-close-others-title = Cerrar las demás pestañas
tab-close-others-question = ¿Seguro que quieres cerrar todas las demás pestañas?
file-open-title = Abrir archivo SQL
file-save-title = Guardar archivo SQL
file-sql-filter = Script SQL
file-read-failed = No se ha podido leer { $path }: { $error }
file-save-failed = No se ha podido guardar { $path }: { $error }
file-too-large = { $path } ocupa { $size }: es demasiado grande para abrirlo en una pestaña del editor. Schemaic pasaría la mayor parte del tiempo volviendo a analizarlo. Ejecútalo desde una pestaña de consulta, o usa Importar para un archivo de datos.
file-too-large-short = El archivo es demasiado grande para abrirlo.
file-large-title = ¿Abrir un archivo grande?
file-large-question = «{ $name }» ocupa { $size }. Schemaic vuelve a analizar todo el documento poco después de cada pausa al escribir, así que un archivo de este tamaño hace que el editor responda con lentitud mientras la pestaña esté abierta. ¿Abrirlo de todos modos?
file-changed-on-disk = { $path } ha cambiado en el disco desde que se abrió. Guardar ahora descartaría esos cambios: vuelve a cargar el archivo (o usa Guardar como con otro nombre).
file-lossy-title = ¿Reemplazar los bytes ilegibles?
file-lossy-question = Schemaic no ha podido leer todos los bytes de «{ $name }» como texto y los ha mostrado como «�». Al guardar se escribe lo que ves, así que cada uno de ellos se reemplaza de forma permanente, en todo el archivo y no solo en las líneas que has editado. ¿Guardar de todos modos?
file-replace-title = ¿Reemplazar el archivo?
file-replace-question = «{ $name }» ya existe: «{ $typed }» se ha guardado añadiendo la extensión .sql. ¿Reemplazarlo?
file-reload-title = Recargar desde el disco
file-reload-unsaved = «{ $name }» tiene cambios sin guardar. Al recargar se descartan. ¿Recargar de todos modos?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Ya hay una exportación en curso. Cancélala o espera a que termine.
export-failed = La exportación ha fallado: { $error }
export-worker-died = La exportación ha fallado: el proceso de trabajo ha terminado: { $error }
export-not-published = La exportación ha fallado: no se ha podido publicar { $path }: { $error }
export-not-renamed = La exportación ha escrito { $part }, pero no ha podido renombrarlo a { $path }: { $error }
export-nothing-matched = No hay nada que exportar: ninguna tabla coincide con la selección.
export-stopped = Exportación cancelada.
dump-failed = El volcado ha fallado: { $error }
blob-load-too-large = Ese archivo ocupa { $size }, y lo máximo que se puede cargar es { $most }.
blob-load-failed = No se ha podido cargar: { $error }
blob-save-failed = No se ha podido guardar: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = No se ha podido escribir { $path }: { $error }
settings-export-title = Exportar ajustes
settings-import-title = Importar ajustes
settings-file-filter = Ajustes de Schemaic
settings-import-nothing = No hay nada que importar: todos los ajustes de { $path } ya coinciden con los tuyos, y no añade ningún fragmento.
settings-import-question = { $path } cambia { $changed ->
        [one] { $changed } ajuste
       *[other] { $changed } ajustes
    } y añade { $snippets ->
        [one] { $snippets } fragmento
       *[other] { $snippets } fragmentos
    }. Los ajustes que cambia sustituyen tus valores actuales; los fragmentos se añaden junto a los tuyos.
settings-import-unreadable = no se ha podido leer: { $error }
connections-export-title = Exportar conexiones
connections-file-filter = Conexiones de Schemaic
connections-replace-title = Reemplazar «{ $file }»
connections-replace-question = Ya hay un archivo llamado «{ $file }» en esa carpeta. La exportación añade esa terminación al nombre que escribiste, así que el diálogo de guardado no preguntó por él. Reemplazarlo no se puede deshacer.
import-choose-file-title = Elegir un archivo de conexiones
import-no-containers = No hay ningún contenedor de base de datos en ejecución.
import-added = { $count ->
        [one] Se ha añadido { $count } conexión.
       *[other] Se han añadido { $count } conexiones.
    }
import-added-foreign = { $count ->
        [one] Se ha añadido { $count } conexión.
       *[other] Se han añadido { $count } conexiones.
    } Solo lectura y la etiqueta de entorno no se trasladan desde otro cliente: configúralas en Gestionar conexiones para todo lo que importe.
conn-delete-title = Eliminar «{ $name }»
conn-delete-question = Esto elimina la conexión, su contraseña guardada, su conversación de IA guardada, su historial de consultas y sus pestañas. No se puede deshacer.
conn-delete-keyring-kept = Schemaic no ha podido acceder al llavero del sistema, así que la contraseña guardada y los secretos SSH de la conexión eliminada **siguen en él**.
conn-delete-keyring-later = Se quitarán la próxima vez que se elimine o se guarde una conexión mientras el llavero esté accesible. Mientras tanto, ninguna conexión nueva puede recibirlos: los ids de conexión nunca se reutilizan.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = No se ha podido leer { $name } ({ $error }).
notice-unreadable-kept = El archivo ilegible se ha conservado como { $name }.corrupt; Schemaic ha recurrido a su copia de seguridad, o a los valores predeterminados si la copia tampoco se podía leer.
notice-unreadable-swept = Esa copia puede contener contraseñas sin cifrar, así que se quitará la próxima vez que elimines una conexión: recupera de ella lo que necesites antes de eso.
notice-missing = Faltaba { $name }.
notice-missing-restored = Schemaic lo ha recuperado de { $name }{ $from }, que seguía ahí. Se volverá a escribir la próxima vez que cambie este ajuste; hasta entonces, esa copia es la única.
notice-keyring-unread = { $n ->
    [one] Schemaic no ha podido leer { $n } secreto guardado del llavero del sistema.
   *[other] Schemaic no ha podido leer { $n } secretos guardados del llavero del sistema.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic no ha podido leer { $n } secreto guardado del llavero del sistema ({ $error }).
   *[other] Schemaic no ha podido leer { $n } secretos guardados del llavero del sistema ({ $error }).
}
notice-keyring-unread-kept = Esas contraseñas no están disponibles en esta sesión y **no** se han eliminado: deja los campos en blanco y volverán en cuanto el llavero esté accesible. Escribir una nueva sobrescribe el secreto guardado.
notice-in-the-clear = { $n ->
    [one] Schemaic no ha podido acceder al llavero del sistema, así que { $n } contraseña se ha guardado **en texto sin cifrar** en `connections.json`.
   *[other] Schemaic no ha podido acceder al llavero del sistema, así que { $n } contraseñas se han guardado **en texto sin cifrar** en `connections.json`.
}
notice-in-the-clear-where = Ese archivo está en la carpeta que abre el botón *Abrir carpeta* de Ajustes → General, junto al registro: revísalo antes de compartirlo. Volver a guardar cuando el llavero esté accesible las devuelve a él.
notice-undeleted = { $n ->
    [one] { $n } contraseña borrada sigue en el llavero del sistema: Schemaic no ha podido eliminarla.
   *[other] { $n } contraseñas borradas siguen en el llavero del sistema: Schemaic no ha podido eliminarlas.
}
notice-undeleted-after = El campo está en blanco en el disco, pero el próximo inicio lo volverá a rellenar. Vuelve a guardar cuando el llavero esté accesible.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = No se ha devuelto SQL
inline-ai-not-sql = El modelo no ha devuelto SQL
inline-ai-failed = la generación ha fallado
ai-fill-no-value = La IA no ha devuelto ningún valor.
conn-gone = la conexión ya no existe
tx-session-not-ready = la conexión de la transacción no está lista: cambia a Confirmación automática y vuelve
tx-session-open-failed = no se ha podido abrir una conexión de transacción: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = Se ha cancelado la sentencia.
tx-committed-before-ddl = La transacción en la que se ejecutó esto ya no existe: MySQL confirma antes de ejecutar una sentencia DDL, así que las sentencias que ya contenía son permanentes y Revertir no las deshará.
tx-rolled-back-by-server = La transacción en la que se ejecutó esto ya no existe: SQL Server la ha revertido, así que se deshacen todas las sentencias que contenía desde que empezó.
tx-doomed = La transacción en la que se ejecutó esto ya no se puede confirmar: SQL Server la ha marcado como no confirmable, así que Revertir es la única salida, y deshace todas las sentencias que contiene.
tx-after-own-commit = La transacción en la que se ejecutó esto ha terminado, y esto ejecutó su propio COMMIT antes de fallar, así que lo anterior a ese COMMIT probablemente se haya confirmado y no deshecho. Revisa los datos antes de volver a ejecutar nada de ello.
tx-lost-on-commit = Se ha perdido la conexión con una transacción abierta. Se perdió durante un COMMIT, así que no se sabe si { $stmts ->
        [0] la transacción se confirmó
        [one] la { $stmts } sentencia ejecutada en ella desde que empezó se confirmó
       *[other] las { $stmts } sentencias ejecutadas en ella desde que empezó se confirmaron
    } o se revirtió: puede que el servidor confirmara antes de que se cortara la conexión. Revisa los datos antes de volver a ejecutar nada de ello.
tx-lost-on-implicit-commit = Se ha perdido la conexión con una transacción abierta. MySQL confirma una transacción abierta antes de ejecutar esta sentencia, así que { $stmts ->
        [0] aún no se había ejecutado nada en ella y no había nada que confirmar
        [one] la { $stmts } sentencia ejecutada en ella desde que empezó probablemente se confirmó entonces, y es permanente
       *[other] las { $stmts } sentencias ejecutadas en ella desde que empezó probablemente se confirmaron entonces, y son permanentes
    }. Revisa los datos antes de volver a ejecutar nada de ello.
tx-lost-rolled-back = Se ha perdido la conexión con una transacción abierta. El servidor revierte una transacción sin terminar cuando su conexión se cierra, así que { $stmts ->
        [0] aún no se había ejecutado nada en ella
        [one] la { $stmts } sentencia ejecutada en ella desde que empezó se ha deshecho
       *[other] las { $stmts } sentencias ejecutadas en ella desde que empezó se han deshecho
    }.
tx-lost-rolled-back-mysql = Se ha perdido la conexión con una transacción abierta. El servidor revierte una transacción sin terminar cuando su conexión se cierra, así que { $stmts ->
        [0] aún no se había ejecutado nada en ella
        [one] la { $stmts } sentencia ejecutada en ella desde que empezó se ha deshecho, salvo una escritura en una tabla que no admita transacciones (MyISAM, MEMORY), que ninguna reversión deshace
       *[other] las { $stmts } sentencias ejecutadas en ella desde que empezó se han deshecho, salvo una escritura en una tabla que no admita transacciones (MyISAM, MEMORY), que ninguna reversión deshace
    }.
tx-not-sent-timeout = El tiempo de espera de la sentencia se agotó mientras esta ejecución aún esperaba la conexión de la pestaña. Una operación anterior en ella (una confirmación, o la nueva lectura posterior) no había terminado, así que la sentencia nunca se envió al servidor. No se ha ejecutado nada, y la transacción está exactamente donde estaba.
tx-not-sent-cancelled = Esta ejecución se canceló mientras aún esperaba la conexión de la pestaña. Una operación anterior en ella (una confirmación, o la nueva lectura posterior) no había terminado, así que la sentencia nunca se envió al servidor. No se ha ejecutado nada, y la transacción está exactamente donde estaba.
snippet-default-name = Fragmento
snippet-delete-title = Eliminar fragmento
snippet-delete-question = ¿Eliminar «{ $name }»? No se puede deshacer.
copy-target-read-only = La conexión de destino es de solo lectura, así que no se ha copiado ninguna fila.
copy-target-gone = La conexión de destino ya no existe, así que no se ha copiado ninguna fila.
copy-not-started = No se ha copiado ninguna fila: { $error }
copy-counter-not-moved = { $count ->
        [one] Se ha copiado { $count } fila, pero el contador de la tabla no se ha avanzado más allá de ella, así que la próxima inserción puede colisionar: { $error }
       *[other] Se han copiado { $count } filas, pero el contador de la tabla no se ha avanzado más allá de ellas, así que la próxima inserción puede colisionar: { $error }
    }
copy-stopped = La copia se ha detenido por un error: { $error }
compare-reading-failed = Leyendo { $database }: { $error }
monitor-no-row-key = Esta tabla no tiene clave de fila: no se pueden seguir los cambios.
monitor-restarted = Las columnas de la tabla han cambiado, así que el seguimiento de cambios se ha reiniciado desde aquí.
timeout-cancelled-app = Cancelada: la sentencia ha superado el tiempo de espera de { $timeout }. Cámbialo o desactívalo en Ajustes → Consulta.
timeout-cancelled-connection = Cancelada: la sentencia ha superado el tiempo de espera de { $timeout }. Cámbialo o desactívalo en los ajustes de esta conexión.
ai-chat-no-database = No se puede acceder a la base de datos, así que no se puede iniciar el asistente para esta conexión. Comprueba la conexión y vuelve a intentarlo.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **No se ha podido iniciar el asistente.** Schemaic da a cada proceso de agente un directorio de trabajo privado propio, y ni el directorio temporal del sistema ni el de configuración han admitido uno, así que no hay dónde ejecutarlo que no sea el directorio de otro. Comprueba que se pueda escribir en el directorio temporal y vuelve a intentarlo.
ai-no-reply-file = No se ha podido crear un archivo temporal para la respuesta.
ai-inline-no-opencode-config = No se ha podido escribir la configuración de OpenCode, así que no se ha iniciado la generación (sin ella, la CLI se ejecutaría con sus propias herramientas activadas).
ai-inline-no-copilot-home = No se ha podido crear el directorio personal privado de GitHub Copilot, así que no se ha iniciado la generación (sin él, la CLI cargaría tu propia configuración de Copilot, incluidos sus servidores MCP y sus hooks).
ai-inline-no-env-writer = { $cli } está marcado como aislado por su entorno, pero Schemaic no tiene forma de preparar ese entorno, así que no se ha iniciado la generación.
ai-inline-no-cursor-rules = No se ha podido escribir el archivo de permisos de Cursor, así que no se ha iniciado la generación (sin él, la CLI escribe archivos sin preguntar).
ai-inline-no-rules-writer = { $cli } está marcado como restringido por un archivo de permisos, pero Schemaic no tiene forma de escribirle uno, así que no se ha iniciado la generación.
ai-no-tools = **Esta sesión no tiene herramientas de base de datos.** { $cause }, así que el asistente no puede consultar nada: solo puede responder a partir del esquema resumido que ya lleva su instrucción. Cerrar y volver a abrir el panel lo intentará de nuevo.
ai-no-endpoint-file = Schemaic no ha podido crear el archivo privado que indica al asistente cómo llegar a tu base de datos
ai-cursor-not-approved = Schemaic no ha podido aprobar sus herramientas de base de datos con Cursor ({ $error })
ai-antigravity-not-registered = Schemaic no ha podido registrar sus herramientas de base de datos en Antigravity
ai-antigravity-shared = Otra ventana de Schemaic ya está usando Antigravity. Antigravity mantiene un único registro MCP para todo el equipo, que solo puede apuntar a una conexión, así que el asistente de esta ventana no tiene herramientas de base de datos
ai-antigravity-not-claimed = Schemaic no ha podido reservar la configuración de Antigravity para esta sesión
ai-refused-no-opencode-config = Schemaic no ha podido escribir la configuración que restringe OpenCode, así que el asistente está desactivado: ejecutarlo sin ese archivo daría a la sesión un shell. Comprueba que se pueda escribir en el directorio de datos de la aplicación.
ai-refused-no-copilot-home = Schemaic no ha podido crear el directorio personal privado de GitHub Copilot, así que el asistente está desactivado: sin él, la CLI cargaría tu propia configuración de Copilot, incluidos sus servidores MCP y sus hooks. Comprueba que se pueda escribir en el directorio de datos de la aplicación.
ai-refused-no-cursor-rules = Schemaic no ha podido escribir el archivo de permisos que impide a Cursor escribir archivos y ejecutar comandos, así que el asistente está desactivado: sin él, Cursor escribe archivos sin preguntar. Comprueba que se pueda escribir en el directorio de datos de la aplicación.
ai-launch-failed-turn = No se ha podido iniciar la CLI `{ $bin }` ({ $error }). Asegúrate de que { $cli } esté instalado, o indica su ruta en Ajustes → IA.
ai-launch-failed-session = No se ha podido iniciar la CLI `{ $bin }` ({ $error }). Asegúrate de que { $cli } esté instalado (o indica su ruta en Ajustes → IA).
ai-turn-ended = El turno de { $cli } ha terminado inesperadamente: { $error }
ai-session-ended-badly = La sesión de IA ha terminado inesperadamente: { $error }
ai-session-ended = La sesión de { $cli } ha terminado. Vuelve a preguntar para iniciar una nueva.
ai-session-restart-failed = La sesión de { $cli } no se ha podido reiniciar tras detenerse ({ $error }), así que ha terminado. Vuelve a preguntar para iniciar una nueva.
ai-session-not-accepting = La sesión de { $cli } ya no acepta preguntas. Vuelve a preguntar para iniciar una nueva.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = El contexto enviado a { $cli } es demasiado grande para una sola línea de comandos ({ $total } { $unit }; esta plataforma admite unos { $limit }). { $levers }
ai-unit-characters = caracteres
ai-unit-bytes = bytes
ai-levers-schema-and-query = Limita Ajustes → IA → Contexto del esquema a la base de datos activa (o a Ninguno), o acorta la consulta del editor.
ai-levers-sample = Baja el acceso de la IA a los datos de esta conexión por debajo de «Dejar que lea datos», lo que deja de enviar la muestra de filas, o limita Ajustes → IA → Contexto del esquema.
ai-batch-shim = { $cli } se ha resuelto como un shim por lotes ({ $bin }), al que no se le puede pasar una instrucción de varias líneas: Windows ejecuta un .cmd a través de cmd.exe, que rechaza el argumento antes de que se inicie { $cli }. Indica el ejecutable real en Ajustes → IA → Ruta de la CLI (una instalación con npm suele tener uno en node_modules, junto al shim).
ai-cli-exited = la CLI de { $cli } ha terminado con el estado { $code }
ai-cli-signalled = la CLI de { $cli } ha terminado por una señal
ai-constraint-sandboxed = { $cli } se ejecuta en un entorno aislado: sus herramientas integradas no pueden escribir archivos ni ejecutar comandos, aunque sí pueden leer este equipo.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Los servidores MCP que hayas registrado en él también están disponibles en esta sesión, y Schemaic no puede restringirlos.
    }
ai-constraint-workspace-rules = { $cli } se limita a responder gracias a un archivo de permisos que Schemaic escribe para esta sesión, no a un entorno aislado: sus propias herramientas tienen prohibido escribir archivos y ejecutar comandos, pero pueden seguir leyendo archivos de este equipo.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Los servidores MCP que hayas registrado en él también se inician con esta sesión; sus herramientas se rechazan salvo que tus propios ajustes las permitan.
    }
ai-constraint-denylist = Esta versión de { $cli } no acepta la opción que vacía sus herramientas integradas, así que se contienen mediante una lista de denegación, algo más débil y que Schemaic no puede garantizar. Actualizar la CLI restaura el aislamiento completo.
ai-constraint-unrestricted = Schemaic no ha podido restringir por completo las herramientas integradas de { $cli } en esta sesión, así que puede que sea capaz de hacer algo más que responder preguntas sobre esta base de datos.
ai-constraint-unknown = Schemaic no ha podido confirmar que este binario de { $cli } acepte las opciones que lo restringen, así que el asistente está desactivado. Comprueba la ruta en Ajustes → IA.
ai-seed-empty = el modelo no ha devuelto ningún contenido
ai-seed-not-json = la respuesta no era JSON válido: { $error }
ai-seed-not-rows = la respuesta no era una matriz JSON de objetos de fila

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } no es un archivo.
import-source-too-big = { $path } ocupa { $size }, más de los { $most } que se leen.
import-source-unreadable = No se ha podido leer { $path }: { $error }
docker-not-found = No se ha encontrado Docker, ni en el PATH ni en WSL.
docker-not-started = No se ha podido iniciar Docker: { $error }
docker-timed-out = Docker no ha respondido en { $secs } segundos.
docker-not-waited = No se ha podido esperar a Docker: { $error }
docker-answered = Docker ha respondido: { $error }
docker-exited = Docker ha terminado con { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = el lector de archivos se ha detenido inesperadamente: { $error }
script-changed-since-read = Este archivo ha cambiado en el disco después de leerse, así que el resumen de arriba no es lo que se ejecutaría. Vuelve a elegirlo.
script-no-terminator = Este archivo no contiene ningún terminador de sentencia, así que no es un script SQL que Schemaic pueda ejecutar. Comprueba que has elegido el archivo correcto.
ai-cursor-enable-unfinished = `mcp enable` no ha terminado
