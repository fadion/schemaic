# Schemaic — Bulgarian catalogue.
#
# Machine-drafted and unreviewed (docs/architecture.md, The string catalogue); a message missing here
# falls back to the en-GB catalogue. Shipped (locale::SHIPPED): offered in
# Settings, and drawn for a system set to Bulgarian.

## Settings › Appearance › Language

settings-language = Език
settings-language-system = Системен ({ $language })
settings-language-restart = Смяната на езика ще влезе в сила при следващото стартиране на Schemaic.

## Settings › Terminal

settings-term-shell = Обвивка
settings-term-font-size = Размер на шрифта
settings-term-cursor-style = Стил на курсора
settings-term-copy-on-select = Копиране при маркиране
settings-term-copy-on-select-hint = Маркираният текст се копира в клипборда в момента, в който маркирането приключи.
settings-term-blink = Мигащ курсор
settings-term-blink-hint = Курсорът мига, докато терминалът е на фокус.

## Settings › AI

settings-ai-agent = CLI на агента
settings-ai-disabled = Изключено
settings-ai-off-hint = Нищо не се изпраща към агент, не се стартира CLI на агент и не се предлагат действия с ИИ. Изберете CLI, за да включите асистента.
settings-ai-cli-path = Път до { $cli }
settings-ai-cli-path-placeholder = Оставете празно за автоматично откриване
settings-ai-detected = Открит автоматично: { $path }
settings-ai-detect-failed = Автоматичното откриване не успя. { $cli } не е намерен — посочете пътя по-горе.
settings-ai-cli-path-missing = Файлът не съществува.
settings-ai-model = Модел
settings-ai-model-placeholder = Оставете празно за модела по подразбиране на средата
settings-ai-model-hint = Всеки идентификатор, който този CLI приема — псевдоним или датирана версия, за да остане същият модел и след обновления.
settings-ai-model-hint-with-chips = Всеки идентификатор, който този CLI приема — псевдоним или датирана версия, за да остане същият модел и след обновления. Бутоните са преки пътища, а не целият списък.
settings-ai-effort = Усилие
settings-ai-instructions = Собствени инструкции
settings-ai-instructions-placeholder = Диалект, конвенции, вътрешни правила…
settings-ai-schema-context = Контекст от схемата
settings-ai-schema-context-hint = Колко от структурата на базата данни се изпраща с всяко съобщение. „Без“ спира и инструментите за схемата, така че асистентът ще ви пита за имената, вместо сам да чете каталога.
settings-ai-data-access = Достъп до данни
settings-ai-data-access-where = Задава се за всяка връзка поотделно, в настройките на връзката.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
ai-data-schema-only = Само схема
ai-data-on-request = Само каквото прикача
ai-data-full = Да чете данни
ai-data-schema-only-hint = Само структура — имена, типове и дефиниции. Нито един ред не напуска този компютър, а прикачването е забранено.
ai-data-on-request-hint = Асистентът не чете данни сам. Редовете, които прикачите от резултат, напускат този компютър заедно с въпроса.
ai-data-full-hint = Асистентът може сам да изпълнява заявки само за четене и да чете примерни редове. Всичко, което прочете, напуска този компютър.
settings-ai-gutter = Цветна линия до отговорите
settings-ai-gutter-hint = Отбелязва отговорите на асистента с цветна линия по десния им край. Ако е изключено, те получават еднакво поле от двете страни.

## Settings › section list

settings-section-general = Общи
settings-section-editor = Редактор
settings-section-formatting = Форматиране на кода
settings-section-query = Заявки
settings-section-terminal = Терминал
settings-section-ai = ИИ асистент
settings-section-appearance = Изглед

## Settings › shared

settings-off = Изключено

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } знак
   *[other] { $n } знака
}

## Settings › General

settings-log = Журнален файл
settings-log-hint = Диагностиката, включително сривовете, се записва в { $path }.
settings-log-hint-none = На този компютър не можа да се отвори журнален файл, така че нищо не се записва.
settings-log-open-folder = Отваряне на папката
settings-bundle = Файл с настройки
settings-bundle-hint = Вашите предпочитания и фрагменти в един файл, който да пренесете на друг компютър. Връзките, подредбата на прозорците, обвивката на терминала и фрагментите за конкретни връзки остават тук.
settings-bundle-export = Експортиране
settings-bundle-import = Импортиране
settings-cli = Команден ред
settings-cli-install = Инсталиране
settings-cli-installing = Инсталиране
settings-cli-remove = Премахване
settings-cli-removing = Премахване

## Settings › modal

settings-title = Настройки
settings-restore-tabs = Възстановяване на разделите при стартиране
settings-restore-tabs-hint = При стартиране на приложението се отварят отново разделите със заявки от последната сесия.

## Settings › Editor

settings-editor-font-size = Размер на шрифта
settings-editor-predictions = Вградени предложения
settings-editor-predictions-hint = Показва вероятните следващи думи след курсора — двойката на ключова дума, външния ключ за съединение, колоните за групиране или вмъкване. Tab или → в края на реда ги приема изцяло, Ctrl+→ (⌘→ в macOS) — дума по дума, а Escape ги отхвърля.
settings-editor-remembered = Предложения от предишни изрази
settings-editor-remembered-hint = Предлага и остатъка от ред от израз, който сте изпълнили, написали в друг раздел или запазили като фрагмент. Изключете, за да не се показват предишни изрази на екрана.
settings-editor-aliases = Псевдоними на таблици
settings-editor-aliases-hint = Когато автодовършването изпише таблица след FROM или JOIN, добавя кратък псевдоним от инициалите ѝ — orders o, order_items oi — и го използва в условието ON на съединението.
settings-editor-rainbow = Цветни скоби
settings-editor-rainbow-hint = Оцветява всяка двойка скоби според дълбочината на влагане, така че затварящата скоба да се свърже с отварящата с един поглед.
settings-editor-whitespace = Показване на празните знаци
settings-editor-whitespace-hint = Показва интервалите и табулациите като бледи знаци, за да се различава табулация от интервали.
settings-editor-guides = Водачи за отстъп
settings-editor-guides-hint = Чертае бледа вертикална линия на всяко ниво на отстъп.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Регистър на ключовите думи
settings-format-case-keep = Както е въведено
settings-format-case-upper = Главни букви
settings-format-case-lower = Малки букви
settings-format-commas-trailing = В края на реда
settings-format-commas-leading = В началото на реда
settings-format-gap-one = Един празен ред
settings-format-gap-none = Без празен ред
settings-format-keyword-case-hint = Как „Форматиране на кода“ изписва ключови думи като SELECT и FROM. Имената, низовете и коментарите запазват регистъра си.
settings-format-function-case = Регистър на функциите
settings-format-function-case-hint = Как „Форматиране на кода“ изписва вградените функции на системата, като COUNT и COALESCE, там където се извикват. Вашите собствени функции запазват регистъра си.
settings-format-type-case = Регистър на типовете
settings-format-type-case-hint = Как „Форматиране на кода“ изписва вградените типове данни на системата, като INT и VARCHAR. Създадените от вас типове запазват регистъра си.
settings-format-commas = Запетаи
settings-format-commas-hint = Къде „Форматиране на кода“ поставя запетаята на списък, който разделя на няколко реда. Списъците на един ред, като аргументите на функция, остават непроменени, освен ако ширината за пренасяне не ги раздели.
settings-format-statement-gap = Между изразите
settings-format-statement-gap-hint = Празните редове, които „Форматиране на кода“ оставя между два израза. „Както е въведено“ запазва празен ред навсякъде, където е имало такъв.
settings-format-wrap = Ширина за пренасяне
settings-format-wrap-hint = Списък, който „Форматиране на кода“ държи на един ред, като аргументите на функция, се разделя по един елемент на ред, щом редът му надхвърли тази ширина.
settings-format-river = Подравняване на ключовите думи вдясно
settings-format-river-hint = Подравнява SELECT, FROM, WHERE и останалите по десния им край, с тялото на всяка клауза до ключовата ѝ дума, вместо ключовата дума да е на отделен ред.
settings-format-align = Подравняване на колоните
settings-format-align-hint = Подравнява AS на псевдонимите в списъка на SELECT, = на присвояванията в UPDATE и типовете на колоните на таблица.
settings-format-join-on = Условието на съединението на отделен ред
settings-format-join-on-hint = Поставя ON на съединението, както и AND и OR в условието му, на отделни редове под съединението.

## Settings › Query

settings-query-row-limit = Ограничение на редовете по подразбиране
settings-query-timeout = Време за изпълнение на израз
settings-query-timeout-hint = Прекратява израз, който се изпълнява по-дълго от това. Изключено по подразбиране — импорт или отчет може основателно да отнеме време.
timeout-off = Без ограничение
timeout-hours = { $n ->
    [one] { $n } час
   *[other] { $n } часа
}
timeout-minutes = { $n ->
    [one] { $n } минута
   *[other] { $n } минути
}
timeout-seconds = { $n ->
    [one] { $n } секунда
   *[other] { $n } секунди
}
settings-query-confirm-writes = Потвърждение преди изпълнение на записи
settings-query-confirm-writes-hint = Пита преди изпълнение на всеки израз, който променя данни или схема.
settings-query-validate = Проверка в базата данни на живо
settings-query-validate-hint = Проверява израза под курсора спрямо базата данни, докато пишете (PREPARE без изпълнение), за да покаже точните грешки. Добавя едно обръщение към базата при всяка пауза.
settings-query-notify = Известие, когато изпълнение във фонов режим приключи
settings-query-notify-hint = Показва известие на работния плот, когато заявка, скрипт, експорт или дъмп, продължил секунда или повече, приключи, докато Schemaic не е активният прозорец. Бутонът в лентата на задачите се осветява и в двата случая. Известието никога не съдържа SQL.
notify-query-finished = Заявката приключи
notify-query-failed = Заявката е неуспешна
notify-script-finished = Скриптът приключи
notify-script-failed = Скриптът е неуспешен
notify-export-finished = Експортът приключи
notify-export-failed = Експортът е неуспешен
notify-dump-finished = Дъмпът приключи
notify-dump-failed = Дъмпът е неуспешен
notify-took = Отне { $duration }.
took-millis = { $ms } мс
took-seconds = { $s } с
took-minutes = { $m } мин { $s } с
settings-query-write-log = Запазване на стойностите в журнала на записите
settings-query-write-log-hint = Всеки запис, който Schemaic или schemaic exec изпълнява, се добавя към write_log.jsonl в папката с настройки и никога не се изчиства. Стойностите в неговия SQL се маскират, освен ако това е включено; паролата се маскира и в двата случая.

## Settings › Appearance

settings-appearance-ui-theme = Тема на интерфейса
settings-appearance-editor-theme = Тема на редактора
settings-appearance-scale = Мащаб на интерфейса

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Клавишни комбинации

## Shortcuts modal › group headings

shortcuts-group-global = Глобални
shortcuts-group-editor = Редактор
shortcuts-group-results-grid = Таблица с резултати
shortcuts-group-schema-tree = Дърво на схемата
shortcuts-group-ai-panel = Панел за ИИ
shortcuts-group-terminal = Терминал
shortcuts-group-er-diagram = ER диаграма

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Търсене навсякъде
shortcuts-command-palette = Палитра с команди
shortcuts-new-tab = Нов раздел за заявки
shortcuts-close-tab = Затваряне на раздела за заявки
shortcuts-next-tab = Следващ раздел
shortcuts-previous-tab = Предишен раздел
shortcuts-jump-to-tab = Към раздел
shortcuts-reopen-tab = Повторно отваряне на последно затворения раздел
shortcuts-open-file = Отваряне на SQL файл
shortcuts-save-file = Запазване на SQL файл
shortcuts-save-file-as = Запазване на SQL файл като
shortcuts-toggle-schema = Показване/скриване на панела на схемата
shortcuts-toggle-ai = Показване/скриване на панела за ИИ
shortcuts-toggle-terminal = Показване/скриване на терминала
shortcuts-run-query = Изпълнение на заявката
shortcuts-autocomplete = Автодовършване
shortcuts-inline-ai = Вградена редакция с ИИ
shortcuts-find-in-editor = Търсене в редактора
shortcuts-find-replace = Търсене и замяна
shortcuts-go-to-line = Към ред
shortcuts-matching-bracket = Към съответната скоба или BEGIN/END
shortcuts-line-comment = Превключване на коментар за ред
shortcuts-block-comment = Превключване на блоков коментар
shortcuts-toggle-case = Смяна на регистъра
shortcuts-join-lines = Сливане на редове
shortcuts-duplicate = Дублиране на ред / маркирано
shortcuts-cut = Изрязване на ред / маркирано
shortcuts-format-code = Форматиране на кода
shortcuts-format-statement = Форматиране на израза
shortcuts-single-line = Обединяване на един ред
shortcuts-move-line = Преместване на реда нагоре / надолу
shortcuts-quick-fix = Прилагане на бързата поправка под курсора
shortcuts-indent = Увеличаване / намаляване на отстъпа
shortcuts-take-prediction = Приемане на вграденото предложение (и →, в края на реда) / следващата му дума
shortcuts-find-in-results = Търсене в резултатите
shortcuts-go-to-row = Към ред от резултатите
shortcuts-copy = Копиране
shortcuts-paste-staged = Поставяне като подготвени промени
shortcuts-select-all = Маркиране на всичко
shortcuts-first-last-cell = Първа / последна клетка
shortcuts-edit-cell = Редактиране на клетката
shortcuts-next-cell = Следваща / предишна клетка при редактиране
shortcuts-commit-edits = Потвърждаване на промените
shortcuts-mark-deletion = Отбелязване на реда за изтриване
shortcuts-results-toolbar = Към лентата с инструменти на резултатите
shortcuts-along-toolbar = Придвижване по лентата с инструменти (Esc връща към таблицата)
shortcuts-open-selected = Отваряне на избраната таблица, колона или обект
shortcuts-context-menu = Контекстно меню за избрания ред
shortcuts-send-message = Изпращане на съобщението
shortcuts-new-line = Нов ред
shortcuts-recall-prompt = Предишна / следваща подкана
shortcuts-copy-paste = Копиране / поставяне
shortcuts-find-in-diagram = Търсене в диаграмата

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
key-space = Интервал

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear history" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Превключване на панел
palette-toggle-panel-hint = схема · ии · терминал · история на заявките · активност на сървъра
palette-panel-schema = Схема
palette-panel-ai = ИИ
palette-panel-terminal = Терминал
palette-panel-history = История на заявките
palette-panel-activity = Активност на сървъра
palette-new-tab = Нов раздел
palette-export-settings = Експортиране на настройките
palette-import-settings = Импортиране на настройките
palette-duplicate-tab = Дублиране на раздела
palette-close-tab = Затваряне на раздела
palette-reopen-closed-tab = Повторно отваряне на затворен раздел
palette-reopen-closed-tab-hint = <раздел>
palette-open-file = Отваряне на файл
palette-open-file-hint = скрипт .sql
palette-save-file = Запазване на файла
palette-save-file-as = Запазване на файла като
palette-next-tab = Следващ раздел
palette-previous-tab = Предишен раздел
palette-format-code = Форматиране на кода
palette-format-statement = Форматиране на израза
palette-format-statement-hint = изразът при курсора
palette-make-single-line = Обединяване на един ред
palette-make-single-line-hint = маркираното или изразът, на един ред
palette-run = Изпълнение
palette-run-hint = изпълнява всички изрази
palette-go-to-line = Отиване на ред
palette-go-to-line-hint = <номер>
palette-go-to-statement = Отиване на израз
palette-search-hint = <търсене>
palette-history = История
palette-clear-history = Изчистване на историята
palette-clear-history-hint = текущата връзка
palette-ask-ai = Въпрос към ИИ
palette-ask-ai-hint = <подкана>
palette-ask-ai-empty = Въведете подкана…
palette-terminal = Терминал
palette-terminal-hint = <команда>
palette-run-in-terminal = Изпълнение в терминала
palette-terminal-empty = Въведете команда…
palette-ui-theme = Тема на интерфейса
palette-ui-theme-hint = светла · тъмна
palette-editor-theme = Тема на редактора
palette-editor-theme-hint = <тема>
palette-font-size = Размер на шрифта
palette-increase-font-size = Увеличаване на шрифта
palette-decrease-font-size = Намаляване на шрифта
palette-indent-style = Стил на отстъпа
palette-indent-style-hint = табулации · интервали
palette-indent-spaces = Интервали
palette-indent-tabs = Табулации
palette-indent-width = Ширина на отстъпа
palette-toggle-word-wrap = Превключване на пренасянето на думи
palette-toggle-whitespace = Превключване на празните знаци
palette-toggle-whitespace-hint = показва интервалите и табулациите
palette-toggle-indent-guides = Превключване на водачите за отстъп
palette-switch-connection = Смяна на връзката
palette-switch-connection-hint = <връзка>
palette-server-variables = Сървърни променливи
palette-server-variables-hint = настройките на активната връзка
palette-enter-number = Въведете число
palette-nothing-found = Нищо не е намерено

## Command palette › what a result row is

palette-row-snippet = фрагмент
palette-row-open-tab = отворен раздел
palette-row-history = история
palette-row-view-body = тяло на изглед
palette-row-body = тяло ({ $kind })
palette-row-trigger-on = тригер върху { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Отваряне в CLI
tree-open = Отваряне
tree-open-in-new-tab = Отваряне в нов раздел
tree-copy-name = Копиране на името
tree-copy-qualified-name = Копиране на пълното име
tree-properties = Свойства
tree-used-by = Използва се от
tree-live-monitor = Наблюдение на живо
tree-er-diagram = ER диаграма
tree-compare-with = Сравняване с
tree-generate = Генериране
tree-generate-ddl = Генериране на DDL
tree-generate-create = CREATE
tree-generate-insert = INSERT
tree-generate-update = UPDATE
tree-generate-delete = DELETE
tree-refresh = Опресняване
tree-collapse-all = Свиване на всичко
tree-favourite = Добавяне в любими
tree-unfavourite = Премахване от любими
tree-colour = Цвят
tree-colour-none = Без
tree-hide = Скриване
tree-import = Импортиране
tree-export = Експортиране
tree-back-up = Резервно копие
tree-restore-from-file = Възстановяване от файл
tree-copy-to = Копиране в
tree-create = Създаване
tree-create-table = Таблица
tree-create-view = Изглед
tree-create-type = Тип
tree-create-domain = Домейн
tree-create-sequence = Последователност
tree-create-synonym = Синоним
tree-create-alias-type = Тип псевдоним
tree-create-xml-schema-collection = Колекция от XML схеми
tree-create-function = Функция
tree-create-procedure = Процедура
tree-create-event = Събитие
tree-create-extension = Разширение
tree-create-database = База данни
tree-create-schema = Схема
tree-create-kind = Създаване: { $kind }
tree-edit-kind = Редактиране: { $kind }
tree-folder-create-extension = Създаване на разширение
tree-edit-table = Редактиране на таблицата
tree-edit-view = Редактиране на изгледа
tree-edit-column = Редактиране на колоната
tree-edit-index = Редактиране на индекса
tree-edit-primary-key = Редактиране на първичния ключ
tree-edit-foreign-key = Редактиране на външния ключ
tree-triggers = Тригери
tree-triggers-count = Тригери ({ $n })
tree-refresh-view = Опресняване на изгледа
tree-truncate = Изпразване
tree-truncate-title = Изпразване на таблицата
tree-truncate-rows = { $n ->
    [one] Да се изтрие ли единственият ред ({ $rows }) в { $table }? Това не може да бъде отменено.
   *[other] Да се изтрият ли всички { $rows } реда в { $table }? Това не може да бъде отменено.
}
tree-truncate-every-row = Да се изтрият ли всички редове в { $table }? Това не може да бъде отменено.
tree-drop = Премахване
tree-drop-index = Премахване на индекса
tree-drop-foreign-key = Премахване на външния ключ
tree-ai-explain = Обяснение от ИИ
tree-users = Потребители и привилегии
tree-server-variables = Сървърни променливи
tree-new-database = Създаване на база данни
tree-show-table-sizes = Показване на размерите на таблиците

## Connection switcher

conn-menu-manage = Управление на връзките

## Shared dialog buttons

dialog-cancel = Отказ
dialog-yes = Да
dialog-no = Не

## Open-transaction prompt

tx-prompt-title = Отворена транзакция
tx-prompt-body = { $tab } има отворена транзакция с { $n ->
    [one] 1 израз
   *[other] { $n } израза
} в нея. Можете да я потвърдите, за да запазите промените, или да я върнете, за да ги отхвърлите.
tx-prompt-body-aborted = { $tab } има транзакция, прекъсната от неуспешен израз, с { $n ->
    [one] 1 израз
   *[other] { $n } израза
} в нея. Тя може само да бъде върната, което отхвърля промените.
tx-prompt-rollback = Връщане
tx-prompt-commit = Потвърждаване

## Error modal

error-modal-title = Грешка
error-modal-none = Няма грешка.
error-modal-ai-fix = Поправка с ИИ
error-modal-explain = Обяснение

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

db-query-failed = заявката е неуспешна: { $error }
db-connection-failed = връзката е неуспешна: { $error }
db-query-cancelled = заявката е прекратена

## The database layer's own refusals (core's model::DbRefusal; a test holds
## the English equal). Names, types, values and the runtime's own words are
## as they came; $has_row and $has_lost are "some" or "none" for an optional
## file row ($row) and lost character ($character in code page $code_page);
## $sql_type is the column's type as the server names it;
## $cap and $file are sizes as the app writes them.

db-refused-init-sql = Няма връзка — началният SQL на тази връзка беше отказан: { $why }
db-refused-transport-entra = Вписването с Microsoft Entra изисква SSL / TLS в режим Verify full: токенът му отваря всяка база данни в Azure SQL, до която акаунтът има достъп, така че сървърът трябва да докаже името си, преди да го получи.
db-refused-transport-aws = Вписването с AWS IAM изисква SSL / TLS поне в режим „Задължително“: токенът му е парола за този сървър и не бива да минава през мрежата некриптиран.
db-refused-transport-gcp = Вписването с Google Cloud IAM изисква SSL / TLS в режим Verify CA, със сървърния CA на инстанцията като Root CA: токенът му отваря всяка инстанция на Cloud SQL, до която акаунтът има достъп.
db-refused-transport = Това вписване изисква по-строг режим на SSL / TLS от този на връзката.
db-refused-backup-in-terminal = Резервно копие на сървърна база данни се прави с нейните собствени инструменти, в терминала.
db-refused-commit-aborted = транзакцията вече е прекъсната — израз в нея е неуспешен или е спрян, така че потвърждаването би отхвърлило работата, вместо да я запази. Нищо не е потвърдено.
db-refused-sqlite-no-manual = Връзките към SQLite все още не поддържат ръчен режим на транзакциите — изразите се изпълняват и потвърждават при изпращането си
db-refused-mssql-read-only-auto = Връзка към SQL Server само за четене работи в режим „Автопотвърждаване“: SQL Server няма транзакция само за четене, в която да се държи раздел в режим „Ръчен“, а всеки израз, изпълнен в режим „Автопотвърждаване“, се връща
db-refused-client-charset = отказано: клиентската кодова таблица на сесията ({ $charset }) може да скрие кавичка вътре в знак, така че проверката на израза не може да го прочете както сървъра
db-refused-sql-mode = отказано: sql_mode на сървъра ({ $mode }) чете кавичките различно от проверката на израза и не можа да бъде променен за тази сесия
db-refused-writing-plan = Плановете тук се изпълняват в транзакция само за четене, а MySQL и MariaDB не създават план за израз, който записва в такава. Вместо това поискайте план за SELECT формата на израза.
db-refused-writing-plan-function = Плановете тук се изпълняват в транзакция само за четене, а MySQL и MariaDB не създават план за израз, който записва в такава. Функция, която той извиква, записва, а това не е позволено за план тук.
db-refused-windows-build-only = Вписването с Windows е налично само във версията на Schemaic за Windows.
db-refused-cloud-iam-engine = Вписването с облачен IAM е за връзки към PostgreSQL и MySQL.
db-refused-mssql-cancel-needs-kill = SQL Server не може да прекрати израз на друга сесия, без да прекрати сесията — използвайте „Прекратяване на сесията“.
db-refused-shadowed-function = Отказано, преди изразът ви да се изпълни: { $schema }.{ $name } е собствена функция на тази база данни с името на вградена функция на PostgreSQL, която заявките само за четене могат да извикват, и PostgreSQL може да я извика на мястото на вградената — затова проверката за само четене не може да гарантира какво би изпълнила заявка тук. Махнете { $schema } от пътя за търсене на връзката или помолете собственика на базата данни да я изтрие или преименува.
db-refused-shadowed-operator = Отказано, преди изразът ви да се изпълни: { $schema } съдържа оператора { $name }, и PostgreSQL може да го използва на мястото на вграден — затова проверката за само четене не може да гарантира какво би изпълнила заявка тук. Махнете { $schema } от пътя за търсене на връзката или помолете собственика на базата данни да го изтрие.
db-refused-blank-value = { $has_row ->
        [some] Ред { $row } има празна стойност за { $column } ({ $sql_type }): SQL Server би я записал като 0 или 1900-01-01, вместо да я откаже. Импортирайте празните полета като NULL или ги попълнете.
       *[none] Празна стойност не може да бъде записана в { $column } ({ $sql_type }): SQL Server би я записал като 0 или 1900-01-01, вместо да я откаже. Задайте NULL в клетката или въведете стойност.
    }
db-refused-code-page = { $has_row ->
        [some] Ред { $row } има стойност за { $column } ({ $sql_type }), която
       *[none] Стойност за { $column } ({ $sql_type })
    } съдържа { $has_lost ->
        [some] `{ $character }`, за който кодовата страница { $code_page } няма знак
       *[none] знак, за който кодовата ѝ страница няма съответствие
    }: SQL Server би го записал като `?` или подобен знак, вместо да го откаже. Съхранявайте колоната като nvarchar или премахнете знака.
db-refused-sqlite-no-file = не е зададен файл на базата данни
db-refused-sqlite-memory = :memory: не е файл на база данни — SQLite пази база данни в паметта само докато е жива връзката, която я е създала, а това приложение отваря нова връзка за всяка операция, така че всеки запис би бил отчетен като запазен и след това изгубен. Посочете път до файл.
db-refused-sqlite-uri = { $file }: това е URI, а не файл на база данни — SQLite би прочел частта след `?` като опции за отваряне, а `?mode=memory` отваря временна база данни, която приема всеки запис и не пази нито един. Посочете пътя до самия файл.
db-refused-sqlite-worker = работният процес на SQLite е неуспешен: { $error }
db-refused-sqlite-no-column = { $table }.{ $column } не е колона на тази таблица, така че няма стойност по подразбиране за записване
db-refused-sqlite-generated = { $column } е генерирана колона: няма стойност по подразбиране, към която да бъде върната, а SQLite я изчислява от реда
db-refused-sqlite-sizes-too-large = Размерите на таблиците се четат само за база данни до { $cap }: SQLite ги измерва, като чете всяка страница от файла, а този е { $file }.

## Why a connection could not be made (core's model::ConnectFailure), after
## db-connection-failed's prefix; a test holds the English equal. $error is
## the driver's, the SSH library's or the system's own text; $host a
## host:port; $path a file; $secs a number of seconds; $key a fingerprint.

db-connect-timed-out = времето изтече
db-connect-init-failed = началният SQL `{ $statement }` е неуспешен: { $error }
db-connect-session-retired = връзката на този раздел беше затворена след спиране, което сървърът не потвърди, и не можа да бъде отворена нова; транзакцията ѝ беше върната. Превключете раздела на „Автопотвърждаване“ и обратно на „Ръчен“, за да отворите друга.
db-connect-begin-failed = транзакцията не можа да започне: { $error }
db-connect-prepare-session = сесията не можа да бъде подготвена: { $error }
db-connect-read-only-setup = сесията не можа да бъде направена само за четене: { $error }
db-connect-guard-session = сесията не можа да бъде защитена: { $error }
db-connect-plan-setup = планът не можа да бъде подготвен: { $error }
db-connect-no-maintenance-db = няма достъпна служебна база данни
db-connect-browser-silent = SQL Server Browser на { $server } не върна порт за инстанцията { $instance }: услугата Browser не работи, UDP порт 1434 е блокиран по пътя или там няма инстанция с това име. Ако инстанцията слуша на фиксиран порт, задайте за хост { $server } без \{ $instance } и този порт.
db-connect-browser-without-tcp = SQL Server Browser на { $server } отговори за инстанцията { $instance }, но не върна TCP порт: инстанцията не слуша по TCP. Разрешете TCP/IP за нея в SQL Server Configuration Manager и я рестартирайте.
db-connect-azure-timed-out = Azure SQL не завърши вписването в рамките на { $secs } секунди. Безсървърна база данни, която е на пауза, се събужда за около минута, а този опит я е събудил — свържете се отново след малко.
db-connect-azure-unavailable = { $server } Безсървърна база данни, която е на пауза, се събужда за около минута, а този опит я е събудил — свържете се отново след малко.
db-ssh-connect-no-answer = SSH връзка с { $host }: няма отговор в рамките на { $secs } с
db-ssh-auth-no-answer = SSH удостоверяване с { $host }: няма отговор в рамките на { $secs } с
db-ssh-auth-error = грешка при SSH удостоверяване: { $error }
db-ssh-auth-failed = SSH удостоверяването е неуспешно
db-ssh-key-load = зареждането на SSH ключа е неуспешно: { $error }
db-ssh-key-auth = грешка при удостоверяване със SSH ключ: { $error }
db-ssh-agent = грешка на SSH агента: { $error }
db-ssh-agent-empty = SSH агентът няма заредени идентичности (изпълнете `ssh-add`)
db-ssh-agent-auth = грешка при удостоверяване чрез SSH агента: { $error }
db-ssh-no-agent = не е намерен SSH агент (стартирайте услугата OpenSSH Authentication Agent или Pageant)
db-ssh-no-agent-socket = няма SSH агент ($SSH_AUTH_SOCK): { $error }
db-ssh-jump-unreachable = междинният SSH хост не можа да достигне { $host }: { $error }
db-ssh-connect-to = SSH връзката с { $host } е неуспешна: { $error }
db-ssh-connect = SSH връзката е неуспешна: { $error }
db-ssh-no-host = SSH тунелът няма хост
db-ssh-bind = свързването на локалния тунел е неуспешно: { $error }
db-ssh-named-instance = До именувана инстанция не може да се стигне през SSH тунел: SQL Server Browser съобщава порта ѝ по UDP, а тунелът не пренася UDP. Задайте за хост { $server } без \{ $instance } и порта, на който слуша инстанцията.
db-ssh-host-key-changed = SSH ключът на хоста { $host } се е ПРОМЕНИЛ, откакто Schemaic му се е доверил за първи път.
db-ssh-host-key-expected = Очакван   { $key }
db-ssh-host-key-offered = Предложен { $key }
db-ssh-host-key-none = (няма записан ключ)
db-ssh-host-key-changed-advice = Това може да означава, че сървърът е преинсталиран или ключът му е сменен — или че нещо прихваща връзката. Нищо не му е изпратено. Проверете новия отпечатък по друг канал (на собствената конзола на сървъра или при администратора му), преди да му се доверите. След това премахнете записа за този хост от { $store } в конфигурационната папка на Schemaic, за да се запише новият ключ.
db-ssh-host-key-store = Schemaic не можа да прочете записа си на доверени SSH ключове на хостове ({ $store }), така че не може да прецени дали ключът на { $host } ({ $key }) е този, на който сте се доверили преди.
db-ssh-host-key-store-advice = Връзката беше отказана, вместо ключът да бъде приет на доверие. Поправете или изтрийте { $store } в конфигурационната папка на Schemaic — изтриването означава, че всеки SSH хост ще бъде приет наново при следващото свързване, така че го направете само ако можете да проверите отпечатъците.
db-tls-setup = настройването на TLS е неуспешно: { $error }
db-tls-client-cert = клиентският сертификат е отхвърлен: { $error }
db-tls-trust-store = хранилището за доверени сертификати на TLS е неизползваемо: { $error }
db-tls-bad-name = { $name } не е име, което сертификат може да съдържа: { $error }
db-tls-ca-empty = CA файлът { $path } не съдържа сертификати
db-tls-ca-unusable = CA файлът { $path } съдържа сертификат, който не може да се използва като доверен корен
db-tls-cert-unreadable = файлът със сертификат { $path } не може да бъде прочетен: { $error }
db-tls-cert-not-pem = { $path } не е PEM файл със сертификат: { $error }
db-tls-cert-is-der = { $path } не съдържа PEM сертификат — DER файл трябва първо да се преобразува (openssl x509 -inform der -in { $path } -out cert.pem)
db-tls-key-unreadable = файлът с ключ { $path } не може да бъде прочетен: { $error }
db-tls-key-encrypted = { $path } е частен ключ, защитен с парола, който Schemaic не може да отвори. Първо го декриптирайте: openssl pkcs8 -in { $path } -out client-key.pem
db-tls-key-not-pem = { $path } не е частен ключ във формат PEM: { $error }
db-sign-in-no-value = { $field ->
        [host] Връзката няма хост, с който да се впише.
       *[user] Връзката няма потребител, с който да се впише.
    }
db-sign-in-leading-dash = { $field ->
        [host] Хостът на връзката започва с „-“, което облачният CLI би прочел като опция, а не като хост.
       *[user] Потребителят на връзката започва с „-“, което облачният CLI би прочел като опция, а не като потребител.
    }
db-sign-in-control = { $field ->
        [host] Хостът на връзката съдържа управляващ знак.
       *[user] Потребителят на връзката съдържа управляващ знак.
    }
db-sign-in-shim-aws = Намереният AWS CLI е .cmd обвивка (инсталираната чрез pip версия 1), а Schemaic не изпълнява обвивки чрез cmd.exe. Инсталирайте AWS CLI версия 2 или се впишете с парола.
db-sign-in-shim-gcloud = Намереният gcloud е .cmd обвивка без вграден Python до нея, а Schemaic не изпълнява обвивки чрез cmd.exe. Преинсталирайте Google Cloud CLI с неговия инсталатор или се впишете с парола.
db-sign-in-shim-azure = Намереният Azure CLI е .cmd/.bat обвивка без Python до нея, а Schemaic не изпълнява обвивки чрез cmd.exe. Преинсталирайте Azure CLI с инсталатора на Microsoft или се впишете със SQL вход.
db-sign-in-no-token = Облачният CLI отговори без токен.
db-sign-in-more-than-token = Облачният CLI отговори с повече от токен.
db-sign-in-azure-bad-answer = Отговорът на Azure CLI не беше токенът, който трябваше да отпечата.
db-sign-in-azure-no-access-token = Azure CLI отговори без токен за достъп.
db-sign-in-signed-out-aws = AWS CLI няма идентификационни данни. Изпълнете `aws configure` или `aws sso login` и се свържете отново.
db-sign-in-signed-out-gcloud = Google Cloud CLI не е вписан. Изпълнете `gcloud auth login` и се свържете отново.
db-sign-in-signed-out-azure = Azure CLI не е вписан. Изпълнете `az login` и се свържете отново.
db-sign-in-no-token-from = { $cli } не можа да получи токен: { $line }
db-sign-in-no-token-from-silent = { $cli } не можа да получи токен.
db-sign-in-no-token-from-azure-silent = Azure CLI не можа да получи токен: не посочи причина
db-sign-in-not-found = { $cli } не е намерен. Инсталирайте го и се впишете с него или се впишете с парола.
db-sign-in-not-found-azure = Azure CLI (az) не е намерен. Инсталирайте го и изпълнете `az login` или се впишете със SQL вход.
db-sign-in-no-answer = { $cli } не отговори в рамките на { $secs } секунди.
db-sign-in-not-started = { $cli } не можа да бъде стартиран: { $error }

## A query's failure in the database layer's own words (core's
## model::QueryFailure), after db-query-failed's prefix; a test holds the
## English equal. $error is the server's or the runtime's text.

db-query-export-stopped = експортът спря да чете
db-query-no-rows-to-export = този израз не връща редове за експортиране
db-query-no-server-settings = Тази система няма сървърни настройки за показване.
db-query-no-status-list = Тази система не публикува списък със състояния — само променливите си.
db-query-no-import-columns = Няма колони, в които да се импортира
db-query-read-stopped = Четенето спря: { $error }
db-query-batch-continued = SQL Server продължава изпълнението на пакет след грешка в израз, освен ако грешката не прекрати пакета, така че изразите след неуспешния може да са се изпълнили.
db-query-sqlite-locked = В SQLite дълго четене блокира записа: експорт или импорт на цяла таблица държи заключване за четене на файла, докато приключи, и нито един запис не може да започне дотогава (MySQL и PostgreSQL не работят така). Изчакайте го или го прекратете и опитайте отново. За да могат четенето и записът във файла да се застъпват трайно, превключете го в режим на журнала WAL — `PRAGMA journal_mode = WAL` — което променя самия файл, затова Schemaic няма да го направи вместо вас.
db-query-rollback-failed-too = { $error } — и връщането също е неуспешно: { $rollback }

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

ddl-statement-failed = израз { $n } е неуспешен: { $error }
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } предишен израз вече е приложен и не може да бъде върнат
   *[other] { $failed } — { $n } предишни израза вече са приложени и не могат да бъдат върнати
}
ddl-cancelled = планът беше прекратен
ddl-dangling-foreign-key = планът оставя външен ключ, който не сочи към нищо: { $row }
ddl-breaks-view = планът би развалил изгледа { $view }, който чете колона, която планът преименува или премахва ({ $why }). Нищо не е променено. Първо редактирайте или премахнете изгледа.
ddl-no-databases = SQLite няма бази данни за създаване или премахване — там базата данни е файл, който Schemaic не създава и не изтрива вместо вас.
ddl-elsewhere-failed = Изгледите в други бази данни, които правят select * от { $table }, не бяха опреснени ({ $error }) и четат и записват грешни колони, докато не бъдат опреснени (sp_refreshview).
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } изглед в друга база данни, който прави select * от { $table }, не можа да бъде опреснен и чете и записва грешни колони, докато не бъде опреснен (sp_refreshview): { $views }.
       *[other] { $count } изгледа в друга база данни, които правят select * от { $table }, не можаха да бъдат опреснени и четат и записват грешни колони, докато не бъдат опреснени (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } база данни не беше претърсена за изглед, който прави select * от { $table }: { $databases }.
       *[other] { $count } бази данни не бяха претърсени за изглед, който прави select * от { $table }: { $databases }.
    }
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , и още { $more }
    }
ddl-rollback-failed = { $error } — и връщането също е неуспешно: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). A server's own words reach the line as they are, not
## through a message here.

write-one-row-delete = изтриването в { $table } засегна { $affected } реда (очакван беше точно 1)
write-one-row-update = обновяването в { $table } засегна { $affected } реда (очакван беше точно 1)
write-one-row-insert = вмъкването в { $table } засегна { $affected } реда (очакван беше точно 1)
write-batch-inserted = пакет от { $rows } реда вмъкна { $wrote }
write-batch-affected = пакет от { $rows } реда засегна { $wrote }
write-row-issue = ред { $line }, колона { $column }: { $problem } ({ $text })
write-row-issue-empty = ред { $line }, колона { $column }: { $problem } (празно)
write-copy-too-long = { $column } съдържа повече текст, отколкото може да пренесе едно четене, затова копирането спря, преди да го запише
write-copy-too-long-unnamed = колона съдържа повече текст, отколкото може да пренесе едно четене, затова копирането спря, преди да го запише
write-copy-bytes = { $column } съдържа байтове, които копирането не може да прочете такива, каквито са, затова спря, преди да ги запише
write-copy-not-its-form = стойността, прочетена за { $column }, не е във формата, която копирането поиска, затова нищо не е записано
write-copy-not-its-form-unnamed = стойността, прочетена за колона, не е във формата, която копирането поиска, затова нищо не е записано
write-commit-cancelled = Потвърждаването е прекратено
write-import-cancelled = Импортирането е прекратено
write-rollback-complete = {" "}— всички промени са върнати
write-rollback-incomplete = {" "}— връщането НЕ ги отмени: механизмът за съхранение на тази таблица не е транзакционен, така че вече записаните редове остават. Проверете таблицата, преди да опитате отново.
write-rollback-unknown = {" "}— и връщането не можа да бъде потвърдено: връзката прекъсна, преди сървърът да отговори. Редове може да са останали, а може и да не са. Проверете таблицата, преди да опитате отново.

## Schema tree › the panel

tree-panel-title = СХЕМА
tree-tip-databases = Показване или скриване на базите данни
tree-tip-options = Опции на схемата
tree-search-placeholder = Търсене…
tree-loading = Зареждане
tree-no-tables = Няма таблици
tree-failed-to-connect = Неуспешно свързване
tree-failed-view = Преглед
tree-single-user-unread = Еднопотребителска — не е прочетена
tree-single-user-read = Прочитане
tree-count-tables = { $n ->
    [one] { $n } таблица
   *[other] { $n } таблици
}
tree-count-cols = { $n ->
    [one] { $n } кол.
   *[other] { $n } кол.
}
tree-count-keys = { $n ->
    [one] { $n } ключ
   *[other] { $n } ключа
}
tree-count-partitions = { $n ->
    [one] { $n } дял
   *[other] { $n } дяла
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = тип
kind-domain = домейн
kind-sequence = последователност
kind-function = функция
kind-procedure = процедура
kind-event = събитие
kind-synonym = синоним
kind-alias-type = тип псевдоним
kind-xml-schema-collection = колекция от XML схеми
kind-extension = разширение
kind-group-type = Типове
kind-group-domain = Домейни
kind-group-sequence = Последователности
kind-group-function = Функции
kind-group-procedure = Процедури
kind-group-event = Събития
kind-group-synonym = Синоними
kind-group-alias-type = Типове псевдоними
kind-group-xml-schema-collection = Колекции от XML схеми
kind-group-extension = Разширения

## Query history panel

history-title = ИСТОРИЯ НА ЗАЯВКИТЕ
history-clear-tip = Изчистване на историята на тази връзка…
history-clear-title = Изчистване на историята на заявките
history-clear-message = Да се { $n ->
    [one] изтрие ли { $n } записана заявка
   *[other] изтрият ли { $n } записани заявки
} за тази връзка? Това не може да бъде отменено.
history-scope-current = Текуща
history-scope-all = Всички
history-cut-note = Показани са най-новите { $shown } от { $matched } — търсете, за да стесните.
history-empty = Все още няма заявки.
history-no-match = Няма съвпадащи заявки.
history-open-in-new-tab = Отваряне в нов раздел
history-delete = Изтриване
history-failed = Неуспешна

## Snippet library panel

snippets-title = БИБЛИОТЕКА С ФРАГМЕНТИ
snippets-save-tip = Запазване на SQL от редактора като фрагмент
snippets-no-match = Няма съвпадащ фрагмент.
snippets-band-this-connection = ТАЗИ ВРЪЗКА
snippets-band-all-connections = ВСИЧКИ ВРЪЗКИ
snippets-builtin = Вграден
snippets-name-placeholder = Име на фрагмента
snippets-insert = Вмъкване в редактора
snippets-open-in-new-tab = Отваряне в нов раздел
snippets-edit = Редактиране
snippets-duplicate = Дублиране
snippets-delete = Изтриване
snippets-show-in = Показване в
snippets-scope-this-connection = Тази връзка
snippets-scope-engine = Всяка връзка към { $engine }
snippets-scope-all-connections = Всички връзки

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = ДНЕС
history-band-this-week = ТАЗИ СЕДМИЦА
history-band-earlier = ПО-РАНО

## Query tab strip

tabs-tip-unsaved = { $path } — незапазени промени
tabs-pin = Закачване
tabs-unpin = Откачване
tabs-rename = Преименуване
tabs-duplicate = Дублиране
tabs-close = Затваряне
tabs-open-file = Отваряне на файл
tabs-save = Запазване
tabs-save-as = Запазване като
tabs-reload = Презареждане от диска
tabs-reopen-last = Повторно отваряне на последния раздел
tabs-close-others = Затваряне на другите раздели
tabs-close-all = Затваряне на всички раздели

## Server Activity panel

activity-title = АКТИВНОСТ НА СЪРВЪРА
activity-refresh-tip = Опресняване сега
activity-auto-refresh-off = Автоматичното опресняване е изключено
activity-auto-refresh-every = Автоматично опресняване на всеки { $secs } с
activity-interval-off = Изключено
activity-interval = { $secs } с
activity-age-seconds = { $s } с
activity-age-minutes = { $m } мин { $s } с
activity-age-hours = { $h } ч { $m } мин
activity-age-days = { $d } д { $h } ч
activity-view-sessions = Сесии
activity-view-locks = Заключвания
activity-search-sessions = Търсене на сесии…
activity-search-locks = Търсене на заключвания…
activity-loading = Зареждане…
activity-sqlite-no-sessions = SQLite няма сървърни сесии — работи вътре в този процес, така че няма нищо друго свързано, което да се види.
activity-sessions-no-match = Няма съвпадащи сесии.
activity-sessions-none = Няма други сесии на този сървър.
activity-sessions-undrawn = { $n ->
    [one] Още { $n } сесия съвпада — стеснете търсенето, за да я видите.
   *[other] Още { $n } сесии съвпадат — стеснете търсенето, за да ги видите.
}
activity-locks-no-match = Няма съвпадащи заключвания.
activity-locks-no-waits = Няма чакащи заключвания на този сървър.
activity-locks-none = Никоя друга сесия не държи заключване.
activity-locks-contended-only = Този сървър показва само заключванията, които се чакат или задържат чакане — не всяко заключване на сесия.
activity-locks-undrawn = { $n ->
    [one] Още { $n } заключване съвпада — стеснете търсенето, за да го видите.
   *[other] Още { $n } заключвания съвпадат — стеснете търсенето, за да ги видите.
}
activity-list-capped = списъкът е ограничен
activity-locks-total = { $n ->
    [one] { $shown } заключване
   *[other] { $shown } заключвания
}
activity-locks-waiting = { $n ->
    [one] { $shown } чакащо
   *[other] { $shown } чакащи
}
activity-state-blocked = Блокирана
activity-state-idle-in-tx = Бездейства в транзакция
activity-state-running = Изпълнява се
activity-state-idle = Бездейства
activity-count-sessions = { $n ->
    [one] { $figure } сесия
   *[other] { $figure } сесии
}
activity-count-running = { $figure } изпълняващи се
activity-count-blocked = { $figure } блокирани
activity-count-idle-in-tx = { $figure } бездействащи в транзакция
activity-lock-wait = Чакане на заключване
activity-lock-wait-sentence = Сесия { $waiter } ({ $waiter_who }) { $waited ->
        [none] чака
       *[other] чака от { $waited }
    } заключване, държано от сесия { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] бездействаща в транзакция
        [blocked] самата блокирана
        [running] която се изпълнява
       *[idle] бездействаща
    }{ $held ->
        [none] {""}
       *[other] {" "}от { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}Още { $others } сесия също държи заключване, което ѝ трябва.
       *[other] {" "}Още { $others } сесии също държат заключване, което ѝ трябва.
    }
activity-kill-holder = Прекратяване на { $id }
activity-terminate-tip = Прекратяване на сесия { $id }
activity-read-only-tip = Тази връзка е отбелязана само за четене.
activity-lock-holds = държи
activity-lock-waits-for = чака
activity-lock-held-by = държано от { $ids }
activity-copy-statement = Копиране на израза
activity-cancel-query = Прекратяване на заявката
activity-kill-session = Прекратяване на сесията
activity-waiting-on = чака заключване, държано от { $ids }
activity-blocks = { $n ->
        [one] блокира { $n } сесия
       *[other] блокира { $n } сесии
    }
activity-read-only-cancel = Тази връзка е отбелязана само за четене, затова Schemaic няма да прекратява заявки в нея. Първо изключете „само за четене“ в настройките на връзката.
activity-read-only-terminate = Тази връзка е отбелязана само за четене, затова Schemaic няма да прекратява сесии в нея. Първо изключете „само за четене“ в настройките на връзката.

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = ИИ АСИСТЕНТ
ai-new-chat-tip = Нов разговор
ai-settings-tip = Настройки на ИИ
ai-empty-ask = Питайте за вашия SQL…
ai-empty-not-connected = { $cli } не е свързан.
ai-input-placeholder = Съобщение…
ai-input-off = ИИ асистентът е изключен
ai-input-db-down = Няма връзка с базата данни
ai-speaker-you = ВИЕ
ai-result-more-lines = { $n ->
    [one] … (още { $n } ред)
   *[other] … (още { $n } реда)
}

## Shared dialog buttons (continued)

dialog-close = Затваряне

## Chart modal

chart-title = Диаграма — { $tab }
chart-caption-kind = Диаграма
chart-caption-values = Стойности
chart-caption-labels = Етикети
chart-row-number = Номер на ред
chart-kind-bar = Стълбове
chart-kind-line = Линия
chart-kind-histogram = Хистограма
chart-save-tip = Запазване на диаграмата…
chart-save-dialog = Запазване на диаграмата
chart-file-stem = { $name } диаграма
chart-nothing-to-save = Няма нищо за запазване.
chart-no-numbers = В този резултат няма колона с числа за диаграма.
chart-all-null = Всяка стойност в тази колона е NULL.
chart-too-large = Тези стойности са твърде големи за изобразяване.
chart-caption-rows = { $n ->
    [one] { $n } ред
   *[other] { $n } реда
}
chart-caption-skipped-null = { $skipped } NULL, не са изобразени
chart-caption-skipped = { $skipped } не са изобразени (NULL или след { $max })
chart-caption-truncated = прочетените редове, а не целият резултат

## Schema editors — shared

editor-preview-sql = Преглед на SQL
editor-change-count = { $n ->
    [0] Няма промени
    [one] 1 промяна
   *[other] { $n } промени
}

## View editor

view-editor-title-edit = Редактиране на изгледа { $view }
view-problem-no-name = Изгледът трябва да има име.
view-problem-no-select = Изгледът трябва да се дефинира чрез SELECT.
view-problem-not-a-query = Тялото на изглед трябва да е заявка — започва със SELECT, WITH, VALUES или TABLE.
view-problem-materialized = Schemaic не може да редактира материализиран изглед — PostgreSQL няма CREATE OR REPLACE за такъв.
view-problem-hidden = Дефиницията на този изглед е криптирана или скрита от този вход, така че тук той може да бъде премахнат, но не и редактиран.
view-problem-verbatim = Заглавната част на този изглед съдържа нещо, което Schemaic не може да възпроизведе, така че тук той може да бъде премахнат, но не и редактиран.
view-editor-title-create = Създаване на изглед в { $location }
view-editor-section-view = Изглед
view-editor-section-definition = Дефиниция
view-editor-section-options = Опции
view-editor-name = Име
view-editor-select = Израз SELECT
view-editor-check-option = Опция CHECK
view-editor-column-names = Имена на колоните
view-editor-column-names-placeholder = незадължително — взимат се от SELECT, ако е празно
view-editor-sql-security = SQL сигурност
view-editor-definer = Definer
view-editor-recreate = Пресъздаване вместо замяна
view-editor-recreate-hint = PostgreSQL може само да добавя колони към съществуващ изглед. Преименуването, смяната на типа или пренареждането на колона изисква DROP и CREATE, което премахва и зависимите изгледи и предоставените права.

## Table properties panel

props-title = Свойства — { $table }
props-reading = Четене на статистиката
props-no-stats-view = Изгледът няма собствено хранилище, така че сървърът не отчита статистика за него.
props-no-stats-table = Сървърът не отчете статистика за тази таблица.
props-on-disk = на диска
props-rows-estimated = реда (приблизително)
props-rows-counted = реда (преброени)
props-rows-not-reported = реда — не са отчетени
props-counting = Преброяване
props-count-rows = Преброяване на редовете
props-count-slow = Пълно сканиране на таблицата. Бавно при голяма таблица.
props-storage-data = Данни
props-storage-indexes = Индекси
props-storage-free = Свободно
props-section-structure = Структура
props-columns = Колони
props-indexes = Индекси
props-foreign-keys = Външни ключове
props-count-columns = { $n ->
    [one] { $n } колона
   *[other] { $n } колони
}
props-count-indexes = { $n ->
    [one] { $n } индекс
   *[other] { $n } индекса
}
props-count-keys = { $n ->
    [one] { $n } ключ
   *[other] { $n } ключа
}
props-type = Тип
props-type-view = Изглед
props-type-table = Таблица
props-section-options = Опции
props-engine = Механизъм
props-row-format = Формат на редовете
props-collation = Колация
props-next-auto-increment = Следваща стойност за автоувеличение
props-created = Създадена
props-updated = Обновена
props-comment = Коментар
props-section-partitioning = Разделяне на дялове
props-partition-of = Дял на
props-partition-bound = Граница
props-partitioned-by = Разделена по
props-partitions-more = { $n ->
    [one] и още { $n } дял
   *[other] и още { $n } дяла
}
props-section-indexes = Индекси
props-index-cardinality = кардиналност { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } сканиране
       *[other] { $shown } сканирания
    }
props-index-not-counted = използването не се отчита
props-index-unused = не е използван от последното нулиране на броячите на сървъра
props-never-analysed = Тази таблица никога не е анализирана, така че сървърът няма оценка на броя редове за нея. Изпълнете ANALYZE или пребройте редовете.
props-analysed-at = Оценка към последния ANALYZE, { $when }.
props-read-live = Прочетено на живо — този сървър има information_schema_stats_expiry = 0.
props-cached-up-to = Кеширано от сървъра: тези стойности може да са отпреди до { $age } (information_schema_stats_expiry = { $secs }).
props-age-seconds = { $n } с
props-age-minutes = { $n } мин
props-age-hours = { $n } ч
props-age-days = { $n } д
props-vacuum-note = { $pct }% от версиите на редовете в тази таблица са мъртви и все още не са освободени. VACUUM би върнал това място.
props-copy = Копиране

## Shared panel search box

panel-search-placeholder = Търсене…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = току-що
ago-minutes = преди { $n } мин
ago-hours = преди { $n } ч
ago-days = преди { $n } д
ago-weeks = преди { $n } седм.

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } ред
   *[other] { $n } реда
}
outcome-rows-capped = { $n }+ реда

## Terminal panel header

term-title = ТЕРМИНАЛ
term-db-cli-tip = Отваряне на CLI на базата данни
term-db-cli-tip-read-only = Отваряне на CLI на базата данни — тази връзка е отбелязана само за четене
term-restart-tip = Рестартиране на терминала
term-settings-tip = Настройки на терминала

## Results panel header

results-title = РЕЗУЛТАТИ
results-properties-tip = Свойства на таблицата…
results-monitor-tip = Наблюдение на живо…
results-restore-editor-tip = Възстановяване на редактора
results-collapse-editor-tip = Свиване на редактора

## Query panel header

query-title = ЗАЯВКА
query-no-database = Без база данни

## Shared dialog buttons (continued)

dialog-stop = Спиране

## Copy table modal

copy-table-title = Копиране на { $table }
copy-table-rows-line = След това се копират всички редове на { $table } в нея (не се показва като SQL)
copy-table-into = Копиране в
copy-table-schema = Схема
copy-table-name = Име
copy-table-back = ‹ назад
copy-table-data = Копиране на данните
copy-table-data-hint = Копира редовете, след като таблицата бъде създадена. Ако е изключено, се копира само структурата.
copy-table-what-with-rows = Структурата и редовете. Външните ключове и тригерите не се копират, а генерираните колони се изчисляват наново.
copy-table-what-structure = Само структурата. Външните ключове и тригерите не се копират.
copy-table-choose-database = Изберете базата данни, в която да се копира.
copy-table-unnamed = Дайте име на копието.
copy-table-onto-itself = Таблица не може да се копира върху себе си — дайте друго име на копието.
copy-table-connection-changed = Активната връзка се смени — отворете отново „Копиране в“ от таблицата.
copy-table-schema-not-loaded = Схемата на { $database } не е заредена — разгънете я в дървото и опитайте отново.
copy-table-preview-subject = Копие на { $table }
copy-table-copying = Копиране на редове… { $n }
copy-table-copied = { $n ->
    [one] Копиран е { $n } ред.
   *[other] Копирани са { $n } реда.
}
copy-table-stopped = Спряно преди всички редове да бъдат копирани. Таблицата беше създадена.

## Database, schema and extension editor

database-editor-title-database = Създаване на база данни
database-problem-no-name = Името не може да бъде празно.
database-problem-bad-collation = Името на колация съдържа само букви, цифри и долни черти.
database-editor-title-schema = Създаване на схема
database-editor-title-extension = Създаване на разширение
database-editor-title-schema-in = Създаване на схема в { $database }
database-editor-title-extension-in = Създаване на разширение в { $database }
database-editor-section-database = База данни
database-editor-section-schema = Схема
database-editor-section-extension = Разширение
database-editor-name = Име
database-editor-charset = Кодова таблица
database-editor-collation = Колация
database-editor-owner = Собственик
database-editor-server-default = по подразбиране за сървъра
database-editor-owner-you = вие
database-editor-no-extensions = Няма останали разширения за инсталиране
database-editor-no-charsets = Няма кодови таблици за предлагане
database-editor-no-collations = Няма колации за предлагане
database-editor-no-roles = Няма роли на този сървър

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

script-title = Импортиране в { $database }
script-dialog-title = Изпълнение на SQL скрипт
script-section = SQL скрипт
script-choose-file = Избор на файл…
script-no-file = Не е избран файл
script-reading = Четене на файла
script-hint = Скриптът може да създаде и избере база данни, различна от тази, в която импортирате. Може също да създаде повече от една база данни. Проверете скрипта, за да се уверите, че прави това, което очаквате.
script-hint-namespace = Скриптът може да създаде и избере база данни, различна от тази, в която импортирате. Може също да създаде повече от една база данни. Отварянето от { $namespace } също не поставя скрипта в нея: всичко, което файлът не уточнява, отива там, където указва search_path на сървъра. Проверете скрипта, за да се уверите, че прави това, което очаквате.
script-what-it-does = Какво прави този файл
script-probe-nothing = Този файл не съдържа изрази, които Schemaic може да изпълни.
script-probe-cut-off = Първите { $size } от този файл са един незавършен израз, така че все още няма какво да се обобщи. При изпълнение файлът се изпраща цял.
script-probe-counted = { $count } { $n ->
    [one] израз, прочетен
   *[other] израза, прочетени
} от първите { $size }.
script-destroys = { $count } { $n ->
    [one] израз в този файл унищожава
   *[other] израза в този файл унищожават
} данни — DROP, TRUNCATE или DELETE. Това не може да бъде отменено оттук.
script-unread = Прочетени са само първите { $size } от този файл и нищо в тях не унищожава данни — но останалото не е прегледано. То може да съдържа DROP, TRUNCATE или DELETE, а това не може да бъде отменено оттук.
script-unqualified = { $count } { $n ->
    [one] израз в този файл действа
   *[other] израза в този файл действат
} върху всеки ред на таблица — DELETE, UPDATE или TRUNCATE без WHERE. Това не може да бъде отменено оттук.
script-tx-own = Този файл отваря собствена транзакция, така че се прилага или изцяло, или изобщо не. Schemaic не добавя своя.
script-tx-none = Този файл не отваря транзакция, така че при грешка по средата изпълненото преди нея остава.
script-connecting = Свързване
script-progress-of = { $done } от { $total }
script-progress-read = прочетени { $done }
script-run = Изпълнение
script-done = { $n ->
    [one] Изпълнен е { $n } израз
   *[other] Изпълнени са { $n } израза
} от { $file }.
script-stopped = Спряно след { $n } { $n ->
    [one] израз
   *[other] израза
}.
script-failed = Неуспех: { $message } — преди това { $n ->
    [one] е изпълнен { $n } израз
   *[other] са изпълнени { $n } израза
}.
script-failed-at = Неуспех на ред { $line } от { $file }: { $message } — преди това { $n ->
    [one] е изпълнен { $n } израз
   *[other] са изпълнени { $n } израза
}.
script-durability-rolled-back = { $file } отвори собствена транзакция, така че сървърът върна цялото изпълнение — нищо не е приложено, включително успешните изрази.
script-durability-applied = { $file } не отвори транзакция, така че всеки изпълнен израз остава приложен.
script-durability-applied-before = Всеки израз, изпълнен преди { $file } да отвори собствена транзакция, остава приложен; изпълненото в транзакцията беше върнато.
script-durability-unknown = Всеки вече изпълнен израз остава приложен, освен ако { $file } не е отворил собствена транзакция.

## Schema comparison modal

compare-title = Сравняване на схеми
compare-choose-database = Изберете база данни
compare-no-connection = Няма връзка към същата система, с която да се сравни.
compare-will-change = ще бъде променена
compare-source-of-truth = еталон
compare-idle = Изберете база данни, с която да сравните.
compare-loading = Четене на двете схеми
compare-select-object = Изберете обект, за да видите двете страни.
compare-fk-cycle-note = Външните ключове на тези таблици образуват цикъл — никой ред на създаване не удовлетворява всички, така че генерирана миграция може да бъде отказана по средата.
compare-same-but-stored-differently = Те съвпадат структурно — разликата по-долу е в начина, по който системата съхранява израза, и няма нищо за мигриране.
compare-summary = { $differ } се различават · { $only_here } само тук · { $only_there } само там · { $same } идентични
compare-summary-uncertain = { $n ->
    [one] { $n } съвпадение, за което сравнението не може да гарантира
   *[other] { $n } съвпадения, за които сравнението не може да гарантира
}
compare-filter-placeholder = Филтриране по име
compare-include-identical = Включване на идентичните
compare-select-all = Избиране на всички
compare-select-none = Нито един
compare-hint-held-back = задържано извън плана; бележката по-горе обяснява защо
compare-described = { $kind } { $name }
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = тялото му трябва да бъде прочетено отново от сървъра, преди да може да се приложи
compare-omitted-unread = това сравнение няма израз за него — дефиницията му не можа да бъде прочетена
compare-omitted-no-statement = това сравнение няма израз за откритата разлика
compare-held-partition = това е { $sub ->
        [yes] поддял
       *[no] дял
    } на { $parent }, който е задържан — пресъздайте ги заедно, ръчно
compare-held-partitioning-differs = разделянето му на дялове се различава, а това сравнение не пише израз, който разделя таблица наново — пресъздайте я ръчно
compare-held-sequence-unwritten = това сравнение не пише израз за последователност — копирайте го от `SHOW CREATE SEQUENCE`, който се показва в панела ѝ
compare-held-partitioned-unrestated = тя е разделена на дялове, а никой CREATE TABLE тук не възпроизвежда схемата ѝ на разделяне — създайте ръчно функцията и схемата на разделяне, а след това таблицата
compare-held-sequence-range = броячът в целевата база е достигнал { $at }, извън новия диапазон от { $min } до { $max }, а сървърът отказва да стесни последователност след стойност, която вече е раздал. Рестартирайте я в целевата база (ALTER SEQUENCE … RESTART WITH) от стойност, която никоя таблица не съдържа, и сравнете отново
compare-note-set-aside = { $aside } — { $side ->
        [target] целевата страна
       *[source] източникът
    } съдържа и { $kept }, а имената в целевата страна не различават регистъра, така че тя може да съдържа само едното от двете; сравнено е { $kept }
compare-note-binding = Тип псевдоним { $alias }: { $bound ->
        [rule] правилото { $rule }, свързано
        [default] стойността по подразбиране { $default }, свързана
       *[both] правилото { $rule } и стойността по подразбиране { $default }, свързани
    } с него в източника, не се сравняват и не се пренасят — свържете ги ръчно (sp_bindrule / sp_bindefault).
compare-hint-blocked = тялото трябва да бъде прочетено отново, преди това да може да се приложи
compare-hint-uncertain = индекс, който този модел чете само частично — съвпадението тук не е сигурно
compare-preview-migration = Преглед на миграцията
compare-plan-subject = { $n ->
    [one] { $n } обект в { $database }
   *[other] { $n } обекта в { $database }
}
compare-selected = { $n ->
        [0] Нищо не е избрано.
        [one] { $n } избран обект
       *[other] { $n } избрани обекта
    }
compare-incomparable = { $left } и { $right } не могат да бъдат сравнени. Имената на типовете, стойностите по подразбиране и формата на индексите не се пренасят между системите, така че всяка миграция, генерирана от разликата, би била грешна.
compare-group-table = Таблици
compare-group-view = Изгледи
compare-group-trigger = Тригери
compare-kind-enum = изброим тип
compare-kind-domain = домейн
compare-kind-xml-schema-collection = колекция от XML схеми
compare-kind-alias-type = тип псевдоним
compare-kind-sequence = последователност
compare-kind-table = таблица
compare-kind-view = изглед
compare-kind-function = функция
compare-kind-procedure = процедура
compare-kind-trigger = тригер
compare-kind-event = събитие
compare-kind-synonym = синоним

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Нито една от двете бази данни не съдържа нещо за сравняване.
compare-empty-agrees = Тези две схеми съвпадат обект по обект.
compare-empty-agrees-noted = Всички обекти съвпадат, но не всичко можа да бъде сравнено — вижте бележките по-горе.
compare-empty-no-match = Нищо не съвпада с този филтър.
compare-empty-only-identical = Всичко, което съвпада с този филтър, е идентично — включете „Включване на идентичните“, за да го видите.

## Schema editors — shared (continued)

editor-comment-placeholder = какво прави

## Event editor

event-editor-title-edit = Редактиране на събитието { $event }
event-editor-title-create = Създаване на събитие в { $location }
event-editor-section-event = Събитие
event-editor-section-schedule = График
event-editor-section-body = Тяло
event-editor-section-options = Опции
event-editor-name = Име
event-editor-body = Тяло
event-editor-runs = Изпълнява се
event-editor-runs-repeating = Повтарящо се
event-editor-runs-once = Еднократно
event-editor-at = В
event-editor-every = На всеки
event-editor-starts = Начало
event-editor-ends = Край
event-editor-status = Състояние
event-editor-status-enabled = Включено
event-editor-status-disabled = Изключено
event-editor-status-replica = Изключено в репликата
event-editor-preserve = Запазване след приключване
event-editor-preserve-hint = ON COMPLETION PRESERVE. Изключено е по подразбиране в MySQL и означава, че сървърът изтрива събитието, след като мине последното му изпълнение — което за еднократно събитие е веднага след изпълнението му.
event-editor-definer = Definer
event-editor-comment = Коментар
event-editor-reading-source = Четене на изходния код на събитието…
event-editor-source-stale = Истинският изходен код на събитието пристигна, след като започнахте да пишете, така че това тяло е изградено върху копието от каталога — което не е това, което съхранява сървърът. Затворете и отворете отново, за да го редактирате.
event-problem-no-name = Събитието трябва да има име.
event-problem-no-time = Еднократното събитие трябва да има час на изпълнение.
event-problem-no-interval = Повтарящото се събитие трябва да има интервал.
event-problem-no-unit = Повтарящото се събитие трябва да има единица за интервала (DAY, HOUR…).
event-problem-no-body = Събитието трябва да има тяло.
event-name-clash = Тук вече има събитие с име { $name }.

## Routine editor

routine-editor-title-edit-function = Редактиране на функцията { $routine }
routine-editor-title-edit-procedure = Редактиране на процедурата { $routine }
routine-editor-title-create-function = Създаване на функция в { $location }
routine-editor-title-create-procedure = Създаване на процедура в { $location }
routine-editor-section-function = Функция
routine-editor-section-procedure = Процедура
routine-editor-section-body = Тяло
routine-editor-section-options = Опции
routine-editor-name = Име
routine-editor-parameters = Параметри
routine-editor-returns = Връща
routine-editor-returns-placeholder-mssql = int или TABLE
routine-editor-language = Език
routine-editor-body = Тяло
routine-editor-recompile = Повторно компилиране
routine-editor-recompile-hint = WITH RECOMPILE — нов план при всяко извикване, никога кеширан.
routine-editor-schema-binding = Обвързване със схемата
routine-editor-schema-binding-hint = WITH SCHEMABINDING — таблиците и изгледите, които чете, не могат да бъдат променени под нея.
routine-editor-execute-as = Изпълнение като
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER или потребител
routine-editor-null-input = Връща NULL при вход NULL
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — тялото не се изпълнява, когато аргумент е NULL.
routine-editor-inlining = Вграждане
routine-editor-inlining-automatic = Автоматично
routine-editor-comment = Коментар
routine-editor-volatility = Променливост
routine-editor-strict = Strict
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Изпълнява се с правата на собственика вместо с тези на извикващия. Фиксирайте search_path по-долу, когато използвате това.
routine-editor-security-definer-hint-mysql = Изпълнява се с правата на DEFINER вместо с тези на извикващия. Това е по подразбиране в MySQL — изключването му е SQL SECURITY INVOKER.
routine-editor-settings = Настройки
routine-editor-add-setting = Добавяне на настройка
routine-editor-deterministic = Детерминистична
routine-editor-deterministic-hint = Гарантира един и същ резултат за едни и същи аргументи. Сървър с двоично журналиране отказва недетерминистична подпрограма, освен ако не се доверява на създателите.
routine-editor-data-access = Достъп до данни
routine-editor-definer = Definer
routine-editor-reading-source = Четене на изходния код на подпрограмата…
routine-editor-source-stale = Истинският изходен код на подпрограмата пристигна, след като започнахте да пишете, така че това тяло е изградено върху копието от каталога — което не е това, което съхранява сървърът. Затворете и отворете отново, за да го редактирате.
routine-problem-function-no-name = Функцията трябва да има име.
routine-problem-procedure-no-name = Процедурата трябва да има име.
routine-problem-function-no-language = Функцията трябва да има език (plpgsql, sql).
routine-problem-procedure-no-language = Процедурата трябва да има език (plpgsql, sql).
routine-problem-no-return-type = Функцията трябва да има тип на връщаната стойност.
routine-problem-procedure-returns = Процедурата не връща нищо — изчистете типа на връщаната стойност.
routine-problem-function-no-body = Функцията трябва да има тяло.
routine-problem-procedure-no-body = Процедурата трябва да има тяло.
routine-problem-bare-parameter = SQL Server именува всеки параметър с @ — @sku varchar(20), @n int OUTPUT.
routine-problem-procedure-refuses = Процедура не може да има WITH { $option }.
routine-problem-scalar-refuses = Скаларна функция не може да има WITH { $option }.
routine-problem-inline-refuses = Вградена таблична функция не може да има WITH { $option }.
routine-problem-multi-refuses = Таблична функция с няколко израза не може да има WITH { $option }.
routine-problem-procedure-refuses-native = Процедура не може да има WITH { $option }, щом е компилирана в машинен код.
routine-problem-scalar-refuses-native = Скаларна функция не може да има WITH { $option }, щом е компилирана в машинен код.
routine-problem-inline-refuses-native = Вградена таблична функция не може да има WITH { $option }, щом е компилирана в машинен код.
routine-problem-multi-refuses-native = Таблична функция с няколко израза не може да има WITH { $option }, щом е компилирана в машинен код.
routine-problem-native-needs-schemabinding = Подпрограма, компилирана в машинен код, изисква WITH SCHEMABINDING — SQL Server изисква двете заедно.
routine-problem-inline-with-execute-as = INLINE = ON не може да се комбинира с EXECUTE AS — функция, която се изпълнява като някой друг, никога не се вгражда.
routine-problem-numbered-rename = { $was } е начало на група номерирани процедури ({ $group }) и преименуването ѝ би премахнало всички тях. Запазете името, за да я редактирате.
routine-problem-trigger-function-args = Тригерна функция не приема декларирани аргументи — те пристигат в TG_ARGV.
routine-clash-function-refused = Тук вече има функция с име { $name }. В тази система преименуването е премахване и създаване, така че сървърът отказва това и нищо не се променя — изберете друго име или първо премахнете { $name }.
routine-clash-procedure-refused = Тук вече има процедура с име { $name }. В тази система преименуването е премахване и създаване, така че сървърът отказва това и нищо не се променя — изберете друго име или първо премахнете { $name }.
routine-clash-function-destroys = Тук вече има функция с име { $name }. В тази система преименуването е премахване и създаване, така че прилагането на това би унищожило { $original } и би оставило { $name } както си е.
routine-clash-procedure-destroys = Тук вече има процедура с име { $name }. В тази система преименуването е премахване и създаване, така че прилагането на това би унищожило { $original } и би оставило { $name } както си е.
routine-clash-function-destroys-new = Тук вече има функция с име { $name }. В тази система преименуването е премахване и създаване, така че прилагането на това би унищожило оригинала и би оставило { $name } както си е.
routine-clash-procedure-destroys-new = Тук вече има процедура с име { $name }. В тази система преименуването е премахване и създаване, така че прилагането на това би унищожило оригинала и би оставило { $name } както си е.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Търсене на акаунти
users-loading-accounts = Зареждане на акаунтите
users-unsupported = Системата на тази връзка няма потребителски акаунти.
users-none = Този сървър не отчита акаунти.
users-no-match = Няма акаунт, който да съвпада с „{ $needle }“.
users-pick-account = Изберете акаунт, за да видите какво може да прави.
users-section-attributes = Атрибути
users-section-privileges = Привилегии
users-attr-authentication = Удостоверяване
users-attr-locked = Заключен
users-attr-password-expired = Паролата е изтекла
users-attr-superuser = Суперпотребител
users-attr-create-role = Създаване на роли
users-attr-create-database = Създаване на бази данни
users-attr-replication = Репликация
users-attr-bypass-rls = Заобикаляне на RLS
users-attr-inherit = Наследяване
users-attr-connection-limit = Ограничение на връзките
users-attr-valid-until = Валиден до
users-attr-type = Тип
users-attr-disabled = Деактивиран
users-attr-default-database = База данни по подразбиране
users-attr-server-roles = Сървърни роли
users-attr-login = Вход
users-attr-default-schema = Схема по подразбиране
users-attr-member-of = Член на
users-attr-yes = Да
users-attr-no = Не
users-login-type-sql = SQL вход
users-login-type-windows = Windows вход
users-login-type-windows-group = Windows група
users-kind-user = Потребител
users-kind-role = Роля
users-kind-login = Вход
users-maintained-by-server = поддържан от сървъра
users-loading-privileges = Зареждане на привилегиите
users-no-privileges = Този акаунт няма привилегии.
users-note-my-own-account = Тази връзка не може да чете mysql.user, така че това не е списъкът с акаунти на сървъра — това е единственият акаунт, който тази връзка вижда: нейният собствен. Свържете се с акаунт със SELECT върху базата данни mysql, за да разгледате останалите.
users-note-ms-logins-elsewhere = Azure SQL Database пази входовете си в master и те не се управляват тук: показани са потребителите и ролите на базата данни.
users-note-ms-logins-only = Показани са само входовете на сървъра: изберете база данни, за да видите нейните потребители и роли.
users-note-ms-short = Този вход няма VIEW ANY DEFINITION, така че сървърът му показва само субектите, които може да вижда — списъкът може да е непълен.
users-note-my-scope = Привилегиите, получени чрез предоставена роля, не се разгъват: SHOW GRANTS показва какво е предоставено пряко на акаунта и ролите, в които членува, но не и какво притежават самите роли. Отворете привилегиите на съответната роля, за да ги видите.
users-note-pg-scope = Привилегиите за схеми, таблици и последователности са тези в { $database } — PostgreSQL ги пази в собствения каталог на всяка база данни, така че привилегиите в други бази данни не са показани. EXECUTE за функции и процедури и привилегиите, предоставени за отделна колона, също не са показани. Привилегиите, получени чрез членство в роля или предоставени на PUBLIC, не се разгъват, нито пък привилегиите, които роля има като собственик на обект или като суперпотребител.
users-note-pg-no-database = Привилегиите за схеми, таблици и последователности не са показани: PostgreSQL ги пази в собствения каталог на всяка база данни, а тази връзка няма избрана база данни. Отворете раздел за заявки към база данни, за да ги видите. EXECUTE за функции и процедури и привилегиите, предоставени за отделна колона, също не са показани. Привилегиите, получени чрез членство в роля или предоставени на PUBLIC, не се разгъват, нито пък привилегиите, които роля има като собственик на обект или като суперпотребител.
users-note-pg-superuser = Тази роля е SUPERUSER: тя заобикаля всяка проверка на правата, така че има всички привилегии за всеки обект, независимо дали нещо по-долу го казва.
users-note-pg-predefined = { $role } е една от предварително дефинираните роли на PostgreSQL: какво може да прави е вградено в сървъра, а не предоставено, така че не е записано в нито един каталог, който се чете тук.
users-note-pg-owns = { $n ->
    [one] Тази роля притежава { $n } обект в { $database } (схеми, таблици, последователности или самата база данни). Собственикът има всички привилегии за това, което притежава, а PostgreSQL не записва предоставяне за това, така че нищо от него не е показано по-долу.
   *[other] Тази роля притежава { $n } обекта в { $database } (схеми, таблици, последователности или самата база данни). Собственикът има всички привилегии за това, което притежава, а PostgreSQL не записва предоставяне за това, така че нищо от него не е показано по-долу.
}
users-note-pg-owns-cluster = { $n ->
    [one] Тази роля притежава { $n } обект в този клъстер (схеми, таблици, последователности или самата база данни). Собственикът има всички привилегии за това, което притежава, а PostgreSQL не записва предоставяне за това, така че нищо от него не е показано по-долу.
   *[other] Тази роля притежава { $n } обекта в този клъстер (схеми, таблици, последователности или самата база данни). Собственикът има всички привилегии за това, което притежава, а PostgreSQL не записва предоставяне за това, така че нищо от него не е показано по-долу.
}
users-note-ms-unshown = { $n ->
    [one] { $n } разрешение не е показано тук, за { $classes }: сървърът го отчита за защитим обект, който този списък не може да назове.
   *[other] { $n } разрешения не са показани тук, за { $classes }: сървърът ги отчита за защитими обекти, които този списък не може да назове.
}
users-note-ms-pick-database = Разрешенията на потребител на база данни са за тази база данни — изберете една, за да ги видите.
users-statements-capped = Показани са първите { $shown } от { $total } израза. „Копиране на привилегиите“ взима всички.
users-new-account = Нов акаунт
users-system-account = Този акаунт принадлежи на сървъра, така че Schemaic няма да го променя.
users-reset-password = Нулиране на паролата
users-gate-no-database = Отворете раздел за заявки към база данни, за да променяте привилегии — изразът трябва да се изпълни в такава.
users-gate-read-only = Тази връзка е само за четене.
users-count = { $n } { $n ->
    [one] акаунт
   *[other] акаунта
}
users-count-filtered = { $shown } от { $n } акаунта
users-copy-privileges = Копиране на привилегиите

## Schema editors — shared (continued)

editor-row-remove = Премахване

## Trigger editor

trigger-editor-title = Тригери върху { $table }
trigger-problem-no-name = Тригерът трябва да има име.
trigger-problem-no-table = Тригерът трябва да има таблица, върху която да се задейства.
trigger-problem-no-event = Тригерът трябва да има поне едно събитие, при което да се задейства.
trigger-problem-pg-no-function = Тригер в PostgreSQL трябва да има функция за изпълнение.
trigger-problem-pg-body = Тригер в PostgreSQL изпълнява функция, а не тяло — изберете или напишете такава за изпълнение.
trigger-problem-truncate-row-level = Тригер за TRUNCATE трябва да е FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = Тригер INSTEAD OF трябва да е FOR EACH ROW.
trigger-problem-instead-of-view-only = Само изглед може да има тригер INSTEAD OF — таблица приема BEFORE или AFTER.
trigger-problem-pg-view-row-timing = Тригер BEFORE или AFTER на изглед трябва да е FOR EACH STATEMENT — използвайте INSTEAD OF, за да действате върху редове.
trigger-problem-update-of-without-update = UPDATE OF посочва колони за тригер за UPDATE.
trigger-problem-pg-instead-of-columns = Тригер INSTEAD OF не приема списък с колони — PostgreSQL го задейства при всеки UPDATE на изгледа.
trigger-problem-one-event = { $engine } задейства тригер при едно събитие — създайте отделен тригер за всяко събитие.
trigger-problem-no-truncate = { $engine } няма тригер за TRUNCATE.
trigger-problem-sqlite-view-timing = Тригерът на изглед трябва да е INSTEAD OF — SQLite няма тригер BEFORE или AFTER за изглед.
trigger-problem-sqlite-row-only = SQLite има само тригери FOR EACH ROW — няма тригер на ниво израз.
trigger-problem-no-body = Тригер в { $engine } трябва да има тяло.
trigger-problem-sqlite-not-block = Тялото на тригер в SQLite трябва да е блок BEGIN … END с поне един израз.
trigger-problem-runs-body = Тригер в { $engine } изпълнява тяло, а не функция — напишете изразите за изпълнение.
trigger-problem-ms-no-before = SQL Server няма тригер BEFORE — използвайте INSTEAD OF, за да действате преди записа.
trigger-problem-ms-view-timing = В SQL Server тригерът на изглед трябва да е INSTEAD OF.
trigger-problem-ms-statement-only = SQL Server задейства тригер веднъж на израз — четете редовете от inserted и deleted.
trigger-problem-ms-instead-of-rank = Тригер INSTEAD OF не може да има ред на задействане — върнете всяко събитие на „Произволен ред“.
trigger-problem-first-without-event = Задейства се първи при { $event }, за което не е тригер.
trigger-problem-last-without-event = Задейства се последен при { $event }, за което не е тригер.
trigger-problem-no-when = { $engine } няма условие WHEN — поставете проверката в тялото с IF.
trigger-problem-ms-no-update-of = SQL Server няма UPDATE OF — проверявайте UPDATE(column) в тялото.
trigger-problem-my-no-view = MySQL не приема тригер върху изглед.
trigger-problem-my-no-instead-of = MySQL няма тригер INSTEAD OF — такива има в PostgreSQL.
trigger-problem-my-no-update-of = MySQL няма UPDATE OF — тригерът вижда всяка колона.
trigger-problem-constraint-trigger = Schemaic не може да редактира тригера за ограничение { $name } — не моделира настройките за отлагане, които той носи.
trigger-problem-not-editable = Schemaic не може да редактира тригера { $name } — дефиницията му е криптирана, скрита от този вход или използва опция, която Schemaic не моделира. Променете го чрез SQL.
trigger-problem-twice = Два тригера носят името { $name }.
trigger-problem-first-clash = { $n ->
    [2] Само един тригер може да се задейства първи при { $event } — { $names } и { $last } са зададени така и двата.
   *[other] Само един тригер може да се задейства първи при { $event } — { $names } и { $last } са зададени така всички.
}
trigger-problem-last-clash = { $n ->
    [2] Само един тригер може да се задейства последен при { $event } — { $names } и { $last } са зададени така и двата.
   *[other] Само един тригер може да се задейства последен при { $event } — { $names } и { $last } са зададени така всички.
}
trigger-editor-preview-subject = тригери върху { $table }
trigger-editor-none-selected = Не е избран тригер.
trigger-editor-readonly-constraint = Това е тригер за ограничение. Schemaic не моделира настройките за отлагане, които той носи, така че тук може да бъде премахнат, но не и редактиран — променете го чрез SQL.
trigger-editor-readonly-hidden = Дефиницията на този тригер е криптирана или скрита от този вход, така че тук той може да бъде премахнат, но не и редактиран.
trigger-editor-readonly-unmodelled = Този тригер използва опция, която Schemaic не моделира, така че тук може да бъде премахнат, но не и редактиран — променете го чрез SQL.
trigger-editor-section-trigger = Тригер
trigger-editor-section-condition = Условие
trigger-editor-section-action = Действие
trigger-editor-name = Име
trigger-editor-timing = Момент
trigger-editor-event = Събитие
trigger-editor-fires = Задейства се
trigger-editor-truncate-hint = Само на ниво израз.
trigger-editor-not-for-replication-hint = Не се задейства, докато агент за репликация пише в таблицата.
trigger-editor-rank-any = Произволен ред
trigger-editor-rank-first = Задейства се първи
trigger-editor-rank-last = Задейства се последен
trigger-editor-when = Кога
trigger-editor-of-columns = За колоните
trigger-editor-of-columns-placeholder = незадължително — всички колони, ако е празно
trigger-editor-body = Тяло
trigger-editor-function = Функция
trigger-editor-edit-function = Редактиране
trigger-editor-new-function = Нова функция
trigger-editor-arguments = Аргументи
trigger-editor-add-argument = Добавяне на аргумент

## Schema editors — shared (continued)

editor-row-move-up = Преместване нагоре
editor-row-move-down = Преместване надолу

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

object-editor-title-edit = Редактиране: { $kind } { $name }
object-editor-title-create = Създаване: { $kind } в { $location }
object-editor-name = Име
object-editor-comment = Коментар
object-editor-type = Тип
object-editor-default = По подразбиране
object-editor-section-type = Тип
object-editor-section-values = Стойности
object-editor-section-domain = Домейн
object-editor-section-constraints = Ограничения
object-editor-section-sequence = Последователност
object-editor-section-position = Позиция
object-editor-section-synonym = Синоним
object-editor-section-alias-type = Тип псевдоним
object-editor-section-xml-schema-collection = Колекция от XML схеми
object-editor-section-stands-for = Заместник на
object-editor-value-placeholder = стойност
object-editor-add-value = Добавяне на стойност
object-editor-values-order = В реда на сравнение — това е редът, който използва ORDER BY
object-editor-add-constraint = Добавяне на ограничение
object-editor-no-base-types = Тази система не предлага базови типове
object-editor-not-null-hint = Всяка колона от този домейн отхвърля NULL. Включването е неуспешно, ако някоя вече съдържа такава стойност.
object-editor-stores = Съхранява
object-editor-increment = Стъпка
object-editor-start = Начало
object-editor-cache = Кеш
object-editor-minimum = Минимум
object-editor-maximum = Максимум
object-editor-not-whole-number = { $field } трябва да е цяло число.
object-problem-type-no-name = Типът трябва да има име.
object-problem-duplicate-value = Стойността { $value } е посочена повече от веднъж.
object-problem-empty-value = Стойността не може да бъде празна — PostgreSQL никога не може да я премахне.
object-problem-domain-no-name = Домейнът трябва да има име.
object-problem-domain-no-base = Домейнът трябва да има тип, на който да се основава.
object-problem-constraint-no-name = Всяко ограничение трябва да има име.
object-problem-constraint-no-predicate = Ограничението { $name } няма предикат.
object-problem-constraint-twice = Ограничението { $name } е посочено два пъти.
object-problem-sequence-no-name = Последователността трябва да има име.
object-problem-zero-increment = Последователност не може да има стъпка 0.
object-problem-min-above-max = Минимумът ({ $min }) е над максимума ({ $max }).
object-problem-start-outside = Началната стойност ({ $start }) е извън { $min }..{ $max }.
object-problem-cache-below-one = Кешът трябва да е поне 1.
object-problem-does-not-fit = { $min }..{ $max } не се побира в { $data_type }.
object-problem-restart-outside = Рестартирането от { $restart } е извън { $min }..{ $max }.
object-problem-counter-outside = Последователността е на { $last }, извън новия диапазон { $min }..{ $max }. Задайте стойност за рестартиране в диапазона.
object-problem-no-name = Името не може да бъде празно.
object-problem-tsql-no-type = Последователността трябва да има тип.
object-problem-start-not-whole = Началото трябва да е цяло число.
object-problem-increment-not-whole = Стъпката трябва да е цяло число.
object-problem-minimum-not-whole = Минимумът трябва да е цяло число.
object-problem-maximum-not-whole = Максимумът трябва да е цяло число.
object-problem-tsql-zero-increment = Стъпката не може да е 0.
object-problem-min-not-below-max = Минимумът трябва да е под максимума.
object-problem-started-outside = Започнала е от { $start }, извън новия диапазон: задайте „Рестартиране от“ на стойност между минимума и максимума.
object-problem-tsql-start-outside = Началото трябва да е между минимума и максимума.
object-problem-cache-not-positive = Размерът на кеша трябва да е цяло число над 0.
object-problem-restart-not-whole = Стойността за рестартиране трябва да е цяло число.
object-problem-tsql-restart-outside = Стойността за рестартиране трябва да е между минимума и максимума.
object-problem-synonym-no-target = Синонимът трябва да има обект, който замества.
object-problem-alias-no-base = Типът псевдоним трябва да има базов тип.
object-editor-cycle = Цикличност
object-editor-cycle-hint = Преминава към другия край на диапазона, вместо да се провали, когато последователността се изчерпи.
object-editor-restart-at = Рестартиране от — броячът последно е раздал { $last }
object-editor-restart-at-unused = Рестартиране от — броячът не е използван
object-editor-restart-not-whole = „Рестартиране от“ трябва да е цяло число.
object-editor-restart-placeholder = оставете празно, за да се запази позицията
object-editor-tsql-restart-placeholder = празно я запазва, освен ако типът не се промени
object-editor-owned-by = Притежавана от
object-editor-detach = Откачане от колоната ѝ
object-editor-detach-hint = Последователността вече не се премахва заедно с колоната и я надживява.
object-editor-cache-hint = Раздава стойности от паметта, по-бързо, с цената на празнина след рестартиране на сървъра.
object-editor-cache-size = Размер на кеша
object-editor-cache-size-placeholder = по подразбиране за сървъра
object-editor-synonym-server = Сървър
object-editor-synonym-server-placeholder = този сървър
object-editor-synonym-database = База данни
object-editor-synonym-database-placeholder = тази база данни
object-editor-synonym-schema = Схема
object-editor-synonym-schema-placeholder = схемата по подразбиране
object-editor-synonym-object = Обект
object-editor-synonym-object-placeholder = таблица, изглед или подпрограма
object-editor-base-type = Базов тип
object-editor-nullable = Допуска NULL
object-editor-nullable-hint = Колона от този тип приема NULL, освен ако колоната не указва друго.
object-editor-schemas = Схеми

## Account and grant editors (Users and privileges)

account-editor-title-create = Създаване на акаунт
account-editor-title-reset = Нулиране на паролата
account-editor-title-privileges = Привилегии — { $account }
account-editor-account = Акаунт
account-editor-kind = Вид
account-editor-name = Име
account-editor-host = Хост
account-editor-host-placeholder = % (всеки хост)
account-editor-for-login = За вход
account-editor-for-login-placeholder = вход — празно за потребител без вход
account-editor-also-user = И потребител в { $database }
account-editor-password = Парола
account-editor-confirm = Потвърждение
account-editor-action = Действие
account-editor-action-grant = Предоставяне
account-editor-action-revoke = Отнемане
account-editor-subject = Предмет
account-editor-subject-privileges = Привилегии
account-editor-subject-role = Роля
account-editor-role = Роля
account-editor-no-roles = Този сървър няма роли
account-editor-with-admin-option = С право на администриране
account-editor-level = Ниво
account-editor-level-global = Целият сървър
account-editor-level-database = База данни
account-editor-level-schema = Схема
account-editor-level-table = Таблица
account-editor-level-sequence = Последователност
account-editor-no-suggestions = Няма предложения
account-editor-section-privileges = Привилегии
account-editor-with-grant-option = С право на предоставяне
account-editor-needs-role = Посочете роля.
account-editor-needs-privileges = Изберете ниво, посочете за какво се отнася и отметнете привилегия.
account-editor-password-required = Изисква се парола.
account-editor-name-required = Изисква се име.
account-editor-entra-only-user = От Microsoft Entra се създава само потребител на база данни.
account-editor-entra-no-password = Потребител от Entra няма вход или парола тук — Entra го вписва.
account-editor-login-needs-password = Входът трябва да има парола.
account-editor-user-for-login-password = Потребител за вход се вписва с паролата на входа — оставете това празно.
account-editor-confirm-password = Въведете паролата отново, за да я потвърдите.
account-editor-passwords-differ = Двете пароли се различават.
account-editor-password-hashed = Прегледаният SQL, който е изразът, който ще се изпълни, съдържа хеш на паролата във формата, в който този сървър я съхранява, а не самата парола — освен ако съдържа знаци, които входът би нормализирал (букви с пълна ширина, лигатури, непрекъсваеми интервали), или сървърът използва разширение за проверка на паролите; тогава тя се изпраща както е въведена.
account-editor-password-as-typed = Паролата се вижда в прегледания SQL, който е изразът, който ще се изпълни.

## Table designer

designer-title-edit = Редактиране на { $table }
designer-title-create = Създаване на таблица в { $location }
designer-tab-table = Таблица
designer-tab-columns = Колони
designer-tab-indexes = Индекси
designer-tab-foreign-keys = Външни ключове
designer-tab-checks = Проверки
designer-list-add = Добавяне
designer-no-suggestions = Няма предложения
designer-name = Име
designer-comment = Коментар
designer-table-comment-placeholder = За какво е тази таблица
designer-engine = Механизъм
designer-collation = Колация
designer-no-column = Не е избрана колона.
designer-type = Тип
designer-type-hint = Записва се направо — сървърът решава какво означава.
designer-nullable = Допуска NULL
designer-nullable-hint = Разрешава NULL в тази колона.
designer-primary-key = Първичен ключ
designer-primary-key-hint = Част от първичния ключ на таблицата, в реда на колоните.
designer-identity = Identity
designer-auto-increment = Автоувеличение
designer-identity-hint = Сървърът задава стойността ({ $keyword }).
designer-default = По подразбиране
designer-default-hint = SQL текст, както бихте го написали: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Генерирана от
designer-generated-hint = Израз, напр. qty * price. Генерирана колона няма стойност по подразбиране.
designer-on-update = При обновяване
designer-index-unique-detail = уникален
designer-index-redundant-detail = излишен
designer-hint-redundant = Индексът { $index } се покрива от { $cover }: всяко търсене, което той обслужва, се обслужва и от другия.
table-problem-no-name = Таблицата трябва да има име.
table-problem-no-columns = Таблицата трябва да има поне една колона.
table-problem-column-no-name = Всяка колона трябва да има име.
table-problem-column-twice = Две колони носят името { $name }.
table-problem-column-no-type = Колоната { $name } трябва да има тип.
table-problem-key-not-column = Първичният ключ посочва { $column }, която не е колона.
table-problem-index-no-name = Всеки индекс трябва да има име.
table-problem-index-twice = Два индекса носят името { $name }.
table-problem-index-no-columns = Индексът { $index } няма колони.
table-problem-xml-one-column = Индексът { $index } трябва да посочва точно една колона.
table-problem-xml-no-options = Индексът { $index } не може да е уникален, да включва колони или да е филтриран: XML или пространствен индекс не приема нищо от това.
table-problem-xml-no-using = Индексът { $index } трябва да има клауза USING XML INDEX … FOR …: вторичен XML индекс се изгражда върху първичен или селективен.
table-problem-selective-no-paths = Индексът { $index } трябва да има пътища FOR (…): селективният XML индекс се състои от пътищата, които издига.
table-problem-xml-parent-gone = Индексът { $index } е изграден върху { $parent }, който не е първичен или селективен XML индекс на тази таблица: той изчезва заедно с родителя си.
table-problem-not-orderable = Индексът { $index } не може да бъде подреден: само columnstore индекс има ORDER.
table-problem-order-not-column = Индексът { $index } е подреден по { $column }, която не е колона.
table-problem-order-not-stored = Индексът { $index } е подреден по { $column }, която той не съхранява.
table-problem-columnstore-unique = Индексът { $index } не може да е уникален: columnstore индексът съхранява колони, а не ключове.
table-problem-columnstore-include = Индексът { $index } не може да включва колони: columnstore индексът изброява всяка колона, която съхранява.
table-problem-columnstore-sorted = Индексът { $index } не може да сортира колоните си: колона в columnstore няма посока.
table-problem-clustered-columnstore-filtered = Индексът { $index } не може да е филтриран: клъстерираният columnstore индекс съхранява всеки ред.
table-problem-clustered-include = Индексът { $index } не може да включва колони: клъстерираният индекс вече съдържа всички колони.
table-problem-unique-include = Индексът { $index } не може да включва колони: ограничението за уникалност тук няма INCLUDE.
table-problem-clustered-filtered = Индексът { $index } не може да е филтриран: клъстерираният индекс покрива всеки ред.
table-problem-constraint-filtered = Индексът { $index } не може да е филтриран: той стои зад ограничение UNIQUE, което тук не приема условие. Премахнете го и добавете уникален индекс с условието.
table-problem-not-partial = Индексът { $index } не може да е филтриран: този индекс не може да е частичен тук.
table-problem-index-not-column = Индексът { $index } посочва { $column }, която не е колона.
table-problem-include-not-column = Индексът { $index } включва { $column }, която не е колона.
table-problem-cluster-clash = Таблицата може да има само един клъстериран индекс, а { $clustered } я клъстерират — направете всички освен един неклъстерирани.
table-problem-cluster-key = първичният ключ
table-problem-cluster-index = индексът { $index }
table-problem-cluster-and = и
table-problem-fk-no-name = Всеки външен ключ трябва да има име.
table-problem-fk-twice = Два външни ключа носят името { $name }.
table-problem-fk-no-table = Външният ключ { $fk } не сочи към таблица.
table-problem-fk-unpaired = Външният ключ { $fk } трябва да съпостави всяка колона с една, към която сочи.
table-problem-fk-not-column = Външният ключ { $fk } посочва { $column }, която не е колона.
table-problem-check-no-name = Всяко ограничение CHECK трябва да има име.
table-problem-check-twice = Две ограничения CHECK носят името { $name }.
table-problem-check-no-predicate = Проверката { $name } няма предикат.
table-problem-without-rowid-no-key = Таблица WITHOUT ROWID трябва да има първичен ключ.
table-problem-strict-type = Таблиците STRICT приемат само INT, INTEGER, REAL, TEXT, BLOB или ANY — { $column } е { $data_type }.
designer-hint-unindexed-fk = Външният ключ { $fk } няма индекс, започващ с колоните му, така че изтриване или промяна на ключ в таблицата, към която сочи, сканира тази.
designer-no-index = Не е избран индекс. Първичният ключ се задава при колоните.
designer-columns = Колони
designer-index-columns-hint-single = Единствената колона от тип xml, geometry или geography, която индексира.
designer-index-columns-hint-columnstore = Колоните, които съхранява, разделени със запетаи.
designer-index-columns-hint-prefix = Разделени със запетаи, в реда на ключа. bio(20) е дължина на префикс; добавете DESC за низходящо сортиране.
designer-index-columns-hint = Разделени със запетаи, в реда на ключа. Добавете DESC за низходяща колона.
designer-index-method = Метод
designer-index-method-hint = Оставете празно за подразбирания (btree).
designer-index-filter = Само редове, където
designer-index-filter-hint = Условието на частичен индекс, без WHERE.
designer-index-storage = Съхранение
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Клъстериран columnstore
designer-storage-primary-xml = Първичен XML
designer-storage-selective-xml = Селективен XML
designer-storage-secondary-xml = Вторичен XML
designer-storage-spatial = Пространствен
designer-index-using = Използване
designer-index-using-hint = Клаузата след колоната: USING XML INDEX … FOR PATH, VALUE или PROPERTY на вторичен XML индекс (или FOR (path) на селективен), [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …) на селективен XML индекс или USING … WITH (…) на пространствен индекс.
designer-index-order = Подредба
designer-index-order-hint = Колоните, по които се сортират сегментите, разделени със запетаи — от SQL Server 2022 нататък, а за неклъстериран индекс от 2025, който сортира само по колони, които съхранява.
designer-index-include = Включване
designer-index-include-hint = Колони, разделени със запетаи, които се пазят в индекса, но не са част от ключа му.
designer-index-unique = Уникален
designer-index-unique-hint = Отказва дублиращи се стойности в тези колони.
designer-fk-detail-unindexed = → { $table } · без индекс
designer-no-fk = Не е избран външен ключ.
designer-fk-ref-table = Сочи към таблица
designer-fk-on-delete = При изтриване
designer-fk-columns-hint = Разделени със запетаи, съпоставени по ред с колоните, към които сочат.
designer-fk-ref-columns = Сочи към колони
designer-no-check = Не е избрана проверка.
designer-check-expression = Израз
designer-check-expression-hint = Условието, на което трябва да отговаря всеки ред. Пише се без CHECK (…).
designer-check-enforced = Прилага се
designer-check-enforced-hint = Ако е изключено, ограничението се записва, без да се прилага — приемат се и съществуващите, и новите редове.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Управление на връзките
connections-new = Нова връзка
connections-import = Импортиране на връзки
connections-export = Експортиране на връзки
connections-duplicate = Дублиране
connections-delete = Изтриване
connections-test = Тест
connections-save = Запазване
connections-test-failed = Връзката е неуспешна
connections-browse = Преглед…
connections-section-general = Общи
connections-section-safeguards = Предпазни мерки
connections-section-database = База данни
connections-section-network = Мрежа
connections-name = Име
connections-colour = Цвят
connections-folder = Папка
connections-no-folders = Все още няма папки
connections-folder-hint = Незадължително. Връзките с една и съща папка се показват заедно.
connections-prominent-colour = Открояващ се цвят в редактора
connections-prominent-colour-hint = Огражда редактора на заявки с цвета на тази връзка.
connections-read-only = Само за четене
connections-read-only-hint = Забранява редактирането на клетки и блокира заявките за запис/DDL в тази връзка.
connections-read-only-mssql = SQL Server няма сесия само за четене, затова четенето се изпълнява в транзакция, която Schemaic връща. Ефектът на процедура извън базата данни оцелява след това; за гаранция се свържете с вход, който може само да чете.
connections-cli-access = Достъп от CLI
connections-cli-access-hint = Позволява на командния ред schemaic и на ИИ агентите чрез schemaic mcp да използват тази връзка.
connections-ai-data-access = Достъп на ИИ до данни
connections-statement-timeout = Време за изпълнение на израз
connections-statement-timeout-hint = Прекратява израз в тази връзка, който се изпълнява по-дълго.
connections-statement-timeout-same = Като в приложението
connections-environment = Среда
env-none = Без
env-local = Локална
env-development = Разработка
env-testing = Тестова
env-staging = Предпродукционна
env-production = Продукционна
connections-type = Тип
connections-init-sql = Начален SQL
connections-init-sql-pragma-hint = Незадължително. Настройки PRAGMA, изпълнявани при всяка връзка, която Schemaic отваря.
connections-init-sql-set-hint = Незадължително. Изрази SET, изпълнявани при всяка връзка, която Schemaic отваря — път за търсене, часова зона, роля.
connections-init-refused = „{ $statement }“: { $why }
connections-init-only-pragmas = при свързване могат да се изпълняват само настройки PRAGMA
connections-init-only-sets = при свързване могат да се изпълняват само изрази SET
connections-init-calls-function = при свързване настройка не може да извиква функция
connections-init-quoted-name = при свързване името на настройка не може да е в кавички
connections-init-names-no-setting = това не посочва настройка
connections-init-not-listed = { $name } не може да се задава при свързване — не е от сесийните настройки, за които Schemaic знае, че не засягат защитите му
connections-init-not-utf8 = клиентската кодова таблица трябва да остане UTF-8 — проверката за само четене на Schemaic чете всеки израз като UTF-8
connections-init-one-value = при свързване настройката приема една стойност и след нея не може да има нищо
connections-init-one-value-after-equals = при свързване настройката приема една стойност, след =
connections-init-transaction = настройки на транзакциите не могат да се задават при свързване — защитата за само четене ги задава
connections-init-pragma = този PRAGMA не може да се изпълнява при свързване — променя файла или защитата за само четене
connections-init-server-scope = настройките { $scope } променят сървъра, а не сесията
connections-init-variable = променливата не е сесийна настройка — тя изчезва с пакета си
connections-init-on-only = { $name } може само да се включва при свързване — Schemaic разчита на нея
connections-host = Хост
connections-port = Порт
connections-named-instance-port = Именувана инстанция: SQL Server Browser дава порта ѝ, така че „Порт“ не се използва. За да пропуснете Browser, задайте хоста без инстанцията и порта, на който тя слуша.
connections-database = База данни
connections-database-hint = Незадължително — базата данни, в която се отваря връзката. Някои хостинг доставчици позволяват само собствената си и отказват връзка, която не я посочва.
connections-authentication = Удостоверяване
ssh-auth-password = Парола
ssh-auth-key-pair = Двойка ключове
ssh-auth-agent = SSH агент
auth-password = Потребител и парола
auth-windows = Windows (този акаунт)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
connections-user = Потребител
connections-password = Парола
connections-azure-cli-hint = Вписва се като този, който е вписан в Azure CLI (първо изпълнете `az login`). Токенът се взима, когато връзката има нужда от него, и никога не се запазва.
connections-aws-iam-hint = Вписва се като потребителя по-долу с токен от идентификационните данни на AWS CLI (първо `aws configure` или `aws sso login`). Потребителят се нуждае от ролята rds_iam (PostgreSQL) или от AWSAuthenticationPlugin (MySQL). Токенът се взима, когато връзката има нужда от него, и никога не се запазва.
connections-gcp-iam-hint = Вписва се като потребителя по-долу — IAM потребителя или служебния акаунт, който Cloud SQL познава — с токен от Google Cloud CLI (първо `gcloud auth login`). Токенът се взима, когато връзката има нужда от него, и никога не се запазва.
connections-windows-hint-named = Вписва се като акаунта в Windows, под който работи Schemaic ({ $account }). Сървърът трябва да познава този акаунт като Windows вход.
connections-windows-hint = Вписва се като акаунта в Windows, под който работи Schemaic. Сървърът трябва да познава този акаунт като Windows вход.
connections-database-file = Файл на базата данни
connections-database-file-dialog = Изберете база данни на SQLite
connections-sqlite-hint = Базата данни на SQLite е един файл, така че няма сървър за достъп — няма хост, потребител, парола или тунел. Ръчните транзакции все още не са налични за нея: изразите се потвърждават при изпращането си.
connections-ssh-tunnel = SSH тунел
connections-ssh-tunnel-hint = Достъп до сървъра през SSH тунел.
connections-ssh-host = SSH хост
connections-ssh-port = SSH порт
connections-ssh-user = SSH потребител
connections-ssh-password = SSH парола
connections-ssh-agent-hint = Подписването се възлага на работещия ви SSH агент (OpenSSH агент / Pageant в Windows). Schemaic не съхранява ключ.
connections-jump-hosts = Междинни хостове
connections-jump-hosts-hint = Незадължително. Хостове, през които да се мине преди SSH хоста, първо най-близкият — всеки се вписва по начина, зададен по-долу.
connections-route-config = ~/.ssh/config: { $address }{ $key ->
        [some] , ключ { $keypath }
       *[none] {""}
    }{ $through ->
        [some] , през { $jump }
       *[none] {""}
    }
connections-route-jump-host = междинен хост { $host }
connections-route-proxy-command = ~/.ssh/config достига { $host } чрез ProxyCommand, който Schemaic не може да изпълни. Вместо това задайте маршрута като междинни хостове.
connections-route-match = ~/.ssh/config може да насочва { $host } през блок Match, който Schemaic не чете. Вместо това задайте маршрута като междинни хостове.
connections-route-match-destination = ~/.ssh/config може да насочва { $host } към друг хост или порт през блок Match, който Schemaic не чете. Вместо това задайте самия адрес като SSH хост.
connections-route-include = ~/.ssh/config включва файл, който Schemaic не можа да прочете, там където се отнася до { $host }. Задайте самия адрес като SSH хост, а маршрута — като междинни хостове.
connections-route-config-jump-password = ~/.ssh/config достига { $host } през { $jump }, а паролата се изпраща само до хостовете, които тази връзка посочва. Добавете междинния хост към „Междинни хостове“, ако приема същата парола, или се впишете с ключ или чрез SSH агента.
connections-route-jump-own-proxy-jump = ~/.ssh/config достига междинния хост { $host } през { $jump }, а Schemaic не следва собствения ProxyJump на междинен хост. Вместо това задайте маршрута като междинни хостове, първо { $jump }.
connections-jump-host-error = Междинен хост { $number }: { $why }
connections-jump-empty = празен — премахнете излишната запетая
connections-jump-space = „{ $entry }“ съдържа интервал
connections-jump-unclosed = „{ $entry }“ отваря [, която не затваря
connections-jump-after-bracket = „{ $entry }“ има текст след ]
connections-jump-no-host = „{ $entry }“ не посочва хост
connections-jump-not-a-port = „{ $entry }“ не е порт
connections-private-key = Частен ключ
connections-private-key-dialog = Изберете частен ключ
connections-passphrase = Парола на ключа
connections-tls = SSL / TLS
tls-disable = Изключено
tls-prefer = При възможност
tls-require = Задължително
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Свързване с некриптиран текст.
tls-prefer-description = Криптира, ако сървърът го предлага. Без проверка.
tls-require-description = Отказва свързване без криптиране. Без проверка.
tls-verify-ca-description = Криптира и проверява сертификата спрямо CA.
tls-verify-full-description = Криптира, проверява сертификата и проверява името на хоста.
tls-verify-ca-checks-host = В MySQL и MariaDB това проверява и името на хоста, така че се държи като Verify full — ограничение на драйвера, а не настройка.
tls-floor-azure-cli = Вписването с Microsoft Entra се свързва с Verify full, каквото и да е избрано тук: токенът му отваря всяка база данни в Azure SQL, до която акаунтът има достъп.
tls-floor-aws-iam = Вписването с AWS IAM се свързва поне със „Задължително“: токенът му не бива да минава през мрежата некриптиран. Verify full изисква пакета с CA на RDS като Root CA.
tls-floor-gcp-iam = Вписването с Google Cloud IAM се свързва поне с Verify CA — задайте за Root CA сървърния CA на инстанцията: токенът му отваря всяка инстанция на Cloud SQL, до която акаунтът има достъп.
tls-floor-other = Това вписване се свързва в по-строг режим от избрания тук.
connections-ca-certificate = CA сертификат
connections-ca-certificate-dialog = Изберете CA сертификат
connections-ca-certificate-hint = Оставете празно, за да се доверите на сертифициращите органи, на които се доверява операционната ви система, включително инсталираните от организацията ви. Посочете файл, за да се доверите на него вместо това.
connections-client-certificate = Клиентски сертификат
connections-client-certificate-dialog = Изберете клиентски сертификат
connections-client-key = Клиентски ключ
connections-client-key-dialog = Изберете клиентски ключ
connections-client-identity-hint = Клиентски сертификат и ключ са нужни само когато сървърът изисква да докажете кои сте (взаимен TLS). Ключът не бива да е защитен с парола.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

import-title = Импортиране в { $table }
import-section-source = Източник
import-section-reading = Четене
import-section-columns = Колони
import-section-preview = Преглед
import-file = Файл
import-choose-file = Избор на файл…
import-file-dialog = Импортиране в таблица
import-no-file = Не е избран файл
import-unnamed-file = (файл)
import-format = Формат
import-sheet = Лист
import-delimiter = Разделител
import-header-row = Първият ред е заглавие
import-header-row-hint = Използва първия ред за имена на колоните вместо като данни.
import-trim = Премахване на празните знаци
import-trim-hint = Премахва интервалите около всяка стойност — включително в кавичките.
import-empty-is-null = Празното поле е NULL
import-empty-is-null-hint = Празно поле означава NULL, а не празен низ.
import-null-tokens = Други стойности, означаващи NULL
import-null-tokens-hint = Разделени със запетаи, напр. NULL, \N, NA.
import-skip = Пропускане
import-nothing-mapped = Нищо не е съпоставено, така че няма какво да се импортира.
import-values-withheld = Първият запис на този файл е твърде голям за преглед, така че са прочетени само имената на колоните. Съпоставянето и импортирането четат целия файл.
import-missing-required = Не са съпоставени и не могат да бъдат празни: { $columns }. Импортирането ще е неуспешно, освен ако базата данни не ги попълни.
import-engine-not-transactional = Механизмът за съхранение на тази таблица ({ $engine }) не е транзакционен, така че неуспешно импортиране не може да бъде отменено — редовете, заредени преди грешката, ще останат.
import-problems = { $count ->
    [one] { $count } проблем във файла — нищо не е импортирано.
   *[other] { $count } проблема във файла — нищо не е импортирано.
}
import-problem-at-line = ред { $line }: { $message }
import-problem-at-column = ред { $line }, { $column }: { $message }
import-issue-not-an-integer = не е цяло число
import-issue-not-a-number = не е число
import-issue-not-a-boolean = не е стойност „истина/неистина“
import-issue-cell-error = листът не можа да изчисли тази клетка
import-issue-null-in-not-null = празно, но колоната не може да е NULL
import-issue-field-count = има { $found } полета, очаквани са { $expected }
import-tail-cut = Показани са { $shown } от { $total }.
import-tail-uncounted = Показани са всички { $total } — проверката спря да брои дотук, така че файлът съдържа повече.
import-tail-cut-and-uncounted = Показани са { $shown } от първите { $total } — проверката спря да брои дотук, така че файлът съдържа повече.
import-memory-json = Този JSON файл е { $size }, а импортът на JSON се държи в паметта, докато се зарежда — очаквайте да са нужни около { $estimate }. CSV със същите данни се зарежда с постоянна памет.
import-memory-xlsx = Тази работна книга е { $size }, а импортът на Excel се държи в паметта, докато се зарежда — очаквайте да са нужни приблизително { $estimate }, тъй като файлът е компресиран. CSV със същия лист се зарежда с постоянна памет.
import-read-failed = Файлът не можа да бъде прочетен: { $error }
import-no-columns-mapped = Няма колони от файла, съпоставени с колони на таблицата
import-preview-record-too-large = първият му JSON запис е по-голям от { $mib } MiB, които прегледът чете, така че няма какво да се покаже
import-refuse-not-objects = очакваха се JSON обекти (масив от тях или по един на ред)
import-refuse-oversize = Тази работна книга е { $size }, а файл на Excel трябва да бъде прочетен изцяло, преди каквото и да е от него да се покаже — { $max } е максимумът, който може да се отвори. Експортирайте листа като CSV и импортирайте него; CSV се зарежда с постоянна памет.
import-refuse-oversize-unsized = Тази работна книга е над { $max }, а файл на Excel трябва да бъде прочетен изцяло, преди каквото и да е от него да се покаже — { $max } е максимумът, който може да се отвори. Експортирайте листа като CSV и импортирайте него; CSV се зарежда с постоянна памет.
import-refuse-inflated = Тази работна книга е { $size } на диска, но се разархивира до поне { $unpacked }, а файл на Excel трябва да бъде прочетен изцяло, преди каквото и да е от него да се покаже — { $ceiling } разархивирано съдържание е максимумът, който ще се отвори, а за съхраняването му е нужна повече памет. Експортирайте листа като CSV и импортирайте него; CSV се зарежда с постоянна памет.
import-refuse-no-such-sheet = тази работна книга няма лист с име „{ $sheet }“ — съдържа { $sheets }
import-refuse-no-sheets = тази работна книга няма листове
import-refuse-too-wide = този лист заявява { $width } колони; работен лист на Excel побира { $max }. Експортирайте го като CSV и импортирайте него.
import-cancelled = Импортирането е прекратено — транзакцията беше върната, така че нищо не е записано.
import-done = { $count ->
    [one] Импортиран е { $count } ред в { $table }.
   *[other] Импортирани са { $count } реда в { $table }.
}
import-next = Напред
import-reading = Четене…
import-back = Назад
import-import = Импортиране
import-importing = Импортиране…
dialog-read-only = Тази връзка е само за четене.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

dump-title-schema = Експортиране на { $database }.{ $schema } в { $format }
dump-title = Експортиране на { $database } в { $format }
dump-sql-dialog = Експортиране в SQL
dump-folder-dialog = Експортиране в { $format }
dump-folder-hint = По един файл { $format } за всяка таблица, записани в папка по ваш избор.
dump-all-selected = Избрани са всички { $total }
dump-some-selected = Избрани са { $chosen } от { $total }
dump-select-all = Избиране на всички
dump-select-none = Нито една
dump-reading-tables = Четене на списъка с таблици…
dump-no-tables = Тази база данни няма таблици.
dump-tables-unreadable = Списъкът с таблици не можа да бъде прочетен.
dump-section-contents = Съдържание
dump-structure = Структура
dump-structure-hint = CREATE TABLE, тригерите ѝ и външните ключове, възстановени след данните.
dump-data = Данни
dump-data-hint = Всеки ред, като изрази INSERT.
dump-other-objects = Други обекти
dump-other-objects-hint = Типове, последователности, подпрограми и събития, на които тези таблици разчитат.
dump-section-replaying = Повторно зареждане
dump-drop-before-create = Премахване преди създаване
dump-drop-hint-up-front = Премахва всяка таблица и изглед преди съответния CREATE, така че файлът да се зарежда в база данни, която вече ги съдържа. С „Една транзакция“ заменя и всяка подпрограма в схемите им и спира, преди да премахне нещо, което не може да върне такова, каквото е. Без „Една транзакция“ оставя подпрограмите както са, така че повторно зареждане в база данни, която все още ги има, спира при първата. Собственикът и правата на всяка таблица се възстановяват накрая.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS преди всеки CREATE, така че файлът да се зарежда в база данни, която вече съдържа тези таблици. Заменя и всяка подпрограма в схемите им. Собственикът, предоставените права и защитата на редовете на всяка таблица се възстановяват накрая.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS преди всеки CREATE, така че файлът да се зарежда в база данни, която вече съдържа тези таблици. Заменя и всяка подпрограма и събитие в схемите им: в MySQL всяка първо се премахва, което отнема всички права върху нея, а файлът ги предоставя отново в края си, ако акаунтът ви може да чете mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS преди всеки CREATE, така че файлът да се зарежда в база данни, която вече съдържа тези таблици.
dump-one-transaction = Една транзакция
dump-one-transaction-hint = Обгръща зареждането, така че при грешка да не остане нищо.
dump-disable-fk-checks = Изключване на проверките на външните ключове
dump-fk-hint = Изключени по време на зареждането, така че редът, в който пристигат редовете, да не води до отказ.
dump-fk-hint-drops = Изключени по време на зареждането, така че редът, в който пристигат редовете, да не води до отказ. Файл, който премахва таблици, ги изключва във всеки случай: премахване при включени проверки изтрива редовете на таблици извън файла чрез техните действия ON DELETE.
dump-no-fk-guard-hint = Тази система няма сесиен превключвател за проверките на външните ключове; вместо това файлът добавя ограниченията след данните.
dump-export = Експортиране
dump-progress = { $count ->
    [one] Записване на { $table } — { $index } от { $total }, досега { $rows } ред
   *[other] Записване на { $table } — { $index } от { $total }, досега { $rows } реда
}
dump-reading-schema = Четене на схемата
dump-replace-files = Замяна на файловете
dump-replace-prompt = { $n ->
    [one] { $n } файл в { $folder } ще бъде заменен и не може да бъде възстановен: { $names }.
   *[other] { $n } файла в { $folder } ще бъдат заменени и не могат да бъдат възстановени: { $names }.
}
dump-replace-prompt-more = { $n } файла в { $folder } ще бъдат заменени и не могат да бъдат възстановени: { $names } и още { $more }.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] Записана е { $n } таблица.
   *[other] Записани са { $n } таблици.
}
dump-done-missing = { $n ->
    [one] { $n } отметната таблица не е намерена и не е във файла: { $names }.
   *[other] { $n } отметнати таблици не са намерени и не са във файла: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } обект, поискан от експорта, не е във файла: { $names }.
   *[other] { $n } обекта, поискани от експорта, не са във файла: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] Редовете на таблицата с ребра на граф { $names } не са във файла; тя се създава празна.
   *[other] Редовете на таблиците с ребра на граф { $names } не са във файла; те се създават празни.
}
dump-done-refused = { $n ->
    [one] Повторно зареждане в база данни, която вече съдържа това, спира, преди да премахне каквото и да е: файлът не може да го върне такова, каквото е: { $names }.
   *[other] Повторно зареждане в база данни, която вече съдържа някое от тези, спира, преди да премахне каквото и да е: файлът не може да ги върне такива, каквито са: { $names }.
}
dump-cancelled-partial = Експортът е прекратен — { $name } не е променен; записаното дотук е в { $part }
files-done = { $n ->
    [one] Записан е { $n } файл.
   *[other] Записани са { $n } файла.
}
files-done-with-note = { $head } { $note }.
files-cancelled-none = Експортът е прекратен — нито един файл не е завършен, така че нищо не е записано в { $folder }.
files-cancelled = { $n ->
    [one] Експортът е прекратен — { $n } файл беше завършен в { $folder }; таблицата в процес на запис остана незаписана.
   *[other] Експортът е прекратен — { $n } файла бяха завършени в { $folder }; таблицата в процес на запис остана незаписана.
}
files-failed-kept = { $n ->
    [one] { $message } — { $n } файл, вече записан в { $folder }, се запазва.
   *[other] { $message } — { $n } файла, вече записани в { $folder }, се запазват.
}
files-replaced = { $n ->
    [one] { $n } съществуващ файл беше заменен: { $names }.
   *[other] { $n } съществуващи файла бяха заменени: { $names }.
}
files-missing = { $n ->
    [one] { $n } отметната таблица не е намерена и не стана файл: { $names }.
   *[other] { $n } отметнати таблици не са намерени и не станаха файлове: { $names }.
}

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k); $count is the total
## itself, which decides the plural.

export-progress-of-about = { $count ->
    [one] { $rows } от ~{ $total } ред
   *[other] { $rows } от ~{ $total } реда
}
export-progress-of = { $count ->
    [one] { $rows } от { $total } ред
   *[other] { $rows } от { $total } реда
}
export-progress-rows = { $count ->
    [one] { $rows } ред
   *[other] { $rows } реда
}
export-progress-starting = Започване
export-progress-file = Файл: { $name }
export-progress-writing = Записване на { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Наблюдение на живо
monitor-badge-watching = Наблюдава се
monitor-badge-paused = На пауза
monitor-badge-stopped = Спряно
monitor-filter-all = Всички
monitor-filter-inserts = Вмъквания
monitor-filter-updates = Обновявания
monitor-filter-deletes = Изтривания
monitor-resume = Възобновяване на проверките
monitor-pause = Пауза на проверките
monitor-clear-tip = Изчистване на журнала
monitor-export-tip = Експортиране на журнала…
monitor-interval = { $secs } с
monitor-clear-title = Да се изчисти ли журналът?
monitor-clear-message = { $count ->
    [one] { $count } промяна ще бъде отхвърлена. Журналът е единственият запис на това, което е съдържал изтрит ред — проверката няма да съобщи промяната отново — и това не може да бъде отменено. Първо го експортирайте, ако искате да го запазите.
   *[other] { $count } промени ще бъдат отхвърлени. Журналът е единственият запис на това, което е съдържал изтрит ред — проверката няма да съобщи промяната отново — и това не може да бъде отменено. Първо го експортирайте, ако искате да го запазите.
}
monitor-col-time = Час
monitor-col-action = Действие
monitor-no-inserts = Все още няма вмъквания.
monitor-no-updates = Все още няма обновявания.
monitor-no-deletes = Все още няма изтривания.
monitor-empty-stopped = Спряно.
monitor-empty-paused = На пауза.
monitor-waiting = Изчакване
monitor-events-since = { $count ->
    [one] { $count } събитие от { $since }
   *[other] { $count } събития от { $since }
}
monitor-no-changes = Все още няма промени
monitor-partial = наблюдават се само първите { $cap } реда по първичен ключ
monitor-capped = най-старите отпадат при ограничението от { $cap } промени
monitor-length-only = { $count ->
    [one] { $columns } е двоична, така че промените ѝ се виждат само по дължина
   *[other] { $columns } са двоични, така че промените им се виждат само по дължина
}
monitor-newest-at-bottom = най-новите са най-долу
monitor-export-dialog = Експортиране на журнала с промени

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Сравняване на резултати
diff-filter-all = Всички
diff-filter-changed = Променени
diff-filter-added = Добавени
diff-filter-removed = Премахнати
diff-match-rows-by = Съпоставяне на редовете по
diff-whole-row = Целия ред
diff-export-tip = Експортиране на сравнението…
diff-nothing-to-export = Няма нищо за експортиране.
diff-export-dialog = Експортиране на сравнението
diff-unnamed-file = сравнението
diff-ran-at = Изпълнено в { $time }
diff-side-facts = { $count ->
    [one] { $count } ред · { $ms } мс
   *[other] { $count } реда · { $ms } мс
}
diff-only-in = Само в { $side }: { $columns }
diff-uncompared = Двоични, не са сравнени: { $columns }
diff-missing-key = и двата резултата трябва да имат колона с име { $column }, по която да се съпоставят редовете
diff-binary-key = { $column } съдържа двоични стойности, които не се сравняват, така че не може да съпоставя редове
diff-summary-identical = { $n ->
        [one] Няма разлики · { $n } ред без промяна
       *[other] Няма разлики · { $n } реда без промяна
    }
diff-summary-changed = { $n } променени
diff-summary-added = { $n } добавени
diff-summary-removed = { $n } премахнати
diff-summary-unchanged = { $n } без промяна
diff-summary-not-compared = { $columns } не са сравнени
diff-repeated-keys = { $count ->
    [one] { $count } ред повтаря ключ, вече зает от своята страна, и е изключен.
   *[other] { $count } реда повтарят ключ, вече зает от своята страна, и са изключени.
}
diff-capped = Резултатът беше ограничен: редовете след ограничението не се сравняват.
diff-col-change = Промяна
diff-no-differences = Няма разлики.
diff-no-changed = Няма променени редове.
diff-no-added = Няма добавени редове.
diff-no-removed = Няма премахнати редове.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

preview-risk-reversible = Преди да приложите
preview-risk-irreversible = Това не може да бъде отменено

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Създаване на таблица { $name } с { $columns } колона
   *[other] Създаване на таблица { $name } с { $columns } колони
}
ddl-line-rebuild-table = Преизграждане на { $name } — не може да се промени на място, затова редовете се копират в нова таблица, която заема мястото на старата
ddl-line-drop-table = Премахване на таблицата
ddl-line-drop-foreign-table = Премахване на външната таблица
ddl-line-truncate-table = Изтриване на всички редове
ddl-line-rename-table = Преименуване на таблицата на { $to }
ddl-line-add-column = Добавяне на колона { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Премахване на колона { $name }
ddl-line-rebuild-computed-column = Преизграждане на изчисляемата колона { $name } като ({ $expression }) около промяната на колоната
ddl-line-rename-column = Преименуване на колона { $from } на { $to }{ $retyped ->
        [yes] , и смяна на типа ѝ от { $old_type } на { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Преместване на колона { $name } в началото
ddl-line-move-column-after = Преместване на колона { $name } след { $after }
ddl-line-move-column = Преместване на колона { $name }
ddl-line-retype-column = Промяна на колона { $name } от { $from } на { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Промяна на колона { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
ddl-clause-none = (няма)
ddl-line-add-primary-key = Добавяне на първичен ключ ({ $columns })
ddl-line-drop-primary-key = Премахване на първичния ключ ({ $columns })
ddl-line-rebuild-primary-key = Преизграждане на първичния ключ ({ $columns }) около промяната на колоната
ddl-line-change-primary-key = Промяна на първичния ключ от ({ $from }) на ({ $to })
ddl-line-add-index = Добавяне на { $unique ->
        [yes] уникален{" "}
       *[no] {""}
    }{ $has_kind ->
        [yes] { $kind }{" "}
       *[no] {""}
    }индекс { $name }{ $has_method ->
        [yes] {" "}с { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}върху ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}с включени ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}подреден по ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}където { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = клъстериран columnstore
ddl-index-kind-primary-xml = първичен XML
ddl-index-kind-selective-xml = селективен XML
ddl-index-kind-secondary-xml = вторичен XML
ddl-index-kind-spatial = пространствен
ddl-line-drop-index = Премахване на индекса { $name }
ddl-line-rename-index = Преименуване на индекса { $from } на { $to }
ddl-line-keep-lossy-index = Индексът { $name } остава непроменен — Schemaic не може да го прочете целия
ddl-line-add-foreign-key = Добавяне на външен ключ { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Премахване на външния ключ { $name }
ddl-line-add-check = Добавяне на проверка { $name } ({ $expression })
ddl-line-drop-check = Премахване на проверката { $name }
ddl-line-table-options = Задаване за таблицата: { $parts }
ddl-table-option-engine = механизъм { $engine }
ddl-table-option-collation = колация { $collation }
ddl-table-option-comment = коментар
ddl-table-option-comment-cleared = коментар (изчистен)
ddl-line-create-view = Създаване на изглед { $name }
ddl-line-create-view-index = Създаване на изглед { $name } и индекса му { $index }
ddl-line-create-view-indexes = Създаване на изглед { $name } и индексите му { $indexes }
ddl-line-redefine-view = Предефиниране на изгледа { $name }
ddl-line-recreate-view = Премахване и пресъздаване на изгледа { $name }
ddl-line-rename-view = Преименуване на изгледа на { $to }
ddl-line-drop-view = Премахване на изгледа
ddl-line-drop-materialized-view = Премахване на материализирания изглед
ddl-line-refresh-view = Преизграждане на редовете на материализирания изглед — четящите са блокирани, докато приключи
ddl-line-refresh-view-concurrently = Преизграждане на редовете на материализирания изглед, без да се блокират четящите
ddl-line-create-trigger = Създаване на тригер { $name }
ddl-line-redefine-trigger = Предефиниране на тригера { $name }
ddl-line-recreate-trigger = Пресъздаване на тригера { $name }
ddl-line-recreate-trigger-as = Пресъздаване на тригера { $from } като { $name }
ddl-line-drop-trigger = Премахване на тригера { $name }
ddl-line-create-routine = Създаване на { $what } { $name }
ddl-line-redefine-routine = Предефиниране на { $what } { $name }
ddl-line-redefine-routine-as = Предефиниране на { $what } { $from } като { $name }
ddl-line-recreate-routine = Пресъздаване на { $what } { $name }
ddl-line-recreate-routine-as = Пресъздаване на { $what } { $from } като { $name }
ddl-line-rename-routine = Преименуване на { $what } { $from } на { $to }
ddl-line-drop-routine = Премахване на { $what } { $name }
ddl-line-create-event = Създаване на събитие { $name }
ddl-line-alter-event = Промяна на събитието { $name }
ddl-line-alter-event-edits = Промяна на събитието { $name }: { $edits }
ddl-line-drop-event = Премахване на събитието { $name }
ddl-event-edit-rename = преименуване на { $to }
ddl-event-edit-schedule = график { $schedule }
ddl-event-edit-status-enabled = състояние: включено
ddl-event-edit-status-disabled = състояние: изключено
ddl-event-edit-status-replica = състояние: изключено в репликата
ddl-event-edit-preserve = запазване след приключване
ddl-event-edit-no-preserve = изтриване след приключване
ddl-event-edit-definer = definer { $definer }
ddl-event-edit-comment = коментар
ddl-event-edit-body = тяло
ddl-line-create-enum = { $values ->
    [one] Създаване на тип { $name } с { $values } стойност
   *[other] Създаване на тип { $name } с { $values } стойности
}
ddl-line-add-enum-value = Добавяне на стойност { $value }
ddl-line-add-enum-value-after = Добавяне на стойност { $value } след { $after }
ddl-line-add-enum-value-before = Добавяне на стойност { $value } преди { $before }
ddl-line-rename-enum-value = Преименуване на стойност { $from } на { $to }
ddl-line-recreate-enum = Пресъздаване на тип { $name } ({ $values ->
    [one] { $values } стойност
   *[other] { $values } стойности
}, с преобразуване на { $columns ->
    [one] { $columns } колона
   *[other] { $columns } колони
})
ddl-line-create-domain = Създаване на домейн { $name } като { $datatype }
ddl-line-set-domain-default = Задаване на стойността по подразбиране на домейна на { $default }
ddl-line-drop-domain-default = Премахване на стойността по подразбиране на домейна
ddl-line-domain-not-null = Домейнът става NOT NULL
ddl-line-domain-nullable = Домейнът започва да допуска NULL
ddl-line-recreate-domain = Пресъздаване на домейн { $name } като { $datatype } (беше { $was }; с преобразуване на { $columns ->
    [one] { $columns } колона
   *[other] { $columns } колони
})
ddl-line-create-sequence = Създаване на последователност { $name }
ddl-line-alter-sequence = Задаване за последователността: { $edits }
ddl-sequence-edit-type = тип { $datatype }
ddl-sequence-edit-increment = стъпка { $value }
ddl-sequence-edit-minimum = минимум { $value }
ddl-sequence-edit-maximum = максимум { $value }
ddl-sequence-edit-start = начало { $value }
ddl-sequence-edit-cache = кеш { $value }
ddl-sequence-edit-cycling-on = цикличност включена
ddl-sequence-edit-cycling-off = цикличност изключена
ddl-sequence-edit-owner = собственик { $owner }
ddl-sequence-edit-no-owner = без собственик
ddl-line-restart-sequence = Рестартиране на последователността от { $value }
ddl-line-create-object = Създаване на { $what } { $name }
ddl-line-replace-object = Премахване на { $what } { $name } и повторното му създаване
ddl-line-replace-object-as = Премахване на { $what } { $name } и повторното му създаване като { $to }
ddl-line-alter-tsql-sequence = Промяна на последователността: { $clauses }
ddl-line-rename-object = Преименуване на { $what } на { $to }
ddl-line-drop-object = Премахване на { $what }
ddl-line-set-object-comment = Задаване на коментар за { $what }
ddl-line-clear-object-comment = Изчистване на коментара за { $what }
ddl-line-create-database = Създаване на база данни { $name }
ddl-line-drop-database = Премахване на базата данни { $name }
ddl-line-create-schema = Създаване на схема { $name }
ddl-line-drop-schema = Премахване на схемата { $name }
ddl-line-create-extension = Създаване на разширение { $name }
ddl-line-create-user = Създаване на потребител { $who }
ddl-line-create-role = Създаване на роля { $who }
ddl-line-create-login = Създаване на вход { $who }
ddl-line-drop-user = Премахване на потребителя { $who }
ddl-line-drop-role = Премахване на ролята { $who }
ddl-line-drop-login = Премахване на входа { $who }
ddl-line-set-password = Задаване на парола за { $who }
ddl-line-grant-privileges = Предоставяне на { $privileges } на { $who }
ddl-line-revoke-privileges = Отнемане на { $privileges } от { $who }
ddl-privileges-none = никакви привилегии
ddl-privileges-count = { $count } привилегии
ddl-line-grant-role = Предоставяне на { $role } на { $member }
ddl-line-revoke-role = Отнемане на { $role } от { $member }
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Премахва { $name } и я пресъздава, като копира всички редове, и възстановява ключовете, индексите, проверките, стойностите по подразбиране и тригерите ѝ, собствените ѝ коментари и тези на колоните ѝ, както и външните ключове на други таблици към нея. Спира, преди да започне, ако таблицата има нещо друго, което Schemaic не възпроизвежда — права, собствен собственик, зависим обект, обвързан със схемата, системно версиониране, ledger, репликация, компресия, опции на индекси, изключен индекс, свързано правило или стойност по подразбиране, класификация на чувствителността, описание на ключ, индекс, стойност по подразбиране, проверка или тригер. Ръчно създадената статистика не се пренася.
       *[other] Премахва { $name } и я пресъздава, като копира всички редове. Всичко за таблицата, което Schemaic не е прочел, не се пренася.
    }
ddl-risk-drop-table = Премахва таблицата и всички редове в нея.
ddl-risk-drop-foreign-table = Премахва външната таблица: дефиницията ѝ изчезва, а редовете, които чете, остават където са, на външния сървър или във файла; изглед или подпрограма, които я посочват, отсега нататък ще са неуспешни.
ddl-risk-truncate-table = Изтрива всички редове в таблицата. Това не може да бъде отменено.
ddl-risk-drop-column = Премахва колоната { $name } и всички данни в нея.
ddl-risk-rebuild-computed-column = Премахва изчисляемата колона { $name } и я добавя отново, така че тя се премества в края на таблицата.
ddl-risk-column-not-null = Колоната { $name } става NOT NULL — изразът е неуспешен, ако някой ред съдържа NULL.
ddl-risk-column-stops-generated = Колоната { $name } престава да е генерирана. Текущите ѝ стойности се запазват, но отсега нататък нищо не ги преизчислява.
ddl-risk-column-becomes-generated = Колоната { $name } става генерирана от { $expression }. Всяка стойност в нея се заменя с изчисленото от израза.
ddl-risk-column-expression-changes = Изразът на колоната { $name } се променя от { $from } на { $to }. В PostgreSQL това премахва и добавя отново колоната, така че всяка стойност се преизчислява и колоната се премества в края на таблицата.
ddl-risk-type-narrows = Стесняването на { $subject } от { $from } до { $to } { $loss ->
        [truncates] отрязва стойностите, които вече не се побират
        [rounds] закръгля всяка стойност в колоната
       *[both] отрязва стойностите, които вече не се побират, и закръгля всяка стойност в колоната
    }.
ddl-risk-type-rewrites = Промяната на { $subject } от { $from } на { $to } презаписва всяка стойност; може да е неуспешна или да загуби точност.
ddl-risk-domain-subject = домейна { $name }
ddl-risk-primary-key-dropped = Оставя таблицата без първичен ключ — редовете вече не могат да се редактират от таблицата с резултати.
ddl-risk-primary-key-rebuilt = Премахва и добавя отново първичния ключ около промяната на колоната. Когато той е клъстерираният индекс, това презаписва цялата таблица, а външен ключ в друга таблица, който сочи към него, спира плана.
ddl-risk-primary-key-replaced = Заменя първичния ключ: { $count ->
        [one] { $columns } вече не е уникална
       *[other] { $columns } вече не са уникални
    } и отсега нататък се приемат дубликати. В MySQL това преизгражда клъстерирания индекс, което презаписва цялата таблица.
ddl-risk-drop-foreign-key = Премахва външния ключ { $name }. Отсега нататък се приемат редове без съответстващ родител, а съществуващите данни не се проверяват отново, ако ключът бъде добавен обратно.
ddl-risk-drop-unique-index = Премахва уникалния индекс { $name }. Отсега нататък се приемат дублиращи се стойности и таблицата с резултати може да загуби ключа, по който редактира редовете.
ddl-risk-drop-view = { $materialized ->
        [yes] Премахва материализирания изглед. { $cost }
       *[no] Премахва изгледа. { $cost }
    }
ddl-risk-view-cost-sqlite = Изгледите, които избират от него, престават да се разрешават, докато не бъде върнат, а тригерите му INSTEAD OF се премахват заедно с него и не се възстановяват.
ddl-risk-view-cost-sqlite-restored = Изгледите, които избират от него, престават да се разрешават, докато не бъде върнат, а тригерите му INSTEAD OF се премахват заедно с него.
ddl-risk-view-cost-sqlserver = Предоставените права върху него, описанието му и тригерите му INSTEAD OF се премахват заедно с него и не се възстановяват.
ddl-risk-view-cost-sqlserver-restored = Предоставените права върху него и описанието му се премахват заедно с него и не се възстановяват; тригерите му INSTEAD OF също се премахват.
ddl-risk-view-cost-other = Зависимите изгледи, правилата, предоставените права и тригерите му INSTEAD OF се премахват заедно с него и не се възстановяват.
ddl-risk-view-cost-other-restored = Зависимите изгледи, правилата и предоставените права се премахват заедно с него и не се възстановяват; тригерите му INSTEAD OF също се премахват.
ddl-risk-recreate-view = Пресъздаването на { $name } първо го премахва. { $cost } { $why ->
        [sqlite] SQLite не може да замени изглед на място, така че всяка редакция на изглед — включително преименуване — минава по този път.
        [sqlserver] sp_rename на SQL Server би оставил съхранената дефиниция на изгледа със старото име, така че преименуването минава по този път.
       *[postgres] PostgreSQL не може да замени изглед, чиито колони са сменили име, тип или ред, така че това е единственият начин да се приложи редакцията.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } израз, премахнат заедно с него, се възстановява след това, в същата транзакция.
       *[other] { $count } израза, премахнати заедно с него, се възстановяват след това, в същата транзакция.
    }
ddl-risk-view-indexes-dropped = Предефинирането на изгледа { $view } премахва { $dropped_count ->
        [one] индекса му
       *[other] индексите му
    } ({ $dropped }) — ALTER VIEW на SQL Server винаги го прави — { $case ->
        [all] и този план { $again_count ->
            [one] го изгражда
           *[other] ги изгражда
        } отново след това, в същата транзакция. При голям изглед това отнема толкова време, колкото е отнело { $again_count ->
            [one] изграждането му
           *[other] изграждането им
        }. Индекс с fill factor, компресия, padding, опции за заключване, файлова група или описание спира плана, преди нещо да се промени — Schemaic не може да изгради такива отново. Ръчно създадената статистика за изгледа не се пренася.
        [none] и този план не { $gone_count ->
            [one] го изгражда
           *[other] ги изгражда
        } отново: изгледът, на който трябва да съответства, няма такъв индекс. Ръчно създадената статистика за изгледа не се пренася.
       *[some] и този план изгражда отново { $again } след това, в същата транзакция, но не и { $gone }, които изгледът, на който трябва да съответства, няма. Индекс с fill factor, компресия, padding, опции за заключване, файлова група или описание спира плана, преди нещо да се промени — Schemaic не може да изгради такива отново. Ръчно създадената статистика за изгледа не се пренася.
    }
ddl-risk-add-not-null-column = Колоната { $name } е NOT NULL без стойност по подразбиране. Ако таблицата вече има редове, няма стойност, която да им се даде, и системата ще откаже целия план.
ddl-risk-drop-check = Премахва проверката { $name }. Отсега нататък се приемат редовете, които ограничението е отказвало, а съществуващите данни не се проверяват отново, ако то бъде добавено обратно.
ddl-risk-keep-lossy-index = Вашата редакция на индекса { $name } не е включена. Той използва нещо, което Schemaic не може да прочете обратно — колона на ключа с израз, клас на оператори или подредба на NULLS — и прилагането на редакцията би означавало пресъздаване на индекса без него. Редактирайте този индекс чрез SQL.
ddl-risk-drop-trigger = Премахва тригера { $name }. Каквото е правил при всеки запис, престава да се случва, и за редовете, записани отсега нататък, няма да бъде приложено.
ddl-risk-redefine-trigger-in-place = Предефинира тригера { $name } на място — той не се премахва, така че правата му остават — и отсега нататък действието му при всеки запис се променя.
ddl-risk-recreate-trigger = Пресъздаването на тригера { $name } първо го премахва. Където DDL не е транзакционен (MySQL), нова дефиниция, която сървърът отхвърли, оставя таблицата без тригер.
ddl-risk-signature-lost = { $kind ->
        [trigger] Тригерът { $name } е подписан (ADD SIGNATURE) и всяка промяна по него
        [function] Функцията { $name } е подписана (ADD SIGNATURE) и всяка промяна по нея
       *[procedure] Процедурата { $name } е подписана (ADD SIGNATURE) и всяка промяна по нея
    } премахва подписа — Schemaic не може да го подпише отново без ключа на сертификата. Каквото е позволявал сертификатът, спира да работи, докато не изпълните ADD SIGNATURE отново.
ddl-risk-redefines-routine = Предефинира { $name }. Всичко, което я извиква, отсега нататък изпълнява новото тяло, включително тригери на други таблици, които тази редакция изобщо не споменава.
ddl-risk-routine-parameters-change = Променя списъка с параметри на { $name }. Извикванията със старите аргументи вече не се свързват с нея.
ddl-risk-recreate-routine = Пресъздаването на { $held } първо я премахва. Където DDL не е транзакционен (MySQL), нова дефиниция, която сървърът отхвърли, не оставя никаква { $what }.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Привилегиите, предоставени за { $held }, и коментарът ѝ не оцеляват при премахването, а този план не ги възстановява — приложете отново всеки GRANT и COMMENT след това.
       *[no] Привилегиите, предоставени за { $held }, не оцеляват при премахването, а този план не ги възстановява — приложете отново всеки GRANT след това.
    }
ddl-risk-drop-numbered-group = Премахва { $what } { $name } и заедно с нея всеки член на номерираната ѝ група: { $members }.
ddl-risk-drop-routine = Премахва { $what } { $name }. PostgreSQL отказва, докато тригер все още я използва, така че такива тригери трябва да бъдат премахнати първо.
ddl-risk-drop-event = Премахва събитието { $name }. Графикът и тялото му изчезват заедно с него — нищо друго в базата данни не пази копие.
ddl-risk-one-shot-event = Това събитие се изпълнява веднъж и след това сървърът го изтрива — това е ON COMPLETION NOT PRESERVE. Включете „Запазване след приключване“, за да го запазите.
ddl-risk-one-shot-event-altered = Събитието { $name } ще се изпълни веднъж и след това сървърът ще го изтрие — това е ON COMPLETION NOT PRESERVE. Включете „Запазване след приключване“, за да го запазите.
ddl-risk-add-enum-value = Добавянето на { $value } не може да бъде отменено на място — PostgreSQL не може да премахне стойност от изброим тип, така че връщането назад означава пресъздаване на типа. Стойността също не може да се използва, докато този план не бъде приложен.
ddl-risk-rename-enum-value = Всеки ред със стойност { $from } отсега нататък ще чете { $to }. Ако сте искали да премахнете { $from }, а не да я преименувате, изтрийте я и приложете това отделно.
ddl-risk-recreate = Пресъздаването на { $kind ->
        [domain] домейна
       *[type] типа
    } { $name } го премахва и изгражда наново. Всяка колона по-долу се преобразува през текст; стойност, която новата дефиниция не приема, проваля целия план и нищо не се прилага{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}и още { $more }
        }. Изглед или функция, изградени върху него, не могат да бъдат изброени тук и биха накарали сървъра да откаже.
       *[no] . Днес нищо не го използва, но изглед или функция, изградени върху него, не могат да бъдат изброени тук и биха накарали сървъра да откаже.
    }
ddl-risk-drop-domain-check = Премахва проверката { $name }. Колоните от този домейн престават да се проверяват, а съществуващите данни не се проверяват отново, ако тя бъде добавена обратно.
ddl-risk-domain-nullable = Всички колони от този домейн започват да приемат NULL, включително тези в таблици, които тази редакция изобщо не споменава.
ddl-risk-domain-not-null = Изразът е неуспешен, ако някоя колона от този домейн вече съдържа NULL.
ddl-risk-drop-sequence = Премахва последователността и позицията, до която е стигнала. Сървърът отказва, докато стойност по подразбиране на колона все още тегли от нея, така че тези стойности по подразбиране трябва да бъдат премахнати първо.
ddl-risk-drop-synonym = Премахва синонима. Нищо не възпрепятства това: изглед, подпрограма или заявка, които го посочват, ще са неуспешни при следващото си изпълнение.
ddl-risk-drop-extension = Премахва разширението и функциите, типовете и операторите, които е инсталирало. Сървърът отказва, докато нещо ваше все още зависи от тях — колона от неговия тип, индекс или сървър върху неговата обвивка — и посочва какво.
ddl-risk-drop-used-object = Премахва { $what }. Сървърът отказва, докато колона все още го използва, така че тези колони трябва първо да сменят типа си.
ddl-risk-restart-sequence = Рестартира брояча от { $value }. Вече раздадените стойности не се променят, така че ключ, до който стигне отново, ще влезе в конфликт.
ddl-risk-replace-synonym = Премахва синонима и го създава отново. Нищо не възпрепятства премахването: двата израза се изпълняват в една транзакция.
ddl-risk-replace-sequence = Премахва последователността и я създава отново: позицията, до която е стигнала, се губи и тя започва отначало от { $start }. Сървърът отказва премахването, докато стойност по подразбиране на колона все още тегли от нея.
ddl-risk-start-value = началната ѝ стойност
ddl-risk-replace-object = Премахва { $what } и го създава отново. Сървърът отказва премахването, докато колона все още го използва, така че тези колони трябва първо да сменят типа си.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Отказва се, докато има права върху него, собствен собственик, разширени свойства или свързано правило или стойност по подразбиране: пресъздаването не би ги възстановило.
       *[no] Отказва се, докато има права върху него, собствен собственик или разширени свойства: пресъздаването не би ги възстановило.
    }
ddl-risk-drop-database = Премахва базата данни { $name } и всичко в нея — всяка таблица с всичките ѝ редове, всеки изглед, индекс, подпрограма и тригер. Нищо не се запазва и няма отмяна: единственият път назад е резервно копие, направено преди изпълнението.
ddl-risk-drop-schema = Премахва схемата { $name }. Сървърът отказва, докато тя все още съдържа таблици, изгледи или подпрограми, така че или се премахва празна схема, или операцията е неуспешна{ $cascade ->
        [yes] {" "}— Schemaic никога не изпраща CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Премахва входа { $who } и заедно с него всички сървърни права и членства в сървърни роли — те не са записани никъде другаде, така че вход, създаден отново с това име, започва без тях. Сървърът отказва, докато нещо е свързано като него, така че сесиите му трябва първо да приключат. Потребителите му във всяка база данни не се премахват: те остават, с правата си, без съпоставен вход, а нов вход с това име не се свързва отново с тях без ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Премахва { $who } и всички негови привилегии. Предоставените права не са записани никъде другаде, така че връщането на акаунта означава всички да се предоставят отново по памет. { $cuts_off ->
        [yes] Всичко, свързано като него, ще е неуспешно при следващия си израз в тази база данни.
       *[no] Всичко, което все още е свързано като него, продължава да работи, докато не прекъсне връзката.
    }
ddl-risk-set-password = Променя паролата на { $who }. Старата не може да бъде възстановена, така че всичко, което все още е настроено с нея — приложение, задача, клиентът на друг разработчик — няма да може да се свърже, докато не бъде обновено ръчно. Вече отворените сесии продължават да работят.
ddl-risk-revoke-privileges = Отнема { $privileges } от { $who }. Всичко, което разчита на това — приложение, задача, изглед, притежаван от някой друг — започва да е неуспешно при следващия си израз.{ $cascades ->
        [yes] {" "}Отнемането е с CASCADE, така че се отнема и от всеки акаунт, на който { $who } го е предоставил, и изпълняваното като тези акаунти се проваля по същия начин.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Където на { $who } вместо това е забранено (DENY), отнемането премахва забраната — което връща достъпа, ако роля, в която членува, го предоставя.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}За таблица то изтрива и всяко предоставяне и DENY на ниво колона за { $cleared }, които { $who } има върху колоните на таблицата: колона, забранена за { $who }, вече не е забранена, а колона, предоставена отделно, вече не е предоставена.
       *[no] {""}
    }
ddl-risk-revoke-role = Отнема ролята { $role } от { $member } и заедно с нея всяка привилегия, която ролята носи.
ddl-risk-blank-password = { $who } няма да има парола: всеки, който може да достигне сървъра, може да влезе като него. Задайте парола тук или с ALTER USER след това.
ddl-risk-host-is-everywhere = Хостът му е % — всеки компютър в мрежата, а не само този.
ddl-risk-grant-server-wide = Предоставя { $privileges } на { $who } за целия сървър — самата инстанция, а не разглежданата база данни.
ddl-risk-grant-every-database = Предоставя { $privileges } на { $who } за всяка база данни на този сървър — включително бази данни, които още не съществуват, а не само разглежданата.
ddl-risk-grant-keeps-column-denies = Предоставяне на { $privileges } на ниво таблица изтрива всеки DENY за него, който { $who } има върху колона на таблицата, затова изразът веднага след предоставянето отново забранява тези колони за { $who }: колона, забранена за { $who }, остава забранена. Ако { $who } има и правото на предоставяне за таблицата, сървърът отказва да я забрани отново без CASCADE и нищо не се прилага.
ddl-risk-star-refresh = Опреснява всеки изглед и вградена функция в тази база данни, които правят select * от { $table }, пряко или чрез синоним: SQL Server ги свързва с колоните на таблицата по позиция и иначе биха чели и записвали грешни колони. Планът е неуспешен, ако някой от тях вече не се компилира с новите колони. Такъв обект в друга база данни, който прави select * от { $table }, се опреснява след потвърждаването на плана, извън транзакцията му: дотогава той чете старите позиции, а ако опресняването му е неуспешно, планът остава приложен и изгледът се посочва тук.
ddl-risk-piecewise = SQL Server не създава, променя и премахва модул, компилиран в машинен код, в транзакция, така че този план изпълнява по един израз наведнъж и не може да бъде спрян: ако израз е неуспешен, предишните остават приложени.
ddl-risk-drop-sequence-table = Премахва последователността: стойността, до която е стигнала, се губи, а всичко, което все още тегли от нея, ще е неуспешно.
ddl-risk-drop-scale = Според статистиката в дървото на схемата това са { $rows } реда.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = предложението е за таблицата `{ $proposed }`, но е отворена `{ $actual }`
propose-no-ops = предложението не изисква промени
propose-no-such-column = `{ $table }` няма колона с име `{ $name }`
propose-column-exists = `{ $table }` вече има колона с име `{ $name }`
propose-no-such-index = `{ $table }` няма индекс с име `{ $name }`
propose-index-exists = `{ $table }` вече има индекс с име `{ $name }`
propose-no-such-foreign-key = `{ $table }` няма външен ключ с име `{ $name }`
propose-no-such-check = `{ $table }` няма ограничение CHECK с име `{ $name }`
propose-key-width-mismatch = външният ключ трябва да има толкова колони, към които сочи, колкото и сочещи колони
propose-empty-index = индексът трябва да има поне една колона
propose-invalid = предложената таблица не би била валидна: { $problem }
propose-not-a-table = `{ $name }` е изглед — предложението може да променя само таблица
propose-no-such-table = няма таблица с име `{ $name }` — извикайте list_schema, за да видите какво съществува
propose-not-an-expression = { $field ->
        [default] стойността по подразбиране `{ $text }` не е единичен SQL израз
        [generated] генериращият израз `{ $text }` не е единичен SQL израз
       *[check] изразът на проверката `{ $text }` не е единичен SQL израз
    }
propose-not-a-type = `{ $text }` не е тип на колона — посочете само типа, без стойност по подразбиране, без NOT NULL и без нищо след него
propose-unknown-action = `{ $text }` не е действие на външен ключ
propose-not-loaded = { $table } в момента не е заредена в { $database } — отворете базата данни в дървото на схемата или изчакайте опресняването да приключи и опитайте отново.
propose-nothing-to-change = { $subject } вече изглежда така — няма какво да се промени.
propose-no-database = В този раздел няма избрана база данни, така че промяна в схемата, приложена оттук, няма къде да отиде. Първо изберете база данни за тази връзка — ако всички бази данни са скрити, окото в СХЕМА е мястото, откъдето да върнете някоя.
propose-other-connection = Тази промяна се отнася до раздел на друга връзка — превключете към него или изберете връзката му в дървото, преди да приложите промяна в схемата.
ai-run-no-database = В този раздел няма избрана база данни, така че израз, изпълнен оттук, няма къде да отиде. Първо изберете база данни за тази връзка — ако всички бази данни са скрити, окото в СХЕМА е мястото, откъдето да върнете някоя.
ai-run-other-connection = Този разговор се отнася до раздел на друга връзка. Нов раздел би се отворил за връзката, избрана в дървото, така че изразът би се изпълнил другаде — първо превключете към онзи раздел или изберете връзката му в дървото.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

ddl-moved-renamed = { $from }, която този план преименува на { $to }
ddl-moved-dropped = { $name }, която този план премахва
ddl-withheld-unreplayable-rename = { $count ->
        [one] Преименуването на { $was } на { $to } не може да пренесе { $count } тригер: SQL кодът му посочва { $was }, а пренаписването му тук би променило написаното от вас. Преименувайте изгледа отделно, след като пресъздадете тригерите с новото име.
       *[other] Преименуването на { $was } на { $to } не може да пренесе { $count } тригера: SQL кодът им посочва { $was }, а пренаписването му тук би променило написаното от вас. Преименувайте изгледа отделно, след като пресъздадете тригерите с новото име.
    }
ddl-withheld-numbered-group = { $name } е начало на група номерирани процедури ({ $members }), а този план първо я премахва, което премахва и всеки член на групата. Редактирайте я под собственото ѝ име и вид, което я променя на място и ги запазва.
ddl-withheld-view-index-create = { $count ->
        [one] Създаването на изгледа { $view } не може да създаде индекса му { $indexes } — вид, който Schemaic не може да възпроизведе. Създайте този изглед чрез SQL.
       *[other] Създаването на изгледа { $view } не може да създаде индексите му { $indexes } — вид, който Schemaic не може да възпроизведе. Създайте този изглед чрез SQL.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Редактирането на изгледа { $view } премахва индекса му { $indexes }, който Schemaic не може да създаде отново — вид, който Schemaic не може да възпроизведе. Редактирайте този изглед чрез SQL.
       *[other] Редактирането на изгледа { $view } премахва индексите му { $indexes }, които Schemaic не може да създаде отново — вид, който Schemaic не може да възпроизведе. Редактирайте този изглед чрез SQL.
    }
ddl-withheld-tsql-lossy-index = Преизграждането на { $table } би премахнало индекса { $index }: той е от вид, който Schemaic не чете изцяло
ddl-withheld-tsql-encrypted-trigger = Преизграждането на { $table } би премахнало тригера { $trigger }: текстът му е криптиран
ddl-withheld-tsql-signed-trigger = Преизграждането на { $table } би премахнало подписа на тригера { $trigger }, който не може да бъде възстановен без частния ключ на сертификата
ddl-withheld-tsql-renamed-trigger = Преизграждането на { $table } под ново име би оставило тригера { $trigger } да посочва старото
ddl-withheld-partly-read = Индексът { $index } беше прочетен само частично — SQLite пази предиката на частичен индекс и ключ с израз само в собствения CREATE текст на индекса{ $why ->
        [may-name] , а този текст може да посочва { $moved }. Премахнете индекса, за да продължите без него.
        [not-found] , така че не може да бъде създаден от прочетеното. Премахнете индекса, за да продължите без него.
        [edited] , така че тази редакция по него не може да бъде записана — прочетеното не е целият индекс. Отменете промяната, за да запазите индекса такъв, какъвто е, или го премахнете.
       *[no-text] {" "}— а за този няма такъв текст, така че пресъздаването му би променило какво индексира. Премахнете индекса, за да продължите без него.
    }
ddl-withheld-condition-names-dropped = Условието на индекса { $index } посочва { $column }, която този план премахва. Първо редактирайте условието или премахнете индекса.
ddl-withheld-strands-triggers = Тази таблица има { $count ->
        [one] { $count } тригер
       *[other] { $count } тригера
    }, а техният SQL се възпроизвежда точно както SQLite го е съхранил — което все още би посочвало { $moved }. SQLite приема такъв тригер и след това отказва всеки запис в таблицата. Първо премахнете или редактирайте тригера или преименувайте колоната без друга промяна, за да може собственият ALTER TABLE на SQLite да го пренапише.
ddl-withheld-strands-other-trigger = { $owner } посочва { $moved }. Преизграждането преименува тази таблица под този тригер и оставя SQL кода му точно какъвто е, така че тригерът би продължил да посочва колона, която я няма — SQLite приема такъв тригер и след това отказва всеки запис в *онази* таблица. Първо премахнете или редактирайте тригера или преименувайте колоната без друга промяна, за да може собственият ALTER TABLE на SQLite да го пренапише.
ddl-withheld-trigger-on = Тригерът { $name } върху { $table }
ddl-withheld-some-trigger = Тригер върху друга таблица
ddl-withheld-strands-generated-column = Генерираната колона { $column } се изчислява от { $moved }. Преизграждането записва таблицата от модела и нищо не пренаписва този израз — SQLite отказва новата таблица с „no such column“ и целият план се връща. Първо редактирайте или премахнете { $column }.
ddl-withheld-cannot-restate = Декларацията на тази таблица съдържа { $clauses }, за което моделът на Schemaic няма поле. Преизграждането записва таблицата обратно от този модел, така че клаузата би изчезнала от таблица, която все още изглежда наред — и всеки запис, зависещ от нея, отсега нататък би се държал различно. Вместо това променете таблицата със скрипт („Копиране на DDL“ е началото на такъв) или първо премахнете клаузата.
ddl-sqlite-clauses = { $first } и { $last }
ddl-sqlite-clause-deferrable = клауза DEFERRABLE на външен ключ
ddl-sqlite-clause-on-conflict = клауза ON CONFLICT
ddl-sqlite-clause-desc-key = колона на първичния ключ с DESC
ddl-sqlite-clause-collate-key = COLLATE за колона на първичния ключ
ddl-withheld-virtual-table = `{ $name }` е виртуална таблица. Колоните ѝ са интерфейсът на модул, а не декларация за съхранение, а истинските ѝ данни се намират в сенчести таблици, които преизграждането би премахнало — FTS5 индекс би изчезнал от таблица, която все още съдържа текста. Вместо това я променете със скрипт (DROP и CREATE VIRTUAL TABLE) или редактирайте таблиците, които тя индексира.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

ddl-used-by-count-view = { $count ->
    [one] { $count } изглед
   *[other] { $count } изгледа
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } подпрограма
   *[other] { $count } подпрограми
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } тригер
   *[other] { $count } тригера
}
ddl-used-by-count-event = { $count ->
    [one] { $count } събитие
   *[other] { $count } събития
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } външен ключ
   *[other] { $count } външни ключа
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } табличен израз
   *[other] { $count } таблични израза
}
ddl-used-by-counts = { $first } и { $last }
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}и още { $more }
    }
ddl-used-by-drop = Използва се от { $listing }.
ddl-used-by-change = { $subject } се използва от { $listing }. { $then ->
        [renamed] След преименуването те все още посочват { $name }.
        [dropped] Те ще се повредят при премахването.
       *[retyped] Проверете ги спрямо новия тип.
    }
ddl-used-by-column = Колоната { $name }

ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = Външните ключове между тези таблици образуват цикъл, така че никой ред на създаване не удовлетворява всички. Един израз ще бъде отказан, защото сочи към таблица, която още не съществува, а нито MySQL, нито MariaDB връщат DDL.
ddl-risk-clash = { $holder } все още държи { $names }, което е нужно за създаването на { $claimer } — { $why ->
        [not-dropped] а този план не го премахва
        [default] и не може да бъде премахнат първо, защото стойност по подразбиране на колона тегли от него
       *[referenced] и не може да бъде премахнат първо, защото друга таблица сочи към него
    }. { $cost ->
        [no-rollback] Един израз ще бъде отказан, а нито MySQL, нито MariaDB връщат DDL.
        [rolls-back] Един израз ще бъде отказан, което връща цялата миграция.
       *[refused] Един израз ще бъде отказан.
    }

preview-title = Прилагане на промени в { $connection } · { $subject }
preview-changes = { $count ->
    [one] { $count } промяна
   *[other] { $count } промени
}
preview-used-by = Използва се от
preview-withheld = Тази система не може да изрази част от този план
preview-withheld-hint = Нищо не се прилага, докато това е в списъка.
preview-omitted = Не е включено в тази миграция
preview-omitted-hint = Прилагането оставя тези обекти такива, каквито са.
preview-copy = Копиране
preview-open-in-editor = Отваряне в редактора
preview-back = Назад
preview-apply = Прилагане
preview-applying = Прилагане…
preview-applied = { $count ->
    [one] Приложен е { $count } израз към { $subject }.
   *[other] Приложени са { $count } израза към { $subject }.
}
preview-refreshed = Схемата е опреснена.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Импортиране на връзки
conn-import-url = URL адрес на връзката
conn-import-add = Добавяне
conn-import-url-hint = URL адрес на връзка, JDBC URL адрес или ред DATABASE_URL от .env — по един на ред.
conn-import-choose-file = Избор на файл…
conn-import-scan = Сканиране на инсталираните клиенти
conn-import-scanning = Сканиране…
conn-import-docker = Docker контейнери
conn-import-asking-docker = Запитване към Docker…
conn-import-looking = Търсене на връзки на този компютър…
conn-import-none-found = Не са намерени връзки. DBeaver, IDE-тата на JetBrains и клиентите за команден ред пазят файловете си на известни места; ако вашият е другаде, изберете го по-горе.
conn-import-nothing-yet = Все още нищо не е намерено — поставете URL адрес, прочетете файл или сканирайте за клиенти, които вече използвате.
conn-import-found = { $count ->
    [one] Намерена е { $count } връзка
   *[other] Намерени са { $count } връзки
}
conn-import-select-all = Избиране на всички
conn-import-select-none = Нито една
conn-import-note-no-password = Няма парола в източника
conn-import-note-already-saved = Вече е запазена
conn-import-note-unexpanded-path = Пътят съдържа неразгънат макрос
conn-import-note-pgpass = Паролата е взета от вашия собствен .pgpass
conn-import-note-port-assumed = Източникът не посочва порт; това е портът по подразбиране
conn-import-note-external-login = Вписва се по начин, който Schemaic не поддържа: задайте SQL вход
conn-import-skipped = { $count ->
    [one] { $count } запис не беше импортиран: { $names }.
   *[other] { $count } записа не бяха импортирани: { $names }.
}
conn-import-skipped-more = { $count ->
    [one] { $count } запис не беше импортиран: { $names } и още { $rest }.
   *[other] { $count } записа не бяха импортирани: { $names } и още { $rest }.
}
conn-import-reason-unsupported-engine = неподдържана система ({ $driver })
conn-import-reason-no-server = няма сървър за свързване
conn-import-reason-not-published = портът му не е публикуван към този компютър
conn-import-reason-unreadable = не можа да бъде прочетен ({ $error })
conn-import-reason-not-an-export = не можа да бъде прочетен (не е експорт на връзки от Schemaic: { $error })
conn-import-reason-newer-export = не можа да бъде прочетен (експортиран от по-нова версия на Schemaic (версия { $version }) — обновете, за да го импортирате)
conn-import-url-empty = Поставете URL адрес на връзка.
conn-import-url-no-scheme = Няма схема — URL адресът започва с mysql://, postgresql://, sqlserver:// или sqlite:, а низът за връзка към SQL Server — със Server=.
conn-import-url-unknown-scheme = Непозната схема „{ $scheme }“ — очаква се mysql, mariadb, postgresql, sqlserver или sqlite.
conn-import-url-no-host = В URL адреса няма хост.
conn-import-url-bad-port = „{ $port }“ не е номер на порт.
conn-import-url-no-path = Няма файл на база данни след sqlite:.
conn-import-url-transport = „{ $server }“ се достига чрез named pipes или LocalDB — Schemaic се свързва само по TCP.
conn-import-url-userinfo = Името на сървъра съдържа @ — потребителското име или паролата се въвеждат в отделно поле, а не в сървъра.
conn-import-url-not-sql-server = Този низ за връзка не указва, че е за SQL Server — .NET драйверите на MySQL и PostgreSQL използват същите ключови думи. Вместо това поставете URL адреса на сървъра (mysql://, postgresql://, sqlserver://) или, за SQL Server, запишете сървъра като tcp:host.
conn-import-row-overflow = Показани са първите { $shown } от { $total }. Останалите { $hidden } също се импортират, ако са отметнати — списъкът спира дотук, за да остане прозорецът отзивчив.
conn-import-import = Импортиране
conn-import-import-count = Импортиране ({ $count })

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

blob-kind-png = PNG изображение
blob-kind-jpeg = JPEG изображение
blob-kind-gif = GIF изображение
blob-kind-bmp = BMP изображение
blob-kind-webp = WebP изображение
blob-kind-ico = Икона
blob-kind-opaque = Двоични данни
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · показани са първите { $shown }
blob-pane-preview = Преглед
blob-pane-hex = Hex
blob-reading = Четене…
blob-empty = Тази клетка е NULL или редът ѝ вече не съществува.
blob-preview-too-large = Това изображение е { $width } × { $height } — твърде голямо за преглед тук. Байтовете са непокътнати: прочетете ги като Hex или ги запазете във файл.
blob-preview-truncated = Прочетени са само първите { $shown } от { $key ->
    [png] това PNG изображение
    [jpeg] това JPEG изображение
    [gif] това GIF изображение
    [bmp] това BMP изображение
    [webp] това WebP изображение
    [ico] тази икона
   *[other] тази стойност
}, така че изобразяването е невъзможно. Прочетете байтовете като Hex.
blob-preview-unreadable = Тези байтове започват като { $key ->
    [png] PNG изображение
    [jpeg] JPEG изображение
    [gif] GIF изображение
    [bmp] BMP изображение
    [webp] WebP изображение
    [ico] икона
   *[other] изображение
}, но не могат да бъдат прочетени като такова. Прочетете ги като Hex или ги запазете във файл.
blob-save-truncated = Можаха да бъдат прочетени само първите { $shown } от { $size }, така че ще се запази само началото на стойността.
blob-load = Зареждане от файл
blob-save = Запазване във файл
blob-load-dialog = Зареждане на двоична стойност от файл
blob-save-dialog = Запазване на двоичната стойност
blob-saved-to = Запазено в { $path }
blob-loaded-file = Файлът е зареден. Потвърдете в таблицата с резултати, за да го запишете.
blob-not-writable = В тази клетка не може да се записва.
blob-file-too-large = Този файл е { $got } — тази колона побира най-много { $most }.
blob-not-accepting = Тази клетка вече не приема стойност — нищо не е подготвено.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = ER диаграма
erd-scope = Обхват
erd-chip-tables = { $count ->
    [one] { $count } таблица
   *[other] { $count } таблици
}
erd-chip-relationships = { $count ->
    [one] { $count } релация
   *[other] { $count } релации
}
erd-chip-hidden = { $count ->
    [one] { $count } несвързана скрита
   *[other] { $count } несвързани скрити
}
erd-find-placeholder = Търсене на таблица или колона
erd-find-none = Няма съвпадения
erd-find-matches = { $hits ->
    [one] { $hits } съвпадение
   *[other] { $hits } съвпадения
}
erd-find-matches-in-tables = { $hits ->
    [one] { $hits } съвпадение в { $tables } таблици
   *[other] { $hits } съвпадения в { $tables } таблици
}
erd-more-columns = още +{ $count }
erd-show-less = по-малко
erd-not-loaded = Схемата за тази база данни все още не е заредена.
erd-no-relationships = Няма релации чрез външни ключове за диаграма.
erd-export-tip = Експортиране на диаграмата…
erd-copy-as = Копиране като
erd-export-dialog = Експортиране на диаграмата
erd-nothing-to-export = Няма нищо за експортиране.
erd-nothing-to-copy = Няма нищо за копиране.
erd-copied-as = Копирано като { $format }
erd-clipboard-unavailable = Клипбордът не е достъпен.
erd-copy-failed = Неуспешно копиране: { $error }
erd-saved = Запазено: { $name }
erd-unnamed-file = файла

## Server variables modal (server_vars_view.rs)

server-vars-title = Сървърни променливи — { $connection }
server-vars-search = Търсене на настройки
server-vars-variables = Променливи
server-vars-status = Състояние
server-vars-loading = Зареждане…
server-vars-unsupported = Системата на тази връзка не публикува такъв списък.
server-vars-no-match = Няма съвпадащи настройки.
server-vars-more = { $count ->
    [one] Още { $count } настройка съвпада — стеснете търсенето
   *[other] Още { $count } настройки съвпадат — стеснете търсенето
}
server-vars-count = { $count ->
    [one] { $count } настройка
   *[other] { $count } настройки
}
server-vars-count-filtered = { $shown } от { $count } настройки
server-vars-refresh = Опресняване

## Edit snippet modal (snippet_edit.rs)

snippet-edit-title = Редактиране на фрагмент — { $name }
snippet-edit-name = Име
snippet-edit-name-placeholder = Име на фрагмента
snippet-edit-shortcut = Съкращение за разгъване
snippet-edit-shortcut-none = няма
snippet-edit-query = Заявка
snippet-edit-save = Запазване

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = План за изпълнение
plan-analyze-unavailable = Analyze не е налично (изразът записва данни)
plan-analyze-hint = Изпълнява израза, за да измери реалните времена
plan-explaining = Анализиране
plan-potential-issues = Възможни проблеми
plan-detail-no-index = не се използва индекс
plan-detail-key-unused = индексът не се използва за търсене
plan-my-full-scan = Пълно сканиране на таблицата `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Пълно сканиране на резултата ({ $detail }, type = ALL)
plan-my-filesort = Filesort върху `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort върху резултата (Extra: Using filesort)
plan-my-temporary = Временна таблица за `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Временна таблица за резултата (Extra: Using temporary)
plan-pg-seq-scan = Последователно сканиране на `{ $table }` (не се използва индекс)
plan-pg-seq-scan-unnamed = Последователно сканиране на таблица (не се използва индекс)
plan-pg-sort = Възел за сортиране — заявката сортира редовете в движение
plan-ms-full-scan = Пълно сканиране на `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Пълно сканиране на таблица ({ $op })
plan-ms-sort = Сортиране — заявката сортира редовете в движение
plan-ms-spool = { $op } — междинна работна таблица в tempdb
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = няма предикат за съединение — всеки ред се съчетава с всеки ред
plan-server-no-statistics = колона, която чете, няма статистика
plan-server-spill = изливане в tempdb — разпределената памет е била недостатъчна
plan-server-convert = преобразуване на тип засяга плана ({ $issue }): { $expression }
plan-server-unmatched-indexes = филтриран индекс не можа да бъде използван
plan-server-memory-grant = разпределената памет е била преценена неправилно
plan-server-wait = чакане на { $wait }
plan-server-other = сървърът предупреждава: { $name }
plan-missing-index = Липсващ индекс за `{ $table }` ({ $keys })
plan-missing-index-include = Липсващ индекс за `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Липсващ индекс за `{ $table }` ({ $keys }) — сървърът очаква подобрение с { $impact }%
plan-missing-index-include-impact = Липсващ индекс за `{ $table }` ({ $keys }; include { $include }) — сървърът очаква подобрение с { $impact }%
plan-ask-ai = Въпрос към ИИ
plan-ms-operation = Операция
plan-ms-object = Обект
plan-ms-estimated-rows = Очаквани редове
plan-ms-estimated-cost = Очаквана цена
plan-ms-actual-rows = Действителни редове
plan-ms-executions = Изпълнения
plan-ms-statement = Израз { $n }: { $kind }
plan-ms-statement-in-procedure = Израз { $n }: { $kind } в процедурата { $name }
plan-ms-statement-in-function = Израз { $n }: { $kind } във функцията { $name }
plan-ms-statement-in-a-procedure = Израз { $n }: { $kind } в процедура
plan-ms-statement-in-a-function = Израз { $n }: { $kind } във функция
plan-ms-plan = План
plan-ms-no-plan = Нито един израз в този пакет няма план.
plan-ms-unread = Планът не можа да бъде прочетен: { $reason }
plan-ms-unread-not-xml = не е XML документ
plan-ms-unread-not-showplan = очакваше се ShowPlanXML, а е намерен { $found }
plan-ms-unread-empty = празен документ

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

used-by-title = Използва се от — { $object }
used-by-copy = Копиране
used-by-not-loaded = Схемата на { $database } все още не е заредена — разгънете я в дървото и попитайте отново.
used-by-nothing-in = Нищо в { $database } не го използва.
used-by-nothing = Нищо не го използва.
used-by-limits = Намерено чрез четене на дефинициите в тази база данни: SQL, изграждан по време на изпълнение, и други бази данни не се виждат.
used-by-dropped-with-it = { $detail } · премахва се заедно с него
used-by-heading-view = { $count ->
    [one] Изглед ({ $count })
   *[other] Изгледи ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Подпрограма ({ $count })
   *[other] Подпрограми ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Тригер ({ $count })
   *[other] Тригери ({ $count })
}
used-by-heading-event = { $count ->
    [one] Събитие ({ $count })
   *[other] Събития ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Външен ключ ({ $count })
   *[other] Външни ключове ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Табличен израз ({ $count })
   *[other] Таблични изрази ({ $count })
}
used-by-noun-view = изглед
used-by-noun-routine = подпрограма
used-by-noun-trigger = тригер
used-by-noun-event = събитие
used-by-noun-foreign-key = външен ключ
used-by-noun-expression = табличен израз

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Тъмна
theme-light = Светла

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

md-code-block = КОД
md-code-copy = Копиране
md-code-insert = Вмъкване
md-code-run = Изпълнение
md-proposal-title = Предложена промяна в { $table }
md-proposal-count = { $count ->
    [one] { $count } промяна
   *[other] { $count } промени
}
md-proposal-review = Преглед на промяната…
md-proposal-unreadable = Асистентът предложи промяна, която не можа да бъде прочетена — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

cell-choose = Изберете…
cell-calendar-month = { $month } { $year }
cell-calendar-now = Сега
cell-calendar-today = Днес
cell-month-1 = Януари
cell-month-2 = Февруари
cell-month-3 = Март
cell-month-4 = Април
cell-month-5 = Май
cell-month-6 = Юни
cell-month-7 = Юли
cell-month-8 = Август
cell-month-9 = Септември
cell-month-10 = Октомври
cell-month-11 = Ноември
cell-month-12 = Декември
cell-weekday-mon = Пн
cell-weekday-tue = Вт
cell-weekday-wed = Ср
cell-weekday-thu = Чт
cell-weekday-fri = Пт
cell-weekday-sat = Сб
cell-weekday-sun = Нд

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Преименуване на псевдоним
alias-rename-title-cte = Преименуване на CTE
alias-rename-placeholder-alias = Псевдоним
alias-rename-placeholder-cte = Име на CTE
alias-rename-new-name = Ново име
alias-rename-rename = Преименуване

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = Тук вече няма какво да се преименува.
alias-rename-empty = Дайте му име.
alias-rename-conflict = „{ $name }“ вече е име в тази заявка: преименуването с него би променило коя таблица чете дадено обръщение.
alias-rename-unsure = Обръщение тук може да чете друга колона с това име или да чете тази там, където преименуването не може да го последва, а заредената схема не може да прецени — затова не се прави редакция. Заредете схемата на таблиците или преименувайте ръчно.
alias-rename-changed = Заявката се промени, докато това беше отворено. Затворете го и преименувайте отново.
alias-rename-read-only = Редакторът е само за четене, докато предложението на ИИ не бъде прието или отхвърлено.

## The editor's usages list (usages_view.rs)

usages-title = { $count ->
    [one] { $count } използване на { $name }
   *[other] { $count } използвания на { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = Невалиден SVG: { $error }
raster-too-large = Изображението е твърде голямо за растеризиране ({ $width }×{ $height })
raster-png-failed = Кодирането в PNG е неуспешно: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Копиране
grid-menu-copy-value = Копиране на стойността
grid-menu-copy-formatted = Копиране с форматиране
grid-menu-copy-as = Копиране като
grid-menu-paste = Поставяне
grid-menu-fill-down = Попълване надолу
grid-menu-fill-up = Попълване нагоре
grid-menu-edit-field = Редактиране на полето
grid-menu-edit-row = Редактиране на реда
grid-menu-view-row = Преглед на реда
grid-menu-edit-binary = Редактиране на двоичните данни
grid-menu-view-binary = Преглед на двоичните данни
grid-menu-filter-by-value = Филтриране по тази стойност
grid-menu-exclude-value = Изключване на тази стойност
grid-menu-follow-relation = Следване на релацията
grid-menu-referenced-by = Сочат към него
grid-menu-set-null = Задаване на NULL
grid-menu-set-default = Задаване на DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Дублиране на реда
   *[other] Дублиране на { $count } реда
}
grid-menu-delete-rows = { $count ->
    [one] Изтриване на реда
   *[other] Изтриване на { $count } реда
}
grid-menu-undo-delete = Отмяна на изтриването
grid-menu-ai-summary = Обобщение от ИИ
grid-attach-to-chat = Прикачване на { $scope } към разговора
grid-attach-scope-rows = { $rows ->
    [one] { $rows } ред
   *[other] { $rows } реда
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } колона
   *[other] { $cols } колони
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } ред
   *[other] { $rows } реда
} и { $cols ->
    [one] { $cols } колона
   *[other] { $cols } колони
}
grid-menu-freeze = Замразяване
grid-menu-unfreeze = Размразяване
grid-menu-hide-column = Скриване на колоната
grid-menu-show-hidden-columns = Показване на скритите колони ({ $count })
grid-menu-show-all = Показване на всички
grid-menu-format-as = Форматиране като
grid-format-none = Без
grid-format-timestamp = Дата и час
grid-format-grouped = Групиране на цифрите
grid-format-bytes = Размер на файл
grid-format-bool = Булева стойност
grid-menu-copy-name = Копиране на името
grid-menu-copy-qualified-name = Копиране на пълното име
grid-menu-copy-values = Копиране на стойностите
grid-menu-sql-in-list = SQL списък за IN

## The results grid's toolbar (grid.rs) — the status line, the commit and row
## controls, and the copy, save, AI and chart icons.

grid-stats = { $rows } · { $cols ->
    [one] { $cols } кол.
   *[other] { $cols } кол.
} · { $ms } мс
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } кол.
   *[other] { $cols } кол.
} · { $ms } мс
grid-rows-read = { $n ->
    [one] { $read } ред
   *[other] { $read } реда
}
grid-rows-read-capped = { $n ->
    [one] { $read } ред
   *[other] { $read } реда
} (ограничено)
grid-rows-read-of-total = { $n ->
    [one] { $read } от { $total } ред
   *[other] { $read } от { $total } реда
}
grid-kept-note = · запазен — само за четене
grid-arena-note = · { $columns } — твърде големи за пълно съхранение; следващите редове са празни
grid-sorted-subset = · сортирана част (ограничено) — не е пълната подредба
grid-read-all = прочитане на всички редове
grid-read-more = прочитане на { $rows } реда
grid-reading = четене…
grid-committing = Потвърждаване…
grid-commit-tip = { $count ->
    [one] Потвърждаване на { $count } промяна ({ $keys })
   *[other] Потвърждаване на { $count } промени ({ $keys })
}
grid-discard-tip = Отхвърляне на всички чакащи промени
grid-copy-sql-tip = Копиране на промените като SQL
grid-add-row-tip = Добавяне на ред
grid-delete-row-tip = Отбелязване на избрания ред за изтриване ({ $key })
grid-clone-row-tip = Клониране на избрания ред
grid-copy-results-tip = Копиране на резултатите…
grid-copy-results-empty = Няма нищо за копиране — няма редове
grid-save-results-tip = Запазване на резултатите във файл…
grid-save-results-empty = Няма нищо за запазване — няма редове
grid-ai-tip = Генериране на редове с ИИ…
grid-ai-generating = Генериране…
grid-ai-off = Действията с ИИ изпращат данни — изключени за тази връзка
grid-ai-fill-value = Попълване на стойност с ИИ
grid-ai-insert-row = Вмъкване на ред с ИИ
grid-ai-seed-table = Попълване на таблицата с ИИ
grid-chart-tip = Диаграма на резултатите
grid-chart-empty = Няма нищо за диаграма — няма редове
grid-chart-this-result = този резултат

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

grid-error-view = Преглед
grid-rollback-tab = Връщане на { $tab }
grid-find-placeholder = Търсене в резултатите
grid-figure-copied = Копирано
grid-figure-rows = { $n ->
    [one] { $n }{ $more } ред
   *[other] { $n }{ $more } реда
}
grid-figure-nulls = { $n }{ $more } NULL
grid-figure-distinct = { $n } различни
grid-figure-sum = Сума { $value }
grid-figure-avg = Средно { $value }
grid-figure-median = Медиана { $value }
grid-figure-min = Мин. { $value }
grid-figure-max = Макс. { $value }
grid-figure-length = { $n ->
    [one] Дължина { $n } знак
   *[other] Дължина { $n } знака
}
grid-figure-length-range = Дължина { $lo }–{ $hi } знака
grid-goto-label = Към ред:

## The results grid's clipboard, fill, attach and export (grid.rs).

export-dest-clipboard = клипборда
export-dest-file = файла
export-note-rows = { $n ->
    [one] Експортиран е { $rows } ред в { $name }
   *[other] Експортирани са { $rows } реда в { $name }
}
export-note-with-caveats = { $head } — { $caveats }
export-note-withheld = { $n ->
    [one] { $cols } съдържа байтове, които този формат не може да пренесе: тези клетки са празни
   *[other] { $cols } съдържат байтове, които този формат не може да пренесе: тези клетки са празни
}
export-note-blanked = { $cols } — твърде големи за пълно съхранение: следващите редове са празни
export-note-cut = { $cols } — съкратени до ограничението на Excel от { $limit } знака за клетка
export-failure-partial = { $message } — { $name } не е променен; записаните редове са в { $part }
export-cancelled-partial = Експортът е прекратен — { $name } не е променен; записаните редове са в { $part }
export-cancelled-nothing = Експортът е прекратен — { $name } не е променен.
copy-sql-as-null = { $n ->
    [one] Копирано в клипборда — { $cols } съдържа байтове, които SQL текстът не може да пренесе: записани са като NULL
   *[other] Копирано в клипборда — { $cols } съдържат байтове, които SQL текстът не може да пренесе: записани са като NULL
}
copy-sql-nothing = { $n ->
    [one] Нищо не е копирано — { $cols } съдържа байтове, които SQL текстът не може да пренесе
   *[other] Нищо не е копирано — { $cols } съдържат байтове, които SQL текстът не може да пренесе
}
copy-sql-left-out = { $n ->
    [one] Копирано в клипборда — { $cols } съдържа байтове, които SQL текстът не може да пренесе: тези стойности са пропуснати
   *[other] Копирано в клипборда — { $cols } съдържат байтове, които SQL текстът не може да пренесе: тези стойности са пропуснати
}

grid-fill-nothing = Няма какво да се попълни: в избраните колони не може да се записва или клетката за копиране е незададена стойност на нов ред или подготвен DEFAULT.
grid-paste-outside = { $n } извън таблицата
grid-paste-read-only = { $n } в колони само за четене
grid-paste-capped = { $n } над ограничението от { $cap } клетки за поставяне
grid-paste-deleted = { $n } в редове, отбелязани за изтриване
grid-paste-nothing = Нищо не е поставено: { $notes }.
grid-paste-partial = { $count ->
        [one] Поставена е { $cells } клетка, пропуснати са { $notes }.
       *[other] Поставени са { $cells } клетки, пропуснати са { $notes }.
    }
grid-row-not-editable = Колоната `{ $column }` не може да се редактира.
grid-row-not-nullable = Колоната `{ $column }` не може да е NULL.
grid-fill-skipped = { $count ->
    [one] Попълнена е { $filled } клетка, пропуснати са { $skipped } в редове, отбелязани за изтриване.
   *[other] Попълнени са { $filled } клетки, пропуснати са { $skipped } в редове, отбелязани за изтриване.
}
grid-attach-summary = { $rows ->
    [one] { $rows } ред
   *[other] { $rows } реда
} × { $cols ->
    [one] { $cols } колона
   *[other] { $cols } колони
}
grid-attach-summary-from = { $rows ->
    [one] { $rows } ред
   *[other] { $rows } реда
} × { $cols ->
    [one] { $cols } колона
   *[other] { $cols } колони
} от { $table }
grid-attach-summary-capped = { $summary } (първите { $taken } от { $total } избрани)
grid-copy-sql-unset = Няма нищо за копиране като SQL — избраните клетки на новия ред все още нямат стойност и приемат стойността по подразбиране на сървъра.
grid-copy-sql-joined = Няма нищо за копиране като SQL: тези редове идват от заявка върху повече от една таблица.
grid-copy-sql-no-keyed-table = Няма нищо за копиране като SQL: редовете трябва да идват от една таблица с ключ.
grid-copy-sql-key-too-large = Няма нищо за копиране като SQL: ключът беше твърде голям, за да се съхрани изцяло в този резултат, така че някои редове не могат да бъдат посочени чрез него.
grid-copy-sql-pinned = Няма нищо за копиране като SQL: този резултат е закачен, така че редовете му са моментна снимка.
grid-copy-sql-read-only = Няма нищо за копиране като SQL: тази връзка е само за четене.
grid-copy-update-too-large = Няма нищо за копиране като UPDATE: избраните колони бяха твърде големи, за да се съхранят изцяло в този резултат, и някои от клетките им са празни.
grid-copy-update-nothing-writable = Няма нищо за копиране като UPDATE: в нито една от избраните колони не може да се записва.
grid-copy-where-in-no-key = Няма нищо за копиране като WHERE IN: тази таблица няма първичен или уникален ключ, чрез който да се посочат редовете ѝ.
grid-copy-new-row-no-key = Няма нищо за копиране: новият ред няма ключ, докато не бъде запазен.
grid-export-fetched-rows = Прочетените редове ({ $count })
grid-export-dialog-title = Експортиране на резултатите
grid-export-already-running = Вече се изпълнява експорт — нищо не е записано.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

grid-row-title = Ред { $row }
grid-row-title-in-table = Ред { $row }  ·  { $table }
grid-row-set-value = Задаване на стойност
grid-row-set-null = Задаване на NULL
grid-row-edit-binary = Редактиране
grid-row-view-binary = Преглед
grid-row-invalid-json = Невалиден JSON: { $error }
grid-row-saving = Запазване

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

grid-rows-affected = { $count ->
    [one] { $count } засегнат ред · { $ms } мс
   *[other] { $count } засегнати реда · { $ms } мс
}
grid-rerun-would-write = Този израз не може да се изпълни отново — би записал в базата данни
grid-filter-not-simple = Тази заявка не може да се филтрира — не е прост SELECT върху една таблица
grid-filter-bracket-in-name = име, съдържащо `]`, не може да се филтрира тук
grid-filter-trailing-input = неочакван текст след условието
grid-staged-requery = Първо потвърдете или отхвърлете промените си — повторното четене на редовете би ги изгубило
grid-staged-landing = По време на повторното четене на таблицата бяха подготвени промени, затова те бяха запазени, а таблицата не беше опреснена — потвърдете или отхвърлете ги и след това опреснете.
grid-reread-without-filter = Филтърът извиква функция, която Schemaic не изпълнява отново без питане, затова след потвърждаването таблицата беше прочетена отново без филтъра и сортирането.
grid-hidden-columns-tip = { $count ->
    [one] { $count } скрита колона — щракнете, за да покажете всички, или щракнете с десния бутон върху заглавие, за да изберете
   *[other] { $count } скрити колони — щракнете, за да покажете всички, или щракнете с десния бутон върху заглавие, за да изберете
}
grid-copied-staged-sql = Подготвените промени са копирани като SQL.
grid-copy-split = { $n ->
        [one] { $n } копирана клетка съдържа табулация или нов ред — при поставяне тя ще бъде прочетена като повече от една клетка.
       *[other] { $n } копирани клетки съдържат табулация или нов ред — при поставяне всяка от тях ще бъде прочетена като повече от една клетка.
    }
grid-duplicate-title = Дублиране на редове
grid-duplicate-question = { $n ->
    [one] Да се дублира ли { $rows } ред? Той се подготвя като чакащ ред и нищо не се записва, докато не потвърдите.
   *[other] Да се дублират ли { $rows } реда? Те се подготвят като чакащи редове и нищо не се записва, докато не потвърдите.
}
grid-nullable-tip = Допуска NULL
grid-cell-auto = <авто>
grid-cell-required = <задължително>
grid-seed-title = Попълване с редове
grid-seed-generate = Генериране

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

footer-panel-schema = Схема
footer-panel-ai = ИИ асистент
footer-panel-history = История на заявките
footer-panel-snippets = Библиотека с фрагменти
footer-panel-activity = Активност на сървъра
footer-panel-terminal = Терминал
footer-panel-narrow = Прозорецът е твърде тесен за този панел — разширете го, за да се върне панелът.
footer-cursor = Ред { $line }, кол. { $col }
footer-indent-spaces = Интервали: { $width }
footer-indent-tabs = Табулации: { $width }
footer-spaces = Интервали
footer-tabs = Табулации
footer-wrap = Пренасяне
footer-no-wrap = Без пренасяне
footer-read-only = Само за четене
footer-write-mode = Режим на запис
footer-tx-auto = Автопотвърждаване
footer-tx-manual = Ръчен
footer-tx-open = Отворена: { $count }
footer-tx-aborted = Транзакцията е прекъсната — върнете я, за да продължите
footer-tx-lost = Транзакцията е изгубена
footer-commit = Потвърждаване
footer-rollback = Връщане
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

lib-tab-default-title = Заявка { $n }
lib-result-default-label = Резултат { $n }
lib-ai-off-note = ИИ асистентът е изключен. Изберете CLI на агента в настройките на ИИ, за да го включите.
lib-test-testing = Тестване…
lib-test-connected = Свързано
lib-test-failed = Връзката е неуспешна

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

lib-effort-minimal = Минимално
lib-effort-low = Ниско
lib-effort-medium = Средно
lib-effort-high = Високо
lib-effort-extra = Повишено
lib-effort-max = Максимално
lib-schema-scope-active = Само активната база данни
lib-schema-scope-all = Всички бази данни
lib-schema-scope-none = Без
lib-cursor-block = Блок
lib-cursor-bar = Черта
lib-cursor-underline = Подчертаване

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = Няма връзка
header-new-connection = Нова връзка
header-find-anywhere-tip = Търсене навсякъде ({ $keys })
header-shortcuts-tip = Клавишни комбинации
header-settings-tip = Настройки
header-disconnected = Връзката е прекъсната
header-retry = Нов опит
colour-red = Червен
colour-orange = Оранжев
colour-amber = Кехлибарен
colour-green = Зелен
colour-teal = Синьозелен
colour-blue = Син
colour-purple = Лилав
colour-pink = Розов

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

disk-changed = „{ $name }“ е променен на диска.
disk-reload = Презареждане
disk-keep-mine = Запазване на моята версия
results-run-hint = Изпълнете заявка  ({ $keys })
results-failed = Изразът е неуспешен.
results-cancelled = Заявката е прекратена.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

results-pinned-total = Закачени: { $count } · { $size }
results-pinned-tip = Закачените резултати се държат в паметта, докато не ги затворите. Затворете тези, с които сте приключили, за да я освободите.
results-chip-nothing-run = В този раздел все още нищо не е изпълнено.
results-chip-kept = запазен — само за четене
results-menu-pin = Закачване
results-menu-unpin = Откачване
results-menu-close = Затваряне
results-menu-compare-with = Сравняване с { $result }
results-menu-close-others = Затваряне на другите резултати
results-menu-close-all = Затваряне на всички резултати

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Търсене навсякъде

## Shared widgets (widgets.rs).

widgets-nothing-to-choose = Няма от какво да се избира

widgets-spinner-verbs = Обмисляне, Размишляване, Мъдруване, Умуване, Чудене, Варене, Печене, Къкрене, Бъркане, Месене, Втасване, Ферментиране, Мариноване, Подправяне, Карамелизиране, Фламбиране, Дестилиране, Кристализиране, Пресмятане, Изчисляване, Дешифриране, Разплитане, Разнищване, Съчиняване, Измисляне, Импровизиране, Скициране, Драскане, Майсторене, Ковене, Шлифоване, Тъкане, Плетене, Бродиране, Дялкане, Скитане, Лутане, Шляене, Подскачане, Пърхане, Кръжене, Въртене, Танцуване, Полюшване, Левитиране, Телепортиране, Медитиране, Мечтаене, Блянуване, Фантазиране, Тананикане, Свирукане, Жонглиране, Гадаене, Омагьосване, Заклинане, Трансмутиране, Метаморфозиране, Покълване, Разцъфтяване, Опрашване, Измътване, Гнездене, Мъркане, Бръмчене, Жужене, Пухтене, Чоплене, Ровене, Копаене, Пресяване, Претърсване, Надничане, Пренареждане, Сглобяване, Архитектуриране, Оркестриране, Хармонизиране, Синтезиране, Смилане, Преживяне, Шушукане, Философстване, Суетене, Щъкане, Мотаене, Отгатване, Галопиране, Сърфиране, Пеене, Рисуване, Ваяне, Сладкодумстване, Зъзнене, Пъплене

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Попитайте ИИ асистента за помощ.
cmdk-accept = Приемане
cmdk-reject = Отхвърляне
cmdk-no-changes = Няма предложени промени
cmdk-diff-summary = { $hunks ->
        [one] { $n } блок · { $removed } − / { $added } +
       *[other] { $n } блока · { $removed } − / { $added } +
    }

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = стойност
params-kind-tip = Текст, число, NULL или суров SQL — щракнете, за да смените
params-missing = Няма стойност за { $names }
params-not-a-number = :{ $name } е число, но съдържа „{ $text }“

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

editor-error-view = Преглед
editor-run-anyway = Изпълнение въпреки това
guard-read-only = Връзка само за четене.
guard-read-only-refused = Връзка само за четене: { $reason }
read-only-several = позволен е само един израз
read-only-not-a-read = позволени са само заявки само за четене ({ $heads })
read-only-denied = `{ $word }` не е позволено в заявка само за четене
read-only-advances-sequence = `NEXT VALUE FOR` придвижва последователност, което заявка само за четене не прави
read-only-unlisted-call = `{ $name }` не е функция, за която е известно, че е само за четене, затова не е позволена в заявка само за четене; функциите от разширения или дефинираните в базата данни също се отказват
read-only-locks = `{ $clause }` заключва редовете, които чете, което заявка само за четене не прави; премахнете клаузата за заключване
guard-no-database = Не е избрана база данни.
guard-writes-one = Този израз променя данни.
guard-writes-several = Тези изрази променят данни.
guard-script = Да се изпълнят ли всички изрази в { $file }? Скриптът може да създава, променя и премахва обекти, а промените му не могат да бъдат отменени оттук.
guard-staged = { $n ->
    [one] Това изпълнение заменя резултата с { $n } подготвена промяна — изпълнението би я отхвърлило. Първо я потвърдете или отхвърлете, или изпълнете въпреки това.
   *[other] Това изпълнение заменя резултата с { $n } подготвени промени — изпълнението би ги отхвърлило. Първо ги потвърдете или отхвърлете, или изпълнете въпреки това.
}
guard-truncate-partition-all = TRUNCATE PARTITION ALL премахва всички редове в таблицата.
guard-truncate = TRUNCATE премахва всички редове в таблицата.
guard-no-where = Открит е израз { $kind } без клауза WHERE.
guard-drop-table = DROP TABLE изтрива таблицата и всички редове в нея.
guard-drop-database = DROP DATABASE изтрива базата данни и всички таблици в нея.
guard-drop-schema = DROP SCHEMA изтрива схемата и всички таблици в нея.
guard-drop-owned = DROP OWNED изтрива всичко, което ролята притежава, всяка таблица заедно с редовете ѝ.
guard-drop-cascade = DROP { $obj } … CASCADE изтрива всяка колона, която зависи от него, заедно със стойностите ѝ.
guard-replace-table = CREATE OR REPLACE TABLE изтрива съществуващата таблица и всички редове в нея.
guard-replace-database = CREATE OR REPLACE DATABASE изтрива съществуващата база данни и всички таблици в нея.
guard-replace-schema = CREATE OR REPLACE SCHEMA изтрива съществуващата схема и всички таблици в нея.
guard-truncate-partition = TRUNCATE PARTITION премахва всички редове в посочените дялове.
guard-drop-partition = DROP PARTITION изтрива дяла и всички редове в него.
quickinfo-table = таблица { $name } · { $database }
quickinfo-view = изглед { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } колона
       *[other] { $n } колони
    }
quickinfo-primary-key = първичен ключ
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = автоувеличение
quickinfo-default = по подразбиране { $value }
quickinfo-generated = генерирана като { $expression }
quickinfo-function = функция { $name }({ $arguments })
quickinfo-function-returns = функция { $name }({ $arguments }) връща { $returns }
quickinfo-procedure = процедура { $name }({ $arguments })
quickinfo-procedure-returns = процедура { $name }({ $arguments }) връща { $returns }
completion-foreign-key = външен ключ
completion-expand-star = разгъване на *
completion-columns = { $count ->
        [one] { $n } колона
       *[other] { $n } колони
    }
editor-diag-fix = { $keys }: { $fix }
diag-syntax = Синтактична грешка
diag-syntax-detail = Синтактична грешка: { $detail }
diag-reserved-alias-backticks = `{ $word }` е запазена ключова дума и не може да се използва като псевдоним (оградете я с обратни апострофи)
diag-reserved-alias-double-quotes = `{ $word }` е запазена ключова дума и не може да се използва като псевдоним (оградете я с двойни кавички)
diag-reserved-alias-brackets = `{ $word }` е запазена ключова дума и не може да се използва като псевдоним (оградете я с квадратни скоби)
diag-table-not-found = Таблицата `{ $table }` не е намерена{ $within ->
        [some] {" "}в `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— може би имате предвид `{ $suggestion }`?
       *[none] {""}
    }
diag-column-not-found = Колоната `{ $column }` не е намерена{ $within ->
        [some] {" "}в `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— може би имате предвид `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` тук има псевдоним `{ $alias }` — уточнете с `{ $alias }`
diag-ambiguous-column = Колоната `{ $column }` е нееднозначна
diag-join-without-condition = JOIN няма условие ON/USING — това е кръстосано съединение; използвайте CROSS JOIN, ако това е целта
diag-comma-join-without-where = Съединение със запетая без WHERE — това е кръстосано съединение; добавете WHERE или използвайте CROSS JOIN, ако това е целта
diag-not-grouped = Колоната `{ $column }` трябва да е в GROUP BY или да се използва в агрегатна функция
diag-insert-row-width = INSERT посочва { $columns } { $columns ->
        [one] колона
       *[other] колони
    }, но този ред има { $values } { $values ->
        [one] стойност
       *[other] стойности
    }
diag-row-width-without-list = `{ $table }` приема { $needed } { $needed ->
        [one] стойност
       *[other] стойности
    } без списък с колони, но този ред има { $values }
diag-row-width-of-table = `{ $table }` има { $columns } { $columns ->
        [one] колона
       *[other] колони
    }, но този ред има { $values } { $values ->
        [one] стойност
       *[other] стойности
    }
diag-nullable-not-in = `{ $column }` може да е NULL, а `NOT IN` не съвпада с нито един ред, щом подзаявката върне такава стойност — добавете `WHERE { $column } IS NOT NULL` към нея или използвайте NOT EXISTS
diag-update-without-where = UPDATE без WHERE променя всички редове в таблицата
diag-delete-without-where = DELETE без WHERE премахва всички редове в таблицата
diag-null-comparison = Сравнение с NULL никога не е истина — използвайте { $fix }
diag-misspelled-keyword = `{ $word }` изглежда като грешно изписана ключова дума
diag-misspelled-function = `{ $word }` изглежда като грешно изписана функция
diag-flow-merge-needs-semicolon = `MERGE` трябва да завършва с `;`
diag-flow-if-without-statement = `IF` няма израз за изпълнение
diag-flow-else-without-statement = `ELSE` няма израз за изпълнение
diag-flow-while-without-statement = `WHILE` няма израз за изпълнение
diag-flow-end-try-without-begin-try = `END TRY` без съответен `BEGIN TRY`
diag-flow-end-catch-without-begin-catch = `END CATCH` без съответен `BEGIN CATCH`
diag-flow-end-without-begin = `END` без съответен `BEGIN`
diag-flow-throw-needs-semicolon = Изразът преди `THROW` трябва да завършва с `;`
diag-flow-end-try-without-catch = След `END TRY` трябва да следва `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` няма израз преди своя `ELSE`
diag-flow-else-without-if = `ELSE` без предхождащ `IF`
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` трябва да следва `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` няма съответен `END TRY`
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` няма съответен `END CATCH`
diag-flow-begin-without-end = `BEGIN` няма съответен `END`

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Въпрос към ИИ
editor-menu-explain = Обяснение
editor-menu-optimise = Оптимизиране
editor-menu-plan = План
editor-menu-copy-single-line = Копиране като един ред
editor-menu-make-single-line = Обединяване на един ред
editor-menu-format-statement = Форматиране на израза
editor-menu-find-usages = Търсене на използванията
editor-menu-create-view = Създаване на изглед
cmdk-optimise-label = Оптимизиране на тази заявка
editor-several-carets = Няколко курсора и нищо маркирано: маркирайте какво да се изпълни или оставете един курсор

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Изпълнение на текущия
editor-run-everything = Изпълнение на всичко

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Търсене
find-invalid = Невалиден
find-bad-pattern = Невалиден шаблон
find-bad-pattern-why = Невалиден шаблон: { $why }
find-match-case = Съвпадение на регистъра
find-whole-word = Цяла дума
find-regex = Регулярен израз
find-scope = Изб.
find-in-selection-tip = Търсене в маркираното
find-replace-placeholder = Замяна
find-replace = Замяна
find-replace-all = Всички
goto-line-label = Към:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

cli-no-client = Не е намерен клиент { $client } (PATH или WSL).
cli-no-client-on-path = Не е намерен клиент { $client } в PATH.
cli-no-tool = Не е намерен { $tool } в PATH.
cli-no-either-tool = Не е намерен нито { $first }, нито { $second } в PATH.
cli-no-either-client = Не е намерен нито клиент { $first }, нито { $second } в PATH.
cli-sqlcmd-windows-in-wsl = Вписването с Windows изисква sqlcmd за Windows, а намереният е в WSL. Инсталирайте sqlcmd в Windows или отворете връзката в раздел за заявки.

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

cli-refused-shim = Клиентът, намерен в PATH, е .cmd/.bat обвивка, която Windows изпълнява чрез cmd.exe — където име на база данни, съдържащо '&' или '|', би станало втора команда. Насочете настройките към истинския изпълним файл или вместо това отворете раздел за заявки.
cli-refused-name-equals = Името на тази база данни съдържа '=', което psql би прочел като низ за връзка и би последвал към друг сървър. Вместо това я отворете в раздел за заявки.
cli-refused-name-uri = Името на тази база данни започва като URI за връзка, който psql би последвал към друг сървър. Вместо това я отворете в раздел за заявки.
cli-refused-file-dash = Името на файла на тази база данни започва с '-', което клиентът sqlite3 би прочел като опция, а не като файла. Преименувайте го или посочете път (./name.db) и го отворете отново.
cli-refused-sqlcmd-entra = На sqlcmd не може да се предаде токенът за вписване от Azure CLI, така че тази връзка не може да се отвори в него. Вместо това я отворете в раздел за заявки.
cli-refused-sqlcmd-host = Хостът на тази връзка съдържа знак, който sqlcmd чете като част от адреса (',', ';', втори '\' или префикс ':'). Задайте порта в отделното му поле и я отворете отново.
cli-refused-sqlcmd-ca = sqlcmd не може да проверява спрямо CA файл. Добавете CA в системното хранилище за доверени сертификати и изчистете полето за CA на връзката или отворете връзката в раздел за заявки.
cli-refused-sqlcmd-client-cert = SQL Server не вписва с клиентски сертификат, така че sqlcmd няма къде да изпрати този на връзката. Изчистете клиентския сертификат и ключ и я отворете отново.
cli-refused-wsl-paths = Пътищата до TLS сертификатите на тази връзка са пътища в Windows, които клиентът в WSL не може да отвори. Инсталирайте собствен клиент или преместете сертификатите в WSL.
cli-refused-tunnelled-verify = Тази връзка проверява сертификата на сървъра и го достига през SSH тунел, така че на клиента би било казано да се свърже със 127.0.0.1 и да провери сертификата спрямо този адрес — на което собственият сертификат на сървъра не може да отговори. Вместо това отворете раздел за заявки или намалете режима на TLS за CLI сесията.
cli-refused-not-container-id = docker ps отпечата нещо, което не е идентификатор на контейнер.
cli-refused-file-line-break = Името на този файл съдържа нов ред, който клиентът би прочел като два аргумента. Изберете друго име.
cli-refused-role = Ролята в началния SQL на тази връзка не може да бъде предадена на инструмента такава, каквато е.
cli-refused-mysql-source-path = Клиентът mysql би прочел пътя до този файл като свои собствени команди (съдържа ';' или '\' или интервал в края). Преместете го или го преименувайте и след това го възстановете.
cli-refused-cloud-token = Тази връзка се вписва с токен от облачен CLI, който не се предава на външен клиент. Вместо това я отворете в раздел за заявки.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

install-no-exe-path = Не може да се определи откъде работи Schemaic: { $error }
install-registry-open = Потребителската ви среда в регистъра не можа да бъде отворена: { $error }
install-path-read = Потребителската ви променлива PATH не можа да бъде прочетена: { $error }
install-path-write = Потребителската ви променлива PATH не можа да бъде записана: { $error }
install-look-at = { $path } не можа да бъде проверен: { $error }
install-read-link = Връзката в { $path } не можа да бъде прочетена: { $error }
install-create = { $path } не можа да бъде създаден: { $error }
install-remove-old = Старият { $path } не можа да бъде премахнат: { $error }
install-remove = { $path } не можа да бъде премахнат: { $error }
install-path-is-windows = Запис в потребителската PATH е инсталация за Windows; на тази платформа вместо това се създава връзка.
install-link-is-unix = Създаването на връзка към командата е инсталация за macOS и Linux.

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Не може да се определи в коя папка е { $exe }.
install-refuse-no-shim = До това копие на Schemaic, в { $dir }, няма schemaic.com, така че няма конзолна команда, която да се добави в PATH. Инсталираното приложение има такава; версията за разработка няма.
install-refuse-semicolon = Това копие на Schemaic е в { $dir }, чието име съдържа ';' — PATH би го прочел като две различни папки. Преместете го на място без такъв знак и опитайте отново.
install-refuse-exe-replaced = { $exe } беше заменен, докато Schemaic работеше, така че връзка към него не би сочила към нищо. Рестартирайте Schemaic и инсталирайте отново.
install-refuse-no-home = Няма домашна папка, в която да се постави връзката, така че няма къде да се търси.
install-refuse-path-not-text = Потребителската ви PATH не е съхранена като текст в регистъра, затова не е променяна.
install-refuse-path-not-unicode = Потребителската ви PATH съдържа знаци, които не могат да бъдат прочетени безопасно, затова не е променяна.
install-refuse-portable = Това е преносимо копие, така че „Премахване“ не може да прецени дали „Инсталиране“ е добавило { $dir } в PATH или папката вече е била там с други програми в нея. Премахнете я от „Променливи на средата“, ако „Инсталиране“ я е добавило.
install-refuse-spot-taken = { $link } вече съществува и не е връзка, затова „Инсталиране“ не го промени. Преместете го и опитайте отново.
install-refuse-points-elsewhere = { $link } вече сочи към { $to }. Премахнете го, ако това е старо копие на Schemaic, и опитайте отново.
install-refuse-unlink-elsewhere = { $link } сочи към { $to }, а не към това копие на Schemaic, затова „Премахване“ не го промени.
install-refuse-unlink-not-ours = { $link } не е връзка, създадена от Schemaic, затова „Премахване“ не я промени.
install-report-already = Командата schemaic вече е в PATH, в { $dir }.
install-report-added = { $dir } е добавена в потребителската ви PATH. Отворете нов терминал, за да използвате schemaic — вече отвореният запазва старата PATH.
install-report-linked = Създадена е връзка { $link } към { $target }.
install-report-linked-if-not-found = Създадена е връзка { $link } към { $target }. Ако нов терминал не намира schemaic, добавете { $dir } в PATH в профила на обвивката си.
install-report-linked-not-on-path = Създадена е връзка { $link } към { $target }. { $dir } все още не е в PATH — добавете я в профила на обвивката си и отворете нов терминал.
install-report-removed = { $dir } е премахната от потребителската ви PATH. Вече отвореният терминал запазва старата PATH.
install-report-path-absent = { $dir } не е в потребителската ви PATH, така че няма какво да се премахне.
install-report-unlinked = { $link } е премахнат.
install-report-link-absent = Няма { $link }, така че няма какво да се премахне.
install-report-package = Командата schemaic в { $dir } е дошла с инсталирания пакет, а не от „Инсталиране“ — премахнете пакета, за да я премахнете.
install-hint-adds-folder = Стартиране на Schemaic по име от терминал. Добавя папката му в потребителската ви PATH.
install-hint-links = Стартиране на Schemaic по име от терминал. Създава връзка в ~/.local/bin.
install-hint-folder-on-path = Стартиране на Schemaic по име от терминал. Папката му, { $dir }, вече е в потребителската ви PATH.
install-hint-comes-with-app = Стартиране на Schemaic по име от терминал. Командата schemaic идва с приложението, в { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

tunnel-still-opening = SSH тунелът все още не е установен
tunnel-could-not-open = SSH тунелът не можа да бъде отворен: { $error }. Опреснете схемата, за да опитате отново.
tunnel-server-failed = Неуспешно свързване през SSH тунела: { $error }. Опреснете схемата, за да опитате отново.

gate-plan-not-connected = Няма връзка — планът не беше изпълнен. Сървърът не отговори на проверката на състоянието, така че нищо не му беше изпратено.
gate-plan-connection-changed = Сменихте връзката, докато Schemaic проверяваше дали е достъпна, така че планът не беше изпълнен. Върнете се и опитайте отново.
gate-plan-tab-moved-on = Сменихте раздела, докато връзката се проверяваше отново, така че планът не беше изпълнен. Той описва раздела, от който е поискан — върнете се в този раздел и опитайте отново.
gate-connection-changed = Не беше изпълнено срещу { $name } — сменихте връзката, докато Schemaic проверяваше дали е достъпна. Върнете се и опитайте отново.
gate-not-connected = Няма връзка с { $name }. Сървърът не отговори — проверете дали работи и дали настройките на тази връзка са правилни.
gate-this-connection = тази връзка
gate-run-tab-moved-on = Сменихте раздела, докато връзката се проверяваше отново, така че изразът не беше изпълнен. Той е проверен спрямо раздела, в който е написан, и изпълнението му тук би го изпълнило другаде. Върнете се в този раздел и го изпълнете отново.
gate-commit-not-connected = Няма връзка — потвърждаването не беше опитано. Подготвените промени са запазени.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

term-shell-failed = { $program } не можа да бъде стартиран: { $error }. Изберете друга обвивка в Настройки, Терминал.
term-cli-read-only = Тази връзка е само за четене в Schemaic. Клиентът ѝ за команден ред не би бил, затова не се отваря тук.
backup-read-only-server = Тази връзка е само за четене в Schemaic, а резервното копие на тази система се записва от сървъра, затова не е направено.
backup-dialog-title = Резервно копие на базата данни
backup-kind-pg-dump = Дъмп на PostgreSQL в custom формат
backup-kind-sql = SQL скрипт
backup-kind-sqlite = База данни на SQLite
backup-not-started = Архивирането не започна: { $error }
backup-done = Резервното копие е записано в { $path }.
backup-failed = Архивирането е неуспешно: { $error }
restore-title = Възстановяване от файл
restore-confirm = Да се изпълни ли { $path } в { $database } на { $connection }? Каквото създаде или замени, не може да бъде отменено. Изпълнява се в терминала, където Ctrl+C го спира.
restore-read-only = Тази връзка е само за четене в Schemaic, така че нищо не се възстановява в нея.
restore-engine-not-here = Резервните копия на тази система не се възстановяват оттук.
restore-unreadable = { $path } не можа да бъде прочетен преди възстановяването, така че нищо не е изпълнено: { $error }
restore-refused = { $path } превключва към друга база данни по време на изпълнение — { $switches } — така че при възстановяване в { $database } ще пише там вместо това. Нищо не е изпълнено. Възстановете го в базата данни, която посочва, или премахнете тези редове от копие на файла.
restore-refused-more = { $path } превключва към друга база данни по време на изпълнение — { $switches } и още { $more } — така че при възстановяване в { $database } ще пише там вместо това. Нищо не е изпълнено. Възстановете го в базата данни, която посочва, или премахнете тези редове от копие на файла.
restore-switch = { $how } `{ $name }` на ред { $line }
restore-switch-unnamed = { $how } на ред { $line }, който не посочва база данни, която Schemaic може да прочете

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Затваряне на „{ $name }“
tab-close-unsaved = „{ $name }“ има незапазени промени. Затварянето ги отхвърля; файлът на диска остава непроменен. Да се затвори ли въпреки това?
tab-close-all-title = Затваряне на всички раздели
tab-close-all-question = Наистина ли искате да затворите всички раздели?
tab-close-others-title = Затваряне на другите раздели
tab-close-others-question = Наистина ли искате да затворите всички други раздели?
file-open-title = Отваряне на SQL файл
file-save-title = Запазване на SQL файл
file-sql-filter = SQL скрипт
file-read-failed = { $path } не можа да бъде прочетен: { $error }
file-save-failed = { $path } не можа да бъде запазен: { $error }
file-too-large = { $path } е { $size } — твърде голям за отваряне в раздел на редактора. Schemaic би прекарвал повечето си време в повторния му анализ. Изпълнете го от раздел за заявки или използвайте „Импортиране“ за файл с данни.
file-too-large-short = Файлът е твърде голям за отваряне.
file-large-title = Да се отвори ли голям файл?
file-large-question = „{ $name }“ е { $size }. Schemaic анализира отново целия документ малко след всяка пауза в писането, така че файл с такъв размер забавя реакцията на редактора, докато разделът е отворен. Да се отвори ли въпреки това?
file-changed-on-disk = { $path } е променен на диска, откакто е отворен. Запазването сега би отхвърлило тези промени — вместо това презаредете файла (или използвайте „Запазване като“ с друго име).
file-lossy-title = Да се заменят ли нечетимите байтове?
file-lossy-question = Schemaic не можа да прочете всеки байт на „{ $name }“ като текст и показа тези байтове като „�“. Запазването записва това, което виждате, така че всеки от тях се заменя завинаги — в целия файл, а не само в редактираните редове. Да се запази ли въпреки това?
file-replace-title = Да се замени ли файлът?
file-replace-question = „{ $name }“ вече съществува — „{ $typed }“ беше запазен с добавено разширение .sql. Да се замени ли?
file-reload-title = Презареждане от диска
file-reload-unsaved = „{ $name }“ има незапазени промени. Презареждането ги отхвърля. Да се презареди ли въпреки това?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = Вече се изпълнява експорт. Прекратете го или изчакайте да приключи.
export-failed = Експортът е неуспешен: { $error }
export-worker-died = Експортът е неуспешен: работният процес спря: { $error }
export-not-published = Експортът е неуспешен: { $path } не можа да бъде публикуван: { $error }
export-not-renamed = Експортът записа { $part }, но не можа да го преименува на { $path }: { $error }
export-nothing-matched = Няма нищо за експортиране — нито една таблица не съвпада с избора.
export-stopped = Експортът е прекратен.
dump-failed = Дъмпът е неуспешен: { $error }
blob-load-too-large = Този файл е { $size } — максимумът, който може да бъде зареден, е { $most }.
blob-load-failed = Зареждането е неуспешно: { $error }
blob-save-failed = Запазването е неуспешно: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = { $path } не можа да бъде записан: { $error }
settings-export-title = Експортиране на настройките
settings-import-title = Импортиране на настройките
settings-file-filter = Настройки на Schemaic
settings-import-nothing = Няма нищо за импортиране: всяка настройка в { $path } вече съвпада с вашата и файлът не добавя фрагменти.
settings-import-question = { $path } променя { $changed ->
        [one] { $changed } настройка
       *[other] { $changed } настройки
    } и добавя { $snippets ->
        [one] { $snippets } фрагмент
       *[other] { $snippets } фрагмента
    }. Променените настройки заменят текущите ви стойности; фрагментите се добавят до вашите.
settings-import-unreadable = не можа да бъде прочетен: { $error }
connections-export-title = Експортиране на връзки
connections-file-filter = Връзки на Schemaic
connections-replace-title = Замяна на „{ $file }“
connections-replace-question = В тази папка вече има файл с име „{ $file }“. Експортът добавя това окончание към въведеното от вас име, затова диалогът за запазване не попита за него. Замяната не може да бъде отменена.
import-choose-file-title = Изберете файл с връзки
import-no-containers = Не работят контейнери с бази данни.
import-added = { $count ->
        [one] Добавена е { $count } връзка.
       *[other] Добавени са { $count } връзки.
    }
import-added-foreign = { $count ->
        [one] Добавена е { $count } връзка.
       *[other] Добавени са { $count } връзки.
    } „Само за четене“ и етикетът за среда не се пренасят от друг клиент — задайте ги в „Управление на връзките“ за всичко, което е важно.
conn-delete-title = Изтриване на „{ $name }“
conn-delete-question = Това изтрива връзката, запазената ѝ парола, запазения разговор с ИИ, историята на заявките и разделите ѝ. Не може да бъде отменено.
conn-delete-keyring-kept = Schemaic не можа да достигне хранилището за ключове на ОС, така че запазената парола и SSH тайните на изтритата връзка **все още са в него**.
conn-delete-keyring-later = Те ще бъдат премахнати следващия път, когато връзка бъде изтрита или запазена, докато хранилището за ключове е достъпно. Междувременно никоя нова връзка не може да ги получи — идентификаторите на връзките никога не се използват повторно.

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

notice-unreadable = { $name } не можа да бъде прочетен ({ $error }).
notice-unreadable-kept = Нечетимият файл е запазен като { $name }.corrupt; Schemaic използва резервното му копие или стойностите по подразбиране, ако и резервното копие е нечетимо.
notice-unreadable-swept = Това копие може да съдържа пароли като некриптиран текст, затова ще бъде премахнато следващия път, когато изтриете връзка — дотогава възстановете от него всичко, което ви е нужно.
notice-missing = { $name } липсваше.
notice-missing-restored = Schemaic го възстанови от { $name }{ $from }, който все още беше там. Той ще бъде записан обратно следващия път, когато тази настройка се промени — дотогава съседното копие е единственото.
notice-keyring-unread = { $n ->
    [one] Schemaic не можа да прочете { $n } запазена тайна от хранилището за ключове на ОС.
   *[other] Schemaic не можа да прочете { $n } запазени тайни от хранилището за ключове на ОС.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic не можа да прочете { $n } запазена тайна от хранилището за ключове на ОС ({ $error }).
   *[other] Schemaic не можа да прочете { $n } запазени тайни от хранилището за ключове на ОС ({ $error }).
}
notice-keyring-unread-kept = Тези пароли не са налични в тази сесия и **не** са изтрити — оставете полетата празни и те ще се върнат, щом хранилището за ключове стане достъпно. Въвеждането на нова парола записва върху съхранената тайна.
notice-in-the-clear = { $n ->
    [one] Schemaic не можа да достигне хранилището за ключове на ОС, затова { $n } парола беше запазена **като некриптиран текст** в `connections.json`.
   *[other] Schemaic не можа да достигне хранилището за ключове на ОС, затова { $n } пароли бяха запазени **като некриптиран текст** в `connections.json`.
}
notice-in-the-clear-where = Този файл е в папката, която отваря бутонът *Отваряне на папката* в Настройки → Общи, до журнала — проверете, преди да го споделите. Повторно запазване, когато хранилището за ключове е достъпно, ги премества обратно в него.
notice-undeleted = { $n ->
    [one] { $n } изчистена парола все още е в хранилището за ключове на ОС — Schemaic не можа да я изтрие.
   *[other] { $n } изчистени пароли все още са в хранилището за ключове на ОС — Schemaic не можа да ги изтрие.
}
notice-undeleted-after = Полето е празно на диска, но при следващото стартиране ще бъде попълнено отново. Запазете отново, когато хранилището за ключове е достъпно.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = Не е върнат SQL
inline-ai-not-sql = Моделът не върна SQL
inline-ai-failed = генерирането е неуспешно
ai-fill-no-value = ИИ не върна стойност.
conn-gone = връзката вече не съществува
tx-session-not-ready = връзката за транзакцията не е готова — превключете на „Автопотвърждаване“ и обратно
tx-session-open-failed = не можа да бъде отворена връзка за транзакция: { $error }

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

tx-cancelled = Изразът беше прекратен.
tx-committed-before-ddl = Транзакцията, в която се изпълни това, вече я няма: MySQL потвърждава, преди да изпълни DDL израз, така че изразите, които вече са били в нея, са постоянни и „Връщане“ няма да ги отмени.
tx-rolled-back-by-server = Транзакцията, в която се изпълни това, вече я няма: SQL Server я върна, така че всеки израз в нея от началото ѝ е отменен.
tx-doomed = Транзакцията, в която се изпълни това, вече не може да бъде потвърдена: SQL Server я отбеляза като непотвърдима, така че „Връщане“ е единственият път напред и то отменя всеки израз в нея.
tx-after-own-commit = Транзакцията, в която се изпълни това, е приключила, а то изпълни собствен COMMIT, преди да се провали, така че изпълненото преди този COMMIT най-вероятно е потвърдено, а не отменено. Проверете данните, преди да изпълнявате каквото и да е от него отново.
tx-lost-on-commit = Връзката беше изгубена при отворена транзакция. Тя беше изгубена при COMMIT, така че не е известно дали { $stmts ->
        [0] транзакцията е била потвърдена или върната
        [one] { $stmts } израз, изпълнен в нея от началото ѝ, е бил потвърден или върнат
       *[other] { $stmts } израза, изпълнени в нея от началото ѝ, са били потвърдени или върнати
    }: сървърът може да е потвърдил, преди връзката да прекъсне. Проверете данните, преди да изпълнявате каквото и да е от тях отново.
tx-lost-on-implicit-commit = Връзката беше изгубена при отворена транзакция. MySQL потвърждава отворена транзакция, преди да изпълни този израз, така че { $stmts ->
        [0] в нея все още нищо не беше изпълнено и нямаше какво да се потвърди
        [one] { $stmts } израз, изпълнен в нея от началото ѝ, най-вероятно е бил потвърден тогава и е постоянен
       *[other] { $stmts } израза, изпълнени в нея от началото ѝ, най-вероятно са били потвърдени тогава и са постоянни
    }. Проверете данните, преди да изпълнявате каквото и да е от тях отново.
tx-lost-rolled-back = Връзката беше изгубена при отворена транзакция. Сървърът връща незавършена транзакция, когато връзката ѝ приключи, така че { $stmts ->
        [0] в нея все още нищо не беше изпълнено
        [one] { $stmts } израз, изпълнен в нея от началото ѝ, е отменен
       *[other] { $stmts } израза, изпълнени в нея от началото ѝ, са отменени
    }.
tx-lost-rolled-back-mysql = Връзката беше изгубена при отворена транзакция. Сървърът връща незавършена транзакция, когато връзката ѝ приключи, така че { $stmts ->
        [0] в нея все още нищо не беше изпълнено
        [one] { $stmts } израз, изпълнен в нея от началото ѝ, е отменен — с изключение на запис в таблица, която не поддържа транзакции (MyISAM, MEMORY), който никое връщане не отменя
       *[other] { $stmts } израза, изпълнени в нея от началото ѝ, са отменени — с изключение на запис в таблица, която не поддържа транзакции (MyISAM, MEMORY), който никое връщане не отменя
    }.
tx-not-sent-timeout = Времето за изпълнение на израз изтече, докато това изпълнение все още чакаше връзката на раздела. Предишна операция по нея — потвърждаване или повторното четене след него — не беше приключила, така че изразът изобщо не беше изпратен към сървъра. Нищо не е изпълнено и транзакцията е точно там, където беше.
tx-not-sent-cancelled = Това изпълнение беше прекратено, докато все още чакаше връзката на раздела. Предишна операция по нея — потвърждаване или повторното четене след него — не беше приключила, така че изразът изобщо не беше изпратен към сървъра. Нищо не е изпълнено и транзакцията е точно там, където беше.
snippet-default-name = Фрагмент
snippet-delete-title = Изтриване на фрагмент
snippet-delete-question = Да се изтрие ли „{ $name }“? Това не може да бъде отменено.
copy-target-read-only = Целевата връзка е само за четене, така че не са копирани редове.
copy-target-gone = Целевата връзка вече я няма, така че не са копирани редове.
copy-not-started = Не са копирани редове: { $error }
copy-counter-not-moved = { $count ->
        [one] Копиран е { $count } ред, но броячът на таблицата не беше преместен след него, така че следващото вмъкване може да влезе в конфликт: { $error }
       *[other] Копирани са { $count } реда, но броячът на таблицата не беше преместен след тях, така че следващото вмъкване може да влезе в конфликт: { $error }
    }
copy-stopped = Копирането спря поради грешка: { $error }
compare-reading-failed = Четене на { $database }: { $error }
monitor-no-row-key = Тази таблица няма ключ на редовете — промените не могат да бъдат проследявани.
monitor-restarted = Колоните на таблицата се промениха, затова проследяването на промените започна отначало оттук.
timeout-cancelled-app = Прекратено: изразът се изпълняваше по-дълго от ограничението от { $timeout }. Променете го или го изключете в Настройки → Заявки.
timeout-cancelled-connection = Прекратено: изразът се изпълняваше по-дълго от ограничението от { $timeout }. Променете го или го изключете в настройките на тази връзка.
ai-chat-no-database = Базата данни не е достъпна, така че асистентът не може да бъде стартиран за тази връзка. Проверете връзката и опитайте отново.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **Асистентът не можа да стартира.** Schemaic дава на всеки процес на агент собствена частна работна папка, а нито системната временна папка, нито конфигурационната папка позволиха създаването на такава — така че няма къде да се стартира, което да не е чужда папка. Проверете дали във временната папка може да се записва и опитайте отново.
ai-no-reply-file = Не можа да бъде създаден временен файл за отговора.
ai-inline-no-opencode-config = Конфигурацията на OpenCode не можа да бъде записана, така че генерирането не беше стартирано (без нея CLI би работил с включени собствени инструменти).
ai-inline-no-copilot-home = Частната домашна папка на GitHub Copilot не можа да бъде създадена, така че генерирането не беше стартирано (без нея CLI би заредил вашата собствена конфигурация на Copilot, включително MCP сървърите и hooks).
ai-inline-no-env-writer = { $cli } е отбелязан като изолиран от средата си, но Schemaic няма начин да настрои тази среда, така че генерирането не беше стартирано.
ai-inline-no-cursor-rules = Файлът с разрешения на Cursor не можа да бъде записан, така че генерирането не беше стартирано (без него CLI записва файлове без питане).
ai-inline-no-rules-writer = { $cli } е отбелязан като ограничен чрез файл с разрешения, но Schemaic няма начин да запише такъв за него, така че генерирането не беше стартирано.
ai-no-tools = **Тази сесия няма инструменти за базата данни.** { $cause }, така че асистентът не може да прави справки — може да отговаря само от структурата на схемата, която вече е в подканата му. Затварянето и повторното отваряне на панела ще опита отново.
ai-no-endpoint-file = Schemaic не можа да създаде частния файл, който казва на асистента как да достигне базата ви данни
ai-cursor-not-approved = Schemaic не можа да одобри инструментите си за базата данни в Cursor ({ $error })
ai-antigravity-not-registered = Schemaic не можа да регистрира инструментите си за базата данни в Antigravity
ai-antigravity-shared = Друг прозорец на Schemaic вече използва Antigravity. Antigravity пази една MCP регистрация за целия компютър, която може да сочи само към една връзка, така че асистентът в този прозорец няма инструменти за базата данни
ai-antigravity-not-claimed = Schemaic не можа да заеме конфигурацията на Antigravity за тази сесия
ai-refused-no-opencode-config = Schemaic не можа да запише конфигурацията, която ограничава OpenCode, така че асистентът е изключен — стартирането му без този файл би дало на сесията обвивка. Проверете дали в папката с данни на приложението може да се записва.
ai-refused-no-copilot-home = Schemaic не можа да създаде частната домашна папка на GitHub Copilot, така че асистентът е изключен — без нея CLI би заредил вашата собствена конфигурация на Copilot, включително MCP сървърите и hooks. Проверете дали в папката с данни на приложението може да се записва.
ai-refused-no-cursor-rules = Schemaic не можа да запише файла с разрешения, който не позволява на Cursor да записва файлове и да изпълнява команди, така че асистентът е изключен — без него Cursor записва файлове без питане. Проверете дали в папката с данни на приложението може да се записва.
ai-launch-failed-turn = CLI `{ $bin }` не можа да бъде стартиран ({ $error }). Уверете се, че { $cli } е инсталиран, или посочете пътя му в Настройки → ИИ.
ai-launch-failed-session = CLI `{ $bin }` не можа да бъде стартиран ({ $error }). Уверете се, че { $cli } е инсталиран (или задайте пътя му в Настройки → ИИ).
ai-turn-ended = Ходът на { $cli } приключи неочаквано: { $error }
ai-session-ended-badly = Сесията с ИИ приключи неочаквано: { $error }
ai-session-ended = Сесията на { $cli } приключи. Попитайте отново, за да започнете нова.
ai-session-restart-failed = Сесията на { $cli } не можа да бъде рестартирана след спирането ({ $error }), така че приключи. Попитайте отново, за да започнете нова.
ai-session-not-accepting = Сесията на { $cli } вече не приема въпроси. Попитайте отново, за да започнете нова.

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = Контекстът, изпратен към { $cli }, е твърде голям за един команден ред ({ $total } { $unit }; тази платформа позволява около { $limit }). { $levers }
ai-unit-characters = знака
ai-unit-bytes = байта
ai-levers-schema-and-query = Стеснете Настройки → ИИ → Контекст от схемата до активната база данни (или „Без“) или съкратете заявката в редактора.
ai-levers-sample = Намалете достъпа на ИИ до данни за тази връзка под „Да чете данни“, което спира изпращането на примерните редове, или стеснете Настройки → ИИ → Контекст от схемата.
ai-batch-shim = { $cli } сочи към пакетна обвивка ({ $bin }), на която не може да се подаде многоредова подкана — Windows изпълнява .cmd чрез cmd.exe и той отказва аргумента, преди { $cli } да бъде стартиран. Задайте в Настройки → ИИ → Път до CLI истинския изпълним файл (инсталация чрез npm обикновено има такъв в node_modules, до обвивката).
ai-cli-exited = CLI { $cli } завърши със статус { $code }
ai-cli-signalled = CLI { $cli } беше прекратен от сигнал
ai-constraint-sandboxed = { $cli } работи в пясъчник: собствените му вградени инструменти не могат да записват файлове или да изпълняват команди, но все още могат да четат този компютър.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Всички MCP сървъри, които сте регистрирали в него, също са достъпни за тази сесия и Schemaic не може да ги ограничи.
    }
ai-constraint-workspace-rules = { $cli } е ограничен до отговори чрез файл с разрешения, който Schemaic записва за тази сесия, а не чрез пясъчник: на собствените му инструменти е забранено да записват файлове и да изпълняват команди, но те все още могат да четат файлове на този компютър.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}MCP сървърите, които сте регистрирали в него, също стартират с тази сесия; инструментите им се отказват, освен ако собствените ви настройки не ги позволяват.
    }
ai-constraint-denylist = Тази версия на { $cli } не приема флага, който изпразва вградените ѝ инструменти, така че те се ограничават чрез списък със забрани — по-слаба защита, която Schemaic не може да гарантира. Обновяването на CLI възстановява пълната изолация.
ai-constraint-unrestricted = Schemaic не можа да ограничи напълно вградените инструменти на { $cli } за тази сесия, така че той може да прави повече от това да отговаря на въпроси за тази база данни.
ai-constraint-unknown = Schemaic не можа да потвърди, че този изпълним файл на { $cli } приема флаговете, които го ограничават, така че асистентът е изключен. Проверете пътя в Настройки → ИИ.
ai-seed-empty = моделът не върна съдържание
ai-seed-not-json = отговорът не беше валиден JSON: { $error }
ai-seed-not-rows = отговорът не беше JSON масив от обекти за редове

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } не е файл.
import-source-too-big = { $path } е { $size }, повече от { $most }, които се четат.
import-source-unreadable = { $path } не можа да бъде прочетен: { $error }
docker-not-found = Docker не е намерен — нито в PATH, нито в WSL.
docker-not-started = Docker не можа да бъде стартиран: { $error }
docker-timed-out = Docker не отговори в рамките на { $secs } секунди.
docker-not-waited = Не можа да се изчака Docker: { $error }
docker-answered = Docker отговори: { $error }
docker-exited = Docker завърши с { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

script-reader-stopped = четецът на файла спря неочаквано: { $error }
script-changed-since-read = Този файл е променен на диска, след като е прочетен, така че обобщението по-горе не е това, което би се изпълнило. Изберете го отново.
script-no-terminator = Този файл не съдържа знак за край на израз, така че не е SQL скрипт, който Schemaic може да изпълни. Проверете дали сте избрали правилния файл.
ai-cursor-enable-unfinished = `mcp enable` не завърши
