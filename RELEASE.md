# Выпуск Tervix

GitHub → **Actions → Release → Run workflow** → введите версию, например `0.2.0`
или `0.2.0-rc.1`. Один запуск проверяет проект, собирает всю release matrix,
формирует changelog и **создаёт GitHub Release с артефактами**. Снимите флажок
`publish`, чтобы собрать и проверить архивы без создания тега и публикации.

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

- Windows x64 и ARM64: отдельные файлы `.exe` и portable ZIP.
- Linux x64: отдельный ELF для систем с совместимой glibc. После скачивания
  выполните `chmod +x <файл>`. Запуск локального терминала в Linux пока недоступен.
- Архив исходников `tervix-v<VERSION>-source.tar.gz` из выбранного коммита.
- `SHA256SUMS.txt` для ZIP, отдельных бинарников и исходников.

## Что делает workflow

1. Проверяет SemVer и отсутствие tag `v<VERSION>`. Существующий tag не заменяется.
2. Применяет введённую версию к workspace packages, локальным dependency
   requirements и соответствующим Cargo.lock entries **только в checkout сборки**.
   Исходный Cargo.toml не нужно редактировать перед каждым запуском.
3. Генерирует `CHANGELOG.md` по non-merge commits от ближайшего `v*` tag-предка:
   Features (`feat`), Fixes (`fix`), Performance (`perf`), Other changes.
   Для первого релиза берётся вся история. В notes указан точный source commit.
4. Собирает Linux x64, запускает `--layout-demo` и кросс-компилирует Windows x64 и ARM64.
5. В каждом ZIP: exe, лицензия проекта, документация, changelog, инвентарь
   зависимостей, найденные тексты лицензий, BUILD.json и точные Cargo.toml/lock
   сборки. Проверяется PE-архитектура: x64 exe нельзя выдать за ARM64.
6. После **успеха всей matrix** проверяет checksum каждого ZIP, наличие changelog
   и target metadata, создаёт общий `SHA256SUMS.txt`.
7. Создаёт tag на выбранном source commit и опубликованный Release (не draft)
   с generated changelog в описании и отдельном файле, всеми ZIP, checksums и
   отдельными BUILD metadata.

Если одна платформа не собралась, Release не создаётся. Успешные промежуточные
артефакты остаются доступны в Actions для диагностики. Workflow запускается только
вручную: push/tag сами по себе ничего не публикуют.

Версия сборки может отличаться от manifest в исходном tag: workflow не создаёт
автоматический commit с version bump и не меняет ветку. В ZIP сохранены точные
   versioned build inputs для воспроизведения. `dirty=true` в BUILD.json отражает
эту намеренную модификацию checkout, а не новую исходную ветку.

## Локальная проверка без публикации

```powershell
python -m unittest discover -s scripts -p "test_*.py"
# Показать будущий changelog; файлы проекта не меняются:
python scripts/prepare_release.py --version 0.2.0
```

`--apply` у prepare script меняет Cargo.toml/lock и пишет CHANGELOG.md; используйте
его в отдельном checkout. Packaging требует совпадения заданной версии с этим
versioned workspace. `verify_release.py` требует обе платформы и формирует общий
checksum-файл; packaging каждой платформы пишет собственный `.zip.sha256`.

## Перед запуском публикации

- Проверьте состав commit: локальные untracked файлы в GitHub build не попадут.
- Проверьте redistribution obligations, включая upstream NOTICE и embedded fonts.
  Автосбор LICENSE файлов и JSON-инвентарь не заменяют юридический review или SBOM.
- Выполните vulnerability/license audit. Cargo-audit/cargo-deny пока не настроены
  как обязательные gates; список открытых вопросов находится в REVIEW.md.
- Проверьте реальные терминалы, WSL, Docker, GPU и ARM64 на соответствующих системах.
  Headless tests и успешная компиляция не заменяют native acceptance.

Артефакты пока не подписаны Authenticode, установщика/автообновления нет.
SHA256 — проверка целостности, не цифровая подпись. Настройки/layout session-only;
полная IME/HiDPI/accessibility/TUI и performance/soak матрица остаётся незавершённой.
