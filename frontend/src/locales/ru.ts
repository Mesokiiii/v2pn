/* Russian — base locale.
 *
 * Convention: keys are dot-separated namespaces. Use {placeholders} for
 * runtime values. Other locales must mirror exactly the same key set —
 * `lib/i18n.ts` types them off of this dictionary at build time.
 *
 * Technical terms (VLESS, REALITY, PROXY, TUN, TCP, …) are intentionally
 * NOT translated — they're protocol identifiers, not English copy.
 */

export const ru = {
  app: {
    name: "v2pn",
    tagline: "Прокси-клиент",
    version: "v0.1.0",
  },

  nav: {
    workspace: "Рабочая зона",
    servers: "Серверы",
    routing: "Маршрутизация",
    logs: "Логи",
    settings: "Настройки",
    subscriptions: "Подписки",
    addSubscription: "Добавить подписку",
  },

  connection: {
    connect: "Подключиться",
    disconnect: "Отключиться",
    cancel: "Отменить",
    connected: "Подключено",
    disconnected: "Отключено",
    connecting: "Подключение…",
    stopping: "Отключение…",
    failed: "Ошибка",
    noServerSelected: "Сервер не выбран",
    via: "через",
  },

  subscription: {
    title: "Подписка",
    servers: "{count} серверов",
    autoUpdate: "автообновление {hours}ч",
    refreshing: "обновляется…",
    expires: "истекает {date}",
    refresh: "Обновить",
    refreshDisabled: "Подписки из текста нельзя обновить — используйте «Импорт»",
    new: "Новая",
    ping: "Пинг",
    pingHint: "Проверить задержку до всех серверов",
    refreshTipBody:
      "Скачивает свежий список серверов с того же URL подписки. Сохраняет ваши локально-добавленные серверы.",
    newTipBody:
      "Добавить ещё одну подписку или вставить отдельную vless:// ссылку.",
    pingTipBody:
      "Меряет TCP до серверного :443. Не показывает работает ли REALITY-туннель внутри — для этого нужен подключённый сервер.",

    modeProxyTip: "Режим PROXY",
    modeProxyTipBody:
      "v2pn выставляет HTTP/SOCKS-прокси в настройках Windows. Прокси подхватят браузеры (Chrome, Edge, Firefox, Yandex), Discord, Slack, Steam. Игры и редкие приложения, которые игнорируют системный прокси, пройдут мимо.",
    modeTunTip: "Режим TUN (полный системный)",
    modeTunTipBody:
      "Создаёт виртуальный сетевой адаптер. Через него идёт весь трафик ОС, без исключений. Перехватывает игры, торренты, мессенджеры, всё что обращается к интернету. Требует прав администратора.",
    modeTunNeedsAdmin:
      "Сейчас v2pn запущен без прав администратора — переключение в TUN недоступно.",
    modeLockedTip: "Режим залочен",
    modeLockedTipBody:
      "Нельзя менять PROXY/TUN пока активно соединение. Сначала отключитесь.",

    usageTip: "Трафик подписки",
    usageTipBody:
      "Сколько вы уже использовали из квоты подписки. Цвет шкалы становится жёлтым на 85% и красным на 100%. Лимит и срок действия задаёт ваш провайдер.",
  },

  servers: {
    list: "Серверы",
    rtt: "RTT",
    selected: "Выбранный сервер",
    offline: "недоступен",
    notMeasured: "—",
  },

  detail: {
    protocol: "Протокол",
    server: "Сервер",
    port: "Порт",
    transport: "Транспорт",
    security: "Безопасность",
    sni: "SNI",
    utls: "uTLS",
  },

  importDialog: {
    title: "Импорт подписки",
    subtitle:
      "Вставьте URL подписки или одиночную ссылку. Формат определяется автоматически.",
    tabUrl: "URL",
    tabText: "Текст",
    urlLabel: "URL подписки",
    urlPlaceholder: "https://example.com/sub/UUID",
    urlHint:
      "Поддерживаются любые панели — Marzban, Marzneshin, Remnawave, 3X-UI, x-ui, sing-box.",
    textLabel: "Конфигурация",
    textPlaceholder:
      "vless://abc@host:443?...\nvmess://...\ntrojan://...\n\n— или —\n\nbase64-блок, sing-box JSON, Clash YAML",
    textHint:
      "Можно несколько ссылок (по одной на строку). Комментарии после {hash} сохраняются как имена серверов.",
    pasteFromClipboard: "Из буфера",
    cancel: "Отмена",
    import: "Импортировать",
    submitHotkey: "⌘↵",
  },

  empty: {
    title: "Пока нет подписок",
    description:
      "Вставьте URL подписки или одну ссылку {kbd}, чтобы начать. v2pn сам не предоставляет серверы — используйте свои.",
    cta: "Импортировать подписку",
    hotkey: "⌘N",
  },

  webappFallback: {
    title: "Эта подписка работает только через веб",
    subtitle:
      "Панель провайдера вернула HTML-страницу установщика вместо обычной подписки. Мы попробовали все стандартные варианты — ни один не сработал. Есть два пути.",
    optionATitle: "Вариант A — открыть в браузере и скопировать настоящую ссылку",
    optionAStep1: "Откройте панель в вашем стандартном браузере.",
    optionAStep2:
      "Правый клик по кнопке {connect} → «Копировать ссылку». Или DevTools {f12} → Network → найдите запрос, который возвращает {vless} / base64 / yaml.",
    optionAStep3: "Вставьте этот реальный URL через диалог импорта.",
    openInBrowser: "Открыть {host} в браузере",
    optionBTitle: "Вариант B — вставить одну ссылку",
    optionBSubtitle:
      "Уже есть {vless} / trojan / hy2 / tuic из другого клиента (Happ, v2rayN, sing-box)? Вставьте сюда.",
    sourcePrefix: "источник:",
  },

  settings: {
    title: "Настройки",
    subtitle: "Настройки сохраняются на время сессии.",

    sectionMode: "Режим подключения",
    sectionModeHint: "Как v2pn перехватывает трафик.",
    modeProxy: "Системный прокси",
    modeProxyHint:
      "Устанавливает HTTP/SOCKS прокси в Windows. Поддерживается большинством браузеров и современных приложений.",
    modeTun: "TUN (полный системный)",
    modeTunHint:
      "Виртуальный сетевой адаптер уровня L3 через Wintun. Перехватывает весь трафик. Требует прав администратора.",

    sectionPorts: "Сетевые порты",
    sectionPortsHint: "Только loopback. Изменение требует переподключения.",
    portMixed: "SOCKS + HTTP",
    portClashApi: "Clash API",
    portTun: "TUN-интерфейс",

    sectionProtocol: "Протокол",
    sectionProtocolHint: "Защита от утечек DNS и тип адресов.",
    protoIpv6: "IPv6 в туннеле",
    protoStrictDns: "Строгий DNS",
    enabled: "включено",
    disabled: "выключено",
    strictDnsOn: "все запросы через прокси",
    strictDnsOff: "разделено",

    sectionLanguage: "Язык интерфейса",
    sectionLanguageHint: "Можно сменить в любой момент.",

    sectionAbout: "О приложении",
    aboutVersion: "Версия v2pn",
    aboutSingbox: "sing-box",
    aboutWintun: "Wintun",
    openLogs: "Открыть папку логов",
    copyDiagnostics: "Скопировать диагностику",
  },

  logs: {
    title: "Логи",
    autoscroll: "автопрокрутка",
    clear: "очистить",
    waiting: "ожидание вывода sing-box…",
  },

  bar: {
    error: "ошибка",
  },

  themes: {
    light: "Светлая",
    dark: "Тёмная",
    toLight: "Переключить на светлую",
    toDark: "Переключить на тёмную",
    hotkey: "Ctrl+Shift+L",
  },

  comingSoon: "В разработке",

  admin: {
    required: "Требуются права администратора",
    tunNeedsAdmin:
      "Режим TUN использует виртуальный сетевой адаптер Wintun, который Windows позволяет настраивать только администратору. Перезапустите v2pn от имени администратора, чтобы включить TUN.",
    restart: "Перезапустить от имени администратора",
    notNow: "Не сейчас",
    badge: "admin",
  },
} as const;

/* The locale shape used by all translations. We loosen `as const` literal
 * types back to `string`, so other locales can supply different texts while
 * the structure is still verified. */
type Loosen<T> = T extends string
  ? string
  : T extends ReadonlyArray<infer U>
  ? Loosen<U>[]
  : T extends object
  ? { [K in keyof T]: Loosen<T[K]> }
  : T;

export type Locale = Loosen<typeof ru>;
