# GERA Launcher

Material You лаунчер для [Horizon](https://github.com/horizon-rust/horizon) —
Minecraft: Java Edition 26.3 на Rust. Самостоятельный, неофициальный, бесплатный.

GERA не зависит от официального Horizon Launcher. У Horizon берутся только клиент и
каталог модов; установку, версии, ресурсы, запуск, сборки и моды GERA делает сама.

**Руководство пользователя:** [docs/GUIDE.md](docs/GUIDE.md) — установка, сборки,
моды, нативная сборка, разбор ошибок.

## Что работает

- Установка официальных сборок `horizon-client` с GitHub: SHA-256, распаковка в свою папку.
- Обнаружение клиентов GERA, версий на диске и своей папки.
- Подготовка ресурсов через штатный `horizon-prepare` из вашей копии Minecraft.
- Импорт готовой папки ресурсов с другого ПК — без повторной подготовки.
- Прямой запуск `horizon-client` с игровой папкой, ником и скином сборки.
- Сборки: отдельные игровые папки, ник, версия Horizon, скин, описание и дублирование.
- Каталог Horizon Mods Registry: поиск, категории, описания, версии, картинки.
- Установка `.hmod` в выбранную сборку с проверкой SHA-256, обновление,
  удаление с резервной копией.
- Свой сборщик нативных `.hnmod`: исходники Horizon и horizon-mixins по привязанным
  коммитам, `horizon-mixins apply`, `cargo build` и отдельный клиент для сборки.
- Material 3 / Material You: Dynamic Color из одного акцента, светлая, тёмная
  и системная темы, восемь готовых палитр и свой цвет; градиенты можно
  отключить для плоских заливок.
- Лёгкий и тяжёлый режимы интерфейса (на игру не влияют): Light — минимум
  анимаций и эффектов, Heavy — полные анимации и размытия.
- Главная: приветствие, активная сборка, большая кнопка «Играть», версия
  Horizon, число модов и обновления модов.
- Диагностика — только по кнопке, без фоновых опросов.

## Чего пока нет

- Microsoft-вход: нужен Azure client ID и OAuth, которых у проекта нет. Сейчас — офлайн-ник.
- Нативные моды с trait-API Horizon (`[api]`): GERA собирает только mixin-моды.
  Остальные нативные моды собираются, но каждая сборка — отдельный клиент и время.

## Сборка

Для нативных модов дополнительно нужны Rust и platform-зависимости Horizon
(`vulkan-headers`, `alsa-lib`, `libxkbcommon`, `wayland`, `libx11`). Сборка идёт на
компьютере игрока и требует 12–20 ГБ свободного места.

Linux: нужны Arch Linux (или производный), Rust, Node.js и WebKitGTK:

```sh
sudo pacman -S --needed rust nodejs npm webkit2gtk-4.1
npm install
npm run tauri dev
```

Windows: нужны Rust (MSVC) и Node.js. Собранный `gera-launcher.exe` — один
самодостаточный файл, `WebView2Loader.dll` рядом не нужна. Данные хранятся в
`%LOCALAPPDATA%\gera-launcher\`.

Релизная сборка:

```sh
npm run tauri build -- --no-bundle
```

Готовый бинарник: `src-tauri/target/release/gera-launcher`
(на Windows — `gera-launcher.exe`).

## Скачать и обновления

Готовые сборки публикуются в
[GitHub Releases](https://github.com/dimagera05092013-cyber/gera-launcher/releases/latest):
`gera-launcher.exe` для Windows 10/11 x64 и `gera-launcher-linux` для Linux x64.

Лаунчер сам проверяет новые релизы при запуске: если версия новее, появляется
окно «Доступна новая версия» с кнопками «Обновиться» и «Позже». Обновление
скачивается, сверяется по SHA-256 и заменяет файл при перезапуске.

Выпуск новой версии:

```sh
scripts/release.sh 0.9.1
```

Скрипт поднимает номер, коммитит, ставит тег и отправляет его на GitHub.
GitHub Actions собирает Windows и Linux сборки и публикует релиз.

## Данные

Состояние лаунчера — в `~/.local/share/gera-launcher/`. Клиенты — в `clients/`, копии
модов — в `backups/mods/`. Игровые папки Horizon не перемещаются.

Подробно: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

Horizon не связан с Mojang Studios или Microsoft. Minecraft — торговая марка
Mojang Synergies AB.

## Лицензия

Код GERA Launcher распространяется по [MIT License](LICENSE). Клиент Horizon и
моды — собственность их авторов и распространяются по своим лицензиям.