# Выпуск Tervix

GitHub → **Actions → Release → Run workflow** → введите версию, например `0.2.0`
или `0.2.0-rc.1`. Снимите флажок `publish`, чтобы проверить сборки без создания тега и релиза.

`prerelease=true` по умолчанию. Для версии с суффиксом (`-rc.1`, `-beta.1`)
prerelease включается автоматически даже при снятом флажке. Для обычного релиза
используйте версию без суффикса и снимите `prerelease`.

## Runner

Все задания выполняются на self-hosted Linux x64 runner с Docker.
Контейнеры устанавливают Rust targets, LLVM, cargo-xwin и тестовые зависимости
без установки пакетов на хост. Windows собирается через cargo-xwin на Linux.
CI запускается при push в main и для PR из этого репозитория. PR из форков
пропускаются, поскольку runner постоянный и имеет доступ к Docker хоста.

## Артефакты

- `tervix-v<VERSION>-x86_64-pc-windows-msvc.exe` для Windows x64.
- `tervix-v<VERSION>-aarch64-pc-windows-msvc.exe` для Windows ARM64 (экспериментально).
- `tervix-v<VERSION>-linux-x86_64` для Linux x64 с совместимой glibc.
  После скачивания выполните `chmod +x <файл>`. Запуск локального терминала в Linux пока недоступен.

GitHub автоматически добавляет ссылки на исходники тега.

## Что делает workflow

1. Проверяет версию и применяет её к Cargo.toml и Cargo.lock в checkout сборки.
2. Собирает Linux x64, запускает `--layout-demo` и кросс-компилирует Windows x64 и ARM64.
3. После успеха всех сборок прикладывает три бинарника к GitHub Release.
   Тег указывает на исходный коммит. Для существующего тега публикация останавливается.

## Локальная проверка без публикации

```powershell
python -m unittest discover -s scripts -p "test_*.py"
# Показать будущий changelog; файлы проекта не меняются:
python scripts/prepare_release.py --version 0.2.0
```

`--apply` меняет Cargo.toml и Cargo.lock и пишет CHANGELOG.md. Используйте его в отдельном checkout.

## Перед запуском публикации

- Проверьте состав commit: локальные untracked файлы в GitHub build не попадут.
- Проверьте redistribution obligations, включая upstream NOTICE и embedded fonts.
  Автосбор LICENSE файлов и JSON-инвентарь не заменяют юридический review или SBOM.
- Выполните vulnerability/license audit. Cargo-audit/cargo-deny пока не настроены
  как обязательные gates; список открытых вопросов находится в REVIEW.md.
- Проверьте реальные терминалы, WSL, Docker, GPU и ARM64 на соответствующих системах.
  Headless tests и успешная компиляция не заменяют native acceptance.

Артефакты пока не подписаны Authenticode, установщика/автообновления нет.
Настройки/layout session-only;
полная IME/HiDPI/accessibility/TUI и performance/soak матрица остаётся незавершённой.
