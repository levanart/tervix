# Состояние реализации Tervix

Обновлено: **2026-11-08**. Срез: **headless-фундамент Phase 0**.

Этот документ описывает фактический код, а не заменяет спецификацию
[AGENTS.md](../AGENTS.md). План дальнейшей работы — [roadmap.md](roadmap.md).

**Tervix пока не является работающим графическим терминалом.** Есть проверяемое
ядро tiling и консольная демонстрация. Phase 0 не завершена; Terminal MVP ещё нет.

## Что уже сделано

### 1. Структура и инженерная основа

- Cargo workspace, resolver 3, Rust 2024, общий набор зависимостей и Cargo.lock.
- Три crates с зависимостями `tervix → tervix-application → tervix-core`.
- Core не обращается к ОС, файловой системе, GUI, PTY или Docker.
- UUID генерируются на границе executable, а не внутри доменных операций.
- Настроены rustfmt, Clippy и запрет unsafe в собственном коде workspace.
- Заявлен MSRV 1.89; фактический локальный прогон выполнен на Rust 1.99.1.

### 2. Рекурсивная раскладка

- Отдельные типы `WindowId`, `WorkspaceId`, `TabId`, `PaneId`, `ViewId`, `SessionId`.
  Наличие ID **не означает**, что агрегаты окон/вкладок или PTY уже реализованы.
- Бинарное дерево `Pane | Split`, без фиксированной сетки или числа колонок.
- Панель хранит независимую пару pane/view ID; runtime handles в дереве нет.
- Split по горизонтали/вертикали, focus по ID, swap, close и resize parent split.
- Проверки уникальности pane/view ID, достижимости focus и непустого дерева.
- Последнюю панель закрыть нельзя; после close родительский split схлопывается.
- Ratio 1000..=9000 basis points (10–90%), без float/NaN.
- Защитные лимиты: до 256 панелей и 32 рёбер от корня до листа.
- Команды транзакционные: ошибочный кандидат не меняет существующую раскладку.

### 3. Геометрия и направленное управление

- Расчёт прямоугольников всех панелей в положительных целых логических единицах.
- Минимумы поддеревьев и clamp размеров с учётом обоих детей split.
- Если окно слишком мало, canvas расширяется до допустимого размера и возвращает
  явные overflow flags. Панели не удаляются и не становятся нулевыми.
- Полное покрытие canvas без перекрытий и зазоров, в том числе на нечётных размерах.
- Переполнение координат возвращает типизированную ошибку.
- Геометрия не переписывает ratios: при увеличении viewport возвращаются
  запрошенные пропорции, насколько позволяют minimum sizes.
- Направленные focus/swap/move с детерминированным выбором соседа, без wrap.
- Move структурно переносит существующую панель на сторону target, сохраняя её
  pane/view ID; swap меняет местами целые панели без изменения topology split.
- Направленный resize ближайшей подходящей ancestor boundary, с bounded step
  и clamp ratio. Это доменные команды, **не подключённые горячие клавиши**.

### 4. Application state и история

- `LayoutController` управляет раскладкой одного будущего tab.
- Layout-only undo последних 32 изменений; focus-only и no-op не занимают undo.
- MRU-история до 32 уникальных доступных панелей, без текущего focus.
- `FocusPrevious`, возврат к недавней доступной панели после закрытия активной.
- Закрытые ID удаляются из MRU; undo восстанавливает layout/focus и согласует MRU.
- Ошибочные команды не меняют layout, undo и историю фокуса.

### 5. Демонстрация, тесты и CI

```sh
cargo run -p tervix -- --layout-demo
```

Демо создаёт три панели, выводит geometry/overflow, выполняет directional actions
и undo. Shell не запускается, файлы настроек и внешние сервисы не изменяются.

В workspace **28 тестов**:

| Файл | Количество | Основное покрытие |
| --- | ---: | --- |
| `crates/tervix-core/tests/layout.rs` | 7 | Split/close/focus, swap, resize, ошибки, depth limit, модель порядка панелей |
| `crates/tervix-core/tests/geometry.rs` | 6 | Exact rectangles, minimum sizes, overflow, крайние координаты, property-based покрытие canvas |
| `crates/tervix-core/tests/navigation.rs` | 7 | Выбор соседей, move/swap, ancestor resize, edges, атомарность, сохранение identity |
| `crates/tervix-application/tests/commands.rs` | 8 | Bounded undo/MRU, close/restore, ошибки и смешанные случайные последовательности |

Настроен `.github/workflows/ci.yml`: stable quality gates и демо на Linux/Windows,
MSRV 1.89 check/test на Linux. **Результаты удалённых CI jobs не проверены.**

## Какие проверки действительно выполнены

Последний локальный прогон реализации: Linux, Rust/Cargo 1.99.1.
Ниже зафиксированы результаты этого прогона, не утверждение о каждом будущем commit.

| Команда | Результат |
| --- | --- |
| `cargo fmt --all -- --check` | Успешно |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Успешно |
| `cargo test --workspace` | 28 passed, 0 failed |
| `cargo check --workspace` | Успешно |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Успешно |
| `cargo run -p tervix -- --layout-demo` | Успешно |
| `git diff --check` | Успешно |

Не выполнены локально: MSRV (rustup отсутствует), Windows build/test,
ConPTY/GUI/WSL/Docker integration/e2e, performance/soak, полный cargo-audit/
cargo-deny и release license inventory. Metadata зависимостей изучены, но это
не заменяет полноценный security/license audit.

## Что пока отсутствует или ограничено

- GUI/Iced, native windows, tabs/workspaces как агрегаты, view registry.
- Реальный terminal pane, PowerShell/cmd/WSL profiles, PTY/ConPTY, VT emulator.
- Сериализация, config schema/migrations, сохранение/восстановление раскладки.
- Shortcut registry и обработчики клавиатуры/мыши, command palette, drag/drop.
- Визуальный focus, scroll/pan overflow и автоматическое раскрытие focused pane.
  Сейчас есть только геометрический контракт; доступность за пределами viewport
  ещё нужно реализовать и проверить в UI.
- Minimum size пока общий для всех tile, без per-view constraints и учёта
  реальных font/header/border metrics. DPI и terminal cell sizing ещё отсутствуют.
- MRU ограничен одним controller: нет восстановления focus между окнами,
  вкладками, диалогами или после перезапуска.
- Undo восстанавливает **только layout/ID**, не процессы и не живые view resources.
  Перед PTY нужен registry reconciliation и явное поведение undo закрытой сессии.
- Нет monitoring, Docker, OS integrations, customization или release packaging.
- Linux property-based тесты не подтверждают session continuity реальных PTY,
  Windows compatibility, accessibility или достижение performance budgets.

## Где продолжать

| Область | Реализация / решение |
| --- | --- |
| Domain IDs и layout | `crates/tervix-core/src/ids.rs`, `layout.rs` |
| Geometry и navigation | `crates/tervix-core/src/geometry.rs`, `navigation.rs` |
| Undo, MRU и dispatch | `crates/tervix-application/src/lib.rs` |
| Headless composition root | `apps/tervix/src/main.rs`, `bootstrap.rs` |
| Начальные границы | [ADR-009](adr/009-initial-layout-foundation.md), статус «предложено» |
| Семантика geometry/navigation | [ADR-010](adr/010-layout-geometry-navigation.md), статус «предложено» |
| Очередь работ и критерии | [roadmap.md](roadmap.md) |

При изменении реализации обновлять эту сводку, фактические проверки и roadmap.
Не помечать TILE/TERM требования целиком выполненными по наличию headless API.
