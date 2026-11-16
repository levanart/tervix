# Tervix preview

Нативная tiling-среда для Windows 11 x64: PowerShell/cmd/WSL/SSH, мониторинг
CPU/RAM и процессов, Docker containers/logs/inspect/exec, RU/EN и GPU renderer.

Это preview, не стабильный 1.0. Настройки и раскладка пока не сохраняются;
полная матрица совместимости и performance budgets не подтверждены.

Распакуйте ZIP и запустите `tervix.exe`. Для PowerShell 7, WSL, OpenSSH и Docker
нужны соответствующие установленные компоненты. Docker подключается явно.
Закрытие Docker exec означает detach; удалённый shell может продолжить работу.

Артефакты не подписаны Authenticode. Сверьте SHA256 с `SHA256SUMS.txt`.
Подробности: README.md, RELEASE.md и SECURITY.md в репозитории.
