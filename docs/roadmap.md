# План ближайших срезов

Источник требований — AGENTS.md. Этот список отражает реализацию, не снижает
приоритет Windows, настоящего терминала, multi-window и keyboard-first tiling.

Обновлено: **2026-11-08**. Подробная [сводка реализации и проверок](status.md).
Phase 0 ещё не завершена. Ниже `[ ]` означает незавершённую работу, а не обещание
сроков. ADR-009/010 предложены для ревью, не считаются автоматически утверждёнными.

## Реализовано: начало Phase 0

- Cargo workspace Rust 2024, три crates с направлением app → application → core.
- TILE-001: рекурсивное дерево, typed IDs, валидация и доменные команды;
  полноценные агрегаты window/workspace/tab пока отсутствуют.
- TILE-002: доменные split/resize/focus/swap/close, направленные focus/move/swap/
  resize и bounded undo; keyboard/pointer adapters ещё не реализованы.
- TILE-003: доменные ratio, уникальность, reachable focus, ресурсные лимиты,
  логические minimum sizes, clamped geometry и explicit overflow canvas.
  Отображение overflow и доступ к off-viewport focus ещё должны появиться в UI.
- TILE-005 частично: bounded MRU history, FocusPrevious и recent-focus fallback
  при close. Визуальное выделение, tab/window/dialog restoration ещё отсутствуют.
- TILE-014 частично: единый typed command API, пока без UI-адаптеров.
- Headless demo, unit/contract/property-based тесты и начальный CI.

## Следующий срез: workspace и сохранение layout

- [ ] Window/workspace/tab агрегаты: владение layouts, порядок/выбор вкладок,
  валидный focus и уникальность identities между агрегатами.
- [ ] Независимый registry view instances с terminal по умолчанию, возможностью
  нескольких экземпляров одного module и без OS handles в domain/UI state.
- [ ] Транзакционные переносы панелей между tab/window с сохранением view ID.
- [ ] Явная политика reconcile registry при close/undo/restore: никакого скрытого
  перезапуска или мнимого восстановления завершённого shell.
- [ ] Версионированные DTO и schema validation, roundtrip и corrupt/unknown-view
  fixtures, recovery без потери восстанавливаемого пользовательского состояния.
- [ ] Детерминированные диагностические layout events без секретных session data.
- [ ] Config adapter: OS conventions paths, atomic writes, backups, migrations,
  recovery и отдельная проверка Windows replace semantics.

**Готово, когда:** headless-тесты подтверждают перенос без потери/дублирования
pane/view, валидные selected tab/focus, roundtrip topology/ratios/ID и безопасный
отказ/recovery повреждённого файла. Схема не считается стабильной до миграционных
тестов; перенос ID не выдаётся за проверенную непрерывность реальных PTY.

Семантика реализованных geometry/navigation: ADR-010. Новых зависимостей нет.

## Завершение Phase 0: Windows spikes

- [ ] Аудит фактических API, лицензий, MSRV и Windows-совместимости зависимостей
  до подключения GUI/PTY; решение по публичным ports/FFI оформить в ADR.
- [ ] Iced spike + настоящий terminal pane: portable-pty/ConPTY + alacritty_terminal.
- [ ] Bounded I/O/backpressure, child lifecycle/cancel/exit/resize, OSC security policy;
  GUI thread не выполняет blocking I/O.
- [ ] Рекурсивные panes и несколько OS windows с доступом к overflow canvas.
- [ ] Windows 11 matrix: PowerShell/cmd, IME, Unicode/wide/combining, интерактивные
  TUI, HiDPI 100/150/200%, multi-monitor и keyboard input.
- [ ] Воспроизводимые input latency/rendering измерения с описанием оборудования.
- [ ] Sysinfo/Bollard spike с disconnected/timeout сценариями вне UI thread.
- [ ] ADR выбора GUI по фактическим результатам, не по Linux-компиляции.

**Готово, когда:** результаты spike и ограничения воспроизводимо задокументированы,
Windows terminal/layout/multi-window сценарии проверены и выбор toolkit прошёл ревью.

## Phase 1: первый используемый MVP

Настоящий терминал, keyboard split/focus/move/resize/close, shortcuts с контекстами
и escape hatch, подтверждения paste/close, минимальное восстановление раскладки,
непрерывность PTY при перемещении, два часа soak и cancel/handle leak tests.

- [ ] Shell profiles и ошибки запуска без падения остального workspace.
- [ ] VT rendering/input, цвета, курсор, UTF-8/wide/combining, alternate screen,
  scrollback/лимиты, selection, clipboard, search, mouse и bracketed paste.
- [ ] Keyboard/pointer adapters к существующим typed commands, shortcut conflicts,
  контексты terminal/layout/dialog и escape hatch для передачи ввода в TUI.
- [ ] Видимый focus, overflow scroll/pan и раскрытие активной панели.
- [ ] Безопасные paste/close confirmations, resize debounce и clean shutdown.
- [ ] Сохранение layout и command configurations без обещания восстановления процессов.
- [ ] Реальные Windows smoke/soak/e2e: TUI, перенос terminal без restart, отмена,
  exit и отсутствие утечек handles; негативные сценарии.

**Готово, когда:** пользователь может работать в настоящем shell и управлять
вложенной раскладкой без мыши, а terminal lifecycle и recovery проверены на Windows.

## Остальные обязательные TILE/CUST

- TILE-004/005: shortcut registry, видимый focus, keyboard-only workflow и
  восстановление focus между окнами, вкладками и диалогами.
- TILE-006: drag/drop preview и отмена без side effects.
- TILE-007/008: полное восстановление, named presets, безопасный import/export.
- TILE-009/012 и CUST-001..009: appearance overrides, fonts/themes/density,
  module visibility, command palette, local settings/reload/fallback terminal.
- TILE-010/011: cross-window transfers, monitor recovery, DPI и debounced PTY resize.
- TILE-015: deterministic serializable event stream без секретов.
- TILE-013 остаётся P2 после надёжного базового дерева.

Docker, monitoring и tools добавляются как обычные независимые view instances,
включая несколько экземпляров в любой раскладке — не как фиксированные страницы.
Подписание, release packaging, NOTICE/third-party inventory, security/license
gates и performance baselines обязательны до релиза и пока не выполнены.

## После базового MVP

- [ ] **Phase 2 — workspace UX:** drag/drop с preview/cancel, detach/cross-window,
  named layouts/presets, themes/fonts/keymaps, настройки и безопасный reload,
  полноценное восстановление геометрии окон с monitor-disconnect recovery.
- [ ] **Phase 3 — monitoring:** shared scheduler и bounded histories, CPU/RAM/
  процессы, сортировка/фильтры, PID-reuse-safe завершение с подтверждением,
  `unsupported` для недоступных GPU metrics и профилирование.
- [ ] **Phase 4 — Docker:** explicit connection, disconnected/reconnect/timeout,
  containers/lifecycle/logs/exec, images/volumes/networks, bounded buffers и
  отдельные подтверждения destructive actions. Никакого автоматического prune.
- [ ] **Phase 5 — integrations/P2:** WSL discovery/cwd/argv, штатный SSH в PTY,
  opt-in Explorer integration, защищённый local IPC, Compose и tools согласно ТЗ.
- [ ] **Phase 6 — hardening/release:** accessibility, fuzz/property/integration/e2e,
  Windows matrix, reproducible performance/soak, cargo-audit/cargo-deny,
  license inventory/NOTICE, packaging, signing/provenance/checksums и release docs.

## Неподтверждённые проверки и инфраструктурный долг

- [ ] Выполнить настроенные Windows и MSRV CI jobs и зафиксировать результаты.
- [ ] Добавить обязательные security/license gates и отслеживание advisory findings.
- [ ] Провести ревью предложенных ADR-009/010 и зафиксировать решения.
- [ ] Не объявлять performance budgets достигнутыми без baseline/hardware и измерений.
- [ ] Перед интеграцией новых системных возможностей дополнить threat model,
  cancellation/failure tests и документацию ограничений.
