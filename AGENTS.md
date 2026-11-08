# AGENTS.md — Tervix: complete product requirements & engineering constitution

> **Single authoritative file.** This is both the full technical specification and mandatory instructions for all AI coding agents, maintainers and contributors. No separate TECHNICAL_SPEC.md or CLAUDE.md is required. If project code contradicts this document, describe the conflict and propose an ADR rather than silently discarding requirements.

# AGENTS.md — Tervix engineering rules

## Mission
You are contributing to **Tervix**, a Windows-first Rust 2024 terminal and developer workspace. Core features: ConPTY terminal emulator, windows/panes, system/process monitor, Docker and WSL. This file is the complete source of truth for product requirements and engineering rules. Keep terminal functionality reliable before adding features.

## Before editing
1. Read the relevant section of this file and affected crate documentation.
2. Locate nearest existing behavior and tests. Do not build a parallel abstraction.
3. For nontrivial changes, write a short plan, constraints, and how to verify it; propose ADR for public APIs, crates, FFI, and security-sensitive decisions.
4. Check the actual Cargo dependency versions, APIs, licensing and platform compatibility. Never invent APIs.

## Architecture
- Cargo workspace; modular monolith. `core` independent of I/O and GUI; application orchestration; adapters implement OS/Docker/PTY ports; UI depends on application DTOs.
- No direct Bollard, sysinfo, Win32 handles or PTY handles in UI/domain.
- Prefer feature ownership over generic `utils`/`managers` and avoid unnecessary traits/crates.
- Preserve public interfaces and persisted config compatibility or document migration.
- ConPTY/Windows Terminal distinction: host console apps via PTY, not embedded Windows Terminal UI.

## Rust style
- Idiomatic Rust 2024; explicit types for IDs and lifecycle states.
- Use `Result` and `?`; `thiserror` for library errors, `anyhow` only at executable boundaries.
- Avoid `unwrap`, `expect`, `panic`, `todo`, `unimplemented` in production runtime.
- Avoid unsafe except isolated FFI modules with `// SAFETY:` invariant explanation and tests.
- Meaningful names. `pub` only as needed; document public APIs, fallibility and invariants.
- No global mutable singletons, blanket lint suppressions, unbounded channels, busy loops, blocking GUI I/O, or locks held across `.await`.
- Prefer clear `match` for enum states; tests should catch new variants requiring explicit behavior.
- No secrets/shell inputs/credentials in tracing. External input and terminal escape sequences are untrusted.

## Quality gate
Run and report status of applicable commands:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace
```
Also run relevant integration/e2e tests on Windows when touching ConPTY, Win32, WSL, Docker or UI. If not run, explain why; never claim unexecuted tests passed. Prefer focused tests during iteration and workspace checks before completion.

## Tests
- Every bug fix gets a regression test where practical; cover failure/cancel/timeout flows.
- Test pure reducers, PTY lifecycle and buffer limits, Docker disconnected states, config migrations, process PID reuse safety.
- Use realistic fixtures; avoid broad snapshots that hide behavior. Keep tests deterministic, mock I/O boundaries rather than internals.
- Do not claim performance targets without reproducible benchmarks and baseline hardware.

## Security
- Never silently elevate privileges, execute commands with shell string concatenation, auto-delete Docker volumes, or trigger destructive operations without explicit user intent.
- Review Docker socket privileges, OSC clipboard handling, remote credentials, Windows registry changes and updates.
- Do not make network calls or modify the developer's external accounts unless the task requires it and user explicitly approves sensitive actions.
- Do not edit LICENSE/NOTICE or introduce incompatible dependencies without review.

## Change reporting
At completion provide: concise summary, files modified, tests actually run, limitations/risks. No fabricated results. Prefer smallest focused solution instead of large speculative refactors.


---

## Полное техническое задание / Full technical specification

**Статус:** проектная спецификация v0.1 · **Дата:** 2026-11-08 · **Язык:** Rust 2024 · **Основная ОС:** Windows 11 x64 · **Лицензия:** Apache-2.0

> Документ задаёт целевое поведение, ограничения, архитектуру и критерии приёмки. Указанные зависимости — кандидаты, а не обещание совместимости конкретных версий. Версии фиксируются после технических прототипов и аудита лицензий.

## 1. Цель и продукт

Tervix — Windows-first нативная графическая рабочая среда для разработчика и системного администратора, объединяющая полноценный терминальный эмулятор, рабочие пространства, мониторинг ресурсов и процессов, Docker, WSL и служебные инструменты. Точка входа — быстрый терминал; панели — самостоятельные функциональные модули.

**Целевая аудитория:** разработчики Rust/.NET/Node.js, DevOps, администраторы Windows и Linux через WSL/SSH, пользователи Docker Desktop/Engine. **Принцип:** единая навигация и UX, локальные данные по умолчанию, явные полномочия, отсутствие обязательного облака или телеметрии.

### 1.1 Не цели первой версии
- Не писать собственный shell (PowerShell, cmd, bash остаются внешними процессами).
- Не заменять Docker Engine, Windows Task Manager, WSL, Windows Terminal системно; реализуется альтернативный GUI и совместимость.
- Не встраивать UI Microsoft Windows Terminal как библиотеку: взаимодействовать с ОС посредством ConPTY.
- Не включать удалённый агент, Kubernetes, встроенный редактор кода, магазин плагинов, синхронизацию аккаунтов в MVP.
- Не требовать административных прав для обычного запуска.

### 1.2 Приоритеты
**P0:** запуск, терминал, окна/вкладки, сохранение настроек, надёжность. **P1:** WSL, мониторинг, Docker, интеграция Windows. **P2:** SSH-профили, менеджер портов, Compose, файловый менеджер, расширения. **P3:** удалённые агенты, плагины третьих сторон, Linux/macOS.

## 2. Технические решения (ADR summary)

- **ADR-001: Windows-first, без преждевременной кроссплатформенности.** Поддержать Windows 11 x64, далее Windows arm64 при проверке зависимости, затем Linux/macOS через интерфейсы платформы. Обновляемая матрица поддержки.
- **ADR-002: Rust Cargo workspace, модульный монолит.** Один процесс приложения и набор независимых библиотечных crates. Никаких локальных HTTP-микросервисов для обычного GUI.
- **ADR-003: Iced как базовая гипотеза GUI.** Типизированный update/view и мультиоконность. Перед окончательным выбором обязателен spike: отображение интерактивного терминала, IME, accessibility, HiDPI, resize, лаги ввода. Альтернатива — GPUI при доказанной Windows-совместимости используемого состояния проекта; Tauri 2 + xterm.js как fallback с менее чистым Rust-only UI.
- **ADR-004: portable-pty + alacritty_terminal.** ConPTY через PTY-адаптер; terminal state machine независимо от UI. Совместимость API и лицензий подтвердить отдельным spike.
- **ADR-005: Безопасные адаптеры.** `sysinfo` и по необходимости `windows` crate для Windows-специфичных метрик, `bollard` для Engine API, `tracing` для журналирования. Никаких команд `docker ...` для базового получения данных, если API доступен.
- **ADR-006: Разделение core / application / ports / adapters / UI.** Бизнес-правила не знают о GPU, Win32, Bollard или Iced.
- **ADR-007: Безопасность раньше расширяемости.** Плагины будущего — отдельные процессы и capability model, без загрузки непроверенного нативного кода в адресное пространство приложения по умолчанию.
- **ADR-008: Apache-2.0 с NOTICE.** Авторские уведомления сохраняются в соответствии с лицензией; требование отображать имя автора в UI форка лицензией автоматически не создаётся.

## 3. Пользовательские сценарии

1. Открыть Tervix → автоматически запускается профиль PowerShell, поддерживаются горячие клавиши, UTF-8, Unicode, история прокрутки.
2. Создать вкладку WSL: пользователь выбирает дистрибутив и директорию; после запуска видит bash/zsh и использует интерактивные TUI (`vim`, `htop`, `btop`).
3. Разделить рабочее пространство вертикально: слева shell, справа мониторинг CPU/RAM.
4. Docker → контейнеры → выбрать контейнер → читать live logs → открыть exec-терминал → остановить с явным подтверждением.
5. Мониторинг → список процессов → поиск, сортировка, раскрытие PID, память, CPU → завершить выбранный процесс после подтверждения.
6. Windows Explorer → «Open in Tervix» открывает терминал в текущей директории (с учётом контекста прав и корректной обработки пробелов).
7. Перезапустить Tervix → восстановить компоновку окон/вкладок, но не заявлять восстановление живых shell-процессов без специального механизма мультиплексора.
8. Docker недоступен → показать понятный статус, диагностику соединения, остальные функции продолжают работать.

## 4. Функциональные требования

### 4.1 Shell, PTY и терминальная эмуляция (TERM)
- TERM-001: запуск PowerShell 7 (если установлен), Windows PowerShell, cmd, WSL; определение профилей и ручная настройка пути.
- TERM-002: каждый pane владеет собственной PTY-сессией; UUID сессии, жизненный цикл Starting/Running/Exited/Failed/Closing.
- TERM-003: двусторонний поток байтов PTY, закрытие stdin, обработка завершения дочернего процесса, Ctrl+C и resize ConPTY.
- TERM-004: поддержка 24-bit color, ANSI SGR, CSI/OSC, alternate screen, курсора, scrollback, мыши, bracketed paste, UTF-8/Unicode, wide characters и combining characters.
- TERM-005: корректность поведения интерактивных приложений: PowerShell, git, vim/neovim, btop, ssh. Поддержка OSC 8 hyperlinks. Небезопасные OSC-команды (например, clipboard) — только по политике.
- TERM-006: буфер прокрутки настраиваемый, лимит памяти, поиск по истории и копирование/вставка; клавиатурные shortcuts не должны красть ввод у TUI без учёта контекста.
- TERM-007: перенаправление текста drag-and-drop как вставка с подтверждением для многострочных/опасных команд (настраивается).
- TERM-008: shell integration и отображение рабочей папки — позднее, через адаптеры/OSC, с безопасным fallback.
- TERM-009: при закрытии pane предлагать подтверждение при активной сессии; невозможность восстановить процесс после перезапуска документировать.
- TERM-010: тестировать управление размером буфера, input encoding, paste, быстрое печатание, backpressure и разрыв дочернего процесса.

### 4.2 Окна, панели, workspace (UI)
- UI-001: множественные системные окна и вкладки, split panes горизонтально/вертикально, перетаскивание вкладок, клавиатурная навигация, фокус.
- UI-002: гибкая схема размещения `Split(Axis, Ratio, Left, Right) | Leaf(PaneId)`; проверять границы ratio и не допускать пустого дерева.
- UI-003: панели терминала, мониторинга, Docker, логов, настроек с единой моделью навигации.
- UI-004: палитра команд Ctrl+Shift+P, shortcut registry, контекстные меню, поиск по функциям.
- UI-005: тёмная/светлая/системная тема, DPI scaling 100–250%, минимум 1366×768, resize без потери содержимого.
- UI-006: профили с иконками/цветом/command/args/cwd/env, экспорт и импорт безопасной конфигурации.
- UI-007: сохранение раскладки окон атомарно с версионированной схемой; при повреждении файла — резервная копия и понятная ошибка.
- UI-008: доступность с keyboard-only navigation, видимым фокусом, контрастностью и screen-reader labels там, где UI toolkit позволяет; недоступность конкретных возможностей — tracked issue, а не скрытое обещание.

### 4.3 Мониторинг и диспетчер процессов (SYS)
- SYS-001: общая загрузка CPU + per-core, физическая и доступная RAM, swap/pagefile, диск, сетевые интерфейсы.
- SYS-002: таблица процессов: PID, имя, путь (если доступен), CPU, resident/private memory, пользователь/владелец (если доступен), состояние, время работы; отсутствующие данные `N/A`, не ноль.
- SYS-003: сортировка/фильтрация/поиск, обновление с конфигурируемым интервалом 500–5000 мс, стандарт 1000 мс для активного окна, сниженная частота при скрытой вкладке.
- SYS-004: графики со скользящим окном 60 сек / 5 мин / 30 мин, кольцевые буферы; нормализованные единицы и корректные временные метки.
- SYS-005: завершение процесса: показать PID, имя, подтверждение и итог; graceful first где применимо, force отдельно. Защита от ошибки повторного использования PID: проверять идентификатор/время старта.
- SYS-006: GPU метрики только через отдельный адаптер Windows/NVIDIA/AMD/Intel, с режимом `unsupported`; не выдавать условные значения за настоящие.
- SYS-007: мониторинг Tervix отдельно от системы: собственные CPU/RAM/GPU, event queue depth, dropped samples и FPS.
- SYS-008: не требовать elevation ради мониторинга; недоступные операции отображать с причиной.

### 4.4 Docker и Compose (DOCKER)
- DOCKER-001: соединение с локальным Docker Desktop Engine (Windows named pipe) и настроенными Docker contexts/remote endpoints; credentials не хранятся открытым текстом.
- DOCKER-002: список контейнеров (включая остановленные), status, image, ports, CPU, memory, uptime; обновление событийно + reconciliation polling.
- DOCKER-003: start/stop/restart/pause/unpause, inspect, remove; удаление и destructive-операции подтверждаются; массовые операции отдельно.
- DOCKER-004: live logs с ограниченным буфером и фильтрами, пауза, таймстемпы, экспорт с предупреждением о секретах.
- DOCKER-005: интерактивный `exec` с TTY и поддержкой resize/attach/detach; отдельная обработка случая, если shell внутри образа отсутствует.
- DOCKER-006: images, volumes, networks: просмотр, создание, inspect, безопасное удаление по отдельным сценариям.
- DOCKER-007: Docker Compose: проекты/сервисы и действия up/down/restart/logs после исследования возможностей API и безопасного использования Compose CLI; показывать точную выполняемую операцию.
- DOCKER-008: никаких автоматических prune, удаления volumes или `down -v`; для prune — dry-run/оценка и подтверждение.
- DOCKER-009: корректно обрабатывать Docker unavailable, access denied, daemon reconnect, таймаут, несовместимую API версию и `WSL integration disabled`.
- DOCKER-010: никогда не считать Docker socket безопасным источником: доступ к Engine практически эквивалентен высоким привилегиям на хосте.

### 4.5 WSL, SSH и Windows (INTEGRATION)
- INT-001: обнаруживать дистрибутивы WSL с `wsl.exe --list --quiet`, обрабатывать локализацию/кодировки, ошибки и отсутствие WSL.
- INT-002: запуск shell через `wsl.exe --distribution <name> -- ...` без сборки единой строки shell и без непреднамеренного исполнения интерпретатором.
- INT-003: корректная передача cwd: локальный Windows cwd и Linux cwd не эквивалентны; преобразование путей только в допустимых сценариях.
- INT-004: SSH: первоначально запуск штатного `ssh` в PTY и использование существующего `~/.ssh/config`; никакого самостоятельного хранения private keys.
- INT-005: Windows shell context menu для папок и пустого пространства, отдельная опция включения/отключения, scoped per-user установка при возможности, корректное удаление при uninstall.
- INT-006: single-instance policy опциональная: новый вызов `tervix --cwd ...` отправляет IPC существующему экземпляру; IPC локальный, аутентифицирован/ограничен правами пользователя, с version negotiation.
- INT-007: обработка ассоциаций протоколов и notification permissions без скрытых side effects.
- INT-008: UAC elevation только для конкретной функции/запроса пользователя, не автоматическая эскалация всего процесса.

### 4.6 Инструменты (TOOLS)
**P2:** файловый браузер (cwd, предпросмотр текста, безопасные операции), инспектор TCP/UDP портов и процессов, управление Windows Services, редактор environment variables, набор диагностических инструментов. Инструменты — отдельные features, отключаемые без влияния на терминальное ядро.

### 4.7 Настройки, обновления и расширения
- Конфиг versioned TOML + миграции, резервная копия перед миграцией, путь по OS conventions.
- Хранить secrets в Windows Credential Manager/OS keychain; не сериализовывать токены вместе с профилями.
- Import/export без secrets по умолчанию; отдельное подтверждение если включено.
- Обновления только проверка/скачивание по запросу или opt-in, проверка подписи/контрольной суммы; rollback/atomic replace где возможно.
- Будущие plugins: manifest с declared capabilities, subprocess isolation + scoped IPC; запрет autoexec; версионированный контракт.

## 5. Нефункциональные требования и измерение

| Номер | Цель | Метод проверки |
|---|---|---|
| NFR-01 | Холодный старт до первого интерактивного терминала ≤ 2 сек на целевой машине | 30 прогонов, P95, фиксированная конфигурация железа |
| NFR-02 | Время отображения символа при обычном вводе P95 < 30 мс | инструментированные keyboard→paint timestamps |
| NFR-03 | Idle CPU < 1% одного логического ядра, когда мониторинг скрыт | Windows Performance Recorder/профайлер |
| NFR-04 | Idle RSS < 180 MiB для одного пустого pane как ориентир, не контракт | сценарный benchmark; корректировка после spike |
| NFR-05 | Без зависания UI от Docker/PTY/system polling > 100 мс | нагрузочные тесты с искусственными задержками |
| NFR-06 | UI > 55 FPS при обычной прокрутке на эталонной машине | frame statistics и трассировка |
| NFR-07 | Нет потери управления после отключения Docker/WSL | fault injection |
| NFR-08 | 10 параллельных PTY и 10k строк вывода без deadlock | soak, тесты backpressure |
| NFR-09 | No elevation by default; no credential leakage in logs | security tests и review |
| NFR-10 | Все настройки переживают корректное завершение и аварийный crash без порчи файла | crash/restart tests |

Цифры — **целевые бюджеты**, подлежат уточнению на контрольных машинах. Отдельно измерять debug/release, Win11 23H2+ и разные GPU; без базовой линии нельзя выдавать цели за достигнутые результаты.

## 6. Архитектура

### 6.1 Границы и зависимости
```text
tervix-app (composition root)
   ├── tervix-ui (Iced views, events, rendering)
   ├── tervix-application (use cases, orchestration)
   │      └── tervix-core (domain model, contracts, errors)
   └── adapters (implement core ports)
          ├── tervix-terminal (PTY + emulator)
          ├── tervix-system (sysinfo/Windows)
          ├── tervix-docker (Bollard)
          ├── tervix-platform-windows (shell integration/IPC)
          └── tervix-config (filesystem/serde/toml)
```

`core` не зависит от внешнего мира; `application` не зависит от UI и конкретных API; adapters могут зависеть от core и внешних SDK; `app` собирает конкретные реализации и DI вручную. UI зависит от DTO/state/application, но не напрямую от Windows API или Bollard. Реализовать Ports & Adapters только на реальных границах I/O, не для каждой структуры/функции.

### 6.2 Модели
- `WorkspaceId`, `WindowId`, `PaneId`, `SessionId`, `ContainerId`, `ProcessIdentity { pid, start_time }` — типизированные ID вместо случайного использования `String`.
- `PaneKind`: Terminal, SystemMonitor, DockerExplorer, LogViewer, Settings, Tool.
- `LayoutNode`: Leaf или Split с ориентировкой и ratio; доменный invariant и тесты структуры.
- `TerminalSession`: жизненный цикл, process identity, terminal settings; не содержит прямой `PtyMaster` в общем состоянии GUI.
- `Sample<T>`: timestamp, value, source, freshness, optional error; `MetricsSnapshot` immutable для UI.
- `DockerConnection`: local/remote, state, capabilities, last_seen; токены — references к credential vault.
- `AppError`: machine-readable code, context, retryable, user-facing message generated at presentation boundary.

### 6.3 Событийная модель
```text
UI input → Command → application use case → Port → Adapter
                                      ↓
                        bounded event channel
                                      ↓
                     state reducer → state snapshot → UI
```
События типизированные, без `serde_json::Value` как общего внутреннего протокола. Очереди bounded, приоритеты input/close/resize выше телеметрии. Coalesce metric updates; PTY bytes не выбрасывать бесшумно: backpressure, bounded spool/scrollback и явное поведение при overflow. Обновления UI выполняются в правильном thread context.

### 6.4 Конкурентность и ресурсный lifecycle
- Tokio используется для фоновых операций; блокирующий PTY reader — выделенный thread или `spawn_blocking` с контролем остановки; Iced UI loop никогда не блокируется `std::thread::sleep`, `wait()` или network I/O.
- На каждую PTY-сессию: единственный владелец lifecycle, cancellation token, reader, writer, event channel, child handle, clean shutdown.
- Для метрик: один scheduler на источник, семплы шарятся между вкладками; скрытые вкладки экономят ресурсы.
- Для Docker: reconnect с exponential backoff + jitter, таймауты, cancellation и event stream resubscription.
- Потоки/таски не создаются на каждый render; нет глобальных `Mutex<HashMap<...>>` как универсального bus.
- Любые async locks удерживаются минимально; запрещено удерживать lock через `.await`, если это не явно доказанная модель.

### 6.5 Контракт UI
- Model → Message → Update → Task → View (в стиле Elm/Iced).
- `view` — чистая функция состояния; побочные эффекты только в application effects.
- Глобальный `AppState` хранит только навигацию и видимые snapshots, а не raw handles OS.
- По feature: `feature/{state,message,update,view,components}`, но в разных crates границы могут отличаться.
- `main.rs` — конфигурация, составление адаптеров, запуск; никаких business rules.

## 7. Структура репозитория

```text
tervix/
├── Cargo.toml                    # workspace resolver = "3"; общие зависимости
├── Cargo.lock                    # commit для бинарного приложения
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── LICENSE
├── NOTICE
├── README.md
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── AGENTS.md                    # единый источник агентных правил
├── .github/
│   ├── workflows/ci.yml
│   ├── workflows/release.yml
│   └── ISSUE_TEMPLATE/
├── apps/
│   └── tervix/                  # composition root / executable
│       ├── Cargo.toml
│       └── src/{main.rs,bootstrap.rs}
├── crates/
│   ├── tervix-core/             # entity, value objects, traits, errors
│   ├── tervix-application/      # use cases, orchestration, DTO
│   ├── tervix-ui/               # shell, navigation, widgets, design system
│   ├── tervix-terminal/         # terminal state, PTY orchestration
│   ├── tervix-system/           # telemetry, process operations
│   ├── tervix-docker/           # Docker API adapter
│   ├── tervix-wsl/              # WSL adapter
│   ├── tervix-platform-windows/ # Win32, Explorer, native IPC
│   └── tervix-config/           # TOML schema/migrations/storage
├── tests/
│   ├── fixtures/
│   ├── integration/
│   └── e2e/
├── benches/
│   ├── terminal_throughput.rs
│   └── metrics_buffer.rs
├── docs/
│   ├── architecture.md
│   ├── adr/
│   ├── security-model.md
│   ├── terminal-compatibility.md
│   ├── windows-integration.md
│   ├── quality-gates.md
│   └── roadmap.md
├── assets/{icons,fonts,themes}/
├── packaging/{msix,installer}/
└── xtask/                        # опционально: build/ci/packaging tasks
```

**Правило роста:** новый crate только если есть независимая ответственность, отдельная граница зависимостей или доказанная необходимость. До первого вертикального slice допустимо начать с 5–6 crates и дробить по мере роста. Избегать «по одному crate на модель».

## 8. Rust Coding Standard

### 8.1 Идиоматичность
1. Edition 2024, stable Rust; MSRV документирован и проверяется CI. Workspace dependencies pin в `Cargo.lock`, без wildcard `*` в зависимостях.
2. Форматирование `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace` — обязательные quality gates.
3. `snake_case` для модулей/функций/полей, `PascalCase` для типов, `SCREAMING_SNAKE_CASE` констант. Содержательные имена, без `data`, `manager`, `util` как универсальных catch-all.
4. API рассчитаны на состояния, которые допустимы по типам: newtypes для PID/IDs, enum для вариантов, builder для сложных конфигураций. Невозможные состояния не представлять набором конфликтующих bool.
5. Предпочитать `&str`/`&Path` во входных параметрах, owned `String`/`PathBuf` при реальной передаче владения; минимизировать `clone()` больших буферов. Не делать всё `Arc<Mutex<_>>` автоматически.
6. При возврате ошибок использовать `Result<T, Error>`, `?`, контекст. `thiserror` для типизированных ошибок библиотек/портов; `anyhow` только на composition boundary/CLI. Не переводить все ошибки в строки.
7. Не использовать `unwrap()`, `expect()`, `panic!`, `todo!`, `unimplemented!` в production-path; исключения в тестах и строго обоснованных инвариантах.
8. `unsafe` запрещён в обычных модулях; локализуется в `platform-windows` FFI, с `// SAFETY:` объяснением инвариантов и тестами. Не отключать lint на весь crate.
9. Публичные API документировать `///` с ошибками, инвариантами, примером при необходимости; `#[must_use]` для важных возвращаемых значений.
10. Запрет скрытых side effects в `view`, `From`, `Display`, getters; если функция вызывает системное действие, имя это отражает.
11. Не использовать глобальные mutable singleton, циклические crate dependencies, massive god modules, `pub use *` без rationale, `#[allow(dead_code)]` чтобы скрыть сломанный дизайн.
12. В async-функциях не держать sync lock через `.await`; ограничить `Arc`, channels, lifecycle. Гарантировать безопасное завершение при cancellation.
13. `tracing` с structured fields и span IDs; секреты и shell input не логировать. Определять лог-уровни и redaction в одном месте.
14. Все feature flags названы по возможностям, а не по временным условиям; platform `cfg` проверяется CI.
15. Dependencies: PR должен объяснять необходимость, лицензию, MSRV, поддержку Windows, maintenance и transitive risk. `cargo deny`/`cargo audit`, SBOM перед релизом.

### 8.2 Границы и паттерны
- **Hexagonal / Ports & Adapters:** traits только для внешних I/O границ (`PtyProvider`, `SystemTelemetry`, `ContainerRuntime`, `ConfigStore`, `ProcessController`). Не создавать интерфейс для каждого сервиса.
- **Functional Core / Imperative Shell:** расчёты нормализации метрик, layout и reducers — чистые тестируемые функции.
- **State Machine:** lifecycle PTY, Docker connections, updater; enum + exhaustive match.
- **Strategy:** выбор реализации terminal provider по платформе/профилю, не giant `if cfg!` внутри бизнес-логики.
- **Observer/Event Streaming:** ограниченные каналы, явное завершение stream, стабилизированные DTO.
- **Repository** только для действительно сохраняемого состояния; не называть Docker API «репозиторием базы данных».
- **Adapter/Anti-Corruption Layer:** `bollard` types, `sysinfo` structures, `windows` handles не уходят в core/UI.
- **Dependency Injection:** вручную через конструкторы и `AppServices`, без runtime DI-контейнера.
- **Event Sourcing** не использовать для GUI: избыточно. **CQRS** только если сложность реально требует отдельной read model.

### 8.3 Размер и изменения
- Один PR — одна сквозная задача, отдельные commits; устойчивая ABI/serde схема при изменении persisted config.
- Функции обычно до ~50 строк, файлы обычно до ~400 строк — ориентиры для ревью, а не искусственное правило разбиения.
- Любой публичный trait, новый crate, FFI или breaking change требует заметки ADR или rationale в PR.
- Комментарии объясняют «почему», ограничения и безопасность, а не пересказывают код.
- Никаких имён «new_new», `temp2`, `fix_final`, магических строк/чисел без констант и обоснования.

### 8.4 Пример портов
```rust
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct CpuSample {
    pub observed_at: SystemTime,
    pub usage_percent: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("telemetry provider is unavailable")]
    Unavailable,
    #[error("telemetry operation failed: {0}")]
    Provider(String),
}

pub trait CpuSampler: Send + Sync {
    fn sample(&self) -> Result<CpuSample, TelemetryError>;
}
```
Пример — интерфейс доменной границы, не реализация. Частоту семплирования и задачи задаёт application layer, а не `CpuSampler`.

## 9. Безопасность и приватность

**Threat model:** локальный пользователь, вредоносные процессы в shell, удалённый Docker daemon, ввод с терминала/OSC, пути с пробелами/спецсимволами, секреты в env и логах, вредоносные clipboard payloads, race при завершении процессов.

- Не собирать команды из конкатенации строк и не передавать в shell, если нужен прямой запуск процессов с argv.
- `Path`/`OsString`, корректное quoting только там, где неизбежен shell, проверка происхождения пути и cwd.
- Не парсить escape sequences самостоятельно regex-ом; ограничить OSC 52, URL opening и прочие внешние действия политикой.
- Для Docker HTTPS — TLS verification, auth через system vault, allowlist endpoints, никакого silent downgrade до незашифрованного транспорта.
- Удаление контейнеров/образов/volumes и завершение процессов имеют separate confirmation и audit event без секретов.
- Обновления/релизы подписывать; CI release permissions minimal; GitHub Actions по commit SHA для sensitive jobs по возможности.
- Тестировать path traversal, malformed PTY sequences, oversized logs, invalid Unicode, timeout, credential leaks, shell injection.
- Сбор технических метрик только локально; телеметрия/analytics — opt-in и документировать список событий.
- Подписанные установочные пакеты и репутация SmartScreen — отдельная задача релиза, не предполагаются автоматически.

## 10. Конфигурация

Предлагаемая user config `config.toml` (схема уточняется):
```toml
schema_version = 1

[ui]
theme = "system"
restore_layout = true
show_sidebar = true

[terminal]
default_profile = "powershell"
scrollback_lines = 10000
confirm_multiline_paste = true

[[terminal.profiles]]
id = "powershell"
name = "PowerShell"
program = "pwsh.exe"
args = []

[[terminal.profiles]]
id = "wsl"
name = "WSL"
program = "wsl.exe"
args = []

[monitoring]
active_refresh_ms = 1000
background_refresh_ms = 5000
history_seconds = 300

[docker]
auto_connect = false
```

Требуется JSON Schema/схема валидации (либо TOML schema documentation), миграция `schema_version`, atomic writes через temp + rename с подходящей Windows реализацией, policy для повреждённого файла и предупреждение об отклонённых ключах.

## 11. Тестирование и совместимость

### 11.1 Пирамида
- Unit: layout reducer, state machine, metric normalization, filters, error mappings, config migrations.
- Contract: моки `PtyProvider`, `ContainerRuntime`, `SystemTelemetry` — фиксированные сценарии и инварианты, не подмена интеграционных тестов.
- Integration: ConPTY+PowerShell/cmd, Docker Engine на тестовом daemon, WSL при наличии runner.
- E2E: создать окно, запустить shell, вставить команду, split, close, восстановить layout; multi-monitor/HiDPI/IME/manual matrix.
- Snapshot/golden для ANSI sequences и UI state, property-based для layout tree and reducers; fuzz VT inputs при подходящем harness.
- Performance/soak: high volume stdout/stderr, миллион строк, 10 PTYs, reconnect Docker, prolonged CPU sampling, cancellation leak checks.

### 11.2 Windows test matrix
Windows 11 x64 (основная), PowerShell 7/Windows PowerShell/cmd, Windows Terminal установлен/не установлен, WSL установлен/не установлен, Docker Desktop работающий/неработающий, scaling 100/150/200%, тёмная/светлая тема, без elevation/с elevation, два монитора, разное системное locale, IME.

### 11.3 Definition of Done
- Выполнены acceptance criteria и негативные сценарии.
- Есть тесты на ошибку/отмену, новая публичная поверхность задокументирована.
- `fmt`, `clippy`, `test`, security/license checks зелёные.
- Не добавлены скрытые блокировки UI, неконтролируемые задачи и утечки handles.
- Новые системные возможности корректно работают без повышенных прав либо сообщают причину отказа.
- Docs + changelog/ADR обновлены при необходимости; review подтверждает корректность лицензий.

## 12. CI/CD и выпуск

**PR workflow:** fmt; clippy; test; doc; cargo-deny/cargo-audit; minimum supported Rust check; Windows build/test; Linux compile (если код платформенно независимый). Cache keyed by toolchain/lockfile. Release по tag, воспроизводимое описание версии и provenance, подпись артефактов, SHA256, архив portable, установщик (MSIX/WinGet позже), LICENSE/NOTICE внутри пакета. Никаких токенов в логах и артефактах. `cargo +stable ...` зафиксирован через `rust-toolchain.toml`.

**Ветвление:** `main` защищена, feature branches, Conventional Commits (`feat(terminal): ...`), PR template с Why/What/Tests/Risks. SemVer; 0.x — breaking changes допустимы с release notes; стабильный 1.0 только после проверенной совместимости.

## 13. План разработки и контрольные точки

### Phase 0 — Research/spikes
**Exit:** лицензии и совместимость утверждены; два spike-приложения: Iced + интерактивная PTY панель, sysinfo + Bollard connection. Проверены keymap, IME, ConPTY resize, TUI, rendering. Зафиксирован ADR выбора GUI. CI и workspace skeleton.

### Phase 1 — Terminal MVP
**Scope:** главное окно, вкладки, PowerShell/cmd/WSL профили, shell I/O, VT state, selection, clipboard, search, scrollback, resize, colors, error dialogs. **Exit:** shell/TUI smoke tests, 2h soak, cancel/exit без утечек.

### Phase 2 — Workspaces & UX
**Scope:** split panes, multi-window, command palette, themes, settings, layout restore, shortcuts, native menus. **Exit:** ручные сценарии переключений/сохранения и HiDPI.

### Phase 3 — System Monitor
**Scope:** CPU/RAM/processes, graphs, filtering, safe kill, error states. **Exit:** профилирование и корректность метрик на Win11.

### Phase 4 — Docker
**Scope:** connection, container list/lifecycle, logs, exec, images, volumes, networks. **Exit:** тесты failure/reconnect, destruction safety.

### Phase 5 — OS Integrations / P2
**Scope:** Explorer integration, configurable shell profiles, SSH workflows, Compose, file explorer, ports. **Exit:** installer/uninstaller clean, security review.

### Phase 6 — Hardening / Beta / 1.0
**Scope:** fuzz/soak/perf benchmarks, accessibility, Windows signing, release and update security, docs, third-party licenses, installer, feedback. **Exit:** publish beta and evaluate stable criteria; не обещать сроки без измерения.

## 14. Реестр рисков

| Риск | Вероятность / влияние | Мера |
|---|---|---|
| UI toolkit не подходит для high-throughput терминала | средняя / высокое | ранний spike, benchmark, fallback GUI |
| Лицензия terminal core/зависимостей несовместима | средняя / высокое | dependency/license audit до интеграции |
| ConPTY странно работает с resize/locale/IME | высокая / среднее | Win11 compatibility harness |
| GPU показатели недоступны универсально | высокая / среднее | capability detection + `unsupported` |
| Docker socket повышает риск злоупотребления | высокая / высокое | explicit actions, least privilege, no remote port by default |
| UI блокируется синхронным I/O | средняя / высокое | bounded tasks, performance tests |
| Ранняя чрезмерная нарезка crates мешает разработке | средняя / среднее | минимальный workspace и ADR на разбиение |
| AI-агент генерирует непроверенные изменения | высокая / среднее | AGENTS, tests, human review, no auto-merge |

## 15. Источники и что из них адаптировано

Проверено 2026-11-08. **Не копировать текст лицензионно защищённых инструкций дословно; ниже собственные нормы Tervix, основанные на проверенных практиках.**

- Zed `.rules`/`AGENTS.md`/`CLAUDE.md`: структура агентных инструкций, унифицированный первичный источник; оба файла в Zed указывают на `.rules`: https://github.com/zed-industries/zed/blob/main/.rules ; https://github.com/zed-industries/zed/blob/main/AGENTS.md
- uv `AGENTS.md` и `STYLE.md`: тестирование около изменений, conservative dependencies, Clippy, safety notes, review ergonomics: https://github.com/astral-sh/uv/blob/main/AGENTS.md ; https://github.com/astral-sh/uv/blob/main/STYLE.md
- rust-analyzer `CLAUDE.md`: crate ownership, минимальные изменения, ограничение публичных API и зависимостей; соблюдение собственной AI policy: https://github.com/rust-lang/rust/blob/main/src/tools/rust-analyzer/CLAUDE.md
- Pelikan `CLAUDE.md`: commands, границы crates, архитектурные docs и workflows: https://github.com/pelikan-io/pelikan/blob/main/CLAUDE.md
- dioxus-terminal `CLAUDE.md`: компактная документация зависимостей виджета, тестового контура и терминальных задач, но не готовая гарантия качества: https://docs.rs/crate/dioxus-terminal/latest/source/CLAUDE.md
- Ratatui `ARCHITECTURE.md`: workspace composition, целевые специализированные crates, dependency direction: https://github.com/ratatui/ratatui/blob/main/ARCHITECTURE.md
- WezTerm: богатый Rust terminal/multiplexer как технический референс: https://github.com/wezterm/wezterm
- Orca: разделение Docker core/adapters и GUI: https://github.com/edvin/orca
- Windows ConPTY docs: https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session
- PTY: https://docs.rs/portable-pty/latest/portable_pty/
- VT terminal core: https://docs.rs/alacritty_terminal/latest/alacritty_terminal/
- Iced: https://docs.rs/iced/latest/iced/
- sysinfo: https://docs.rs/sysinfo/latest/sysinfo/
- Bollard: https://docs.rs/bollard/latest/bollard/
- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/

## 16. Решения, которые надо утвердить после spike
1. Iced действительно проходит rendering/IME/Windows benchmark? При провале — решение Tauri 2 + xterm.js или иной доказанный GUI.
2. Какой минимальный поддерживаемый Rust toolchain с учётом sysinfo/iced/bollard и MSRV зависимостей?
3. Подход к terminal scrollback: ownership/locking/лимиты памяти/selection?
4. Поддерживать ли Docker Desktop `docker context` из GUI с локальной конфигурацией пользователя?
5. Как реализовать GPU metrics и поддерживать ли их до beta?
6. Политика split panes, drag-and-drop и восстановления сессий после crash.
7. Какой native installer и механизм signed updates?
8. Приоритет Windows arm64/Linux/macOS после 1.0.

**Главный критерий успеха:** Tervix удобен как терминал сам по себе; дополнительные панели не ухудшают его отзывчивость, стабильность и безопасность.


---


# Critical product direction: deep customization and FIRST-CLASS TILING

**Non-negotiable:** Tervix is a highly customizable, keyboard-friendly, multi-window, tiling developer workspace, NOT a terminal with a few hard-coded sidebars. Users must decide which tools occupy each area, drag/resize/rearrange them and restore their exact workspaces. Tiling is an essential P0/P1 architecture constraint, not a cosmetic Phase 6 feature. UI must remain useful without mouse interaction.

## Tiling terminology and product model

- **OS window:** independent native window, movable between monitors, supporting maximize/minimize, native DPI and decorations.
- **Workspace:** named, versioned, persistable composition of tabs/panes and their settings. Multiple workspaces can exist within one or more windows.
- **Tab:** ordered workspace/page in a window, containing a complete tiling layout; individual panels can be dragged to another tab.
- **Pane:** rectangular tile in a layout; hosts exactly one view instance (terminal, system dashboard, process list, Docker browser, logs, Docker exec, WSL, file explorer, settings, etc.).
- **View instance:** per-pane session/state; distinct from a module type. A terminal session, for example, is not the same as a tile ID.
- **Dock/floating panel:** optional mode for non-tiling overlays and utility windows. Floating panels must never replace or obstruct the core tiling workflow.

## Layout engine invariants and behavior (TILE requirements)

- TILE-001 [P0]: Typed layout tree, e.g. `Node::Split { id, axis, children, weights } | Node::Pane { id, view_id }` and explicit tab/workspace/window IDs. Use validated domain commands and unit tests, not raw mutation from UI widgets. Binary splits are acceptable for MVP; architecture should permit recursive arbitrarily deep splits.
- TILE-002 [P0]: Split active pane horizontally or vertically, resize boundary by mouse dragging OR keyboard shortcuts, focus directional neighbor (up/down/left/right), move active pane directional, swap neighboring panes, close pane, and undo layout changes (at least bounded undo stack for layout only).
- TILE-003 [P0]: Minimum tile dimensions and clamped split ratios. No zero-size, orphaned, duplicated pane IDs, cycles, inaccessible panes, or invalid focus. Define behavior when a window becomes too small: adaptive scroll/collapse, constrained resize, or explicit overflow, never silent data loss.
- TILE-004 [P0]: User-defined shortcuts via a conflict-aware shortcut registry with contexts (`global`, `terminal`, `layout`, `dialog`). Default terminal keybindings must not hijack Ctrl+C, Ctrl+V or application-specific TUI input unexpectedly. Define an escape hatch for sending captured shortcuts to terminal.
- TILE-005 [P0]: Active tile highlight, focus history, keyboard-only create/resize/move/swap/close and visible directional focus cues; proper focus restoration after tab/window changes, dialogs, rearrangement.
- TILE-006 [P1]: Drag any panel header to reposition a tile, split target at side/edge, swap tile, create tab, or detach to a new native window. Display explicit drop preview/valid targets. Drag cancellation must preserve the layout without side effects.
- TILE-007 [P1]: Save/load all layout topology, panel types, sizing, focused pane, selected tabs, theme and window geometry via versioned TOML/JSON schema. Use stable IDs, atomic writes, migration, backups and validation/recovery for corrupt layouts. Restore *layout and command configurations*, not fictional continuity of terminated child processes or Docker exec sessions.
- TILE-008 [P1]: Named layouts/presets: `Terminal`, `Development`, `Docker`, `Monitoring`, plus custom user layouts. Export/import layouts excluding secrets, machine-specific credentials and sensitive history by default.
- TILE-009 [P1]: Each tile has configurable title, icon, accent, visibility of header/toolbars, zoom/font sizes when meaningful, update/poll frequency when relevant. Appearance controls apply at global -> workspace -> module/pane override levels with deterministic precedence.
- TILE-010 [P1]: Support multiple screens, independent OS windows, drop across windows and per-monitor scaling. Persist geometry with monitor-disconnect recovery (clamp off-screen windows to available displays).
- TILE-011 [P1]: Resize must propagate terminal dimensions to PTY using debounced measurements; avoid spamming resize events and avoid destroying terminal scrollback. System charts must resize/reflow without restarting collection; module instances preserve identity during moves.
- TILE-012 [P1]: User can disable/hide module categories, reorder top bars and tool buttons, configure sidebar/dock behavior, hide status elements, and choose whether tabs/headings are visible. Optional compact/focus/zen modes.
- TILE-013 [P2]: Support nested stacks/tab groups, optional floating tool panels, synchronized views and custom dashboard widgets only after the baseline tree is robust.
- TILE-014 [P1]: Layout navigation/actions should be exposed as typed commands so keyboard, command palette, menus and mouse all invoke the same validated application actions.
- TILE-015 [P1]: Layout change event stream uses stable IDs and must be deterministic, serializable for diagnostics (without secret session data), and testable headlessly.

## Recommended layout domain model (illustrative, not mandatory API)

```rust
struct WindowId(uuid::Uuid);
struct WorkspaceId(uuid::Uuid);
struct TabId(uuid::Uuid);
struct PaneId(uuid::Uuid);
struct ViewId(uuid::Uuid);

enum SplitAxis { Horizontal, Vertical }
enum LayoutNode {
    Pane { id: PaneId, view: ViewId },
    Split {
        axis: SplitAxis,
        ratio: f32, // validate: 0.0 < ratio < 1.0 and min sizes
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}
```

Example in a developer's monitor (completely user adjustable):

```text
┌───────────────────────────────────────────────────────────────────┐
│ Tervix  Workspace: Development                 [tabs] [+] [cmd] │
├────────────────────────────┬──────────────────────────────────────┤
│                            │ Docker containers      CPU / Memory │
│ PowerShell or WSL          ├────────────────────┬─────────────────┤
│ (interactive terminal)     │ Docker logs        │ Process list    │
│                            │                    │                 │
├────────────────────────────┴────────────────────┴─────────────────┤
│ Status: profile · cwd · docker status · active shell · CPU usage │
└───────────────────────────────────────────────────────────────────┘
```

This is only an example. No fixed columns or permanently reserved regions: users may arrange ANY supported module in ANY tile, including more than one instance of the same module.

## Customization and design system

- CUST-001: Themes: light/dark/system/custom palettes, per-pane background, foreground, accents, transparency only where platform/toolkit supports it; accessibility contrast overrides.
- CUST-002: Font family, fallback fonts, ligatures where feasible, font size, line height, cursor appearance and blink rules for terminal. Optional Nerd Fonts support without bundled proprietary fonts.
- CUST-003: Configurable spacing, borders, rounding, panel header visibility, tab styles, sidebar positions, density and animations. Animation can be disabled for reduced motion and low-power devices.
- CUST-004: Keybindings as configurable commands, human-readable TOML schema, validation with useful error messages and conflict diagnostics; import/export of keymap.
- CUST-005: Global, workspace, per-profile and per-pane settings with clearly defined resolution order. Save UI changes persistently and reflect file edits with safe reload/rollback where possible.
- CUST-006: Pane content can vary independently: one tile PowerShell, second WSL, third Docker logs, fourth btop-like system monitor, fifth process/task-manager list. Duplicate instances are supported.
- CUST-007: Command palette lists panes, actions, workspaces and configurable commands; searchable without requiring mouse.
- CUST-008: Configuration paths and examples documented; schema versioning and migration tests required. Invalid customization must not stop users from opening a fallback terminal.
- CUST-009: No mandatory account, cloud theme store or telemetry. Custom themes/layouts remain local by default.

## Testing and acceptance for tiling

- Property-based tests (e.g. proptest) for arbitrary valid sequences of split/move/swap/close/undo/deserialize; verify no pane duplication/loss except explicitly closed, every split ratio valid, focus reachable, workspace nonempty or specified empty-state behavior.
- Serialization roundtrips and migration fixtures; corrupted or unknown panel type must degrade gracefully and preserve recoverable state.
- Headless tests for commands and layout reducers, integration UI tests for resize, drag preview/cancel, cross-window docking, DPI changes, monitor removal, keyboard-only navigation and pointer interactions.
- Regression: moving a terminal must not kill/restart its PTY; moving monitoring panels must not spawn duplicate pollers; dragging Docker exec must preserve its session or cleanly report disconnection.
- Performance: change layout with multiple active terminals and charts without blocking GUI or dropping input; measure p95 frame time/input latency and memory consumption instead of claiming unmeasured figures.

## Feature priority adjustment

- **Phase 0:** Prove terminal renderer + recursive layout tree + multi-window interactions in Iced on real Windows hardware. If toolkit fails key tiling/IME/performance requirements, document ADR and assess alternatives before committing UI architecture.
- **Phase 1:** Functional terminal PLUS keyboard-operated vertical/horizontal splits, resize/focus, minimal layout persistence. This is the first usable MVP.
- **Phase 2:** Visual drag-and-drop docking, customizable shortcuts/themes, workspace presets, multi-window panel transfers, robust layout restoration.
- **Phase 3+:** Monitoring, Docker and developer tools are regular dockable pane types in the existing layout system, never hard-coded pages that cannot be tiled.

## Agent workflow additions

- Never simplify user-requested tiling to fixed grid, fixed split counts, static sidebars or single-window tabs. If shipping a scoped MVP, implement foundational layout types first and explicitly track unimplemented TILE requirements.
- For every UI feature specify how it behaves as a pane, in multiple panes, when detached, during resize, after restore, and with keyboard-only control.
- Domain layout commands are transactional: validate before commit, produce an explicit result/error, maintain invariants and avoid half-applied drag operations.
- Respect user state. Do not discard settings/layouts on migration. Prefer backward-compatible behavior, provide backups and recovery.
- Keep view rendering and layout algorithms separated from PTY, Docker and sysinfo adapters. No direct OS access inside GUI widgets.
- The default pane is terminal, but module APIs must support any available pane type without a special top-level redesign.
