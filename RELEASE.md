# Выпуск Tervix

GitHub → **Actions → Release → Run workflow** → введите версию, например `0.2.0`
или `0.2.0-rc.1`. Один запуск проверяет проект, собирает всю release matrix,
формирует changelog и **создаёт GitHub Release с артефактами**. Дополнительного
`publish=true` или отдельного ручного шага публикации нет.

`prerelease=true` по умолчанию. Для версии с суффиксом (`-rc.1`, `-beta.1`)
prerelease включается автоматически даже при снятом флажке. Для обычного релиза
используйте версию без суффикса и снимите `prerelease`.

## Runner и предварительная подготовка

Все jobs используют **`[self-hosted, Linux, X64]`**. Достаточно одного runner:
build/check/test и обе cross-сборки выполняются последовательно, затем тот же
runner выполняет publish job. GitHub-hosted runners, Wine и Windows VM не нужны.

Администратор runner заранее устанавливает:

- Git, Bash, Python **3.11+**, Rustup и GitHub CLI (`gh`);
- полный LLVM/Clang toolchain: `clang`, `clang-cl`, `lld-link`, `llvm-rc`, плюс
  `pkg-config` и обычные Linux build tools;
- системные библиотеки для сборки Iced на Linux (например, libxkbcommon/Wayland
  development packages) и Vulkan loader + Mesa/lavapipe либо другой headless
  WGPU adapter для тестов. Графическое окно тесты не открывают.

Workflow проверяет команды в PATH и **не выполняет `sudo`/`apt`**. Он устанавливает
Rust 1.89.0 и оба Windows Rust targets в профиль пользователя runner, а
**cargo-xwin 0.23.0** — в уникальный job-local каталог. cargo-xwin (MIT, MSRV 1.89)
использует LLVM и загружает Microsoft CRT/Windows SDK; их условия описаны в
[upstream](https://github.com/rust-cross/cargo-xwin/tree/v0.23.0).
Нужен сетевой доступ к crates.io, GitHub и серверам Microsoft. Входить через
`gh auth login` не требуется: publish job использует временный `GITHUB_TOKEN`.

CI также направлен на этот runner. Fork PR не запускает задания на self-hosted
машине; trusted push и PR из того же репозитория проходят обычные checks.

## Платформы

| Артефакт | Target | Runner | Статус |
| --- | --- | --- | --- |
| `tervix-v<VERSION>-windows-x86_64.zip` | `x86_64-pc-windows-msvc` | Linux X64 / cargo-xwin | Основная сборка Windows 11 x64 |
| `tervix-v<VERSION>-windows-arm64.zip` | `aarch64-pc-windows-msvc` | Linux X64 / cargo-xwin | Экспериментальная, нужна проверка на устройстве |

Это все Windows-архитектуры, включённые в текущую release matrix. Linux/macOS
не публикуются: `tervix-terminal` явно отклоняет запуск локального терминала вне
Windows, а Windows chrome/resize и Docker transport требуют портирования.
Компиляция отдельных crates на Linux не равна поддержке приложения.

## Что делает workflow

1. Проверяет SemVer и отсутствие tag `v<VERSION>`. Существующий tag не заменяется.
2. Применяет введённую версию к workspace packages, локальным dependency
   requirements и соответствующим Cargo.lock entries **только в checkout сборки**.
   Исходный Cargo.toml не нужно редактировать перед каждым запуском.
3. Генерирует `CHANGELOG.md` по non-merge commits от ближайшего `v*` tag-предка:
   Features (`feat`), Fixes (`fix`), Performance (`perf`), Other changes.
   Для первого релиза берётся вся история. В notes указан точный source commit.
4. На Linux X64 выполняет fmt, clippy, Linux-hosted headless tests и docs, затем
   последовательно `cargo xwin check/build` для обеих Windows-архитектур.
   Windows ConPTY/named-pipe tests на Linux не выполняются; это отдельная native
   acceptance, и cross-компиляция не объявляется её заменой.
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
