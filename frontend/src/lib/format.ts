/* Server-name parsing utilities. */

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[i]}`;
}

export function formatExpire(unixSeconds: number | null | undefined): string {
  if (!unixSeconds) return "—";
  const d = new Date(unixSeconds * 1000);
  return d.toLocaleDateString(undefined, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  });
}

/** Map of name fragments → ISO 3166-1 alpha-2. Order matters: more specific
 * patterns first. Case- and language-insensitive matching. */
const COUNTRY_PATTERNS: Array<[RegExp, string]> = [
  // RU
  [/(росси[яийеёт]|russia|RUS\b|РФ)/i, "RU"],
  // CIS / Eastern Europe
  [/(украин|ukraine|укр)/i, "UA"],
  [/(беларус|belarus|byelo)/i, "BY"],
  [/(казахст|kazakh)/i, "KZ"],
  [/(узбекист|uzbek)/i, "UZ"],
  [/(армен|armenia)/i, "AM"],
  [/(азербайдж|azerbaijan)/i, "AZ"],
  [/(грузи[яи]|georgia)/i, "GE"],
  [/(молдов|moldova)/i, "MD"],
  // Europe
  [/(великобритани|англи|united kingdom|britain|england|UK\b|GB\b)/i, "GB"],
  [/(германи|germany|deutschland|GER\b|DEU\b)/i, "DE"],
  [/(нидерланд|netherlands|holland|dutch|NL\b)/i, "NL"],
  [/(польш|poland|polska|PL\b)/i, "PL"],
  [/(финлянд|finland|suomi|FI\b)/i, "FI"],
  [/(швеци|sweden|sverige|SE\b)/i, "SE"],
  [/(швейцари|switzerland|swiss|CH\b)/i, "CH"],
  [/(норвеги|norway|norge|NO\b)/i, "NO"],
  [/(дани[яи]|denmark|DK\b)/i, "DK"],
  [/(исланди|iceland|IS\b)/i, "IS"],
  [/(ирланди|ireland|IE\b)/i, "IE"],
  [/(литв|lithuania|lietuva|LT\b)/i, "LT"],
  [/(латви|latvia|latvija|LV\b)/i, "LV"],
  [/(эстони|estonia|EE\b)/i, "EE"],
  [/(чехи|czech|CZ\b)/i, "CZ"],
  [/(словаки|slovakia|SK\b)/i, "SK"],
  [/(словени|slovenia|SI\b)/i, "SI"],
  [/(венгри|hungary|HU\b)/i, "HU"],
  [/(румын|romania|RO\b)/i, "RO"],
  [/(болгари|bulgaria|BG\b)/i, "BG"],
  [/(серби|serbia|RS\b)/i, "RS"],
  [/(хорвати|croatia|HR\b)/i, "HR"],
  [/(автри|austria|österreich|AT\b)/i, "AT"],
  [/(бельги|belgium|belgique|BE\b)/i, "BE"],
  [/(франци|france|FR\b)/i, "FR"],
  [/(итал|italy|italia|IT\b)/i, "IT"],
  [/(испан|spain|españa|ES\b)/i, "ES"],
  [/(португал|portugal|PT\b)/i, "PT"],
  [/(грец|greece|GR\b)/i, "GR"],
  [/(турц|turkey|türkiye|TR\b)/i, "TR"],
  [/(кипр|cyprus|CY\b)/i, "CY"],
  [/(мальт|malta|MT\b)/i, "MT"],
  [/(люксембург|luxembourg|LU\b)/i, "LU"],
  [/(албани|albania|AL\b)/i, "AL"],
  // Middle East / Africa
  [/(дубай|оаэ|UAE|emirates|AE\b)/i, "AE"],
  [/(израиль|israel|IL\b)/i, "IL"],
  [/(саудов|saudi|SA\b)/i, "SA"],
  [/(катар|qatar|QA\b)/i, "QA"],
  [/(южная африк|south africa|ZA\b)/i, "ZA"],
  // Americas
  [/(США|usa|america|united states|U\.?S\.?\b)/i, "US"],
  [/(канад|canada|CA\b)/i, "CA"],
  [/(мексик|mexico|MX\b)/i, "MX"],
  [/(бразили|brazil|BR\b)/i, "BR"],
  [/(аргентин|argentina|AR\b)/i, "AR"],
  [/(чили|chile|CL\b)/i, "CL"],
  // Asia
  [/(япон|japan|JP\b)/i, "JP"],
  [/(южная корея|korea|KR\b)/i, "KR"],
  [/(китай|china|hong\s?kong|CN\b|HK\b)/i, "HK"],
  [/(тайвань|taiwan|TW\b)/i, "TW"],
  [/(сингапур|singapore|SG\b)/i, "SG"],
  [/(малайзи|malaysia|MY\b)/i, "MY"],
  [/(индия|india|IN\b)/i, "IN"],
  [/(индонез|indonesia|ID\b)/i, "ID"],
  [/(вьетнам|vietnam|VN\b)/i, "VN"],
  [/(таиланд|thailand|TH\b)/i, "TH"],
  // Oceania
  [/(австрали|australia|AU\b)/i, "AU"],
  [/(новая зеланди|new zealand|NZ\b)/i, "NZ"],
];

/** Strip the leading emoji-flag glyph(s) and decode them to ISO. */
function extractEmojiFlag(name: string): string | null {
  // Regional indicator: U+1F1E6..U+1F1FF (each = letter A..Z)
  const m = name.match(/[\u{1F1E6}-\u{1F1FF}]{2}/u);
  if (!m) return null;
  const cps = [...m[0]].map((c) => c.codePointAt(0)!);
  if (cps.length !== 2) return null;
  const A = 0x1f1e6;
  const a = cps[0]! - A;
  const b = cps[1]! - A;
  if (a < 0 || a > 25 || b < 0 || b > 25) return null;
  return (
    String.fromCharCode(65 + a) + String.fromCharCode(65 + b)
  );
}

export function inferCountryCode(
  name: string,
  fallback: string | null = null
): string | null {
  if (fallback && /^[a-zA-Z]{2}$/.test(fallback)) return fallback.toUpperCase();
  const fromEmoji = extractEmojiFlag(name);
  if (fromEmoji) return fromEmoji;
  for (const [rx, cc] of COUNTRY_PATTERNS) {
    if (rx.test(name)) return cc;
  }
  return null;
}

/** Display-friendly server name: strips leading emoji-flag glyphs and any
 * leading 2-letter ISO prefix that duplicates the column we already render. */
export function displayName(name: string): string {
  if (!name) return "";
  let s = name;
  // Strip leading emoji flag (regional indicator pair).
  s = s.replace(/^[\u{1F1E6}-\u{1F1FF}]{2}\s*/u, "");
  // Strip leading "RU " / "us " / "DE | " etc.
  s = s.replace(/^([a-zA-Z]{2})\s*[|·\-—–]?\s*/, (full, code) => {
    // Only strip if the code is a known country. Otherwise keep — could be a
    // legitimate prefix like "LT" meaning "Long-term" rather than Lithuania.
    return COUNTRY_PATTERNS.some(([, cc]) => cc === code.toUpperCase()) ? "" : full;
  });
  return s.trim();
}
