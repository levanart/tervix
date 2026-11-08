# Tervix

Windows-first терминал и настраиваемая tiling-среда разработчика на Rust 2024.
Требования и инженерные правила: [AGENTS.md](AGENTS.md).

## Текущее состояние: начало Phase 0

Это **не готовый терминал**: GUI, PTY, Docker и мониторинг ещё не реализованы.
Первый срез — независимое от ОС ядро рекурсивной раскладки:

- типизированные UUID окон, рабочих пространств, вкладок, панелей, view и сессий;
- горизонтальные/вертикальные вложенные split, resize, focus, swap и close;
- направленные focus/move/swap/resize с детерминированным выбором соседа;
- логическая геометрия с minimum sizes и явным overflow для маленьких окон;
- транзакционная валидация, уникальность pane/view ID, запрет закрытия последней панели;
- layout-only undo последних 32 изменений;
- история фокуса и возврат к последней посещённой доступной панели;
- unit/contract и property-based тесты, CI для Linux и Windows.

## Запуск

Требуется stable Rust; заявленный MSRV начального среза — 1.89 (проверка в CI).

```sh
cargo run -p tervix -- --layout-demo
```

Демонстрация строит дерево из трёх панелей, рассчитывает geometry и overflow,
выполняет направленные focus/move/resize и отменяет изменения раскладки.
Она не запускает shell, не меняет настройки и не обращается к Docker.

## Структура

- `crates/tervix-core` — I/O-free доменные типы и правила раскладки.
- `crates/tervix-application` — команды приложения и ограниченная история undo.
- `apps/tervix` — composition root, выделение UUID, headless-демонстрация.
- `docs/adr/009-initial-layout-foundation.md` — предложенные границы первого среза.
- `docs/adr/010-layout-geometry-navigation.md` — семантика geometry, navigation и focus history.
- [Состояние реализации](docs/status.md) — что сделано, 28 тестов, фактические проверки и ограничения.
- [Roadmap](docs/roadmap.md) — очередь оставшихся задач и критерии готовности, включая P0.

## Проверки

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace
cargo doc --workspace --no-deps
```

Linux-тесты ядра не подтверждают работоспособность ConPTY, IME или Iced на Windows.
Phase 0 завершится только после соответствующих spike и измерений.
