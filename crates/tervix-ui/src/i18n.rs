//! UI-only session language. Raw terminal output and external identities never enter the catalog.
use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Language {
    #[default]
    Ru,
    En,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ru => "RU · Русский",
            Self::En => "EN · English",
        })
    }
}

macro_rules! catalog {
    ($($key:ident => ($en:literal, $ru:literal)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Key { $($key),+ }
        impl Language {
            pub(crate) fn text(self, key: Key) -> &'static str {
                match (self, key) { $((Self::En, Key::$key) => $en, (Self::Ru, Key::$key) => $ru),+ }
            }
        }
        #[cfg(test)] const KEYS: &[Key] = &[$(Key::$key),+];
    };
}

catalog! {
    DockHandle => ("Drag pane", "Перенести панель"),
    DockHint => ("Release to dock · Esc cancels", "Отпустите для переноса · Esc — отмена"),
    DockInvalid => ("Drag onto another pane: center swaps, edges dock · Esc cancels", "Перетащите на другую панель: центр — обмен, край — разделение · Esc — отмена"),
    DockSwap => ("Swap panes", "Обмен панелями"),
    DockLeft => ("Dock left", "Разместить слева"),
    DockRight => ("Dock right", "Разместить справа"),
    DockUp => ("Dock above", "Разместить сверху"),
    DockDown => ("Dock below", "Разместить снизу"),
    NewTerminal => ("New terminal", "Новый терминал"),
    NewTerminalHint => ("New terminal pane · Ctrl+Alt+N", "Новая панель терминала · Ctrl+Alt+N"),
    LauncherPlacement => ("Create a separate pane next to the focused pane:", "Создать отдельную панель рядом с активной:"),
    LauncherRight => ("To the right", "Справа"),
    LauncherBelow => ("Below", "Снизу"),
    LauncherKeys => ("Up/Down or Tab selects · Left/Right changes split · Enter opens · F5 refreshes WSL · Esc cancels", "Вверх/вниз или Tab — выбор · Влево/вправо — разделение · Enter — запуск · F5 — обновить WSL · Esc — отмена"),
    LauncherPending => ("Confirm or cancel the pending action before opening a terminal", "Подтвердите или отмените текущее действие перед открытием терминала"),
    LauncherFailed => ("Cannot create terminal pane: {0}", "Не удалось создать панель терминала: {0}"),
    LauncherCreated => ("New pane created; existing terminal sessions preserved", "Создана новая панель; существующие сессии терминалов сохранены"),
    LauncherRefreshWsl => ("Refresh WSL distributions", "Обновить дистрибутивы WSL"),
    LanguageLabel => ("Language · session only", "Язык · только в этой сессии"),
    ConfigureOn => ("Done · Ctrl+Alt+E", "Готово · Ctrl+Alt+E"),
    Configure => ("Edit layout · Ctrl+Alt+E", "Изменить раскладку · Ctrl+Alt+E"),
    ConfigureCompact => ("Edit layout", "Изменить раскладку"),
    ConfigureCompactOn => ("Done", "Готово"),
    General => ("General / language", "Основные / язык"),
    ToolCategory => ("Tools", "Инструменты"),
    TerminalCategory => ("Terminals", "Терминалы"),
    ConnectionSetup => ("Connection setup", "Подключение"),
    DockerConnectedCompact => ("Engine connected", "Engine подключён"),
    CopyCompact => ("Copy", "Копия"),
    SshExited => ("SSH exited with code {0}. Connect explicitly to start a new session.", "SSH завершён с кодом {0}. Для новой сессии нажмите Подключить."),
    CopyFilteredHelp => ("Copy filtered bounded history", "Копировать отфильтрованную ограниченную историю"),
    LogSummaryCompact => ("{0} {1} · {2} entries · {3} lost · {4} cut", "{0} {1} · {2} записей · {3} потеряно · {4} обрезано"),
    Back => ("Back", "Назад"),
    Actions => ("Actions", "Действия"),
    ReplaceWsl => ("Start / Replace", "Запустить / заменить"),
    ConfirmWsl => ("Replace this WSL session? The active shell will be closed. Enter confirms; Escape cancels.", "Заменить эту сессию WSL? Активная оболочка будет закрыта. Enter — подтвердить; Escape — отменить."),
    Undo => ("Undo", "Отменить"),
    RestoreTiles => ("Restore tiles", "Восстановить панели"),
    RestoreTilesCompact => ("Restore tiles", "Вернуть панели"),
    EditHint => ("EDIT / Drag the header handle · split · resize", "РЕДАКТОР / Перенос за значок заголовка · разделение · размер"),
    WorkspaceHint => ("WORKSPACE / Edit layout to arrange panes · Arrows navigate", "РАБОЧАЯ ОБЛАСТЬ / Изменить раскладку — размещение панелей · Arrows — навигация"),
    CloseWorkspaceQuestion => ("Close workspace?", "Закрыть рабочую область?"),
    CloseTerminalQuestion => ("Close this terminal?", "Закрыть этот терминал?"),
    PasteQuestion => ("Paste multiline text?", "Вставить многострочный текст?"),
    CloseWorkspaceDetail => ("Local shells will be terminated. Docker exec connections detach only; their remote processes may keep running.", "Локальные оболочки будут завершены. Docker exec только отключается: удалённые процессы могут продолжить работу."),
    CloseTerminalDetail => ("Local shells terminate on close. Docker exec detaches only; use exit inside its shell to end the remote process. Undo does not restart it.", "Локальная оболочка завершится. Docker exec только отключается; для завершения удалённого процесса введите exit в оболочке. Отмена раскладки не перезапускает процесс."),
    PasteDetail => ("This text contains control characters or multiple lines.", "Текст содержит управляющие символы или несколько строк."),
    ConfirmHint => ("Enter to confirm · Escape to cancel", "Enter — подтвердить · Escape — отменить"),
    Cancel => ("Cancel", "Отмена"),
    Confirm => ("Confirm action", "Подтвердить действие"),
    CloseWorkspace => ("Close workspace", "Закрыть рабочую область"),
    TerminateShell => ("Terminate shell", "Завершить оболочку"),
    Paste => ("Paste text", "Вставить текст"),
    LayoutMeasureError => ("Layout could not be measured. Try resizing the window.", "Не удалось рассчитать раскладку. Попробуйте изменить размер окна."),
    SystemDefault => ("system default", "системный по умолчанию"),
    Focused => ("FOCUSED", "АКТИВНАЯ"),
    Pane => ("PANE", "ПАНЕЛЬ"),
    View => ("VIEW", "ВИД"),
    Close => ("Close", "Закрыть"),
    Add => ("+ Add", "+ Добавить"),
    SplitV => ("Split ↔", "Разделить ↔"),
    SplitH => ("Split ↕", "Разделить ↕"),
    Restore => ("Restore", "Восстановить"),
    Stretch => ("Stretch", "Развернуть"),
    Terminal => ("Terminal · default shell", "Терминал · оболочка по умолчанию"),
    PowerShell => ("PowerShell", "PowerShell"),
    Cmd => ("Command Prompt", "Командная строка"),
    Wsl => ("WSL · Linux shell", "WSL · оболочка Linux"),
    Ssh => ("SSH · remote shell", "SSH · удалённая оболочка"),
    DockerExec => ("Docker · exec TTY (close detaches)", "Docker · exec TTY (закрытие отключает)"),
    Monitor => ("System monitor", "Монитор системы"),
    Processes => ("Processes", "Процессы"),
    DockerContainers => ("Docker · containers", "Docker · контейнеры"),
    DockerLogs => ("Docker · live logs", "Docker · журнал"),
    DockerInspect => ("Docker · inspect / stats", "Docker · сведения / метрики"),
    Help => ("Workspace help", "Справка рабочей области"),
    Settings => ("Settings", "Настройки"),
    ExecDisconnected => ("Select a container on a connected Engine and choose Exec sh / Exec bash. Close detaches only; use exit inside the shell to terminate it.", "Выберите контейнер подключённого Engine и нажмите Exec sh / Exec bash. Закрытие только отключает; для завершения оболочки введите exit."),
    HelpTitle => ("A workspace that fits you", "Рабочая область под ваши задачи"),
    HelpIntro => ("All shortcuts below start with Ctrl+Alt and use physical key positions. Layout changes require Edit layout mode; app settings are separate.", "Все сочетания ниже начинаются с Ctrl+Alt и используют физические позиции клавиш. Для изменения панелей — Изменить раскладку; настройки приложения открываются отдельно."),
    HelpKeys => ("E   Edit layout / Done\nT   Cycle pane view (edit)\nR   Toggle reduced motion\nV / H   Split left–right / top–bottom (edit)\nArrows   Focus neighbor\nShift+Arrows   Resize split (edit)\nP   Previous focus\nF   Stretch / restore pane (edit)\nW   Close pane (edit)\nZ   Undo layout change (edit)", "E   Изменить раскладку / Готово\nT   Следующий вид панели (редактор)\nR   Уменьшить анимацию\nV / H   Разделить панель (редактор)\nArrows   Перейти к соседу\nShift+Arrows   Размер разделителя (редактор)\nP   Предыдущая панель\nF   Развернуть / восстановить (редактор)\nW   Закрыть панель (редактор)\nZ   Отменить изменение (редактор)"),
    PointerResize => ("Pointer resizing", "Изменение размера мышью"),
    HelpTools => ("Tools: Q search, S sort, O reverse order, B chart range. WSL: D drafts the next distribution, L refreshes; Start / Replace applies it. All shortcuts use Ctrl+Alt.", "Инструменты: Q — поиск, S — сортировка, O — обратный порядок, B — график. WSL: D — черновик следующего дистрибутива, L — обновить; Запустить / заменить применяет выбор. Всё с Ctrl+Alt."),
    HelpDrag => ("In Edit layout, drag the header move icon: center swaps panes, edges dock. Dividers resize nested splits. Release adds one undo step. Escape, Done, launcher, language change, leaving the canvas, focus loss or window resize cancels docking. Restore stretched tiles before docking.", "В редакторе раскладки тяните значок переноса: центр — обмен, край — размещение рядом. Разделители меняют размер вложенных панелей. Отпускание — один шаг отмены. Escape, Готово, launcher, смена языка, выход за canvas, потеря фокуса или изменение размера окна отменяют перенос. Сначала восстановите развёрнутые панели."),
    HelpOverflow => ("Small windows keep every pane reachable by scrolling. Stretch preserves the tree and view identities. Ctrl+C and Ctrl+V remain available to terminal applications.", "В маленьком окне все панели доступны прокруткой. Разворачивание сохраняет дерево и идентификаторы. Ctrl+C и Ctrl+V доступны терминальным приложениям."),
    HelpTerminal => ("Type in the focused terminal in normal mode. Ctrl+Shift+V pastes (multiline requires confirmation); Ctrl+C goes to the shell. Layouts/preferences are session-only. Native window transfer is not connected.", "В обычном режиме ввод идёт в активный терминал. Ctrl+Shift+V — вставка (многострочная с подтверждением); Ctrl+C — оболочке. Раскладка и настройки действуют только в сессии. Перенос между окнами ещё не подключён."),
    Preferences => ("Workspace preferences", "Настройки рабочей области"),
    Appearance => ("APPEARANCE & ACCESSIBILITY", "ОФОРМЛЕНИЕ И ДОСТУПНОСТЬ"),
    ReducedMotion => ("Reduced motion · Ctrl+Alt+R", "Уменьшить анимацию · Ctrl+Alt+R"),
    Solid => ("Solid background · reduce transparency", "Сплошной фон · без прозрачности"),
    TransparencyHelp => ("Translucent navy uses window alpha where the compositor supports it; no acrylic or desktop blur. Solid mode removes desktop translucency. Titlebar: drag to move, double-click to maximize; Ctrl+Alt+F9 minimize, Ctrl+Alt+F10 maximize/restore, Alt+F4 close.", "Прозрачность фона зависит от композитора; без acrylic и размытия. Сплошной фон отключает прозрачность. Заголовок: перетаскивание — перемещение, двойной щелчок — разворачивание; Ctrl+Alt+F9 — свернуть, Ctrl+Alt+F10 — развернуть/восстановить, Alt+F4 — закрыть."),
    MotionHelp => ("Disables 160 ms pane geometry and accent transitions. Pointer drags always track directly; dividers return after geometry settles. No animation frames are requested while idle.", "Отключает переходы геометрии и акцента длительностью 160 мс. Перетаскивание всегда без задержки; разделители возвращаются после перехода. В покое кадры анимации не запрашиваются."),
    SessionOnly => ("LOCAL · SESSION ONLY", "ЛОКАЛЬНО · ТОЛЬКО В СЕССИИ"),
    SessionHelp => ("Global preferences for this app session: changes apply immediately to every pane. They are not saved to disk. Use Edit layout to arrange panes.", "Общие настройки приложения на текущую сессию: изменения сразу действуют во всех панелях и не сохраняются на диск. Для размещения панелей — Изменить раскладку."),
    DeveloperWorkspace => ("DEVELOPER WORKSPACE", "РАБОЧАЯ СРЕДА РАЗРАБОТЧИКА"),
    LocalSession => ("LOCAL / SESSION", "ЛОКАЛЬНО / СЕССИЯ"),
    Minimize => ("Minimize · Ctrl+Alt+F9", "Свернуть · Ctrl+Alt+F9"),
    Maximize => ("Maximize / restore · Ctrl+Alt+F10", "Развернуть / восстановить · Ctrl+Alt+F10"),
    CloseWindow => ("Close workspace · Alt+F4", "Закрыть рабочую область · Alt+F4"),
    NativeResizeUnavailable => ("Native edge resizing is unavailable on this platform", "Изменение размера за края недоступно на этой платформе"),
    ClosingShells => ("Closing shells — waiting for terminal workers to release resources", "Закрытие оболочек — ожидание освобождения ресурсов терминалов"),
    Ready => ("Ready — split a pane to start exploring your workspace", "Готово — разделите панель и настройте рабочую область"),
    ResizeCancelled => ("Resize cancelled — original layout restored", "Изменение размера отменено — исходная раскладка восстановлена"),
    EnableEdit => ("Choose Edit layout (Ctrl+Alt+E) to change pane layout", "Нажмите Изменить раскладку (Ctrl+Alt+E) для изменения панелей"),
    EditStatus => ("Edit layout • choose pane views, split, move, resize or close • Done returns to work", "Редактор раскладки • вид, разделение, перенос, размер, закрытие • Готово — вернуться к работе"),
    NormalStatus => ("Workspace • app settings and tool connections are available without editing layout", "Рабочая область • настройки приложения и подключения доступны без редактора раскладки"),
    StretchStatus => ("Stretch changes only presentation; layout and view identities are preserved", "Разворачивание меняет только отображение; раскладка и идентификаторы сохранены"),
    EnableViewEdit => ("Choose Edit layout (Ctrl+Alt+E) to change pane views", "Нажмите Изменить раскладку (Ctrl+Alt+E) для смены вида панели"),
    LayoutUpdated => ("Layout updated • session-only changes • Ctrl+Alt+Z to undo", "Раскладка обновлена • только в этой сессии • Ctrl+Alt+Z — отменить"),
    LayoutUnchanged => ("Layout unchanged: {0}", "Раскладка не изменена: {0}"),
    ResizeUnchanged => ("Resize unchanged: {0}", "Размер не изменён: {0}"),
    Resizing => ("Resizing • release to apply • Esc to cancel", "Изменение размера • отпустите для применения • Esc — отменить"),
    ResizeApplied => ("Resize applied • Ctrl+Alt+Z to undo", "Размер изменён • Ctrl+Alt+Z — отменить"),
    ViewChanged => ("View changed • Ctrl+Alt+T cycles views", "Вид изменён • Ctrl+Alt+T — следующий вид"),
    MotionOn => ("Reduced motion on • session-only preference", "Анимация уменьшена • только в этой сессии"),
    MotionOff => ("Reduced motion off • session-only preference", "Анимация включена • только в этой сессии"),
    Error => ("Operation failed: {0}", "Ошибка операции: {0}"),
    PendingAction => ("Confirm or cancel the pending terminal action (Enter / Esc)", "Подтвердите или отмените действие терминала (Enter / Esc)"),
    PasteTargetChanged => ("Paste cancelled: terminal target changed or an action is pending", "Вставка отменена: терминал изменился или ожидает подтверждения"),
    PasteTooLarge => ("Paste rejected: maximum 64 KiB", "Вставка отклонена: максимум 64 КиБ"),
    PasteEnded => ("Paste cancelled: original terminal session ended", "Вставка отменена: исходная сессия терминала завершена"),
    NoTerminal => ("Terminal provider not attached", "Провайдер терминала не подключён"),
    Starting => ("Starting shell…", "Запуск оболочки…"),
    Closing => ("Closing terminal transport…", "Закрытие соединения терминала…"),
    RestartTerminal => ("Restart terminal", "Перезапустить терминал"),
    ExecDetach => ("Engine exec TTY · close detaches only · use exit to end shell", "Engine exec TTY · закрытие только отключает · exit завершает оболочку"),
    Exited => ("Exited ({0}) — layout undo does not restart shells", "Завершён ({0}) — отмена раскладки не перезапускает оболочки"),
    Closed => ("Closed — layout undo does not restart shells", "Закрыт — отмена раскладки не перезапускает оболочки"),
    Running => ("Running", "Работает"),
    Failed => ("Terminal failed: {0}", "Ошибка терминала: {0}"),
    WslChoose => ("Choose a WSL pane before selecting a distribution", "Выберите панель WSL перед выбором дистрибутива"),
    WslNotReady => ("WSL discovery is not ready · Ctrl+Alt+L refreshes", "Список WSL не готов · Ctrl+Alt+L — обновить"),
    WslReplaced => ("WSL profile replaced", "Профиль WSL заменён"),
    WslDefault => ("System default distribution", "Дистрибутив системы по умолчанию"),
    WslEmpty => ("No installed distributions", "Установленных дистрибутивов нет"),
    WslInstalled => ("Installed: {0} · choose, then Start / Replace", "Установлено: {0} · выберите и нажмите Запустить / заменить"),
    WslDiscover => ("Discover distributions", "Найти дистрибутивы"),
    WslLoading => ("Discovering WSL…", "Поиск дистрибутивов WSL…"),
    WslUnavailable => ("WSL is unavailable", "WSL недоступна"),
    DiscoveryError => ("Discovery failed: {0}", "Ошибка обнаружения: {0}"),
    DiscoveryUnavailable => ("Discovery unavailable", "Обнаружение недоступно"),
    Refresh => ("Refresh", "Обновить"),
    SshConnecting => ("SSH connecting · SSH handles authentication and host verification", "Подключение SSH · аутентификацию и проверку сервера выполняет SSH"),
    SshActive => ("Exit the active session before connecting again", "Завершите активную сессию перед повторным подключением"),
    SshHost => ("SSH host / config alias · Ctrl+Alt+Q", "SSH адрес / псевдоним из config · Ctrl+Alt+Q"),
    SshHostPlaceholder => ("dev-alias or host.example", "dev-alias или host.example"),
    SshUser => ("Username · optional, inherits SSH config", "Пользователь · необязательно, из SSH config"),
    SshConfigDefault => ("SSH config default", "По умолчанию из SSH config"),
    SshPort => ("Port · optional, 1–65535", "Порт · необязательно, 1–65535"),
    ConnectSsh => ("Connect SSH", "Подключить SSH"),
    SshHelp => ("Tab / Shift+Tab fields · Enter connects. Uses Windows ssh.exe and existing ~/.ssh/config, agent and known_hosts. No credentials stored.", "Tab / Shift+Tab — поля · Enter — подключить. Используются Windows ssh.exe, ~/.ssh/config, агент и known_hosts. Учётные данные не сохраняются."),
    SshHostError => ("Host must be a literal SSH alias, DNS name, IPv4 or IPv6 address (maximum 253 characters)", "Укажите SSH-псевдоним, DNS-имя, IPv4 или IPv6 (не более 253 символов)"),
    SshUserError => ("Username must start with a letter, digit or underscore and contain only letters, digits, underscore, dot or hyphen (maximum 64 characters)", "Имя пользователя: буквы, цифры, подчёркивание, точка или дефис; первый символ — буква, цифра или подчёркивание (до 64 символов)"),
    SshPortError => ("Port must be empty or a decimal number from 1 to 65535", "Оставьте порт пустым или введите целое число от 1 до 65535"),
    Name => ("Name", "Имя"),
    Memory => ("Memory", "Память"),
    Uptime => ("Uptime", "Время работы"),
    TelemetryPending => ("Telemetry pending", "Ожидание метрик"),
    TelemetryUnavailable => ("Telemetry unavailable", "Метрики недоступны"),
    Stale => ("Stale", "Устарело"),
    Filter => ("Filter", "Фильтр"),
    TelemetryConnecting => ("Connecting to system telemetry…", "Подключение к системным метрикам…"),
    TelemetryError => ("Telemetry unavailable · {0}", "Метрики недоступны · {0}"),
    FreshSample => ("Waiting for a fresh sample…", "Ожидание свежих данных…"),
    Cadence => ("LIVE · {0}s cadence · {1}s ago", "ОНЛАЙН · интервал {0} с · {1} с назад"),
    MetricsHelp => ("Metrics will appear after the first successful observation. CPU stays N/A until a usable interval baseline is available.", "Метрики появятся после первого успешного измерения. CPU — нет данных до получения базового интервала."),
    ProcessSearch => ("Search name, PID or executable path…", "Поиск по имени, PID или пути программы…"),
    Ascending => ("Ascending ↑", "По возрастанию ↑"),
    Descending => ("Descending ↓", "По убыванию ↓"),
    ProcessDetail => ("{0} · start {1} · {2} · private {3}\n{4}", "{0} · запуск {1} · {2} · частная память {3}\n{4}"),
    SystemOverview => ("System overview", "Обзор системы"),
    SharedSampler => ("One shared sampler · whole-machine utilization", "Общий сборщик · загрузка всей системы"),
    CpuMachine => ("CPU · WHOLE MACHINE", "CPU · ВСЯ СИСТЕМА"),
    LogicalCores => ("Logical cores: {0}", "Логических ядер: {0}"),
    MemoryInUse => ("MEMORY · IN USE", "ПАМЯТЬ · ИСПОЛЬЗУЕТСЯ"),
    MemoryAvailable => ("{0} available / {1} total", "{0} доступно / {1} всего"),
    PhysicalMemory => ("PHYSICAL MEMORY", "ФИЗИЧЕСКАЯ ПАМЯТЬ"),
    MemoryUsedAvailable => ("In use {0} · Available {1}", "Используется {0} · Доступно {1}"),
    Utilization => ("UTILIZATION", "ЗАГРУЗКА"),
    Seconds60 => ("60 s", "60 с"),
    Minutes5 => ("5 min", "5 мин"),
    MemoryLegend => ("● Memory in use", "● Используется память"),
    HistoryCaption => ("{0}s rolling window · {1} observations · gaps = unavailable", "Окно {0} с · измерений: {1} · пропуски = нет данных"),
    LogicalProcessors => ("LOGICAL PROCESSORS", "ЛОГИЧЕСКИЕ ПРОЦЕССОРЫ"),
    Cores => ("Cores: {0}", "Ядер: {0}"),
    Pagefile => ("PAGEFILE / SWAP", "ФАЙЛ ПОДКАЧКИ / SWAP"),
    Configured => ("{0} configured", "Выделено: {0}"),
    ReadOnly => ("Read-only · no elevation", "Только чтение · без повышения прав"),
    Now => ("now", "сейчас"),
    SecondUnit => ("s", "с"),
    GiB => ("GiB", "ГиБ"),
    MiB => ("MiB", "МиБ"),
    NotAvailable => ("N/A", "Нет данных"),
    HoursMinutes => ("{0}h {1}m", "{0} ч {1} мин"),
    DockerUnavailable => ("Docker service is unavailable", "Сервис Docker недоступен"),
    DockerPreparing => ("Preparing Docker pane…", "Подготовка панели Docker…"),
    DockerDisconnected => ("DISCONNECTED · explicit Connect required", "ОТКЛЮЧЕНО · нажмите «Подключить»"),
    DockerConnecting => ("CONNECTING · negotiating Engine API…", "ПОДКЛЮЧЕНИЕ · согласование Engine API…"),
    DockerConnected => ("CONNECTED · containers: {0} · {1}", "ПОДКЛЮЧЕНО · контейнеров: {0} · {1}"),
    DockerRetrying => ("RECONNECTING · {0} · retained rows are STALE · bounded backoff", "ПЕРЕПОДКЛЮЧЕНИЕ · {0} · строки УСТАРЕЛИ · ограниченная задержка"),
    Connect => ("Connect", "Подключить"),
    Disconnect => ("Disconnect", "Отключить"),
    DiscoverContexts => ("Discover contexts", "Найти контексты"),
    DockerLoading => ("Loading from Engine… · request has a bounded deadline", "Загрузка из Engine… · время ожидания ограничено"),
    UnknownDocker => ("Unknown Docker pane", "Неизвестная панель Docker"),
    ConfirmDocker => ("Confirm action: {0}?", "Подтвердить действие: {0}?"),
    Engine => ("Engine: {0}", "Движок: {0}"),
    DockerConfirmDetail => ("This changes the selected container. Remove never forces or deletes volumes. The captured endpoint and full identity are checked again before dispatch.", "Действие изменит выбранный контейнер. Удаление не принудительное и не удаляет тома. Перед выполнением повторно проверяются адрес движка и полный идентификатор."),
    Start => ("Start", "Запустить"),
    Stop => ("Stop", "Остановить"),
    Restart => ("Restart", "Перезапустить"),
    Pause => ("Pause", "Приостановить"),
    Unpause => ("Unpause", "Возобновить"),
    Remove => ("Remove", "Удалить"),
    DockerKeys => ("↑/↓ or j/k select · l / Enter logs · i inspect · Q search / S sort (Ctrl+Alt)\nc connect · x disconnect · s start · t stop · Shift+R restart · p pause · u unpause · d remove (confirm)", "↑/↓ или j/k — выбор · l / Enter — журнал · i — сведения · Q — поиск / S — сортировка (Ctrl+Alt)\nc — подключить · x — отключить · s — запуск · t — стоп · Shift+R — перезапуск · p — пауза · u — продолжить · d — удалить (подтверждение)"),
    DockerSearch => ("Search name, image, status, ports…", "Поиск по имени, образу, состоянию, портам…"),
    Sort => ("Sort", "Сортировка"),
    Status => ("Status", "Состояние"),
    Image => ("Image", "Образ"),
    Ports => ("Ports", "Порты"),
    OpenLogs => ("Logs", "Журнал"),
    OpenInspect => ("Details", "Сведения"),
    ExecSh => ("Terminal", "Терминал"),
    ExecBash => ("Terminal · /bin/bash", "Терминал · /bin/bash"),
    NoContainers => ("No matching containers (running and stopped are included).", "Контейнеры не найдены (учитываются запущенные и остановленные)."),
    NoDockerData => ("No live data. Select the actual host engine and Connect. Docker Desktop is never started automatically.", "Нет актуальных данных. Выберите движок хоста и подключитесь. Docker Desktop не запускается автоматически."),
    SelectLogs => ("Select a container in Docker Containers and choose Open logs. Each new pane retains its own container identity.", "Выберите контейнер в панели Docker и откройте журнал. Каждая новая панель хранит свой идентификатор контейнера."),
    Resume => ("Resume", "Продолжить"),
    Resubscribe => ("Resubscribe", "Переподписаться"),
    Follow => ("Follow newest", "Следить за новыми"),
    LogFilter => ("Filter log text (case-sensitive)", "Фильтр журнала (с учётом регистра)"),
    AllChannels => ("All", "Все потоки"),
    Combined => ("Combined", "Объединённый"),
    Frozen => (" · PAUSED (frozen frame)", " · ПАУЗА (кадр заморожен)"),
    Truncated => (" [truncated]", " [обрезано]"),
    LogWaiting => ("Waiting for log subscription. Reopening after close/undo requires Resubscribe; no live session is restored.", "Ожидание подписки на журнал. После закрытия/отмены нажмите «Переподписаться»; живая сессия не восстанавливается."),
    SelectInspect => ("Select a container in Docker Containers and choose Inspect / stats.", "Выберите контейнер в панели Docker и откройте сведения / метрики."),
    RefreshInspect => ("Refresh inspect / stats", "Обновить сведения / метрики"),
    InspectPrivacy => ("Allowlisted details only · no environment, commands, labels or mounts", "Только разрешённые сведения · без окружения, команд, меток и подключений"),
    InspectDetails => ("Name: {0}\nImage: {1}\nPlatform: {2}\nRunning: {3} · Paused: {4}\nExit: {5}\nStarted: {6}\nFinished: {7}", "Имя: {0}\nОбраз: {1}\nПлатформа: {2}\nРаботает: {3} · Пауза: {4}\nКод выхода: {5}\nЗапуск: {6}\nЗавершение: {7}"),
    InspectStats => ("CPU: {0} (100% = one CPU)\nMemory: {1} / {2} bytes (raw, includes cache)\nObserved: {3}", "CPU: {0} (100% = один CPU)\nПамять: {1} / {2} байт (включая кэш)\nИзмерено: {3}"),
    StatsUnavailable => ("Stats: N/A · use Refresh to collect a real sample", "Метрики: нет данных · нажмите «Обновить» для измерения"),
    DetailsUnavailable => ("Details / CPU / memory: N/A · disconnected or stale endpoint", "Сведения / CPU / память: нет данных · отключённый или устаревший движок"),
    Yes => ("yes", "да"),
    No => ("no", "нет"),
    LocalEngine => ("Local host · docker_engine", "Локальный хост · docker_engine"),
    DesktopEngine => ("Docker Desktop · Linux host engine", "Docker Desktop · движок Linux хоста"),
    IndependentWsl => ("Independent WSL daemon · unsupported (Unix socket needs secure bridge)", "Отдельный демон WSL · не поддерживается (Unix-сокету нужен защищённый мост)"),
    DockerInitial => ("Disconnected · choose the actual host engine, then Connect. Context discovery does not connect.", "Отключено · выберите движок хоста и подключитесь. Поиск контекстов не подключает."),
    EndpointChanged => ("Endpoint changed. Open this container again from the connected table.", "Движок изменился. Откройте контейнер заново из подключённой таблицы."),
    ContextsDiscovered => ("Contexts discovered. Choose one and explicitly Connect.", "Контексты найдены. Выберите один и явно подключитесь."),
    ActionCompleted => ("Action completed · table reconciles automatically", "Действие выполнено · таблица обновляется автоматически"),
    ActionError => ("{0} · action is never replayed automatically", "{0} · действие не повторяется автоматически"),
    StaleTarget => ("Stale target rejected. Select the container again.", "Устаревшая цель отклонена. Выберите контейнер заново."),
    ActionWait => ("An action is pending; wait for its outcome.", "Действие выполняется; дождитесь результата."),
    ActionPending => ("Action pending…", "Выполнение действия…"),
    ContextsLoading => ("Discovering contexts…", "Поиск контекстов…"),
    ConnectingHost => ("Connecting to explicitly selected host engine…", "Подключение к выбранному движку хоста…"),
    UnsupportedTransport => ("Unsupported transport. Independent WSL dockerd requires a secure explicit bridge; this is not the Desktop host engine.", "Транспорт не поддерживается. Для отдельного dockerd в WSL нужен явно настроенный защищённый мост; это не движок Docker Desktop."),
    DisconnectedReaders => ("Disconnected. Log readers cancelled.", "Отключено. Чтение журналов отменено."),
    ExecStarted => ("Exec TTY started. Close detaches only; use exit inside the shell to terminate it. Undo never restarts exec.", "Exec TTY запущен. Закрытие только отключает; exit в оболочке завершает её. Отмена не перезапускает exec."),
    SelectContainer => ("Select a container on a connected engine first.", "Сначала выберите контейнер подключённого движка."),
    ContainerAbsent => ("Container target is absent or stale; open it from the current engine table.", "Контейнер отсутствует или устарел; откройте его из таблицы текущего движка."),
    ActionCancelled => ("Action cancelled.", "Действие отменено."),
    Context => ("Context: {0}{1}{2}", "Контекст: {0}{1}{2}"),
    LogStreaming => ("Streaming", "Чтение"),
    LogEnded => ("Ended", "Завершено"),
    LogCancelled => ("Cancelled", "Отменено"),
    SshSession => ("SSH session", "Сессия SSH"),
    SshDraft => ("SSH draft: {0} | user: {1} | port: {2}", "Черновик SSH: {0} | пользователь: {1} | порт: {2}"),
    SshNotConnected => ("SSH disconnected", "SSH отключён"),
}

impl Language {
    // Replace numbered slots in one pass: external values containing `{1}` are never reprocessed.
    pub(crate) fn format(self, key: Key, args: &[&str]) -> String {
        let template = self.text(key);
        let mut result = String::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            result.push_str(&rest[..start]);
            let tail = &rest[start + 1..];
            if let Some(end) = tail.find('}')
                && let Ok(index) = tail[..end].parse::<usize>()
                && let Some(value) = args.get(index)
            {
                result.push_str(value);
                rest = &tail[end + 1..];
            } else {
                result.push('{');
                rest = tail;
            }
        }
        result.push_str(rest);
        result
    }

    pub(crate) fn count(self, count: usize, noun: Noun) -> String {
        let forms = match (self, noun) {
            (Self::Ru, Noun::Pane) => ["панель", "панели", "панелей"],
            (Self::Ru, Noun::Process) => ["процесс", "процесса", "процессов"],
            (Self::En, Noun::Pane) => ["pane", "panes", "panes"],
            (Self::En, Noun::Process) => ["process", "processes", "processes"],
        };
        let index = match self {
            Self::En => usize::from(count != 1),
            Self::Ru if (11..=14).contains(&(count % 100)) => 2,
            Self::Ru => match count % 10 {
                1 => 0,
                2..=4 => 1,
                _ => 2,
            },
        };
        format!("{count} {}", forms[index])
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Noun {
    Pane,
    Process,
}

/// Stored UI notification; translation is deferred until view so toggles update existing notices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notice {
    key: Key,
    args: Vec<String>,
}
impl From<Key> for Notice {
    fn from(key: Key) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }
}
impl From<String> for Notice {
    fn from(detail: String) -> Self {
        Key::Error.with([detail])
    }
}
impl Key {
    pub(crate) fn with(self, args: impl IntoIterator<Item = String>) -> Notice {
        Notice {
            key: self,
            args: args.into_iter().collect(),
        }
    }
}
impl Notice {
    pub(crate) fn render(&self, language: Language) -> String {
        language.format(
            self.key,
            &self.args.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    }
}

/// Localized pick-list item retaining the original typed command value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice<T> {
    pub(crate) value: T,
    label: String,
}
impl<T> Choice<T> {
    pub(crate) fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}
impl<T> fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_key_has_both_languages_and_matching_argument_slots() {
        for &key in KEYS {
            let en = Language::En.text(key);
            let ru = Language::Ru.text(key);
            assert!(!en.trim().is_empty() && !ru.trim().is_empty(), "{key:?}");
            for index in 0..10 {
                let slot = format!("{{{index}}}");
                assert_eq!(
                    en.matches(&slot).count(),
                    ru.matches(&slot).count(),
                    "{key:?}"
                );
            }
            for language in [Language::Ru, Language::En] {
                assert!(
                    !language
                        .format(key, &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"])
                        .contains('{'),
                    "{key:?}"
                );
            }
        }
    }
    #[test]
    fn russian_and_english_counts_cover_teens_and_hundreds() {
        for (n, pane, process) in [
            (0, "панелей", "процессов"),
            (1, "панель", "процесс"),
            (2, "панели", "процесса"),
            (5, "панелей", "процессов"),
            (11, "панелей", "процессов"),
            (12, "панелей", "процессов"),
            (14, "панелей", "процессов"),
            (21, "панель", "процесс"),
            (22, "панели", "процесса"),
            (111, "панелей", "процессов"),
            (114, "панелей", "процессов"),
            (121, "панель", "процесс"),
        ] {
            assert_eq!(Language::Ru.count(n, Noun::Pane), format!("{n} {pane}"));
            assert_eq!(
                Language::Ru.count(n, Noun::Process),
                format!("{n} {process}")
            );
        }
        assert_eq!(Language::En.count(1, Noun::Process), "1 process");
        assert_eq!(Language::En.count(21, Noun::Pane), "21 panes");
    }
    #[test]
    fn external_values_are_preserved_and_never_interpreted_as_templates() {
        let detail = "Ubuntu {1} / имя / container-name";
        let notice = Key::Error.with([detail.into()]);
        assert_eq!(
            notice.render(Language::Ru),
            format!("Ошибка операции: {detail}")
        );
        assert_eq!(
            notice.render(Language::En),
            format!("Operation failed: {detail}")
        );
    }
}
