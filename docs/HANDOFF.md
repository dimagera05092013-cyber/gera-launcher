# GERA Launcher — контекст и история работы

Этот документ — передача контекста в новую сессию. Прочитай его целиком, прежде
чем что-то делать: здесь цель пользователя, правила общения, текущее состояние
проекта и что уже проверено.

Дата последнего обновления: 18 ноября 2026.

---

## 1. Кто пользователь и чего он хочет

- Пользователь — **Gera**, живёт на **Omarchy Linux** (Arch, Hyprland с Lua-конфигом).
- Общается по-русски. Требует: кратко, по делу, сразу выполнять, не задавать
  лишних вопросов, не выдумывать данные, проверять результат.
- Не любит официальный лаунчер Minecraft и не хочет ставить его на второй ПК.
- Главная текущая цель: **запустить GERA Launcher на втором, чистом ПК с Windows
  и играть в Minecraft (Horizon) без официального лаунчера**, перенеся всё одним
  zip-архивом.
- Глобальные правила (из `~/.config/opencode/AGENTS.md`): русский язык, краткость,
  не усложнять, опасные действия — с подтверждением, файлы игры не раздавать,
  лицензию Minecraft не обходить, настройки системы/провайдера не менять без просьбы.

## 2. Что такое GERA Launcher

Независимый лаунчер для **Horizon** — Minecraft: Java Edition 26.3 на Rust.
Стек: **Rust + Tauri 2 + Svelte 5 + TypeScript**.

Умеет:
- устанавливать официальные сборки `horizon-client` с GitHub (SHA-256, распаковка);
- готовить ресурсы из легальной копии Minecraft через штатный `horizon-prepare`;
- импортировать готовую папку ресурсов с другого ПК (папка или zip);
- запускать `horizon-client` с игровой папкой, ником и скином;
- профили/сборки с отдельными игровыми папками, дублирование;
- каталог Horizon Mods Registry, установка `.hmod` с проверкой SHA-256;
- показывать встроенные нативные моды клиента (AppleSkin, Minimap, Zoom и др.);
- собирать нативные `.hnmod` (Rust, horizon-mixins, cargo build);
- Material You интерфейс, темы, лёгкий/тяжёлый режимы;
- **Minecraft Java Edition** (`vanilla.rs`, страница «Java»): установка официальных
  версий Mojang (клиент, библиотеки, нативы, assets), системная Java или
  скачивание Temurin JRE, запуск офлайн-ником.

Проект — git-репозиторий `https://github.com/gera2347/gera-launcher`; релизы
собирает GitHub Actions, лаунчер обновляется по воздуху.

Пути:
- проект: `/home/gera/gera-launcher`
- данные: `~/.local/share/gera-launcher`
- состояние: `~/.local/share/gera-launcher/state.json`
- логи сборок: `~/.local/share/gera-launcher/logs`
- установленный Linux-бинарник: `~/.local/bin/gera-launcher`

## 3. Текущее состояние (что сделано)

### 3.1 Моды восстановлены
- Профиль `default` («Основной») переключён на прямой клиент
  `nightly-2026-10-31-9d979ca` — в нём полный официальный набор нативных модов.
- Из бэкапов восстановлены `.hmod`: `compass_bar-0.1.0.hmod`, `factory_blocks.hmod`
  в `~/.local/share/horizon/horizon-mods/`.
- Добавлен блок «Встроенные моды» на странице «Моды» (команда `mods_builtin`,
  тип `BuiltinMod`). Читает `horizon-mods.txt` (`mod.<id>:true|false`).
- Главная считает моды: 9 встроенных + 2 `.hmod` = **11**.
- Тест `mods::builtin_tests::reads_builtin_mod_switches` проходит.

### 3.2 Порт на Windows (сделан)
- Новый модуль `src-tauri/src/sysutil.rs`:
  - распаковка `.tar.gz` (`flate2` + `tar`) и `.zip` (`zip`);
  - свободное место (`df` на Linux, `GetDiskFreeSpaceExW` на Windows);
  - открытие файлов/ссылок (`xdg-open` / `explorer`);
  - завершение процессов (`kill -KILL` / `taskkill /T /F`);
  - проверка живости процесса (`/proc` / `OpenProcess`);
  - ссылки на папки (`symlink` / junction через `mklink /J`);
  - `quiet()` — на Windows добавляет `CREATE_NO_WINDOW`, чтобы PowerShell, curl,
    cargo, `cmd` и сама игра не открывали чёрные консольные окна;
  - `plain_path()` — убирает префикс `\\?\` у путей, иначе `mklink`/`explorer`
    их не понимают (важно при дублировании профилей).
- Найден и исправлен **критичный баг**: `gera-launcher.exe` импортирует
  `WebView2Loader.dll`, а в архивах его не было — на чистой Windows лаунчер
  вообще не стартовал (ошибка 0xc0000135). Теперь DLL лежит рядом с exe в
  `release/windows/` и в обоих zip, в инструкциях про это написано.
- Тёмная тема: на Windows заголовок окна теперь следует теме GERA
  (`apply_window_theme`, `Window::set_theme`). Ошибки PowerShell с BOM
  (`\u{feff}`) больше не ломают разбор списка запущенных игр.
- Установка клиента: на Windows качается `horizon-windows-x64.zip`, проверяется
  SHA-256, распаковывается и разворачивается из вложенной папки. На Linux —
  `horizon-linux-x86_64.tar.gz`.
- Учтены `.exe`-имена: `horizon-client.exe`, `horizon-prepare.exe`,
  `horizon-mixins.exe`.
- Пути: `%LOCALAPPDATA%\gera-launcher`, домашняя папка через `USERPROFILE`.
- Обнаружение запущенной игры на Windows — PowerShell CIM
  (`Get-CimInstance Win32_Process`), отмена сборки — `taskkill`.
- Нативная сборка на Windows связывает ресурсы junction-ссылкой.
- `unzip` больше не требуется (архивы распаковывает сам GERA).
- Зависимости: `flate2`, `tar`, `zip`, `windows-sys` (только Windows).
- Проверки: `cargo check` Linux/Windows — ок; `npm run check` — 0 ошибок;
  `cargo test --lib` — 3 теста на Linux; **6 тестов на Windows-таргете под Wine**
  (распаковка tar.gz/zip, `plain_path`, `GetDiskFreeSpaceExW`, `OpenProcess`).
- Документация обновлена: `docs/GUIDE.md`, `README.md` (раздел Windows).

### 3.2.1 Как проверялась Windows-сборка без Windows
Windows на второй машине не ставится, поэтому проверка шла так:
- `cargo test --lib --target x86_64-pc-windows-gnu` с `CARGO_TARGET_..._RUNNER=wine`
  и `WINEPATH`, указывающим на папку с `WebView2Loader.dll` — 6/6 тестов ОК,
  реально вызывались Windows-API (`GetDiskFreeSpaceExW`, `OpenProcess`).
- Headless-режим под Wine: `gera-launcher.exe --import-resources <версия> <папка>`
  — код возврата 0, ресурсы скопировались в
  `C:\users\<user>\AppData\Local\gera-launcher\clients\...\.local\horizon-client\26.3`.
  Это проверяет пути `%LOCALAPPDATA%`, `copy_tree`, CLI.
- GUI под Wine: окно «GERA Launcher» создаётся, приложение доходит до WebView2 и
  показывает системное окно «Could not find the WebView2 Runtime» — только потому,
  что в Wine нет Edge WebView2. На Windows 10/11 рантайм есть из коробки.
  Полноценный интерфейс под Wine не отрисовывается.


### 3.2.2 Один exe для Windows (MSVC, сделано 17.11.2026)
- Собран `x86_64-pc-windows-msvc` без `WebView2Loader.dll`: на MSVC
  `webview2-com-sys` линкует `WebView2LoaderStatic.lib` прямо в exe.
- `tauri-build` (через `static_vcruntime`) статически линкует `libcmt` и
  `libvcruntime`, а UCRT остаётся системным (`ucrtbase.dll`, входит в Win10/11).
  Поэтому `+crt-static` добавлять **не надо** — с ним линковка ломается на
  `NODEFAULTLIB:libucrt.lib`.
- Кросс-сборка с Linux (MSVC): `rustup target add x86_64-pc-windows-msvc`,
  `cargo install cargo-xwin`, clang 23.1.5 (пакеты Arch `clang` + `clang-libs`,
  распакованы в `~/.local/share/gera-launcher/toolchains/clang-23.1.5`),
  системные `lld-link` и `llvm-rc`.
- Команда (сборка только exe, без установщика):
```sh
cd ~/gera-launcher
npm run build
cd src-tauri
export PATH="$HOME/.local/share/gera-launcher/toolchains/clang-23.1.5/usr/bin:$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.local/share/gera-launcher/toolchains/clang-23.1.5/usr/lib"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo xwin build --release --target x86_64-pc-windows-msvc \
  --features tauri/custom-protocol --bin gera-launcher
```
  `--features tauri/custom-protocol` обязателен: иначе exe открывает
  `localhost:1420`. `--bin` не спасает от линковки cdylib-цели, но с MSVC она
  собирается нормально (если включать `+crt-static`, сборка падает).
- Проверка импортов: `llvm-objdump -p gera-launcher.exe | grep "DLL Name"` —
  только системные DLL, без `WebView2Loader.dll` и `vcruntime140.dll`.
- Проверка под Wine: `gera-launcher.exe --import-resources <версия> <ресурсы>`
  копирует файлы и возвращает 0; GUI доходит до окна WebView2 (рантайма в Wine
  нет). Полноценно GUI проверяется только на Windows.
- Размер: **4,7 МБ**, один файл. Артефакт:
  `~/gera-launcher/release/windows/gera-launcher.exe`.
- Старый GNU-вариант с DLL остался в `release/windows/legacy/` и в полном zip.

### 3.3 Windows-артефакты
- `~/gera-launcher/release/windows/gera-launcher.exe` — **4,7 МБ**, PE32+ x86-64,
  один самодостаточный файл (MSVC, см. 3.2.2). DLL не нужна.
- `~/gera-launcher/release/windows/gera-launcher-windows.zip` — 1,9 МБ:
  только `gera-launcher.exe` и `КАК ЗАПУСТИТЬ.txt`.
- `~/gera-launcher/release/windows/gera-launcher-windows-full.zip` — **606 МБ**
  (старая GNU-сборка): exe + `WebView2Loader.dll` + `import/26.3/` (ресурсы,
  686 МБ распакованных) + `ИНСТРУКЦИЯ.txt`. Для нового одиночного exe DLL не
  требуется; при желании full-zip можно пересобрать с новым exe.
- `~/gera-launcher/release/windows/legacy/WebView2Loader.dll` — 161 КБ, нужен
  только старым GNU-сборкам.
- Ресурсы взяты из
  `~/.local/share/horizon-launcher/versions/nightly-2026-10-31-9d979ca/.local/horizon-client/26.3`
  (это ресурсы с копии пользователя; файлы игры никуда не публикуются).

Инструкция для второго ПК (лежит в архиве):
1. Распаковать архив целиком.
2. Папку `import` скопировать в `%LOCALAPPDATA%\gera-launcher\`.
3. Запустить `gera-launcher.exe` (SmartScreen → «Подробнее» → «Выполнить всё равно»).
4. Настройки → Обновления → установить сборку `nightly` (~160 МБ клиент под Windows).
5. Главная → «Играть» — ресурсы подхватятся из `import`.

Официальный лаунчер, Java и `.minecraft` на втором ПК не нужны. Нужны только
Windows 10/11 x64 и интернет при первом запуске.

### 3.4 Как собирать
Linux:
```sh
cd ~/gera-launcher
npm run tauri build -- --no-bundle
install -m 755 src-tauri/target/release/gera-launcher ~/.local/bin/gera-launcher
```
Важно: `cargo build --release` напрямую **нельзя** — без фичи `custom-protocol`
бинарник при запуске открывает `http://localhost:1420` и показывает
«Could not connect to localhost». Собирать только через `npm run tauri build`.

Windows (кросс-сборка с Linux, актуальный способ — MSVC, см. 3.2.2):
```sh
cd ~/gera-launcher
npm run build
cd src-tauri
export PATH="$HOME/.local/share/gera-launcher/toolchains/clang-23.1.5/usr/bin:$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.local/share/gera-launcher/toolchains/clang-23.1.5/usr/lib"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
cargo xwin build --release --target x86_64-pc-windows-msvc \
  --features tauri/custom-protocol --bin gera-launcher
```
Бинарник: `src-tauri/target/x86_64-pc-windows-msvc/release/gera-launcher.exe`.
Один файл, `WebView2Loader.dll` больше не нужна.

Старый способ (GNU, оставлен для совместимости; требует DLL рядом):
```sh
cd ~/gera-launcher
export PATH="$HOME/.local/share/gera-launcher/toolchains/llvm-mingw-20261104-ucrt-ubuntu-22.04-x86_64/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
npm run tauri build -- --target x86_64-pc-windows-gnu --no-bundle
```
Бинарник: `src-tauri/target/x86_64-pc-windows-gnu/release/gera-launcher.exe`.

Проверки:
```sh
npm run check
cd src-tauri && cargo test --lib
```
Тесты Windows-таргета без Windows (под Wine):
```sh
cd src-tauri
export PATH="$HOME/.local/share/gera-launcher/toolchains/llvm-mingw-20261104-ucrt-ubuntu-22.04-x86_64/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
export WINEPREFIX=/tmp/opencode/wineprefix
export WINEPATH='Z:\home\gera\gera-launcher\src-tauri\target\x86_64-pc-windows-gnu\release'
cargo test --lib --target x86_64-pc-windows-gnu
```

### 3.5 Закрытие лаунчера при запуске игры
- В `Settings` добавлено поле `close_on_launch` (serde default = `false`,
  camelCase `closeOnLaunch` во фронтенде).
- В `launch_game` (commands.rs) при включённой настройке окно `main`
  скрывается сразу после `spawn()`, а через 1,2 с закрывается. Игровой процесс
  живёт отдельно и продолжает работать; лог запуска уже пишется в файл.
- Переключатель: «Настройки → Игровые настройки → Закрывать лаунчер при
  запуске игры». У пользователя включён.

### 3.6 Почему «чёрный экран» при запуске игры
Замеры на машине пользователя (2 потока, ~3 ГБ RAM), `Horizon: core-ready`:
- холодный кэш `~/.local/share/horizon/.horizon-cache` — **21 с** (лог из
  `~/.local/share/gera-launcher/logs/launch-1794805913859-default.log`);
- тёплый кэш, запуск из лаунчера — **8,2 с**;
- тёплый кэш, прямой запуск `horizon-play` — **10,7 с**.
То есть лаунчер сам по себе не замедляет игру: 21 с было один раз после
пересоздания кэша (подготовка ресурсов). Второй замер шёл при открытом окне
лаунчера и оказался даже быстрее — конкуренция за CPU незначима.
Реально ускоряют: тёплый кэш (не запускать «Подготовить» без нужды) и
закрытие лаунчера (освобождает память на 3 ГБ машине).

### 3.7 Раздел «Серверы» (сделан)
- Новая страница `Серверы` в боковом меню (`PageId = "servers"`).
- Карточка: баннер сверху, затем иконка / название + сборка / адрес + статус +
  игроки. Клик по карточке запускает привязанную сборку с адресом сервера,
  кнопка «⋮» открывает отдельное меню (Подключиться, Изменить, Закрепить,
  Обновить статус, Удалить).
- Сортировка: вручную, по онлайну, по названию, по состоянию. Ручной порядок
  перетаскивается за ручку и сохраняется. Закреплённые карточки всегда выше.
- Добавление и правка: название, адрес, порт, описание, сборка, баннер, иконка,
  текст/цвет заглушки. Баннер и иконка выбираются файлом (PNG/JPEG/WebP/GIF,
  до 4 МБ) или берутся с сервера через Server List Ping. Ошибки полей
  показываются в диалоге.
- Импорт из Horizon: читается `<game dir>/servers.dat` (NBT, обычный или gzip),
  иконки из поля `icon` (base64 PNG). Исходный файл только читается; дубликаты
  по паре адрес+порт пропускаются, существующие карточки не перезаписываются.
- Сборка у сервера хранится по стабильному `profileId`. Если сборку удалили,
  карточка честно сообщает об этом, а запуск не подменяется другой сборкой.
- Запуск: `horizon-client … --server HOST[:PORT]` (штатный флаг Horizon,
  он же `--quickPlayMultiplayer`). Используются те же аргументы, что у обычного
  запуска (`--resources`, `--game-dir`, `--home`, `--name`).
- Статус: настоящий Server List Ping (TCP + VarInt), асинхронно через
  `spawn_blocking`, с таймаутами и ограничением частоты. Показываются
  online/offline, игроки, задержка и MOTD; «онлайн» никогда не выдумывается.
- Хранение: `state.json` → `servers: []` (`id`, `name`, `host`, `port`,
  `description`, `banner`, `icon`, `fallbackText`, `fallbackColor`,
  `profileId`, `pinned`, `position`, `createdAt`, `lastPlayed`). Картинки —
  отдельные файлы `<data>/servers/<id>-banner|icon.<ext>`, в JSON только имя.
- Команды: `server_save`, `server_remove`, `server_set_image`,
  `server_clear_image`, `server_images`, `server_import_scan`,
  `server_import_add`, `server_reorder`, `server_pin`, `server_set_profile`,
  `server_status`, `launch_server`.
- Проверки: `cargo test --lib` — 12 тестов, включая разбор настоящего
  `servers.dat`, SLP-ответ, round-trip картинок, дедупликацию, порядок,
  закрепление, сохранение после перезапуска и запуск с `--server`.
  `npm run check` — 0 ошибок, 0 предупреждений.
- Живой прогон: импортированы 6 серверов из Horizon (иконки 64×64 на диске),
  карточки показывают реальный онлайн/игроков/задержку/MOTD; закрепление,
  сортировка, правка и сохранение после перезапуска проверены. Один запуск
  через карточку реально стартовал Horizon с `--server 203.0.113.10:25565`.
- Исправлено по ходу проверки: Enter на кнопке «⋮» больше не запускает игру
  (событие не всплывает в карточку), меню забирает фокус и закрывается по Esc,
  диалоги забирают фокус, поля в диалоге не сливаются с фоном.

### 3.8 Автообновление через GitHub Releases (сделано 17.11.2026)

Код публикуется в открытом репозитории GitHub (`gera-launcher`). Сборка и
публикация — через GitHub Actions.

- `src-tauri/src/update.rs` — проверка последнего релиза через
  `api.github.com/repos/<owner>/gera-launcher/releases/latest`, сравнение
  версий, скачивание ассета, SHA-256, замена файла и перезапуск.
- Константа `REPO` в `update.rs` — `gera2347/gera-launcher`.
  `GERA_UPDATE_REPO` в окружении переопределяет её (для отладки),
  `GERA_UPDATE_MOCK=0.9.1` показывает окно обновления без сети.
- Ассеты релиза: `gera-launcher.exe` + `.sha256` (Windows),
  `gera-launcher-linux` + `.sha256` (Linux). Без контрольной суммы обновление
  отменяется.
- Windows: работающий exe нельзя перезаписать, но можно переименовать —
  текущий файл становится `gera-launcher.exe.old`, новый копируется на его
  место, GERA запускается заново. `.old` удаляется при следующем старте.
- Linux: новый файл копируется рядом и подменяет текущий через `rename`,
  затем процесс перезапускается.
- UI: `src/lib/update.svelte.ts` (состояние), компонент
  `src/lib/components/UpdateDialog.svelte` («Доступна новая версия»,
  «Обновиться» / «Позже», прогресс, текст релиза). Проверка запускается в
  `App.svelte` после `init()`; вручную — «Настройки → О программе →
  Обновления».
- Прогресс скачивания идёт событием `update-progress` (опрашиваем размер
  файла, который пишет `curl`).
- CI: `.github/workflows/release.yml` — по тегу `v*` собирает Windows
  (windows-latest, MSVC) и Linux (ubuntu-22.04), считает SHA-256 и публикует
  релиз с заметками из `docs/release-notes/<версия>.md`.
  `.github/workflows/check.yml` — `npm run check` и `cargo test --lib`.
- Выпуск: `scripts/release.sh 0.9.1` — поднимает версию в `package.json`,
  `Cargo.toml`, `tauri.conf.json`, коммитит, ставит тег и пушит.
- Версия 0.9.0 — бета; 1.0.0 планируется после проверки на реальной Windows
  и первого живого автообновления.

Живой релиз и тест (17.11.2026):
- Репозиторий: https://github.com/gera2347/gera-launcher (аккаунт был
  `dimagera05092013-cyber`, переименован; `REPO` в `update.rs` обновлён).
- Релиз: https://github.com/gera2347/gera-launcher/releases/tag/v0.9.0
  (Windows exe 4,7 МБ, Linux 9,4 МБ, оба с `.sha256`).
- GitHub Actions прошли: Checks и Release (windows-latest, ubuntu-22.04).
- Живой тест автообновления на Linux: установлен 0.9.0, создан временный релиз
  0.9.1, GERA показала окно, скачала файл, сверила SHA-256, заменила бинарник
  и перезапустилась. Временный релиз удалён.
- Версия 0.9.1 (фикс цвета при наведении): живое обновление 0.9.0 → 0.9.1
  на Linux прошло — окно, скачивание, SHA-256, замена файла, перезапуск;
  установленный бинарник совпал с ассетом CI по хешу.
- Windows-цикл замены exe на реальной Windows ещё не проверялся.

Ограничения:
- Публичный репозиторий: код виден всем (пользователь согласился).
- Без подписи кода SmartScreen всё равно предупреждает — автообновление это
  не лечит.
- Анонимный GitHub API даёт 60 запросов в час на IP; одной проверки при
  запуске хватает с запасом.
- Живой цикл «скачать → проверить → заменить → перезапустить» на Windows
  ещё не проверялся: нужен реальный релиз.

## 4. Ключевые файлы

- `src-tauri/src/sysutil.rs` — платформенные операции (архивы, диск, процессы).
- `src-tauri/src/clients.rs` — установка/импорт клиентов, выбор ассета по ОС.
- `src-tauri/src/horizon.rs` — обнаружение клиентов, запуск, процессы, логи.
- `src-tauri/src/native.rs` — сборка нативных модов, окружение, cargo.
- `src-tauri/src/mods.rs` — каталог, `.hmod`, встроенные моды, тест.
- `src-tauri/src/servers.rs` — серверы: адреса, картинки, NBT-импорт, SLP-пинг.
- `src-tauri/src/commands.rs` — Tauri-команды, запуск игры, импорт, open_path.
- `src-tauri/src/paths.rs` — пути данных Linux/Windows.
- `src-tauri/src/update.rs` — проверка релизов GitHub, SHA-256, замена exe.
- `src/lib/api.ts`, `src/lib/types.ts` — фронтенд-API и типы.
- `src/lib/update.svelte.ts` — состояние автообновления.
- `src/lib/components/UpdateDialog.svelte` — окно «Доступна новая версия».
- `src/pages/Home.svelte` — главная, счётчик модов.
- `src/pages/Servers.svelte` — раздел «Серверы» целиком.
- `src/pages/Mods.svelte` — каталог, «Встроенные моды», «Установлено».
- `src/pages/Settings.svelte` — обновления, подготовка, импорт, сборщик.
- `docs/GUIDE.md` — руководство пользователя.
- `README.md` — описание проекта.

## 5. Что НЕ проверено / ограничения

- **Windows-версия на реальной Windows не запускалась.** Проверено под Wine:
  тесты Windows-таргета, headless-импорт, создание окна (см. 3.2.1). GUI целиком
  под Wine не работает из-за отсутствия WebView2. Если пользователь сообщает об
  ошибке — проси текст/скрин.
- Старые GNU-сборки (включая `gera-launcher-windows-full.zip`) требуют
  `WebView2Loader.dll`. Новый MSVC-exe из 3.2.2 работает один, без DLL.
- Новый одиночный exe проверен под Wine только частично: CLI-импорт ресурсов и
  запуск GUI (до окна WebView2). На реальной Windows он ещё не запускался.
- Автообновление на Linux проверено живым релизом (см. 3.8). На Windows
  замена работающего exe не проверялась: нужна реальная Windows 10/11.
- Нативная сборка на Windows требует установленного Rust и сборочных
  зависимостей Horizon под Windows; остальные функции от них не зависят.
- Кастомный мод `hero_worldmap` (карта и метки) не восстановлен как бинарник:
  старый клиент с ним перезаписан, готовой копии нет. Исходники
  `~/.local/share/opencode-agent/hero-mods/hero-worldmap/` и данные карты
  `~/.local/share/horizon/config/hero_worldmap` целы, но мод привязан к другой
  ревизии Horizon. Нужна отдельная пересборка.
- `opencode stats` падает: `Database is not empty and has no session table`.
- Серверы: пинг не делает SRV-запрос (адрес вида `_minecraft._tcp` не
  разрешается) и не поддерживает legacy-серверы до 1.7. DNS-резолв может
  занять больше времени, чем таймаут соединения. Запуск идёт только через
  Horizon (offline-режим), вход в Java Edition не поддерживается.
- Перетаскивание карточек проверено логикой (unit-тест `server_reorder`) и
  разметкой; мышью в автоматическом прогоне не тянулось — на машине нет
  инструмента для синтетических кликов.

## 6. Идеи пользователя (пока НЕ делать)

- Автоматическая установка Rust при первой попытке собрать нативный мод
  (скачать rustup, поставить в папку GERA, на Windows — GNU-тулчейн + MinGW,
  убрать зависимость от `git` в `mixins_tool`). Пользователь сказал:
  «просто идея, пока делать не надо».

## 7. Хронология запросов (сжато)

1. «Восстанови моды, которые были при прямом запуске Horizon» → найдены
   официальные нативные моды в клиенте `nightly-2026-10-31-9d979ca`, профиль
   переключён, `.hmod` восстановлены из бэкапов.
2. «Сделай блок встроенных модов и счётчик» → добавлены `mods_builtin`,
   `BuiltinMod`, блок «Встроенные моды», главная считает 11 модов.
3. «Сделай порт на винду, потом проверю» → сделан порт (см. 3.2), собраны
   Linux и Windows бинарники, Linux проверен на запущенном лаунчере.
4. «Сколько токенов осталось?» → объяснено, что баланс не виден, модель
   бесплатная, `opencode stats` сломан.
5. «Как установить Rust на Windows?» → даны инструкции (rustup + MSVC Build
   Tools, либо GNU + MinGW).
6. «Сделай zip с лаунчером, .minecraft и инструкцией для Windows» → создан
   `gera-launcher-windows-full.zip` (609 МБ) с `import/26.3` и инструкцией.
7. «Через этот zip я на голом ПК смогу играть?» → да, нужны Windows 10/11 x64
   и интернет; клиент Horizon скачивается, официальный лаунчер не нужен.
8. «Открой папку с zip в проводнике» → открыто
   `~/gera-launcher/release/windows`.
9. «Можешь подключиться ко второму ПК и оптимизировать под Windows?» →
   подключиться нельзя; проверка сделана под Wine, найден и исправлен баг с
   `WebView2Loader.dll`, убраны консольные окна, добавлена тёмная рамка окна,
   обновлены оба zip. Windows на второй ПК пользователь пока ставить не готов.
10. «Сделай, чтобы игра быстрее заводилась (чёрный экран), и настройку, чтобы
    лаунчер закрывался при запуске игры» → добавлена настройка
    `closeOnLaunch` (окно скрывается и закрывается через 1,2 с). Замеры
    показали: долгий чёрный экран = холодный кэш ресурсов (21 с), тёплый —
    8–11 с в любом способе запуска (см. 3.5–3.6).
11. «Сделай раздел Серверы: свои серверы, импорт из Horizon, баннеры, привязка
    сборки, пинг и запуск в один клик» → сделано (см. 3.7).
12. «Сделай Windows-версию одним exe, без DLL» → собран MSVC-exe 4,7 МБ,
    `WebView2Loader.dll` больше не нужна (см. 3.2.2).
13. «Хочу автообновление по воздуху через GitHub, с окном „Доступна новая
    версия“» → открытый репозиторий, GitHub Actions, релиз 0.9.0 и апдейтер
    (см. 3.8). Аккаунт GitHub: `gera2347` (аккаунт переименован).
14. «При наведении мыши активная кнопка становится серой, а должна оставаться
    цветной» → причина: `.build-pill:hover:not(:disabled)` (три класса)
    перебивал `.build-pill.active` (два класса). Добавлено исключение
    `:not(.active)` и отдельный hover для активной карточки; в `IconButton`
    hover больше не затирает цвета `tonal`, `filled` и `active`.
    Выпущена версия 0.9.1.

15. «Срочно: пусть Minecraft Java тоже будет в лаунчере и можно играть без
    ебли» → добавлен свой загрузчик Mojang (`src-tauri/src/vanilla.rs`) и раздел
    **Java**: список версий, установка с прогрессом, один клик «Играть»,
    системная Java вместо скачивания. Причина краша 26.3 — нативные jar'ы
    LWJGL не попадали в classpath; теперь добавляются. Проверено: игра доходит
    до главного меню. CLI для проверки: `--java-install <версия>`,
    `--java-play <версия> <ник>`. Выпущена версия 0.10.0.

## 8. Как продолжать

- Если пользователь сообщает об ошибке на Windows — попроси точный текст/скрин,
  найди место в коде, исправь, пересобери Windows-версию и обнови
  `release/windows/gera-launcher.exe`, `WebView2Loader.dll` и оба zip.
- При сборке Windows exe **всегда** клади рядом `WebView2Loader.dll`
  (tauri-build копирует его в `target/x86_64-pc-windows-gnu/release/`).
- Перед пересборкой zip проверяй импорты:
  `x86_64-w64-mingw32-objdump -p gera-launcher.exe | grep "DLL Name"` — кроме
  системных DLL должен быть только `WebView2Loader.dll`.
- Не пересобирай full-zip без необходимости: 606 МБ, ресурсы не меняются.
  Обновлять его можно точечно: `7z u gera-launcher-windows-full.zip <файлы>`
  (занимает секунды, остальные записи не пережимаются).
- Не трогай профили, миры и настройки пользователя без просьбы.
- Для раздела «Серверы»: `cargo test --lib` (12 тестов) и `npm run check`.
  Сборка установщика: `npm run tauri build -- --no-bundle`, затем скопировать
  `src-tauri/target/release/gera-launcher` в `~/.local/bin/gera-launcher`.
- Никогда не записывай `servers.dat` Horizon: импорт только читает его.
- Не запускай игру из автоматических проверок на сервере с листочком
  (`203.0.113.10`) — пользователь об этом отдельно просил.
- Перед удалением/заменой важных файлов — резервная копия и подтверждение.
